//! Multiplayer voxel demo. Same world / mining mechanics as
//! `voxel_demo` plus a server / client mode on top of
//! `infengine_net`. Run modes:
//!
//! ```sh
//! cargo run -p voxel_demo_mp                            # offline single-player
//! cargo run -p voxel_demo_mp -- --server 0.0.0.0:7777   # host
//! cargo run -p voxel_demo_mp -- --client 127.0.0.1:7777 # join
//! ```
//!
//! Wire format: `infengine_net::NetMessage::Bytes { kind, data }`
//! with two kinds:
//! - `"vx.edit"` carries `EditMsg { x, y, z, block }` for a single
//!   block-edit (server -> all clients on apply; client -> server
//!   as a request).
//! - `"vx.player"` carries `PlayerMsg { id, pos, yaw, pitch }` for
//!   pose broadcasts so each client can render others as cubes.

use std::collections::{HashMap, HashSet, VecDeque};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use glam::{Mat4, Vec3};
use infengine_editor::EditorUi;
use infengine_net::{NetClient, NetServer};
use infengine_render::{MeshHandle, MeshVertex, Renderer};
use infengine_voxel::{
    BlockDef, BlockId, BlockRegistry, CHUNK_SIZE, Chunk, ChunkCoord, ChunkSubmesh, RayHit, World,
    chunk_mesh_with_atlas, raycast,
};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

/// One block-edit travelling over the wire. Sent as
/// `kind = "vx.edit"`. Server applies + broadcasts to every client
/// including the originator; clients apply on receive (don't predict
/// locally for v1).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct EditMsg {
    x: i32,
    y: i32,
    z: i32,
    block: u16,
}

/// Player pose snapshot. Sent every tick:
/// - client -> server as a single-player update (`id` ignored).
/// - server -> all clients as a per-player broadcast (with the
///   originator's `id`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PlayerMsg {
    id: u32,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
}

/// Server -> all clients: a player has left. Recipients drop the
/// id from their `remote` table so the leaver's cube + pose stop
/// rendering immediately instead of waiting for an eventual
/// timeout.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PlayerRemoveMsg {
    id: u32,
}

// `vx.leave` is sent as an empty-payload `client.send_bytes`
// (no struct needed - presence of the kind string is the signal).

/// Full per-client inventory snapshot. Server -> client only. The
/// server is authoritative; every break / place / drag-and-drop /
/// craft action on the server resolves to a fresh snapshot, sent
/// back to the originating client via `vx.inv`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InvSnapshotMsg {
    /// Main 36-slot inventory grid (Vec to keep bincode varlen-
    /// friendly; we always send exactly INVENTORY_SLOTS entries).
    inventory: Vec<Option<(u16, u8)>>,
    /// 2x2 crafting grid (always 4 entries).
    craft: Vec<Option<(u16, u8)>>,
}

/// Encoded `SlotKind`. Bincode-friendly + version-stable so we
/// don't accidentally break the wire format by renaming a variant.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum NetSlot {
    Inv(u32),
    Craft(u32),
    CraftOut,
}

impl From<SlotKind> for NetSlot {
    fn from(k: SlotKind) -> Self {
        match k {
            SlotKind::Inv(i) => NetSlot::Inv(i as u32),
            SlotKind::Craft(i) => NetSlot::Craft(i as u32),
            SlotKind::CraftOut => NetSlot::CraftOut,
        }
    }
}

impl From<NetSlot> for SlotKind {
    fn from(k: NetSlot) -> Self {
        match k {
            NetSlot::Inv(i) => SlotKind::Inv(i as usize),
            NetSlot::Craft(i) => SlotKind::Craft(i as usize),
            NetSlot::CraftOut => SlotKind::CraftOut,
        }
    }
}

/// Snapshot of one server-side mob. Sent as `vx.mob`; clients
/// render the latest reported pose. `kind` reserves a discriminator
/// for future mob types - 0 = sheep for now.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct MobMsg {
    id: u32,
    kind: u8,
    pos: [f32; 3],
    yaw: f32,
}

/// Server-side mob state. Not serialised; only `MobMsg` goes over
/// the wire. The server picks a random `target` for the mob to
/// wander toward + retargets when it gets there or 5 s pass.
#[derive(Debug, Clone, Copy)]
struct MobState {
    id: u32,
    kind: u8,
    pos: [f32; 3],
    target: [f32; 3],
    yaw: f32,
    /// Time-of-day clock value at the next forced retarget. Set
    /// to `day_time + RETARGET_AFTER` whenever the mob picks a new
    /// goal; the wander loop re-picks once we cross this.
    retarget_at: f32,
    /// Current hit points. Reaches zero = mob despawns + drops
    /// loot. Reset on spawn to `MOB_MAX_HEALTH`.
    health: f32,
}

const MOB_CAP: usize = 12;
const MOB_SPEED: f32 = 1.2;
const MOB_MAX_HEALTH: f32 = 8.0;
const MOB_HIT_DAMAGE: f32 = 2.0;
/// Maximum range (in blocks) for the player's melee attack on a
/// mob. Same number as voxel break reach so the two interactions
/// feel symmetric.
const MOB_ATTACK_REACH: f32 = 5.5;

/// Server -> all clients: a damage / death event for one mob.
/// Lets every client play the same hurt flash + sound regardless
/// of whether they were the attacker.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct MobEventMsg {
    id: u32,
    /// 0 = hurt, 1 = death.
    kind: u8,
}

/// Client -> server: request to damage a mob. Server validates
/// reach against the sender's last reported pose, applies damage,
/// broadcasts the resulting `MobEventMsg` + an inventory snapshot
/// to the attacker when the mob dies + drops loot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct MobHitMsg {
    mob_id: u32,
}

/// Client -> server inventory intent: a drag-and-drop completion.
/// Server validates + applies + sends back an `InvSnapshotMsg`.
/// `split` mirrors the local Drag action and tells the server how
/// many items to move (all / half / one).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct InvActionMsg {
    from: NetSlot,
    to: NetSlot,
    #[serde(default)]
    split: SplitMode,
}

/// Versioned wrapper around the server's persisted state. Stored
/// at `voxel_server_state.bin` prefixed with the `INFV` magic so
/// we can evolve the format without breaking older saves.
const SERVER_STATE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServerStateV2 {
    version: u32,
    edits: Vec<EditMsg>,
    /// `(client_name, inventory)` tuples. Keyed by name so a
    /// reconnect re-attaches the right state even though UDP
    /// client ids reset every run.
    inventories: Vec<(String, PersistedInv)>,
    /// Current day-of-cycle clock (seconds, wraps at
    /// `DAY_LENGTH_S`). Persisted so a host restart doesn't reset
    /// the world to mid-morning.
    day_time: f32,
}

/// On-disk per-client inventory snapshot. `(block_id, count)`
/// tuples mirror the in-memory `[Option<(BlockId, u8)>; N]`
/// layout but use `u16` for the block so the format doesn't break
/// if `BlockId` ever grows fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedInv {
    inventory: Vec<Option<(u16, u8)>>,
    craft: Vec<Option<(u16, u8)>>,
}

impl PersistedInv {
    fn from_client_inv(inv: &ClientInv) -> Self {
        Self {
            inventory: inv
                .inventory
                .iter()
                .map(|s| s.map(|(b, c)| (b.0, c)))
                .collect(),
            craft: inv
                .craft_grid
                .iter()
                .map(|s| s.map(|(b, c)| (b.0, c)))
                .collect(),
        }
    }
    fn into_client_inv(self) -> ClientInv {
        let mut inventory: [Option<(BlockId, u8)>; INVENTORY_SLOTS] = [None; INVENTORY_SLOTS];
        let mut craft_grid: [Option<(BlockId, u8)>; 4] = [None; 4];
        for (i, s) in self.inventory.into_iter().enumerate() {
            if i < inventory.len() {
                inventory[i] = s.map(|(b, c)| (BlockId(b), c));
            }
        }
        for (i, s) in self.craft.into_iter().enumerate() {
            if i < craft_grid.len() {
                craft_grid[i] = s.map(|(b, c)| (BlockId(b), c));
            }
        }
        ClientInv {
            inventory,
            craft_grid,
        }
    }
}

/// Active networking role of this binary. Owns the endpoint state +
/// per-tick broadcasting / receiving cadence.
enum NetRole {
    Offline,
    Server {
        server: NetServer,
        /// Latest pose every connected client has reported, plus the
        /// host's own under id 0. Broadcast every tick.
        poses: HashMap<u32, PlayerMsg>,
        /// Per-client authoritative inventory + craft grid. The host
        /// uses `self.inventory` directly (id 0 is implicit and not
        /// stored here). New entries are seeded with the same
        /// starter stacks the host gets locally.
        inventories: HashMap<u32, ClientInv>,
        /// Per-client queue of pending `InvSnapshotMsg` sends. Built
        /// up while resolving messages, drained at the end of the
        /// net tick.
        pending_inv_sends: HashSet<u32>,
    },
    Client {
        client: NetClient,
        /// Latest snapshot of remote players (id != my own).
        remote: HashMap<u32, PlayerMsg>,
        /// Queue of block edits the client wants applied. Drained
        /// each tick and sent to the server as `vx.edit` payloads.
        pending_edits: Vec<EditMsg>,
        /// Outbound drag-and-drop intents waiting for an ack
        /// (via `vx.inv` snapshot). Each is retried every tick
        /// until the server's reply lands.
        pending_inv_actions: Vec<InvActionMsg>,
    },
}

/// Server-side per-client inventory state.
#[derive(Debug, Clone)]
struct ClientInv {
    inventory: [Option<(BlockId, u8)>; INVENTORY_SLOTS],
    craft_grid: [Option<(BlockId, u8)>; 4],
}

impl ClientInv {
    /// Fresh inventory matching the host's starting layout: hotbar
    /// slots 1..=BLOCK_TYPES.len() get a full stack each.
    fn starter() -> Self {
        let mut inv: [Option<(BlockId, u8)>; INVENTORY_SLOTS] = [None; INVENTORY_SLOTS];
        for (i, bt) in BLOCK_TYPES.iter().enumerate() {
            let slot = i + 1;
            if slot < HOTBAR_SLOTS {
                inv[slot] = Some((BlockId(bt.id), STACK_MAX));
            }
        }
        Self {
            inventory: inv,
            craft_grid: [None; 4],
        }
    }

    /// Encode as a wire snapshot for the `vx.inv` message.
    fn to_snapshot(&self) -> InvSnapshotMsg {
        InvSnapshotMsg {
            inventory: self
                .inventory
                .iter()
                .map(|s| s.map(|(b, c)| (b.0, c)))
                .collect(),
            craft: self
                .craft_grid
                .iter()
                .map(|s| s.map(|(b, c)| (b.0, c)))
                .collect(),
        }
    }
}

/// Pop one of `block` into the inventory. First tries to top up an
/// existing non-full stack, then claims the first empty slot.
/// Returns `true` when the block was successfully stored (false
/// means the inventory was completely full + the drop was lost).
fn inv_pickup(inv: &mut [Option<(BlockId, u8)>; INVENTORY_SLOTS], block: BlockId) -> bool {
    if block == BlockId::AIR {
        return false;
    }
    for slot in inv.iter_mut() {
        if let Some((b, count)) = slot.as_mut()
            && *b == block
            && *count < STACK_MAX
        {
            *count += 1;
            return true;
        }
    }
    for slot in inv.iter_mut() {
        if slot.is_none() {
            *slot = Some((block, 1));
            return true;
        }
    }
    false
}

/// Decrement one item of `block` somewhere in the inventory.
/// Returns `true` if a stack matched + was decremented (slot is
/// cleared when the count hits zero); `false` means the inventory
/// didn't have the block.
fn inv_consume(inv: &mut [Option<(BlockId, u8)>; INVENTORY_SLOTS], block: BlockId) -> bool {
    for slot in inv.iter_mut() {
        if let Some((b, count)) = slot.as_mut()
            && *b == block
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                *slot = None;
            }
            return true;
        }
    }
    false
}

/// Apply a drag&drop on a `ClientInv`. Mirrors the local
/// `apply_inv_action` logic so the server's authoritative view
/// stays in sync with what the client UI predicts.
fn inv_apply_drag(inv: &mut ClientInv, from: SlotKind, to: SlotKind, split: SplitMode) {
    if from == to {
        return;
    }
    // Can't drop INTO the crafting output.
    if matches!(to, SlotKind::CraftOut) {
        return;
    }
    // Protect the break-tool slot.
    if matches!(to, SlotKind::Inv(0)) {
        return;
    }
    let from_stack = match from {
        SlotKind::Inv(i) => inv.inventory.get(i).copied().flatten(),
        SlotKind::Craft(i) => inv.craft_grid.get(i).copied().flatten(),
        SlotKind::CraftOut => match_recipe(&inv.craft_grid),
    };
    let Some(from_stack) = from_stack else {
        return;
    };
    let to_stack: Option<(BlockId, u8)> = match to {
        SlotKind::Inv(i) => inv.inventory.get(i).copied().flatten(),
        SlotKind::Craft(i) => inv.craft_grid.get(i).copied().flatten(),
        SlotKind::CraftOut => unreachable!(),
    };
    let write_to = |inv: &mut ClientInv, slot: SlotKind, val: Option<(BlockId, u8)>| match slot {
        SlotKind::Inv(i) => {
            if i < inv.inventory.len() {
                inv.inventory[i] = val;
            }
        }
        SlotKind::Craft(i) => {
            if i < inv.craft_grid.len() {
                inv.craft_grid[i] = val;
            }
        }
        SlotKind::CraftOut => {}
    };
    if matches!(from, SlotKind::CraftOut) {
        match to_stack {
            None => {
                for cell in inv.craft_grid.iter_mut() {
                    if let Some((_, c)) = cell.as_mut() {
                        *c = c.saturating_sub(1);
                        if *c == 0 {
                            *cell = None;
                        }
                    }
                }
                write_to(inv, to, Some(from_stack));
            }
            Some((tb, tc))
                if tb == from_stack.0
                    && (tc as u16 + from_stack.1 as u16) <= STACK_MAX as u16 =>
            {
                for cell in inv.craft_grid.iter_mut() {
                    if let Some((_, c)) = cell.as_mut() {
                        *c = c.saturating_sub(1);
                        if *c == 0 {
                            *cell = None;
                        }
                    }
                }
                write_to(inv, to, Some((tb, tc + from_stack.1)));
            }
            _ => {}
        }
        return;
    }
    let take = match split {
        SplitMode::All => 0u8,
        SplitMode::Half => (from_stack.1 as u16).div_ceil(2) as u8,
        SplitMode::One => 1u8,
    };
    if take > 0 {
        let take = take.max(1).min(from_stack.1);
        let rem = from_stack.1 - take;
        match to_stack {
            Some((tb, tc)) if tb == from_stack.0 => {
                let merge = (STACK_MAX - tc).min(take);
                let new_to = tc + merge;
                let leftover = take - merge;
                write_to(inv, to, Some((tb, new_to)));
                let back = rem + leftover;
                write_to(
                    inv,
                    from,
                    if back == 0 {
                        None
                    } else {
                        Some((from_stack.0, back))
                    },
                );
            }
            None => {
                write_to(inv, to, Some((from_stack.0, take)));
                write_to(
                    inv,
                    from,
                    if rem == 0 {
                        None
                    } else {
                        Some((from_stack.0, rem))
                    },
                );
            }
            Some(_) => {}
        }
        return;
    }
    match to_stack {
        Some((tb, tc)) if tb == from_stack.0 => {
            let take = (STACK_MAX - tc).min(from_stack.1);
            let new_to = tc + take;
            let rem = from_stack.1 - take;
            write_to(inv, to, Some((tb, new_to)));
            write_to(
                inv,
                from,
                if rem == 0 {
                    None
                } else {
                    Some((from_stack.0, rem))
                },
            );
        }
        _ => {
            write_to(inv, to, Some(from_stack));
            write_to(inv, from, to_stack);
        }
    }
}

impl NetRole {
    fn label(&self) -> &'static str {
        match self {
            NetRole::Offline => "offline",
            NetRole::Server { .. } => "server",
            NetRole::Client { .. } => "client",
        }
    }
}

/// Initial chunk grid loaded eagerly before the player drops in. The
/// world streams more chunks in as the player walks beyond this
/// footprint.
const INITIAL_CHUNKS: i32 = 3;
/// Number of stacked chunk layers along the Y axis. Each chunk is
/// `CHUNK_SIZE` (=16) tall, so 4 layers give us 64 vertical voxels
/// - enough for mountains, sky-builds, and underground digging.
const VERTICAL_CHUNKS: i32 = 4;
/// Chunks within this Chebyshev distance of the player chunk are
/// kept loaded; new ones get queued for generation.
const VIEW_DISTANCE_CHUNKS: i32 = 4;
/// Chunks beyond this distance are unloaded (slightly larger than
/// `VIEW_DISTANCE_CHUNKS` so the player jiggling on the boundary
/// doesn't churn chunks every step).
const UNLOAD_DISTANCE_CHUNKS: i32 = 6;
/// Maximum number of worker results consumed per frame (each runs
/// `Renderer::create_mesh` for several submeshes - bounded so the
/// main thread doesn't stall on a sudden burst).
const RESULTS_PER_FRAME: usize = 4;
/// Number of background worker threads generating + meshing chunks.
const NUM_WORKERS: usize = 2;
const PLAYER_SPEED: f32 = 6.0;
/// Initial upward velocity on jump. Peak height = `v^2 / (2 * |g|)`,
/// so 8.0 ^ 2 / 36 ≈ 1.78 voxels - enough to clear a one-block step
/// with margin, similar to Minecraft's 1.25-block jump.
const JUMP_VELOCITY: f32 = 8.0;
const GRAVITY: f32 = -18.0;
const PLAYER_HEIGHT: f32 = 1.7;
const PLAYER_RADIUS: f32 = 0.3;
const COLLISION_EPSILON: f32 = 1e-4;
/// Hotbar slots: index 0 is the "break" slot (empty hand), the rest
/// hold the block ids in `Self::HOTBAR_BLOCKS`.
/// Hotbar occupies the first row of the inventory grid; the
/// remaining `INVENTORY_ROWS - 1` rows are the open-inventory
/// extension Minecraft players expect.
const HOTBAR_SLOTS: usize = 9;
const INVENTORY_COLS: usize = 9;
const INVENTORY_ROWS: usize = 4;
const INVENTORY_SLOTS: usize = INVENTORY_COLS * INVENTORY_ROWS;
const STACK_MAX: u8 = 64;

/// Length of one full day/night cycle in seconds. Short enough to
/// see the sun move during a single play session - feel free to
/// crank up for atmosphere later. 240s = 4 min full cycle.
const DAY_LENGTH_S: f32 = 240.0;

/// Shapeless 2x2 crafting recipe. Each entry is `(BlockId, count)`
/// the player must place into the 2x2 grid (order doesn't matter).
/// Empty inputs are encoded by leaving them out of the list.
struct Recipe {
    inputs: &'static [(u16, u8)],
    output: (u16, u8),
    /// Human-readable name shown in the recipe book.
    name: &'static str,
}

/// Demo recipe table. Real Minecraft has shaped + tagged recipes;
/// this is a deliberately tiny showcase for the engine. New entries
/// = one line here, no other changes needed.
const RECIPES: &[Recipe] = &[
    // 4 dirt -> 1 grass (sodding).
    Recipe {
        inputs: &[(2, 1), (2, 1), (2, 1), (2, 1)],
        output: (3, 1),
        name: "Sodded grass",
    },
    // 4 leaves -> 1 wood (the tree's revenge).
    Recipe {
        inputs: &[(6, 1), (6, 1), (6, 1), (6, 1)],
        output: (5, 1),
        name: "Bundled log",
    },
    // 1 wood -> 4 planks (classic Minecraft).
    Recipe {
        inputs: &[(5, 1)],
        output: (7, 4),
        name: "Oak planks",
    },
    // 4 planks -> 1 wood (consolidate; lets you reverse the craft
    // chain).
    Recipe {
        inputs: &[(7, 1), (7, 1), (7, 1), (7, 1)],
        output: (5, 1),
        name: "Consolidated log",
    },
    // 4 stone -> 1 cobble (compressed cobblestone block).
    Recipe {
        inputs: &[(1, 1), (1, 1), (1, 1), (1, 1)],
        output: (8, 1),
        name: "Cobblestone",
    },
    // 4 cobble -> 4 stone (polish cobbles back into stone).
    Recipe {
        inputs: &[(8, 1), (8, 1), (8, 1), (8, 1)],
        output: (1, 4),
        name: "Polished stone",
    },
    // 4 sand -> 1 glass (we have no furnace yet, so the craft does
    // double duty as the heat source).
    Recipe {
        inputs: &[(4, 1), (4, 1), (4, 1), (4, 1)],
        output: (9, 1),
        name: "Glass block",
    },
    // 2 wood + 2 leaves -> 4 torches. Two-block input
    // disambiguates this from the planks recipe (1 wood alone).
    Recipe {
        inputs: &[(5, 1), (5, 1), (6, 1), (6, 1)],
        output: (10, 4),
        name: "Torches",
    },
];

/// Inspect the 2x2 crafting grid and return the recipe output if
/// every input is satisfied. The grid is shapeless: order doesn't
/// matter. Returns `None` when no recipe matches or any required
/// stack is short.
fn match_recipe(grid: &[Option<(BlockId, u8)>; 4]) -> Option<(BlockId, u8)> {
    match_recipe_named(grid).map(|(b, c, _)| (b, c))
}

/// Same as `match_recipe` but also returns the matching recipe's
/// human-readable name. Used by the UI to display the result name
/// as a tooltip on the output slot.
fn match_recipe_named(
    grid: &[Option<(BlockId, u8)>; 4],
) -> Option<(BlockId, u8, &'static str)> {
    // Aggregate the grid into a (block, count) tally so order
    // doesn't matter for shapeless matching.
    let mut tally: Vec<(u16, u8)> = Vec::with_capacity(4);
    for (b, c) in grid.iter().flatten() {
        if let Some(slot) = tally.iter_mut().find(|(bb, _)| *bb == b.0) {
            slot.1 = slot.1.saturating_add(*c);
        } else {
            tally.push((b.0, *c));
        }
    }
    for recipe in RECIPES {
        // Aggregate the recipe inputs the same way.
        let mut needed: Vec<(u16, u8)> = Vec::with_capacity(recipe.inputs.len());
        for (b, c) in recipe.inputs {
            if let Some(slot) = needed.iter_mut().find(|(bb, _)| *bb == *b) {
                slot.1 = slot.1.saturating_add(*c);
            } else {
                needed.push((*b, *c));
            }
        }
        // The match requires the tally to have at least every
        // (block, count) the recipe needs - and the tally must
        // contain no other blocks (otherwise the user just stuffed
        // random extras in).
        if needed.len() != tally.len() {
            continue;
        }
        let mut ok = true;
        for (b, n) in &needed {
            match tally.iter().find(|(bb, _)| bb == b) {
                Some((_, have)) if *have >= *n => {}
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            return Some((BlockId(recipe.output.0), recipe.output.1, recipe.name));
        }
    }
    None
}

/// Identifier for an inventory cell. Used as the drag&drop payload
/// so we know both ends of a drag operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SlotKind {
    /// One of the 36 main inventory slots (0..HOTBAR_SLOTS = hotbar,
    /// HOTBAR_SLOTS..INVENTORY_SLOTS = storage rows).
    Inv(usize),
    /// One of the 4 cells in the 2x2 crafting grid.
    Craft(usize),
    /// The single crafting-output cell. Drag-only (you can't drop
    /// stacks onto it).
    CraftOut,
}

/// Quantity of items to move on a single drag completion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum SplitMode {
    /// Move every item in the source stack.
    #[default]
    All,
    /// Move ceil(count/2) - lets the player halve down to 1
    /// through repeated shift-drags.
    Half,
    /// Move exactly one item - useful for filling crafting slots
    /// without dumping the rest of the stack.
    One,
}

/// UI intent captured during the egui pass + applied after the
/// closure exits. Drag operations are the main interaction; the
/// hotbar still supports a plain click to change the active slot.
#[derive(Debug, Clone, Copy)]
enum InvAction {
    /// Plain click on a hotbar slot: changes the active selection.
    SelectHotbar(usize),
    /// Drag&drop finished: move stack from `from` to `to`. The
    /// `split` field determines how much of the source stack
    /// transfers - `All` is the standard whole-stack move, `Half`
    /// shifts the upper half (good for splitting), `One` peels
    /// off a single item (good for filling craft slots).
    Drag {
        from: SlotKind,
        to: SlotKind,
        split: SplitMode,
    },
}

/// Visual snapshot of one slot, lifted out of the inventory state
/// for the egui pass. `(color, count, uv_rect, name)` where
/// `uv_rect` is the atlas tile to paint (we always show the top
/// face) and `name` is the block's display name used for the
/// hover tooltip.
type SlotIcon = ([f32; 3], u8, [f32; 4], &'static str);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum AppState {
    MainMenu,
    /// Direct-IP connect dialog: user types `host:port` and hits Join.
    DirectConnect,
    /// Browses servers fetched from the configured master server.
    ServerBrowser,
    /// Configures + spawns a host (game server). User can opt to
    /// announce on the master so others see it in the browser.
    HostSetup,
    Playing,
    Paused,
    Settings { back_to: BackTo },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum BackTo {
    MainMenu,
    Pause,
}

#[derive(Debug, Clone)]
struct Settings {
    mouse_sensitivity: f32,
    lock_cursor: bool,
    hide_cursor: bool,
    show_fps: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.6,
            lock_cursor: true,
            hide_cursor: true,
            show_fps: true,
        }
    }
}

struct Voxel {
    state: AppState,
    settings: Settings,
    requested_quit: bool,

    renderer: Option<Renderer>,
    ui: Option<EditorUi>,
    registry: BlockRegistry,
    world: World,
    /// Mesh handles grouped by the chunk that produced them. Lets the
    /// streaming code rebuild / unload only the chunks that change.
    chunk_meshes: Vec<(ChunkCoord, BlockId, MeshHandle)>,
    /// Chunks queued for generation that haven't been dispatched to a
    /// worker yet.
    pending_chunks: VecDeque<ChunkCoord>,
    /// Set of chunks either queued OR currently being generated by a
    /// worker thread. Used to avoid re-dispatching the same coord.
    pending_set: HashSet<ChunkCoord>,
    /// Worker pool the streaming code dispatches generation to. CPU-
    /// heavy `generate_chunk` + `chunk_mesh` run on the workers; the
    /// main thread only does the GPU buffer upload + neighbour
    /// remeshing.
    workers: Option<WorkerPool>,
    /// Most recent ray hit while looking around in Play. Drives the
    /// crosshair / wireframe overlay and the click-to-edit step.
    target: Option<RayHit>,

