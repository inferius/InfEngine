//! Floating drop entities + their physics, pickup, and magnet
//! pull.
//!
//! Owns the on-the-ground item lifecycle: a mining break or a
//! liquid washing a decoration away calls `spawn` to mint a
//! fresh `DropItem`, the host steps gravity / drag / pickup each
//! tick via `tick_physics`, and a server broadcast tells every
//! connected peer about spawn / pickup events. Despawn timing
//! follows MC's 5-minute rule scaled down (`DROP_DESPAWN_SEC`)
//! so worlds don't accumulate forgotten clutter.
//!
//! Two parameters tune the "vacuum" feel of pickups: drops
//! magnetise toward the player inside `DROP_ATTRACT_RADIUS` with
//! acceleration scaled by `DROP_MAGNET_STRENGTH`, and snap into
//! the inventory once they're closer than `DROP_COLLECT_RADIUS`.
//!
//! State lives on `Voxel`: `dropped_items: Vec<DropItem>` (the
//! host-authoritative list, mirrored on clients for rendering)
//! and `next_drop_id: u32` (monotonic id source on the host).

use infengine_voxel::BlockId;

use crate::liquid::liquid_for;
use crate::net::{DropSpawnMsg, NetRole};
use crate::{PLAYER_HEIGHT, Voxel};

/// Floating drop entity left in the world by a mining break or
/// a liquid washing a decoration away. Drops bob in place with a
/// gentle sine wave, get picked up when a player walks within
/// `DROP_ATTRACT_RADIUS`, snap collected within
/// `DROP_COLLECT_RADIUS`, and despawn after `DROP_DESPAWN_SEC`.
/// Authoritative on the host; clients receive `vx.drop_spawn` +
/// `vx.drop_pickup` updates from the server.
#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DropItem {
    pub(crate) id: u32,
    pub(crate) block: u16,
    /// Centre of the drop in world coords; `pos[1]` is the body
    /// origin (not eye height like the player).
    pub(crate) pos: [f32; 3],
    /// Linear velocity in m/s. Drops accelerate downward via
    /// gravity until they hit ground, then bob in place.
    pub(crate) vel: [f32; 3],
    /// Seconds since spawn. Drives both the despawn timer and
    /// the bob phase.
    pub(crate) age: f32,
}

/// Distance within which a drop magnetises toward the player.
/// Inside this radius the drop accelerates toward the player's
/// chest each tick; once it reaches `DROP_COLLECT_RADIUS` it
/// snaps into the inventory.
pub(crate) const DROP_ATTRACT_RADIUS: f32 = 4.5;
/// Final pickup radius. The drop teleports into the inventory
/// when its centre is closer than this to the player.
pub(crate) const DROP_COLLECT_RADIUS: f32 = 2.5;
/// Drops despawn after this many seconds without pickup.
pub(crate) const DROP_DESPAWN_SEC: f32 = 90.0;
/// Pickup-eligible delay: the drop ignores the player for the
/// first ~0.4 s after spawning so the player isn't insta-
/// picking-up the block they just broke without time to read
/// it visually. Configurable so mods / settings can tune the
/// loot pace.
pub(crate) const DROP_PICKUP_DELAY_SEC: f32 = 0.4;
/// Magnet pull strength (m/s of acceleration toward player when
/// inside `DROP_ATTRACT_RADIUS`). Higher = snappier collection.
pub(crate) const DROP_MAGNET_STRENGTH: f32 = 14.0;

/// Spawn a fresh drop entity at the voxel centre. Host-only
/// path: assigns a new id, appends to `dropped_items`, and
/// broadcasts a `vx.drop_spawn` so every connected peer renders
/// it. Caller passes the source `block` (whatever was just
/// broken or washed) - air drops are skipped.
pub(crate) fn spawn(voxel: &mut Voxel, block: BlockId, pos: [i32; 3]) {
    if matches!(voxel.net, NetRole::Client { .. }) {
        return;
    }
    if block.is_air() {
        return;
    }
    let id = voxel.next_drop_id;
    voxel.next_drop_id = voxel.next_drop_id.wrapping_add(1).max(1);
    // Slight randomised XZ kick + small upward bounce so
    // multiple drops at the same voxel scatter visibly.
    let phase = (id as f32) * 1.732;
    let kick_x = phase.sin() * 0.6;
    let kick_z = phase.cos() * 0.6;
    let drop = DropItem {
        id,
        block: block.0,
        pos: [
            pos[0] as f32 + 0.5,
            pos[1] as f32 + 0.4,
            pos[2] as f32 + 0.5,
        ],
        vel: [kick_x, 2.0, kick_z],
        age: 0.0,
    };
    voxel.dropped_items.push(drop);
    if let NetRole::Server { server, .. } = &mut voxel.net {
        let cfg = bincode::config::standard();
        let msg = DropSpawnMsg {
            id: drop.id,
            block: drop.block,
            pos: drop.pos,
        };
        if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
            server.broadcast_bytes("vx.drop_spawn", buf);
        }
    }
}

