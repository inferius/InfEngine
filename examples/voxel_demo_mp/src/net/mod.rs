//! Wire-protocol definitions for the multiplayer voxel demo.
//!
//! Every type that flies over the `infengine_net` socket - block
//! edits, player poses, mob snapshots, damage notifications,
//! inventory snapshots / actions, drop spawns - lives here, plus
//! the bincode-serialised save format (`ServerStateV2`,
//! `PersistedInv`) and the runtime networking role enum
//! (`NetRole`).
//!
//! Wire format: `infengine_net::NetMessage::Bytes { kind, data }`.
//! Each `*Msg` here is the `data` payload for one `kind` string;
//! the kind names ("vx.edit", "vx.player", "vx.mob", ...) are
//! inline string literals where they're encoded - see
//! `Voxel::tick_net` and friends in `main.rs`.
//!
//! The save format (`ServerStateV2` + `PersistedInv`) shares the
//! same serde + bincode dependency tree, so it ships with the
//! wire types until Phase A4 carves out a dedicated `world_save/`
//! module. The save plumbing itself (`save_server_state`,
//! `load_server_state`) stays in `main.rs` for now.

use std::collections::{HashMap, HashSet};

use infengine_net::{NetClient, NetServer};
use infengine_voxel::BlockId;
use serde::{Deserialize, Serialize};

use crate::block::BUCKET_WATER_ID;
use crate::inventory::inv_pickup;
use crate::{HOTBAR_SLOTS, INVENTORY_SLOTS, PLAYER_MAX_HEALTH, SplitMode};

/// One block-edit travelling over the wire. Sent as
/// `kind = "vx.edit"`. Server applies + broadcasts to every client
/// including the originator; clients apply on receive (don't predict
/// locally for v1).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct EditMsg {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
    pub(crate) block: u16,
}

/// Player pose snapshot. Sent every tick:
/// - client -> server as a single-player update (`id` ignored).
/// - server -> all clients as a per-player broadcast (with the
///   originator's `id`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct PlayerMsg {
    pub(crate) id: u32,
    pub(crate) pos: [f32; 3],
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
}

/// Server -> all clients: a player has left. Recipients drop the
/// id from their `remote` table so the leaver's cube + pose stop
/// rendering immediately instead of waiting for an eventual
/// timeout.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct PlayerRemoveMsg {
    pub(crate) id: u32,
}

// `vx.leave` is sent as an empty-payload `client.send_bytes`
// (no struct needed - presence of the kind string is the signal).

/// Full per-client inventory snapshot. Server -> client only. The
/// server is authoritative; every break / place / drag-and-drop /
/// craft action on the server resolves to a fresh snapshot, sent
/// back to the originating client via `vx.inv`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InvSnapshotMsg {
    /// Main 36-slot inventory grid (Vec to keep bincode varlen-
    /// friendly; we always send exactly INVENTORY_SLOTS entries).
    pub(crate) inventory: Vec<Option<(u16, u8)>>,
    /// 2x2 crafting grid (always 4 entries).
    pub(crate) craft: Vec<Option<(u16, u8)>>,
}

/// Encoded `SlotKind`. Bincode-friendly + version-stable so we
/// don't accidentally break the wire format by renaming a variant.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum NetSlot {
    Inv(u32),
    Craft(u32),
    CraftOut,
}

/// Snapshot of one server-side mob. Sent as `vx.mob`; clients
/// render the latest reported pose. `kind` reserves a discriminator
/// for future mob types - 0 = sheep for now.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct MobMsg {
    pub(crate) id: u32,
    pub(crate) kind: u8,
    pub(crate) pos: [f32; 3],
    pub(crate) yaw: f32,
    /// Current HP rounded to the nearest integer. Drives the
    /// floating health bar render on every peer. `serde(default)`
    /// keeps the wire format back-compatible with older v1
    /// snapshots that didn't include health.
    #[serde(default)]
    pub(crate) health: u8,
}

/// Server -> all clients: a damage / death event for one mob.
/// Lets every client play the same hurt flash + sound regardless
/// of whether they were the attacker.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct MobEventMsg {
    pub(crate) id: u32,
    /// 0 = hurt, 1 = death.
    pub(crate) kind: u8,
}

/// Client -> server: request to damage a mob. Server validates
/// reach against the sender's last reported pose, applies damage,
/// broadcasts the resulting `MobEventMsg` + an inventory snapshot
/// to the attacker when the mob dies + drops loot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct MobHitMsg {
    pub(crate) mob_id: u32,
}

