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
pub(crate) const SERVER_STATE_VERSION: u32 = 2;

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

fn default_player_health() -> f32 {
    PLAYER_MAX_HEALTH
}

/// On-disk per-client inventory snapshot. `(block_id, count)`
/// tuples mirror the in-memory `[Option<(BlockId, u8)>; N]`
/// layout but use `u16` for the block so the format doesn't break
/// if `BlockId` ever grows fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedInv {
    pub(crate) inventory: Vec<Option<(u16, u8)>>,
    pub(crate) craft: Vec<Option<(u16, u8)>>,
}

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