    player_pos: Vec3,
    player_vel: Vec3,
    player_yaw: f32,
    player_pitch: f32,

    // True while the OS reports this window as focused. Cursor
    // lock + mouse-motion handling are gated on it so two instances
    // on the same machine don't fight over input.
    has_focus: bool,
    last_mouse: Option<[f32; 2]>,
    mouse_dx: f32,
    mouse_dy: f32,
    esc_prev: bool,
    /// Previous-frame state of the E key. Drives the inventory-open
    /// toggle: only fires on the rising edge so holding E doesn't
    /// flicker the overlay.
    e_prev: bool,
    lmb_pending: bool,
    rmb_pending: bool,
    /// `None` = break slot (empty hand). `Some(block)` = place that
    /// block. Indexed by `selected_slot`.
    /// Flat slot array. Indices `0..HOTBAR_SLOTS` are the hotbar
    /// row (visible at the bottom of the screen, scrollable via
    /// mouse wheel); indices `HOTBAR_SLOTS..INVENTORY_SLOTS` are
    /// the extra rows shown when the player opens the inventory
    /// via `E`. Each slot holds an optional `(BlockId, count)` -
    /// `count` saturates at `STACK_MAX`.
    inventory: [Option<(BlockId, u8)>; INVENTORY_SLOTS],
    selected_slot: usize,
    /// True when the inventory overlay is up. Pauses player input
    /// + shows the cursor.
    inventory_open: bool,
    /// In multiplayer roles, Esc opens an overlay menu *without*
    /// pausing the world simulation - other peers shouldn't see a
    /// frozen ghost just because someone hit Esc. This flag drives
    /// the overlay; `update_play` keeps ticking either way.
    mp_menu_open: bool,
    /// 2x2 crafting grid laid out row-major (idx = row*2 + col).
    /// Drained on a successful craft. Only relevant while the
    /// inventory overlay is open.
    craft_grid: [Option<(BlockId, u8)>; 4],
    /// Accumulated mouse-wheel ticks since the last tick consume; the
    /// sign drives slot selection.
    wheel_accum: f32,
    /// `true` when the player's feet sit on a solid voxel; resets
    /// every frame after collision resolution.
    grounded: bool,

    viewport: (u32, u32),
    fps_ema: f32,
    /// Accumulated in-world time (seconds). Wraps every
    /// `DAY_LENGTH_S`. Drives sun direction + sky color so the
    /// world has a visible day/night cycle.
    day_time: f32,

    /// Networking role + state. `Offline` runs the demo unchanged;
    /// `Server` / `Client` flip on the per-tick replication path.
    net: NetRole,
    /// Authoritative edit log on the server. Late-joining clients
    /// get the full history rebroadcast so their generated world
    /// catches up with everyone else's. Server-only; clients leave
    /// it empty.
    edit_history: Vec<EditMsg>,
    /// Client ids the server saw last tick. Used to detect joins
    /// for the catch-up broadcast.
    known_client_ids: HashSet<u32>,
    /// Last time the server's `edit_history` was flushed to disk.
    /// `None` until the first save fires (30 s after Play start);
    /// any later instant gates the next periodic flush.
    last_state_save: Option<std::time::Instant>,
    /// Side map from client name -> their persisted inventory.
    /// Populated on `load_server_state` for hosts; consulted when
    /// a client connects so reconnects pick up the same stacks.
    /// Empty in offline / client modes.
    saved_inventories: HashMap<String, ClientInv>,
    /// Monotonic tick counter incremented at the start of each
    /// `tick_net` call. Used to throttle pending-edit / pending-
    /// inventory-action resends so we don't spam UDP every frame.
    net_tick: u64,
    /// Audio mixer. Lazily initialised on first `on_start` because
    /// the cpal backend can fail (no output device, locked driver,
    /// etc.) and we don't want the demo to refuse to launch.
    audio: Option<infengine_audio::AudioSystem>,
    /// Multi-variant sound bank keyed by event name (e.g.
    /// `"dig:grass"`, `"step:wood"`, `"mob/sheep/say"`). Loaded
    /// from `--sounds <dir>` on startup; missing keys fall back
    /// to the procedural synth in `sfx_at`.
    audio_bank: infengine_audio::AudioBank,
    /// Optional directory of .ogg sound effects. When set, `on_start`
    /// tries to load `break.ogg`, `place.ogg`, `step.ogg` into
    /// `sound_assets`.
    sounds_dir: Option<std::path::PathBuf>,
    /// Footstep cooldown: counts down each frame while the player
    /// is moving + grounded. Resets to STEP_PERIOD whenever a
    /// step sting fires.
    step_cooldown: f32,
    /// F3 toggle: show a debug-info panel with player pos, chunk
    /// counts, net status, etc. Default off so the casual HUD
    /// stays clean.
    show_debug: bool,
    f3_prev: bool,
    /// World coords of every placed torch. Updated on every world
    /// edit (break clears, place inserts). Used per frame to push
    /// point lights to the renderer near the player.
    torch_positions: HashSet<[i32; 3]>,
    /// Server-authoritative mob list (sheep + future kinds). Only
    /// populated in `NetRole::Server` / `NetRole::Offline`. Each
    /// tick the server updates pos / yaw + broadcasts `vx.mob`.
    mobs: Vec<MobState>,
    /// Client-side view of remote mobs: `id -> latest MobMsg`.
    /// In offline / server mode this stays in sync with `mobs` so
    /// the same render path works.
    remote_mobs: HashMap<u32, MobMsg>,
    /// Day-clock value of the last successful mob spawn. Used to
    /// throttle spawn attempts to one every `MOB_SPAWN_PERIOD`
    /// seconds.
    last_mob_spawn_time: f32,
    /// Counter for the next mob id. Reset to 1 because 0 is the
    /// host player's id in the pose table - no point in collisions.
    next_mob_id: u32,
    /// Per-mob step-sound cooldown (seconds). Drained while the
    /// mob is moving; sound fires + resets when it reaches zero.
    mob_step_timers: HashMap<u32, f32>,
    /// Per-mob baa/say cooldown (seconds). Decrements every tick;
    /// fire + roll a new random delay when it reaches zero.
    mob_say_timers: HashMap<u32, f32>,
    /// Last known (x, z) for each mob - used to detect movement
    /// for step-sound triggering on the client (the server doesn't
    /// broadcast velocity, so we infer from pos deltas).
    mob_last_xz: HashMap<u32, [f32; 2]>,
    /// Visual hurt-flash decay per mob (seconds remaining). Tinted
    /// red while > 0, fades each frame. Populated locally on the
    /// attacker + by `vx.mob_event` broadcasts on every other
    /// peer so everyone sees the flash.
    mob_hurt_flash: HashMap<u32, f32>,
    /// Cube mesh used to render every remote player as a coloured
    /// 1x1x1 marker. Built lazily once the renderer is up.
    player_cube: Option<MeshHandle>,
    /// Dedicated cube mesh for sheep bodies. Has its own
    /// `AlbedoHandle` binding so the texture (loaded from
    /// `assets/textures/entity/sheep/sheep.tga`) doesn't leak onto
    /// the player marker cubes.
    sheep_mesh: Option<MeshHandle>,
    /// Dedicated cube mesh for sheep heads, bound to a 16x16
    /// crop of the MC sheep entity texture (the face region).
    /// Falls back to flat-colour rendering on `player_cube` when
    /// the source `sheep.tga` is missing.
    sheep_head_mesh: Option<MeshHandle>,
    /// CPU particle system: short-lived debris on every block break.
    particles: infengine_particles::ParticleSystem,
    /// Shared atlas bind group for the voxel chunk submeshes. Built
    /// once in `on_start`; every new chunk submesh clones the Arc
    /// instead of uploading a fresh texture.
    voxel_atlas: Option<infengine_render::AlbedoHandle>,
    /// Raw RGBA copy of the block atlas, retained so the egui UI
    /// can upload its own texture handle for slot icons (without
    /// re-reading the disk / re-running the procedural builder).
    atlas_rgba: Option<(u32, u32, Vec<u8>)>,
    /// egui texture handle for the block atlas. Loaded lazily on
    /// first inventory render so we don't pay the upload cost until
    /// the player actually opens the menu.
    egui_atlas: Option<egui::TextureHandle>,
    /// Optional directory of PNG / TGA block textures. When set,
    /// `on_start` tries to load the atlas from there before
    /// falling back to the procedural builder.
    texture_dir: Option<std::path::PathBuf>,
    /// Master-server endpoint the in-game browser queries. Settable
    /// via `--master <addr>` argv; defaults to the configured
    /// public master.
    master_addr: String,
    /// Buffers + state backing the three new menu screens. Lifted
    /// out of `AppState` so the enum stays Copy.
    direct_addr_buf: String,
    host_name_buf: String,
    host_port_buf: String,
    host_announce: bool,
    browser_servers: Vec<infengine_net::master::ServerInfo>,
    browser_error: Option<String>,
    browser_last_refresh: Option<std::time::Instant>,
    /// In-flight master `ListServers` query. The refresh path now
    /// runs on a worker thread so the UI doesn't freeze for the
    /// 2-second master timeout. `Some` while a query is pending;
    /// cleared on result delivery.
    browser_query_rx:
        Option<std::sync::mpsc::Receiver<Result<Vec<infengine_net::master::ServerInfo>, String>>>,
    /// In-flight master `Register` query (host announce). Same
    /// async pattern as the browser refresh.
    announce_query_rx: Option<std::sync::mpsc::Receiver<Result<u64, String>>>,
    /// Returned by the master server on successful Register; the
    /// host echoes it back on Heartbeat + Unregister.
    master_token: Option<u64>,
    /// Last heartbeat we sent to the master (server mode). Drives
    /// the periodic re-heartbeat in `tick_net`.
    last_heartbeat: Option<std::time::Instant>,
}

impl Voxel {
    fn new_with_net(net: NetRole) -> Self {
        let mut v = Self::new();
        v.net = net;
        v
    }

    /// Set a single voxel in the world + maintain the torch index
    /// so dynamic point lights track placed/broken torches. Every
    /// edit-apply path funnels through here so we only have one
    /// place that knows about torch bookkeeping.
    fn world_set_tracked(&mut self, voxel: [i32; 3], block: BlockId) {
        self.world.set(voxel, block);
        if block.0 == TORCH_ID {
            self.torch_positions.insert(voxel);
        } else {
            self.torch_positions.remove(&voxel);
        }
    }

