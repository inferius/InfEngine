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

/// Two-sample interpolation buffer for one remote player's pose.
/// The render path samples at `now - INTERP_DELAY` so the most
/// recent snapshot is one tick "in the future" and we always have
/// the surrounding pair to lerp between.
#[derive(Debug, Clone, Copy)]
struct RemoteInterp {
    prev: PlayerMsg,
    current: PlayerMsg,
    prev_t: std::time::Instant,
    current_t: std::time::Instant,
}

/// Render delay (seconds) applied to the interpolation buffer. A
/// little less than a typical send interval - lets the lerp always
/// have both endpoints without showing a noticeable lag.
const INTERP_DELAY: f32 = 0.10;

impl RemoteInterp {
    fn new(p: PlayerMsg) -> Self {
        let now = std::time::Instant::now();
        Self {
            prev: p,
            current: p,
            prev_t: now,
            current_t: now,
        }
    }

    fn push(&mut self, p: PlayerMsg) {
        let now = std::time::Instant::now();
        self.prev = self.current;
        self.prev_t = self.current_t;
        self.current = p;
        self.current_t = now;
    }

    /// Pose interpolated at "now minus the render delay", clamped
    /// to the available buffer so we don't extrapolate past the
    /// last snapshot when the sender pauses.
    fn sample(&self, now: std::time::Instant) -> PlayerMsg {
        let render_t = now - std::time::Duration::from_secs_f32(INTERP_DELAY);
        let span = self
            .current_t
            .saturating_duration_since(self.prev_t)
            .as_secs_f32()
            .max(1e-4);
        let into = render_t
            .saturating_duration_since(self.prev_t)
            .as_secs_f32();
        let t = (into / span).clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        // Shortest-arc lerp for yaw + pitch so wrap-around (PI -> -PI)
        // doesn't whip the camera the long way around.
        let lerp_angle = |a: f32, b: f32| {
            let mut d = b - a;
            let tau = std::f32::consts::TAU;
            while d > std::f32::consts::PI {
                d -= tau;
            }
            while d < -std::f32::consts::PI {
                d += tau;
            }
            a + d * t
        };
        PlayerMsg {
            id: self.current.id,
            pos: [
                lerp(self.prev.pos[0], self.current.pos[0]),
                lerp(self.prev.pos[1], self.current.pos[1]),
                lerp(self.prev.pos[2], self.current.pos[2]),
            ],
            yaw: lerp_angle(self.prev.yaw, self.current.yaw),
            pitch: lerp_angle(self.prev.pitch, self.current.pitch),
        }
    }
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
    /// Current HP rounded to the nearest integer. Drives the
    /// floating health bar render on every peer. `serde(default)`
    /// keeps the wire format back-compatible with older v1
    /// snapshots that didn't include health.
    #[serde(default)]
    health: u8,
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
    /// Seconds remaining in the death-fade animation. > 0 means
    /// the mob is dying: rendered with falling tint + tilted
    /// orientation, no longer wanders or collides. On 0 the
    /// server emits `vx.mob_remove` + evicts.
    dying: f32,
    /// Per-mob attack cooldown timer. Currently used by zombies:
    /// when the value reaches zero the mob deals contact damage
    /// to whichever player it's standing next to and resets to
    /// `ZOMBIE_ATTACK_COOLDOWN`.
    attack_cd: f32,
}

/// Length of the death-fade in seconds. Both server and clients
/// run the same countdown so the animation reads identically on
/// every peer.
const MOB_DEATH_FADE: f32 = 0.55;

/// In-progress mining session. The player has been holding LMB on
/// `voxel` for `progress` seconds; the block falls once `progress`
/// reaches `total`. Tracking the original block lets us reset the
/// session if someone else breaks it first or if the target shifts.
#[derive(Debug, Clone, Copy)]
struct MiningState {
    voxel: [i32; 3],
    progress: f32,
    total: f32,
    block: BlockId,
    /// Time (seconds) since the last dig sound + crack particle
    /// burst. Reset to 0 each time the periodic effect fires.
    fx_cooldown: f32,
}

/// Seconds required to fully mine each block kind. Higher = tougher
/// material. Air falls through to a tiny "miss" delay so the swing
/// still produces feedback.
fn block_hardness(block: BlockId) -> f32 {
    match block.0 {
        0 => 0.05,        // air (no-op swing)
        1 | 8 => 1.6,     // stone, cobble
        2 | 3 => 0.55,    // dirt, grass
        4 => 0.5,         // sand
        5 => 1.0,         // wood log
        6 => 0.25,        // leaves
        7 => 0.9,         // planks
        9 => 0.45,        // glass
        10 => 0.1,        // torch
        11 => 0.6,        // wool
        12 => 0.4,        // meat
        13 => 2.4,        // coal ore
        14 => 3.2,        // iron ore
        _ => 0.8,
    }
}

const MOB_CAP_DEFAULT: usize = 12;
const MOB_SPEED: f32 = 1.2;
const MOB_MAX_HEALTH: f32 = 8.0;
const MOB_HIT_DAMAGE: f32 = 2.0;
/// Mob kind discriminator carried in `MobMsg.kind` + tagged on
/// every `MobState`. Sheep are passive wanderers, zombies chase
/// the nearest player and deal contact damage at night.
const KIND_SHEEP: u8 = 0;
const KIND_ZOMBIE: u8 = 1;
/// Per-kind balance knobs.
const ZOMBIE_SPEED: f32 = 1.8;
const ZOMBIE_MAX_HEALTH: f32 = 14.0;
const ZOMBIE_TOUCH_DAMAGE: f32 = 2.5;
const ZOMBIE_ATTACK_COOLDOWN: f32 = 0.9;
const ZOMBIE_ATTACK_RANGE: f32 = 1.4;
const ZOMBIE_SIGHT_RANGE: f32 = 22.0;
/// Squared distance from the nearest player past which a mob is
/// despawned. Stops sheep + zombies from piling up in unloaded
/// chunks the player will never visit again.
const MOB_DESPAWN_DIST_SQ: f32 = 80.0 * 80.0;
/// Seconds the zombie body offsets forward after striking. Drives
/// the lunge animation timer.
const MOB_LUNGE_TIME: f32 = 0.30;
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

/// Client -> server: request to damage another player. The server
/// validates the attacker's reach + forwards `DamageMsg` to the
/// target client.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PvPHitMsg {
    target_id: u32,
}

/// Server -> client: "you took N HP of damage". Sent to a single
/// target via `send_bytes_to_client`; the client applies it to
/// their local `player_health` + flashes / plays the hurt SFX.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct DamageMsg {
    amount: f32,
}

/// Server -> client: "snap your local position here". Used by
/// the host's /summon command to drag a remote peer to the
/// host's location. The receiving client zeroes its velocity to
/// avoid synthesizing fall damage from the jump.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct TeleportMsg {
    pos: [f32; 3],
}

const PVP_DAMAGE: f32 = 1.5;
const PVP_REACH: f32 = 5.5;

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
    /// Host player health (0..=PLAYER_MAX_HEALTH). `serde(default)`
    /// fills with `PLAYER_MAX_HEALTH` when loading older saves
    /// that didn't include HP.
    #[serde(default = "default_player_health")]
    player_health: f32,
    /// Host kill counter persisted across sessions.
    #[serde(default)]
    kill_count: u32,
    /// World seed used by the noise generators. Older saves
    /// without this field load with seed 0, which the loader
    /// upgrades to a fresh random value so terrain doesn't snap
    /// to the deterministic seed-0 layout.
    #[serde(default)]
    world_seed: u32,
}