/// Client -> server: request to damage another player. The server
/// validates the attacker's reach + forwards `DamageMsg` to the
/// target client.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct PvPHitMsg {
    pub(crate) target_id: u32,
}

/// Server -> client: "you took N HP of damage". Sent to a single
/// target via `send_bytes_to_client`; the client applies it to
/// their local `player_health` + flashes / plays the hurt SFX.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct DamageMsg {
    pub(crate) amount: f32,
}

/// Server -> client: "snap your local position here". Used by
/// the host's /summon command to drag a remote peer to the
/// host's location. The receiving client zeroes its velocity to
/// avoid synthesizing fall damage from the jump.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct TeleportMsg {
    pub(crate) pos: [f32; 3],
}

/// Client -> server inventory intent: a drag-and-drop completion.
/// Server validates + applies + sends back an `InvSnapshotMsg`.
/// `split` mirrors the local Drag action and tells the server how
/// many items to move (all / half / one).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct InvActionMsg {
    pub(crate) from: NetSlot,
    pub(crate) to: NetSlot,
    #[serde(default)]
    pub(crate) split: SplitMode,
}

/// W5.3a: client -> host. "I just opened the chest at `pos`."
/// Host registers the sender as the chest's viewer + ships an
/// immediate `ChestStateMsg` so the client paints real contents
/// instead of an empty grid. Single-viewer for Wave 5; the next
/// client that opens the same chest displaces the previous viewer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct ChestOpenMsg {
    pub(crate) pos: [i32; 3],
}

/// W5.3a: host -> client (snapshot). Full chest contents at `pos`.
/// Sent on open + on every mutation while the client is registered
/// as the viewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChestStateMsg {
    pub(crate) pos: [i32; 3],
    pub(crate) slots: Vec<Option<(u16, u8)>>,
    /// Cursor-held stack on the host's behalf for this viewer.
    /// `None` means the client's cursor should be empty. Mirrors
    /// the host's `chest_held` field; on every host-side mutation
    /// the snapshot bundles the new cursor state so the client
    /// re-renders without a separate round-trip.
    pub(crate) cursor: Option<(u16, u8)>,
}

/// W5.3a: client -> host. Replay one GridAction against the
/// chest at `pos`. Host validates + applies + sends back a
/// `ChestStateMsg` with the new slot contents.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct ChestActMsg {
    pub(crate) pos: [i32; 3],
    /// `true` = the player-side action (player inventory grid
    /// pulls from / pushes to the chest); `false` = the chest-side
    /// action (chest grid).
    pub(crate) player_side: bool,
    pub(crate) action: ChestAct,
}

/// W5.3a: serde-friendly mirror of `container::ui::GridAction`.
/// Stays in net.rs so the GridAction enum can keep its internal
/// `pub(crate)` visibility without leaking serde into the container
/// module.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum ChestAct {
    Take(u32),
    Place(u32),
    TakeHalf(u32),
    PlaceOne(u32),
    Transfer(u32),
}

/// W5.3a: client -> host. "I just closed the chest." Host clears
/// the viewer registration; subsequent mutations no longer fan
/// out a `ChestStateMsg` to this client.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct ChestCloseMsg {
    pub(crate) pos: [i32; 3],
}

/// W5.3b: same shape as `ChestOpenMsg` but for furnaces.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct FurnaceOpenMsg {
    pub(crate) pos: [i32; 3],
}

/// W5.3b: host -> client. Furnace contents + the runtime triple
/// (fuel + cook progress) so the client renders live bars.
/// Throttled to ~5 Hz while open on the host side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FurnaceStateMsg {
    pub(crate) pos: [i32; 3],
    pub(crate) slots: Vec<Option<(u16, u8)>>,
    pub(crate) fuel_left_ms: u32,
    pub(crate) fuel_full_ms: u32,
    pub(crate) cook_progress_ms: u32,
    pub(crate) cursor: Option<(u16, u8)>,
}

/// W5.3b: client -> host. Mirror of `ChestActMsg` for furnaces.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct FurnaceActMsg {
    pub(crate) pos: [i32; 3],
    pub(crate) player_side: bool,
    pub(crate) action: ChestAct,
}

/// W5.3b: client -> host. Mirror of `ChestCloseMsg` for furnaces.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct FurnaceCloseMsg {
    pub(crate) pos: [i32; 3],
}