    /// Authoritative damage application. Caller must own the
    /// world simulation (offline or host server). Subtracts
    /// `MOB_HIT_DAMAGE` from the mob's health, broadcasts a hurt
    /// or death event so every client renders the flash, and on
    /// death drops loot into `attacker_inv`. `attacker_inv` is
    /// `None` for "no inventory credit" (e.g. fall damage path
    /// later).
    fn damage_mob(&mut self, mob_id: u32, attacker_client_id: Option<u32>) {
        // Find the mob (server-side).
        let Some(idx) = self.mobs.iter().position(|m| m.id == mob_id) else {
            return;
        };
        let mob_pos;
        let killed;
        {
            let m = &mut self.mobs[idx];
            m.health -= MOB_HIT_DAMAGE;
            mob_pos = m.pos;
            killed = m.health <= 0.0;
        }
        // Local visual flash on every peer that has the mob.
        self.mob_hurt_flash.insert(mob_id, 0.30);
        // Broadcast the event so remote clients flash + play
        // sound. Hurt = 0, Death = 1.
        if let NetRole::Server { server, .. } = &self.net {
            let cfg = bincode::config::standard();
            let evt = MobEventMsg {
                id: mob_id,
                kind: if killed { 1 } else { 0 },
            };
            if let Ok(buf) = bincode::serde::encode_to_vec(evt, cfg) {
                server.broadcast_bytes("vx.mob_event", buf);
            }
        }
        // Hurt SFX locally too.
        self.sfx_at("mob/sheep/say", mob_pos, |a, g| {
            let _ = a.play_beep(280.0, 0.12, 0.20 * g);
        });
        if !killed {
            return;
        }
        // Death: remove from mob list + remote table + tell every
        // peer to drop it from their table.
        self.mobs.swap_remove(idx);
        self.remote_mobs.remove(&mob_id);
        self.mob_hurt_flash.remove(&mob_id);
        self.mob_step_timers.remove(&mob_id);
        self.mob_say_timers.remove(&mob_id);
        self.mob_last_xz.remove(&mob_id);
        if let NetRole::Server { server, .. } = &self.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(mob_id, cfg) {
                server.broadcast_bytes("vx.mob_remove", buf);
            }
        }
        // Loot: 1-3 wool + 1-2 meat. Quantities seeded from the
        // mob id so a save/replay produces identical drops.
        let seed = mob_id.wrapping_mul(2654435761);
        let wool_qty = 1 + (seed & 0x3) as u8; // 1..=3 (mostly 1-2)
        let meat_qty = 1 + ((seed >> 8) & 0x1) as u8; // 1..=2
        let wool_qty = wool_qty.min(3);
        let meat_qty = meat_qty.min(2);
        // Credit drops to the attacker's inventory. Host attacker
        // = `None` (use `self.inventory` directly), remote client
        // = look up by id in `inventories`.
        match attacker_client_id {
            None | Some(0) => {
                for _ in 0..wool_qty {
                    self.pickup(BlockId(11));
                }
                for _ in 0..meat_qty {
                    self.pickup(BlockId(12));
                }
            }
            Some(cid) => {
                if let NetRole::Server {
                    inventories,
                    pending_inv_sends,
                    ..
                } = &mut self.net
                    && let Some(inv) = inventories.get_mut(&cid)
                {
                    for _ in 0..wool_qty {
                        inv_pickup(&mut inv.inventory, BlockId(11));
                    }
                    for _ in 0..meat_qty {
                        inv_pickup(&mut inv.inventory, BlockId(12));
                    }
                    pending_inv_sends.insert(cid);
                }
            }
        }
        // Particle burst at the death position so the kill reads
        // visually even when the SFX is muted.
        let voxel = [
            mob_pos[0].floor() as i32,
            mob_pos[1].floor() as i32,
            mob_pos[2].floor() as i32,
        ];
        self.spawn_break_particles(voxel);
    }

    /// Raycast a slab-tested ray against every known mob's AABB.
    /// Returns `Some((mob_id, distance))` for the nearest hit
    /// inside `max_dist`, or `None`. Uses the rendered cube
    /// extents (body 1.2x0.8x0.7 + head bump) approximated as one
    /// bigger 1.2x1.4x0.8 box so the player doesn't have to pixel-
    /// hunt the small head.
    fn raycast_mob(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
    ) -> Option<(u32, f32)> {
        let mut best: Option<(u32, f32)> = None;
        for m in self.remote_mobs.values() {
            // Centre the AABB on the body centre (body is drawn at
            // pos.y + 0.4 with extents 1.2 / 2, 0.8 / 2, 0.7 / 2).
            // Expand vertically to also cover the head.
            let cx = m.pos[0];
            let cy = m.pos[1] + 0.55;
            let cz = m.pos[2];
            let hx = 0.7;
            let hy = 0.75;
            let hz = 0.5;
            let min = [cx - hx, cy - hy, cz - hz];
            let max = [cx + hx, cy + hy, cz + hz];
            if let Some(t) = ray_aabb(origin.into(), dir.into(), min, max)
                && t <= max_dist
                && best.is_none_or(|(_, bt)| t < bt)
            {
                best = Some((m.id, t));
            }
        }
        best
    }

    /// Find the world-space Y of the topmost solid voxel at column
    /// `(wx, wz)` at or below `max_y`. `None` if the column is air
    /// all the way down.
    ///
    /// We scan from `max_y` downward (instead of from the world
    /// ceiling) so a sheep walking under a tree's leaf canopy
    /// doesn't get its surface snap teleported up to the canopy
    /// top - the relevant ground is the first solid block below
    /// the mob's current head, not the highest block in the
    /// column.
    fn surface_y_below(&self, wx: i32, wz: i32, max_y: i32) -> Option<i32> {
        let top = max_y.min((CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1).max(0);
        (0..=top).rev().find(|&y| !self.world.get([wx, y, wz]).is_air())
    }

    /// Server-side mob update: gravity-snap to surface, walk
    /// toward target, retarget on arrival / timeout, spawn new
    /// sheep when the cap allows. Builds the broadcast list as a
    /// side effect.
    fn tick_mobs(&mut self, dt: f32) {
        // Only the authoritative side ticks mobs. Clients render
        // from `remote_mobs` which is updated by `vx.mob`.
        if matches!(self.net, NetRole::Client { .. }) {
            return;
        }
        // Step every mob toward its target.
        let day_time = self.day_time;
        let mut next_id = self.next_mob_id;
        let mut rand_seed: u32 =
            (self.net_tick.wrapping_mul(2654435761) ^ 0xC0FFEE) as u32;
        let mut rand = || -> u32 {
            rand_seed ^= rand_seed << 13;
            rand_seed ^= rand_seed >> 17;
            rand_seed ^= rand_seed << 5;
            rand_seed
        };
        let mut rand01 = || (rand() as f32 / u32::MAX as f32).clamp(0.0, 1.0);
        // Walk each mob. Collect a fresh broadcast set in
        // `remote_mobs` so the host's own render path stays in sync.
        let mobs_count = self.mobs.len();
        for i in 0..mobs_count {
            let mut m = self.mobs[i];
            let dx = m.target[0] - m.pos[0];
            let dz = m.target[2] - m.pos[2];
            let d2 = dx * dx + dz * dz;
            if d2 < 0.25 || day_time >= m.retarget_at {
                // Pick a new wander target nearby (within 12 blocks).
                let theta = rand01() * std::f32::consts::TAU;
                let dist = 4.0 + rand01() * 8.0;
                m.target = [
                    m.pos[0] + theta.cos() * dist,
                    m.pos[1],
                    m.pos[2] + theta.sin() * dist,
                ];
                m.retarget_at = (day_time + 5.0).rem_euclid(DAY_LENGTH_S);
            } else {
                let inv = d2.sqrt().max(1e-3).recip();
                let vx = dx * inv * MOB_SPEED;
                let vz = dz * inv * MOB_SPEED;
                // Axis-resolved collision against world voxels so
                // the sheep can't walk into trees / walls. The body
                // is one cell tall; we test both the body level
                // (head height) and the level above the foot to
                // catch tree trunks reliably.
                let step_x = vx * dt;
                let step_z = vz * dt;
                let mut ax_block = false;
                let mut az_block = false;
                let try_x = m.pos[0] + step_x;
                let body_y = m.pos[1].floor() as i32;
                if !self.world.get([try_x.floor() as i32, body_y, m.pos[2].floor() as i32]).is_air()
                    || !self
                        .world
                        .get([try_x.floor() as i32, body_y + 1, m.pos[2].floor() as i32])
                        .is_air()
                {
                    ax_block = true;
                } else {
                    m.pos[0] = try_x;
                }
                let try_z = m.pos[2] + step_z;
                if !self
                    .world
                    .get([m.pos[0].floor() as i32, body_y, try_z.floor() as i32])
                    .is_air()
                    || !self
                        .world
                        .get([m.pos[0].floor() as i32, body_y + 1, try_z.floor() as i32])
                        .is_air()
                {
                    az_block = true;
                } else {
                    m.pos[2] = try_z;
                }
                // Both axes blocked: bail out by forcing a retarget
                // next tick. Stops the sheep from grinding into a
                // corner forever.
                if ax_block && az_block {
                    m.retarget_at = day_time;
                }
                // `Quat::from_rotation_y(yaw)` rotates +X to
                // `(cos yaw, 0, -sin yaw)` in our right-handed
                // Y-up frame, so the world-velocity needs the
                // negated-Z form to keep body + head pointing the
                // same direction the sheep is walking.
                m.yaw = (-vz).atan2(vx);
            }
            // Surface snap: keep the sheep's feet on the ground.
            // Scan from one block above the mob's current feet
            // downwards so we ignore tree canopies and other
            // overhead voxels - the sheep should snap to the
            // floor under itself, not the tallest block in the
            // column.
            let wx = m.pos[0].floor() as i32;
            let wz = m.pos[2].floor() as i32;
            let scan_top = m.pos[1].floor() as i32;
            if let Some(sy) = self.surface_y_below(wx, wz, scan_top) {
                m.pos[1] = sy as f32 + 1.0;
            }
            self.mobs[i] = m;
            self.remote_mobs.insert(
                m.id,
                MobMsg {
                    id: m.id,
                    kind: m.kind,
                    pos: m.pos,
                    yaw: m.yaw,
                },
            );
        }
        // Spawn pass: if below cap + last spawn far enough behind,
        // try once. Pick a column near the origin; require its
        // surface to be grass (id 3).
        const MOB_SPAWN_PERIOD: f32 = 6.0;
        let due = ((day_time - self.last_mob_spawn_time).rem_euclid(DAY_LENGTH_S))
            > MOB_SPAWN_PERIOD;
        if self.mobs.len() < MOB_CAP && due {
            let wx = (rand01() * 40.0 - 20.0).round() as i32;
            let wz = (rand01() * 40.0 - 20.0).round() as i32;
            // Spawn scan: probe the column ceiling so the sheep
            // lands on real ground (canopy is fine here too since
            // mobs spawn from above).
            let ceiling = (CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1;
            if let Some(sy) = self.surface_y_below(wx, wz, ceiling) {
                let top = self.world.get([wx, sy, wz]);
                if top.0 == 3 {
                    let id = next_id;
                    next_id = next_id.wrapping_add(1).max(1);
                    let pos = [wx as f32 + 0.5, sy as f32 + 1.0, wz as f32 + 0.5];
                    let mob = MobState {
                        id,
                        kind: 0,
                        pos,
                        target: pos,
                        yaw: 0.0,
                        retarget_at: (day_time + 1.0).rem_euclid(DAY_LENGTH_S),
                        health: MOB_MAX_HEALTH,
                    };
                    self.mobs.push(mob);
                    self.remote_mobs.insert(
                        id,
                        MobMsg {
                            id,
                            kind: 0,
                            pos,
                            yaw: 0.0,
                        },
                    );
                    self.last_mob_spawn_time = day_time;
                }
            }
        }
        self.next_mob_id = next_id;
        // Broadcast every mob's pose to all connected clients.
        if let NetRole::Server { server, .. } = &self.net {
            let cfg = bincode::config::standard();
            for m in &self.mobs {
                let msg = MobMsg {
                    id: m.id,
                    kind: m.kind,
                    pos: m.pos,
                    yaw: m.yaw,
                };
                if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                    server.broadcast_bytes("vx.mob", buf);
                }
            }
        }
    }

    /// Tick the per-mob step + baa sound timers. Runs on every
    /// peer (the server's audio comes through too if the host is
    /// playing). Step sounds fire only when the mob actually moved
    /// since the last tick; baa fires on a randomised cooldown.
    fn tick_mob_audio(&mut self, dt: f32) {
        // Decay every hurt flash regardless of audio availability
        // so the visual tint clears even when SFX are disabled.
        self.mob_hurt_flash.retain(|_, t| {
            *t -= dt;
            *t > 0.0
        });
        if self.audio.is_none() {
            return;
        }
        // Snapshot the mob list so we don't keep `self.remote_mobs`
        // borrowed while calling back into `&mut self` via sfx_at.
        let mobs: Vec<MobMsg> = self.remote_mobs.values().copied().collect();
        // Drop timers for mobs that no longer exist (despawned).
        let live: HashSet<u32> = mobs.iter().map(|m| m.id).collect();
        self.mob_step_timers.retain(|id, _| live.contains(id));
        self.mob_say_timers.retain(|id, _| live.contains(id));
        self.mob_last_xz.retain(|id, _| live.contains(id));
        for m in mobs {
            // Step detection: compare to last known XZ. If moved
            // by more than a small epsilon, count it as active
            // motion; decrement timer + fire when due.
            let last = self.mob_last_xz.get(&m.id).copied();
            self.mob_last_xz.insert(m.id, [m.pos[0], m.pos[2]]);
            let moved = match last {
                Some([lx, lz]) => {
                    let dx = m.pos[0] - lx;
                    let dz = m.pos[2] - lz;
                    (dx * dx + dz * dz).sqrt() > 0.02
                }
                None => false,
            };
            let step_t = self.mob_step_timers.entry(m.id).or_insert(0.45);
            if moved {
                *step_t -= dt;
                if *step_t <= 0.0 {
                    *step_t = 0.45;
                    let pos = m.pos;
                    self.sfx_at("mob/sheep/step", pos, |a, g| {
                        let _ = a.play_swish(0.18 * g);
                    });
                }
            }
            // Baa: random initial timer, refresh on each fire.
            let say_t = self
                .mob_say_timers
                .entry(m.id)
                .or_insert_with(|| 5.0 + (m.id as f32 * 0.71).fract() * 12.0);
            *say_t -= dt;
            if *say_t <= 0.0 {
                // Pseudo-random new delay in [6, 20) s. Hash by
                // mob id + current net tick so we don't get a
                // chorus when many sheep retime together.
                let h = (m.id.wrapping_mul(2654435761) ^ self.net_tick as u32) as f32;
                *say_t = 6.0 + (h % 14.0).abs();
                let pos = m.pos;
                self.sfx_at("mob/sheep/say", pos, |a, g| {
                    let _ = a.play_beep(300.0, 0.18, 0.18 * g);
                });
            }
        }
    }

    /// Play a positional sound at `pos`. Volume falls off linearly
    /// with distance from the player and cuts at `AUDIBLE_RANGE`.
    /// `key` looks up the multi-variant `sound_assets` table; if
    /// found, a random variant plays with the computed gain. If
    /// the key is missing, `synth` is invoked as a procedural
    /// fallback (also gain-attenuated).
    fn sfx_at(
        &mut self,
        key: &str,
        pos: [f32; 3],
        synth: impl FnOnce(&mut infengine_audio::AudioSystem, f32),
    ) {
        const AUDIBLE_RANGE: f32 = 24.0;
        let Some(a) = self.audio.as_mut() else {
            return;
        };
        let listener = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
        // Seed the variant shuffle from tick + spatial position so
        // back-to-back triggers don't reuse the same recording.
        let seed = self.net_tick.wrapping_mul(2654435761)
            ^ (pos[0].to_bits() as u64).wrapping_mul(7919)
            ^ (pos[2].to_bits() as u64);
        if self
            .audio_bank
            .play_at(a, key, listener, pos, AUDIBLE_RANGE, seed)
            .is_some()
        {
            return;
        }
        // Bank miss / out of range. For out-of-range we still want
        // the synth path to gate on distance, so reuse the same
        // formula here.
        let dx = pos[0] - listener[0];
        let dy = pos[1] - listener[1];
        let dz = pos[2] - listener[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist >= AUDIBLE_RANGE {
            return;
        }
        let gain = (1.0 - dist / AUDIBLE_RANGE).clamp(0.0, 1.0);
        synth(a, gain);
    }


    /// Path of the server-side world delta file. Lives next to the
    /// launch directory so a host can be torn down + brought back up
    /// without losing the players' edits.
    const STATE_FILE: &'static str = "voxel_server_state.bin";

    /// Magic bytes at the head of every save: `"INFV"` (InfEngine
    /// Voxel). Distinguishes the new versioned format from the old
    /// raw `Vec<EditMsg>` format - if the first 4 bytes aren't the
    /// magic, the loader treats the whole file as a v1 edit log.
    const STATE_MAGIC: &'static [u8; 4] = b"INFV";

    /// Read the on-disk edit log and apply every entry to the local
    /// world. Called once on the server right after the world's
    /// procedural base is generated. Tries the versioned format
    /// first, falls back to the legacy raw edit log on a magic
    /// mismatch so existing save files keep working.
    fn load_server_state(&mut self) {
        let path = std::path::PathBuf::from(Self::STATE_FILE);
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let cfg = bincode::config::standard();
        // Path A: new versioned format with `INFV` magic.
        if bytes.len() >= 4 && &bytes[..4] == Self::STATE_MAGIC {
            match bincode::serde::decode_from_slice::<ServerStateV2, _>(&bytes[4..], cfg) {
                Ok((state, _)) => {
                    info!(
                        version = state.version,
                        edits = state.edits.len(),
                        clients = state.inventories.len(),
                        day_time = state.day_time,
                        path = %path.display(),
                        "loaded server state (v2)"
                    );
                    for e in &state.edits {
                        self.world_set_tracked([e.x, e.y, e.z], BlockId(e.block));
                    }
                    self.edit_history = state.edits;
                    self.day_time = state.day_time.rem_euclid(DAY_LENGTH_S);
                    // Restore inventories into a side map indexed by
                    // client name. They get re-keyed to live client
                    // ids on connect (see `tick_net`).
                    self.saved_inventories = state
                        .inventories
                        .into_iter()
                        .map(|(name, p)| (name, p.into_client_inv()))
                        .collect();
                    self.rebuild_all_meshes();
                    return;
                }
                Err(err) => {
                    warn!(error = %err, path = %path.display(), "state load v2 decode failed");
                    return;
                }
            }
        }
        // Path B: legacy `Vec<EditMsg>` format.
        let edits: Vec<EditMsg> = match bincode::serde::decode_from_slice(&bytes, cfg) {
            Ok((e, _)) => e,
            Err(err) => {
                warn!(error = %err, path = %path.display(), "state load (legacy) decode failed");
                return;
            }
        };
        info!(count = edits.len(), path = %path.display(), "loaded server state (legacy v1)");
        for e in &edits {
            self.world_set_tracked([e.x, e.y, e.z], BlockId(e.block));
        }
        self.edit_history = edits;
        self.rebuild_all_meshes();
    }

    /// Flush the in-memory edit log + per-client inventories +
    /// day-of-cycle to disk in the new versioned `INFV` format.
    /// Called periodically while the server runs (every 30 s) so a
    /// crash loses at most half a minute of state.
    /// Fire-and-forget `vx.leave` to the server (if any). Called
    /// on explicit disconnect / quit so the server doesn't have
    /// to wait its 5 s silence reaper to broadcast a removal.
    fn notify_server_leaving(&self) {
        if let NetRole::Client { client, .. } = &self.net {
            let _ = client.send_bytes("vx.leave", Vec::new());
        }
    }

    fn save_server_state(&mut self) {
        if !matches!(self.net, NetRole::Server { .. }) {
            return;
        }
        let path = std::path::PathBuf::from(Self::STATE_FILE);
        let cfg = bincode::config::standard();
        // Build a name -> PersistedInv map by joining live client
        // ids with their reported names (from NetServer) + the
        // saved_inventories side table (for clients that already
        // disconnected this run but still have state we want to
        // preserve across restarts).
        let mut by_name: HashMap<String, PersistedInv> = self
            .saved_inventories
            .iter()
            .map(|(n, inv)| (n.clone(), PersistedInv::from_client_inv(inv)))
            .collect();
        if let NetRole::Server {
            server,
            inventories,
            ..
        } = &self.net
        {
            for (cid, inv) in inventories.iter() {
                if let Some(name) = server.client_name(*cid) {
                    by_name.insert(name, PersistedInv::from_client_inv(inv));
                }
            }
        }
        let state = ServerStateV2 {
            version: SERVER_STATE_VERSION,
            edits: self.edit_history.clone(),
            inventories: by_name.into_iter().collect(),
            day_time: self.day_time,
        };
        match bincode::serde::encode_to_vec(&state, cfg) {
            Ok(payload) => {
                let mut bytes = Vec::with_capacity(payload.len() + 4);
                bytes.extend_from_slice(Self::STATE_MAGIC);
                bytes.extend_from_slice(&payload);
                if let Err(err) = std::fs::write(&path, &bytes) {
                    warn!(error = %err, path = %path.display(), "state save failed");
                }
            }
            Err(err) => warn!(error = %err, "state save encode failed"),
        }
        self.last_state_save = Some(std::time::Instant::now());
    }

    /// Push a small burst of debris particles into the system at the
    /// centre of `voxel`. Used as a "block broken" effect from every
    /// edit-apply path so both the player and remote players see
    /// puffs when something mines.
    fn spawn_break_particles(&mut self, voxel: [i32; 3]) {
        let pos = [
            voxel[0] as f32 + 0.5,
            voxel[1] as f32 + 0.5,
            voxel[2] as f32 + 0.5,
        ];
        let mut rng: u32 = 0xCAFEBABE
            ^ (voxel[0] as u32)
            ^ ((voxel[1] as u32) << 7)
            ^ ((voxel[2] as u32) << 13);
        let mut rand01 = || -> f32 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
        };
        for _ in 0..12 {
            let vx = (rand01() * 2.0 - 1.0) * 3.0;
            let vy = rand01() * 3.5;
            let vz = (rand01() * 2.0 - 1.0) * 3.0;
            self.particles.particles.push(infengine_particles::Particle {
                position: pos,
                velocity: [vx, vy, vz],
                color: [0.75, 0.55, 0.30, 1.0],
                size: 0.12,
                age: 0.0,
                lifetime: 0.5,
            });
        }
    }

    /// Burst of small dust particles when a block is placed. Half
    /// the count of `spawn_break_particles` so it reads as a
    /// gentle "click" rather than a full debris cloud, tinted with
    /// the block's albedo.
    fn spawn_place_particles(&mut self, voxel: [i32; 3], color: [f32; 3]) {
        let pos = [
            voxel[0] as f32 + 0.5,
            voxel[1] as f32 + 0.5,
            voxel[2] as f32 + 0.5,
        ];
        let mut rng: u32 = 0xABAD1DEA
            ^ (voxel[0] as u32)
            ^ ((voxel[1] as u32) << 7)
            ^ ((voxel[2] as u32) << 13);
        let mut rand01 = || -> f32 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
        };
        for _ in 0..6 {
            let vx = (rand01() * 2.0 - 1.0) * 1.5;
            let vy = rand01() * 1.0 + 0.5;
            let vz = (rand01() * 2.0 - 1.0) * 1.5;
            self.particles.particles.push(infengine_particles::Particle {
                position: pos,
                velocity: [vx, vy, vz],
                color: [color[0], color[1], color[2], 0.9],
                size: 0.08,
                age: 0.0,
                lifetime: 0.3,
            });
        }
    }

    /// Add one `block` to the inventory, Minecraft-style. The
    /// algorithm walks the whole inventory:
    ///
    /// 1. Find an existing non-full stack of the same block. Bump
    ///    its count and return.
    /// 2. Otherwise drop a fresh `(block, 1)` into the first empty
    ///    slot.
    /// 3. If everything is full, silently discard (no item entities
    ///    in this demo yet).
    ///
    /// `BlockId::AIR` is a no-op.
    fn pickup(&mut self, block: BlockId) {
        if block == BlockId::AIR {
            return;
        }
        // First pass: top up an existing stack.
        for slot in self.inventory.iter_mut() {
            if let Some((b, count)) = slot.as_mut()
                && *b == block
                && *count < STACK_MAX
            {
                *count += 1;
                return;
            }
        }
        // Second pass: claim an empty slot. Prefer hotbar first so
        // freshly mined blocks become hot-keyable immediately.
        for slot in self.inventory.iter_mut() {
            if slot.is_none() {
                *slot = Some((block, 1));
                return;
            }
        }
    }

    /// Read the (BlockId, count) currently sitting in `kind`. The
    /// crafting output is virtual: it's whatever recipe matches the
    /// current 2x2 grid (or `None` if no match).
    fn slot_read(&self, kind: SlotKind) -> Option<(BlockId, u8)> {
        match kind {
            SlotKind::Inv(i) => self.inventory.get(i).copied().flatten(),
            SlotKind::Craft(i) => self.craft_grid.get(i).copied().flatten(),
            SlotKind::CraftOut => match_recipe(&self.craft_grid),
        }
    }

    /// Write `value` into `kind`. Never writes to `CraftOut`
    /// (that's a virtual cell - taking from it goes through
    /// `consume_craft` instead).
    fn slot_write(&mut self, kind: SlotKind, value: Option<(BlockId, u8)>) {
        match kind {
            SlotKind::Inv(i) => {
                if i < self.inventory.len() {
                    self.inventory[i] = value;
                }
            }
            SlotKind::Craft(i) => {
                if i < self.craft_grid.len() {
                    self.craft_grid[i] = value;
                }
            }
            SlotKind::CraftOut => {}
        }
    }

    /// Drain one item from every populated craft cell. Called when
    /// the player takes the craft output.
    fn consume_craft(&mut self) {
        for slot in self.craft_grid.iter_mut() {
            if let Some((_, c)) = slot.as_mut() {
                *c = c.saturating_sub(1);
                if *c == 0 {
                    *slot = None;
                }
            }
        }
    }

    /// Dispatch one inventory-UI action. Drag&drop semantics: a
    /// drag from `from` to `to` either swaps the two stacks or
    /// (when the blocks match) merges into the destination.
    /// Hotbar slot 0 is the break-tool slot and refuses incoming
    /// stacks so its X icon never disappears.
    fn apply_inv_action(&mut self, action: InvAction) {
        match action {
            InvAction::SelectHotbar(idx) => {
                if idx < HOTBAR_SLOTS {
                    self.selected_slot = idx;
                }
            }
            InvAction::Drag { from, to, split } => {
                if from == to {
                    return;
                }
                // Can't drop INTO the crafting output - it's a
                // virtual cell.
                if matches!(to, SlotKind::CraftOut) {
                    return;
                }
                // Protect the break-tool slot.
                if matches!(to, SlotKind::Inv(0)) {
                    return;
                }
                let Some(from_stack) = self.slot_read(from) else {
                    return;
                };
                let to_stack = self.slot_read(to);
                let craft_take = matches!(from, SlotKind::CraftOut);
                // CraftOut drag has special rules: must land in an
                // empty slot or merge into a same-block stack with
                // room for the full output. Splitting is a no-op
                // here - the output is a single fixed packet.
                if craft_take {
                    match to_stack {
                        None => {
                            self.consume_craft();
                            self.slot_write(to, Some(from_stack));
                        }
                        Some((tb, tc))
                            if tb == from_stack.0
                                && (tc as u16 + from_stack.1 as u16) <= STACK_MAX as u16 =>
                        {
                            self.consume_craft();
                            self.slot_write(to, Some((tb, tc + from_stack.1)));
                        }
                        _ => {}
                    }
                    return;
                }
                // Partial move (`SplitMode::Half` or `One`).
                // `All` falls through to the whole-stack swap path.
                let take = match split {
                    SplitMode::All => 0u8,
                    SplitMode::Half => (from_stack.1 as u16).div_ceil(2) as u8,
                    SplitMode::One => 1u8,
                };
                if take > 0 {
                    let take = take.max(1).min(from_stack.1);
                    let rem = from_stack.1 - take;
                    match to_stack {
                        Some((tb, tc)) if tb == from_stack.0 => {
                            let merge = (STACK_MAX - tc).min(take);
                            let new_to = tc + merge;
                            let leftover = take - merge;
                            self.slot_write(to, Some((tb, new_to)));
                            // Any leftover from the take goes back
                            // into the source slot.
                            let back = rem + leftover;
                            self.slot_write(
                                from,
                                if back == 0 {
                                    None
                                } else {
                                    Some((from_stack.0, back))
                                },
                            );
                        }
                        None => {
                            self.slot_write(to, Some((from_stack.0, take)));
                            self.slot_write(
                                from,
                                if rem == 0 {
                                    None
                                } else {
                                    Some((from_stack.0, rem))
                                },
                            );
                        }
                        Some(_) => {
                            // Target occupied by a different block:
                            // a split into an unrelated slot would
                            // either swap (loses semantics) or
                            // refuse. We refuse.
                        }
                    }
                    return;
                }
                // Standard whole-stack drag between two real slots.
                match to_stack {
                    Some((tb, tc)) if tb == from_stack.0 => {
                        // Same block -> merge.
                        let take = (STACK_MAX - tc).min(from_stack.1);
                        let new_to = tc + take;
                        let rem = from_stack.1 - take;
                        self.slot_write(to, Some((tb, new_to)));
                        self.slot_write(
                            from,
                            if rem == 0 { None } else { Some((from_stack.0, rem)) },
                        );
                    }
                    _ => {
                        // Different blocks (or empty target) -> swap.
                        self.slot_write(to, Some(from_stack));
                        self.slot_write(from, to_stack);
                    }
                }
            }
        }
    }

    /// Spawn a NetClient at `addr` and transition into Playing.
    fn connect_as_client(&mut self, addr: &str) {
        match NetClient::connect(addr, "voxel-mp-client") {
            Ok(c) => {
                info!(addr = %addr, "joining server");
                self.net = NetRole::Client {
                    client: c,
                    remote: HashMap::new(),
                    pending_edits: Vec::new(),
                    pending_inv_actions: Vec::new(),
                };
                self.state = AppState::Playing;
                self.mouse_dx = 0.0;
                self.mouse_dy = 0.0;
            }
            Err(err) => {
                warn!(error = %err, addr = %addr, "client connect failed");
                self.browser_error = Some(format!("connect failed: {err}"));
            }
        }
    }

    /// Bind a NetServer on the configured port, optionally announce
    /// to the master server, then drop into Playing.
    fn start_hosting(&mut self) {
        let port: u16 = self.host_port_buf.parse().unwrap_or(7777);
        let bind = format!("0.0.0.0:{port}");
        match NetServer::bind(&bind) {
            Ok(s) => {
                info!(addr = %bind, "hosting voxel server");
                self.net = NetRole::Server {
                    server: s,
                    poses: HashMap::new(),
                    inventories: HashMap::new(),
                    pending_inv_sends: HashSet::new(),
                };
                self.state = AppState::Playing;
                self.mouse_dx = 0.0;
                self.mouse_dy = 0.0;
                if self.host_announce {
                    self.announce_to_master(port);
                }
            }
            Err(err) => {
                warn!(error = %err, port, "server bind failed");
                self.browser_error = Some(format!("host failed: {err}"));
            }
        }
    }

    /// Register this host with the configured master server. Caches
    /// the returned token so periodic heartbeats land on the right
    /// directory entry.
    fn announce_to_master(&mut self, port: u16) {
        use infengine_net::master::{MASTER_PROTO, MasterRequest, ServerInfo};
        if self.announce_query_rx.is_some() {
            return; // in-flight already
        }
        let info = ServerInfo {
            name: self.host_name_buf.clone(),
            addr: format!("0.0.0.0:{port}"),
            players: 0,
            max_players: 16,
            mode: "voxel".into(),
        };
        let req = MasterRequest::Register {
            proto: MASTER_PROTO,
            info,
        };
        let addr = self.master_addr.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use infengine_net::master::{MasterResponse, request_blocking};
            let result =
                match request_blocking(&addr, &req, std::time::Duration::from_secs(2)) {
                    Some(MasterResponse::Registered { token }) => Ok(token),
                    Some(MasterResponse::Error { message }) => {
                        Err(format!("master: {message}"))
                    }
                    _ => Err("master timeout".into()),
                };
            let _ = tx.send(result);
        });
        self.announce_query_rx = Some(rx);
    }

    /// Fetch the server list from the master and stash it for the
    /// browser UI to render.
    fn refresh_browser(&mut self) {
        use infengine_net::master::{MASTER_PROTO, MasterRequest};
        if self.browser_query_rx.is_some() {
            return; // already in flight
        }
        self.browser_error = None;
        let req = MasterRequest::ListServers {
            proto: MASTER_PROTO,
        };
        let addr = self.master_addr.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use infengine_net::master::{MasterResponse, request_blocking};
            let result =
                match request_blocking(&addr, &req, std::time::Duration::from_secs(2)) {
                    Some(MasterResponse::List { servers }) => Ok(servers),
                    Some(MasterResponse::Error { message }) => {
                        Err(format!("master: {message}"))
                    }
                    _ => Err("master timeout".into()),
                };
            let _ = tx.send(result);
        });
        self.browser_query_rx = Some(rx);
    }

    /// Drain any completed background master queries. Called every
    /// tick from `on_update` so async results land back on the UI
    /// without polling tight loops.
    fn poll_master_queries(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let browser_done = if let Some(rx) = &self.browser_query_rx {
            match rx.try_recv() {
                Ok(Ok(servers)) => {
                    info!(count = servers.len(), "fetched server list");
                    self.browser_servers = servers;
                    self.browser_error = None;
                    self.browser_last_refresh = Some(std::time::Instant::now());
                    true
                }
                Ok(Err(err)) => {
                    warn!(error = %err, "master list failed");
                    self.browser_error = Some(err);
                    self.browser_last_refresh = Some(std::time::Instant::now());
                    true
                }
                Err(TryRecvError::Empty) => false,
                Err(TryRecvError::Disconnected) => true,
            }
        } else {
            false
        };
        if browser_done {
            self.browser_query_rx = None;
        }
        let announce_done = if let Some(rx) = &self.announce_query_rx {
            match rx.try_recv() {
                Ok(Ok(token)) => {
                    info!(token, "announced to master");
                    self.master_token = Some(token);
                    self.last_heartbeat = Some(std::time::Instant::now());
                    self.browser_error = None;
                    true
                }
                Ok(Err(err)) => {
                    warn!(error = %err, "announce failed");
                    self.browser_error = Some(err);
                    true
                }
                Err(TryRecvError::Empty) => false,
                Err(TryRecvError::Disconnected) => true,
            }
        } else {
            false
        };
        if announce_done {
            self.announce_query_rx = None;
        }
    }

    /// Per-tick networking step:
    /// - Server: poll, drain client edits + poses, apply each edit
    ///   locally, broadcast applied edits, broadcast every player's
    ///   latest pose.
    /// - Client: poll, drain server snapshots, apply received edits,
    ///   update remote-player table, flush pending edits + own pose
    ///   upstream.
    fn tick_net(&mut self) {
        self.net_tick = self.net_tick.wrapping_add(1);
        let net_tick = self.net_tick;
        let player_pos = self.player_pos;
        let player_yaw = self.player_yaw;
        let player_pitch = self.player_pitch;
        // Retransmit period (in net ticks) for pending unsynced
        // payloads. UDP losses are rare on LAN so resending every
        // tick is wasteful; every 4 ticks is roughly 67 ms at
        // 60 Hz which still feels snappy.
        const RETRY_PERIOD: u64 = 4;
        let do_retry = net_tick.is_multiple_of(RETRY_PERIOD);
        // Server-side heartbeat to the master server. Cheap so we
        // can do it inline.
        if let NetRole::Server { server, poses, .. } = &self.net
            && let Some(token) = self.master_token
            && self
                .last_heartbeat
                .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(5))
        {
            use infengine_net::master::{MasterRequest, request_blocking};
            let players = poses.len().saturating_sub(1) as u32 + server.client_count() as u32;
            let req = MasterRequest::Heartbeat { token, players };
            let addr = self.master_addr.clone();
            // Fire-and-forget so the server tick never stalls on a
            // network round trip. We don't care about the response;
            // a stale token just causes the next heartbeat to log
            // a warning.
            std::thread::spawn(move || {
                let _ = request_blocking(&addr, &req, std::time::Duration::from_millis(500));
            });
            self.last_heartbeat = Some(std::time::Instant::now());
        }
        // Drain incoming bytes + edits to apply. Collect first so the
        // borrow on `self.net` releases before we mutate the world.
        let mut to_apply: Vec<EditMsg> = Vec::new();
        // Inventory snapshot the client received this tick. Applied
        // to `self.inventory` / `self.craft_grid` after the
        // `&mut self.net` borrow releases.
        let mut client_snapshot: Option<InvSnapshotMsg> = None;
        // Mob pose updates received this tick (client). Folded into
        // `self.remote_mobs` after the match exits.
        let mut mob_updates: Vec<MobMsg> = Vec::new();
        let mut player_removals: Vec<u32> = Vec::new();
        // Mob damage requests + events to process after the match
        // arm closes (damage_mob borrows the whole `&mut self`).
        let mut mob_hits_to_apply: Vec<(u32, u32)> = Vec::new();
        let mut mob_events: Vec<MobEventMsg> = Vec::new();
        let mut mob_removals: Vec<u32> = Vec::new();
        let cfg = bincode::config::standard();
        match &mut self.net {
            NetRole::Offline => return,
            NetRole::Server {
                server,
                poses,
                inventories,
                pending_inv_sends,
            } => {
                server.poll();
                // Track host pose under id 0.
                poses.insert(
                    0,
                    PlayerMsg {
                        id: 0,
                        pos: [player_pos.x, player_pos.y, player_pos.z],
                        yaw: player_yaw,
                        pitch: player_pitch,
                    },
                );
                // Raw inbound per client: (client_id, edit) +
                // (client_id, drag action). Resolved after the
                // drain loop so we can validate against the
                // authoritative inventory.
                let mut raw_edits: Vec<(u32, EditMsg)> = Vec::new();
                let mut raw_drags: Vec<(u32, InvActionMsg)> = Vec::new();
                for (client_id, kind, data) in server.drain_bytes() {
                    match kind.as_str() {
                        "vx.edit" => {
                            if let Ok((e, _)) =
                                bincode::serde::decode_from_slice::<EditMsg, _>(&data, cfg)
                            {
                                raw_edits.push((client_id, e));
                            }
                        }
                        "vx.player" => {
                            if let Ok((mut p, _)) =
                                bincode::serde::decode_from_slice::<PlayerMsg, _>(&data, cfg)
                            {
                                p.id = client_id;
                                poses.insert(client_id, p);
                            }
                        }
                        "vx.inv_act" => {
                            if let Ok((m, _)) =
                                bincode::serde::decode_from_slice::<InvActionMsg, _>(&data, cfg)
                            {
                                raw_drags.push((client_id, m));
                            }
                        }
                        "vx.mob_hit" => {
                            if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                                MobHitMsg,
                                _,
                            >(&data, cfg)
                            {
                                mob_hits_to_apply.push((client_id, m.mob_id));
                            }
                        }
                        "vx.fellout" => {
                            // Wipe the storage rows + craft grid -
                            // matches the local client penalty. Keep
                            // the hotbar so the player isn't bare.
                            if let Some(inv) = inventories.get_mut(&client_id) {
                                for s in inv.inventory[HOTBAR_SLOTS..].iter_mut() {
                                    *s = None;
                                }
                                for s in inv.craft_grid.iter_mut() {
                                    *s = None;
                                }
                                pending_inv_sends.insert(client_id);
                            }
                        }
                        "vx.leave" => {
                            // Explicit disconnect: drop the pose
                            // straight away so the reaper's 5 s
                            // grace doesn't strand a frozen ghost.
                            poses.remove(&client_id);
                            tracing::info!(
                                client_id,
                                "client requested explicit leave"
                            );
                        }
                        _ => {}
                    }
                }
                // Roster snapshot: use the authoritative connection
                // list (`server.client_ids`) rather than the pose
                // table, so dropped clients are detected as soon as
                // the reaper or an explicit `vx.leave` removes them
                // from the socket layer.
                let current_ids: HashSet<u32> =
                    server.client_ids().into_iter().collect();
                let new_ids: Vec<u32> = current_ids
                    .iter()
                    .filter(|id| !self.known_client_ids.contains(*id))
                    .copied()
                    .collect();
                let departed_ids: Vec<u32> = self
                    .known_client_ids
                    .iter()
                    .filter(|id| !current_ids.contains(*id))
                    .copied()
                    .collect();
                // Drop server-side state for clients that left so
                // their pose / inventory don't keep replicating.
                // Persist their inventory by name (if known) so the
                // next save flushes it, then evict.
                for id in &departed_ids {
                    poses.remove(id);
                    if let Some(inv) = inventories.remove(id)
                        && let Some(name) = server.client_name(*id)
                    {
                        self.saved_inventories.insert(name, inv);
                    }
                    pending_inv_sends.remove(id);
                    // Tell every still-connected peer to forget
                    // the departed player's cube.
                    let msg = PlayerRemoveMsg { id: *id };
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        server.broadcast_bytes("vx.player_remove", buf);
                    }
                }
                if !new_ids.is_empty() && !self.edit_history.is_empty() {
                    tracing::info!(
                        joined = ?new_ids,
                        edits = self.edit_history.len(),
                        "rebroadcasting edit history for new client(s)"
                    );
                    for e in &self.edit_history {
                        if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                }
                // Seed inventories for any newly-joined clients +
                // queue them an initial snapshot so their client UI
                // mirrors the authoritative server state. Restore
                // from the persisted side map when the client's
                // name matches a previous session.
                for id in &new_ids {
                    if !inventories.contains_key(id) {
                        let name = server.client_name(*id);
                        let inv = name
                            .as_ref()
                            .and_then(|n| self.saved_inventories.remove(n))
                            .unwrap_or_else(ClientInv::starter);
                        inventories.insert(*id, inv);
                    }
                    pending_inv_sends.insert(*id);
                }
                self.known_client_ids = current_ids;
                // Validate every incoming edit against the sender's
                // inventory + spatial range before accepting it.
                //
                // - BREAK: sender's pose must be within REACH of the
                //   target voxel. Passing the inventory check is
                //   trivial (breaks never consume); the pickup
                //   credit happens here too.
                // - PLACE: sender must have the block AND be within
                //   REACH of the target voxel. Silent reject on
                //   either failure (the next `vx.inv` snapshot
                //   reconciles the predicted client state).
                const REACH: f32 = 6.5;
                for (cid, e) in raw_edits {
                    // Look up the sender's last reported pose; if
                    // we've never seen one, fall back to a sentinel
                    // far away so the range check rejects the edit.
                    let pose = poses.get(&cid).copied().unwrap_or(PlayerMsg {
                        id: cid,
                        pos: [1e9, 1e9, 1e9],
                        yaw: 0.0,
                        pitch: 0.0,
                    });
                    let voxel_center = [
                        e.x as f32 + 0.5,
                        e.y as f32 + 0.5,
                        e.z as f32 + 0.5,
                    ];
                    let dx = pose.pos[0] - voxel_center[0];
                    let dy = pose.pos[1] - voxel_center[1];
                    let dz = pose.pos[2] - voxel_center[2];
                    let dist2 = dx * dx + dy * dy + dz * dz;
                    if dist2 > REACH * REACH {
                        tracing::debug!(
                            client_id = cid,
                            block = e.block,
                            "edit rejected: out of reach"
                        );
                        // Tell the client what the voxel actually
                        // looks like server-side so its prediction
                        // gets reverted on the next tick.
                        let truth = self.world.get([e.x, e.y, e.z]);
                        let revert = EditMsg {
                            x: e.x,
                            y: e.y,
                            z: e.z,
                            block: truth.0,
                        };
                        if let Ok(buf) = bincode::serde::encode_to_vec(revert, cfg) {
                            server.send_bytes_to_client(cid, "vx.edit", buf);
                        }
                        pending_inv_sends.insert(cid);
                        continue;
                    }
                    if e.block == 0 {
                        // BREAK - capture old block to credit
                        // pickup, then apply.
                        let voxel = [e.x, e.y, e.z];
                        let broken = self.world.get(voxel);
                        if broken != BlockId::AIR
                            && let Some(inv) = inventories.get_mut(&cid)
                            && inv_pickup(&mut inv.inventory, broken)
                        {
                            pending_inv_sends.insert(cid);
                        }
                        to_apply.push(e);
                    } else {
                        // PLACE - require + consume from sender's
                        // inventory. Silently drop the placement if
                        // they don't have it (anti-cheat / desync).
                        let block = BlockId(e.block);
                        let ok = inventories
                            .get_mut(&cid)
                            .is_some_and(|inv| inv_consume(&mut inv.inventory, block));
                        if ok {
                            pending_inv_sends.insert(cid);
                            to_apply.push(e);
                        } else {
                            // Reverted: send the voxel's true state
                            // so the client's predicted block
                            // vanishes; also push the snapshot to
                            // restore the (unchanged) inventory.
                            let truth = self.world.get([e.x, e.y, e.z]);
                            let revert = EditMsg {
                                x: e.x,
                                y: e.y,
                                z: e.z,
                                block: truth.0,
                            };
                            if let Ok(buf) = bincode::serde::encode_to_vec(revert, cfg) {
                                server.send_bytes_to_client(cid, "vx.edit", buf);
                            }
                            pending_inv_sends.insert(cid);
                        }
                    }
                }
                // Apply incoming drag&drop intents on the server's
                // authoritative inventory.
                for (cid, act) in raw_drags {
                    if let Some(inv) = inventories.get_mut(&cid) {
                        inv_apply_drag(inv, act.from.into(), act.to.into(), act.split);
                        pending_inv_sends.insert(cid);
                    }
                }
                // Server also records every received edit so future
                // joiners get a complete history.
                for e in &to_apply {
                    self.edit_history.push(*e);
                }
                // Broadcast each player's pose (one packet each;
                // future: batch). Also rebroadcast any incoming
                // edits we'll apply below.
                for p in poses.values() {
                    if let Ok(buf) = bincode::serde::encode_to_vec(p, cfg) {
                        server.broadcast_bytes("vx.player", buf);
                    }
                }
                for e in &to_apply {
                    if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
                // Push authoritative inventory snapshots to every
                // client whose state changed this tick.
                for cid in pending_inv_sends.drain() {
                    if let Some(inv) = inventories.get(&cid) {
                        let snap = inv.to_snapshot();
                        if let Ok(buf) = bincode::serde::encode_to_vec(&snap, cfg) {
                            server.send_bytes_to_client(cid, "vx.inv", buf);
                        }
                    }
                }
            }
            NetRole::Client {
                client,
                remote,
                pending_edits,
                pending_inv_actions,
            } => {
                client.poll();
                let my_id = client.client_id;
                // Sweep own id out of the remote table - if a
                // pre-handshake broadcast snuck our pose in before
                // we knew our own id, drop it now.
                if let Some(mine) = my_id {
                    remote.remove(&mine);
                }
                for (kind, data) in client.drain_bytes() {
                    match kind.as_str() {
                        "vx.edit" => {
                            if let Ok((e, _)) =
                                bincode::serde::decode_from_slice::<EditMsg, _>(&data, cfg)
                            {
                                // Server-echoed edit: retire any
                                // pending request that matches it
                                // by coordinate + block. Idempotent
                                // dups are cheap; this just stops
                                // the retry-resend loop.
                                if let Some(idx) = pending_edits.iter().position(|p| {
                                    p.x == e.x
                                        && p.y == e.y
                                        && p.z == e.z
                                        && p.block == e.block
                                }) {
                                    pending_edits.remove(idx);
                                }
                                to_apply.push(e);
                            }
                        }
                        "vx.player" => {
                            if let Ok((p, _)) =
                                bincode::serde::decode_from_slice::<PlayerMsg, _>(&data, cfg)
                                && my_id != Some(p.id)
                            {
                                remote.insert(p.id, p);
                            }
                        }
                        "vx.inv" => {
                            if let Ok((s, _)) = bincode::serde::decode_from_slice::<
                                InvSnapshotMsg,
                                _,
                            >(&data, cfg)
                            {
                                // Keep only the freshest snapshot
                                // this tick (last-write-wins).
                                client_snapshot = Some(s);
                                // Server acknowledged our state, so
                                // any pending drag intents are now
                                // resolved one way or the other.
                                pending_inv_actions.clear();
                            }
                        }
                        "vx.mob" => {
                            if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                                MobMsg,
                                _,
                            >(&data, cfg)
                            {
                                mob_updates.push(m);
                            }
                        }
                        "vx.mob_event" => {
                            if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                                MobEventMsg,
                                _,
                            >(&data, cfg)
                            {
                                mob_events.push(m);
                            }
                        }
                        "vx.mob_remove" => {
                            if let Ok((id, _)) =
                                bincode::serde::decode_from_slice::<u32, _>(&data, cfg)
                            {
                                mob_removals.push(id);
                            }
                        }
                        "vx.player_remove" => {
                            if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                                PlayerRemoveMsg,
                                _,
                            >(&data, cfg)
                            {
                                player_removals.push(m.id);
                            }
                        }
                        _ => {}
                    }
                }
                // Re-send pending edits / inventory drags on a
                // throttled schedule. Server-applied edits are
                // idempotent (set+broadcast), so duplicate arrivals
                // are correctness-safe, but we don't need them every
                // single tick - `RETRY_PERIOD` gates the resend.
                if do_retry {
                    for e in pending_edits.iter() {
                        if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                            let _ = client.send_bytes("vx.edit", buf);
                        }
                    }
                    for a in pending_inv_actions.iter() {
                        if let Ok(buf) = bincode::serde::encode_to_vec(a, cfg) {
                            let _ = client.send_bytes("vx.inv_act", buf);
                        }
                    }
                }
                // Send own pose upstream.
                let me = PlayerMsg {
                    id: my_id.unwrap_or(0),
                    pos: [player_pos.x, player_pos.y, player_pos.z],
                    yaw: player_yaw,
                    pitch: player_pitch,
                };
                if let Ok(buf) = bincode::serde::encode_to_vec(me, cfg) {
                    let _ = client.send_bytes("vx.player", buf);
                }
            }
        }
        // Apply collected edits after the borrow on `self.net`
        // releases - both paths funnel through here so the
        // chunk-rebuild logic only lives in one place.
        for e in to_apply {
            let voxel = [e.x, e.y, e.z];
            self.world_set_tracked(voxel, BlockId(e.block));
            let coords = Self::affected_chunks(voxel);
            self.rebuild_chunks(&coords);
        }
        // Authoritative inventory snapshot from the server: replace
        // the local view wholesale. Anything the client mispredicted
        // (drag intent the server rejected, pickup the server
        // assigned to someone else) reconciles on this single
        // assignment.
        if let Some(snap) = client_snapshot {
            for (i, s) in snap.inventory.iter().enumerate() {
                if i < self.inventory.len() {
                    self.inventory[i] = s.map(|(b, c)| (BlockId(b), c));
                }
            }
            for (i, s) in snap.craft.iter().enumerate() {
                if i < self.craft_grid.len() {
                    self.craft_grid[i] = s.map(|(b, c)| (BlockId(b), c));
                }
            }
        }
        for m in mob_updates {
            self.remote_mobs.insert(m.id, m);
        }
        // Drop departed players from the client-side remote table
        // so their cubes stop rendering as soon as the server
        // tells us they left.
        if !player_removals.is_empty()
            && let NetRole::Client { remote, .. } = &mut self.net
        {
            for id in &player_removals {
                remote.remove(id);
            }
        }
        // Apply mob hits requested by clients. Server validates
        // reach against the sender's last known pose before
        // routing into the authoritative damage path.
        for (cid, mob_id) in mob_hits_to_apply {
            let sender_pose = match &self.net {
                NetRole::Server { poses, .. } => poses.get(&cid).copied(),
                _ => None,
            };
            let mob_pos = self.remote_mobs.get(&mob_id).map(|m| m.pos);
            let ok = match (sender_pose, mob_pos) {
                (Some(p), Some(mp)) => {
                    let dx = p.pos[0] - mp[0];
                    let dy = p.pos[1] - mp[1];
                    let dz = p.pos[2] - mp[2];
                    dx * dx + dy * dy + dz * dz
                        <= (MOB_ATTACK_REACH + 1.0).powi(2)
                }
                _ => false,
            };
            if ok {
                self.damage_mob(mob_id, Some(cid));
            }
        }
        // Client-side: apply incoming mob events (visual flash +
        // death sound). The server doesn't run this branch since
        // it already played them when authoritative damage fired.
        for evt in mob_events {
            self.mob_hurt_flash.insert(evt.id, 0.30);
            if let Some(mob) = self.remote_mobs.get(&evt.id).copied() {
                let pos = mob.pos;
                let say_freq = if evt.kind == 1 { 200.0 } else { 280.0 };
                self.sfx_at("mob/sheep/say", pos, move |a, g| {
                    let _ = a.play_beep(say_freq, 0.16, 0.20 * g);
                });
            }
        }
        // Removals: clean up local mob bookkeeping mirroring what
        // damage_mob does server-side.
        for id in mob_removals {
            self.remote_mobs.remove(&id);
            self.mob_hurt_flash.remove(&id);
            self.mob_step_timers.remove(&id);
            self.mob_say_timers.remove(&id);
            self.mob_last_xz.remove(&id);
        }
        // Periodic state flush on the server so a crash loses at
        // most ~30 seconds of edits.
        if matches!(self.net, NetRole::Server { .. })
            && self
                .last_state_save
                .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(30))
        {
            self.save_server_state();
        }
    }

    fn new() -> Self {
        let mut registry = BlockRegistry::new();
        // Drive the runtime block registry from the BLOCK_TYPES
        // table so adding a new block kind is a one-place change.
        for bt in BLOCK_TYPES {
            let def = if bt.transparent {
                BlockDef::transparent(bt.name, bt.color)
            } else {
                BlockDef::solid(bt.name, bt.color)
            };
            registry.insert(BlockId(bt.id), def);
        }
        Self {
            state: AppState::MainMenu,
            settings: Settings::default(),
            requested_quit: false,
            renderer: None,
            ui: None,
            registry,
            world: World::new(),
            chunk_meshes: Vec::new(),
            pending_chunks: VecDeque::new(),
            pending_set: HashSet::new(),
            workers: None,
            target: None,
            player_pos: Vec3::new(0.0, 32.0, 0.0),
            player_vel: Vec3::ZERO,
            player_yaw: 0.0,
            player_pitch: -0.4,
            has_focus: true,
            last_mouse: None,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            esc_prev: false,
            e_prev: false,
            lmb_pending: false,
            rmb_pending: false,
            inventory: {
                // Seed the hotbar with one starter stack per known
                // block type so the demo is playable from the first
                // frame. Slot 0 stays empty - that's the "break /
                // empty hand" tool.
                let mut inv: [Option<(BlockId, u8)>; INVENTORY_SLOTS] = [None; INVENTORY_SLOTS];
                for (i, bt) in BLOCK_TYPES.iter().enumerate() {
                    let slot = i + 1;
                    if slot < HOTBAR_SLOTS {
                        inv[slot] = Some((BlockId(bt.id), STACK_MAX));
                    }
                }
                inv
            },
            selected_slot: 0,
            inventory_open: false,
            mp_menu_open: false,
            craft_grid: [None; 4],
            wheel_accum: 0.0,
            grounded: false,
            viewport: (1, 1),
            fps_ema: 60.0,
            // Start near mid-morning so the demo opens on a bright
            // sky rather than at sunrise.
            day_time: DAY_LENGTH_S * 0.3,
            net: NetRole::Offline,
            player_cube: None,
            sheep_mesh: None,
            sheep_head_mesh: None,
            voxel_atlas: None,
            atlas_rgba: None,
            egui_atlas: None,
            texture_dir: None,
            particles: {
                // Particle system starts with one "ambient" emitter
                // (rate=0, max=0) that only contributes its gravity
                // + interpolation curves to direct-burst particles.
                let mut s = infengine_particles::ParticleSystem::new();
                s.add_emitter(infengine_particles::Emitter {
                    rate: 0.0,
                    max_particles: 0,
                    gravity: [0.0, -10.0, 0.0],
                    color_start: [0.75, 0.55, 0.30, 1.0],
                    color_end: [0.50, 0.32, 0.18, 0.0],
                    size_start: 0.12,
                    size_end: 0.04,
                    lifetime: 0.5,
                    ..Default::default()
                });
                s
            },
            edit_history: Vec::new(),
            known_client_ids: HashSet::new(),
            last_state_save: None,
            saved_inventories: HashMap::new(),
            net_tick: 0,
            audio: None,
            audio_bank: infengine_audio::AudioBank::new(),
            sounds_dir: None,
            step_cooldown: 0.0,
            show_debug: false,
            f3_prev: false,
            torch_positions: HashSet::new(),
            mobs: Vec::new(),
            remote_mobs: HashMap::new(),
            last_mob_spawn_time: 0.0,
            next_mob_id: 1,
            mob_step_timers: HashMap::new(),
            mob_say_timers: HashMap::new(),
            mob_last_xz: HashMap::new(),
            mob_hurt_flash: HashMap::new(),
            master_addr: format!(
                "91.232.214.139:{}",
                infengine_net::master::MASTER_PORT
            ),
            direct_addr_buf: "127.0.0.1:7777".into(),
            host_name_buf: "My InfEngine Server".into(),
            host_port_buf: "7777".into(),
            host_announce: true,
            browser_servers: Vec::new(),
            browser_error: None,
            browser_last_refresh: None,
            browser_query_rx: None,
            announce_query_rx: None,
            master_token: None,
            last_heartbeat: None,
        }
    }

    /// Synchronously generate the initial chunk grid so the player
    /// drops into a non-empty world. Streaming workers take over once
    /// the player starts moving.
    fn build_world(&mut self) {
        for cz in -INITIAL_CHUNKS..=INITIAL_CHUNKS {
            for cx in -INITIAL_CHUNKS..=INITIAL_CHUNKS {
                for cy in 0..VERTICAL_CHUNKS {
                    let cc: ChunkCoord = [cx, cy, cz];
                    self.world.insert_chunk(cc, generate_chunk_at(cc));
                }
            }
        }
        self.rebuild_all_meshes();
        info!(
            chunks = self.world.iter_chunks().count(),
            submeshes = self.chunk_meshes.len(),
            "initial world built"
        );
    }

    /// Streaming step: queue missing chunks within `VIEW_DISTANCE`
    /// of the player, unload chunks beyond `UNLOAD_DISTANCE`, then
    /// process up to `CHUNKS_PER_FRAME` queued chunks this frame.
    fn stream_chunks(&mut self) {
        let cs = CHUNK_SIZE as i32;
        let player_chunk = [
            (self.player_pos.x.floor() as i32).div_euclid(cs),
            0,
            (self.player_pos.z.floor() as i32).div_euclid(cs),
        ];

        // Queue missing chunks inside the view radius. Vertical
        // column always loaded (cy 0..VERTICAL_CHUNKS) so mountain
        // peaks + sky builds never pop in.
        for dz in -VIEW_DISTANCE_CHUNKS..=VIEW_DISTANCE_CHUNKS {
            for dx in -VIEW_DISTANCE_CHUNKS..=VIEW_DISTANCE_CHUNKS {
                for cy in 0..VERTICAL_CHUNKS {
                    let cc = [player_chunk[0] + dx, cy, player_chunk[2] + dz];
                    if self.world.chunk(cc).is_none() && self.pending_set.insert(cc) {
                        self.pending_chunks.push_back(cc);
                    }
                }
            }
        }

        // Unload chunks beyond the keep radius. Collect first to
        // avoid borrowing `self.world` immutably while mutating.
        let to_unload: Vec<ChunkCoord> = self
            .world
            .iter_chunks()
            .map(|(c, _)| *c)
            .filter(|c| {
                let dx = (c[0] - player_chunk[0]).abs();
                let dz = (c[2] - player_chunk[2]).abs();
                dx > UNLOAD_DISTANCE_CHUNKS || dz > UNLOAD_DISTANCE_CHUNKS
            })
            .collect();
        if !to_unload.is_empty() {
            let renderer = self.renderer.as_mut().expect("renderer ready");
            let mut keep = Vec::with_capacity(self.chunk_meshes.len());
            for entry in self.chunk_meshes.drain(..) {
                if to_unload.contains(&entry.0) {
                    renderer.remove_mesh(entry.2);
                } else {
                    keep.push(entry);
                }
            }
            self.chunk_meshes = keep;
            for cc in &to_unload {
                self.world.remove_chunk(*cc);
            }
        }

        // Dispatch pending chunks to the worker pool. The request
        // channel is unbounded; workers consume at their own rate.
        if let Some(pool) = self.workers.as_ref() {
            while let Some(cc) = self.pending_chunks.pop_front() {
                if pool.req_tx.send(cc).is_err() {
                    self.pending_chunks.push_front(cc);
                    break;
                }
            }
        }

        // Drain up to RESULTS_PER_FRAME finished chunks from the
        // workers. Each result: insert chunk + register meshes + remesh
        // the 4 horizontal neighbours so face culling at boundaries
        // stays correct.
        let mut delivered: Vec<WorkerResult> = Vec::new();
        if let Some(pool) = self.workers.as_ref() {
            while delivered.len() < RESULTS_PER_FRAME
                && let Ok(r) = pool.res_rx.try_recv()
            {
                delivered.push(r);
            }
        }
        for result in delivered {
            self.pending_set.remove(&result.coord);
            // The chunk might have been unloaded above (player walked
            // past it while the worker was busy). Drop stale data.
            let dx = (result.coord[0] - player_chunk[0]).abs();
            let dz = (result.coord[2] - player_chunk[2]).abs();
            if dx > UNLOAD_DISTANCE_CHUNKS || dz > UNLOAD_DISTANCE_CHUNKS {
                continue;
            }
            if self.world.chunk(result.coord).is_some() {
                continue;
            }
            self.world.insert_chunk(result.coord, result.chunk);
            // Use the worker's submeshes directly for this chunk so
            // the parallel meshing work isn't thrown away. Boundary
            // faces between this chunk and an already-loaded
            // neighbour stay emitted (worker treated outside as air),
            // but they sit at the boundary plane between two solid
            // voxels and are never visible.
            let cc = result.coord;
            {
                let renderer = self.renderer.as_mut().expect("renderer ready");
                let atlas = self.voxel_atlas.clone();
                for sub in result.submeshes {
                    let handle = renderer.create_mesh(
                        &format!("voxel_chunk_{}_{}_{}_b{}", cc[0], cc[1], cc[2], sub.block.0),
                        &sub.vertices,
                        &sub.indices,
                    );
                    if let Some(a) = atlas.clone() {
                        renderer.bind_mesh_albedo_arc(handle, a);
                    }
                    self.chunk_meshes.push((cc, sub.block, handle));
                }
            }
            // Re-mesh the 4 horizontal neighbours so their boundary
            // faces facing the freshly-inserted chunk get hidden.
            let neighbours = [
                [cc[0] - 1, cc[1], cc[2]],
                [cc[0] + 1, cc[1], cc[2]],
                [cc[0], cc[1], cc[2] - 1],
                [cc[0], cc[1], cc[2] + 1],
            ];
            self.rebuild_chunks(&neighbours);
        }
    }

    fn affected_chunks(world_voxel: [i32; 3]) -> Vec<ChunkCoord> {
        // The edited voxel always belongs to one chunk. If it sits on
        // a chunk boundary (local 0 or CHUNK_SIZE-1 on any axis) the
        // neighbour chunk's faces facing it become visible/hidden too,
        // so we rebuild that chunk as well.
        let cs = CHUNK_SIZE as i32;
        let cc = [
            world_voxel[0].div_euclid(cs),
            world_voxel[1].div_euclid(cs),
            world_voxel[2].div_euclid(cs),
        ];
        let lc = [
            world_voxel[0].rem_euclid(cs),
            world_voxel[1].rem_euclid(cs),
            world_voxel[2].rem_euclid(cs),
        ];
        let mut out = vec![cc];
        if lc[0] == 0 {
            out.push([cc[0] - 1, cc[1], cc[2]]);
        }
        if lc[0] == cs - 1 {
            out.push([cc[0] + 1, cc[1], cc[2]]);
        }
        if lc[1] == 0 {
            out.push([cc[0], cc[1] - 1, cc[2]]);
        }
        if lc[1] == cs - 1 {
            out.push([cc[0], cc[1] + 1, cc[2]]);
        }
        if lc[2] == 0 {
            out.push([cc[0], cc[1], cc[2] - 1]);
        }
        if lc[2] == cs - 1 {
            out.push([cc[0], cc[1], cc[2] + 1]);
        }
        out
    }

    fn rebuild_chunks(&mut self, coords: &[ChunkCoord]) {
        let renderer = self.renderer.as_mut().expect("renderer ready");
        // Release old mesh handles for the affected chunks so the
        // mesh registry doesn't accumulate stale buffers across edits.
        let mut keep = Vec::with_capacity(self.chunk_meshes.len());
        for entry in self.chunk_meshes.drain(..) {
            if coords.contains(&entry.0) {
                renderer.remove_mesh(entry.2);
            } else {
                keep.push(entry);
            }
        }
        self.chunk_meshes = keep;
        let atlas = self.voxel_atlas.clone();
        for coord in coords {
            let Some(chunk) = self.world.chunk(*coord) else {
                continue;
            };
            let origin = World::chunk_world_origin(*coord);
            for sub in chunk_mesh_with_atlas(chunk, origin, atlas_uv_rect, block_is_opaque) {
                let handle = renderer.create_mesh(
                    &format!(
                        "voxel_chunk_{}_{}_{}_b{}",
                        coord[0], coord[1], coord[2], sub.block.0
                    ),
                    &sub.vertices,
                    &sub.indices,
                );
                if let Some(a) = atlas.clone() {
                    renderer.bind_mesh_albedo_arc(handle, a);
                }
                self.chunk_meshes.push((*coord, sub.block, handle));
            }
        }
    }

    /// Walk every chunk in the world and register its mesh handles.
    /// Used at startup. Edits use `rebuild_chunks` for the affected
    /// chunk + its solid neighbours so the cost stays local.
    fn rebuild_all_meshes(&mut self) {
        self.chunk_meshes.clear();
        let atlas = self.voxel_atlas.clone();
        let renderer = self.renderer.as_mut().expect("renderer ready");
        let coords: Vec<ChunkCoord> = self.world.iter_chunks().map(|(c, _)| *c).collect();
        for coord in coords {
            let chunk = self.world.chunk(coord).expect("chunk present");
            let origin = World::chunk_world_origin(coord);
            let subs = chunk_mesh_with_atlas(chunk, origin, atlas_uv_rect, block_is_opaque);
            for sub in subs {
                let handle = renderer.create_mesh(
                    &format!(
                        "voxel_chunk_{}_{}_{}_b{}",
                        coord[0], coord[1], coord[2], sub.block.0
                    ),
                    &sub.vertices,
                    &sub.indices,
                );
                if let Some(a) = atlas.clone() {
                    renderer.bind_mesh_albedo_arc(handle, a);
                }
                self.chunk_meshes.push((coord, sub.block, handle));
            }
        }
    }

    fn apply_cursor_settings(&self, window: &EngineWindow) {
        use infengine_window::raw_winit::window::CursorGrabMode;
        let playing = matches!(self.state, AppState::Playing);
        let raw = window.raw();
        // Only grab when we have OS focus. Trying to grab an
        // unfocused window on Windows silently drops the lock and
        // can teleport the cursor onto a sibling window mid-click.
        // Inventory overlay needs a usable cursor + breaks the
        // lock so the mouse can leave the window without being
        // snapped back to the centre.
        let active = playing
            && self.has_focus
            && !self.inventory_open
            && !self.mp_menu_open;
        let grab = if active && self.settings.lock_cursor {
            if self.settings.hide_cursor {
                CursorGrabMode::Locked
            } else {
                CursorGrabMode::Confined
            }
        } else {
            CursorGrabMode::None
        };
        if raw.set_cursor_grab(grab).is_err() && grab == CursorGrabMode::Locked {
            let _ = raw.set_cursor_grab(CursorGrabMode::Confined);
        }
        raw.set_cursor_visible(!(active && self.settings.hide_cursor));
    }

    fn update_play(&mut self, ctx: &FrameContext<'_>, dt: f32) {
        // While the inventory is open we still tick physics (so the
        // player keeps falling / settling on the ground + the server
        // sees a live character) but freeze all player-driven inputs:
        // mouse-look, WASD, jump, hotbar wheel. This matches the
        // Minecraft convention - opening the inventory in multiplayer
        // never pauses the world for anyone.
        let input_active = !self.inventory_open && !self.mp_menu_open;

        if input_active {
            let dx = std::mem::take(&mut self.mouse_dx);
            let dy = std::mem::take(&mut self.mouse_dy);
            let sens = self.settings.mouse_sensitivity * 0.0035;
            self.player_yaw -= dx * sens;
            self.player_pitch -= dy * sens;
            let limit = std::f32::consts::FRAC_PI_2 - 0.05;
            self.player_pitch = self.player_pitch.clamp(-limit, limit);

            // Consume mouse wheel ticks - cycle hotbar slots (wheel up =
            // previous slot, matches Minecraft convention).
            let wheel = std::mem::take(&mut self.wheel_accum);
            let ticks = wheel.round() as i32;
            if ticks != 0 {
                let mut slot = self.selected_slot as i32 - ticks;
                slot = slot.rem_euclid(HOTBAR_SLOTS as i32);
                self.selected_slot = slot as usize;
            }
        } else {
            // Discard captured deltas so they don't snap the camera
            // when the menu closes.
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
            self.wheel_accum = 0.0;
        }

        let forward_horizontal =
            Vec3::new(self.player_yaw.cos(), 0.0, -self.player_yaw.sin()).normalize();
        // Right vector via the standard 90-deg horizontal rotation of
        // forward. For yaw=0 (forward = +X), right = +Z which matches
        // a player facing east strafing south.
        let right = Vec3::new(-forward_horizontal.z, 0.0, forward_horizontal.x);

        let mut move_axis = 0.0;
        let mut strafe = 0.0;
        // Movement speed multiplier: Shift = sneak (0.4x), Ctrl =
        // sprint (1.7x), no modifier = normal walk. Modifiers are
        // also gated on `input_active` so the menu doesn't sprint
        // the player while idle.
        let mut speed_mul = 1.0_f32;
        if input_active {
            if ctx.input.is_pressed(Key::W) {
                move_axis += 1.0;
            }
            if ctx.input.is_pressed(Key::S) {
                move_axis -= 1.0;
            }
            if ctx.input.is_pressed(Key::A) {
                strafe -= 1.0;
            }
            if ctx.input.is_pressed(Key::D) {
                strafe += 1.0;
            }
            if ctx.input.is_pressed(Key::ShiftLeft)
                || ctx.input.is_pressed(Key::ShiftRight)
            {
                speed_mul *= 0.4;
            }
            if ctx.input.is_pressed(Key::ControlLeft)
                || ctx.input.is_pressed(Key::ControlRight)
            {
                speed_mul *= 1.7;
            }
        }
        let horizontal = (forward_horizontal * move_axis + right * strafe)
            .normalize_or_zero()
            * (PLAYER_SPEED * speed_mul);
        self.player_vel.x = horizontal.x;
        self.player_vel.z = horizontal.z;

        if input_active && ctx.input.is_pressed(Key::Space) && self.grounded {
            self.player_vel.y = JUMP_VELOCITY;
            self.grounded = false;
        }
        self.player_vel.y += GRAVITY * dt;

        // Footstep audio: fire a short click every ~0.4 s of
        // grounded horizontal motion. Sprint shortens the cadence
        // proportionally so it stays in sync with the leg.
        let speed = (self.player_vel.x * self.player_vel.x
            + self.player_vel.z * self.player_vel.z)
            .sqrt();
        if input_active && self.grounded && speed > 0.5 {
            self.step_cooldown -= dt * (speed / PLAYER_SPEED);
            if self.step_cooldown <= 0.0 {
                const STEP_PERIOD: f32 = 0.42;
                self.step_cooldown = STEP_PERIOD;
                // Sample the voxel just below the player's feet so
                // the footstep sound matches the surface they're
                // actually standing on.
                let foot = [
                    self.player_pos.x.floor() as i32,
                    (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                    self.player_pos.z.floor() as i32,
                ];
                let under = self.world.get(foot);
                let mat = block_material(under);
                let key = format!("step:{mat}");
                let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
                self.sfx_at(&key, pos, |a, g| {
                    let _ = a.play_swish(0.10 * g);
                });
            }
        } else {
            // Reset cooldown so a step doesn't fire instantly the
            // moment we re-enter motion (keeps cadence natural).
            self.step_cooldown = 0.0;
        }

        // Axis-resolved voxel AABB collision.
        let delta = self.player_vel * dt;
        let (new_pos, hit_floor, hit_ceiling) = sweep_aabb(&self.world, self.player_pos, delta);
        self.player_pos = new_pos;
        if hit_floor {
            self.player_vel.y = 0.0;
            self.grounded = true;
        } else {
            self.grounded = false;
        }
        if hit_ceiling {
            self.player_vel.y = 0.0;
        }

        if self.player_pos.y < -20.0 {
            // Fell through the world - respawn above origin. Acts
            // as a soft "death": storage rows wipe so falling has
            // real cost, hotbar is kept so the player isn't reduced
            // to bare hands. Locally we always wipe (optimistic);
            // in MP client mode we also fire a `vx.fellout` so the
            // server's authoritative inventory mirrors the penalty.
            tracing::warn!(
                pos = ?[self.player_pos.x, self.player_pos.y, self.player_pos.z],
                "player fell through the world - respawning"
            );
            self.player_pos = Vec3::new(0.0, 32.0, 0.0);
            self.player_vel = Vec3::ZERO;
            let mut dropped = 0usize;
            for slot in self.inventory[HOTBAR_SLOTS..].iter_mut() {
                if slot.take().is_some() {
                    dropped += 1;
                }
            }
            for slot in self.craft_grid.iter_mut() {
                if slot.take().is_some() {
                    dropped += 1;
                }
            }
            if dropped > 0 {
                tracing::info!(dropped, "fall-out penalty: cleared storage stacks");
                self.spawn_break_particles([0, 32, 0]);
            }
            if let NetRole::Client { client, .. } = &self.net {
                let _ = client.send_bytes("vx.fellout", Vec::new());
            }
        }
    }

    fn render_ui(&mut self, window: &EngineWindow) {
        let state = self.state;
        let fps = self.fps_ema;
        let show_fps = self.settings.show_fps;
        let day_time = self.day_time;
        let show_debug = self.show_debug;
        let dbg_player = self.player_pos;
        let dbg_yaw = self.player_yaw;
        let dbg_pitch = self.player_pitch;
        let dbg_chunks = self.chunk_meshes.len();
        let dbg_grounded = self.grounded;
        let dbg_net = match &self.net {
            NetRole::Offline => "offline".to_string(),
            NetRole::Server { server, poses, .. } => {
                format!(
                    "server (cli={}, poses={})",
                    server.client_count(),
                    poses.len(),
                )
            }
            NetRole::Client {
                client,
                pending_edits,
                pending_inv_actions,
                ..
            } => format!(
                "client (id={:?}, edits={}, inv_acts={})",
                client.client_id,
                pending_edits.len(),
                pending_inv_actions.len(),
            ),
        };
        let settings_edit = &mut self.settings;
        // Lazy-load the egui atlas texture on first render after
        // on_start has populated the pixel cache. We use NEAREST
        // sampling so the 16x16 tiles stay crisp at slot size.
        if self.egui_atlas.is_none()
            && let (Some(ui), Some((aw, ah, rgba))) = (self.ui.as_ref(), self.atlas_rgba.as_ref())
        {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [*aw as usize, *ah as usize],
                rgba,
            );
            let handle = ui.context().load_texture(
                "voxel_atlas_egui",
                img,
                egui::TextureOptions::NEAREST,
            );
            self.egui_atlas = Some(handle);
        }
        let atlas_tex_id: Option<egui::TextureId> =
            self.egui_atlas.as_ref().map(|h| h.id());
        // Snapshot tuple: (color, count, uv_rect_for_top_face). UV
        // is only used when the egui atlas texture is loaded;
        // otherwise we fall back to a solid color swatch.
        let icon = |slot: Option<(BlockId, u8)>| -> Option<SlotIcon> {
            slot.and_then(|(b, c)| {
                let bt = block_type(b.0);
                self.registry
                    .get(b)
                    .map(|d| (d.color, c, atlas_uv_rect(b, [0, 1, 0]), bt.map(|t| t.name).unwrap_or("?")))
            })
        };
        let hotbar_snapshot: Vec<Option<SlotIcon>> = self.inventory
            [..HOTBAR_SLOTS]
            .iter()
            .map(|s| icon(*s))
            .collect();
        let inventory_open = self.inventory_open;
        let mp_menu_open = self.mp_menu_open;
        let mp_role_label: Option<&'static str> = match &self.net {
            NetRole::Offline => None,
            NetRole::Server { .. } => Some("hosting server"),
            NetRole::Client { .. } => Some("connected to server"),
        };
        let inventory_snapshot: Vec<Option<SlotIcon>> = if inventory_open {
            self.inventory[HOTBAR_SLOTS..]
                .iter()
                .map(|s| icon(*s))
                .collect()
        } else {
            Vec::new()
        };
        let craft_snapshot: [Option<SlotIcon>; 4] =
            std::array::from_fn(|i| icon(self.craft_grid[i]));
        let craft_match: Option<(BlockId, u8, &'static str)> =
            match_recipe_named(&self.craft_grid);
        let craft_output_snapshot = craft_match.and_then(|(b, c, _)| {
            let bt = block_type(b.0);
            self.registry.get(b).map(|d| {
                (
                    d.color,
                    c,
                    atlas_uv_rect(b, [0, 1, 0]),
                    bt.map(|t| t.name).unwrap_or("?"),
                )
            })
        });
        let craft_recipe_name: Option<&'static str> = craft_match.map(|(_, _, n)| n);
        let selected_slot = self.selected_slot;
        // Click actions to apply after the egui closure exits (the
        // closure can't mutate `self` directly because the renderer
        // already borrowed it).
        let mut inv_actions: Vec<InvAction> = Vec::new();
        let mut action_start = false;
        let mut action_resume = false;
        let mut action_quit = false;
        let mut action_menu_to_settings = false;
        let mut action_pause_to_settings = false;
        let mut action_back_settings = false;
        let mut action_main_menu = false;
        let mut action_to_direct = false;
        let mut action_to_browser = false;
        let mut action_to_host = false;
        let mut action_join_direct = false;
        let mut action_join_browser: Option<String> = None;
        let mut action_browser_refresh = false;
        let mut action_host_start = false;
        // MP overlay actions: continue playing (close overlay) /
        // explicit disconnect-and-return-to-main-menu.
        let mut action_mp_resume = false;
        let mut action_mp_disconnect = false;
        let mut action_mp_quit = false;
        let direct_buf = &mut self.direct_addr_buf;
        let host_name = &mut self.host_name_buf;
        let host_port = &mut self.host_port_buf;
        let host_announce = &mut self.host_announce;
        let master_addr = &mut self.master_addr;
        let browser_servers = &self.browser_servers;
        let browser_error = &self.browser_error;
        let browser_last_refresh = self.browser_last_refresh;
        let browser_querying = self.browser_query_rx.is_some();

        let Some(ui) = self.ui.as_mut() else {
            return;
        };
        ui.run(window.raw(), |ctx| match state {
            AppState::MainMenu => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.heading("InfEngine - Voxel Demo");
                        ui.add_space(20.0);
                        if ui.button("Play offline").clicked() {
                            action_start = true;
                        }
                        if ui.button("Host server...").clicked() {
                            action_to_host = true;
                        }
                        if ui.button("Direct connect...").clicked() {
                            action_to_direct = true;
                        }
                        if ui.button("Browse servers...").clicked() {
                            action_to_browser = true;
                        }
                        ui.add_space(10.0);
                        if ui.button("Settings").clicked() {
                            action_menu_to_settings = true;
                        }
                        if ui.button("Quit").clicked() {
                            action_quit = true;
                        }
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.label("Master server:");
                            ui.text_edit_singleline(master_addr);
                        });
                    });
                });
            }
            AppState::DirectConnect => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading("Direct connect");
                        ui.add_space(10.0);
                        ui.label("Enter host:port of the game server:");
                        ui.text_edit_singleline(direct_buf);
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Join").clicked() {
                                action_join_direct = true;
                            }
                            if ui.button("Back").clicked() {
                                action_main_menu = true;
                            }
                        });
                    });
                });
            }
            AppState::ServerBrowser => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Server browser");
                    ui.horizontal(|ui| {
                        ui.label("Master:");
                        ui.text_edit_singleline(master_addr);
                        if ui.button("Refresh").clicked() {
                            action_browser_refresh = true;
                        }
                        if ui.button("Back").clicked() {
                            action_main_menu = true;
                        }
                    });
                    // Diagnostic status banner so the user knows
                    // whether the last query reached the master.
                    ui.horizontal(|ui| {
                        let age = browser_last_refresh
                            .map(|t| format!("{:.0}s ago", t.elapsed().as_secs_f32()));
                        let status = if browser_querying {
                            "[...] querying master".to_string()
                        } else {
                            match (browser_error.as_deref(), age.as_deref()) {
                                (Some(err), _) => format!("[ERR] {err}"),
                                (None, Some(a)) => format!(
                                    "[OK] {} server(s) - refreshed {a}",
                                    browser_servers.len()
                                ),
                                (None, None) => "[idle] press Refresh".to_string(),
                            }
                        };
                        let color = if browser_querying {
                            egui::Color32::from_rgb(140, 180, 220)
                        } else if browser_error.is_some() {
                            egui::Color32::from_rgb(220, 100, 100)
                        } else if browser_servers.is_empty() {
                            egui::Color32::from_rgb(220, 200, 100)
                        } else {
                            egui::Color32::from_rgb(140, 220, 140)
                        };
                        ui.colored_label(color, status);
                    });
                    ui.separator();
                    if browser_servers.is_empty() && browser_error.is_none() {
                        ui.label(
                            "(no servers - host one from the menu or wait for someone to announce)"
                        );
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for s in browser_servers {
                            ui.horizontal(|ui| {
                                if ui.button("Join").clicked() {
                                    action_join_browser = Some(s.addr.clone());
                                }
                                ui.label(format!(
                                    "{}  -  {} ({}/{}{})",
                                    s.addr,
                                    s.name,
                                    s.players,
                                    s.max_players,
                                    if s.mode.is_empty() {
                                        String::new()
                                    } else {
                                        format!(", {}", s.mode)
                                    }
                                ));
                            });
                        }
                    });
                });
            }
            AppState::HostSetup => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.heading("Host server");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label("Display name:");
                            ui.text_edit_singleline(host_name);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Bind port:");
                            ui.text_edit_singleline(host_port);
                        });
                        ui.checkbox(host_announce, "Announce on master server");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Start hosting").clicked() {
                                action_host_start = true;
                            }
                            if ui.button("Back").clicked() {
                                action_main_menu = true;
                            }
                        });
                    });
                });
            }
            AppState::Paused => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading("Paused");
                        ui.add_space(20.0);
                        if ui.button("Resume").clicked() {
                            action_resume = true;
                        }
                        if ui.button("Settings").clicked() {
                            action_pause_to_settings = true;
                        }
                        if ui.button("Main Menu").clicked() {
                            action_main_menu = true;
                        }
                    });
                });
            }
            AppState::Settings { .. } => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.add(
                        egui::Slider::new(&mut settings_edit.mouse_sensitivity, 0.1..=2.0)
                            .text("Mouse sensitivity"),
                    );
                    ui.checkbox(&mut settings_edit.show_fps, "Show FPS");
                    ui.checkbox(
                        &mut settings_edit.lock_cursor,
                        "Lock cursor in window (Play)",
                    );
                    ui.checkbox(&mut settings_edit.hide_cursor, "Hide cursor (Play)");
                    ui.separator();
                    if ui.button("Back").clicked() {
                        action_back_settings = true;
                    }
                });
            }
            AppState::Playing => {
                egui::Area::new(egui::Id::new("voxel_hud"))
                    .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label("Mouse aim, WASD move, Space jump");
                            ui.label("Shift sneak, Ctrl sprint");
                            ui.label("LMB break, RMB place, E inventory, Esc menu");
                            // Day clock - map [0..DAY_LENGTH_S) onto
                            // 24h so the player has a sense of where
                            // the sun is.
                            let frac = (day_time / DAY_LENGTH_S).rem_euclid(1.0);
                            let hours_total = frac * 24.0;
                            let hh = hours_total.floor() as i32;
                            let mm = ((hours_total - hh as f32) * 60.0) as i32;
                            ui.label(format!("Time: {hh:02}:{mm:02}"));
                            if show_fps {
                                ui.separator();
                                ui.label(format!("FPS: {fps:.0}"));
                            }
                        });
                    });
                // Multiplayer in-game menu. Opens on Esc when a
                // net role is active. World keeps simulating
                // underneath (other peers must not see a frozen
                // ghost), so this is a thin overlay rather than a
                // pause state.
                if mp_menu_open
                    && let Some(role) = mp_role_label
                {
                    egui::Window::new("Game menu")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(format!("Status: {role}"));
                            ui.separator();
                            if ui
                                .add_sized([220.0, 32.0], egui::Button::new("Resume"))
                                .clicked()
                            {
                                action_mp_resume = true;
                            }
                            if ui
                                .add_sized(
                                    [220.0, 32.0],
                                    egui::Button::new("Disconnect to main menu"),
                                )
                                .clicked()
                            {
                                action_mp_disconnect = true;
                            }
                            if ui
                                .add_sized([220.0, 32.0], egui::Button::new("Quit"))
                                .clicked()
                            {
                                action_mp_quit = true;
                            }
                        });
                }
                // F3 debug panel: top-right corner, lots of state in
                // one place. Mirrors Minecraft's debug overlay.
                if show_debug {
                    egui::Area::new(egui::Id::new("voxel_debug"))
                        .anchor(egui::Align2::RIGHT_TOP, [-12.0, 12.0])
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.label("[F3] Debug");
                                ui.separator();
                                ui.label(format!(
                                    "pos: {:.2}, {:.2}, {:.2}",
                                    dbg_player.x, dbg_player.y, dbg_player.z
                                ));
                                ui.label(format!(
                                    "look: yaw {:.2} pitch {:.2}",
                                    dbg_yaw, dbg_pitch
                                ));
                                ui.label(format!(
                                    "grounded: {}",
                                    if dbg_grounded { "yes" } else { "no" }
                                ));
                                ui.label(format!("chunk meshes: {dbg_chunks}"));
                                ui.label(format!("fps: {fps:.0}"));
                                ui.label(format!("net: {dbg_net}"));
                            });
                        });
                }
                // Bottom hotbar - empty slot (= break) + one slot per
                // registered block. Highlighted slot is the active
                // selection (driven by mouse wheel). Clicks here
                // change the active slot; drag&drop is wired through
                // the inventory window.
                egui::Area::new(egui::Id::new("voxel_hotbar"))
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (i, slot) in hotbar_snapshot.iter().enumerate() {
                                    let resp = slot_with_dnd(
                                        ui,
                                        *slot,
                                        atlas_tex_id,
                                        SlotKind::Inv(i),
                                        i == selected_slot,
                                        i == 0,
                                        inventory_open,
                                        &mut inv_actions,
                                    );
                                    if resp.clicked() {
                                        inv_actions.push(InvAction::SelectHotbar(i));
                                    }
                                }
                            });
                        });
                    });

                // Inventory overlay: 3 extra rows of slots + a hotbar
                // mirror + a 2x2 crafting grid + a 1-slot output.
                // Only drawn when the player has the menu open. The
                // Window's title bar is the only drag handle (egui
                // default behaviour with `.resizable(false)`).
                if inventory_open {
                    let screen = ctx.content_rect();
                    egui::Window::new("Inventory")
                        .collapsible(false)
                        .resizable(false)
                        .default_pos([
                            screen.center().x - 220.0,
                            screen.center().y - 220.0,
                        ])
                        .default_size([440.0, 420.0])
                        .show(ctx, |ui| {
                            ui.label("E to close - drag stacks between slots");
                            ui.separator();
                            // Storage rows (rows 1..INVENTORY_ROWS).
                            for row in 0..(INVENTORY_ROWS - 1) {
                                ui.horizontal(|ui| {
                                    for col in 0..INVENTORY_COLS {
                                        let snap_idx = row * INVENTORY_COLS + col;
                                        let inv_idx = HOTBAR_SLOTS + snap_idx;
                                        let slot = inventory_snapshot
                                            .get(snap_idx)
                                            .copied()
                                            .unwrap_or(None);
                                        slot_with_dnd(
                                            ui,
                                            slot,
                                            atlas_tex_id,
                                            SlotKind::Inv(inv_idx),
                                            false,
                                            false,
                                            true,
                                            &mut inv_actions,
                                        );
                                    }
                                });
                            }
                            ui.add_space(8.0);
                            // Hotbar row reflected inside the window
                            // so the player can shuffle stacks
                            // between storage and the hotbar without
                            // closing the menu (matches Minecraft).
                            ui.horizontal(|ui| {
                                for (i, slot) in hotbar_snapshot.iter().enumerate() {
                                    let resp = slot_with_dnd(
                                        ui,
                                        *slot,
                                        atlas_tex_id,
                                        SlotKind::Inv(i),
                                        i == selected_slot,
                                        i == 0,
                                        true,
                                        &mut inv_actions,
                                    );
                                    if resp.clicked() {
                                        inv_actions.push(InvAction::SelectHotbar(i));
                                    }
                                }
                            });
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label("Craft (2x2)");
                                    for row in 0..2 {
                                        ui.horizontal(|ui| {
                                            for col in 0..2 {
                                                let idx = row * 2 + col;
                                                slot_with_dnd(
                                                    ui,
                                                    craft_snapshot[idx],
                                                    atlas_tex_id,
                                                    SlotKind::Craft(idx),
                                                    false,
                                                    false,
                                                    true,
                                                    &mut inv_actions,
                                                );
                                            }
                                        });
                                    }
                                });
                                ui.vertical(|ui| {
                                    ui.add_space(20.0);
                                    ui.label(" ->");
                                });
                                ui.vertical(|ui| {
                                    ui.label("Output");
                                    let resp = slot_with_dnd(
                                        ui,
                                        craft_output_snapshot,
                                        atlas_tex_id,
                                        SlotKind::CraftOut,
                                        false,
                                        false,
                                        true,
                                        &mut inv_actions,
                                    );
                                    if let Some(name) = craft_recipe_name {
                                        resp.on_hover_text(name);
                                    }
                                });
                            });
                            // Carry preview is unnecessary - egui's
                            // dnd_drag_source paints the dragged
                            // stack at the cursor while held. The
                            // recipe book lives in its own window
                            // beside this one (see below) so it can
                            // grow / scroll without pushing the
                            // crafting grid off-screen.
                        });
                    // Recipe book as a separate beside-inventory
                    // window. Resizable + scrollable so it can fit
                    // the full recipe table even on a small display
                    // without cramping the Inventory layout.
                    let rb_default = [
                        screen.center().x + 230.0,
                        screen.center().y - 220.0,
                    ];
                    egui::Window::new("Recipe book")
                        .collapsible(true)
                        .resizable(true)
                        .default_pos(rb_default)
                        .default_size([320.0, 420.0])
                        .min_width(280.0)
                        .min_height(200.0)
                        .show(ctx, |ui| {
                            ui.label(
                                "Drop blocks into the 2x2 grid to craft. \
                                 Hold Shift for half-stacks, Ctrl/Cmd for one.",
                            );
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for r in RECIPES {
                                        ui.horizontal(|ui| {
                                            for (bid, n) in r.inputs {
                                                let bt = block_type(*bid);
                                                let icon = self
                                                    .registry
                                                    .get(BlockId(*bid))
                                                    .map(|d| {
                                                        (
                                                            d.color,
                                                            *n,
                                                            atlas_uv_rect(
                                                                BlockId(*bid),
                                                                [0, 1, 0],
                                                            ),
                                                            bt.map(|t| t.name)
                                                                .unwrap_or("?"),
                                                        )
                                                    });
                                                draw_slot_widget(
                                                    ui,
                                                    icon,
                                                    atlas_tex_id,
                                                    false,
                                                    false,
                                                    egui::Sense::hover(),
                                                );
                                            }
                                            ui.label(" -> ");
                                            let out_bt = block_type(r.output.0);
                                            let out_icon = self
                                                .registry
                                                .get(BlockId(r.output.0))
                                                .map(|d| {
                                                    (
                                                        d.color,
                                                        r.output.1,
                                                        atlas_uv_rect(
                                                            BlockId(r.output.0),
                                                            [0, 1, 0],
                                                        ),
                                                        out_bt
                                                            .map(|t| t.name)
                                                            .unwrap_or("?"),
                                                    )
                                                });
                                            draw_slot_widget(
                                                ui,
                                                out_icon,
                                                atlas_tex_id,
                                                false,
                                                false,
                                                egui::Sense::hover(),
                                            );
                                            ui.label(r.name);
                                        });
                                    }
                                });
                        });
                }

                // Crosshair drawn directly on the egui screen layer.
                // Two thin segments through the screen centre.
                let screen = ctx.content_rect();
                let center = screen.center();
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("voxel_crosshair"),
                ));
                let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(240, 240, 240));
                let half = 8.0_f32;
                painter.line_segment(
                    [
                        egui::pos2(center.x - half, center.y),
                        egui::pos2(center.x + half, center.y),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        egui::pos2(center.x, center.y - half),
                        egui::pos2(center.x, center.y + half),
                    ],
                    stroke,
                );
            }
        });

        // Apply inventory clicks captured during the egui pass. The
        // egui closure can't touch `self.inventory` directly (the
        // renderer already holds a `&mut self`), so we buffer the
        // intents and run them out here.
        //
        // Multiplayer client mode: every drag is forwarded to the
        // server as `vx.inv_act`. The local view is reconciled when
        // the server's authoritative snapshot lands. Hotbar selection
        // stays local for snappy feedback (it's purely cosmetic).
        let is_client = matches!(self.net, NetRole::Client { .. });
        for action in inv_actions {
            match action {
                InvAction::SelectHotbar(idx) => {
                    if idx < HOTBAR_SLOTS {
                        self.selected_slot = idx;
                    }
                }
                InvAction::Drag { from, to, split } => {
                    if is_client {
                        let msg = InvActionMsg {
                            from: from.into(),
                            to: to.into(),
                            split,
                        };
                        if let NetRole::Client {
                            client,
                            pending_inv_actions,
                            ..
                        } = &mut self.net
                        {
                            let cfg = bincode::config::standard();
                            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                                let _ = client.send_bytes("vx.inv_act", buf);
                            }
                            pending_inv_actions.push(msg);
                        }
                    } else {
                        self.apply_inv_action(InvAction::Drag { from, to, split });
                    }
                }
            }
        }

        if action_start {
            self.state = AppState::Playing;
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }
        if action_resume {
            self.state = AppState::Playing;
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }
        if action_quit {
            self.requested_quit = true;
        }
        if action_mp_resume {
            self.mp_menu_open = false;
        }
        if action_mp_disconnect {
            // Best-effort vx.leave so the server pre-empts its
            // 5s silence reaper + broadcasts the removal right
            // away. Then drop the net role + return to the main
            // menu so the user can rejoin / host again.
            self.notify_server_leaving();
            self.net = NetRole::Offline;
            self.mp_menu_open = false;
            self.state = AppState::MainMenu;
            self.remote_mobs.clear();
            self.mobs.clear();
        }
        if action_mp_quit {
            self.notify_server_leaving();
            self.requested_quit = true;
        }
        if action_menu_to_settings {
            self.state = AppState::Settings {
                back_to: BackTo::MainMenu,
            };
        }
        if action_pause_to_settings {
            self.state = AppState::Settings {
                back_to: BackTo::Pause,
            };
        }
        if action_back_settings && let AppState::Settings { back_to } = self.state {
            self.state = match back_to {
                BackTo::MainMenu => AppState::MainMenu,
                BackTo::Pause => AppState::Paused,
            };
        }
        if action_main_menu {
            self.state = AppState::MainMenu;
        }
        if action_to_direct {
            self.state = AppState::DirectConnect;
        }
        if action_to_browser {
            self.state = AppState::ServerBrowser;
            self.browser_last_refresh = None;
            // Auto-refresh on entering the browser so the user
            // immediately sees the current directory contents.
            self.refresh_browser();
        }
        if action_to_host {
            self.state = AppState::HostSetup;
        }
        if action_join_direct {
            let addr = self.direct_addr_buf.clone();
            self.connect_as_client(&addr);
        }
        if let Some(addr) = action_join_browser {
            self.connect_as_client(&addr);
        }
        if action_browser_refresh {
            self.refresh_browser();
        }
        if action_host_start {
            self.start_hosting();
        }
    }
}