fn default_player_health() -> f32 {
    PLAYER_MAX_HEALTH
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
/// Maximum player hit points. 20 mirrors the Minecraft scale (10
/// hearts of 2 HP each).
const PLAYER_MAX_HEALTH: f32 = 20.0;
const STAMINA_MAX: f32 = 100.0;
const STAMINA_DRAIN: f32 = 20.0;
const STAMINA_REGEN: f32 = 18.0;
const STAMINA_REACTIVATE: f32 = 25.0;
/// Free-fall distance, in voxels, before fall damage kicks in. A
/// 3-block drop is harmless; anything taller deals
/// `fall_distance - SAFE_FALL` HP.
const SAFE_FALL: f32 = 3.0;
/// Seconds without damage before HP starts ticking up.
const REGEN_DELAY: f32 = 4.0;
/// HP regenerated per second once `regen_accum` exceeds the delay.
/// Rate of 0.5 HP/s + 4 s delay = a 5 s grace + 40 s to fill from
/// near-death to full.
const REGEN_RATE: f32 = 0.5;
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
    /// Render distance in chunks (Chebyshev). The unload radius is
    /// computed as `view_distance + 2` so the player can wobble on
    /// the boundary without thrashing chunk loads.
    view_distance: u8,
    /// Camera head bob while walking. Off-by-default-friendly:
    /// nauseating for some players, polish for others.
    head_bob: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.6,
            lock_cursor: true,
            hide_cursor: true,
            show_fps: true,
            view_distance: VIEW_DISTANCE_CHUNKS as u8,
            head_bob: true,
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
    /// Previous-frame state of the F key. Rising edge consumes
    /// one piece of food from the inventory + restores HP.
    f_prev: bool,
    /// F1 toggles HUD visibility. Useful for clean screenshots
    /// and stress-testing the renderer without overlay overhead.
    hud_visible: bool,
    f1_prev: bool,
    /// True while the chat input box is open. Gates player input
    /// like the inventory / MP menu overlays.
    chat_open: bool,
    /// Buffer the chat input box edits while open.
    chat_input: String,
    /// Recent chat lines: `(arrival_instant, formatted_text)`.
    /// Older entries fade out + drop after a few seconds.
    chat_log: Vec<(std::time::Instant, String)>,
    /// Player's previously sent chat lines (most recent last).
    /// Used by Up/Down arrow scrolling in the chat input box.
    /// Capped at 20 entries so very long sessions don't grow it
    /// unbounded.
    chat_history: Vec<String>,
    /// Index of the currently-shown history entry, or `None` when
    /// the player is typing fresh text. Reset whenever a new line
    /// is sent.
    chat_history_idx: Option<usize>,
    /// Previous-frame state of the T key for rising-edge open.
    t_prev: bool,
    lmb_pending: bool,
    rmb_pending: bool,
    /// Level state of the mouse buttons (set on Pressed events,
    /// cleared on Released). Drives the hold-to-mine loop - the
    /// edge-triggered `lmb_pending` still routes single clicks to
    /// melee attacks on mobs.
    lmb_held: bool,
    rmb_held: bool,
    /// Active mining session. `None` when the player isn't holding
    /// LMB on a voxel; populated with the targeted cell + the time
    /// elapsed / required to break the block. Cancels when LMB
    /// releases, the aim moves to a different voxel, or the
    /// targeted voxel disappears (someone else broke it first).
    mining: Option<MiningState>,
    /// Current player hit points. Capped at `PLAYER_MAX_HEALTH`.
    /// Reaches zero = death, which respawns at origin + wipes the
    /// storage rows the same way the fall-out path already does.
    player_health: f32,
    /// World-Y the player last left solid ground at. `None` when
    /// grounded; populated on the rising edge of an airborne
    /// transition + read on landing for fall-damage calculation.
    fall_start_y: Option<f32>,
    /// Seconds remaining in the red screen-flash overlay that
    /// fires on any non-trivial damage event. Decays in update.
    hurt_flash: f32,
    /// Seconds elapsed since the last damage. Once it crosses
    /// `REGEN_DELAY`, HP starts ticking up at `REGEN_RATE` HP/s
    /// (capped at `PLAYER_MAX_HEALTH`).
    regen_accum: f32,
    /// Mobs the local player has killed this session. Bumped in
    /// `damage_mob` whenever the killing blow is attributed to the
    /// host (id 0). Surfaced in the HUD + F3 debug panel.
    kill_count: u32,
    /// Current camera vertical FOV in radians. Smoothly lerps
    /// toward a wider angle while the player sprints, snaps back
    /// when they stop. Read by both the wgpu camera + the egui
    /// HUD's screen-projection so the two stay in sync.
    fov_y_rad: f32,
    /// `true` for the last frame's "are we actually sprinting"
    /// state - used by the FOV lerp.
    sprinting: bool,
    /// Sprint stamina in 0..STAMINA_MAX. Drains while sprinting,
    /// regens when not. Reaching zero blocks the sprint speed
    /// bonus until it regens above the small reactivation
    /// threshold.
    stamina: f32,
    /// Per-remote-player interpolation buffer. We keep the
    /// previous + current `PlayerMsg` and their arrival instants
    /// so the render path can lerp between them at `now - delay`,
    /// turning a choppy pose stream (one update per net tick) into
    /// smooth motion regardless of the send rate. Both Server and
    /// Client roles write into this map - the host needs the same
    /// smoothing for the joined players it renders locally.
    remote_interp: HashMap<u32, RemoteInterp>,
    /// Local player's display name. Sent to the server as the
    /// `Hello` name (so the server's `client_name` lookup returns
    /// it), used as the chat sender tag for the host, and pushed
    /// to other clients via `vx.names`. Defaults to a random
    /// `Player####` if the user hasn't set one in settings.
    player_name: String,
    /// Latest known display name per player id. Filled from the
    /// `vx.names` roster the server broadcasts on join/leave. Used
    /// for nameplates above remote players and the Tab playerlist
    /// overlay. The local player's id always maps to
    /// `self.player_name`; remote ids come from the server.
    player_names: HashMap<u32, String>,
    /// True while Tab is held. Drives the player list overlay
    /// shown in the HUD.
    tab_held: bool,
    /// Floating "+N <block>" toasts shown on the right side of
    /// the HUD whenever the player picks something up. Entries
    /// fade out after a couple of seconds; consecutive picks of
    /// the same block collapse into a single counter.
    pickup_toasts: Vec<(std::time::Instant, BlockId, u8)>,
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
    /// Seconds since the last heartbeat thump. Drives the low-HP
    /// audio cue: fires more rapidly the closer the player is to
    /// dying. Stays at 0 above the threshold.
    heartbeat_accum: f32,
    /// Advancing phase used by the camera head-bob. Increments
    /// each frame proportionally to the player's horizontal speed
    /// when grounded; eases back toward zero off-ground. Kept
    /// separate from `day_time` so a stationary player has a
    /// stable view.
    head_bob_phase: f32,
    /// 0 = first person, 1 = third person behind, 2 = third
    /// person front. Cycled with F5 the way Minecraft does it.
    camera_mode: u8,
    f5_prev: bool,
    q_prev: bool,
    /// Player's saved home position. /sethome stores the current
    /// pose; /home teleports back. Persisted via the config file
    /// so the location survives across launches.
    home_pos: Option<[f32; 3]>,
    /// Wallclock instant the session started running. Used by the
    /// F3 debug overlay to show how long the player has been
    /// playing. Stays None until on_start runs.
    session_started: Option<std::time::Instant>,
    /// Last time the server broadcast its authoritative
    /// `day_time`. Used to throttle the broadcast cadence to
    /// roughly once per second.
    last_day_broadcast: Option<std::time::Instant>,
    /// Creative-style fly mode. Toggled by `/fly`. Disables
    /// gravity + fall damage and remaps Space / Shift to vertical
    /// thrust. Off by default so a vanilla session is unchanged.
    fly_mode: bool,
    /// `/godmode` toggle. When on, apply_player_damage is a
    /// no-op so the local player can't be hurt. Useful for
    /// testing the world without dying.
    god_mode: bool,
    /// Maximum concurrent live mob count. Tweakable via /mobcap
    /// so players on slow machines can cut the spawn pressure;
    /// defaults to `MOB_CAP_DEFAULT`.
    mob_cap: usize,
    /// `/freezetime` toggle. When on, the day_time clock stops
    /// advancing so the sun + sky stay frozen. Useful for
    /// screenshots / debugging.
    freeze_time: bool,
    /// 0 = clear, 1 = rain. Drives per-frame rain particle
    /// spawning above the player so the demo can have weather
    /// without actually wiring it into the gameplay.
    weather: u8,
    /// Names the player has muted via /mute. Chat lines whose
    /// sender tag matches one of these (case-insensitive) are
    /// dropped before they hit `chat_log`. Stored by lowercased
    /// name to keep matching trivial.
    muted_players: HashSet<String>,
    /// Seconds remaining in the current lightning flash. While
    /// positive the sky washes toward white. Decays each frame
    /// via the update tick.
    lightning_flash: f32,
    /// Seconds until the next lightning strike attempt while
    /// weather=1. Initialised to a positive value so the first
    /// strike isn't instant on /weather rain.
    next_lightning_at: f32,
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
    /// Seconds remaining in the local death-fade animation per
    /// mob. Server populates from `MobState.dying` when a kill
    /// fires; client populates on `vx.mob_event { kind: 1 }`. The
    /// render path tilts + fades the cube while the timer counts
    /// down to zero.
    mob_dying: HashMap<u32, f32>,
    /// Per-mob lunge-animation timer. Set to `MOB_LUNGE_TIME` on
    /// every melee swing (currently only zombies). Renderer reads
    /// it to push the body briefly forward + return.
    mob_lunge: HashMap<u32, f32>,
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
        let already_dying;
        {
            let m = &mut self.mobs[idx];
            already_dying = m.dying > 0.0;
            if !already_dying {
                m.health -= MOB_HIT_DAMAGE;
            }
            mob_pos = m.pos;
            killed = m.health <= 0.0 && !already_dying;
        }
        // No re-damage during the death fade.
        if already_dying {
            return;
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
        // Death: start the fade timer. Mob stays in `self.mobs`
        // (skipping wander logic) until the fade completes, then
        // `tick_mobs` evicts it + broadcasts `vx.mob_remove`. Loot
        // drops immediately so the attacker doesn't have to wait
        // for the animation.
        self.mobs[idx].dying = MOB_DEATH_FADE;
        self.mob_dying.insert(mob_id, MOB_DEATH_FADE);
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
                self.kill_count = self.kill_count.saturating_add(1);
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
        let mut to_remove: Vec<u32> = Vec::new();
        for i in 0..mobs_count {
            let mut m = self.mobs[i];
            // Dying mobs skip wander + collision: they just count
            // down the fade timer. When it hits zero the body
            // gets evicted from `mobs` + a `vx.mob_remove` goes
            // out so every client drops it from `remote_mobs`.
            if m.dying > 0.0 {
                m.dying = (m.dying - dt).max(0.0);
                self.mob_dying.insert(m.id, m.dying);
                if m.dying <= 0.0 {
                    to_remove.push(m.id);
                }
                self.mobs[i] = m;
                self.remote_mobs.insert(
                    m.id,
                    MobMsg {
                        id: m.id,
                        kind: m.kind,
                        pos: m.pos,
                        yaw: m.yaw,
                        health: m.health.max(0.0).round() as u8,
                    },
                );
                continue;
            }
            // Per-kind targeting:
            //  - Sheep wander randomly + retarget on arrival or
            //    5 s timeout.
            //  - Zombies pick the nearest player within
            //    `ZOMBIE_SIGHT_RANGE` and lock onto their pose;
            //    when no player is in range they fall back to the
            //    sheep wander logic so they don't freeze in place.
            let chase_player_pos: Option<[f32; 3]> = if m.kind == KIND_ZOMBIE {
                self.nearest_player_pos(m.pos, ZOMBIE_SIGHT_RANGE)
            } else {
                None
            };
            let speed = if m.kind == KIND_ZOMBIE {
                ZOMBIE_SPEED
            } else {
                MOB_SPEED
            };
            if let Some(p) = chase_player_pos {
                m.target = p;
                m.retarget_at = (day_time + 1.0).rem_euclid(DAY_LENGTH_S);
            }
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
                let vx = dx * inv * speed;
                let vz = dz * inv * speed;
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
            // Zombie touch damage: decrement attack cooldown each
            // frame; when in range of the host + cooldown expired,
            // hit the local player + reset. Remote-player damage
            // is server-authoritative but would need a per-id
            // damage message - skipped for the demo; the host
            // gets the attack feedback.
            if m.kind == KIND_ZOMBIE {
                m.attack_cd = (m.attack_cd - dt).max(0.0);
                let dx = self.player_pos.x - m.pos[0];
                let dy = self.player_pos.y - m.pos[1];
                let dz = self.player_pos.z - m.pos[2];
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 <= ZOMBIE_ATTACK_RANGE * ZOMBIE_ATTACK_RANGE && m.attack_cd <= 0.0 {
                    m.attack_cd = ZOMBIE_ATTACK_COOLDOWN;
                    self.apply_player_damage(ZOMBIE_TOUCH_DAMAGE);
                    // Visual lunge: bump the local timer + tell
                    // every other peer to do the same so the
                    // animation reads identically on each client.
                    self.mob_lunge.insert(m.id, MOB_LUNGE_TIME);
                    if let NetRole::Server { server, .. } = &self.net {
                        let cfg = bincode::config::standard();
                        let evt = MobEventMsg { id: m.id, kind: 2 };
                        if let Ok(buf) = bincode::serde::encode_to_vec(evt, cfg) {
                            server.broadcast_bytes("vx.mob_event", buf);
                        }
                    }
                }
            }
            self.mobs[i] = m;
            self.remote_mobs.insert(
                m.id,
                MobMsg {
                    id: m.id,
                    kind: m.kind,
                    pos: m.pos,
                    yaw: m.yaw,
                    health: m.health.max(0.0).round() as u8,
                },
            );
        }
        // Despawn pass: mobs far from every known player get
        // evicted (no point ticking + broadcasting an entity the
        // player will never see again). Dying mobs are exempt -
        // we want to finish their fade even when the player
        // walked away.
        for m in &self.mobs {
            if m.dying > 0.0 {
                continue;
            }
            let nearest = self.nearest_player_pos(m.pos, 1.0e6);
            if let Some(p) = nearest {
                let dx = p[0] - m.pos[0];
                let dy = p[1] - m.pos[1];
                let dz = p[2] - m.pos[2];
                if dx * dx + dy * dy + dz * dz > MOB_DESPAWN_DIST_SQ
                    && !to_remove.contains(&m.id)
                {
                    to_remove.push(m.id);
                }
            }
        }
        // Evict mobs whose death fade just finished, plus anything
        // the despawn pass flagged. Broadcast `vx.mob_remove` so
        // clients drop them from the render table at the same
        // moment the local sim does.
        for id in &to_remove {
            if let Some(pos) = self.mobs.iter().position(|m| m.id == *id) {
                self.mobs.swap_remove(pos);
            }
            self.remote_mobs.remove(id);
            self.mob_hurt_flash.remove(id);
            self.mob_dying.remove(id);
            self.mob_step_timers.remove(id);
            self.mob_say_timers.remove(id);
            self.mob_last_xz.remove(id);
            if let NetRole::Server { server, .. } = &self.net {
                let cfg = bincode::config::standard();
                if let Ok(buf) = bincode::serde::encode_to_vec(*id, cfg) {
                    server.broadcast_bytes("vx.mob_remove", buf);
                }
            }
        }
        // Spawn pass: if below cap + last spawn far enough behind,
        // try once. Pick a column near the origin; require its
        // surface to be grass (id 3).
        const MOB_SPAWN_PERIOD: f32 = 6.0;
        let due = ((day_time - self.last_mob_spawn_time).rem_euclid(DAY_LENGTH_S))
            > MOB_SPAWN_PERIOD;
        if self.mobs.len() < self.mob_cap && due {
            // Spawn in a ring 12..32 blocks around the host so
            // the world feels more populated as the player
            // explores. Pure origin-anchored spawns were
            // confusing once they wandered far from spawn.
            let angle = rand01() * std::f32::consts::TAU;
            let radius = 12.0 + rand01() * 20.0;
            let cx = self.player_pos.x;
            let cz = self.player_pos.z;
            let wx = (cx + angle.cos() * radius).round() as i32;
            let wz = (cz + angle.sin() * radius).round() as i32;
            // Spawn scan: probe the column ceiling so the sheep
            // lands on real ground (canopy is fine here too since
            // mobs spawn from above).
            let ceiling = (CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1;
            if let Some(sy) = self.surface_y_below(wx, wz, ceiling) {
                let top = self.world.get([wx, sy, wz]);
                if top.0 == 3 {
                    // Pick kind: zombies only spawn at night
                    // (sun_dir.y < 0) and crowd out sheep with a
                    // 60% chance during the dark hours. Daytime is
                    // sheep-only.
                    let (sun_dir, _sky) = day_night(day_time);
                    let is_night = sun_dir.y < 0.0;
                    let kind = if is_night && rand01() < 0.6 {
                        KIND_ZOMBIE
                    } else {
                        KIND_SHEEP
                    };
                    let id = next_id;
                    next_id = next_id.wrapping_add(1).max(1);
                    let pos = [wx as f32 + 0.5, sy as f32 + 1.0, wz as f32 + 0.5];
                    let mob = MobState {
                        id,
                        kind,
                        pos,
                        target: pos,
                        yaw: 0.0,
                        retarget_at: (day_time + 1.0).rem_euclid(DAY_LENGTH_S),
                        health: if kind == KIND_ZOMBIE {
                            ZOMBIE_MAX_HEALTH
                        } else {
                            MOB_MAX_HEALTH
                        },
                        dying: 0.0,
                        attack_cd: 0.0,
                    };
                    let initial_hp = if kind == KIND_ZOMBIE {
                        ZOMBIE_MAX_HEALTH
                    } else {
                        MOB_MAX_HEALTH
                    } as u8;
                    self.mobs.push(mob);
                    self.remote_mobs.insert(
                        id,
                        MobMsg {
                            id,
                            kind,
                            pos,
                            yaw: 0.0,
                            health: initial_hp,
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
                    health: m.health.max(0.0).round() as u8,
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
        // Client-side death fade: server's `tick_mobs` runs the
        // authoritative copy + broadcasts `vx.mob_remove` when the
        // timer hits zero, but every peer also runs the decay
        // locally so the visual fade stays smooth between net
        // ticks. The eventual `vx.mob_remove` (or pure-client
        // simulation timeout) clears the entry.
        self.mob_dying.retain(|_, t| {
            *t = (*t - dt).max(0.0);
            *t > 0.0
        });
        self.mob_lunge.retain(|_, t| {
            *t = (*t - dt).max(0.0);
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
                    self.player_health =
                        state.player_health.clamp(0.0, PLAYER_MAX_HEALTH);
                    self.kill_count = state.kill_count;
                    // Restore the world seed before the chunk
                    // stream starts re-meshing so the workers
                    // pick up the right noise. Older saves with
                    // seed=0 stay at zero; on_start will fill in
                    // a random value the first time the loader
                    // doesn't catch.
                    if state.world_seed != 0 {
                        set_world_seed(state.world_seed);
                    }
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
    /// Raycast against every remote player's AABB. Returns the
    /// nearest hit `(id, distance)` within `max_dist`, or `None`.
    /// Skips our own id so we never target ourselves. Uses the
    /// interpolated render pose so the hit-test matches what the
    /// player actually sees on screen.
    fn raycast_remote_player(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
    ) -> Option<(u32, f32)> {
        let own_id: u32 = match &self.net {
            NetRole::Offline => return None,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let now = std::time::Instant::now();
        let mut best: Option<(u32, f32)> = None;
        for (&id, interp) in self.remote_interp.iter() {
            if id == own_id {
                continue;
            }
            let p = interp.sample(now);
            // Use the same body+head extents as the humanoid
            // render. Box centred at pose - (0, 0.7, 0).
            let cx = p.pos[0];
            let cy = p.pos[1] - 0.7;
            let cz = p.pos[2];
            let hx = 0.35;
            let hy = 1.0;
            let hz = 0.35;
            let min = [cx - hx, cy - hy, cz - hz];
            let max = [cx + hx, cy + hy, cz + hz];
            if let Some(t) = ray_aabb(origin.into(), dir.into(), min, max)
                && t <= max_dist
                && best.is_none_or(|(_, bt)| t < bt)
            {
                best = Some((id, t));
            }
        }
        best
    }

    /// Pick the nearest player position (host + remote pose
    /// table) within `range` of `from`. Returns `None` if no
    /// player is in range or there are no players known. Used by
    /// zombie AI to pick a chase target.
    fn nearest_player_pos(&self, from: [f32; 3], range: f32) -> Option<[f32; 3]> {
        let r2 = range * range;
        let mut best: Option<([f32; 3], f32)> = None;
        let mut consider = |p: [f32; 3]| {
            let dx = p[0] - from[0];
            let dy = p[1] - from[1];
            let dz = p[2] - from[2];
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2 <= r2 && best.is_none_or(|(_, b)| d2 < b) {
                best = Some((p, d2));
            }
        };
        // Host's own pose is always in play.
        consider([self.player_pos.x, self.player_pos.y, self.player_pos.z]);
        // Plus every remote player tracked by the server pose
        // table (clients don't have other peers' poses authoritatively).
        if let NetRole::Server { poses, .. } = &self.net {
            for p in poses.values() {
                if p.id == 0 {
                    continue;
                }
                consider(p.pos);
            }
        }
        best.map(|(p, _)| p)
    }

    /// Probe the voxel column directly under the player's feet
    /// at world XZ `(wx, wz)` and return true if any cell across
    /// the player's collision footprint is solid. Used by the
    /// sneak edge-clamp so a grounded sneaking player can't walk
    /// off the side of a block.
    fn has_floor_under(&self, wx: f32, wz: f32) -> bool {
        let foot_y = (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32;
        // The player AABB is `(wx - r, wz - r) -> (wx + r, wz + r)`
        // at foot height. Check the 4 corner cells (or fewer when
        // the player straddles fewer voxels) so we don't miss the
        // diagonal case where the supporting block sits on one
        // corner only.
        let r = PLAYER_RADIUS - COLLISION_EPSILON;
        let xs = [(wx - r).floor() as i32, (wx + r).floor() as i32];
        let zs = [(wz - r).floor() as i32, (wz + r).floor() as i32];
        for x in xs {
            for z in zs {
                if !self.world.get([x, foot_y, z]).is_air() {
                    return true;
                }
            }
        }
        false
    }

    /// Apply HP damage to the local player. Reaches zero = death
    /// = respawn at origin + storage-row wipe + inventory snapshot
    /// fired through the existing `vx.fellout` channel so the
    /// server's authoritative copy stays in sync.
    fn apply_player_damage(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        // Godmode swallows damage entirely. Toggled via /godmode.
        if self.god_mode {
            return;
        }
        self.player_health = (self.player_health - amount).max(0.0);
        self.hurt_flash = 0.5;
        // Damage resets the regen grace timer.
        self.regen_accum = 0.0;
        // Synth hurt grunt so the player hears the hit even when
        // no asset pack is loaded.
        let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
        self.sfx_at("hurt", pos, |a, g| {
            let _ = a.play_thud(140.0, 0.35 * g);
        });
        if self.player_health <= 0.0 {
            tracing::warn!("player died (HP = 0) - respawning");
            self.player_pos = Vec3::new(0.0, 32.0, 0.0);
            self.player_vel = Vec3::ZERO;
            self.player_health = PLAYER_MAX_HEALTH;
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
                tracing::info!(dropped, "death penalty: cleared storage stacks");
            }
            // Local chat toast so the player gets a visible
            // confirmation matching the audible thud. The line
            // stays local; in MP the kill/death announcement is
            // handled separately by the server when it's
            // implemented.
            self.chat_log.push((
                std::time::Instant::now(),
                if dropped > 0 {
                    format!("You died. Lost {dropped} storage stacks.")
                } else {
                    "You died and respawned at spawn.".to_string()
                },
            ));
            // Broadcast a public death notice in MP so peers see
            // the event. Host tags it with `self.player_name`;
            // clients send the raw line and the server's chat
            // path wraps it with the sender's identity.
            let public = "* I just died!".to_string();
            let cfg = bincode::config::standard();
            match &mut self.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    let line = format!("* {} died.", self.player_name);
                    self.chat_log
                        .push((std::time::Instant::now(), line.clone()));
                    if let Ok(buf) = bincode::serde::encode_to_vec(&line, cfg) {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
                NetRole::Client { client, .. } => {
                    if let Ok(buf) = bincode::serde::encode_to_vec(&public, cfg) {
                        let _ = client.send_bytes("vx.chat", buf);
                    }
                    let _ = client.send_bytes("vx.fellout", Vec::new());
                }
            }
        }
    }

    /// Fire-and-forget `vx.leave` to the server (if any). Called
    /// on explicit disconnect / quit so the server doesn't have
    /// to wait its 5 s silence reaper to broadcast a removal.
    fn notify_server_leaving(&self) {
        if let NetRole::Client { client, .. } = &self.net {
            let _ = client.send_bytes("vx.leave", Vec::new());
        }
    }

    /// Resolve the config file path used for settings persistence.
    /// Lives next to the exe in release builds so the user can
    /// nuke the file without finding a hidden appdata directory;
    /// during `cargo run` it sits next to the workspace target so
    /// repeated runs share the same file.
    fn config_path() -> std::path::PathBuf {
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            return dir.join("voxel_demo_mp.config");
        }
        std::path::PathBuf::from("voxel_demo_mp.config")
    }

    /// Persist user-facing settings (name, sensitivity, toggles)
    /// to disk as a tiny key=value text file. Called from the
    /// Settings panel's Back button + on shutdown so the next run
    /// keeps the player's identity.
    fn save_config(&self) {
        use std::fmt::Write;
        let mut text = String::new();
        let _ = writeln!(text, "player_name={}", self.player_name);
        let _ = writeln!(
            text,
            "mouse_sensitivity={}",
            self.settings.mouse_sensitivity
        );
        let _ = writeln!(text, "show_fps={}", self.settings.show_fps);
        let _ = writeln!(text, "lock_cursor={}", self.settings.lock_cursor);
        let _ = writeln!(text, "hide_cursor={}", self.settings.hide_cursor);
        let _ = writeln!(text, "view_distance={}", self.settings.view_distance);
        let _ = writeln!(text, "head_bob={}", self.settings.head_bob);
        if let Some(p) = self.home_pos {
            let _ = writeln!(text, "home={},{},{}", p[0], p[1], p[2]);
        }
        let path = Self::config_path();
        if let Err(err) = std::fs::write(&path, text) {
            warn!(error = %err, path = %path.display(), "config save failed");
        }
    }

    /// Inverse of `save_config`. Silently ignores missing /
    /// malformed files - first launch just writes defaults the
    /// next time the user opens Settings.
    fn load_config(&mut self) {
        let path = Self::config_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "player_name" => {
                    let sane: String = value
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(24)
                        .collect();
                    if !sane.is_empty() {
                        self.player_name = sane;
                    }
                }
                "mouse_sensitivity" => {
                    if let Ok(v) = value.parse::<f32>() {
                        self.settings.mouse_sensitivity = v.clamp(0.05, 4.0);
                    }
                }
                "show_fps" => {
                    self.settings.show_fps = matches!(value, "true" | "1" | "yes");
                }
                "lock_cursor" => {
                    self.settings.lock_cursor =
                        matches!(value, "true" | "1" | "yes");
                }
                "hide_cursor" => {
                    self.settings.hide_cursor =
                        matches!(value, "true" | "1" | "yes");
                }
                "view_distance" => {
                    if let Ok(v) = value.parse::<u8>() {
                        self.settings.view_distance = v.clamp(2, 12);
                    }
                }
                "head_bob" => {
                    self.settings.head_bob =
                        matches!(value, "true" | "1" | "yes");
                }
                "home" => {
                    let parts: Vec<&str> = value.split(',').collect();
                    if parts.len() == 3
                        && let (Ok(x), Ok(y), Ok(z)) = (
                            parts[0].trim().parse::<f32>(),
                            parts[1].trim().parse::<f32>(),
                            parts[2].trim().parse::<f32>(),
                        )
                    {
                        self.home_pos = Some([x, y, z]);
                    }
                }
                _ => {}
            }
        }
    }

    /// Notify the player visually that an autosave just landed.
    /// A short-lived chat line is the cheapest path that doesn't
    /// require a new HUD layer.
    fn note_save(&mut self) {
        self.chat_log.push((
            std::time::Instant::now(),
            "[autosave] World state saved.".to_string(),
        ));
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
            player_health: self.player_health,
            kill_count: self.kill_count,
            world_seed: world_seed(),
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
        self.note_save();
    }

    /// Push a small burst of debris particles into the system at the
    /// centre of `voxel`. Used as a "block broken" effect from every
    /// edit-apply path so both the player and remote players see
    /// puffs when something mines.
    fn spawn_break_particles(&mut self, voxel: [i32; 3]) {
        // No-tint variant: callers that don't know the source
        // block fall through to a neutral dirt-ish color.
        self.spawn_break_particles_tinted(voxel, [0.75, 0.55, 0.30]);
    }

    /// Tinted variant. `color` is the linear-RGB albedo of the
    /// broken block; the debris cubes pick up that hue with a
    /// small per-particle brightness jitter so they read as
    /// material-matched chips instead of a generic puff.
    fn spawn_break_particles_tinted(&mut self, voxel: [i32; 3], color: [f32; 3]) {
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
            // Small per-particle brightness jitter so the puff
            // doesn't read as a flat monochrome cluster.
            let j = 0.85 + rand01() * 0.30;
            self.particles.particles.push(infengine_particles::Particle {
                position: pos,
                velocity: [vx, vy, vz],
                color: [
                    (color[0] * j).clamp(0.0, 1.0),
                    (color[1] * j).clamp(0.0, 1.0),
                    (color[2] * j).clamp(0.0, 1.0),
                    1.0,
                ],
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
                self.note_pickup(block);
                return;
            }
        }
        // Second pass: claim an empty slot. Prefer hotbar first so
        // freshly mined blocks become hot-keyable immediately.
        for slot in self.inventory.iter_mut() {
            if slot.is_none() {
                *slot = Some((block, 1));
                self.note_pickup(block);
                return;
            }
        }
    }

    /// Register a successful pickup for the floating "+1 wood"
    /// toast HUD. Collapses adjacent picks of the same block into
    /// a single entry so spamming the same stack doesn't bury the
    /// HUD in a wall of one-liners.
    fn note_pickup(&mut self, block: BlockId) {
        let now = std::time::Instant::now();
        if let Some(last) = self.pickup_toasts.last_mut()
            && last.1 == block
            && now.duration_since(last.0).as_secs_f32() < 1.5
        {
            last.0 = now;
            last.2 = last.2.saturating_add(1);
            return;
        }
        self.pickup_toasts.push((now, block, 1));
        // Cap the buffer so a long mining session doesn't grow
        // unbounded. Oldest entries are the first to roll off the
        // top of the HUD anyway.
        if self.pickup_toasts.len() > 16 {
            let drop = self.pickup_toasts.len() - 16;
            self.pickup_toasts.drain(0..drop);
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
        // Sanitize: trimmed + non-empty so the server never sees
        // a blank Hello name. Falls back to a session-anchored
        // pseudo-random `Player####` when the user cleared the
        // field in settings.
        let trimmed: String = self
            .player_name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(24)
            .collect();
        if trimmed.is_empty() {
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
                % 10000;
            self.player_name = format!("Player{n:04}");
        } else if trimmed != self.player_name {
            self.player_name = trimmed;
        }
        match NetClient::connect(addr, &self.player_name) {
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
                // Pick a world seed for this session if one
                // wasn't restored from the on-disk state. The
                // nanosecond clock + a fixed mixing constant give
                // a different terrain layout per cold start.
                if world_seed() == 0 {
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos())
                        .unwrap_or(0)
                        .wrapping_mul(0x9E37_79B1)
                        | 1;
                    set_world_seed(seed);
                }
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
        // Pose updates collected this tick. Folded into the
        // smoothing buffer (`remote_interp`) after the net match
        // arm releases its borrow on `self.net`.
        let mut interp_updates: Vec<PlayerMsg> = Vec::new();
        // Local damage accumulator drained after the match arm
        // releases its borrow on `self.net`. Used by:
        //  - vx.damage (client side): server-pushed PvP damage.
        //  - vx.pvp_hit (server side, host = target): a remote
        //    client whacked the host.
        let mut pvp_self_damage: f32 = 0.0;
        // Chat lines received this tick. Appended to `chat_log`
        // after the net match arm releases its borrow on `self.net`.
        let mut chat_inbox: Vec<String> = Vec::new();
        // Roster update from the server's `vx.names` broadcast.
        // Last-write-wins per tick: only the latest snapshot is
        // applied to `player_names` after the match arm closes.
        let mut names_update: Option<Vec<(u32, String)>> = None;
        // Server-pushed teleport. Drained after the net borrow
        // releases so we can write `self.player_pos` directly.
        let mut teleport_request: Option<TeleportMsg> = None;
        // Latest authoritative day_time from the server. Last
        // wins so a mid-tick burst doesn't bounce the sky.
        let mut day_time_update: Option<f32> = None;
        let mut seed_update: Option<u32> = None;
        let mut weather_update: Option<u8> = None;
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
                                interp_updates.push(p);
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
                        "vx.chat" => {
                            // Decode + tag with the sender's
                            // client name, then broadcast so every
                            // peer (including the originator) sees
                            // the same final line. The whisper
                            // marker (\u{1}whisper\u{1}<id>\u{1}<text>)
                            // is intercepted and re-routed to a
                            // single client instead.
                            if let Ok((text, _)) = bincode::serde::decode_from_slice::<
                                String,
                                _,
                            >(&data, cfg)
                            {
                                let sender = server
                                    .client_name(client_id)
                                    .unwrap_or_else(|| format!("p{client_id}"));
                                if let Some(rest) =
                                    text.strip_prefix("\u{1}whisper\u{1}")
                                {
                                    let mut parts = rest.splitn(2, '\u{1}');
                                    let tid_str = parts.next().unwrap_or("");
                                    let body = parts.next().unwrap_or("");
                                    if let Ok(tid) = tid_str.parse::<u32>()
                                        && !body.is_empty()
                                    {
                                        let line = format!(
                                            "[whisper from {sender}] {body}"
                                        );
                                        if tid == 0 {
                                            chat_inbox.push(line);
                                        } else if let Ok(buf) =
                                            bincode::serde::encode_to_vec(
                                                &line, cfg,
                                            )
                                        {
                                            server.send_bytes_to_client(
                                                tid, "vx.chat", buf,
                                            );
                                        }
                                    }
                                } else {
                                    let line = format!("<{sender}> {text}");
                                    chat_inbox.push(line.clone());
                                    if let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            &line, cfg,
                                        )
                                    {
                                        server.broadcast_bytes("vx.chat", buf);
                                    }
                                }
                            }
                        }
                        "vx.pvp_hit" => {
                            // Validate the attacker's reach + the
                            // existence of the target, then forward
                            // a `vx.damage` to the target client.
                            // Host (id 0) attacks are handled
                            // client-side (see the lmb dispatch);
                            // this path only fires for remote
                            // clients hitting other peers.
                            if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                                PvPHitMsg,
                                _,
                            >(&data, cfg)
                            {
                                let attacker_pos =
                                    poses.get(&client_id).copied().map(|p| p.pos);
                                let target_pos = if m.target_id == 0 {
                                    Some([
                                        player_pos.x,
                                        player_pos.y,
                                        player_pos.z,
                                    ])
                                } else {
                                    poses.get(&m.target_id).copied().map(|p| p.pos)
                                };
                                if let (Some(ap), Some(tp)) =
                                    (attacker_pos, target_pos)
                                {
                                    let dx = ap[0] - tp[0];
                                    let dy = ap[1] - tp[1];
                                    let dz = ap[2] - tp[2];
                                    let d2 = dx * dx + dy * dy + dz * dz;
                                    if d2 <= (PVP_REACH + 1.0).powi(2) {
                                        if m.target_id == 0 {
                                            // Host is the target.
                                            pvp_self_damage += PVP_DAMAGE;
                                        } else {
                                            let dmg = DamageMsg {
                                                amount: PVP_DAMAGE,
                                            };
                                            if let Ok(buf) =
                                                bincode::serde::encode_to_vec(
                                                    dmg, cfg,
                                                )
                                            {
                                                server.send_bytes_to_client(
                                                    m.target_id,
                                                    "vx.damage",
                                                    buf,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
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
                    // Grab the name BEFORE we lose track of the
                    // mapping below so the broadcast chat line
                    // reads sensibly.
                    let depart_name = self
                        .player_names
                        .get(id)
                        .cloned()
                        .or_else(|| server.client_name(*id))
                        .unwrap_or_else(|| format!("p{id}"));
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
                    // Public departure notice. Hits the host's
                    // local log via chat_inbox + broadcasts to
                    // every still-connected peer.
                    let line = format!("* {depart_name} left.");
                    chat_inbox.push(line.clone());
                    if let Ok(buf) = bincode::serde::encode_to_vec(&line, cfg) {
                        server.broadcast_bytes("vx.chat", buf);
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
                    // Public arrival notice. Same path as the
                    // depart line above; the server's broadcast
                    // pushes to every peer including the new
                    // joiner, so they immediately see who else
                    // is around.
                    let name = server
                        .client_name(*id)
                        .unwrap_or_else(|| format!("p{id}"));
                    let line = format!("* {name} joined.");
                    chat_inbox.push(line.clone());
                    if let Ok(buf) = bincode::serde::encode_to_vec(&line, cfg) {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
                // Roster broadcast: whenever the connected set
                // changes, ship the full id->name table to every
                // client so nameplates / Tab playerlist stay
                // current. Host owns id 0 with its own
                // `player_name`.
                // Send world seed to brand-new joiners so their
                // chunk workers generate matching terrain.
                if !new_ids.is_empty()
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(world_seed(), cfg)
                {
                    for id in &new_ids {
                        server.send_bytes_to_client(*id, "vx.seed", buf.clone());
                    }
                }
                // Ship current weather to new joiners too.
                if !new_ids.is_empty()
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(self.weather, cfg)
                {
                    for id in &new_ids {
                        server.send_bytes_to_client(
                            *id,
                            "vx.weather",
                            buf.clone(),
                        );
                    }
                }
                if !new_ids.is_empty() || !departed_ids.is_empty() {
                    let mut entries: Vec<(u32, String)> =
                        vec![(0, self.player_name.clone())];
                    for cid in server.client_ids() {
                        let name = server
                            .client_name(cid)
                            .unwrap_or_else(|| format!("p{cid}"));
                        entries.push((cid, name));
                    }
                    self.player_names.clear();
                    for (id, n) in &entries {
                        self.player_names.insert(*id, n.clone());
                    }
                    if let Ok(buf) = bincode::serde::encode_to_vec(&entries, cfg) {
                        server.broadcast_bytes("vx.names", buf);
                    }
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
                // Authoritative day_time broadcast on a slow
                // cadence (~once per second). Clients update
                // their local clock so the sun stays in sync
                // even if /day or /night was used to skip ahead.
                let now = std::time::Instant::now();
                let due = self
                    .last_day_broadcast
                    .map(|t| now.duration_since(t).as_secs_f32() >= 1.0)
                    .unwrap_or(true);
                if due
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(self.day_time, cfg)
                {
                    server.broadcast_bytes("vx.day", buf);
                    self.last_day_broadcast = Some(now);
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
                                interp_updates.push(p);
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
                        "vx.chat" => {
                            if let Ok((line, _)) = bincode::serde::decode_from_slice::<
                                String,
                                _,
                            >(&data, cfg)
                            {
                                chat_inbox.push(line);
                            }
                        }
                        "vx.names" => {
                            if let Ok((entries, _)) = bincode::serde::decode_from_slice::<
                                Vec<(u32, String)>,
                                _,
                            >(&data, cfg)
                            {
                                names_update = Some(entries);
                            }
                        }
                        "vx.damage" => {
                            if let Ok((d, _)) = bincode::serde::decode_from_slice::<
                                DamageMsg,
                                _,
                            >(&data, cfg)
                            {
                                pvp_self_damage += d.amount;
                            }
                        }
                        "vx.teleport" => {
                            if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                                TeleportMsg,
                                _,
                            >(&data, cfg)
                            {
                                teleport_request = Some(m);
                            }
                        }
                        "vx.day" => {
                            if let Ok((dt, _)) =
                                bincode::serde::decode_from_slice::<f32, _>(
                                    &data, cfg,
                                )
                            {
                                day_time_update = Some(dt);
                            }
                        }
                        "vx.seed" => {
                            if let Ok((seed, _)) =
                                bincode::serde::decode_from_slice::<u32, _>(
                                    &data, cfg,
                                )
                            {
                                seed_update = Some(seed);
                            }
                        }
                        "vx.weather" => {
                            if let Ok((w, _)) =
                                bincode::serde::decode_from_slice::<u8, _>(
                                    &data, cfg,
                                )
                            {
                                weather_update = Some(w);
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
            // Diff vs the pre-snapshot inventory so server-driven
            // pickups generate toasts the same way local pickups
            // do. Counts blocks by id; any positive delta becomes
            // a `+N` toast.
            let mut pre: HashMap<u16, u32> = HashMap::new();
            for (b, c) in self.inventory.iter().flatten() {
                *pre.entry(b.0).or_insert(0) += *c as u32;
            }
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
            let mut post: HashMap<u16, u32> = HashMap::new();
            for (b, c) in self.inventory.iter().flatten() {
                *post.entry(b.0).or_insert(0) += *c as u32;
            }
            for (id, after) in post {
                let before = pre.get(&id).copied().unwrap_or(0);
                if after > before {
                    let delta = (after - before).min(u8::MAX as u32) as u8;
                    let block = BlockId(id);
                    let now = std::time::Instant::now();
                    if let Some(last) = self.pickup_toasts.last_mut()
                        && last.1 == block
                        && now.duration_since(last.0).as_secs_f32() < 1.5
                    {
                        last.0 = now;
                        last.2 = last.2.saturating_add(delta);
                    } else {
                        self.pickup_toasts.push((now, block, delta));
                    }
                }
            }
            if self.pickup_toasts.len() > 16 {
                let drop = self.pickup_toasts.len() - 16;
                self.pickup_toasts.drain(0..drop);
            }
        }
        for m in mob_updates {
            self.remote_mobs.insert(m.id, m);
        }
        // Fold incoming pose updates into the interpolation buffer
        // so the render path can lerp toward each new snapshot
        // instead of teleporting.
        for p in interp_updates {
            self.remote_interp
                .entry(p.id)
                .and_modify(|r| r.push(p))
                .or_insert_with(|| RemoteInterp::new(p));
        }
        // Drain chat lines collected while the `net` borrow was
        // held in the receive match. They render in the HUD chat
        // log with a 10s fade.
        if !chat_inbox.is_empty() {
            let now = std::time::Instant::now();
            for line in chat_inbox {
                // Mute filter: drop lines whose `<sender>` tag
                // or `* sender` prefix matches a muted name.
                if !self.muted_players.is_empty()
                    && line_is_muted(&line, &self.muted_players)
                {
                    continue;
                }
                self.chat_log.push((now, line));
            }
            // Cap log length so very long sessions don't grow
            // unbounded.
            let max_chat = 64usize;
            if self.chat_log.len() > max_chat {
                let drop = self.chat_log.len() - max_chat;
                self.chat_log.drain(0..drop);
            }
        }
        // Apply the latest names roster (client-side; the server
        // already updates `player_names` inline when it builds the
        // broadcast).
        if let Some(entries) = names_update {
            self.player_names.clear();
            for (id, n) in entries {
                self.player_names.insert(id, n);
            }
        }
        if let Some(dt) = day_time_update {
            self.day_time = dt.rem_euclid(DAY_LENGTH_S);
        }
        if let Some(w) = weather_update {
            self.weather = w;
        }
        if let Some(seed) = seed_update
            && seed != world_seed()
        {
            // Server picked a new world seed: adopt it and drop
            // every generated chunk so the next streaming pass
            // re-runs the workers with the new noise. We can't
            // mid-flight remap existing chunks since heights /
            // ore positions / biomes all changed.
            set_world_seed(seed);
            let to_unload: Vec<ChunkCoord> = self
                .world
                .iter_chunks()
                .map(|(c, _)| *c)
                .collect();
            if !to_unload.is_empty() {
                let renderer = self
                    .renderer
                    .as_mut()
                    .expect("renderer ready");
                for entry in self.chunk_meshes.drain(..) {
                    renderer.remove_mesh(entry.2);
                }
                for cc in &to_unload {
                    self.world.remove_chunk(*cc);
                }
            }
            self.pending_chunks.clear();
            self.pending_set.clear();
        }
        if let Some(t) = teleport_request {
            self.player_pos = Vec3::new(t.pos[0], t.pos[1], t.pos[2]);
            self.player_vel = Vec3::ZERO;
            self.fall_start_y = None;
            self.chat_log.push((
                std::time::Instant::now(),
                "You were summoned.".to_string(),
            ));
        }
        if pvp_self_damage > 0.0 {
            self.apply_player_damage(pvp_self_damage);
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
            // Death event: start the local fade timer so the
            // body tilts + fades for `MOB_DEATH_FADE` seconds.
            // The server emits `vx.mob_remove` after the same
            // delay, which will evict the entry from
            // `remote_mobs` at the right moment.
            if evt.kind == 1 {
                self.mob_dying.insert(evt.id, MOB_DEATH_FADE);
            }
            // Attack event: kick off the local lunge timer so the
            // attacker's body briefly pushes forward.
            if evt.kind == 2 {
                self.mob_lunge.insert(evt.id, MOB_LUNGE_TIME);
            }
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
            self.mob_dying.remove(&id);
            self.mob_step_timers.remove(&id);
            self.mob_say_timers.remove(&id);
            self.mob_last_xz.remove(&id);
        }
        // Drop interpolation entries for any player who left.
        for id in &player_removals {
            self.remote_interp.remove(id);
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
            f_prev: false,
            hud_visible: true,
            f1_prev: false,
            chat_open: false,
            chat_input: String::new(),
            chat_log: Vec::new(),
            chat_history: Vec::new(),
            chat_history_idx: None,
            t_prev: false,
            lmb_pending: false,
            rmb_pending: false,
            lmb_held: false,
            rmb_held: false,
            mining: None,
            player_health: PLAYER_MAX_HEALTH,
            fall_start_y: None,
            hurt_flash: 0.0,
            regen_accum: 0.0,
            kill_count: 0,
            fov_y_rad: 60_f32.to_radians(),
            sprinting: false,
            stamina: STAMINA_MAX,
            remote_interp: HashMap::new(),
            player_name: {
                // Pseudo-random 4-digit suffix so repeat-launch
                // sessions don't accidentally collide.
                let n = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
                    % 10000;
                format!("Player{n:04}")
            },
            player_names: HashMap::new(),
            tab_held: false,
            pickup_toasts: Vec::new(),
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
            heartbeat_accum: 0.0,
            fly_mode: false,
            god_mode: false,
            mob_cap: MOB_CAP_DEFAULT,
            freeze_time: false,
            weather: 0,
            lightning_flash: 0.0,
            next_lightning_at: 6.0,
            muted_players: HashSet::new(),
            head_bob_phase: 0.0,
            camera_mode: 0,
            f5_prev: false,
            q_prev: false,
            home_pos: None,
            session_started: None,
            last_day_broadcast: None,
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
            mob_dying: HashMap::new(),
            mob_lunge: HashMap::new(),
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
        let view_dist = self.settings.view_distance.clamp(2, 12) as i32;
        let unload_dist = view_dist + 2;

        // Queue missing chunks inside the view radius. Vertical
        // column always loaded (cy 0..VERTICAL_CHUNKS) so mountain
        // peaks + sky builds never pop in.
        for dz in -view_dist..=view_dist {
            for dx in -view_dist..=view_dist {
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
                dx > unload_dist || dz > unload_dist
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
            if dx > unload_dist || dz > unload_dist {
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
            && !self.mp_menu_open
            && !self.chat_open;
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
        let input_active =
            !self.inventory_open && !self.mp_menu_open && !self.chat_open;

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

        let mut move_axis: f32 = 0.0;
        let mut strafe: f32 = 0.0;
        // Movement speed multiplier: Shift = sneak (0.4x), Ctrl =
        // sprint (1.7x), no modifier = normal walk. Modifiers are
        // also gated on `input_active` so the menu doesn't sprint
        // the player while idle.
        let mut speed_mul = 1.0_f32;
        let mut sneaking = false;
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
                sneaking = true;
            }
            if ctx.input.is_pressed(Key::ControlLeft)
                || ctx.input.is_pressed(Key::ControlRight)
            {
                // Stamina gate: sprint refuses to engage while
                // the player is below the reactivation
                // threshold so a fully-drained bar enforces a
                // brief walking penalty.
                let want_sprint = move_axis.abs() + strafe.abs() > 0.0;
                let stamina_ok = if self.sprinting {
                    self.stamina > 0.0
                } else {
                    self.stamina >= STAMINA_REACTIVATE
                };
                if want_sprint && stamina_ok {
                    speed_mul *= 1.7;
                    self.sprinting = true;
                } else {
                    self.sprinting = false;
                }
            } else {
                self.sprinting = false;
            }
        } else {
            self.sprinting = false;
        }
        // Stamina accounting: drain while sprinting, regen
        // otherwise. Clamped to [0, STAMINA_MAX] each tick.
        if self.sprinting {
            self.stamina = (self.stamina - STAMINA_DRAIN * dt).max(0.0);
        } else {
            self.stamina =
                (self.stamina + STAMINA_REGEN * dt).min(STAMINA_MAX);
        }
        // Smooth FOV lerp toward the per-state target. 60 deg
        // baseline, 70 deg while sprinting. The exponential lerp
        // factor `1 - exp(-rate * dt)` keeps the easing
        // framerate-independent.
        let target_fov = if self.sprinting {
            70_f32.to_radians()
        } else {
            60_f32.to_radians()
        };
        let lerp_rate = 8.0_f32;
        let alpha = 1.0 - (-lerp_rate * dt).exp();
        self.fov_y_rad += (target_fov - self.fov_y_rad) * alpha;
        let horizontal = (forward_horizontal * move_axis + right * strafe)
            .normalize_or_zero()
            * (PLAYER_SPEED * speed_mul);
        let mut hvel_x = horizontal.x;
        let mut hvel_z = horizontal.z;
        // Sneak edge protection: a grounded sneaking player can't
        // walk off a block. We probe per-axis along the candidate
        // delta and zero the component that would land the feet
        // over thin air. Both axes blocked = no horizontal move
        // at all (lets the player creep up to the edge safely).
        if sneaking && self.grounded {
            let test_step = 0.4;
            let test_x = self.player_pos.x + hvel_x.signum() * test_step;
            if hvel_x.abs() > 1e-3 && !self.has_floor_under(test_x, self.player_pos.z) {
                hvel_x = 0.0;
            }
            let test_z = self.player_pos.z + hvel_z.signum() * test_step;
            if hvel_z.abs() > 1e-3 && !self.has_floor_under(self.player_pos.x, test_z) {
                hvel_z = 0.0;
            }
        }
        self.player_vel.x = hvel_x;
        self.player_vel.z = hvel_z;

        if self.fly_mode {
            // Free-flight: Space = up, Shift = down. Velocity is
            // replaced (not integrated) so the player stops the
            // instant the key is released, the way Minecraft's
            // creative fly works. Ctrl held while flying doubles
            // the vertical thrust + horizontal speed multiplier
            // for fast traversal.
            let boost = input_active
                && (ctx.input.is_pressed(Key::ControlLeft)
                    || ctx.input.is_pressed(Key::ControlRight));
            let up = input_active && ctx.input.is_pressed(Key::Space);
            let down = input_active
                && (ctx.input.is_pressed(Key::ShiftLeft)
                    || ctx.input.is_pressed(Key::ShiftRight));
            let thrust = if boost { 16.0 } else { 8.0 };
            let v = if up && !down {
                thrust
            } else if down && !up {
                -thrust
            } else {
                0.0
            };
            self.player_vel.y = v;
            if boost {
                self.player_vel.x *= 1.7;
                self.player_vel.z *= 1.7;
            }
            self.grounded = false;
        } else {
            if input_active && ctx.input.is_pressed(Key::Space) && self.grounded {
                self.player_vel.y = JUMP_VELOCITY;
                self.grounded = false;
                // Jumping costs a flat 5 stamina so spamming the
                // bunny-hop drains the pool. Floor at 0.
                self.stamina = (self.stamina - 5.0).max(0.0);
                // Sprint-jump boost: add forward kick so a moving
                // jump covers more ground than a stand-still jump.
                if self.sprinting {
                    let fwd_h = Vec3::new(
                        self.player_yaw.cos(),
                        0.0,
                        -self.player_yaw.sin(),
                    )
                    .normalize_or_zero();
                    self.player_vel.x += fwd_h.x * 3.0;
                    self.player_vel.z += fwd_h.z * 3.0;
                }
            }
            self.player_vel.y += GRAVITY * dt;
        }

        // Footstep audio: fire a short click every ~0.4 s of
        // grounded horizontal motion. Sprint shortens the cadence
        // proportionally so it stays in sync with the leg.
        let speed = (self.player_vel.x * self.player_vel.x
            + self.player_vel.z * self.player_vel.z)
            .sqrt();
        // Advance the head-bob phase proportionally to horizontal
        // speed while grounded. Off the ground we let it ease
        // back to a quiet rest so a jump doesn't freeze the bob
        // mid-cycle.
        if self.grounded && speed > 0.5 {
            self.head_bob_phase += dt * (speed * 1.6 + 1.0);
        } else {
            // Bleed phase back to zero so the camera settles flat.
            self.head_bob_phase = (self.head_bob_phase
                - dt * std::f32::consts::PI)
                .max(0.0);
        }
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
                // Sprint dust: a small puff per step matching the
                // surface colour. Walking sneak / normal speed
                // skips this so dust is a sprint-only flourish.
                if self.sprinting {
                    let color = self
                        .registry
                        .get(under)
                        .map(|d| d.color)
                        .unwrap_or([0.6, 0.55, 0.45]);
                    let foot_voxel = foot;
                    // Tiny burst that fades fast; reuses the
                    // existing particle system rather than adding
                    // a dedicated sprint emitter.
                    let base = [
                        self.player_pos.x,
                        self.player_pos.y - PLAYER_HEIGHT,
                        self.player_pos.z,
                    ];
                    let mut rng: u32 = 0xD15D_15D1
                        ^ ((foot_voxel[0] as u32).wrapping_mul(7))
                        ^ ((foot_voxel[2] as u32).wrapping_mul(13))
                        ^ (self.net_tick as u32);
                    let mut rand01 = || -> f32 {
                        rng ^= rng << 13;
                        rng ^= rng >> 17;
                        rng ^= rng << 5;
                        (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
                    };
                    for _ in 0..4 {
                        let vx = (rand01() * 2.0 - 1.0) * 1.2;
                        let vy = rand01() * 0.6 + 0.2;
                        let vz = (rand01() * 2.0 - 1.0) * 1.2;
                        self.particles.particles.push(
                            infengine_particles::Particle {
                                position: base,
                                velocity: [vx, vy, vz],
                                color: [color[0], color[1], color[2], 0.85],
                                size: 0.08,
                                age: 0.0,
                                lifetime: 0.30,
                            },
                        );
                    }
                }
            }
        } else {
            // Reset cooldown so a step doesn't fire instantly the
            // moment we re-enter motion (keeps cadence natural).
            self.step_cooldown = 0.0;
        }

        // Lightning state machine. Only fires while raining.
        // `next_lightning_at` counts down; on zero we trigger a
        // 0.18s flash + a thud and seed the next interval at a
        // randomized 10..=30 s window so strikes feel sporadic.
        if self.weather == 1 {
            self.next_lightning_at -= dt;
            if self.next_lightning_at <= 0.0 {
                self.lightning_flash = 0.18;
                let mut rng: u32 = ((self.net_tick as u32)
                    .wrapping_mul(0xC2B2_AE35))
                    ^ 0xDEAD_BEEF;
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let bias = (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0);
                self.next_lightning_at = 10.0 + bias * 20.0;
                // Distant thunder + chat note so peers know it
                // wasn't a graphics glitch.
                let pos = [
                    self.player_pos.x,
                    self.player_pos.y,
                    self.player_pos.z,
                ];
                self.sfx_at("thunder", pos, |a, g| {
                    let _ = a.play_thud(55.0, 0.7 * g);
                });
                self.chat_log.push((
                    std::time::Instant::now(),
                    "* lightning flashes overhead".to_string(),
                ));
            }
        }
        if self.lightning_flash > 0.0 {
            self.lightning_flash = (self.lightning_flash - dt).max(0.0);
        }
        // Precipitation spawner. weather=1 = rain (fast falling
        // blue streaks); weather=2 = snow (slow drifting white
        // flakes). Both stay purely visual.
        if self.weather == 1 || self.weather == 2 {
            const FALL_PER_FRAME: u32 = 20;
            let mut rng: u32 = ((self.net_tick as u32)
                .wrapping_mul(0x9E37_79B1))
                ^ 0xCAFE_F00D;
            let mut rand01 = || -> f32 {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
            };
            let base = [
                self.player_pos.x,
                self.player_pos.y + 14.0,
                self.player_pos.z,
            ];
            for _ in 0..FALL_PER_FRAME {
                let dx = (rand01() * 2.0 - 1.0) * 22.0;
                let dz = (rand01() * 2.0 - 1.0) * 22.0;
                let (vel, color, size, life) = if self.weather == 1 {
                    (
                        [0.0_f32, -16.0, 0.0_f32],
                        [0.55, 0.65, 0.85, 0.65],
                        0.05,
                        1.2,
                    )
                } else {
                    // Snow: small lateral drift + slow descent.
                    let drift_x = (rand01() * 2.0 - 1.0) * 0.6;
                    let drift_z = (rand01() * 2.0 - 1.0) * 0.6;
                    (
                        [drift_x, -1.6, drift_z],
                        [0.92, 0.95, 1.00, 0.85],
                        0.08,
                        7.0,
                    )
                };
                self.particles.particles.push(
                    infengine_particles::Particle {
                        position: [base[0] + dx, base[1], base[2] + dz],
                        velocity: vel,
                        color,
                        size,
                        age: 0.0,
                        lifetime: life,
                    },
                );
            }
        }
        // Low-HP heartbeat thump. Interval shrinks from ~1.2 s
        // at HP=5 down to ~0.5 s at HP=1. Above HP=5 we leave the
        // accumulator at 0 so re-entering the danger zone fires
        // the first beat promptly.
        if self.player_health > 0.0 && self.player_health < 5.0 {
            self.heartbeat_accum += dt;
            let interval = (0.5 + 0.2 * self.player_health).clamp(0.5, 1.4);
            if self.heartbeat_accum >= interval {
                self.heartbeat_accum = 0.0;
                let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
                let vol_scale = (1.0 - (self.player_health / 5.0)).clamp(0.2, 1.0);
                self.sfx_at("heartbeat", pos, move |a, _g| {
                    let _ = a.play_thud(70.0, 0.28 * vol_scale);
                });
            }
        } else {
            self.heartbeat_accum = 0.0;
        }

        // Axis-resolved voxel AABB collision.
        let was_grounded = self.grounded;
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
        // Fall-damage tracking. Note the world-Y the moment we
        // first leave the ground, then on the landing transition
        // compute the drop height + apply damage.
        if was_grounded && !self.grounded {
            self.fall_start_y = Some(self.player_pos.y);
        }
        if !was_grounded
            && self.grounded
            && let Some(start_y) = self.fall_start_y.take()
        {
            let drop = (start_y - self.player_pos.y).max(0.0);
            // Fly mode disables fall damage so toggling /fly off
            // mid-air doesn't immediately murder the player.
            if drop > SAFE_FALL && !self.fly_mode {
                let dmg = drop - SAFE_FALL;
                self.apply_player_damage(dmg);
                let landing = [
                    self.player_pos.x.floor() as i32,
                    (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                    self.player_pos.z.floor() as i32,
                ];
                self.spawn_break_particles(landing);
            }
        }
        // Hurt-flash decay.
        if self.hurt_flash > 0.0 {
            self.hurt_flash = (self.hurt_flash - dt).max(0.0);
        }
        // HP regen: tick the grace timer; once the player has gone
        // `REGEN_DELAY` seconds without damage, drip HP back in at
        // `REGEN_RATE` HP/s until full.
        if self.player_health > 0.0 && self.player_health < PLAYER_MAX_HEALTH {
            self.regen_accum += dt;
            if self.regen_accum > REGEN_DELAY {
                let gain = REGEN_RATE * dt;
                self.player_health =
                    (self.player_health + gain).min(PLAYER_MAX_HEALTH);
            }
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
        let player_health = self.player_health;
        let stamina_frac =
            (self.stamina / STAMINA_MAX).clamp(0.0, 1.0);
        let player_hurt_flash = self.hurt_flash;
        let show_debug = self.show_debug;
        let hud_visible = self.hud_visible;
        let kill_count = self.kill_count;
        let chat_open = self.chat_open;
        let biome_at_player = biome_at(self.player_pos.x, self.player_pos.z);
        let biome_name = match biome_at_player {
            Biome::Plains => "Plains",
            Biome::Mountain => "Mountain",
            Biome::Desert => "Desert",
        };
        // Snapshot recent chat lines (newest last) along with their
        // age so the renderer can fade older entries.
        let chat_now = std::time::Instant::now();
        let chat_lines: Vec<(String, f32)> = self
            .chat_log
            .iter()
            .map(|(t, line)| {
                (
                    line.clone(),
                    chat_now.saturating_duration_since(*t).as_secs_f32(),
                )
            })
            .collect();
        let mut chat_send_now = false;
        // View-projection for projecting mob HP bars to screen.
        // Built from the same camera state the world pass uses;
        // captured here so the egui closure doesn't need renderer
        // access.
        let hud_cam_pos = self.player_pos;
        let hud_forward = Vec3::new(
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        );
        let hud_aspect =
            self.viewport.0.max(1) as f32 / self.viewport.1.max(1) as f32;
        let hud_view_proj = Mat4::perspective_rh(
            self.fov_y_rad,
            hud_aspect,
            0.1,
            500.0,
        ) * Mat4::look_at_rh(hud_cam_pos, hud_cam_pos + hud_forward, Vec3::Y);
        // Star field: fades in as the sun drops below the horizon.
        // The raw sun elevation (un-clamped) gives a smooth
        // night-factor in [0, 1] when the sun is below the horizon.
        let phase = (self.day_time / DAY_LENGTH_S).rem_euclid(1.0);
        let raw_elev = (phase * std::f32::consts::TAU).sin();
        let night_factor = (-raw_elev).clamp(0.0, 1.0);
        // Mob HP snapshot: (pos, current, max). Only mobs that
        // are damaged + not dying show a bar so the world doesn't
        // get cluttered with full-health bars.
        // Targeted-mob info shown next to the crosshair: kind name
        // + current/max HP. Drives a small label so the player can
        // see how many more hits a sheep / zombie needs.
        let aim_forward = Vec3::new(
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        );
        let aim_origin = self.player_pos;
        // Targeted-block label: name + (if actively mining) the
        // progress percentage. Hidden when the crosshair isn't on
        // anything breakable.
        let targeted_block_info: Option<(&'static str, Option<u8>)> =
            self.target.and_then(|hit| {
                let b = self.world.get(hit.voxel);
                if b == BlockId::AIR {
                    return None;
                }
                let name = block_type(b.0).map(|t| t.name).unwrap_or("?");
                let pct = self.mining.as_ref().and_then(|m| {
                    if m.voxel == hit.voxel && m.block == b {
                        Some(((m.progress / m.total).clamp(0.0, 1.0) * 100.0)
                            as u8)
                    } else {
                        None
                    }
                });
                Some((name, pct))
            });
        let targeted_mob_info: Option<(&'static str, u8, u8)> = self
            .raycast_mob(aim_origin, aim_forward, MOB_ATTACK_REACH * 1.5)
            .and_then(|(id, _)| self.remote_mobs.get(&id).copied())
            .map(|m| {
                let max = if m.kind == KIND_ZOMBIE {
                    ZOMBIE_MAX_HEALTH as u8
                } else {
                    MOB_MAX_HEALTH as u8
                };
                let name = if m.kind == KIND_ZOMBIE { "Zombie" } else { "Sheep" };
                (name, m.health, max)
            });
        let mob_bars: Vec<([f32; 3], u8, u8)> = self
            .remote_mobs
            .values()
            .filter(|m| {
                let dying = self.mob_dying.get(&m.id).copied().unwrap_or(0.0);
                let max = if m.kind == KIND_ZOMBIE {
                    ZOMBIE_MAX_HEALTH as u8
                } else {
                    MOB_MAX_HEALTH as u8
                };
                m.health > 0 && m.health < max && dying <= 0.0
            })
            .map(|m| {
                let max = if m.kind == KIND_ZOMBIE {
                    ZOMBIE_MAX_HEALTH as u8
                } else {
                    MOB_MAX_HEALTH as u8
                };
                (m.pos, m.health, max)
            })
            .collect();
        // Remote nameplate snapshot: world-space pose + display
        // name for each other player so the egui pass can project
        // them onto the screen. Skips own_id so the local player
        // never sees a tag on their own head.
        let own_id_for_plates: u32 = match &self.net {
            NetRole::Offline => u32::MAX,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let plate_now = std::time::Instant::now();
        let nameplates: Vec<([f32; 3], String, [u8; 3])> = self
            .remote_interp
            .iter()
            .filter(|(id, _)| **id != own_id_for_plates)
            .map(|(id, r)| {
                let p = r.sample(plate_now);
                let name = self
                    .player_names
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| format!("p{id}"));
                // Stable name colour per id: hash-derived hue,
                // brightened so the text reads against the sky.
                let h = (id.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                let tau = std::f32::consts::TAU;
                let r_c = (0.6 + 0.4 * (h * tau).sin().abs()).min(1.0);
                let g_c =
                    (0.6 + 0.4 * ((h + 0.33) * tau).sin().abs()).min(1.0);
                let b_c =
                    (0.6 + 0.4 * ((h + 0.66) * tau).sin().abs()).min(1.0);
                let color = [
                    (r_c * 255.0) as u8,
                    (g_c * 255.0) as u8,
                    (b_c * 255.0) as u8,
                ];
                (p.pos, name, color)
            })
            .collect();
        // Pickup toast snapshot: trim entries older than the
        // fade window so the HUD doesn't keep stale text alive.
        // We do the housekeeping here so the egui closure sees a
        // clean list.
        let toast_now = std::time::Instant::now();
        let toast_lifetime = 2.4_f32;
        self.pickup_toasts
            .retain(|(t, _, _)| toast_now.duration_since(*t).as_secs_f32() <= toast_lifetime);
        let pickup_snapshot: Vec<(f32, String, [f32; 3])> = self
            .pickup_toasts
            .iter()
            .rev()
            .take(6)
            .map(|(t, b, c)| {
                let age = toast_now.duration_since(*t).as_secs_f32();
                let alpha = (1.0 - (age / toast_lifetime)).clamp(0.0, 1.0);
                let name = block_type(b.0).map(|t| t.name).unwrap_or("?");
                let color = self
                    .registry
                    .get(*b)
                    .map(|d| d.color)
                    .unwrap_or([1.0, 1.0, 1.0]);
                (alpha, format!("+{c} {name}"), color)
            })
            .collect();
        // Tab playerlist snapshot: pairs of (display_name,
        // is_self). Drawn as a modal centered overlay while Tab
        // is held.
        let playerlist_visible = self.tab_held && !matches!(self.net, NetRole::Offline);
        let playerlist: Vec<(String, bool)> = if playerlist_visible {
            let mut list: Vec<(String, bool)> = self
                .player_names
                .iter()
                .map(|(id, n)| (n.clone(), *id == own_id_for_plates))
                .collect();
            // Self first, then alphabetical so the order is stable
            // across ticks.
            list.sort_by(|a, b| match (a.1, b.1) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
            });
            list
        } else {
            Vec::new()
        };
        let dbg_player = self.player_pos;
        let dbg_yaw = self.player_yaw;
        let dbg_pitch = self.player_pitch;
        let dbg_chunks = self.chunk_meshes.len();
        let dbg_grounded = self.grounded;
        let dbg_mobs = self.remote_mobs.len();
        let dbg_particles = self.particles.live_count();
        let dbg_fly = self.fly_mode;
        let dbg_cam = match self.camera_mode {
            0 => "1st",
            1 => "3rd back",
            _ => "3rd front",
        };
        let dbg_playtime = self
            .session_started
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        let dbg_pending = self.pending_chunks.len();
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
        // Mutable borrow for the chat input box captured by the
        // egui closure. Taken last so earlier self.* reads
        // (raycast_mob, registry, etc.) finish before this
        // exclusive borrow begins.
        let chat_buf = &mut self.chat_input;
        let chat_history_ref = &self.chat_history;
        let chat_history_idx = &mut self.chat_history_idx;
        let player_name_buf = &mut self.player_name;

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
                        ui.label(
                            egui::RichText::new(format!(
                                "Playing as: {}",
                                player_name_buf
                            ))
                            .color(egui::Color32::from_rgb(180, 200, 200))
                            .italics(),
                        );
                        ui.add_space(10.0);
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
                    ui.horizontal(|ui| {
                        ui.label("Player name:");
                        ui.text_edit_singleline(player_name_buf);
                    });
                    ui.add(
                        egui::Slider::new(&mut settings_edit.mouse_sensitivity, 0.1..=2.0)
                            .text("Mouse sensitivity"),
                    );
                    ui.add(
                        egui::Slider::new(&mut settings_edit.view_distance, 2..=12)
                            .text("Render distance (chunks)"),
                    );
                    ui.checkbox(&mut settings_edit.show_fps, "Show FPS");
                    ui.checkbox(
                        &mut settings_edit.lock_cursor,
                        "Lock cursor in window (Play)",
                    );
                    ui.checkbox(&mut settings_edit.hide_cursor, "Hide cursor (Play)");
                    ui.checkbox(&mut settings_edit.head_bob, "Camera head bob");
                    ui.separator();
                    if ui.button("Back").clicked() {
                        action_back_settings = true;
                    }
                });
            }
            AppState::Playing => {
                if hud_visible {
                egui::Area::new(egui::Id::new("voxel_hud"))
                    .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label("Mouse aim, WASD move, Space jump");
                            ui.label("Shift sneak, Ctrl sprint");
                            ui.label("LMB break, RMB place, E inventory, F eat, Esc menu");
                            ui.label("T chat, Tab playerlist, Q drop, F1 hide HUD, F3 debug, F5 camera");
                            // Day clock - map [0..DAY_LENGTH_S) onto
                            // 24h so the player has a sense of where
                            // the sun is.
                            let frac = (day_time / DAY_LENGTH_S).rem_euclid(1.0);
                            let hours_total = frac * 24.0;
                            let hh = hours_total.floor() as i32;
                            let mm = ((hours_total - hh as f32) * 60.0) as i32;
                            // Cardinal direction the player is
                            // facing. Yaw=0 -> east (+X); rotate
                            // by quadrants for the standard
                            // N/E/S/W readout.
                            let yaw_norm = dbg_yaw.rem_euclid(std::f32::consts::TAU);
                            let octant = (yaw_norm / std::f32::consts::FRAC_PI_4)
                                .round() as i32
                                & 7;
                            let dir = match octant {
                                0 => "E",
                                1 => "NE",
                                2 => "N",
                                3 => "NW",
                                4 => "W",
                                5 => "SW",
                                6 => "S",
                                _ => "SE",
                            };
                            ui.label(format!(
                                "Time: {hh:02}:{mm:02}    Kills: {kill_count}    Facing: {dir}    Biome: {biome_name}"
                            ));
                            ui.label(format!(
                                "XYZ: {:.0}, {:.0}, {:.0}",
                                dbg_player.x,
                                dbg_player.y,
                                dbg_player.z
                            ));
                            // Health row: 10 hearts of 2 HP each.
                            // A heart is full if `player_health` >=
                            // 2 * (i + 1), half if at the half-step,
                            // empty otherwise.
                            ui.horizontal(|ui| {
                                ui.label("HP:");
                                for i in 0..10 {
                                    let need_full = 2.0 * (i as f32 + 1.0);
                                    let need_half = need_full - 1.0;
                                    let glyph = if player_health >= need_full {
                                        "\u{2665}"
                                    } else if player_health >= need_half {
                                        "\u{2661}"
                                    } else {
                                        " "
                                    };
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 50, 50),
                                        glyph,
                                    );
                                }
                            });
                            // Stamina bar. Width tracks the
                            // current fraction; tint shifts from
                            // green at full down to orange at
                            // low. Hidden when sprinting is
                            // impossible (full + not in use).
                            if stamina_frac < 0.999 || self.sprinting {
                                ui.horizontal(|ui| {
                                    ui.label("SP:");
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(120.0, 8.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    painter.rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_rgba_unmultiplied(
                                            30, 30, 30, 220,
                                        ),
                                    );
                                    let fill = egui::Rect::from_min_size(
                                        rect.min,
                                        egui::vec2(
                                            rect.width() * stamina_frac,
                                            rect.height(),
                                        ),
                                    );
                                    let col = if stamina_frac > 0.5 {
                                        egui::Color32::from_rgb(100, 200, 80)
                                    } else if stamina_frac > 0.2 {
                                        egui::Color32::from_rgb(220, 180, 60)
                                    } else {
                                        egui::Color32::from_rgb(220, 90, 60)
                                    };
                                    painter.rect_filled(fill, 2.0, col);
                                });
                            }
                            if show_fps {
                                ui.separator();
                                ui.label(format!("FPS: {fps:.0}"));
                            }
                        });
                    });
                }
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
                                ui.label(format!("pending chunks: {dbg_pending}"));
                                ui.label(format!("mobs (remote): {dbg_mobs}"));
                                ui.label(format!("particles: {dbg_particles}"));
                                ui.label(format!(
                                    "camera: {dbg_cam}{}",
                                    if dbg_fly { "  fly" } else { "" }
                                ));
                                ui.label(format!("fps: {fps:.0}"));
                                ui.label(format!("net: {dbg_net}"));
                                let h = dbg_playtime / 3600;
                                let m = (dbg_playtime % 3600) / 60;
                                let s = dbg_playtime % 60;
                                ui.label(format!("session: {h:02}:{m:02}:{s:02}"));
                                ui.label(format!("seed: {}", world_seed()));
                            });
                        });
                }
                // Bottom hotbar - empty slot (= break) + one slot per
                // registered block. Highlighted slot is the active
                // selection (driven by mouse wheel). Clicks here
                // change the active slot; drag&drop is wired through
                // the inventory window.
                if hud_visible {
                egui::Area::new(egui::Id::new("voxel_hotbar"))
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.vertical_centered(|ui| {
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
                                // Row of digit hints under each
                                // slot. Highlights the active slot
                                // so the player can map their key
                                // press to the selected stack at a
                                // glance.
                                ui.horizontal(|ui| {
                                    for i in 0..HOTBAR_SLOTS {
                                        let label = if i < 9 {
                                            format!("{}", i + 1)
                                        } else {
                                            String::new()
                                        };
                                        let color = if i == selected_slot {
                                            egui::Color32::from_rgb(255, 220, 100)
                                        } else {
                                            egui::Color32::from_rgb(170, 170, 170)
                                        };
                                        ui.add_sized(
                                            [38.0, 14.0],
                                            egui::Label::new(
                                                egui::RichText::new(label)
                                                    .color(color)
                                                    .monospace(),
                                            ),
                                        );
                                    }
                                });
                            });
                        });
                    });
                }

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

                // Hurt-flash: full-screen red overlay that fades
                // out over ~0.5 s. Painted on the Foreground layer
                // so it sits above the world but below the
                // crosshair / inventory windows.
                if player_hurt_flash > 0.0 {
                    let alpha =
                        (player_hurt_flash / 0.5).clamp(0.0, 1.0).powf(1.3);
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_hurt_flash"),
                    ));
                    let rect = ctx.content_rect();
                    layer.rect_filled(
                        rect,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(
                            180,
                            0,
                            0,
                            (110.0 * alpha) as u8,
                        ),
                    );
                }
                // Low-HP vignette: a pulsing red border that
                // ramps in below 6 HP and intensifies the closer
                // the player is to dying. The pulse uses day_time
                // as a continuous clock so we don't have to plumb
                // dt into the egui closure.
                if hud_visible && player_health > 0.0 && player_health < 6.0 {
                    let danger = ((6.0 - player_health) / 6.0).clamp(0.0, 1.0);
                    let pulse = 0.55
                        + 0.45
                            * (day_time * std::f32::consts::TAU * 1.6).sin()
                                .abs();
                    let base_alpha = (40.0 + 110.0 * danger) * pulse;
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_low_hp_vignette"),
                    ));
                    let rect = ctx.content_rect();
                    let thickness = (rect.width().min(rect.height()) * 0.22)
                        .max(40.0);
                    // Four edge bands so the centre stays clear -
                    // a darker corner read than a flat tint.
                    let top = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.max.x, rect.min.y + thickness),
                    );
                    let bottom = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x, rect.max.y - thickness),
                        rect.max,
                    );
                    let left = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.min.x + thickness, rect.max.y),
                    );
                    let right = egui::Rect::from_min_max(
                        egui::pos2(rect.max.x - thickness, rect.min.y),
                        rect.max,
                    );
                    for r in [top, bottom, left, right] {
                        layer.rect_filled(
                            r,
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(
                                160,
                                0,
                                0,
                                (base_alpha as u8).min(180),
                            ),
                        );
                    }
                }
                // Star field: dimly visible at dusk, brighter at
                // midnight, gone during the day. Each star sits at
                // a fixed direction on the celestial sphere; we
                // project that direction (treated as a far-away
                // world point relative to the player) through the
                // HUD view-projection so the field rotates with
                // the camera but doesn't slide with translation.
                if night_factor > 0.0 && hud_visible {
                    const STAR_COUNT: u32 = 90;
                    const STAR_DIST: f32 = 250.0;
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Background,
                        egui::Id::new("voxel_stars"),
                    ));
                    let screen_rect = ctx.content_rect();
                    for i in 0..STAR_COUNT {
                        // Cheap deterministic direction: hash i to
                        // two angles + sample a hemisphere above
                        // the horizon (y > 0 in the celestial
                        // frame, rotated into world via day_time
                        // so the sky drifts).
                        let h1 =
                            (i.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                        let h2 = ((i.wrapping_mul(40503))
                            .wrapping_add(0x9E37_79B9))
                            as f32
                            / u32::MAX as f32;
                        let azimuth = h1 * std::f32::consts::TAU;
                        let elev = h2 * std::f32::consts::FRAC_PI_2 * 0.95
                            + 0.05;
                        // Rotate the star around +X by the sun
                        // phase so the sky slowly drifts overnight.
                        let drift = phase * std::f32::consts::TAU;
                        let base = glam::Vec3::new(
                            elev.cos() * azimuth.cos(),
                            elev.sin(),
                            elev.cos() * azimuth.sin(),
                        );
                        let drifted = glam::Vec3::new(
                            base.x,
                            base.y * drift.cos() - base.z * drift.sin(),
                            base.y * drift.sin() + base.z * drift.cos(),
                        );
                        if drifted.y <= 0.05 {
                            continue;
                        }
                        let world = glam::Vec3::new(
                            hud_cam_pos.x + drifted.x * STAR_DIST,
                            hud_cam_pos.y + drifted.y * STAR_DIST,
                            hud_cam_pos.z + drifted.z * STAR_DIST,
                        );
                        let clip = hud_view_proj
                            * glam::Vec4::new(world.x, world.y, world.z, 1.0);
                        if clip.w <= 0.0 {
                            continue;
                        }
                        let ndc = glam::Vec3::new(
                            clip.x / clip.w,
                            clip.y / clip.w,
                            clip.z / clip.w,
                        );
                        if !(0.0..=1.0).contains(&ndc.z) {
                            continue;
                        }
                        let sx = (ndc.x * 0.5 + 0.5) * screen_rect.width()
                            + screen_rect.min.x;
                        let sy = (-ndc.y * 0.5 + 0.5) * screen_rect.height()
                            + screen_rect.min.y;
                        // Per-star brightness jitter so the field
                        // doesn't look uniform.
                        let brightness = 0.55 + (h2 * 3.7).fract() * 0.45;
                        let alpha = (220.0 * night_factor * brightness) as u8;
                        layer.circle_filled(
                            egui::pos2(sx, sy),
                            1.4,
                            egui::Color32::from_rgba_unmultiplied(
                                235, 240, 255, alpha,
                            ),
                        );
                    }
                }
                // Floating mob HP bars. Projected onto the screen
                // from each mob's world-space pos + a vertical
                // offset so the bar sits a bit above the head.
                // We hide bars at full HP + during the death fade.
                let screen = ctx.content_rect();
                if hud_visible && !mob_bars.is_empty() {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_mob_hp"),
                    ));
                    for (pos, cur, max) in &mob_bars {
                        let world = glam::Vec3::new(pos[0], pos[1] + 1.55, pos[2]);
                        let clip = hud_view_proj
                            * glam::Vec4::new(world.x, world.y, world.z, 1.0);
                        if clip.w <= 0.0 {
                            continue;
                        }
                        let ndc = glam::Vec3::new(
                            clip.x / clip.w,
                            clip.y / clip.w,
                            clip.z / clip.w,
                        );
                        if !(0.0..=1.0).contains(&ndc.z) {
                            continue;
                        }
                        let sx = (ndc.x * 0.5 + 0.5) * screen.width()
                            + screen.min.x;
                        let sy = (-ndc.y * 0.5 + 0.5) * screen.height()
                            + screen.min.y;
                        // Scale bar width with depth so close
                        // enemies get a chunkier readout.
                        let depth_scale = (1.0 / clip.w * 30.0).clamp(0.25, 1.5);
                        let w = 50.0 * depth_scale;
                        let h = 5.0 * depth_scale;
                        let bg = egui::Rect::from_center_size(
                            egui::pos2(sx, sy),
                            egui::vec2(w, h),
                        );
                        layer.rect_filled(
                            bg,
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(40, 0, 0, 200),
                        );
                        let frac = (*cur as f32 / *max as f32).clamp(0.0, 1.0);
                        let fill = egui::Rect::from_min_size(
                            bg.min,
                            egui::vec2(bg.width() * frac, bg.height()),
                        );
                        let bar_col = if frac > 0.5 {
                            egui::Color32::from_rgb(100, 200, 80)
                        } else if frac > 0.25 {
                            egui::Color32::from_rgb(220, 180, 60)
                        } else {
                            egui::Color32::from_rgb(220, 70, 60)
                        };
                        layer.rect_filled(fill, 1.0, bar_col);
                        layer.rect_stroke(
                            bg,
                            1.0,
                            egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                // Floating nameplates above remote players. Same
                // projection trick as the mob HP bars; rendered
                // slightly higher than a sheep bar so it sits
                // clearly above the head cube. Hidden when the
                // player has toggled the HUD off (F1).
                if hud_visible && !nameplates.is_empty() {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_nameplates"),
                    ));
                    for (pos, name, color) in &nameplates {
                        let world = glam::Vec3::new(pos[0], pos[1] + 0.55, pos[2]);
                        let clip = hud_view_proj
                            * glam::Vec4::new(world.x, world.y, world.z, 1.0);
                        if clip.w <= 0.0 {
                            continue;
                        }
                        let ndc = glam::Vec3::new(
                            clip.x / clip.w,
                            clip.y / clip.w,
                            clip.z / clip.w,
                        );
                        if !(0.0..=1.0).contains(&ndc.z) {
                            continue;
                        }
                        let sx = (ndc.x * 0.5 + 0.5) * screen.width()
                            + screen.min.x;
                        let sy = (-ndc.y * 0.5 + 0.5) * screen.height()
                            + screen.min.y;
                        // Fade text size with depth so far-away
                        // names don't dominate the screen.
                        let depth_scale = (1.0 / clip.w * 30.0).clamp(0.4, 1.4);
                        let font = egui::FontId::proportional(13.0 * depth_scale);
                        // Draw a 1px black outline by stamping the
                        // text four times before the foreground
                        // pass, then the main text on top.
                        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                            layer.text(
                                egui::pos2(sx + dx, sy + dy),
                                egui::Align2::CENTER_BOTTOM,
                                name,
                                font.clone(),
                                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 220),
                            );
                        }
                        layer.text(
                            egui::pos2(sx, sy),
                            egui::Align2::CENTER_BOTTOM,
                            name,
                            font,
                            egui::Color32::from_rgb(color[0], color[1], color[2]),
                        );
                    }
                }
                // Tab playerlist overlay: centered modal listing
                // every known peer (incl. self) while Tab is held.
                if playerlist_visible && !playerlist.is_empty() {
                    egui::Area::new(egui::Id::new("voxel_playerlist"))
                        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 40.0))
                        .order(egui::Order::Foreground)
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    20, 20, 20, 210,
                                ))
                                .show(ui, |ui| {
                                    ui.set_min_width(180.0);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Players ({})",
                                            playerlist.len()
                                        ))
                                        .strong()
                                        .color(egui::Color32::from_rgb(
                                            255, 220, 100,
                                        )),
                                    );
                                    ui.separator();
                                    for (name, is_self) in &playerlist {
                                        let mut txt = egui::RichText::new(name);
                                        if *is_self {
                                            txt = txt
                                                .color(egui::Color32::from_rgb(
                                                    160, 230, 160,
                                                ))
                                                .strong();
                                        } else {
                                            txt = txt.color(
                                                egui::Color32::from_rgb(
                                                    230, 230, 230,
                                                ),
                                            );
                                        }
                                        ui.label(txt);
                                    }
                                });
                        });
                }
                // Targeted-block label: name + mining progress
                // (when actively breaking the same voxel). Skipped
                // when the crosshair already shows a mob label
                // since both would overlap.
                if hud_visible
                    && targeted_mob_info.is_none()
                    && let Some((name, pct)) = targeted_block_info
                {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_block_label"),
                    ));
                    let center = screen.center();
                    let text = match pct {
                        Some(p) => format!("{name}  {p}%"),
                        None => name.to_string(),
                    };
                    layer.text(
                        egui::pos2(center.x, center.y + 22.0),
                        egui::Align2::CENTER_TOP,
                        text,
                        egui::FontId::proportional(13.0),
                        egui::Color32::from_rgb(220, 220, 220),
                    );
                }
                // Targeted-mob label: small text just under the
                // crosshair when the player is aiming at a mob.
                // Hidden when no mob is in line of sight.
                if hud_visible
                    && let Some((name, cur, max)) = targeted_mob_info
                {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_mob_label"),
                    ));
                    let center = screen.center();
                    layer.text(
                        egui::pos2(center.x, center.y + 22.0),
                        egui::Align2::CENTER_TOP,
                        format!("{name}  {cur}/{max}"),
                        egui::FontId::proportional(14.0),
                        egui::Color32::from_rgb(240, 240, 240),
                    );
                }
                // Chunk-loading toast at the top centre while
                // a non-trivial batch is in flight. Cheaply
                // signals that the world is streaming in (e.g.
                // right after spawn or a /seed cycle).
                if hud_visible && dbg_pending > 8 {
                    egui::Area::new(egui::Id::new("voxel_loading"))
                        .anchor(egui::Align2::CENTER_TOP, [0.0, 14.0])
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    20, 20, 20, 200,
                                ))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Generating world... ({dbg_pending} chunks pending)"
                                        ))
                                        .color(egui::Color32::from_rgb(
                                            255, 215, 120,
                                        )),
                                    );
                                });
                        });
                }
                // Pickup toasts on the right edge so the chat
                // log + hotbar stay readable on the left + bottom.
                if hud_visible && !pickup_snapshot.is_empty() {
                    egui::Area::new(egui::Id::new("voxel_pickup_toasts"))
                        .anchor(egui::Align2::RIGHT_BOTTOM, [-18.0, -120.0])
                        .show(ctx, |ui| {
                            for (alpha, line, color) in &pickup_snapshot {
                                let a = (255.0 * alpha) as u8;
                                let r = (color[0] * 255.0) as u8;
                                let g = (color[1] * 255.0) as u8;
                                let b = (color[2] * 255.0) as u8;
                                // Brighten the swatch a touch so
                                // darker block colours (e.g. coal)
                                // still pop against the night sky.
                                let r2 = (r as u16 * 5 / 4).min(255) as u8;
                                let g2 = (g as u16 * 5 / 4).min(255) as u8;
                                let b2 = (b as u16 * 5 / 4).min(255) as u8;
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(12.0, 12.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_rgba_unmultiplied(
                                            r2, g2, b2, a,
                                        ),
                                    );
                                    ui.colored_label(
                                        egui::Color32::from_rgba_unmultiplied(
                                            245, 245, 245, a,
                                        ),
                                        line,
                                    );
                                });
                            }
                        });
                }
                // Chat: log of recent lines at bottom-left + an
                // input box when `chat_open`. Each log entry
                // fades out over ~10 s; messages older than that
                // are dropped after this pass.
                if hud_visible || chat_open {
                    egui::Area::new(egui::Id::new("voxel_chat"))
                        .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -90.0])
                        .show(ctx, |ui| {
                            for (line, age) in &chat_lines {
                                let alpha = ((10.0 - age) / 2.0).clamp(0.0, 1.0);
                                if alpha <= 0.0 {
                                    continue;
                                }
                                let a = (255.0 * alpha) as u8;
                                ui.colored_label(
                                    egui::Color32::from_rgba_unmultiplied(
                                        240, 240, 240, a,
                                    ),
                                    line,
                                );
                            }
                            if chat_open {
                                let resp = ui.add(
                                    egui::TextEdit::singleline(chat_buf)
                                        .desired_width(360.0)
                                        .hint_text("press T to chat, Enter sends"),
                                );
                                resp.request_focus();
                                // History scrolling via Up/Down.
                                // Older entries are at the start
                                // of the Vec, newest at the end;
                                // Up walks backward, Down walks
                                // forward. Down past the newest
                                // entry clears the line so the
                                // player can compose fresh text.
                                let up_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::ArrowUp));
                                let down_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::ArrowDown));
                                if !chat_history_ref.is_empty() {
                                    if up_pressed {
                                        let new_idx = match *chat_history_idx {
                                            None => chat_history_ref.len() - 1,
                                            Some(i) => i.saturating_sub(1),
                                        };
                                        *chat_history_idx = Some(new_idx);
                                        *chat_buf =
                                            chat_history_ref[new_idx].clone();
                                    } else if down_pressed {
                                        match *chat_history_idx {
                                            Some(i)
                                                if i + 1 < chat_history_ref.len() =>
                                            {
                                                *chat_history_idx = Some(i + 1);
                                                *chat_buf =
                                                    chat_history_ref[i + 1]
                                                        .clone();
                                            }
                                            Some(_) => {
                                                *chat_history_idx = None;
                                                chat_buf.clear();
                                            }
                                            None => {}
                                        }
                                    }
                                }
                                if resp.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    chat_send_now = true;
                                }
                            }
                        });
                }
                // Crosshair drawn directly on the egui screen layer.
                // Two thin segments through the screen centre.
                if hud_visible {
                    let center = screen.center();
                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_crosshair"),
                    ));
                    let stroke =
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(240, 240, 240));
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
            }
        });

        // Drop chat lines older than 10 s. Done after the render
        // pass so the egui closure had a chance to display them
        // one last frame.
        self.chat_log
            .retain(|(t, _)| chat_now.saturating_duration_since(*t).as_secs_f32() < 10.0);
        // Send the chat line if the user pressed Enter.
        if chat_send_now {
            let text = self.chat_input.trim().to_string();
            self.chat_input.clear();
            self.chat_open = false;
            self.chat_history_idx = None;
            if !text.is_empty() {
                // Push to history, dedup adjacent repeats so
                // hitting Up + Enter twice doesn't double-fill
                // the log.
                if self.chat_history.last() != Some(&text) {
                    self.chat_history.push(text.clone());
                    if self.chat_history.len() > 20 {
                        let drop = self.chat_history.len() - 20;
                        self.chat_history.drain(0..drop);
                    }
                }
                // Slash-command dispatch. Commands run locally
                // (no broadcast); everything else routes to chat
                // normally below.
                let mut handled_locally = false;
                if let Some(rest) = text.strip_prefix('/') {
                    let mut parts = rest.splitn(2, char::is_whitespace);
                    let cmd = parts.next().unwrap_or("").to_lowercase();
                    let arg = parts.next().unwrap_or("").trim();
                    let mut local_lines: Vec<String> = Vec::new();
                    match cmd.as_str() {
                        "help" | "?" => {
                            local_lines.push(
                                "Commands: /help, /players, /me <act>, /clear, /clearinventory, /tp <name>, /spawn, /summon <name>, /give <block> [n], /day, /night, /time <hh>, /sleep, /heal, /kill, /fly, /home, /sethome, /setblock <x> <y> <z> <block>, /w <name> <msg>"
                                    .to_string(),
                            );
                        }
                        "day" => {
                            // Snap day_time to mid-morning.
                            self.day_time = DAY_LENGTH_S * 0.25;
                            local_lines.push("Time set to morning.".to_string());
                            handled_locally = true;
                        }
                        "heal" => {
                            // Refill local HP. Resets the regen
                            // timer so the player doesn't get a
                            // double-tick the next frame.
                            self.player_health = PLAYER_MAX_HEALTH;
                            self.regen_accum = 0.0;
                            self.hurt_flash = 0.0;
                            self.heartbeat_accum = 0.0;
                            local_lines.push("Healed to full.".to_string());
                            handled_locally = true;
                        }
                        "kill" => {
                            // Force the player's HP to zero so
                            // the existing apply_player_damage
                            // death path runs (drops, respawn,
                            // chat broadcast). Useful for testing
                            // + when stuck in geometry.
                            let dmg = self.player_health + 1.0;
                            self.apply_player_damage(dmg);
                            handled_locally = true;
                        }
                        "seed" => {
                            // /seed prints the active value;
                            // /seed <n> (host-only) re-rolls the
                            // world. Adopting a new seed drops
                            // every generated chunk + broadcasts
                            // vx.seed so connected clients follow
                            // suit on the next net tick.
                            if arg.is_empty() {
                                local_lines.push(format!(
                                    "World seed: {}",
                                    world_seed()
                                ));
                            } else if !matches!(
                                self.net,
                                NetRole::Server { .. } | NetRole::Offline
                            ) {
                                local_lines.push(
                                    "/seed <n> is host-only.".to_string(),
                                );
                            } else if let Ok(new_seed) = arg.parse::<u32>() {
                                set_world_seed(new_seed);
                                let to_unload: Vec<ChunkCoord> = self
                                    .world
                                    .iter_chunks()
                                    .map(|(c, _)| *c)
                                    .collect();
                                if !to_unload.is_empty() {
                                    let renderer = self
                                        .renderer
                                        .as_mut()
                                        .expect("renderer ready");
                                    for entry in self.chunk_meshes.drain(..) {
                                        renderer.remove_mesh(entry.2);
                                    }
                                    for cc in &to_unload {
                                        self.world.remove_chunk(*cc);
                                    }
                                }
                                self.pending_chunks.clear();
                                self.pending_set.clear();
                                if let NetRole::Server { server, .. } =
                                    &mut self.net
                                    && let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            new_seed,
                                            bincode::config::standard(),
                                        )
                                {
                                    server.broadcast_bytes("vx.seed", buf);
                                }
                                local_lines.push(format!(
                                    "World seed set to {new_seed}; chunks regenerating."
                                ));
                            } else {
                                local_lines.push(format!(
                                    "Cannot parse seed: {arg}"
                                ));
                            }
                            handled_locally = true;
                        }
                        "uptime" => {
                            // Session timer formatted as hh:mm:ss.
                            let secs = self
                                .session_started
                                .map(|t| t.elapsed().as_secs())
                                .unwrap_or(0);
                            let h = secs / 3600;
                            let m = (secs % 3600) / 60;
                            let s = secs % 60;
                            local_lines.push(format!(
                                "Uptime: {h:02}:{m:02}:{s:02}"
                            ));
                            handled_locally = true;
                        }
                        "version" | "ver" => {
                            // Print the build's CARGO_PKG_VERSION
                            // plus the active net role so quick
                            // diagnostic chat doesn't need F3.
                            let role = match &self.net {
                                NetRole::Offline => "offline",
                                NetRole::Server { .. } => "host",
                                NetRole::Client { .. } => "client",
                            };
                            local_lines.push(format!(
                                "voxel_demo_mp v{}  [{role}]",
                                env!("CARGO_PKG_VERSION")
                            ));
                            handled_locally = true;
                        }
                        "save" => {
                            // Host-only: flush the server state
                            // (edit history + inventories + day
                            // clock + HP + kill count) to disk
                            // right now instead of waiting for
                            // the autosave cadence.
                            if !matches!(self.net, NetRole::Server { .. }) {
                                local_lines.push(
                                    "/save is host-only.".to_string(),
                                );
                            } else {
                                self.save_server_state();
                                local_lines
                                    .push("Server state saved.".to_string());
                            }
                            handled_locally = true;
                        }
                        "dmg" | "damage" => {
                            // Host-only: deal damage to a named
                            // peer or self. Usage: /dmg <name>
                            // <amount>. Routes through the same
                            // vx.damage path PvP uses.
                            if !matches!(self.net, NetRole::Server { .. } | NetRole::Offline) {
                                local_lines.push(
                                    "/dmg is host-only.".to_string(),
                                );
                            } else {
                                let mut parts = arg.split_whitespace();
                                let target = parts.next().unwrap_or("");
                                let amount: f32 = parts
                                    .next()
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(2.0);
                                if target.is_empty() {
                                    local_lines.push(
                                        "Usage: /dmg <name> [amount]"
                                            .to_string(),
                                    );
                                } else {
                                    let target_id = self
                                        .player_names
                                        .iter()
                                        .find(|(_, n)| {
                                            n.eq_ignore_ascii_case(target)
                                        })
                                        .map(|(id, _)| *id);
                                    match target_id {
                                        None => {
                                            local_lines.push(format!(
                                                "No player named {target}."
                                            ));
                                        }
                                        Some(0) => {
                                            self.apply_player_damage(amount);
                                            local_lines.push(format!(
                                                "Self-damaged {amount}."
                                            ));
                                        }
                                        Some(id) => {
                                            let cfg =
                                                bincode::config::standard();
                                            if let NetRole::Server {
                                                server,
                                                ..
                                            } = &mut self.net
                                                && let Ok(buf) =
                                                    bincode::serde::encode_to_vec(
                                                        DamageMsg { amount },
                                                        cfg,
                                                    )
                                            {
                                                server.send_bytes_to_client(
                                                    id,
                                                    "vx.damage",
                                                    buf,
                                                );
                                            }
                                            local_lines.push(format!(
                                                "Damaged {target} for {amount}."
                                            ));
                                        }
                                    }
                                }
                                handled_locally = true;
                            }
                        }
                        "killmobs" => {
                            // Host-only: nuke every live mob.
                            // Pre-bleed HP to one hit so the
                            // damage_mob call triggers the death
                            // path the normal way (drops, remove
                            // broadcast, event).
                            if !matches!(self.net, NetRole::Server { .. } | NetRole::Offline) {
                                local_lines.push(
                                    "/killmobs is host-only.".to_string(),
                                );
                            } else {
                                let ids: Vec<u32> =
                                    self.mobs.iter().map(|m| m.id).collect();
                                let n = ids.len();
                                for id in ids {
                                    if let Some(m) = self
                                        .mobs
                                        .iter_mut()
                                        .find(|m| m.id == id)
                                    {
                                        m.health = MOB_HIT_DAMAGE;
                                    }
                                    self.damage_mob(id, None);
                                }
                                local_lines.push(format!("Killed {n} mobs."));
                            }
                            handled_locally = true;
                        }
                        "horde" => {
                            // Host-only: spawn N zombies around
                            // the player in one go. Defaults to
                            // 5; clamped 1..=20. Useful for
                            // testing combat scenarios. Same
                            // ring-spawn logic the spawner uses.
                            if !matches!(
                                self.net,
                                NetRole::Server { .. } | NetRole::Offline
                            ) {
                                local_lines.push(
                                    "/horde is host-only.".to_string(),
                                );
                            } else {
                                let n: usize = arg
                                    .parse()
                                    .ok()
                                    .unwrap_or(5usize)
                                    .clamp(1, 20);
                                let mut spawned = 0;
                                for i in 0..n {
                                    let angle = (i as f32 / n as f32)
                                        * std::f32::consts::TAU;
                                    let radius = 6.0;
                                    let cx = self.player_pos.x
                                        + angle.cos() * radius;
                                    let cz = self.player_pos.z
                                        + angle.sin() * radius;
                                    let pos = [cx, self.player_pos.y, cz];
                                    let id = self.next_mob_id;
                                    self.next_mob_id = self
                                        .next_mob_id
                                        .wrapping_add(1)
                                        .max(1);
                                    let mob = MobState {
                                        id,
                                        kind: KIND_ZOMBIE,
                                        pos,
                                        target: pos,
                                        yaw: 0.0,
                                        retarget_at: (self.day_time + 1.0)
                                            .rem_euclid(DAY_LENGTH_S),
                                        health: ZOMBIE_MAX_HEALTH,
                                        dying: 0.0,
                                        attack_cd: 0.0,
                                    };
                                    self.mobs.push(mob);
                                    self.remote_mobs.insert(
                                        id,
                                        MobMsg {
                                            id,
                                            kind: KIND_ZOMBIE,
                                            pos,
                                            yaw: 0.0,
                                            health: ZOMBIE_MAX_HEALTH as u8,
                                        },
                                    );
                                    spawned += 1;
                                }
                                local_lines.push(format!(
                                    "Spawned {spawned} zombies."
                                ));
                            }
                            handled_locally = true;
                        }
                        "spawnmob" => {
                            // Host-only: spawn a sheep or zombie
                            // a few blocks in front of the player.
                            // Clients can't run this since the
                            // server owns mob state.
                            if !matches!(self.net, NetRole::Server { .. } | NetRole::Offline) {
                                local_lines.push(
                                    "/spawnmob is host-only.".to_string(),
                                );
                            } else {
                                let kind = if arg.eq_ignore_ascii_case("zombie") {
                                    KIND_ZOMBIE
                                } else {
                                    KIND_SHEEP
                                };
                                let fwd_h = Vec3::new(
                                    self.player_yaw.cos(),
                                    0.0,
                                    -self.player_yaw.sin(),
                                )
                                .normalize_or_zero();
                                let spawn_at = self.player_pos + fwd_h * 3.0;
                                let id = self.next_mob_id;
                                self.next_mob_id =
                                    self.next_mob_id.wrapping_add(1).max(1);
                                let pos = [
                                    spawn_at.x,
                                    spawn_at.y,
                                    spawn_at.z,
                                ];
                                let mob = MobState {
                                    id,
                                    kind,
                                    pos,
                                    target: pos,
                                    yaw: 0.0,
                                    retarget_at: (self.day_time + 1.0)
                                        .rem_euclid(DAY_LENGTH_S),
                                    health: if kind == KIND_ZOMBIE {
                                        ZOMBIE_MAX_HEALTH
                                    } else {
                                        MOB_MAX_HEALTH
                                    },
                                    dying: 0.0,
                                    attack_cd: 0.0,
                                };
                                let initial_hp = if kind == KIND_ZOMBIE {
                                    ZOMBIE_MAX_HEALTH
                                } else {
                                    MOB_MAX_HEALTH
                                } as u8;
                                self.mobs.push(mob);
                                self.remote_mobs.insert(
                                    id,
                                    MobMsg {
                                        id,
                                        kind,
                                        pos,
                                        yaw: 0.0,
                                        health: initial_hp,
                                    },
                                );
                                local_lines.push(format!(
                                    "Spawned {}.",
                                    if kind == KIND_ZOMBIE {
                                        "zombie"
                                    } else {
                                        "sheep"
                                    }
                                ));
                            }
                            handled_locally = true;
                        }
                        "resetinventory" | "resetinv" => {
                            // Restore the freshly-seeded starter
                            // inventory: hotbar slots 1..=N get
                            // one full stack of each known block,
                            // slot 0 + storage stays empty. Mostly
                            // useful after /clearinventory in
                            // local play.
                            for s in self.inventory.iter_mut() {
                                *s = None;
                            }
                            for (i, bt) in BLOCK_TYPES.iter().enumerate() {
                                let slot = i + 1;
                                if slot < HOTBAR_SLOTS {
                                    self.inventory[slot] =
                                        Some((BlockId(bt.id), STACK_MAX));
                                }
                            }
                            for s in self.craft_grid.iter_mut() {
                                *s = None;
                            }
                            local_lines.push(
                                "Inventory reset to starter.".to_string(),
                            );
                            handled_locally = true;
                        }
                        "clearinventory" | "clearinv" => {
                            // Empty every slot including hotbar
                            // + craft grid. Useful for testing.
                            // In MP this only zeroes the local
                            // predicted view; the next vx.inv
                            // snapshot from the host wipes it.
                            for s in self.inventory.iter_mut() {
                                *s = None;
                            }
                            for s in self.craft_grid.iter_mut() {
                                *s = None;
                            }
                            local_lines.push(
                                "Inventory cleared.".to_string(),
                            );
                            handled_locally = true;
                        }
                        "sleep" => {
                            // Skip the night: if the sun is below
                            // the horizon, fast-forward day_time
                            // to morning. Daytime use is a no-op
                            // (matches the MC bed rule). Host-only
                            // in MP so the night gets skipped for
                            // every connected player via the
                            // existing day_time broadcast.
                            let phase = (self.day_time / DAY_LENGTH_S)
                                .rem_euclid(1.0);
                            let raw_elev =
                                (phase * std::f32::consts::TAU).sin();
                            let is_night = raw_elev < -0.05;
                            if !is_night {
                                local_lines.push(
                                    "You can only sleep at night.".to_string(),
                                );
                            } else if matches!(self.net, NetRole::Client { .. }) {
                                local_lines.push(
                                    "Only the host can advance time.".to_string(),
                                );
                            } else {
                                self.day_time = DAY_LENGTH_S * 0.20;
                                self.last_day_broadcast = None; // force resend
                                local_lines.push(
                                    "You slept until morning.".to_string(),
                                );
                            }
                            handled_locally = true;
                        }
                        "sethome" => {
                            // Stash the player's current pose for
                            // /home recall. Saved to disk along
                            // with the rest of the config on the
                            // next Settings exit.
                            self.home_pos = Some([
                                self.player_pos.x,
                                self.player_pos.y,
                                self.player_pos.z,
                            ]);
                            self.save_config();
                            local_lines.push("Home set.".to_string());
                            handled_locally = true;
                        }
                        "home" => {
                            // Teleport back to the saved home, if
                            // any. Velocity zeroed so no fall
                            // damage on landing.
                            if let Some(p) = self.home_pos {
                                self.player_pos =
                                    Vec3::new(p[0], p[1], p[2]);
                                self.player_vel = Vec3::ZERO;
                                self.fall_start_y = None;
                                local_lines
                                    .push("Teleported home.".to_string());
                            } else {
                                local_lines.push(
                                    "No home set. Use /sethome first."
                                        .to_string(),
                                );
                            }
                            handled_locally = true;
                        }
                        "setblock" => {
                            // /setblock x y z block: place a
                            // block at exact world coords. Routes
                            // through the normal edit path so the
                            // change persists in the edit history
                            // + replicates to peers. "air"
                            // clears the cell.
                            let parts: Vec<&str> = arg.split_whitespace().collect();
                            if parts.len() != 4 {
                                local_lines.push(
                                    "Usage: /setblock <x> <y> <z> <block>"
                                        .to_string(),
                                );
                            } else {
                                let x = parts[0].parse::<i32>();
                                let y = parts[1].parse::<i32>();
                                let z = parts[2].parse::<i32>();
                                let block_id: Option<u16> = if parts[3]
                                    .eq_ignore_ascii_case("air")
                                {
                                    Some(0)
                                } else {
                                    BLOCK_TYPES
                                        .iter()
                                        .find(|bt| {
                                            bt.name.eq_ignore_ascii_case(
                                                parts[3],
                                            )
                                        })
                                        .map(|bt| bt.id)
                                };
                                match (x, y, z, block_id) {
                                    (Ok(x), Ok(y), Ok(z), Some(bid)) => {
                                        let voxel = [x, y, z];
                                        let block = BlockId(bid);
                                        let edit = EditMsg {
                                            x,
                                            y,
                                            z,
                                            block: bid,
                                        };
                                        self.world_set_tracked(voxel, block);
                                        let coords = Self::affected_chunks(voxel);
                                        self.rebuild_chunks(&coords);
                                        let cfg = bincode::config::standard();
                                        match &mut self.net {
                                            NetRole::Offline => {}
                                            NetRole::Server { server, .. } => {
                                                self.edit_history.push(edit);
                                                if let Ok(buf) =
                                                    bincode::serde::encode_to_vec(
                                                        edit, cfg,
                                                    )
                                                {
                                                    server.broadcast_bytes(
                                                        "vx.edit",
                                                        buf,
                                                    );
                                                }
                                            }
                                            NetRole::Client {
                                                pending_edits, ..
                                            } => {
                                                pending_edits.push(edit);
                                            }
                                        }
                                        local_lines.push(format!(
                                            "Set ({x},{y},{z}) = {}",
                                            parts[3]
                                        ));
                                    }
                                    _ => {
                                        local_lines.push(
                                            "Bad args. Coords must be ints + block must exist."
                                                .to_string(),
                                        );
                                    }
                                }
                                handled_locally = true;
                            }
                        }
                        "explode" => {
                            // Host-only: blast a sphere of voxels
                            // to air around the player. Default
                            // radius 4. Routes every cleared cell
                            // through the normal edit pipeline so
                            // the change replicates + saves.
                            if !matches!(
                                self.net,
                                NetRole::Server { .. } | NetRole::Offline
                            ) {
                                local_lines.push(
                                    "/explode is host-only.".to_string(),
                                );
                            } else {
                                let radius: i32 = arg
                                    .parse()
                                    .ok()
                                    .unwrap_or(4i32)
                                    .clamp(1, 16);
                                let center = [
                                    self.player_pos.x.floor() as i32,
                                    self.player_pos.y.floor() as i32,
                                    self.player_pos.z.floor() as i32,
                                ];
                                let mut cleared = 0u32;
                                let r2 = radius * radius;
                                let cfg = bincode::config::standard();
                                for dy in -radius..=radius {
                                    for dz in -radius..=radius {
                                        for dx in -radius..=radius {
                                            if dx * dx + dy * dy + dz * dz
                                                > r2
                                            {
                                                continue;
                                            }
                                            let v = [
                                                center[0] + dx,
                                                center[1] + dy,
                                                center[2] + dz,
                                            ];
                                            if v[1] < 1 {
                                                // Skip the bedrock
                                                // floor so the
                                                // crater doesn't
                                                // open into the
                                                // void.
                                                continue;
                                            }
                                            if self.world.get(v).0 == 0 {
                                                continue;
                                            }
                                            self.world_set_tracked(
                                                v,
                                                BlockId::AIR,
                                            );
                                            cleared += 1;
                                            let edit = EditMsg {
                                                x: v[0],
                                                y: v[1],
                                                z: v[2],
                                                block: 0,
                                            };
                                            if let NetRole::Server {
                                                server,
                                                ..
                                            } = &mut self.net
                                            {
                                                self.edit_history.push(edit);
                                                if let Ok(buf) =
                                                    bincode::serde::encode_to_vec(
                                                        edit, cfg,
                                                    )
                                                {
                                                    server.broadcast_bytes(
                                                        "vx.edit",
                                                        buf,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                // Remesh every chunk the
                                // explosion bounding box touches
                                // (plus its 1-cell halo).
                                let cs = CHUNK_SIZE as i32;
                                let min_cc = [
                                    (center[0] - radius - 1).div_euclid(cs),
                                    (center[1] - radius - 1).div_euclid(cs),
                                    (center[2] - radius - 1).div_euclid(cs),
                                ];
                                let max_cc = [
                                    (center[0] + radius + 1).div_euclid(cs),
                                    (center[1] + radius + 1).div_euclid(cs),
                                    (center[2] + radius + 1).div_euclid(cs),
                                ];
                                let mut coords: Vec<ChunkCoord> = Vec::new();
                                for cz in min_cc[2]..=max_cc[2] {
                                    for cy in min_cc[1]..=max_cc[1] {
                                        for cx in min_cc[0]..=max_cc[0] {
                                            coords.push([cx, cy, cz]);
                                        }
                                    }
                                }
                                self.rebuild_chunks(&coords);
                                local_lines.push(format!(
                                    "Exploded {cleared} blocks (r={radius})."
                                ));
                            }
                            handled_locally = true;
                        }
                        "mobcap" => {
                            // /mobcap reports the cap; /mobcap N
                            // sets it. Hard ceiling at 64 so the
                            // server doesn't blow up its tick
                            // budget; floor at 0 to allow a
                            // disabled spawner.
                            if arg.is_empty() {
                                local_lines.push(format!(
                                    "Mob cap: {} (live: {})",
                                    self.mob_cap,
                                    self.mobs.len()
                                ));
                            } else if let Ok(v) = arg.parse::<usize>() {
                                let new_cap = v.min(64);
                                self.mob_cap = new_cap;
                                local_lines.push(format!(
                                    "Mob cap set to {new_cap}."
                                ));
                            } else {
                                local_lines.push(format!(
                                    "Cannot parse mob cap: {arg}"
                                ));
                            }
                            handled_locally = true;
                        }
                        "weather" => {
                            // /weather clear | rain. Sets the
                            // local state immediately. If we're
                            // hosting, also broadcast a vx.weather
                            // packet so connected clients flip
                            // their visuals to match.
                            let kind = match arg.to_lowercase().as_str() {
                                "rain" => Some(1u8),
                                "snow" => Some(2u8),
                                "clear" | "" => Some(0u8),
                                _ => None,
                            };
                            match kind {
                                Some(k) => {
                                    self.weather = k;
                                    if let NetRole::Server { server, .. } =
                                        &mut self.net
                                        && let Ok(buf) =
                                            bincode::serde::encode_to_vec(
                                                k,
                                                bincode::config::standard(),
                                            )
                                    {
                                        server.broadcast_bytes("vx.weather", buf);
                                    }
                                    local_lines.push(format!(
                                        "Weather: {}",
                                        match k {
                                            1 => "rain",
                                            2 => "snow",
                                            _ => "clear",
                                        }
                                    ));
                                }
                                None => {
                                    local_lines.push(
                                        "Usage: /weather clear|rain"
                                            .to_string(),
                                    );
                                }
                            }
                            handled_locally = true;
                        }
                        "freezetime" | "freeze" => {
                            self.freeze_time = !self.freeze_time;
                            local_lines.push(format!(
                                "Time clock {}",
                                if self.freeze_time {
                                    "PAUSED"
                                } else {
                                    "RUNNING"
                                }
                            ));
                            handled_locally = true;
                        }
                        "godmode" | "god" => {
                            self.god_mode = !self.god_mode;
                            local_lines.push(format!(
                                "Godmode {}",
                                if self.god_mode { "ON" } else { "OFF" }
                            ));
                            handled_locally = true;
                        }
                        "fly" => {
                            // Toggle the local fly mode flag. The
                            // movement integrator reads it; off
                            // by default so a vanilla player
                            // experience is unchanged.
                            self.fly_mode = !self.fly_mode;
                            if self.fly_mode {
                                self.player_vel = Vec3::ZERO;
                                self.fall_start_y = None;
                                local_lines.push(
                                    "Fly mode ON. Space = up, Shift = down."
                                        .to_string(),
                                );
                            } else {
                                local_lines
                                    .push("Fly mode OFF.".to_string());
                            }
                            handled_locally = true;
                        }
                        "night" => {
                            // Snap day_time to midnight.
                            self.day_time = DAY_LENGTH_S * 0.75;
                            local_lines.push("Time set to night.".to_string());
                            handled_locally = true;
                        }
                        "time" => {
                            // /time <hh>: jump to a specific
                            // 24-hour mark. Accepts 0..24, wraps
                            // anything else.
                            if arg.is_empty() {
                                let frac = (self.day_time / DAY_LENGTH_S)
                                    .rem_euclid(1.0);
                                let hours = frac * 24.0;
                                let hh = hours.floor() as i32;
                                let mm = ((hours - hh as f32) * 60.0) as i32;
                                local_lines.push(format!(
                                    "Current time: {hh:02}:{mm:02}"
                                ));
                            } else if let Ok(hour) = arg.parse::<f32>() {
                                let h = hour.rem_euclid(24.0);
                                self.day_time = DAY_LENGTH_S * (h / 24.0);
                                local_lines.push(format!(
                                    "Time set to {:.1}h.",
                                    h
                                ));
                            } else {
                                local_lines.push(format!(
                                    "Cannot parse hour: {arg}"
                                ));
                            }
                            handled_locally = true;
                        }
                        "give" => {
                            // Add stacks of a named block to the
                            // local inventory. In MP this only
                            // affects the host's authoritative
                            // copy; a connected client running
                            // /give just edits their predicted
                            // view (the next vx.inv snapshot
                            // wipes it). Mostly useful for
                            // testing + creative play.
                            if arg.is_empty() {
                                local_lines.push(
                                    "Usage: /give <block> [count]"
                                        .to_string(),
                                );
                            } else {
                                let mut parts = arg.split_whitespace();
                                let block_name = parts.next().unwrap_or("");
                                let count: u32 = parts
                                    .next()
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(64);
                                let block_id = BLOCK_TYPES
                                    .iter()
                                    .find(|bt| {
                                        bt.name.eq_ignore_ascii_case(block_name)
                                    })
                                    .map(|bt| bt.id);
                                if let Some(id) = block_id {
                                    let bid = BlockId(id);
                                    let mut added = 0u32;
                                    while added < count {
                                        let before = self
                                            .inventory
                                            .iter()
                                            .filter_map(|s| s.map(|(_, c)| c as u32))
                                            .sum::<u32>();
                                        self.pickup(bid);
                                        let after = self
                                            .inventory
                                            .iter()
                                            .filter_map(|s| s.map(|(_, c)| c as u32))
                                            .sum::<u32>();
                                        if after <= before {
                                            // Inventory was
                                            // full; stop early so
                                            // we don't infinite
                                            // loop.
                                            break;
                                        }
                                        added += 1;
                                    }
                                    local_lines.push(format!(
                                        "Gave {added} x {block_name}."
                                    ));
                                } else {
                                    local_lines.push(format!(
                                        "Unknown block: {block_name}"
                                    ));
                                }
                                handled_locally = true;
                            }
                        }
                        "summon" => {
                            // Host-only: yank a remote peer to
                            // the host's pose by sending them a
                            // vx.teleport. Silently ignored on
                            // any other role to keep clients from
                            // grabbing each other.
                            if !matches!(self.net, NetRole::Server { .. }) {
                                local_lines.push(
                                    "/summon is host-only.".to_string(),
                                );
                            } else if arg.is_empty() {
                                local_lines
                                    .push("Usage: /summon <name>".to_string());
                            } else {
                                let target_id = self
                                    .player_names
                                    .iter()
                                    .find(|(_, n)| {
                                        n.eq_ignore_ascii_case(arg)
                                    })
                                    .map(|(id, _)| *id);
                                if let Some(id) = target_id {
                                    if id == 0 {
                                        local_lines.push(
                                            "Can't summon yourself."
                                                .to_string(),
                                        );
                                    } else {
                                        let msg = TeleportMsg {
                                            pos: [
                                                self.player_pos.x,
                                                self.player_pos.y + 0.1,
                                                self.player_pos.z,
                                            ],
                                        };
                                        if let NetRole::Server {
                                            server, ..
                                        } = &mut self.net
                                            && let Ok(buf) =
                                                bincode::serde::encode_to_vec(
                                                    msg,
                                                    bincode::config::standard(),
                                                )
                                        {
                                            server.send_bytes_to_client(
                                                id,
                                                "vx.teleport",
                                                buf,
                                            );
                                        }
                                        local_lines.push(format!(
                                            "Summoned {arg}."
                                        ));
                                    }
                                } else {
                                    local_lines
                                        .push(format!("No player named {arg}."));
                                }
                                handled_locally = true;
                            }
                        }
                        "tppos" | "goto" => {
                            // Teleport to literal coords. Usage:
                            // /tppos <x> <y> <z>. Velocity zeroed
                            // + fall tracker reset so no fall
                            // damage on landing.
                            let coords: Vec<&str> =
                                arg.split_whitespace().collect();
                            if coords.len() != 3 {
                                local_lines.push(
                                    "Usage: /tppos <x> <y> <z>".to_string(),
                                );
                            } else {
                                match (
                                    coords[0].parse::<f32>(),
                                    coords[1].parse::<f32>(),
                                    coords[2].parse::<f32>(),
                                ) {
                                    (Ok(x), Ok(y), Ok(z)) => {
                                        self.player_pos = Vec3::new(x, y, z);
                                        self.player_vel = Vec3::ZERO;
                                        self.fall_start_y = None;
                                        local_lines.push(format!(
                                            "Teleported to ({x:.1}, {y:.1}, {z:.1})."
                                        ));
                                    }
                                    _ => {
                                        local_lines.push(
                                            "Coords must be numbers."
                                                .to_string(),
                                        );
                                    }
                                }
                            }
                            handled_locally = true;
                        }
                        "pos" => {
                            // Print precise pose. Pairs with the
                            // HUD-line XYZ readout (which is
                            // rounded) for when the player needs
                            // exact coordinates to /tppos back to.
                            local_lines.push(format!(
                                "Pos: ({:.2}, {:.2}, {:.2})  yaw: {:.2}  pitch: {:.2}",
                                self.player_pos.x,
                                self.player_pos.y,
                                self.player_pos.z,
                                self.player_yaw,
                                self.player_pitch,
                            ));
                            handled_locally = true;
                        }
                        "mute" => {
                            if arg.is_empty() {
                                if self.muted_players.is_empty() {
                                    local_lines.push(
                                        "No one muted.".to_string(),
                                    );
                                } else {
                                    let mut names: Vec<String> = self
                                        .muted_players
                                        .iter()
                                        .cloned()
                                        .collect();
                                    names.sort();
                                    local_lines.push(format!(
                                        "Muted: {}",
                                        names.join(", ")
                                    ));
                                }
                            } else {
                                self.muted_players
                                    .insert(arg.to_lowercase());
                                local_lines
                                    .push(format!("Muted {arg}."));
                            }
                            handled_locally = true;
                        }
                        "unmute" => {
                            if arg.is_empty() {
                                self.muted_players.clear();
                                local_lines.push(
                                    "All mutes cleared.".to_string(),
                                );
                            } else {
                                self.muted_players
                                    .remove(&arg.to_lowercase());
                                local_lines.push(format!("Unmuted {arg}."));
                            }
                            handled_locally = true;
                        }
                        "wave" | "shrug" | "dance" => {
                            // Built-in emote shortcuts: each one
                            // is a canned /me line broadcast via
                            // the existing chat path. Keeps the
                            // chat log readable + lets remote
                            // peers see the action.
                            let line = match cmd.as_str() {
                                "wave" => "waves",
                                "shrug" => {
                                    "shrugs (\\_(\u{30C4})_/)"
                                }
                                _ => "dances",
                            };
                            let cfg = bincode::config::standard();
                            match &mut self.net {
                                NetRole::Offline => {
                                    self.chat_log.push((
                                        std::time::Instant::now(),
                                        format!(
                                            "* {} {line}",
                                            self.player_name
                                        ),
                                    ));
                                }
                                NetRole::Server { server, .. } => {
                                    let line = format!(
                                        "* {} {line}",
                                        self.player_name
                                    );
                                    self.chat_log.push((
                                        std::time::Instant::now(),
                                        line.clone(),
                                    ));
                                    if let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            &line, cfg,
                                        )
                                    {
                                        server.broadcast_bytes("vx.chat", buf);
                                    }
                                }
                                NetRole::Client { client, .. } => {
                                    let emote = format!("* {line}");
                                    if let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            &emote, cfg,
                                        )
                                    {
                                        let _ = client
                                            .send_bytes("vx.chat", buf);
                                    }
                                }
                            }
                            handled_locally = true;
                        }
                        "fw" | "firework" => {
                            // Spawn a vertical rocket trail of
                            // particles from the player's feet
                            // that "explodes" in a multi-colour
                            // burst near apex. Cosmetic; nothing
                            // is persisted.
                            let base = [
                                self.player_pos.x,
                                self.player_pos.y - 0.6,
                                self.player_pos.z,
                            ];
                            // Streak: a column of fading orange.
                            for i in 0..20 {
                                let t = i as f32 / 20.0;
                                self.particles.particles.push(
                                    infengine_particles::Particle {
                                        position: [
                                            base[0],
                                            base[1] + t * 9.0,
                                            base[2],
                                        ],
                                        velocity: [0.0, 6.0, 0.0],
                                        color: [
                                            1.0,
                                            0.7 - t * 0.4,
                                            0.2,
                                            0.9 - t * 0.6,
                                        ],
                                        size: 0.18,
                                        age: 0.0,
                                        lifetime: 0.7 - t * 0.4,
                                    },
                                );
                            }
                            // Burst: scattered sparks at the
                            // apex with hash-derived hue.
                            let apex = [base[0], base[1] + 9.0, base[2]];
                            let mut rng: u32 = ((self.net_tick as u32)
                                .wrapping_mul(0x9E37_79B1))
                                ^ 0xFAFAFA;
                            let mut rand01 = || -> f32 {
                                rng ^= rng << 13;
                                rng ^= rng >> 17;
                                rng ^= rng << 5;
                                (rng as f32 / u32::MAX as f32)
                                    .clamp(0.0, 1.0)
                            };
                            let hue_base = rand01();
                            for _ in 0..60 {
                                let theta = rand01() * std::f32::consts::TAU;
                                let phi = (rand01() - 0.5)
                                    * std::f32::consts::PI;
                                let speed = 5.0 + rand01() * 3.0;
                                let vx = theta.cos() * phi.cos() * speed;
                                let vy = phi.sin() * speed;
                                let vz = theta.sin() * phi.cos() * speed;
                                let h =
                                    (hue_base + rand01() * 0.2).fract();
                                let tau = std::f32::consts::TAU;
                                let r =
                                    (0.5 + 0.5 * (h * tau).sin()).clamp(0.0, 1.0);
                                let g = (0.5
                                    + 0.5 * ((h + 0.33) * tau).sin())
                                    .clamp(0.0, 1.0);
                                let b = (0.5
                                    + 0.5 * ((h + 0.66) * tau).sin())
                                    .clamp(0.0, 1.0);
                                self.particles.particles.push(
                                    infengine_particles::Particle {
                                        position: apex,
                                        velocity: [vx, vy, vz],
                                        color: [r, g, b, 1.0],
                                        size: 0.18,
                                        age: 0.0,
                                        lifetime: 1.6,
                                    },
                                );
                            }
                            self.sfx_at("firework", base, |a, g| {
                                let _ = a.play_thud(180.0, 0.3 * g);
                            });
                            local_lines.push("Boom.".to_string());
                            handled_locally = true;
                        }
                        "announce" => {
                            // Host-only system broadcast.
                            // Prefixed `[SERVER]` so peers can
                            // distinguish operator messages from
                            // regular chat. Body is sent through
                            // vx.chat directly so the recipient
                            // chat path treats it like any other
                            // line.
                            if !matches!(self.net, NetRole::Server { .. }) {
                                local_lines.push(
                                    "/announce is host-only.".to_string(),
                                );
                            } else if arg.is_empty() {
                                local_lines.push(
                                    "Usage: /announce <message>"
                                        .to_string(),
                                );
                            } else {
                                let cfg = bincode::config::standard();
                                let line = format!("[SERVER] {arg}");
                                self.chat_log.push((
                                    std::time::Instant::now(),
                                    line.clone(),
                                ));
                                if let NetRole::Server { server, .. } =
                                    &mut self.net
                                    && let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            &line, cfg,
                                        )
                                {
                                    server.broadcast_bytes("vx.chat", buf);
                                }
                            }
                            handled_locally = true;
                        }
                        "worldgen" | "gen" => {
                            // /worldgen with no arg prints the
                            // current mode; with classic / flat /
                            // islands / amplified it switches +
                            // regenerates every cached chunk.
                            // Host-only since the workers share
                            // the WORLDGEN atomic.
                            if !matches!(
                                self.net,
                                NetRole::Server { .. } | NetRole::Offline
                            ) {
                                local_lines.push(
                                    "/worldgen is host-only.".to_string(),
                                );
                            } else if arg.is_empty() {
                                local_lines.push(format!(
                                    "Worldgen: {:?}",
                                    WorldGen::current()
                                ));
                            } else if let Some(g) = WorldGen::from_str(arg) {
                                WORLDGEN.store(
                                    g as u8,
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                                let to_unload: Vec<ChunkCoord> = self
                                    .world
                                    .iter_chunks()
                                    .map(|(c, _)| *c)
                                    .collect();
                                if !to_unload.is_empty() {
                                    let renderer = self
                                        .renderer
                                        .as_mut()
                                        .expect("renderer ready");
                                    for entry in self.chunk_meshes.drain(..)
                                    {
                                        renderer.remove_mesh(entry.2);
                                    }
                                    for cc in &to_unload {
                                        self.world.remove_chunk(*cc);
                                    }
                                }
                                self.pending_chunks.clear();
                                self.pending_set.clear();
                                local_lines.push(format!(
                                    "Worldgen set to {g:?}; chunks regenerating."
                                ));
                            } else {
                                local_lines.push(
                                    "Usage: /worldgen classic|flat|islands|amplified"
                                        .to_string(),
                                );
                            }
                            handled_locally = true;
                        }
                        "clearedits" | "wipe" => {
                            // Host-only: drop the entire edit
                            // history + every cached chunk. Next
                            // streaming pass regenerates from
                            // scratch under the current seed.
                            if !matches!(
                                self.net,
                                NetRole::Server { .. } | NetRole::Offline
                            ) {
                                local_lines.push(
                                    "/clearedits is host-only.".to_string(),
                                );
                            } else {
                                let n_edits = self.edit_history.len();
                                self.edit_history.clear();
                                self.torch_positions.clear();
                                let to_unload: Vec<ChunkCoord> = self
                                    .world
                                    .iter_chunks()
                                    .map(|(c, _)| *c)
                                    .collect();
                                if !to_unload.is_empty() {
                                    let renderer = self
                                        .renderer
                                        .as_mut()
                                        .expect("renderer ready");
                                    for entry in self.chunk_meshes.drain(..) {
                                        renderer.remove_mesh(entry.2);
                                    }
                                    for cc in &to_unload {
                                        self.world.remove_chunk(*cc);
                                    }
                                }
                                self.pending_chunks.clear();
                                self.pending_set.clear();
                                local_lines.push(format!(
                                    "Wiped {n_edits} edits + regenerating chunks."
                                ));
                            }
                            handled_locally = true;
                        }
                        "chunkinfo" => {
                            // Diagnostic: chunk count + size +
                            // pending queue + edit history length.
                            let cs = CHUNK_SIZE as i32;
                            let voxels_per_chunk =
                                (cs as u64).pow(3);
                            let total = self.chunk_meshes.len() as u64
                                * voxels_per_chunk;
                            local_lines.push(format!(
                                "Chunks: {}  pending: {}  edits: {}  voxels: {total}",
                                self.chunk_meshes.len(),
                                self.pending_chunks.len(),
                                self.edit_history.len(),
                            ));
                            handled_locally = true;
                        }
                        "spawn" => {
                            // Snap the host back to the origin
                            // spawn pad. Works in any role since
                            // the local player's pose is always
                            // authoritative on its own client.
                            self.player_pos = Vec3::new(0.0, 32.0, 0.0);
                            self.player_vel = Vec3::ZERO;
                            self.fall_start_y = None;
                            local_lines.push("Teleported to spawn.".to_string());
                            handled_locally = true;
                        }
                        "w" | "whisper" | "tell" | "msg" => {
                            // Private message routed via the
                            // server. Host short-circuits to a
                            // local lookup when whispering itself
                            // / a local peer.
                            let mut parts = arg.splitn(2, char::is_whitespace);
                            let target = parts.next().unwrap_or("").trim();
                            let body = parts.next().unwrap_or("").trim();
                            if target.is_empty() || body.is_empty() {
                                local_lines.push(
                                    "Usage: /w <name> <message>".to_string(),
                                );
                            } else {
                                let target_id = self
                                    .player_names
                                    .iter()
                                    .find(|(_, n)| {
                                        n.eq_ignore_ascii_case(target)
                                    })
                                    .map(|(id, _)| *id);
                                match target_id {
                                    None => {
                                        local_lines.push(format!(
                                            "No player named {target}."
                                        ));
                                    }
                                    Some(target_id) => {
                                        let cfg = bincode::config::standard();
                                        let line_in = format!(
                                            "[whisper to {target}] {body}"
                                        );
                                        let line_out = format!(
                                            "[whisper from {}] {body}",
                                            self.player_name
                                        );
                                        self.chat_log.push((
                                            std::time::Instant::now(),
                                            line_in,
                                        ));
                                        match &mut self.net {
                                            NetRole::Offline => {
                                                // No-op offline.
                                            }
                                            NetRole::Server { server, .. } => {
                                                if target_id != 0
                                                    && let Ok(buf) =
                                                        bincode::serde::encode_to_vec(
                                                            &line_out, cfg,
                                                        )
                                                {
                                                    server.send_bytes_to_client(
                                                        target_id,
                                                        "vx.chat",
                                                        buf,
                                                    );
                                                }
                                            }
                                            NetRole::Client { client, .. } => {
                                                let payload = format!(
                                                    "\u{1}whisper\u{1}{target_id}\u{1}{body}"
                                                );
                                                if let Ok(buf) =
                                                    bincode::serde::encode_to_vec(
                                                        &payload, cfg,
                                                    )
                                                {
                                                    let _ = client
                                                        .send_bytes("vx.chat", buf);
                                                }
                                            }
                                        }
                                    }
                                }
                                handled_locally = true;
                            }
                        }
                        "tp" => {
                            // /tp <name>: teleport the local
                            // player to the pose of the named
                            // peer. Works in any role - the
                            // server uses `poses`, clients use
                            // `remote_interp` since `remote`
                            // mirrors the same data either way.
                            if arg.is_empty() {
                                local_lines.push("Usage: /tp <name>".to_string());
                            } else {
                                let target_id = self
                                    .player_names
                                    .iter()
                                    .find(|(_, n)| {
                                        n.eq_ignore_ascii_case(arg)
                                    })
                                    .map(|(id, _)| *id);
                                if let Some(id) = target_id {
                                    let pose: Option<[f32; 3]> = match &self.net {
                                        NetRole::Offline => None,
                                        NetRole::Server { poses, .. } => {
                                            poses.get(&id).map(|p| p.pos)
                                        }
                                        NetRole::Client { remote, .. } => {
                                            remote.get(&id).map(|p| p.pos)
                                        }
                                    };
                                    if let Some(p) = pose {
                                        self.player_pos = Vec3::new(
                                            p[0],
                                            p[1] + 0.1,
                                            p[2],
                                        );
                                        self.player_vel = Vec3::ZERO;
                                        self.fall_start_y = None;
                                        local_lines.push(format!(
                                            "Teleported to {arg}."
                                        ));
                                    } else {
                                        local_lines.push(format!(
                                            "No pose known for {arg} yet."
                                        ));
                                    }
                                } else {
                                    local_lines
                                        .push(format!("No player named {arg}."));
                                }
                                handled_locally = true;
                            }
                        }
                        "players" | "who" => {
                            let mut names: Vec<&String> =
                                self.player_names.values().collect();
                            names.sort_by(|a, b| {
                                a.to_lowercase().cmp(&b.to_lowercase())
                            });
                            let joined: Vec<String> =
                                names.into_iter().cloned().collect();
                            let summary = if joined.is_empty() {
                                "(no peers known yet)".to_string()
                            } else {
                                joined.join(", ")
                            };
                            local_lines.push(format!(
                                "Players ({}): {summary}",
                                self.player_names.len()
                            ));
                        }
                        "clear" => {
                            self.chat_log.clear();
                            handled_locally = true;
                        }
                        "me" => {
                            if !arg.is_empty() {
                                // /me <action> -> broadcast as
                                // "* name action" so other players
                                // see an emote, not a command.
                                let action = arg.to_string();
                                match &self.net {
                                    NetRole::Offline => {
                                        local_lines.push(format!(
                                            "* {} {action}",
                                            self.player_name
                                        ));
                                    }
                                    NetRole::Server { server, .. } => {
                                        let line = format!(
                                            "* {} {action}",
                                            self.player_name
                                        );
                                        self.chat_log.push((
                                            std::time::Instant::now(),
                                            line.clone(),
                                        ));
                                        if let Ok(buf) =
                                            bincode::serde::encode_to_vec(
                                                &line,
                                                bincode::config::standard(),
                                            )
                                        {
                                            server.broadcast_bytes("vx.chat", buf);
                                        }
                                    }
                                    NetRole::Client { client, .. } => {
                                        // Smuggle the emote through
                                        // the normal chat path so
                                        // the server tags it; the
                                        // server doesn't know /me
                                        // specifically.
                                        let emote = format!("* {action}");
                                        if let Ok(buf) =
                                            bincode::serde::encode_to_vec(
                                                &emote,
                                                bincode::config::standard(),
                                            )
                                        {
                                            let _ = client
                                                .send_bytes("vx.chat", buf);
                                        }
                                    }
                                }
                                handled_locally = true;
                            } else {
                                local_lines
                                    .push("Usage: /me <action>".to_string());
                            }
                        }
                        _ => {
                            local_lines.push(format!("Unknown command: /{cmd}"));
                        }
                    }
                    if !local_lines.is_empty() {
                        let now = std::time::Instant::now();
                        for l in local_lines {
                            self.chat_log.push((now, l));
                        }
                        handled_locally = true;
                    }
                }
                if !handled_locally {
                    match &self.net {
                        NetRole::Offline => {
                            // No-op offline: chat only makes sense
                            // in MP. Drop the input silently.
                        }
                        NetRole::Server { server, .. } => {
                            // Host prepends its own name + appends
                            // to the local log, then broadcasts so
                            // every client renders the same line.
                            let line =
                                format!("<{}> {text}", self.player_name);
                            self.chat_log
                                .push((std::time::Instant::now(), line.clone()));
                            if let Ok(buf) = bincode::serde::encode_to_vec(
                                &line,
                                bincode::config::standard(),
                            ) {
                                server.broadcast_bytes("vx.chat", buf);
                            }
                        }
                        NetRole::Client { client, .. } => {
                            // Client sends the raw text; the server
                            // tags it with the sender name before
                            // broadcasting back.
                            if let Ok(buf) = bincode::serde::encode_to_vec(
                                &text,
                                bincode::config::standard(),
                            ) {
                                let _ = client.send_bytes("vx.chat", buf);
                            }
                        }
                    }
                }
            }
        }
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
            // Persist whatever the user just edited so the next
            // launch picks it up. Cheap (a few dozen bytes), so
            // doing it on Back is fine.
            self.save_config();
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
        // Load persisted settings before any user interaction so
        // the main menu badge + Settings panel show the last
        // chosen identity instead of a fresh random Player####.
        self.load_config();
        self.session_started = Some(std::time::Instant::now());
        // Pick a default world seed for solo / offline play so
        // each cold start gets a different layout. Hosts override
        // via start_hosting; persisted state will eventually load
        // here once the save format carries a seed field.
        if world_seed() == 0 {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
                .wrapping_mul(0x9E37_79B1)
                | 1;
            set_world_seed(seed);
        }
        info!(width = w, height = h, seed = world_seed(), "voxel_demo started");
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
        {
            match (state, button) {
                (ElementState::Pressed, MouseButton::Left) => {
                    self.lmb_pending = true;
                    self.lmb_held = true;
                }
                (ElementState::Released, MouseButton::Left) => {
                    self.lmb_held = false;
                    // Releasing the button cancels any active
                    // mining session.
                    self.mining = None;
                }
                (ElementState::Pressed, MouseButton::Right) => {
                    self.rmb_pending = true;
                    self.rmb_held = true;
                }
                (ElementState::Released, MouseButton::Right) => {
                    self.rmb_held = false;
                }
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
                    // Chat input takes priority - Esc just dismisses
                    // it without opening the MP menu underneath.
                    if self.chat_open {
                        self.chat_open = false;
                        self.chat_input.clear();
                    } else if mp {
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
        // F1 hides / shows the entire HUD (hotbar, crosshair,
        // health, mob bars). Useful for screenshots + benchmarks.
        let f1_now = ctx.input.is_pressed(Key::F1);
        if f1_now && !self.f1_prev && matches!(self.state, AppState::Playing) {
            self.hud_visible = !self.hud_visible;
        }
        self.f1_prev = f1_now;
        // F5 cycles the camera through first-person and two
        // third-person poses (behind, then in front). Disabled
        // while chat is open so accidental F5 mid-typing doesn't
        // disorient the player.
        let f5_now = ctx.input.is_pressed(Key::F5);
        if f5_now
            && !self.f5_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            self.camera_mode = (self.camera_mode + 1) % 3;
        }
        self.f5_prev = f5_now;
        // Q drops one item from the selected hotbar slot. No
        // dropped-entity model yet so the item simply
        // disappears - useful for trimming overstocked stacks.
        let q_now = ctx.input.is_pressed(Key::Q);
        if q_now
            && !self.q_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
            && !self.inventory_open
            && self.selected_slot < HOTBAR_SLOTS
            && let Some((b, c)) = self.inventory[self.selected_slot]
        {
            // Ctrl held = drop the entire stack at once.
            let drop_all = ctx.input.is_pressed(Key::ControlLeft)
                || ctx.input.is_pressed(Key::ControlRight);
            let new_count = if drop_all { 0 } else { c.saturating_sub(1) };
            self.inventory[self.selected_slot] = if new_count == 0 {
                None
            } else {
                Some((b, new_count))
            };
            // Small puff at the player's feet so the drop is
            // visible; mirrors the break-particle texture so
            // identifying the block stays easy.
            let color = self
                .registry
                .get(b)
                .map(|d| d.color)
                .unwrap_or([0.8, 0.8, 0.8]);
            let voxel = [
                self.player_pos.x.floor() as i32,
                (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                self.player_pos.z.floor() as i32,
            ];
            self.spawn_break_particles_tinted(voxel, color);
        }
        self.q_prev = q_now;
        // Tab held = playerlist overlay. Hidden in singleplayer
        // (the overlay drawer also guards on net role, but the
        // flag is gated here too so debug panels stay clean).
        self.tab_held =
            matches!(self.state, AppState::Playing) && ctx.input.is_pressed(Key::Tab);
        // 1..9 select the matching hotbar slot directly (Minecraft
        // convention). Slot 0 is the empty/break-hand slot; we
        // map digit 1 to slot 0 so the user-facing numbering
        // matches the visual order in the hotbar.
        if matches!(self.state, AppState::Playing) && !self.chat_open {
            let digits = [
                Key::Digit1,
                Key::Digit2,
                Key::Digit3,
                Key::Digit4,
                Key::Digit5,
                Key::Digit6,
                Key::Digit7,
                Key::Digit8,
                Key::Digit9,
            ];
            for (i, key) in digits.iter().enumerate() {
                if ctx.input.is_pressed(*key) && i < HOTBAR_SLOTS {
                    self.selected_slot = i;
                    break;
                }
            }
        }

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

        // T opens the chat input. Only fires in MP roles + while
        // no other overlay is up. Esc + Enter both close it.
        let t_now = ctx.input.is_pressed(Key::T);
        let t_just = t_now && !self.t_prev;
        self.t_prev = t_now;
        if t_just
            && matches!(self.state, AppState::Playing)
            && !matches!(self.net, NetRole::Offline)
            && !self.inventory_open
            && !self.mp_menu_open
            && !self.chat_open
        {
            self.chat_open = true;
            self.chat_input.clear();
        }

        // F = eat. Consumes one meat block (id 12) from anywhere in
        // the inventory + restores 4 HP. No-op when at full health
        // or when the player has no meat. Gated on Playing state +
        // no overlay so the F key doesn't fire while the menu eats
        // text input.
        let f_now = ctx.input.is_pressed(Key::F);
        let f_just = f_now && !self.f_prev;
        self.f_prev = f_now;
        if f_just
            && matches!(self.state, AppState::Playing)
            && !self.inventory_open
            && !self.mp_menu_open
            && self.player_health < PLAYER_MAX_HEALTH
        {
            // Find first slot holding meat (offline / host path).
            let mut ate = false;
            for slot in self.inventory.iter_mut() {
                if let Some((b, c)) = slot.as_mut()
                    && *b == BlockId(12)
                {
                    *c = c.saturating_sub(1);
                    if *c == 0 {
                        *slot = None;
                    }
                    ate = true;
                    break;
                }
            }
            if ate {
                self.player_health =
                    (self.player_health + 4.0).min(PLAYER_MAX_HEALTH);
                let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
                self.sfx_at("eat", pos, |a, g| {
                    let _ = a.play_thud(280.0, 0.18 * g);
                });
            }
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
        let combat_gate =
            lmb && !self.inventory_open && !self.mp_menu_open && !self.chat_open;
        let mob_target = if combat_gate {
            self.raycast_mob(self.player_pos, forward, MOB_ATTACK_REACH)
        } else {
            None
        };
        let pvp_target = if combat_gate {
            self.raycast_remote_player(self.player_pos, forward, PVP_REACH)
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
        // Pick whichever attack target is closest: player < mob <
        // voxel. Voxel break path is suppressed when any combat
        // target wins.
        let mut best_dist = f32::INFINITY;
        let mut attack_player: Option<u32> = None;
        let mut attack_mob: Option<u32> = None;
        if let Some((id, d)) = pvp_target
            && d < best_dist
        {
            best_dist = d;
            attack_player = Some(id);
            attack_mob = None;
        }
        if let Some((id, d)) = mob_target
            && d < best_dist
        {
            best_dist = d;
            attack_mob = Some(id);
            attack_player = None;
        }
        if let Some(d) = voxel_dist
            && d < best_dist
        {
            attack_mob = None;
            attack_player = None;
        }
        // Player-vs-player attack: dispatch to server-side damage
        // application. Host applies directly; client sends a
        // `vx.pvp_hit` request the server can validate.
        if let Some(target_id) = attack_player {
            match &self.net {
                NetRole::Client { client, .. } => {
                    let cfg = bincode::config::standard();
                    let msg = PvPHitMsg { target_id };
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        let _ = client.send_bytes("vx.pvp_hit", buf);
                    }
                }
                NetRole::Server { server, .. } => {
                    // Host is attacker id 0 -> send DamageMsg
                    // straight to the target.
                    let cfg = bincode::config::standard();
                    let dmg = DamageMsg { amount: PVP_DAMAGE };
                    if let Ok(buf) = bincode::serde::encode_to_vec(dmg, cfg) {
                        server.send_bytes_to_client(target_id, "vx.damage", buf);
                    }
                }
                NetRole::Offline => {}
            }
        }
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
        // Mining: LMB held + a voxel in view, but no mob in front
        // (a melee swing takes priority). Tick the active session
        // toward `total`; on completion, set `mine_break` so the
        // block-edit path below executes the break exactly once.
        let lmb_active = self.lmb_held
            && !self.inventory_open
            && !self.mp_menu_open
            && !self.chat_open
            && attack_mob.is_none()
            && attack_player.is_none();
        let mut mine_break: Option<[i32; 3]> = None;
        if lmb_active && let Some(hit) = self.target {
            let block = self.world.get(hit.voxel);
            if block != BlockId::AIR {
                // Restart the session if either the target voxel
                // changed or the block at the target has been
                // replaced (e.g. by another player).
                let restart = match self.mining {
                    Some(s) => s.voxel != hit.voxel || s.block != block,
                    None => true,
                };
                if restart {
                    self.mining = Some(MiningState {
                        voxel: hit.voxel,
                        progress: 0.0,
                        total: block_hardness(block).max(0.05),
                        block,
                        fx_cooldown: 0.0,
                    });
                }
                // Pull the state out by value so we can call
                // `sfx_at` (which needs `&mut self`) without the
                // borrow checker complaining about overlap with
                // `self.mining`. Write the updated state back at
                // the end of the tick.
                if let Some(mut m) = self.mining.take() {
                    m.progress += dt;
                    m.fx_cooldown -= dt;
                    let done = m.progress >= m.total;
                    let voxel_center = [
                        m.voxel[0] as f32 + 0.5,
                        m.voxel[1] as f32 + 0.5,
                        m.voxel[2] as f32 + 0.5,
                    ];
                    let fire_fx = m.fx_cooldown <= 0.0 || done;
                    if fire_fx {
                        m.fx_cooldown = 0.22;
                    }
                    let key = format!("dig:{}", block_material(block));
                    let freq = 220.0 + (block.0 as f32) * 18.0;
                    let total = m.total;
                    let progress = m.progress;
                    if fire_fx {
                        self.sfx_at(&key, voxel_center, move |a, g| {
                            let _ = a.play_thud(
                                freq,
                                0.10 * g * (0.5 + 0.5 * progress / total),
                            );
                        });
                    }
                    if done {
                        mine_break = Some(m.voxel);
                    } else {
                        self.mining = Some(m);
                    }
                }
            } else {
                self.mining = None;
            }
        } else {
            self.mining = None;
        }
        // Suppress the legacy single-click break: mining now drives
        // it via `mine_break`. Keep `lmb` consumed so a fresh
        // press doesn't bleed through somewhere else.
        let _ = lmb;
        // Block break: fires once when `mine_break` is set (i.e.
        // the hold-to-mine session completed). The path applies
        // the world edit, broadcasts, credits the pickup, and
        // plays the break SFX.
        if !self.inventory_open
            && !self.mp_menu_open
            && let Some(voxel) = mine_break
        {
            // Capture the block we're about to break so we can
            // pop it into the inventory (greedy collection - air
            // just no-ops).
            let broken = self.world.get(voxel);
            // Tint the debris with the broken block's albedo so a
            // stone break sprays grey chips, wood sprays brown,
            // glass sprays pale blue, etc.
            let break_color = self
                .registry
                .get(broken)
                .map(|d| d.color)
                .unwrap_or([0.75, 0.55, 0.30]);
            let edit = EditMsg {
                x: voxel[0],
                y: voxel[1],
                z: voxel[2],
                block: 0,
            };
            if is_client {
                self.world_set_tracked(voxel, BlockId::AIR);
                let coords = Self::affected_chunks(voxel);
                self.rebuild_chunks(&coords);
                self.spawn_break_particles_tinted(voxel, break_color);
                if let NetRole::Client { pending_edits, .. } = &mut self.net {
                    pending_edits.push(edit);
                }
            } else {
                self.world_set_tracked(voxel, BlockId::AIR);
                let coords = Self::affected_chunks(voxel);
                self.rebuild_chunks(&coords);
                self.spawn_break_particles_tinted(voxel, break_color);
                if let NetRole::Server { server, .. } = &self.net {
                    let cfg = bincode::config::standard();
                    if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
                self.edit_history.push(edit);
            }
            if !is_client && broken != BlockId::AIR {
                self.pickup(broken);
            }
            // Final break SFX (the loop already plays intermediate
            // dig sounds while the player holds LMB).
            let voxel_center = [
                voxel[0] as f32 + 0.5,
                voxel[1] as f32 + 0.5,
                voxel[2] as f32 + 0.5,
            ];
            let key = format!("dig:{}", block_material(broken));
            let freq = 220.0 + (broken.0 as f32) * 28.0;
            self.sfx_at(&key, voxel_center, move |a, g| {
                let _ = a.play_thud(freq, 0.30 * g);
            });
        }
        // Block place: RMB single click stays instant; no mining.
        if !self.inventory_open
            && !self.mp_menu_open
            && !self.chat_open
            && attack_mob.is_none()
            && let Some(hit) = self.target
        {
            if false {
                // Sentinel arm so the existing `else if rmb ...`
                // structure stays a single `if/else if` chain.
                unreachable!()
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
        // never out of sync with what a client sees. /freezetime
        // pauses the advance.
        if !self.freeze_time {
            self.day_time = (self.day_time + dt).rem_euclid(DAY_LENGTH_S);
        }

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
            // Head-bob offset: a small vertical bob + lateral
            // sway scaled by current horizontal speed so a still
            // player has a rock-steady camera. Skip while in fly
            // mode - hovering shouldn't shake the view.
            let h_speed = (self.player_vel.x * self.player_vel.x
                + self.player_vel.z * self.player_vel.z)
                .sqrt();
            let bob_amount =
                if !self.settings.head_bob || self.fly_mode || !self.grounded {
                    0.0
                } else {
                    (h_speed / PLAYER_SPEED).clamp(0.0, 1.5)
                };
            let bob_y = (self.head_bob_phase * 2.0).sin() * 0.05 * bob_amount;
            let bob_x = self.head_bob_phase.sin() * 0.03 * bob_amount;
            let right = Vec3::new(
                -(-self.player_yaw.sin()),
                0.0,
                -self.player_yaw.cos(),
            )
            .normalize_or_zero();
            let bob = Vec3::new(right.x * bob_x, bob_y, right.z * bob_x);
            // Third-person modes pull the camera back along the
            // view ray (mode 1) or push it in front of the player
            // looking backward (mode 2). The offset is short
            // enough that it usually stays inside a 1-chunk view
            // bubble even on tight walls.
            const TP_DIST: f32 = 4.5;
            match self.camera_mode {
                0 => {
                    cam.position = player_pos + bob;
                    cam.target = player_pos + bob + forward;
                }
                1 => {
                    cam.position = player_pos + bob - forward * TP_DIST;
                    cam.target = player_pos + bob;
                }
                _ => {
                    cam.position = player_pos + bob + forward * TP_DIST;
                    cam.target = player_pos + bob;
                }
            }
            cam.fov_y_rad = self.fov_y_rad;
        }
        // Day/night cycle. `day_time` ramps from 0..DAY_LENGTH_S
        // and feeds both the sun direction (rotating around the +Z
        // axis) and the clear color (sunrise -> noon -> sunset ->
        // night). The sky color is a perceptual mix between three
        // anchor swatches keyed on the sun's elevation.
        let (sun_dir, mut sky) = day_night(self.day_time);
        // Precipitation re-tints the sky. Rain: dark stormy
        // grey. Snow: lighter, slightly blue-tinged overcast.
        if self.weather == 1 {
            let target = [0.42, 0.46, 0.52];
            let t = 0.6;
            sky[0] = sky[0] * (1.0 - t) + target[0] * t;
            sky[1] = sky[1] * (1.0 - t) + target[1] * t;
            sky[2] = sky[2] * (1.0 - t) + target[2] * t;
        } else if self.weather == 2 {
            let target = [0.70, 0.74, 0.80];
            let t = 0.55;
            sky[0] = sky[0] * (1.0 - t) + target[0] * t;
            sky[1] = sky[1] * (1.0 - t) + target[1] * t;
            sky[2] = sky[2] * (1.0 - t) + target[2] * t;
        }
        // Active lightning flash washes the sky toward white.
        if self.lightning_flash > 0.0 {
            let t = (self.lightning_flash / 0.18).clamp(0.0, 1.0);
            let target = [1.0, 1.0, 1.0];
            sky[0] = sky[0] * (1.0 - t) + target[0] * t;
            sky[1] = sky[1] * (1.0 - t) + target[1] * t;
            sky[2] = sky[2] * (1.0 - t) + target[2] * t;
        }
        renderer.set_sun_dir(sun_dir);
        renderer.set_clear_color(sky[0] as f64, sky[1] as f64, sky[2] as f64, 1.0);
        // Tie sun intensity to the real (un-clamped) elevation so
        // night is actually dim. We recompute the raw direction
        // here because `day_night` clamps the dir vector when the
        // sun is below the horizon (to keep shadows sane), losing
        // the sign we need for the intensity curve.
        let phase = (self.day_time / DAY_LENGTH_S).rem_euclid(1.0);
        let theta = phase * std::f32::consts::TAU;
        let raw_elev = theta.sin();
        // 3.0 at noon, ~0.4 at midnight (soft moonlight), smooth
        // transition through the horizon. Clamp avoids negatives.
        let sun_intensity = (1.7 + raw_elev * 1.3).clamp(0.4, 3.0);
        renderer.set_sun_intensity(sun_intensity);
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
        // visible. The interpolation buffer (`remote_interp`) is
        // populated by every received `vx.player` regardless of
        // role, so a single sample call here covers Server + Client
        // paths uniformly. We skip our own id (0 for the host, the
        // assigned id for a client) to avoid rendering ourselves.
        let now = std::time::Instant::now();
        let own_id: u32 = match &self.net {
            NetRole::Offline => u32::MAX,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let remote_iter: Vec<PlayerMsg> = if matches!(self.net, NetRole::Offline) {
            Vec::new()
        } else {
            self.remote_interp
                .iter()
                .filter(|(id, _)| **id != own_id)
                .map(|(_, r)| r.sample(now))
                .collect()
        };
        // In third-person we draw the local player too so the
        // pulled-back camera has something to look at. We
        // synthesize a PlayerMsg from current state and prepend
        // it to the render list.
        let mut local_iter = remote_iter;
        if self.camera_mode != 0 {
            local_iter.push(PlayerMsg {
                id: own_id,
                pos: [player_pos.x, player_pos.y, player_pos.z],
                yaw: self.player_yaw,
                pitch: self.player_pitch,
            });
        }
        if !local_iter.is_empty() {
            let cube = self.player_cube.get_or_insert_with(|| renderer.cube_handle());
            for p in local_iter {
                // Hash id to a stable colour + a slightly cooler
                // head tone so the two parts read as separate body
                // segments instead of a single block.
                let h = (p.id.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                let tau = std::f32::consts::TAU;
                let body_color = [
                    (0.4 + 0.5 * (h * tau).sin().abs()).min(1.0),
                    (0.4 + 0.5 * ((h + 0.33) * tau).sin().abs()).min(1.0),
                    (0.4 + 0.5 * ((h + 0.66) * tau).sin().abs()).min(1.0),
                    1.0,
                ];
                let head_color = [
                    (body_color[0] * 0.85).min(1.0),
                    (body_color[1] * 0.85).min(1.0),
                    (body_color[2] * 0.85).min(1.0),
                    1.0,
                ];
                let rot = glam::Quat::from_rotation_y(p.yaw);
                let body = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.65, 1.0, 0.4),
                    rot,
                    Vec3::new(p.pos[0], p.pos[1] - 1.2, p.pos[2]),
                );
                renderer.draw_mesh_pbr(*cube, body.to_cols_array(), body_color, 0.0, 0.7);
                let head = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.55, 0.55, 0.55),
                    rot,
                    Vec3::new(p.pos[0], p.pos[1] - 0.45, p.pos[2]),
                );
                renderer.draw_mesh_pbr(*cube, head.to_cols_array(), head_color, 0.0, 0.7);
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
                // Death-fade progress in `[0, 1]`. Zero = alive.
                let dying_t = self
                    .mob_dying
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0);
                let death_progress = if dying_t > 0.0 {
                    ((MOB_DEATH_FADE - dying_t) / MOB_DEATH_FADE).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let body_brightness = 1.0 - 0.55 * death_progress;
                let tilt = glam::Quat::from_rotation_z(
                    -std::f32::consts::FRAC_PI_2 * death_progress,
                );
                let death_sink = 0.18 * death_progress;
                // Per-kind body proportions + meshes + base colour.
                // Sheep uses the textured sheep_body + sheep_head
                // meshes; zombies use the plain player_cube with a
                // dim green tint stretched into a humanoid box.
                let is_zombie = mob.kind == KIND_ZOMBIE;
                let (body_scale, body_y_off, base_body_tint, body_use_mesh) = if is_zombie {
                    (
                        Vec3::new(0.65, 1.55, 0.45),
                        0.78,
                        [0.30, 0.55, 0.30, 1.0],
                        player_cube_handle,
                    )
                } else {
                    (
                        Vec3::new(1.2, 0.8, 0.7),
                        0.4,
                        body_tint,
                        body_mesh,
                    )
                };
                let tint = [
                    (base_body_tint[0] * (1.0 - hurt_flash) + 1.0 * hurt_flash)
                        * body_brightness,
                    (base_body_tint[1] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                        * body_brightness,
                    (base_body_tint[2] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                        * body_brightness,
                    base_body_tint[3],
                ];
                let body_rot = glam::Quat::from_rotation_y(mob.yaw) * tilt;
                // Lunge offset: a brief forward shove on the
                // attack frame. The timer starts at MOB_LUNGE_TIME
                // + decays to 0; a sin curve gives a smooth
                // out-and-back motion within that window.
                let lunge_t = self
                    .mob_lunge
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, MOB_LUNGE_TIME);
                let lunge_phase =
                    (1.0 - lunge_t / MOB_LUNGE_TIME).clamp(0.0, 1.0);
                let lunge_amp = 0.35
                    * (lunge_phase * std::f32::consts::PI).sin().max(0.0);
                let lunge_fwd =
                    Vec3::new(mob.yaw.cos(), 0.0, -mob.yaw.sin()) * lunge_amp;
                let body = Mat4::from_scale_rotation_translation(
                    body_scale,
                    body_rot,
                    Vec3::new(
                        mob.pos[0] + lunge_fwd.x,
                        mob.pos[1] + body_y_off - death_sink,
                        mob.pos[2] + lunge_fwd.z,
                    ),
                );
                renderer.draw_mesh_pbr(
                    body_use_mesh,
                    body.to_cols_array(),
                    tint,
                    0.0,
                    0.9,
                );
                let fwd = Vec3::new(mob.yaw.cos(), 0.0, -mob.yaw.sin());
                let (head_scale, head_off_y, head_off_fwd, head_mesh_use, base_head_color) =
                    if is_zombie {
                        (
                            Vec3::new(0.55, 0.55, 0.55),
                            1.85,
                            0.0,
                            player_cube_handle,
                            [0.22, 0.42, 0.22, 1.0],
                        )
                    } else {
                        (
                            Vec3::new(0.60, 0.45, 0.45),
                            0.8,
                            0.45,
                            head_mesh,
                            head_color,
                        )
                    };
                let head_pos = Vec3::new(
                    mob.pos[0] + lunge_fwd.x,
                    mob.pos[1] + head_off_y - death_sink,
                    mob.pos[2] + lunge_fwd.z,
                ) + fwd * head_off_fwd;
                let head_rot = glam::Quat::from_rotation_y(mob.yaw) * tilt;
                let head = Mat4::from_scale_rotation_translation(
                    head_scale,
                    head_rot,
                    head_pos,
                );
                let head_tint = [
                    (base_head_color[0] * (1.0 - hurt_flash) + 1.0 * hurt_flash)
                        * body_brightness,
                    (base_head_color[1] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                        * body_brightness,
                    (base_head_color[2] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                        * body_brightness,
                    base_head_color[3],
                ];
                renderer.draw_mesh_pbr(
                    head_mesh_use,
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
        // Mining-progress overlay: an extra wire box tinted from
        // bright yellow (just-started) through orange to red
        // (almost broken). Drawn slightly larger than the targeted
        // voxel so it doesn't z-fight with the regular outline.
        if let Some(m) = self.mining {
            let t = (m.progress / m.total).clamp(0.0, 1.0);
            let r = 1.0;
            let g = 1.0 - t * 0.8;
            let b = 0.1 * (1.0 - t);
            let min = [
                m.voxel[0] as f32 - 0.02,
                m.voxel[1] as f32 - 0.02,
                m.voxel[2] as f32 - 0.02,
            ];
            let max = [
                m.voxel[0] as f32 + 1.02,
                m.voxel[1] as f32 + 1.02,
                m.voxel[2] as f32 + 1.02,
            ];
            renderer.debug_box_3d(min, max, [r, g, b, 1.0]);
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

/// Global world-generation seed. Mixed into every noise function so
/// distinct hosts (or /seed-cycled sessions) produce different
/// terrain + ore distributions while staying deterministic across
/// workers and clients running the same value. Set on host start
/// + replicated to clients via `vx.seed`.
static WORLD_SEED: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

fn world_seed() -> u32 {
    WORLD_SEED.load(std::sync::atomic::Ordering::Relaxed)
}

fn set_world_seed(value: u32) {
    WORLD_SEED.store(value, std::sync::atomic::Ordering::Relaxed);
}

fn seed_offset() -> f32 {
    // Mix the seed into a deterministic float used as a phase
    // offset for the sin/cos noise stack. Picking a small range
    // keeps the curves visually similar across seeds while still
    // producing distinct terrain.
    (world_seed().wrapping_mul(0x9E37_79B1) as f32 / u32::MAX as f32) * 1000.0
}

fn biome_at(wx: f32, wz: f32) -> Biome {
    // Two layered sin/cos waves keep the biome map smooth and
    // deterministic. Range maps roughly to `[-1, 1]` so the cutoffs
    // are easy to reason about. A seed offset shifts the wave so
    // distinct hosts get different biome maps.
    let s = seed_offset();
    let n = ((wx + s) * 0.012).sin() * 0.6
        + ((wz + s) * 0.009).cos() * 0.5
        + (((wx + wz) + s) * 0.004).sin() * 0.3;
    if n < -0.5 {
        Biome::Desert
    } else if n > 0.5 {
        Biome::Mountain
    } else {
        Biome::Plains
    }
}

/// Cheap, deterministic 3D hash used by the ore generator. Mixes
/// integer voxel coordinates + a per-ore-kind salt into a [0, 1]
/// float. Two octaves of bit-rotation give a noticeably "veiny"
/// distribution rather than salt-and-pepper.
fn ore_hash(x: i32, y: i32, z: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (y as u32).wrapping_mul(0x9E37_79B1);
    h ^= (z as u32).wrapping_mul(0x1656_67B1);
    h ^= salt ^ world_seed();
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    // Blob softening: average with a neighbour cell so the result
    // forms small pockets instead of isolated voxels.
    let mut g = (x as u32 ^ y as u32 ^ z as u32).wrapping_mul(salt | 1);
    g ^= g >> 17;
    g = g.wrapping_mul(0xC2B2_AE35);
    let a = h as f32 / u32::MAX as f32;
    let b = g as f32 / u32::MAX as f32;
    (a * 0.7 + b * 0.3).clamp(0.0, 1.0)
}

/// Cheap, deterministic 2D hash. Used for tree placement so two
/// peers running the same generator drop trees in the same spots.
/// Returns true if a chat line's sender tag matches any entry
/// in the muted set. Recognises both the standard `<sender>`
/// prefix used by regular chat and the `* sender` prefix used
/// by /me-style emotes.
fn line_is_muted(line: &str, muted: &HashSet<String>) -> bool {
    if let Some(rest) = line.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        let sender = &rest[..end];
        if muted.contains(&sender.to_lowercase()) {
            return true;
        }
    }
    if let Some(rest) = line.strip_prefix("* ")
        && let Some(end) = rest.find(' ')
    {
        let sender = &rest[..end];
        if muted.contains(&sender.to_lowercase()) {
            return true;
        }
    }
    false
}

fn hash2(wx: i32, wz: i32) -> u32 {
    let mut h = (wx as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (wz as u32).wrapping_mul(0x1656_67B1);
    h ^= world_seed();
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
    BlockType {
        id: 13, name: "coal", color: [0.30, 0.30, 0.30], transparent: false,
        top: TileSpec { col: 2, row: 3, files: &["coal_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 2, row: 3, files: &["coal_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 2, row: 3, files: &["coal_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 14, name: "iron", color: [0.75, 0.65, 0.55], transparent: false,
        top: TileSpec { col: 3, row: 3, files: &["iron_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 3, files: &["iron_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 3, row: 3, files: &["iron_ore.png"], keep_alpha: false },
    },
    BlockType {
        // Snow cap. Caps mountain biome heights above the snow
        // line in gen_classic; otherwise behaves like a stone-y
        // surface block. Falls back to a near-white solid colour
        // when no snow.png ships with the assets pack.
        id: 15, name: "snow", color: [0.95, 0.97, 1.00], transparent: false,
        top: TileSpec { col: 0, row: 0, files: &["snow.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["snow.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["snow.png"], keep_alpha: false },
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
        1 | 8 | 13 | 14 => "stone", // stone, cobble, coal ore, iron ore
        2 => "grass",               // dirt - grass-ish sound is fine
        3 => "grass",
        4 => "sand",
        5 | 7 | 10 => "wood",       // wood, planks, torch
        6 => "grass",               // leaves
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
    // Cheap procedural cave check: 3D noise made of sin/cos
    // octaves. Returns true when (wx, wy, wz) sits inside a void
    // pocket. Tuned so caves only carve into stone (under the
    // grass / dirt cap) + leave the surface intact, and so two
    // octaves combine into a "blobby tunnel" shape rather than
    // single-block holes.
    let cave_at = |wx: f32, wy: f32, wz: f32| -> bool {
        if wy < 1.0 {
            return false; // keep a bedrock floor at y=0
        }
        let a = (wx * 0.09).sin() * 0.5
            + (wz * 0.08).cos() * 0.5
            + (wy * 0.12).sin() * 0.5;
        let b = (wx * 0.20 + 17.0).sin() * (wy * 0.16 + 9.0).cos()
            + (wz * 0.21 + 31.0).sin() * 0.6;
        let n = a * 0.55 + b * 0.45;
        n > 0.78
    };
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let biome = biome_at(wx, wz);
        let h = height_at(wx, wz, biome);
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            return BlockId::AIR;
        }
        // Cave carve: only below the 3-block dirt / grass cap so
        // we don't punch holes through the surface.
        if wy < h - 3 && cave_at(wx, wy as f32, wz) {
            return BlockId::AIR;
        }
        // Surface block per biome. Mountains above the snow line
        // swap to snow (id 15) for visual identity.
        let surface = match biome {
            Biome::Desert => BlockId(4),
            Biome::Plains => BlockId(3),
            Biome::Mountain => {
                if h >= 26 {
                    BlockId(15)
                } else {
                    BlockId(1)
                }
            }
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
            // Stone interior: roll for ore veins. Two deterministic
            // 3D hashes seed coal + iron pockets; each is a noise
            // value that exceeds a threshold rarely. Coal is more
            // common + can live high; iron is rarer + only deep.
            // Mountain biome boosts both ore probabilities (lower
            // thresholds) and raises the iron Y ceiling so peaks
            // become genuinely worth digging into.
            let ox = wx as i32;
            let oy = wy;
            let oz = wz as i32;
            let h_coal = ore_hash(ox, oy, oz, 0x9E37_79B9);
            let h_iron = ore_hash(ox, oy, oz, 0x85EB_CA6B);
            let (iron_thr, iron_max_y, coal_thr) = match biome {
                Biome::Mountain => (0.965, 24, 0.90),
                _ => (0.985, 14, 0.94),
            };
            if h_iron > iron_thr && oy < iron_max_y {
                BlockId(14)
            } else if h_coal > coal_thr {
                BlockId(13)
            } else {
                BlockId(1)
            }
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