impl ChestAct {
    /// Map this wire-level enum to the internal `container::ui::
    /// GridAction`. Inverse of `from_grid_action`.
    pub(crate) fn to_grid_action(self) -> crate::container::ui::GridAction {
        use crate::container::ui::GridAction as G;
        match self {
            ChestAct::Take(i) => G::Take(i as usize),
            ChestAct::Place(i) => G::Place(i as usize),
            ChestAct::TakeHalf(i) => G::TakeHalf(i as usize),
            ChestAct::PlaceOne(i) => G::PlaceOne(i as usize),
            ChestAct::Transfer(i) => G::Transfer(i as usize),
        }
    }

    /// Map an internal `GridAction` into the wire variant.
    pub(crate) fn from_grid_action(action: crate::container::ui::GridAction) -> Self {
        use crate::container::ui::GridAction as G;
        match action {
            G::Take(i) => ChestAct::Take(i as u32),
            G::Place(i) => ChestAct::Place(i as u32),
            G::TakeHalf(i) => ChestAct::TakeHalf(i as u32),
            G::PlaceOne(i) => ChestAct::PlaceOne(i as u32),
            G::Transfer(i) => ChestAct::Transfer(i as u32),
        }
    }
}

/// Net wire message for a fresh drop the host wants every peer
/// to render. Sent on spawn; clients append to their local
/// `dropped_items` mirror.
#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DropSpawnMsg {
    pub(crate) id: u32,
    pub(crate) block: u16,
    pub(crate) pos: [f32; 3],
}

/// Versioned wrapper around the server's persisted state. Stored
/// at `voxel_server_state.bin` prefixed with the `INFV` magic so
/// we can evolve the format without breaking older saves.
///
/// Pre-B.7a single-file format. The runtime now writes the modular
/// multi-world layout (`world_save::save_world`); this type stays in
/// the tree so a future migration arrow can decode a legacy file out
/// of `voxel_server_state.bin.migrated` and unfold it into the new
/// shape. Marked `dead_code` to suppress the unused-import noise.
#[allow(dead_code)]
pub(crate) const SERVER_STATE_VERSION: u32 = 2;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ServerStateV2 {
    pub(crate) version: u32,
    pub(crate) edits: Vec<EditMsg>,
    /// `(client_name, inventory)` tuples. Keyed by name so a
    /// reconnect re-attaches the right state even though UDP
    /// client ids reset every run.
    pub(crate) inventories: Vec<(String, PersistedInv)>,
    /// Current day-of-cycle clock (seconds, wraps at
    /// `DAY_LENGTH_S`). Persisted so a host restart doesn't reset
    /// the world to mid-morning.
    pub(crate) day_time: f32,
    /// Host player health (0..=PLAYER_MAX_HEALTH). `serde(default)`
    /// fills with `PLAYER_MAX_HEALTH` when loading older saves
    /// that didn't include HP.
    #[serde(default = "default_player_health")]
    pub(crate) player_health: f32,
    /// Host kill counter persisted across sessions.
    #[serde(default)]
    pub(crate) kill_count: u32,
    /// World seed used by the noise generators. Older saves
    /// without this field load with seed 0, which the loader
    /// upgrades to a fresh random value so terrain doesn't snap
    /// to the deterministic seed-0 layout.
    #[serde(default)]
    pub(crate) world_seed: u32,
    /// Snow voxels with layer counts (1..=8). Saves the partial
    /// stack so reloading a paused blizzard keeps the drift.
    #[serde(default)]
    pub(crate) snow_layers: Vec<([i32; 3], u8)>,
    /// Sticky torch orientations keyed by voxel. Same encoding
    /// as the in-memory `torch_orient` map.
    #[serde(default)]
    pub(crate) torch_orient: Vec<([i32; 3], u8)>,
    /// Persisted water spread depth per wet voxel. 0 = source,
    /// 1..=7 = flow (MC's level range). Missing entries default to
    /// 0 on load - acceptable for old saves but means the
    /// pre-spread budget is forfeit (water won't widen further
    /// after a reload of a v0 save).
    #[serde(default)]
    pub(crate) liquid_level: Vec<([i32; 3], u8)>,
}

#[allow(dead_code)]
fn default_player_health() -> f32 {
    PLAYER_MAX_HEALTH
}

/// On-disk per-client inventory snapshot. `(block_id, count)`
/// tuples mirror the in-memory `[Option<(BlockId, u8)>; N]`
/// layout but use `u16` for the block so the format doesn't break
/// if `BlockId` ever grows fields.
///
/// Like `ServerStateV2`, kept dead-but-reachable for a future legacy
/// migration arrow. The B.7a path writes `world_save::players::
/// PlayerSave` instead.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedInv {
    pub(crate) inventory: Vec<Option<(u16, u8)>>,
    pub(crate) craft: Vec<Option<(u16, u8)>>,
}