impl AppHandler for Voxel {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, "voxel_demo started");
        match Renderer::new(window) {
            Ok(r) => {
                let ui = EditorUi::new(r.wgpu_device(), r.surface_format(), window.raw(), 1);
                self.ui = Some(ui);
                self.renderer = Some(r);
            }
            Err(err) => {
                error!(error = %err, "renderer init failed");
                std::process::exit(2);
            }
        }
        // Build the procedural block atlas once + cache the shared
        // bind group. Every chunk submesh clones the Arc when it
        // creates its mesh handle so the GPU only pays the upload
        // cost once at startup.
        if let Some(r) = self.renderer.as_mut() {
            let (aw, ah, atlas_rgba) = self
                .texture_dir
                .as_ref()
                .and_then(|d| load_block_atlas_from_dir(d))
                .unwrap_or_else(build_block_atlas);
            self.voxel_atlas = Some(r.make_mesh_albedo(aw, ah, &atlas_rgba));
            // Keep the pixel data around for the egui inventory UI.
            self.atlas_rgba = Some((aw, ah, atlas_rgba));
            // Sheep body texture: a plain wool tile. We deliberately
            // don't use the Minecraft entity `sheep.tga` here -
            // that's a UV-mapped atlas for a multi-cube model
            // (head/body/legs split across face strips), and
            // wrapping it onto our single cube produces stripey
            // holes. The wool block tile, on the other hand, has
            // no UV layout assumptions: every face looks like wool.
            //
            // Alpha is force-cleared to 255 so the global cutout
            // shader path (`alpha < 0.5 -> discard`) can't poke
            // gaps through soft edges in the source PNG.
            if let Some(dir) = self.texture_dir.as_ref() {
                let path = dir.join("wool.png");
                if path.exists()
                    && let Ok(mut img) = image::open(&path).map(|i| i.to_rgba8())
                {
                    for px in img.chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                    let mesh = r.create_cube_mesh("voxel_demo_mp::sheep_body");
                    let albedo =
                        r.make_mesh_albedo(img.width(), img.height(), &img);
                    r.bind_mesh_albedo_arc(mesh, albedo);
                    info!(path = %path.display(), "sheep body texture bound");
                    self.sheep_mesh = Some(mesh);
                }
                // Sheep head: the source `sheep.tga` is a UV atlas
                // for the full MC sheep model. We extract just the
                // face region + tile it across all 6 faces of a
                // dedicated head cube. Not pixel-perfect (the
                // proper fix is per-face UVs on a custom mesh) but
                // every visible side of the head shows the face,
                // which reads as a sheep from any angle.
                //
                // MC sheep texture layout (relative to the head
                // strip): the front face sits at U=[8/64, 16/64],
                // V=[8/32, 16/32] - i.e. the second 8x8 tile from
                // the left, second row. Bedrock's `sheep.tga` is
                // 64x64 (Java is 64x32), so we scale by texture
                // height to land on the same logical pixels.
                let sheep_root = dir
                    .parent()
                    .unwrap_or(dir.as_path())
                    .join("entity")
                    .join("sheep");
                let sheep_paths = [
                    sheep_root.join("sheep.tga"),
                    sheep_root.join("sheep.png"),
                    dir.join("sheep.tga"),
                    dir.join("sheep.png"),
                ];
                let sheep_src = sheep_paths.iter().find(|p| p.exists()).cloned();
                if let Some(path) = sheep_src
                    && let Ok(mut img) = image::open(&path).map(|i| i.to_rgba8())
                {
                    let w = img.width();
                    let h = img.height();
                    // Force alpha=255 so the cutout shader can't
                    // poke holes through soft edges in the source.
                    for px in img.chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                    // Build a head cube with per-face UVs into the
                    // standard sheep texture layout (8x8 face tiles
                    // at the head strip on a 64-wide texture). Each
                    // face only shows the right region of the
                    // texture: front gets the face, sides get the
                    // wool sides, top gets the head crown.
                    let (verts, indices) = build_sheep_head_mesh(w, h);
                    let mesh = r.create_mesh("voxel_demo_mp::sheep_head", &verts, &indices);
                    let albedo = r.make_mesh_albedo(w, h, &img);
                    r.bind_mesh_albedo_arc(mesh, albedo);
                    info!(
                        path = %path.display(),
                        texture = format!("{w}x{h}"),
                        "sheep head per-face UV mesh bound"
                    );
                    self.sheep_head_mesh = Some(mesh);
                }
            }
        }
        self.workers = Some(WorkerPool::spawn(NUM_WORKERS));
        self.build_world();
        // Best-effort audio: a missing output device just disables
        // SFX rather than blocking the demo from launching.
        match infengine_audio::AudioSystem::new() {
            Ok(a) => self.audio = Some(a),
            Err(err) => {
                warn!(error = %err, "audio init failed - SFX disabled");
            }
        }
        // Load user-supplied sound assets from `--sounds <dir>`.
        // Routed through the engine-level `AudioBank` helper so
        // shuffling + distance roll-off is reusable across demos.
        if let (Some(a), Some(dir)) = (self.audio.as_ref(), self.sounds_dir.as_ref()) {
            let scans: &[(&str, &str, &[&str])] = &[
                ("dig:grass", "dig", &["grass1.ogg", "grass2.ogg", "grass3.ogg", "grass4.ogg"]),
                ("dig:stone", "dig", &["stone1.ogg", "stone2.ogg", "stone3.ogg", "stone4.ogg"]),
                ("dig:sand", "dig", &["sand1.ogg", "sand2.ogg", "sand3.ogg", "sand4.ogg"]),
                ("dig:wood", "dig", &["wood1.ogg", "wood2.ogg", "wood3.ogg", "wood4.ogg"]),
                ("dig:gravel", "dig", &["gravel1.ogg", "gravel2.ogg", "gravel3.ogg", "gravel4.ogg"]),
                ("dig:glass", "dig", &["glass1.ogg", "glass2.ogg", "glass3.ogg"]),
                ("step:grass", "step", &["grass1.ogg", "grass2.ogg", "grass3.ogg", "grass4.ogg"]),
                ("step:stone", "step", &["stone1.ogg", "stone2.ogg", "stone3.ogg", "stone4.ogg"]),
                ("step:sand", "step", &["sand1.ogg", "sand2.ogg", "sand3.ogg", "sand4.ogg"]),
                ("step:wood", "step", &["wood1.ogg", "wood2.ogg", "wood3.ogg", "wood4.ogg"]),
                ("step:gravel", "step", &["gravel1.ogg", "gravel2.ogg", "gravel3.ogg", "gravel4.ogg"]),
                (
                    "mob/sheep/say",
                    "mob/sheep",
                    &["say1.ogg", "say2.ogg", "say3.ogg"],
                ),
                (
                    "mob/sheep/step",
                    "mob/sheep",
                    &["step1.ogg", "step2.ogg", "step3.ogg", "step4.ogg", "step5.ogg"],
                ),
            ];
            for (key, subdir, files) in scans {
                let paths: Vec<std::path::PathBuf> = files
                    .iter()
                    .map(|f| dir.join(subdir).join(f))
                    .filter(|p| p.exists())
                    .collect();
                if paths.is_empty() {
                    continue;
                }
                let n = self.audio_bank.load_variants(a, key, &paths);
                if n > 0 {
                    info!(key = %key, count = n, "loaded sound asset variants");
                }
            }
        }
        // Server resumes any persisted edits so a restart picks up
        // where the world left off. Clients sync via the catch-up
        // broadcast on first heartbeat.
        if matches!(self.net, NetRole::Server { .. }) {
            self.load_server_state();
        }
    }

    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(ui) = self.ui.as_mut() {
            ui.handle_engine_event(window, event);
        }
        // Window close: send a `vx.leave` first so the server can
        // reap our pose instantly + broadcast the removal to the
        // other peers. The actual process exit happens on the next
        // `on_update` via `requested_quit`.
        if matches!(event, WindowEvent::CloseRequested) {
            self.notify_server_leaving();
            self.requested_quit = true;
        }
        // Focus tracking. Clicking another window on the same machine
        // shifts focus; without this guard `apply_cursor_settings`
        // would keep wrestling the cursor away from whichever window
        // the user actually clicked.
        if let WindowEvent::Focused(focused) = event {
            self.has_focus = *focused;
            if !*focused {
                self.lmb_pending = false;
                self.rmb_pending = false;
                self.mouse_dx = 0.0;
                self.mouse_dy = 0.0;
                self.last_mouse = None;
            }
        }
        // Track cursor moved deltas only when no grab is active (i.e. in
        // menus this is the only source; in Play raw DeviceEvent
        // supplies the deltas).
        if let WindowEvent::CursorMoved { position, .. } = event
            && matches!(self.state, AppState::Playing)
            && !(self.settings.lock_cursor || self.settings.hide_cursor)
        {
            let now = [position.x as f32, position.y as f32];
            if let Some(prev) = self.last_mouse {
                self.mouse_dx += now[0] - prev[0];
                self.mouse_dy += now[1] - prev[1];
            }
            self.last_mouse = Some(now);
        }
        // Edge-triggered LMB / RMB while in Play state. Both flags are
        // consumed in `on_update` next frame. Ignored when the window
        // doesn't have focus so a click on a sibling instance never
        // mines a block here.
        if matches!(self.state, AppState::Playing)
            && self.has_focus
            && let WindowEvent::MouseInput { state, button, .. } = event
            && matches!(state, ElementState::Pressed)
        {
            match button {
                MouseButton::Left => self.lmb_pending = true,
                MouseButton::Right => self.rmb_pending = true,
                _ => {}
            }
        }
        // Mouse wheel cycles through hotbar slots.
        if matches!(self.state, AppState::Playing)
            && let WindowEvent::MouseWheel { delta, .. } = event
        {
            use infengine_window::raw_winit::event::MouseScrollDelta;
            let y = match delta {
                MouseScrollDelta::LineDelta(_, y) => *y,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 50.0,
            };
            self.wheel_accum += y;
        }
    }

    fn on_device_event(&mut self, event: &infengine_window::raw_winit::event::DeviceEvent) {
        use infengine_window::raw_winit::event::DeviceEvent;
        if !matches!(self.state, AppState::Playing) {
            return;
        }
        // Device events fire globally - any other instance with cursor
        // lock would otherwise rotate its camera off our motion. Gate
        // on focus to keep input owner-local.
        if !self.has_focus {
            return;
        }
        if !(self.settings.lock_cursor || self.settings.hide_cursor) {
            return;
        }
        if let DeviceEvent::MouseMotion { delta } = event {
            self.mouse_dx += delta.0 as f32;
            self.mouse_dy += delta.1 as f32;
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.clamp(1e-4, 0.1);
        self.fps_ema = self.fps_ema * 0.9 + (1.0 / dt) * 0.1;

        // Pull any pending master-query results back onto the main
        // thread (refresh + announce both run on background workers).
        self.poll_master_queries();

        // Esc behaviour depends on net role:
        //  - Offline: classic Play <-> Paused toggle (whole world
        //    halts while the pause overlay is up).
        //  - Server / Client: world must keep running for the
        //    other peers, so Esc just toggles `mp_menu_open` which
        //    drives an overlay UI; physics / net tick continue.
        // Settings always backs out via Esc regardless of role.
        let esc_now = ctx.input.is_pressed(Key::Escape);
        let esc_just = esc_now && !self.esc_prev;
        self.esc_prev = esc_now;
        if esc_just {
            let mp = !matches!(self.net, NetRole::Offline);
            match self.state {
                AppState::Playing => {
                    if mp {
                        self.mp_menu_open = !self.mp_menu_open;
                    } else {
                        self.state = AppState::Paused;
                    }
                }
                AppState::Paused => self.state = AppState::Playing,
                AppState::Settings { back_to } => {
                    self.state = match back_to {
                        BackTo::MainMenu => AppState::MainMenu,
                        BackTo::Pause => AppState::Paused,
                    };
                }
                AppState::MainMenu
                | AppState::DirectConnect
                | AppState::ServerBrowser
                | AppState::HostSetup => {}
            }
        }

        // E toggles the inventory overlay while playing. When the
        // overlay is open we release the cursor + freeze player
        // input so the UI is interactable.
        // F3 toggles the debug panel. Mirrors Minecraft's debug
        // overlay: cheap to leave running, off by default.
        let f3_now = ctx.input.is_pressed(Key::F3);
        if f3_now && !self.f3_prev && matches!(self.state, AppState::Playing) {
            self.show_debug = !self.show_debug;
        }
        self.f3_prev = f3_now;

        let e_now = ctx.input.is_pressed(Key::E);
        let e_just = e_now && !self.e_prev;
        self.e_prev = e_now;
        if e_just && matches!(self.state, AppState::Playing) {
            self.inventory_open = !self.inventory_open;
            // Drop any queued mouse-button edits the click that
            // closes the inventory would otherwise be replayed
            // against the world.
            self.lmb_pending = false;
            self.rmb_pending = false;
        }

        if matches!(self.state, AppState::Playing) {
            // update_play itself gates player input on
            // `self.inventory_open` - we still need to tick it every
            // frame so gravity + collision + net pose broadcasts
            // keep running while the menu is up.
            self.update_play(ctx, dt);
            self.stream_chunks();
        } else {
            // Paused / in a menu screen: drop captured mouse motion
            // so the camera doesn't lurch when play resumes.
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
            self.wheel_accum = 0.0;
        }
        self.apply_cursor_settings(window);

        // Build UI state first so the immediate-mode pass mutates self
        // freely. The renderer is borrowed afterwards once UI state has
        // settled.
        self.render_ui(window);

        let forward = Vec3::new(
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        );

        // Cast a ray from the eye along the look direction so the
        // crosshair / wireframe / edit code can target the voxel the
        // player is currently aiming at.
        self.target = if matches!(self.state, AppState::Playing) {
            raycast(&self.world, self.player_pos, forward, 6.0)
        } else {
            None
        };

        // Apply pending mouse-button edits before rendering. LMB
        // always breaks the targeted voxel. RMB places the selected
        // hotbar block at the adjacent empty cell, but is ignored
        // when the empty (break) slot is selected.
        let lmb = self.lmb_pending;
        let rmb = self.rmb_pending;
        self.lmb_pending = false;
        self.rmb_pending = false;
        // The currently-held block: contents of the selected hotbar
        // slot. `None` = break / empty hand (or 0-count remnant).
        let selected_block = self.inventory[self.selected_slot].map(|(b, _)| b);
        // Net mode flag: clients defer block edits to the server +
        // wait for the server echo before applying. Offline + server
        // apply locally as before.
        let is_client = matches!(self.net, NetRole::Client { .. });
        // Mob attack interception: if LMB and a mob is closer than
        // the targeted voxel (or there's no voxel target), route
        // the click as a melee strike instead of a block break.
        let mob_target = if lmb && !self.inventory_open && !self.mp_menu_open {
            self.raycast_mob(self.player_pos, forward, MOB_ATTACK_REACH)
        } else {
            None
        };
        let voxel_dist = self.target.map(|h| {
            let c = Vec3::new(
                h.voxel[0] as f32 + 0.5,
                h.voxel[1] as f32 + 0.5,
                h.voxel[2] as f32 + 0.5,
            );
            (c - self.player_pos).length()
        });
        let attack_mob = match (mob_target, voxel_dist) {
            (Some((id, d_mob)), Some(d_vox)) if d_mob <= d_vox => Some(id),
            (Some((id, _)), None) => Some(id),
            _ => None,
        };
        if let Some(mob_id) = attack_mob {
            // Consume the LMB so it doesn't also break a voxel.
            if is_client {
                // Client: send hit request to server, optimistic
                // flash so the player gets immediate feedback.
                let msg = MobHitMsg { mob_id };
                if let NetRole::Client { client, .. } = &self.net {
                    let cfg = bincode::config::standard();
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        let _ = client.send_bytes("vx.mob_hit", buf);
                    }
                }
                self.mob_hurt_flash.insert(mob_id, 0.30);
                if let Some(mob) = self.remote_mobs.get(&mob_id).copied() {
                    let pos = mob.pos;
                    self.sfx_at("mob/sheep/say", pos, |a, g| {
                        let _ = a.play_beep(280.0, 0.12, 0.18 * g);
                    });
                }
            } else {
                // Offline / host: authoritative damage path. Host
                // is attacker id 0.
                self.damage_mob(mob_id, Some(0));
            }
        }
        // Block edits pause when the inventory overlay is open so
        // panel clicks don't accidentally mine the world behind it.
        if !self.inventory_open
            && !self.mp_menu_open
            && attack_mob.is_none()
            && let Some(hit) = self.target
        {
            if lmb {
                // Capture the block we're about to break so we can
                // pop it into the inventory (creative-but-greedy
                // collection - air just no-ops).
                let broken = self.world.get(hit.voxel);
                let edit = EditMsg {
                    x: hit.voxel[0],
                    y: hit.voxel[1],
                    z: hit.voxel[2],
                    block: 0,
                };
                if is_client {
                    // Client-side prediction: apply immediately for
                    // snappy feedback, queue for the server. The
                    // server's echo will re-apply (idempotent set);
                    // a future rollback layer would reconcile on a
                    // mismatch.
                    self.world_set_tracked(hit.voxel, BlockId::AIR);
                    let coords = Self::affected_chunks(hit.voxel);
                    self.rebuild_chunks(&coords);
                    self.spawn_break_particles(hit.voxel);
                    if let NetRole::Client { pending_edits, .. } = &mut self.net {
                        pending_edits.push(edit);
                    }
                } else {
                    self.world_set_tracked(hit.voxel, BlockId::AIR);
                    let coords = Self::affected_chunks(hit.voxel);
                    self.rebuild_chunks(&coords);
                    self.spawn_break_particles(hit.voxel);
                    if let NetRole::Server { server, .. } = &self.net {
                        let cfg = bincode::config::standard();
                        if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                    self.edit_history.push(edit);
                }
                // Inventory bookkeeping:
                //  - Offline / Server (host): authoritative locally,
                //    credit the pickup straight away.
                //  - Client: defer - the server credits + replies
                //    with a `vx.inv` snapshot. Predicting locally
                //    would risk a double-credit when the snapshot
                //    arrives.
                if !is_client && broken != BlockId::AIR {
                    self.pickup(broken);
                }
                // Break sound: per-material `dig:*` from the asset
                // pack with distance roll-off. Air swings still get
                // a soft click so the player feels the swing.
                let voxel_center = [
                    hit.voxel[0] as f32 + 0.5,
                    hit.voxel[1] as f32 + 0.5,
                    hit.voxel[2] as f32 + 0.5,
                ];
                if broken == BlockId::AIR {
                    self.sfx_at("place:air", voxel_center, |a, g| {
                        let _ = a.play_click(180.0, 0.10 * g);
                    });
                } else {
                    let key = format!("dig:{}", block_material(broken));
                    let freq = 220.0 + (broken.0 as f32) * 28.0;
                    self.sfx_at(&key, voxel_center, move |a, g| {
                        let _ = a.play_thud(freq, 0.30 * g);
                    });
                }
            } else if rmb && let Some(block) = selected_block {
                // Reject placement when it would intersect the player
                // capsule - otherwise the player gets stuck inside
                // the freshly placed block.
                let candidate = self.player_pos; // current eye pos
                let test_pos = candidate;
                let (min, max) = body_aabb(test_pos);
                let p = hit.place_at;
                let voxel_min = [p[0] as f32, p[1] as f32, p[2] as f32];
                let voxel_max = [p[0] as f32 + 1.0, p[1] as f32 + 1.0, p[2] as f32 + 1.0];
                let overlaps = max.x > voxel_min[0]
                    && min.x < voxel_max[0]
                    && max.y > voxel_min[1]
                    && min.y < voxel_max[1]
                    && max.z > voxel_min[2]
                    && min.z < voxel_max[2];
                if !overlaps {
                    let edit = EditMsg {
                        x: hit.place_at[0],
                        y: hit.place_at[1],
                        z: hit.place_at[2],
                        block: block.0,
                    };
                    if is_client {
                        self.world_set_tracked(hit.place_at, block);
                        let coords = Self::affected_chunks(hit.place_at);
                        self.rebuild_chunks(&coords);
                        if let NetRole::Client { pending_edits, .. } = &mut self.net {
                            pending_edits.push(edit);
                        }
                    } else {
                        self.world_set_tracked(hit.place_at, block);
                        let coords = Self::affected_chunks(hit.place_at);
                        self.rebuild_chunks(&coords);
                        if let NetRole::Server { server, .. } = &self.net {
                            let cfg = bincode::config::standard();
                            if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                                server.broadcast_bytes("vx.edit", buf);
                            }
                        }
                        self.edit_history.push(edit);
                    }
                    // Decrement the active stack now that placement
                    // succeeded. Clients defer this - the server's
                    // `vx.inv` snapshot reflects the decrement.
                    if !is_client
                        && let Some((_, count)) =
                            self.inventory[self.selected_slot].as_mut()
                    {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            self.inventory[self.selected_slot] = None;
                        }
                    }
                    // Place sound: reuse the same `dig:*` material
                    // bank (MC does the same - the place sound IS
                    // the dig sound), just played at the placement
                    // voxel with the destination block's material.
                    let key = format!("dig:{}", block_material(block));
                    let freq = 380.0 + (block.0 as f32) * 22.0;
                    let place_center = [
                        hit.place_at[0] as f32 + 0.5,
                        hit.place_at[1] as f32 + 0.5,
                        hit.place_at[2] as f32 + 0.5,
                    ];
                    self.sfx_at(&key, place_center, move |a, g| {
                        let _ = a.play_click(freq, 0.18 * g);
                    });
                    // Albedo-tinted dust puff so the place reads
                    // visually too.
                    let color = self
                        .registry
                        .get(block)
                        .map(|d| d.color)
                        .unwrap_or([0.7, 0.7, 0.7]);
                    self.spawn_place_particles(hit.place_at, color);
                }
            }
        }
        // Net tick: poll + apply + broadcast / send.
        self.tick_net();
        // Authoritative side simulates wandering mobs. The render
        // path reads `self.remote_mobs` which gets populated by
        // both `tick_mobs` (server) and `vx.mob` (client).
        self.tick_mobs(dt);
        // Mob audio runs on every peer (host + each client): step
        // sting per moving sheep, occasional baa, both
        // distance-attenuated through `sfx_at`.
        self.tick_mob_audio(dt);
        // Tick particles regardless of net role.
        self.particles.tick(dt);
        // Tick the day clock unconditionally - even paused / in
        // menus the world keeps turning so the host's lighting is
        // never out of sync with what a client sees.
        self.day_time = (self.day_time + dt).rem_euclid(DAY_LENGTH_S);

        let player_pos = self.player_pos;
        let chunk_meshes = self.chunk_meshes.clone();
        let registry = &self.registry;
        let target = self.target;
        let ui = self.ui.as_mut();
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        {
            let cam = renderer.camera_3d_mut();
            cam.position = player_pos;
            cam.target = player_pos + forward;
        }
        // Day/night cycle. `day_time` ramps from 0..DAY_LENGTH_S
        // and feeds both the sun direction (rotating around the +Z
        // axis) and the clear color (sunrise -> noon -> sunset ->
        // night). The sky color is a perceptual mix between three
        // anchor swatches keyed on the sun's elevation.
        let (sun_dir, sky) = day_night(self.day_time);
        renderer.set_sun_dir(sun_dir);
        renderer.set_clear_color(sky[0] as f64, sky[1] as f64, sky[2] as f64, 1.0);
        // Sun + moon as 3D meshes far away. Going through the
        // mesh3d pass means the depth buffer occludes them when
        // terrain stands between the player + the sky (rooms,
        // mountains, etc.). We over-saturate the albedo (>1.0) so
        // the PBR shader's `direct + ambient` output clamps to
        // bright on every face - no need for a dedicated unlit
        // pipeline yet.
        let player_cube_handle =
            *self.player_cube.get_or_insert_with(|| renderer.cube_handle());
        const CELESTIAL_DIST: f32 = 140.0;
        // Sun: only render while above the horizon (matches MC
        // exactly + avoids a fake-glow disk piercing the floor
        // at night). Smooth alpha gate uses the same elevation
        // curve as the sky color.
        let sun_elev = sun_dir.y;
        if sun_elev > -0.05 {
            let sun_world = Vec3::new(
                player_pos.x + sun_dir.x * CELESTIAL_DIST,
                player_pos.y + sun_dir.y * CELESTIAL_DIST,
                player_pos.z + sun_dir.z * CELESTIAL_DIST,
            );
            let m = Mat4::from_scale_rotation_translation(
                Vec3::splat(14.0),
                glam::Quat::IDENTITY,
                sun_world,
            );
            renderer.draw_mesh_pbr(
                player_cube_handle,
                m.to_cols_array(),
                [5.0, 4.5, 2.2, 1.0],
                0.0,
                1.0,
            );
        }
        // Moon: opposite of sun, slightly smaller + cooler tint.
        let moon_dir = -sun_dir;
        if moon_dir.y > -0.05 {
            let moon_world = Vec3::new(
                player_pos.x + moon_dir.x * CELESTIAL_DIST,
                player_pos.y + moon_dir.y * CELESTIAL_DIST,
                player_pos.z + moon_dir.z * CELESTIAL_DIST,
            );
            let m = Mat4::from_scale_rotation_translation(
                Vec3::splat(10.0),
                glam::Quat::IDENTITY,
                moon_world,
            );
            renderer.draw_mesh_pbr(
                player_cube_handle,
                m.to_cols_array(),
                [2.8, 3.0, 3.8, 1.0],
                0.0,
                1.0,
            );
        }
        // Push the 8 nearest torch positions to the renderer as
        // point lights. We sort by squared distance so the player
        // always sees the closest sources lit; far torches just
        // drop out (MAX_LIGHTS = 8 cap in the shader).
        let mut torches: Vec<([i32; 3], f32)> = self
            .torch_positions
            .iter()
            .map(|p| {
                let dx = p[0] as f32 + 0.5 - player_pos.x;
                let dy = p[1] as f32 + 0.5 - player_pos.y;
                let dz = p[2] as f32 + 0.5 - player_pos.z;
                (*p, dx * dx + dy * dy + dz * dz)
            })
            .collect();
        torches.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (p, _) in torches.into_iter().take(8) {
            renderer.queue_point_light(
                [p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5],
                [1.0, 0.78, 0.40],
                3.0,
                8.0,
            );
        }
        // Render remote players as coloured cubes so each peer is
        // visible. Server uses its own `poses` map (skipping id 0 -
        // that's the host's own pose); client uses the `remote`
        // table (already strips its own id).
        let remote_iter: Vec<PlayerMsg> = match &self.net {
            NetRole::Offline => Vec::new(),
            NetRole::Server { poses, .. } => {
                poses.values().filter(|p| p.id != 0).copied().collect()
            }
            NetRole::Client { remote, .. } => remote.values().copied().collect(),
        };
        if !remote_iter.is_empty() {
            let cube = self.player_cube.get_or_insert_with(|| renderer.cube_handle());
            for p in remote_iter {
                let m = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.6, 1.7, 0.6),
                    glam::Quat::from_rotation_y(p.yaw),
                    Vec3::new(p.pos[0], p.pos[1] - 0.85, p.pos[2]),
                );
                // Hash id to a stable colour.
                let h = (p.id.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                let tau = std::f32::consts::TAU;
                let color = [
                    (0.4 + 0.5 * (h * tau).sin().abs()).min(1.0),
                    (0.4 + 0.5 * ((h + 0.33) * tau).sin().abs()).min(1.0),
                    (0.4 + 0.5 * ((h + 0.66) * tau).sin().abs()).min(1.0),
                    1.0,
                ];
                renderer.draw_mesh_pbr(*cube, m.to_cols_array(), color, 0.0, 0.7);
            }
        }
        // Render every known sheep (server: from `mobs`, client:
        // from `remote_mobs` populated by `vx.mob`). Sheep are
        // boxy white cubes with a small head cube on top for
        // silhouette. Scaled wider than tall so they read as
        // four-legged rather than human-shaped.
        if !self.remote_mobs.is_empty() {
            let player_cube = self
                .player_cube
                .get_or_insert_with(|| renderer.cube_handle());
            // Body uses the dedicated textured sheep mesh when
            // available; head always uses the plain cube mesh
            // tinted skin-tone so the two parts can have different
            // albedos without needing two separate textured
            // meshes (the MC sheep model has a separate head
            // material too).
            let body_mesh = self.sheep_mesh.unwrap_or(*player_cube);
            let body_tint: [f32; 4] = if self.sheep_mesh.is_some() {
                // White tint = let the wool texture show through
                // unmodulated.
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.92, 0.92, 0.92, 1.0]
            };
            // Head: prefer the textured `sheep_head_mesh` (cropped
            // face region of the MC entity texture) so the front
            // shows a real sheep face. Falls back to the
            // untextured player_cube with a skin tint when the
            // crop wasn't loaded.
            let head_mesh = self.sheep_head_mesh.unwrap_or(*player_cube);
            let head_color: [f32; 4] = if self.sheep_head_mesh.is_some() {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.92, 0.86, 0.78, 1.0]
            };
            let mobs_snapshot: Vec<MobMsg> =
                self.remote_mobs.values().copied().collect();
            for mob in mobs_snapshot {
                let hurt_flash = self
                    .mob_hurt_flash
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                // Mix toward red when recently damaged.
                let tint = [
                    body_tint[0] * (1.0 - hurt_flash) + 1.0 * hurt_flash,
                    body_tint[1] * (1.0 - hurt_flash) + 0.25 * hurt_flash,
                    body_tint[2] * (1.0 - hurt_flash) + 0.25 * hurt_flash,
                    body_tint[3],
                ];
                let body = Mat4::from_scale_rotation_translation(
                    Vec3::new(1.2, 0.8, 0.7),
                    glam::Quat::from_rotation_y(mob.yaw),
                    Vec3::new(mob.pos[0], mob.pos[1] + 0.4, mob.pos[2]),
                );
                renderer.draw_mesh_pbr(
                    body_mesh,
                    body.to_cols_array(),
                    tint,
                    0.0,
                    0.9,
                );
                // Head: small cube offset along the body's forward
                // (yaw rotated +X).
                // Match the rotation-Y convention used by the body
                // matrix below: +X under that rotation becomes
                // (cos yaw, 0, -sin yaw).
                let fwd = Vec3::new(mob.yaw.cos(), 0.0, -mob.yaw.sin());
                let head_pos = Vec3::new(mob.pos[0], mob.pos[1] + 0.8, mob.pos[2])
                    + fwd * 0.45;
                let head = Mat4::from_scale_rotation_translation(
                    // MC sheep head is 6 wide x 6 tall x 8 deep, with
                    // depth along the forward axis. Our cube faces
                    // +X forward, so X = depth = 0.60, Y = 0.45,
                    // Z = 0.45.
                    Vec3::new(0.60, 0.45, 0.45),
                    glam::Quat::from_rotation_y(mob.yaw),
                    head_pos,
                );
                let head_tint = [
                    head_color[0] * (1.0 - hurt_flash) + 1.0 * hurt_flash,
                    head_color[1] * (1.0 - hurt_flash) + 0.25 * hurt_flash,
                    head_color[2] * (1.0 - hurt_flash) + 0.25 * hurt_flash,
                    head_color[3],
                ];
                renderer.draw_mesh_pbr(
                    head_mesh,
                    head.to_cols_array(),
                    head_tint,
                    0.0,
                    0.9,
                );
            }
        }
        // Render any live debris particles before the world cubes
        // so chunk faces draw on top when a particle ends up
        // inside a block.
        renderer.draw_particles(&self.particles);
        let model = Mat4::IDENTITY.to_cols_array();
        for (_coord, block, handle) in &chunk_meshes {
            if let Some(def) = registry.get(*block) {
                renderer.draw_mesh_pbr(
                    *handle,
                    model,
                    [def.color[0], def.color[1], def.color[2], 1.0],
                    def.metallic,
                    def.roughness,
                );
            }
        }
        // Wire outline previewing the next action.
        //   Empty slot     -> wire around the voxel that will be
        //                     broken (target.voxel, black stroke).
        //   Block slot     -> wire around the cell where the block
        //                     will be placed (target.place_at,
        //                     tinted with the block's albedo).
        if let Some(hit) = target {
            let (cell, color) = match selected_block {
                None => (hit.voxel, [0.0, 0.0, 0.0, 0.95]),
                Some(block) => {
                    let c = registry
                        .get(block)
                        .map(|d| {
                            [
                                (d.color[0] * 1.4).min(1.0),
                                (d.color[1] * 1.4).min(1.0),
                                (d.color[2] * 1.4).min(1.0),
                                0.95,
                            ]
                        })
                        .unwrap_or([1.0, 1.0, 1.0, 0.95]);
                    (hit.place_at, c)
                }
            };
            let min = [
                cell[0] as f32 - 0.005,
                cell[1] as f32 - 0.005,
                cell[2] as f32 - 0.005,
            ];
            let max = [
                cell[0] as f32 + 1.005,
                cell[1] as f32 + 1.005,
                cell[2] as f32 + 1.005,
            ];
            renderer.debug_box_3d(min, max, color);
        }
        let render_result = if let Some(ui) = ui {
            renderer.render(|d, q, e, v, s| ui.render(d, q, e, v, s))
        } else {
            renderer.render(|_, _, _, _, _| {})
        };
        if let Err(err) = render_result {
            warn!(error = ?err, "frame render failed");
        }

        if self.requested_quit {
            std::process::exit(0);
        }
    }

    fn on_resize(&mut self, w: u32, h: u32) {
        self.viewport = (w, h);
        if let Some(r) = self.renderer.as_mut() {
            r.resize(w, h);
        }
    }
}