/// Step every drop's physics + pickup logic for one frame.
/// Host runs the full simulation. Clients only advance the age +
/// apply a simple gravity preview so the bob looks right between
/// server snapshots; pickup is host-side.
pub(crate) fn tick_physics(voxel: &mut Voxel, dt: f32) {
    // Local player AABB centre for pickup checks. Drops pulled
    // in via simple radius (no AABB sweep needed for a cosmetic
    // item collision).
    let player_pos = voxel.player_pos;
    let mut to_pickup: Vec<usize> = Vec::new();
    let mut to_despawn: Vec<usize> = Vec::new();
    for (i, drop) in voxel.dropped_items.iter_mut().enumerate() {
        drop.age += dt;
        // Gravity + ground rest. Sample one voxel below the drop
        // centre; if it's solid (anything other than air or
        // another liquid) treat as floor.
        let below_vy = (drop.pos[1] - 0.2).floor() as i32;
        let below = [
            drop.pos[0].floor() as i32,
            below_vy,
            drop.pos[2].floor() as i32,
        ];
        let below_b = if matches!(voxel.net, NetRole::Client { .. }) {
            // Clients have the same world so this works on both
            // sides.
            voxel.world.get(below)
        } else {
            voxel.world.get(below)
        };
        let on_ground = !below_b.is_air()
            && liquid_for(below_b).is_none()
            && (drop.pos[1] - (below_vy as f32 + 1.0)) <= 0.05;
        if on_ground {
            drop.vel[0] *= 0.6;
            drop.vel[2] *= 0.6;
            drop.vel[1] = 0.0;
            drop.pos[1] = below_vy as f32 + 1.0;
        } else {
            drop.vel[1] -= 14.0 * dt;
            drop.vel[1] = drop.vel[1].max(-12.0);
        }
        drop.pos[0] += drop.vel[0] * dt;
        drop.pos[1] += drop.vel[1] * dt;
        drop.pos[2] += drop.vel[2] * dt;
        // Despawn after the configured lifetime.
        if drop.age >= DROP_DESPAWN_SEC {
            to_despawn.push(i);
            continue;
        }
        // Pickup: host-authoritative. Inside the attract radius
        // the drop accelerates toward the player's chest; once
        // it's within the smaller collect radius it teleports
        // into the inventory.
        if matches!(voxel.net, NetRole::Server { .. } | NetRole::Offline)
            && drop.age >= DROP_PICKUP_DELAY_SEC
        {
            let chest_y = player_pos.y - PLAYER_HEIGHT * 0.4;
            let dx = player_pos.x - drop.pos[0];
            let dy = chest_y - drop.pos[1];
            let dz = player_pos.z - drop.pos[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if dist <= DROP_COLLECT_RADIUS {
                to_pickup.push(i);
            } else if dist <= DROP_ATTRACT_RADIUS && dist > 1e-3 {
                // Magnet pull toward the player. Strength ramps
                // as the drop gets closer so it accelerates into
                // the collect band.
                let pull =
                    DROP_MAGNET_STRENGTH * (1.0 - dist / DROP_ATTRACT_RADIUS);
                let inv = pull * dt / dist;
                drop.vel[0] += dx * inv;
                drop.vel[1] += dy * inv;
                drop.vel[2] += dz * inv;
            }
        }
    }
    // Apply pickups + despawns in reverse so indices stay valid
    // as we drain.
    let mut removed_ids: Vec<u32> = Vec::new();
    let mut picked_blocks: Vec<BlockId> = Vec::new();
    for &i in to_pickup.iter().chain(to_despawn.iter()).rev() {
        if i >= voxel.dropped_items.len() {
            continue;
        }
        let drop = voxel.dropped_items.swap_remove(i);
        removed_ids.push(drop.id);
        if to_pickup.contains(&i) {
            picked_blocks.push(BlockId(drop.block));
        }
    }
    for b in picked_blocks {
        voxel.pickup(b);
    }
    if !removed_ids.is_empty()
        && let NetRole::Server { server, .. } = &mut voxel.net
    {
        let cfg = bincode::config::standard();
        for id in &removed_ids {
            if let Ok(buf) = bincode::serde::encode_to_vec(*id, cfg) {
                server.broadcast_bytes("vx.drop_remove", buf);
            }
        }
    }
}