#[allow(dead_code)]
impl PersistedInv {
    pub(crate) fn from_client_inv(inv: &ClientInv) -> Self {
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
    pub(crate) fn into_client_inv(self) -> ClientInv {
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
pub(crate) enum NetRole {
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

impl NetRole {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            NetRole::Offline => "offline",
            NetRole::Server { .. } => "server",
            NetRole::Client { .. } => "client",
        }
    }
}

/// Server-side per-client inventory state.
#[derive(Debug, Clone)]
pub(crate) struct ClientInv {
    pub(crate) inventory: [Option<(BlockId, u8)>; INVENTORY_SLOTS],
    pub(crate) craft_grid: [Option<(BlockId, u8)>; 4],
}

impl ClientInv {
    /// Fresh inventory matching the host's curated starting
    /// bundle: 4 wood tools + small block stacks so the player
    /// can immediately mine across every category.
    pub(crate) fn starter() -> Self {
        let mut inv: [Option<(BlockId, u8)>; INVENTORY_SLOTS] =
            [None; INVENTORY_SLOTS];
        let seed: &[(u16, u8)] = &[
            (16, 1),
            (23, 1),
            (29, 1),
            (32, 1),
            (2, 16),
            (7, 16),
            (8, 16),
            (BUCKET_WATER_ID, 1),
        ];
        // Seed straight from slot 0 - leaving slot 0 empty made
        // `/give` fill it silently while the player assumed slot
        // 0 was a dedicated "empty hand". With the seed shifted
        // forward, any extra `/give` lands at slot 8+ and the
        // player can see the new item appear at the end of the
        // hotbar.
        for (i, (bid, count)) in seed.iter().enumerate() {
            if i < HOTBAR_SLOTS {
                inv[i] = Some((BlockId(*bid), *count));
            }
        }
        Self {
            inventory: inv,
            craft_grid: [None; 4],
        }
    }

    /// Encode as a wire snapshot for the `vx.inv` message.
    pub(crate) fn to_snapshot(&self) -> InvSnapshotMsg {
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

/// Aggregate of mutable refs to every per-tick collector that
/// `process_client_inbox` populates while draining the socket. All
/// fields are `&mut` borrows of locals declared in `tick_net`;
/// after the inbox drain returns, `tick_net` applies these to the
/// authoritative game state once the `&mut self.net` borrow has
/// released.
///
/// Lives in `net` because the wire-decode mapping (topic kind
/// string -> Msg type -> collector field) is part of the protocol
/// contract, not gameplay logic. The aggregate lets the free fn
/// operate on split borrows without taking on the full `&mut Voxel`
/// (which would conflict with the `&mut self.net` borrow held by
/// the caller).
pub(crate) struct ClientInboxCtx<'a> {
    pub to_apply: &'a mut Vec<EditMsg>,
    pub client_snapshot: &'a mut Option<InvSnapshotMsg>,
    pub mob_updates: &'a mut Vec<MobMsg>,
    pub mob_events: &'a mut Vec<MobEventMsg>,
    pub mob_removals: &'a mut Vec<u32>,
    pub player_removals: &'a mut Vec<u32>,
    pub interp_updates: &'a mut Vec<PlayerMsg>,
    pub pvp_self_damage: &'a mut f32,
    pub pvp_self_heal: &'a mut f32,
    pub chat_inbox: &'a mut Vec<String>,
    pub names_update: &'a mut Option<Vec<(u32, String)>>,
    pub teleport_request: &'a mut Option<TeleportMsg>,
    pub day_time_update: &'a mut Option<f32>,
    pub seed_update: &'a mut Option<u32>,
    pub weather_update: &'a mut Option<u8>,
    pub snow_updates: &'a mut Vec<([i32; 3], u8)>,
    pub liquid_level_updates: &'a mut Vec<([i32; 3], u8)>,
    pub drop_spawns: &'a mut Vec<DropSpawnMsg>,
    pub drop_removes: &'a mut Vec<u32>,
    pub torch_orient_updates: &'a mut Vec<([i32; 3], u8)>,
    /// W125: door place/toggle state updates (lower pos, facing,
    /// open) received this tick. Applied + remeshed by dispatch.
    pub door_state_updates: &'a mut Vec<([i32; 3], u8, bool)>,
    /// W126: chest facing records (pos, facing) from places.
    pub chest_facing_updates: &'a mut Vec<([i32; 3], u8)>,
    pub kick_reason: &'a mut Option<String>,
    /// W5.3a/b: incoming chest + furnace state snapshots from the
    /// host. Applied by the main loop to overwrite the local
    /// chest/furnace contents + runtime triple.
    pub chest_state_updates: &'a mut Vec<ChestStateMsg>,
    pub furnace_state_updates: &'a mut Vec<FurnaceStateMsg>,
}

/// Drain the client socket's inbound queue, decode each `vx.*`
/// message, and route its payload into the matching collector on
/// `ctx`. Server echoes for already-pending edits are retired by
/// matching coordinates+block against `pending_edits`; the local
/// remote pose table is kept own-id-free so a pre-handshake
/// broadcast can't accidentally render a duplicate of the local
/// player.
///
/// The caller is responsible for applying the collected payloads
/// to the world / inventory / chat state after this returns - the
/// `&mut self.net` borrow held here prevents us from touching
/// anything else on the Voxel.
pub(crate) fn process_client_inbox(
    client: &mut NetClient,
    remote: &mut HashMap<u32, PlayerMsg>,
    pending_edits: &mut Vec<EditMsg>,
    pending_inv_actions: &mut Vec<InvActionMsg>,
    ctx: &mut ClientInboxCtx<'_>,
) {
    let cfg = bincode::config::standard();
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
                    ctx.to_apply.push(e);
                }
            }
            "vx.player" => {
                if let Ok((p, _)) =
                    bincode::serde::decode_from_slice::<PlayerMsg, _>(&data, cfg)
                    && my_id != Some(p.id)
                {
                    remote.insert(p.id, p);
                    ctx.interp_updates.push(p);
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
                    *ctx.client_snapshot = Some(s);
                    // Server acknowledged our state, so
                    // any pending drag intents are now
                    // resolved one way or the other.
                    pending_inv_actions.clear();
                }
            }
            "vx.chest_state" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<ChestStateMsg, _>(&data, cfg)
                {
                    ctx.chest_state_updates.push(m);
                }
            }
            "vx.furnace_state" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<FurnaceStateMsg, _>(&data, cfg)
                {
                    ctx.furnace_state_updates.push(m);
                }
            }
            "vx.mob" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<MobMsg, _>(&data, cfg)
                {
                    ctx.mob_updates.push(m);
                }
            }
            "vx.mob_event" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<MobEventMsg, _>(&data, cfg)
                {
                    ctx.mob_events.push(m);
                }
            }
            "vx.mob_remove" => {
                if let Ok((id, _)) =
                    bincode::serde::decode_from_slice::<u32, _>(&data, cfg)
                {
                    ctx.mob_removals.push(id);
                }
            }
            "vx.player_remove" => {
                if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                    PlayerRemoveMsg,
                    _,
                >(&data, cfg)
                {
                    ctx.player_removals.push(m.id);
                }
            }
            "vx.chat" => {
                if let Ok((line, _)) =
                    bincode::serde::decode_from_slice::<String, _>(&data, cfg)
                {
                    ctx.chat_inbox.push(line);
                }
            }
            "vx.names" => {
                if let Ok((entries, _)) = bincode::serde::decode_from_slice::<
                    Vec<(u32, String)>,
                    _,
                >(&data, cfg)
                {
                    *ctx.names_update = Some(entries);
                }
            }
            "vx.damage" => {
                if let Ok((d, _)) =
                    bincode::serde::decode_from_slice::<DamageMsg, _>(&data, cfg)
                {
                    // Negative amounts represent
                    // heals (server uses the same
                    // channel for both directions).
                    if d.amount >= 0.0 {
                        *ctx.pvp_self_damage += d.amount;
                    } else {
                        *ctx.pvp_self_heal += -d.amount;
                    }
                }
            }
            "vx.teleport" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<TeleportMsg, _>(&data, cfg)
                {
                    *ctx.teleport_request = Some(m);
                }
            }
            "vx.day" => {
                if let Ok((dt, _)) =
                    bincode::serde::decode_from_slice::<f32, _>(&data, cfg)
                {
                    *ctx.day_time_update = Some(dt);
                }
            }
            "vx.seed" => {
                if let Ok((seed, _)) =
                    bincode::serde::decode_from_slice::<u32, _>(&data, cfg)
                {
                    *ctx.seed_update = Some(seed);
                }
            }
            "vx.weather" => {
                if let Ok((w, _)) =
                    bincode::serde::decode_from_slice::<u8, _>(&data, cfg)
                {
                    *ctx.weather_update = Some(w);
                }
            }
            "vx.snow" => {
                if let Ok(((v, layers), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.snow_updates.push((v, layers));
                }
            }
            "vx.liquid_lvl" => {
                if let Ok(((v, level), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.liquid_level_updates.push((v, level));
                }
            }
            "vx.drop_spawn" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<DropSpawnMsg, _>(&data, cfg)
                {
                    ctx.drop_spawns.push(m);
                }
            }
            "vx.drop_remove" => {
                if let Ok((id, _)) =
                    bincode::serde::decode_from_slice::<u32, _>(&data, cfg)
                {
                    ctx.drop_removes.push(id);
                }
            }
            "vx.torch_orient" => {
                if let Ok(((v, orient), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.torch_orient_updates.push((v, orient));
                }
            }
            "vx.door_state" => {
                // W125: door facing/open from the host.
                if let Ok(((v, facing, open), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8, bool), _>(
                        &data, cfg,
                    )
                {
                    ctx.door_state_updates.push((v, facing, open));
                }
            }
            "vx.chest_facing" => {
                // W126: chest latch direction from the host.
                if let Ok(((v, facing), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.chest_facing_updates.push((v, facing));
                }
            }
            "vx.kick" => {
                if let Ok((reason, _)) =
                    bincode::serde::decode_from_slice::<String, _>(&data, cfg)
                {
                    *ctx.kick_reason = Some(reason);
                }
            }
            _ => {}
        }
    }
}

/// Aggregate for the host-side inbox drain. Wraps the subset of
/// `Voxel` state the server arm of `tick_net` mutates (`day_time`,
/// `last_day_broadcast`, plus the same per-tick collectors the
/// client arm uses), along with three local outputs the surrounding
/// `tick_net` consumes after the drain releases (`raw_edits`,
/// `path_requests`, `raw_drags`).
///
/// The `NetRole::Server { server, poses, inventories,
/// pending_inv_sends }` borrow held by `tick_net` is still passed
/// as separate `&mut` parameters - those four references are
/// mutually disjoint from the `Voxel` field refs in this ctx, so
/// the split-borrow shape stays unambiguous to the compiler.
pub(crate) struct ServerInboxCtx<'a> {
    /// Authoritative day clock - bumped by `vx.sleep` when the
    /// host accepts a client's bed request.
    pub day_time: &'a mut f32,
    /// Last-broadcast timestamp of `vx.day`. Cleared so the next
    /// tick definitely re-emits the time of day after a sleep
    /// skip moves the clock.
    pub last_day_broadcast: &'a mut Option<std::time::Instant>,
    /// True while the host has /worldlock engaged - client edits
    /// are silently dropped.
    pub world_locked: bool,
    /// Snapshot of the host's eye position for PvP-hit reach
    /// validation against id 0.
    pub host_player_pos: [f32; 3],
    /// Outputs filled by the drain + drained after the match arm
    /// closes for validated world / inventory mutation.
    pub raw_edits: &'a mut Vec<(u32, EditMsg)>,
    pub path_requests: &'a mut Vec<(u32, [i32; 3])>,
    pub raw_drags: &'a mut Vec<(u32, InvActionMsg)>,
    /// Shared collectors (same shape as `ClientInboxCtx`).
    pub interp_updates: &'a mut Vec<PlayerMsg>,
    pub mob_hits_to_apply: &'a mut Vec<(u32, u32)>,
    pub torch_orient_updates: &'a mut Vec<([i32; 3], u8)>,
    /// W125: door place/toggle state updates (lower pos, facing,
    /// open) received this tick. Applied + remeshed by dispatch.
    pub door_state_updates: &'a mut Vec<([i32; 3], u8, bool)>,
    /// W126: chest facing records (pos, facing) from places.
    pub chest_facing_updates: &'a mut Vec<([i32; 3], u8)>,
    pub chat_inbox: &'a mut Vec<String>,
    pub pvp_self_damage: &'a mut f32,
    /// W5.3a: pending chest open / act / close events for the
    /// host's main loop to apply against the live `chests` map +
    /// `chest_viewers` registration.
    pub chest_opens: &'a mut Vec<(u32, ChestOpenMsg)>,
    pub chest_acts: &'a mut Vec<(u32, ChestActMsg)>,
    pub chest_closes: &'a mut Vec<(u32, ChestCloseMsg)>,
    /// W5.3b: same for furnaces.
    pub furnace_opens: &'a mut Vec<(u32, FurnaceOpenMsg)>,
    pub furnace_acts: &'a mut Vec<(u32, FurnaceActMsg)>,
    pub furnace_closes: &'a mut Vec<(u32, FurnaceCloseMsg)>,
}