/// Output from one worker thread: the generated chunk + its submeshes
/// ready to upload as GPU buffers on the main thread.
struct WorkerResult {
    coord: ChunkCoord,
    chunk: Chunk,
    submeshes: Vec<ChunkSubmesh>,
}

/// Thread pool that runs `generate_chunk` + `chunk_mesh` off the main
/// thread. The main thread enqueues `ChunkCoord` requests on
/// `req_tx`; workers produce `WorkerResult` on `res_rx`. Dropping the
/// pool closes `req_tx`; the workers see `recv` return Err and exit.
struct WorkerPool {
    req_tx: Sender<ChunkCoord>,
    res_rx: Receiver<WorkerResult>,
    threads: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    fn spawn(num_workers: usize) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<ChunkCoord>();
        let req_rx = Arc::new(Mutex::new(req_rx));
        let (res_tx, res_rx) = mpsc::channel::<WorkerResult>();
        let mut threads = Vec::with_capacity(num_workers);
        for i in 0..num_workers {
            let req_rx = Arc::clone(&req_rx);
            let res_tx = res_tx.clone();
            threads.push(
                thread::Builder::new()
                    .name(format!("voxel_worker_{i}"))
                    .spawn(move || {
                        loop {
                            // Briefly lock the shared receiver to grab the
                            // next coord. The lock is held only across one
                            // recv() call; generation runs lock-free.
                            let coord = {
                                let rx = req_rx.lock().expect("worker mutex");
                                match rx.recv() {
                                    Ok(c) => c,
                                    Err(_) => return,
                                }
                            };
                            let chunk = generate_chunk_at(coord);
                            let origin = World::chunk_world_origin(coord);
                            let submeshes = chunk_mesh_with_atlas(&chunk, origin, atlas_uv_rect, block_is_opaque);
                            if res_tx
                                .send(WorkerResult {
                                    coord,
                                    chunk,
                                    submeshes,
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    })
                    .expect("spawn voxel worker"),
            );
        }
        drop(res_tx); // workers keep their own clones
        Self {
            req_tx,
            res_rx,
            threads,
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // Close the request channel by replacing the sender with a
        // dummy that we then drop, which causes worker recv() to fail.
        // Replacing the field with a fresh channel and dropping is
        // equivalent; using `mem::replace` keeps the type stable.
        let (dummy_tx, _) = mpsc::channel::<ChunkCoord>();
        let _ = std::mem::replace(&mut self.req_tx, dummy_tx);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Free-standing generator the workers call. Independent of any `Self`
/// so it ports cleanly into the worker closure.
/// Three large-scale biomes drive both the terrain height curve and
/// the surface block. Biome selection rides on a slow 2D noise so a
/// peer running the same generator picks the same biome at every
/// (wx, wz) - no extra net traffic needed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Biome {
    Desert,
    Plains,
    Mountain,
}

fn biome_at(wx: f32, wz: f32) -> Biome {
    // Two layered sin/cos waves keep the biome map smooth and
    // deterministic. Range maps roughly to `[-1, 1]` so the cutoffs
    // are easy to reason about.
    let n = (wx * 0.012).sin() * 0.6 + (wz * 0.009).cos() * 0.5
        + ((wx + wz) * 0.004).sin() * 0.3;
    if n < -0.5 {
        Biome::Desert
    } else if n > 0.5 {
        Biome::Mountain
    } else {
        Biome::Plains
    }
}

/// Cheap, deterministic 2D hash. Used for tree placement so two
/// peers running the same generator drop trees in the same spots.
fn hash2(wx: i32, wz: i32) -> u32 {
    let mut h = (wx as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (wz as u32).wrapping_mul(0x1656_67B1);
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h.wrapping_mul(0xC2B2_AE35)
}

/// One tile inside the 4x4 atlas. `files` is the ordered fallback
/// list of source-file names the loader tries; first match wins.
/// `keep_alpha=true` means the source alpha channel survives into
/// the atlas (required for cutout textures like leaves);
/// `false` forces the atlas pixel alpha to 255 so anti-aliased
/// edges in the source don't accidentally discard solid pixels
/// via the global alpha-cutout shader path.
struct TileSpec {
    col: u32,
    row: u32,
    files: &'static [&'static str],
    keep_alpha: bool,
}

/// One block kind's full definition. Adding a new block = one
/// entry here + a hotbar slot below; the registry, atlas loader,
/// UV lookup and opacity test all read from this table so there's
/// no second source of truth to keep in sync.
struct BlockType {
    id: u16,
    name: &'static str,
    color: [f32; 3],
    transparent: bool,
    top: TileSpec,
    bottom: TileSpec,
    sides: TileSpec,
}

const ATLAS_COLS: u32 = 4;
const ATLAS_ROWS: u32 = 4;
const TILE_PX: u32 = 16;

const BLOCK_TYPES: &[BlockType] = &[
    BlockType {
        id: 1, name: "stone", color: [0.50, 0.50, 0.55], transparent: false,
        top: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 2, name: "dirt", color: [0.45, 0.32, 0.20], transparent: false,
        top: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
    },
    BlockType {
        id: 3, name: "grass", color: [0.35, 0.62, 0.28], transparent: false,
        top: TileSpec {
            col: 2, row: 0,
            files: &["grass_carried.png", "grass_top.png"],
            keep_alpha: false,
        },
        bottom: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec {
            col: 2, row: 1,
            files: &["grass_side_carried.png", "grass_side.tga"],
            keep_alpha: false,
        },
    },
    BlockType {
        id: 4, name: "sand", color: [0.92, 0.86, 0.62], transparent: false,
        top: TileSpec { col: 3, row: 0, files: &["sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 0, files: &["sand.png"], keep_alpha: false },
        sides: TileSpec { col: 3, row: 0, files: &["sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 5, name: "wood", color: [0.55, 0.38, 0.22], transparent: false,
        top: TileSpec { col: 3, row: 1, files: &["log_oak_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["log_oak_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["log_oak.png"], keep_alpha: false },
    },
    BlockType {
        id: 6, name: "leaves", color: [0.24, 0.50, 0.20], transparent: true,
        top: TileSpec {
            col: 1, row: 1,
            files: &["leaves_oak.tga", "leaves_oak_carried.png", "leaves_oak_opaque.png"],
            keep_alpha: true,
        },
        bottom: TileSpec {
            col: 1, row: 1,
            files: &["leaves_oak.tga", "leaves_oak_carried.png", "leaves_oak_opaque.png"],
            keep_alpha: true,
        },
        sides: TileSpec {
            col: 1, row: 1,
            files: &["leaves_oak.tga", "leaves_oak_carried.png", "leaves_oak_opaque.png"],
            keep_alpha: true,
        },
    },
    BlockType {
        id: 7, name: "planks", color: [0.78, 0.60, 0.36], transparent: false,
        top: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
    },
    BlockType {
        id: 8, name: "cobble", color: [0.42, 0.42, 0.45], transparent: false,
        top: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
    },
    BlockType {
        id: 9, name: "glass", color: [0.85, 0.92, 0.98], transparent: true,
        top: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
        sides: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 10, name: "torch", color: [1.00, 0.78, 0.40], transparent: true,
        top: TileSpec { col: 3, row: 2, files: &["torch_on.png", "torch.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["torch_on.png", "torch.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["torch_on.png", "torch.png"], keep_alpha: true },
    },
    BlockType {
        id: 11, name: "wool", color: [0.95, 0.95, 0.95], transparent: false,
        top: TileSpec { col: 0, row: 3, files: &["wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 3, files: &["wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 3, files: &["wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 12, name: "meat", color: [0.85, 0.45, 0.40], transparent: false,
        top: TileSpec { col: 1, row: 3, files: &["meat.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 3, files: &["meat.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 3, files: &["meat.png"], keep_alpha: false },
    },
];

/// Block id of the torch (light source). Kept as a const so the
/// renderer-side light scan doesn't have to do a string lookup.
const TORCH_ID: u16 = 10;

/// World-generation profile. Selected once at startup (via the
/// `--worldgen=<name>` CLI flag) and read by every chunk-generator
/// call so background workers stay in sync without plumbing a
/// shared param through every call. Loaded into `WORLDGEN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorldGen {
    /// Default rolling biomes (Plains / Mountain / Desert) with
    /// small oak trees in plains.
    Classic = 0,
    /// Pancake-flat dirt + grass at y=8. Builders welcome.
    Flat = 1,
    /// Sparse islands rising out of empty space. Walk-the-plank
    /// vibes; great test for fall-out penalty.
    Islands = 2,
    /// Like Classic but with the height map amplified ~2.5x.
    /// Tall mountains, deep valleys, more dramatic skyline.
    Amplified = 3,
}

impl WorldGen {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "classic" | "default" => Some(Self::Classic),
            "flat" => Some(Self::Flat),
            "islands" | "island" => Some(Self::Islands),
            "amplified" | "amp" => Some(Self::Amplified),
            _ => None,
        }
    }
    fn current() -> Self {
        match WORLDGEN.load(std::sync::atomic::Ordering::Relaxed) {
            1 => Self::Flat,
            2 => Self::Islands,
            3 => Self::Amplified,
            _ => Self::Classic,
        }
    }
}

/// Shared world-gen mode. Set once on startup, read from every
/// worker thread that generates chunks. `Relaxed` is fine because
/// the value never changes mid-run.
static WORLDGEN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn block_type(id: u16) -> Option<&'static BlockType> {
    BLOCK_TYPES.iter().find(|b| b.id == id)
}

/// Map a block id to the Minecraft sound-material name. Used to
/// pick the right `dig:*` / `step:*` variant from `sound_assets`.
/// Unknown blocks default to `"stone"` because that's the least
/// surprising thud / step click.
fn block_material(b: BlockId) -> &'static str {
    match b.0 {
        1 | 8 => "stone",       // stone, cobble
        2 => "grass",           // dirt - MC actually uses "gravel"/"grass" mix; grass reads OK
        3 => "grass",
        4 => "sand",
        5 | 7 | 10 => "wood",   // wood, planks, torch
        6 => "grass",           // leaves - MC has "grass" mapping
        9 => "glass",
        _ => "stone",
    }
}

fn block_is_opaque(b: BlockId) -> bool {
    block_type(b.0).map(|t| !t.transparent).unwrap_or(true)
}

fn atlas_uv_rect(block: BlockId, normal: [i8; 3]) -> [f32; 4] {
    let Some(bt) = block_type(block.0) else {
        return [0.0, 0.0, 1.0, 1.0];
    };
    let tile = if normal[1] > 0 {
        &bt.top
    } else if normal[1] < 0 {
        &bt.bottom
    } else {
        &bt.sides
    };
    let u_step = 1.0 / ATLAS_COLS as f32;
    let v_step = 1.0 / ATLAS_ROWS as f32;
    let u0 = tile.col as f32 * u_step;
    let v0 = tile.row as f32 * v_step;
    [u0, v0, u0 + u_step, v0 + v_step]
}

/// Try to load the atlas from a directory of 16x16 PNG / TGA files
/// (one per block kind). Returns `None` if the directory is missing
/// or any required texture fails to load. The names match the
/// Mojang Bedrock sample pack so users can drop those textures
/// straight in:
///
/// ```text
/// <dir>/stone.png
/// <dir>/dirt.png
/// <dir>/grass_top.png
/// <dir>/sand.png
/// <dir>/log_oak.png
/// <dir>/leaves_oak.tga    (preferred - has alpha)
/// <dir>/leaves_oak_opaque.png  (fallback)
/// ```
fn load_block_atlas_from_dir(dir: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
    let w = ATLAS_COLS * TILE_PX;
    let h = ATLAS_ROWS * TILE_PX;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut copy_tile = |spec: &TileSpec| -> Option<()> {
        // Pick the first existing file from the fallback list.
        let path = spec
            .files
            .iter()
            .map(|name| dir.join(name))
            .find(|p| p.exists())?;
        let img = image::open(&path).ok()?.to_rgba8();
        let img = if img.width() == TILE_PX && img.height() == TILE_PX {
            img
        } else {
            image::imageops::resize(&img, TILE_PX, TILE_PX, image::imageops::FilterType::Nearest)
        };
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let px = spec.col * TILE_PX + tx;
                let py = spec.row * TILE_PX + ty;
                let idx = ((py * w + px) * 4) as usize;
                let pixel = img.get_pixel(tx, ty);
                rgba[idx] = pixel.0[0];
                rgba[idx + 1] = pixel.0[1];
                rgba[idx + 2] = pixel.0[2];
                // Opaque tiles force alpha to 255 so anti-aliased
                // edges in the source can't trip the global
                // alpha-cutout shader.
                rgba[idx + 3] = if spec.keep_alpha { pixel.0[3] } else { 255 };
            }
        }
        Some(())
    };
    // Iterate every (block, face) tile in the registry. Dedup by
    // (col, row) so a tile shared by multiple faces / blocks is
    // copied once.
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for bt in BLOCK_TYPES {
        for spec in [&bt.top, &bt.bottom, &bt.sides] {
            if !seen.insert((spec.col, spec.row)) {
                continue;
            }
            if copy_tile(spec).is_none() {
                tracing::warn!(
                    block = bt.name,
                    files = ?spec.files,
                    "atlas texture missing or unreadable"
                );
                return None;
            }
        }
    }
    tracing::info!(dir = %dir.display(), "loaded block atlas from disk");
    Some((w, h, rgba))
}

/// Procedural 64x32 RGBA atlas built once at startup. Each 16x16
/// tile is the block's base colour modulated by a deterministic
/// per-pixel noise so the texture reads as voxelly fabric rather
/// than flat colour.
/// Day/night driver. Given the current `time` in seconds (modulo
/// `DAY_LENGTH_S`), return `(sun_dir, sky_color)`:
/// - `sun_dir` points TOWARDS the sun (matches `Renderer::set_sun_dir`).
///   The sun arcs from +X at sunrise through +Y at noon to -X at
///   sunset, with a small +Z tilt so shadows are never axis-aligned.
/// - `sky_color` is the linear-RGB clear colour. Three anchors:
///   deep navy at midnight, warm orange at horizon, bright cyan at noon.
fn day_night(time: f32) -> (Vec3, [f32; 3]) {
    let phase = (time / DAY_LENGTH_S).rem_euclid(1.0);
    let theta = phase * std::f32::consts::TAU;
    // Sun position on a circle in the X-Y plane, tilted slightly
    // along +Z so shadows never align perfectly with cardinal axes.
    let sun = Vec3::new(theta.cos(), theta.sin(), 0.35).normalize();
    let elev = sun.y; // -1 (under ground) .. +1 (overhead)
    let night = [0.04_f32, 0.06, 0.12];
    let horizon = [0.95_f32, 0.55, 0.30];
    let noon = [0.50_f32, 0.70, 0.92];
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    // Below horizon -> night..horizon; above -> horizon..noon.
    let sky = if elev < 0.0 {
        lerp(night, horizon, (elev + 0.2) * 5.0)
    } else {
        lerp(horizon, noon, elev * 2.0)
    };
    // Below horizon: clamp the sun direction to a "moon"-ish weak
    // angle from above so shadows still render (the shadow pass
    // doesn't model occluded suns). The PBR shader gets a much
    // dimmer normal product because the sun_intensity stays fixed
    // but the n.l term goes near zero by geometry alone.
    let sun = if elev <= 0.05 {
        Vec3::new(0.2, 0.95, 0.25).normalize()
    } else {
        sun
    };
    (sun, sky)
}

fn build_block_atlas() -> (u32, u32, Vec<u8>) {
    let w = ATLAS_COLS * TILE_PX;
    let h = ATLAS_ROWS * TILE_PX;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    // Paint every tile referenced by BLOCK_TYPES once. The
    // procedural fallback approximates the block's albedo with
    // per-pixel noise so the texture reads as voxelly fabric
    // rather than flat colour. Each tile picks the colour of the
    // first BlockType that points at it.
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let paint = |rgba: &mut Vec<u8>, col: u32, row: u32, color: [u8; 3]| {
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let px = col * TILE_PX + tx;
                let py = row * TILE_PX + ty;
                let idx = ((py * w + px) * 4) as usize;
                let n =
                    ((tx.wrapping_mul(31) ^ ty.wrapping_mul(17)) % 64) as i32 - 32;
                let bias = (n as f32 / 32.0) * 24.0;
                rgba[idx] = (color[0] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = (color[1] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = (color[2] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = 255;
            }
        }
    };
    let f2u = |v: f32| (v * 255.0).clamp(0.0, 255.0) as u8;
    for bt in BLOCK_TYPES {
        let base = [f2u(bt.color[0]), f2u(bt.color[1]), f2u(bt.color[2])];
        for spec in [&bt.top, &bt.bottom, &bt.sides] {
            if seen.insert((spec.col, spec.row)) {
                paint(&mut rgba, spec.col, spec.row, base);
            }
        }
    }
    (w, h, rgba)
}

fn generate_chunk_at(coord: ChunkCoord) -> Chunk {
    match WorldGen::current() {
        WorldGen::Classic => gen_classic(coord, 1.0),
        WorldGen::Amplified => gen_classic(coord, 2.5),
        WorldGen::Flat => gen_flat(coord),
        WorldGen::Islands => gen_islands(coord),
    }
}

/// Pancake-flat terrain at y=8: one row of grass, three rows of
/// dirt, stone below. Empty above. Trees disabled.
fn gen_flat(coord: ChunkCoord) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|_x, y, _z| {
        let wy = origin.y as i32 + y as i32;
        match wy {
            n if n > 8 => BlockId::AIR,
            8 => BlockId(3),     // grass
            5..=7 => BlockId(2), // dirt
            _ => BlockId(1),     // stone
        }
    });
    chunk
}

/// Sparse-island terrain: only where 2D noise exceeds a threshold
/// does land rise; everywhere else is bottomless air. Trees grow
/// on the island tops.
fn gen_islands(coord: ChunkCoord) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    let mask = |wx: f32, wz: f32| -> f32 {
        // Two octaves of sin/cos; treat the result as height
        // potential. Anything < threshold = ocean (air).
        let a = (wx * 0.04).sin() + (wz * 0.05).cos();
        let b = (wx * 0.11 + 17.0).sin() * 0.5 + (wz * 0.13 + 9.0).cos() * 0.5;
        a + b * 0.7
    };
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let m = mask(wx, wz);
        if m <= 0.55 {
            return BlockId::AIR;
        }
        let h = 10 + ((m - 0.55) * 18.0).round() as i32;
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            return BlockId::AIR;
        }
        if wy == h {
            BlockId(3) // grass crown
        } else if wy >= h - 2 {
            BlockId(2) // dirt
        } else {
            BlockId(1) // stone core
        }
    });
    chunk
}

/// Classic biome-driven terrain. `amp_scale` lets the Amplified
/// preset reuse this generator with a 2.5x vertical multiplier.
fn gen_classic(coord: ChunkCoord, amp_scale: f32) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let height_at = |wx: f32, wz: f32, biome: Biome| -> i32 {
        let base = match biome {
            Biome::Desert => 12.0,
            Biome::Plains => 18.0,
            Biome::Mountain => 30.0,
        };
        let amp = (match biome {
            Biome::Desert => 3.0,
            Biome::Plains => 6.0,
            Biome::Mountain => 18.0,
        }) * amp_scale;
        let h = base
            + ((wx * 0.13).sin() * 0.5 + (wz * 0.17).cos() * 0.5) * amp
            + ((wx + wz) * 0.07).sin() * amp * 0.4;
        h.round() as i32
    };
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let biome = biome_at(wx, wz);
        let h = height_at(wx, wz, biome);
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            return BlockId::AIR;
        }
        let surface = match biome {
            Biome::Desert => BlockId(4),
            Biome::Plains => BlockId(3),
            Biome::Mountain => BlockId(1),
        };
        let near_surface = match biome {
            Biome::Desert => BlockId(4),
            Biome::Plains => BlockId(2),
            Biome::Mountain => BlockId(1),
        };
        if wy == h {
            surface
        } else if wy >= h - 3 {
            near_surface
        } else {
            BlockId(1)
        }
    });
    // Tree pass: in plains biomes drop a small oak with wood pillar
    // + a leafy canopy. We loop over (-2 .. CHUNK_SIZE+2) so trees
    // anchored just outside the chunk still drop their leaves into
    // this chunk - otherwise canopies that overhang the boundary
    // would clip. Each block is only written when its `(lx, lz)`
    // pair sits inside the current chunk.
    let cs = CHUNK_SIZE as i32;
    let set_in_chunk = |chunk: &mut Chunk, lx: i32, ly: i32, lz: i32, block: BlockId| {
        if lx < 0 || lx >= cs || lz < 0 || lz >= cs || ly < 0 || ly >= cs {
            return;
        }
        chunk.set(lx as usize, ly as usize, lz as usize, block);
    };
    for x in -2..(cs + 2) {
        for z in -2..(cs + 2) {
            let wx = origin.x as i32 + x;
            let wz = origin.z as i32 + z;
            let biome = biome_at(wx as f32 + 0.5, wz as f32 + 0.5);
            if biome != Biome::Plains {
                continue;
            }
            if hash2(wx, wz) % 64 != 7 {
                continue;
            }
            let ground = height_at(wx as f32 + 0.5, wz as f32 + 0.5, biome);
            // Trunk: 4 wood blocks above the surface voxel.
            for ty in 1..=4 {
                let wy = ground + ty;
                let local_y = wy - origin.y as i32;
                set_in_chunk(&mut chunk, x, local_y, z, BlockId(5));
            }
            // Canopy: two 5x5 layers (corners trimmed) at trunk
            // top -1 / 0, a 3x3 cap above, and a single top block.
            for layer in 0i32..2 {
                let wy = ground + 3 + layer;
                let local_y = wy - origin.y as i32;
                for dx in -2i32..=2 {
                    for dz in -2i32..=2 {
                        if dx.abs() == 2 && dz.abs() == 2 {
                            continue;
                        }
                        if dx == 0 && dz == 0 && layer == 0 {
                            continue;
                        }
                        set_in_chunk(&mut chunk, x + dx, local_y, z + dz, BlockId(6));
                    }
                }
            }
            {
                let wy = ground + 5;
                let local_y = wy - origin.y as i32;
                for dx in -1i32..=1 {
                    for dz in -1i32..=1 {
                        set_in_chunk(&mut chunk, x + dx, local_y, z + dz, BlockId(6));
                    }
                }
            }
            // Single block top.
            {
                let wy = ground + 6;
                let local_y = wy - origin.y as i32;
                set_in_chunk(&mut chunk, x, local_y, z, BlockId(6));
            }
        }
    }
    chunk
}

/// Player AABB at `pos`, where `pos.y` is the eye (head) height.
fn body_aabb(pos: Vec3) -> (Vec3, Vec3) {
    (
        Vec3::new(
            pos.x - PLAYER_RADIUS,
            pos.y - PLAYER_HEIGHT,
            pos.z - PLAYER_RADIUS,
        ),
        Vec3::new(pos.x + PLAYER_RADIUS, pos.y, pos.z + PLAYER_RADIUS),
    )
}

/// Returns true if `pos`'s AABB overlaps any solid voxel.
fn aabb_intersects_solid(world: &World, pos: Vec3) -> bool {
    let (min, max) = body_aabb(pos);
    let x0 = min.x.floor() as i32;
    let x1 = (max.x - COLLISION_EPSILON).floor() as i32;
    let y0 = min.y.floor() as i32;
    let y1 = (max.y - COLLISION_EPSILON).floor() as i32;
    let z0 = min.z.floor() as i32;
    let z1 = (max.z - COLLISION_EPSILON).floor() as i32;
    for y in y0..=y1 {
        for z in z0..=z1 {
            for x in x0..=x1 {
                if !world.get([x, y, z]).is_air() {
                    return true;
                }
            }
        }
    }
    false
}

/// Move the player AABB by `delta`, resolving collision with solid
/// voxels along each axis separately. Returns the new position, plus
/// flags indicating whether the player landed on a floor (negative
/// Y resolve) or bonked their head (positive Y resolve).
/// Slab-test ray-AABB intersection. Returns the parametric `t`
/// of the entry point along `dir` (assumed approximately
/// unit-length, but any positive length works) when the ray hits
/// the box; `None` if it misses. Re-used by the mob picker.
/// Build a 1x1x1 cube mesh with per-face UVs targeting the MC
/// sheep head texture layout. The face strip lives in the top-
/// left of a 64-wide texture; each 8x8 tile maps to one face of
/// an 8x8x8 cube unwrap:
///
/// ```text
///                u
///        +---+---+---+
///        | T | B |   |     0..8: -  8..16: top  16..24: bottom
///   v    +---+---+---+---+
///        | R | F | L | K |   right, front, left, back
///        +---+---+---+---+
/// ```
///
/// `tex_w` / `tex_h` are the source texture dimensions; UVs are
/// normalised against them so the same layout works for Java
/// (64x32) and Bedrock (64x64) sheep textures.
fn build_sheep_head_mesh(tex_w: u32, tex_h: u32) -> (Vec<MeshVertex>, Vec<u32>) {
    let tw = tex_w as f32;
    let th = tex_h as f32;
    // Helper: convert pixel coordinates to normalised UV.
    let uv = |u: u32, v: u32| -> [f32; 2] { [u as f32 / tw, v as f32 / th] };
    let mut verts: Vec<MeshVertex> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);
    let mut add_face = |corners: [[f32; 3]; 4],
                       normal: [f32; 3],
                       uv0: [f32; 2],
                       uv1: [f32; 2],
                       uv2: [f32; 2],
                       uv3: [f32; 2]| {
        let base = verts.len() as u32;
        let uvs = [uv0, uv1, uv2, uv3];
        for i in 0..4 {
            verts.push(MeshVertex {
                position: corners[i],
                normal,
                uv: uvs[i],
            });
        }
        // Engine cube pipeline uses CCW front-face but the
        // `cube_geometry` reference winds quads as
        // `[0, 2, 1, 0, 3, 2]` when corners are listed
        // bottom-left -> bottom-right -> top-right -> top-left
        // viewed from outside. Matching that here so back-face
        // culling hides the interior of the head, not the outside.
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    };
    // Cube corners centred on the origin, edge length 1. Vertex
    // order per face is bottom-left, bottom-right, top-right,
    // top-left when viewed from outside the cube. UV (0,0) is
    // top-left of the texture, (1,1) is bottom-right - so the
    // matching corner UV order is `(u0,v1)`, `(u1,v1)`, `(u1,v0)`,
    // `(u0,v0)`.
    let h = 0.5_f32;
    // Bedrock sheep.geo.json: head bone has cube size [6, 6, 8] at
    // texture origin (0, 0). That's MC box-uv layout, NOT a plain
    // 8x8 face strip. The unwrap (with `d` = depth, `w` = width,
    // `h` = height):
    //
    //   ```
    //   (d, 0)         top  size w x d  -> (8, 0)..(14, 8)
    //   (d+w, 0)       bottom size w x d -> (14, 0)..(20, 8)
    //   (0, d)         right size d x h -> (0, 8)..(8, 14)
    //   (d, d)         front size w x h -> (8, 8)..(14, 14)
    //   (d+w, d)       left  size d x h -> (14, 8)..(22, 14)
    //   (d+w+d, d)     back  size w x h -> (22, 8)..(28, 14)
    //   ```
    //
    // Our render orientation has the sheep facing +X (the head's
    // depth axis runs along world X). We map MC's "front" (face)
    // tile onto our +X face so the eyes / mouth follow the
    // sheep's movement direction.
    //
    // -Z face: head's right side (MC east). Tile (0, 8)..(8, 14).
    add_face(
        [[-h, -h, -h], [h, -h, -h], [h, h, -h], [-h, h, -h]],
        [0.0, 0.0, -1.0],
        uv(0, 14),
        uv(8, 14),
        uv(8, 8),
        uv(0, 8),
    );
    // +Z face: head's left side (MC west). Tile (14, 8)..(22, 14).
    add_face(
        [[h, -h, h], [-h, -h, h], [-h, h, h], [h, h, h]],
        [0.0, 0.0, 1.0],
        uv(14, 14),
        uv(22, 14),
        uv(22, 8),
        uv(14, 8),
    );
    // +X face: the sheep's FACE. MC "front" tile (8, 8)..(14, 14).
    add_face(
        [[h, -h, -h], [h, -h, h], [h, h, h], [h, h, -h]],
        [1.0, 0.0, 0.0],
        uv(8, 14),
        uv(14, 14),
        uv(14, 8),
        uv(8, 8),
    );
    // -X face: back of head. MC "back" tile (22, 8)..(28, 14).
    add_face(
        [[-h, -h, h], [-h, -h, -h], [-h, h, -h], [-h, h, h]],
        [-1.0, 0.0, 0.0],
        uv(22, 14),
        uv(28, 14),
        uv(28, 8),
        uv(22, 8),
    );
    // Top face. MC "top" tile (8, 0)..(14, 8).
    add_face(
        [[-h, h, -h], [h, h, -h], [h, h, h], [-h, h, h]],
        [0.0, 1.0, 0.0],
        uv(8, 8),
        uv(14, 8),
        uv(14, 0),
        uv(8, 0),
    );
    // Bottom face. MC "bottom" tile (14, 0)..(20, 8).
    add_face(
        [[-h, -h, h], [h, -h, h], [h, -h, -h], [-h, -h, -h]],
        [0.0, -1.0, 0.0],
        uv(14, 8),
        uv(20, 8),
        uv(20, 0),
        uv(14, 0),
    );
    (verts, indices)
}