/// Drain the server socket's inbound queue and dispatch each
/// message to the right collector / inline broadcast. Mirrors the
/// per-topic match block from `Voxel::tick_net`'s server arm.
///
/// `server`, `poses`, `inventories`, `pending_inv_sends` are
/// passed as plain `&mut` parameters because they live inside the
/// `NetRole::Server { .. }` destructuring at the call site - the
/// `&mut self.net` borrow keeps them disjoint from the `Voxel`
/// refs inside `ctx`.
///
/// Wide ABI on purpose: every coupling between the wire decode
/// and the host's authoritative state lives explicitly in this
/// signature, so the call site in `tick_net` reads as a single
/// data-flow declaration.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_server_inbox(
    server: &mut NetServer,
    poses: &mut HashMap<u32, PlayerMsg>,
    inventories: &mut HashMap<u32, ClientInv>,
    pending_inv_sends: &mut HashSet<u32>,
    pvp_reach: f32,
    pvp_damage: f32,
    day_length_s: f32,
    recipes_lookup: &dyn Fn(usize) -> Option<&'static [(u16, u8)]>,
    ctx: &mut ServerInboxCtx<'_>,
) {
    let cfg = bincode::config::standard();
    for (client_id, kind, data) in server.drain_bytes() {
        match kind.as_str() {
            "vx.edit" => {
                // World lock: ignore client edits
                // entirely. The host's own edits go
                // through the local pipeline + still
                // broadcast.
                if ctx.world_locked {
                    continue;
                }
                if let Ok((e, _)) =
                    bincode::serde::decode_from_slice::<EditMsg, _>(&data, cfg)
                {
                    ctx.raw_edits.push((client_id, e));
                }
            }
            "vx.player" => {
                if let Ok((mut p, _)) =
                    bincode::serde::decode_from_slice::<PlayerMsg, _>(&data, cfg)
                {
                    p.id = client_id;
                    poses.insert(client_id, p);
                    ctx.interp_updates.push(p);
                }
            }
            "vx.inv_act" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<InvActionMsg, _>(&data, cfg)
                {
                    ctx.raw_drags.push((client_id, m));
                }
            }
            "vx.chest_open" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<ChestOpenMsg, _>(&data, cfg)
                {
                    ctx.chest_opens.push((client_id, m));
                }
            }
            "vx.chest_act" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<ChestActMsg, _>(&data, cfg)
                {
                    ctx.chest_acts.push((client_id, m));
                }
            }
            "vx.chest_close" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<ChestCloseMsg, _>(&data, cfg)
                {
                    ctx.chest_closes.push((client_id, m));
                }
            }
            "vx.furnace_open" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<FurnaceOpenMsg, _>(&data, cfg)
                {
                    ctx.furnace_opens.push((client_id, m));
                }
            }
            "vx.furnace_act" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<FurnaceActMsg, _>(&data, cfg)
                {
                    ctx.furnace_acts.push((client_id, m));
                }
            }
            "vx.furnace_close" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<FurnaceCloseMsg, _>(&data, cfg)
                {
                    ctx.furnace_closes.push((client_id, m));
                }
            }
            "vx.recipe" => {
                // Client asked the server to auto-
                // fill its craft grid with recipe
                // index `idx`. Validate the sender's
                // inventory carries every input,
                // move them into craft_grid, and
                // queue an inv snapshot back.
                if let Ok((idx, _)) =
                    bincode::serde::decode_from_slice::<u32, _>(&data, cfg)
                    && let Some(recipe_inputs) = recipes_lookup(idx as usize)
                    && let Some(inv) = inventories.get_mut(&client_id)
                {
                    let mut taken: Vec<(usize, (BlockId, u8))> = Vec::new();
                    let mut placed: [Option<(BlockId, u8)>; 4] = [None; 4];
                    let mut ok = true;
                    for (i, (bid, n)) in recipe_inputs.iter().enumerate() {
                        if i >= 4 {
                            break;
                        }
                        let need = *n;
                        let target = BlockId(*bid);
                        let mut found: Option<usize> = None;
                        for (si, slot) in inv.inventory.iter().enumerate() {
                            if let Some((b, c)) = slot
                                && *b == target
                                && *c >= need
                            {
                                found = Some(si);
                                break;
                            }
                        }
                        match found {
                            Some(si) => {
                                let original =
                                    inv.inventory[si].expect("verified");
                                taken.push((si, original));
                                let remaining =
                                    original.1.saturating_sub(need);
                                inv.inventory[si] = if remaining == 0 {
                                    None
                                } else {
                                    Some((original.0, remaining))
                                };
                                placed[i] = Some((target, need));
                            }
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        // Return any existing
                        // craft-grid stacks to the
                        // inventory via inv_pickup
                        // before overwriting.
                        let returns: Vec<(BlockId, u8)> = inv
                            .craft_grid
                            .iter_mut()
                            .filter_map(|s| s.take())
                            .collect();
                        for (b, c) in returns {
                            for _ in 0..c {
                                inv_pickup(&mut inv.inventory, b);
                            }
                        }
                        inv.craft_grid = placed;
                    } else {
                        for (si, original) in taken {
                            inv.inventory[si] = Some(original);
                        }
                    }
                    pending_inv_sends.insert(client_id);
                }
            }
            "vx.mob_hit" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<MobHitMsg, _>(&data, cfg)
                {
                    ctx.mob_hits_to_apply.push((client_id, m.mob_id));
                }
            }
            "vx.sleep" => {
                // Client requested a time skip (bed
                // RMB). Authoritative side validates
                // it's night before honouring; force
                // a vx.day broadcast next tick so
                // every peer sees the morning jump.
                let phase = (*ctx.day_time / day_length_s).rem_euclid(1.0);
                let elev = (phase * std::f32::consts::TAU).sin();
                if elev < -0.05 {
                    *ctx.day_time = day_length_s * 0.20;
                    *ctx.last_day_broadcast = None;
                    let sender = server
                        .client_name(client_id)
                        .unwrap_or_else(|| format!("p{client_id}"));
                    let line =
                        format!("* {sender} sleeps through the night");
                    ctx.chat_inbox.push(line.clone());
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec(&line, cfg)
                    {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
            }
            "vx.path" => {
                // Defer processing until the match
                // arm releases self.net so we can
                // call world_set_tracked + broadcast.
                if let Ok((v, _)) =
                    bincode::serde::decode_from_slice::<[i32; 3], _>(&data, cfg)
                {
                    ctx.path_requests.push((client_id, v));
                }
            }
            "vx.torch_orient" => {
                // Cosmetic lean info that rides
                // alongside a torch place. Edit itself
                // was already reach-validated via
                // `vx.edit`; here we just record + fan
                // out to every other peer so they
                // render the matching wall / floor
                // mount.
                if let Ok(((v, orient), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.torch_orient_updates.push((v, orient));
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec((v, orient), cfg)
                    {
                        server.broadcast_bytes("vx.torch_orient", buf);
                    }
                }
            }
            "vx.door_state" => {
                // W125: a client placed/toggled a door. Record
                // locally + fan out so every peer flips the same
                // panel. (Reach was validated by the paired
                // vx.edit for places; toggles are cosmetic+door
                // -local so a light touch is fine here.)
                if let Ok(((v, facing, open), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8, bool), _>(
                        &data, cfg,
                    )
                {
                    ctx.door_state_updates.push((v, facing, open));
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec((v, facing, open), cfg)
                    {
                        server.broadcast_bytes("vx.door_state", buf);
                    }
                }
            }
            "vx.chest_facing" => {
                // W126: cosmetic chest yaw riding alongside a
                // chest place (reach validated by the vx.edit).
                if let Ok(((v, facing), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.chest_facing_updates.push((v, facing));
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec((v, facing), cfg)
                    {
                        server.broadcast_bytes("vx.chest_facing", buf);
                    }
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
                if let Ok((text, _)) =
                    bincode::serde::decode_from_slice::<String, _>(&data, cfg)
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
                            let line =
                                format!("[whisper from {sender}] {body}");
                            if tid == 0 {
                                ctx.chat_inbox.push(line);
                            } else if let Ok(buf) =
                                bincode::serde::encode_to_vec(&line, cfg)
                            {
                                server.send_bytes_to_client(
                                    tid, "vx.chat", buf,
                                );
                            }
                        }
                    } else {
                        let line = format!("<{sender}> {text}");
                        ctx.chat_inbox.push(line.clone());
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(&line, cfg)
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
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<PvPHitMsg, _>(&data, cfg)
                {
                    let attacker_pos =
                        poses.get(&client_id).copied().map(|p| p.pos);
                    let target_pos = if m.target_id == 0 {
                        Some(ctx.host_player_pos)
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
                        if d2 <= (pvp_reach + 1.0).powi(2) {
                            if m.target_id == 0 {
                                // Host is the target.
                                *ctx.pvp_self_damage += pvp_damage;
                            } else {
                                let dmg = DamageMsg { amount: pvp_damage };
                                if let Ok(buf) =
                                    bincode::serde::encode_to_vec(dmg, cfg)
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
}


// W47.6 - per-frame net IO dispatch lives in dispatch.rs.
pub(crate) mod dispatch;


// W47.8 - chest + furnace net message appliers + broadcasts.
pub(crate) mod container_apply;