fn ray_aabb(origin: [f32; 3], dir: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<f32> {
    let mut tmin = 0.0_f32;
    let mut tmax = f32::INFINITY;
    for axis in 0..3 {
        let inv = 1.0 / dir[axis];
        let mut t1 = (min[axis] - origin[axis]) * inv;
        let mut t2 = (max[axis] - origin[axis]) * inv;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
        if tmax < tmin {
            return None;
        }
    }
    if tmax < 0.0 {
        None
    } else {
        Some(tmin.max(0.0))
    }
}

fn sweep_aabb(world: &World, pos: Vec3, delta: Vec3) -> (Vec3, bool, bool) {
    let mut new_pos = pos;
    let mut hit_floor = false;
    let mut hit_ceiling = false;

    // X axis.
    let candidate = Vec3::new(new_pos.x + delta.x, new_pos.y, new_pos.z);
    if aabb_intersects_solid(world, candidate) {
        if delta.x > 0.0 {
            let wall = (candidate.x + PLAYER_RADIUS).floor();
            new_pos.x = wall - PLAYER_RADIUS - COLLISION_EPSILON;
        } else if delta.x < 0.0 {
            let wall = (candidate.x - PLAYER_RADIUS).ceil();
            new_pos.x = wall + PLAYER_RADIUS + COLLISION_EPSILON;
        }
    } else {
        new_pos.x = candidate.x;
    }

    // Y axis.
    let candidate = Vec3::new(new_pos.x, new_pos.y + delta.y, new_pos.z);
    if aabb_intersects_solid(world, candidate) {
        if delta.y > 0.0 {
            let wall = candidate.y.floor();
            new_pos.y = wall - COLLISION_EPSILON;
            hit_ceiling = true;
        } else if delta.y < 0.0 {
            let wall = (candidate.y - PLAYER_HEIGHT).ceil();
            new_pos.y = wall + PLAYER_HEIGHT + COLLISION_EPSILON;
            hit_floor = true;
        }
    } else {
        new_pos.y = candidate.y;
    }

    // Z axis.
    let candidate = Vec3::new(new_pos.x, new_pos.y, new_pos.z + delta.z);
    if aabb_intersects_solid(world, candidate) {
        if delta.z > 0.0 {
            let wall = (candidate.z + PLAYER_RADIUS).floor();
            new_pos.z = wall - PLAYER_RADIUS - COLLISION_EPSILON;
        } else if delta.z < 0.0 {
            let wall = (candidate.z - PLAYER_RADIUS).ceil();
            new_pos.z = wall + PLAYER_RADIUS + COLLISION_EPSILON;
        }
    } else {
        new_pos.z = candidate.z;
    }

    (new_pos, hit_floor, hit_ceiling)
}

/// Helper to render a slot with full drag&drop integration.
///
/// - `kind` identifies this slot (used as the DnD payload + as the
///   drop target id).
/// - When the slot is non-empty and `draggable` is true, the cell
///   becomes a drag source (the widget paints itself at the
///   cursor while held).
/// - Every slot is a drop target. On release, the action is
///   appended to `actions` for the post-pass to apply.
/// - Returns the underlying widget response so callers can layer
///   click handlers / tooltips on top.
#[allow(clippy::too_many_arguments)]
fn slot_with_dnd(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    kind: SlotKind,
    selected: bool,
    is_break_slot: bool,
    draggable: bool,
    actions: &mut Vec<InvAction>,
) -> egui::Response {
    // Scope the drag-source id through the parent ui so the same
    // SlotKind can appear in multiple layers (e.g. the hotbar Area
    // + the hotbar mirror inside the Inventory window) without
    // egui flagging it as a layer-id switch on the same widget.
    let id = ui.make_persistent_id(("inv_slot", kind));
    let resp = if draggable && slot.is_some() {
        ui.dnd_drag_source(id, kind, |ui| {
            draw_slot_widget(
                ui,
                slot,
                atlas,
                selected,
                is_break_slot,
                egui::Sense::click_and_drag(),
            )
        })
        .response
    } else {
        draw_slot_widget(
            ui,
            slot,
            atlas,
            selected,
            is_break_slot,
            egui::Sense::click(),
        )
    };
    if let Some(from) = resp.dnd_release_payload::<SlotKind>() {
        let from = *from;
        if from != kind {
            // Modifier mapping:
            //   Ctrl/Cmd held = move exactly one item (good for
            //                   filling craft slots).
            //   Shift held    = move half the stack (split-in-half
            //                   matches Minecraft's right-click
            //                   pickup behaviour).
            //   No modifier   = move the whole stack.
            let (shift, ctrl) =
                ui.input(|i| (i.modifiers.shift, i.modifiers.command));
            let split = if ctrl {
                SplitMode::One
            } else if shift {
                SplitMode::Half
            } else {
                SplitMode::All
            };
            actions.push(InvAction::Drag { from, to: kind, split });
        }
    }
    // Hover tooltip: show the block name + stack count. The wrapped
    // response is dropped silently because all we want is the side
    // effect of registering the tooltip.
    if let Some((_, count, _, name)) = slot {
        let resp = if count > 1 {
            resp.on_hover_text(format!("{name} x{count}"))
        } else {
            resp.on_hover_text(name)
        };
        return resp;
    }
    resp
}

/// Draw one inventory cell. Returns the response so the caller
/// can wire click handlers / drag detection / tooltips on top.
///
/// - `slot`: `Some((color, count, uv_rect))` for a stack, `None`
///   for an empty cell.
/// - `atlas`: optional egui texture id of the block atlas. When
///   set the slot paints the matching tile; when `None` it falls
///   back to a flat color swatch from `slot.0`.
/// - `selected`: outlines the cell in gold (active hotbar slot).
/// - `is_break_slot`: draws the white "X" overlay marking the
///   creative break tool (only meaningful for hotbar slot 0).
/// - `sense`: the egui interaction model (click, click_and_drag,
///   etc.) the underlying response should report.
#[allow(clippy::too_many_arguments)]
fn draw_slot_widget(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    selected: bool,
    is_break_slot: bool,
    sense: egui::Sense,
) -> egui::Response {
    let size = egui::vec2(40.0, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter_at(rect);
    // Slot background: dark gray when empty, otherwise either the
    // atlas tile (preferred) or a flat color swatch.
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(24));
    if let Some((c, _, uv, _)) = slot {
        match atlas {
            Some(tex_id) => {
                let uv_rect = egui::Rect::from_min_max(
                    egui::pos2(uv[0], uv[1]),
                    egui::pos2(uv[2], uv[3]),
                );
                // Inset by 2px so the outline frame stays visible.
                let icon_rect = rect.shrink(2.0);
                painter.image(tex_id, icon_rect, uv_rect, egui::Color32::WHITE);
            }
            None => {
                let fill = egui::Color32::from_rgb(
                    (c[0] * 255.0) as u8,
                    (c[1] * 255.0) as u8,
                    (c[2] * 255.0) as u8,
                );
                painter.rect_filled(rect.shrink(2.0), 2.0, fill);
            }
        }
    }
    let outline = if selected {
        egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 220, 80))
    } else if resp.hovered() {
        egui::Stroke::new(2.0, egui::Color32::from_gray(200))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(120))
    };
    painter.rect_stroke(rect, 4.0, outline, egui::StrokeKind::Inside);
    if slot.is_none() && is_break_slot {
        // Draw an "X" to mark the break (empty hand) slot.
        let pad = 10.0;
        let stroke = egui::Stroke::new(2.0, egui::Color32::from_gray(190));
        painter.line_segment(
            [
                egui::pos2(rect.min.x + pad, rect.min.y + pad),
                egui::pos2(rect.max.x - pad, rect.max.y - pad),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(rect.max.x - pad, rect.min.y + pad),
                egui::pos2(rect.min.x + pad, rect.max.y - pad),
            ],
            stroke,
        );
    }
    if let Some((_, count, _, _)) = slot {
        // Stack count badge in the bottom-right corner. Hide for
        // singletons because Minecraft does the same.
        if count > 1 {
            let pos = egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0);
            // Drop shadow first for readability over light tiles.
            painter.text(
                pos + egui::vec2(1.0, 1.0),
                egui::Align2::RIGHT_BOTTOM,
                count.to_string(),
                egui::FontId::proportional(14.0),
                egui::Color32::BLACK,
            );
            painter.text(
                pos,
                egui::Align2::RIGHT_BOTTOM,
                count.to_string(),
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            );
        }
    }
    resp
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Parse `--server [addr]` / `--client <addr>` / no-arg offline.
    let mut args = std::env::args().skip(1);
    let mut net = NetRole::Offline;
    let mut title_suffix = String::new();
    let mut texture_dir: Option<std::path::PathBuf> = None;
    let mut sounds_dir: Option<std::path::PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--textures" => {
                let p = args.next().unwrap_or_default();
                texture_dir = Some(std::path::PathBuf::from(p));
            }
            "--sounds" => {
                let p = args.next().unwrap_or_default();
                sounds_dir = Some(std::path::PathBuf::from(p));
            }
            "--worldgen" => {
                let m = args.next().unwrap_or_default();
                match WorldGen::from_str(&m) {
                    Some(g) => {
                        info!(mode = ?g, "worldgen mode set");
                        WORLDGEN
                            .store(g as u8, std::sync::atomic::Ordering::Relaxed);
                    }
                    None => {
                        warn!(arg = %m, "unknown worldgen mode (classic|flat|islands|amplified)");
                    }
                }
            }
            "--server" => {
                let addr = args.next().unwrap_or_else(|| "0.0.0.0:7777".to_string());
                match NetServer::bind(&addr) {
                    Ok(s) => {
                        info!(addr = %addr, "voxel server bound");
                        title_suffix = format!(" [SERVER {addr}]");
                        net = NetRole::Server {
                            server: s,
                            poses: HashMap::new(),
                            inventories: HashMap::new(),
                            pending_inv_sends: HashSet::new(),
                        };
                    }
                    Err(err) => {
                        error!(error = %err, addr = %addr, "server bind failed");
                        return ExitCode::from(2);
                    }
                }
            }
            "--client" => {
                let addr = args
                    .next()
                    .unwrap_or_else(|| "127.0.0.1:7777".to_string());
                match NetClient::connect(&addr, "voxel-mp-client") {
                    Ok(c) => {
                        info!(addr = %addr, "voxel client connecting");
                        title_suffix = format!(" [CLIENT -> {addr}]");
                        net = NetRole::Client {
                            client: c,
                            remote: HashMap::new(),
                            pending_edits: Vec::new(),
                            pending_inv_actions: Vec::new(),
                        };
                    }
                    Err(err) => {
                        error!(error = %err, addr = %addr, "client connect failed");
                        return ExitCode::from(3);
                    }
                }
            }
            other => {
                warn!(arg = %other, "ignoring unknown argv");
            }
        }
    }
    info!(role = %net.label(), "voxel_demo_mp starting");

    let config = WindowConfig {
        title: format!("InfEngine - voxel_demo_mp{title_suffix}"),
        width: 1280,
        height: 720,
        resizable: true,
    };
    let mut voxel = Voxel::new_with_net(net);
    // Default asset resolution. We probe two locations in order:
    //
    //   1. `<exe-dir>/assets/...` - ships next to the binary in
    //      a release zip. Standard game-distro layout: drop the
    //      `assets/` folder beside `voxel_demo_mp.exe`.
    //   2. `<CARGO_MANIFEST_DIR>/assets/...` - dev-mode fallback
    //      so `cargo run` from the source tree still works
    //      without copying anything around.
    //
    // Both paths are checked separately for `textures/blocks` and
    // `sounds`, so a release with only one subfolder still picks
    // it up. Missing both directories simply leaves the field
    // `None` and the demo falls back to procedural atlas + synth
    // audio.
    fn resolve_asset_subdir(rel: &[&str]) -> Option<std::path::PathBuf> {
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let mut p = dir.to_path_buf();
            for seg in rel {
                p.push(seg);
            }
            candidates.push(p);
        }
        let mut dev = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for seg in rel {
            dev.push(seg);
        }
        candidates.push(dev);
        candidates.into_iter().find(|p| p.exists())
    }
    voxel.texture_dir = texture_dir
        .or_else(|| resolve_asset_subdir(&["assets", "textures", "blocks"]));
    voxel.sounds_dir = sounds_dir
        .or_else(|| resolve_asset_subdir(&["assets", "sounds"]));
    info!(
        textures = ?voxel.texture_dir,
        sounds = ?voxel.sounds_dir,
        "resolved asset paths"
    );
    if let Err(err) = run(config, voxel) {
        error!(error = %err, "event loop failed");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
