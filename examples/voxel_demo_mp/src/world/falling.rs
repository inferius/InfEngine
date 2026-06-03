//! W64 - falling-block physics (sand / concrete_powder / gravel).
//!
//! MC mirror: `net/minecraft/world/level/block/FallingBlock.java`.
//! In MC a falling block schedules a tick on placement / neighbour
//! change; when the cell below is "free" it spawns a
//! `FallingBlockEntity` that drops under gravity and re-deposits the
//! block on landing.
//!
//! The voxel engine has no per-entity gravity sim for blocks, so we
//! approximate with a stepwise voxel relocation: each falling tick
//! every queued unsupported falling cell drops one voxel (clear old,
//! set new), re-queuing the destination + the cell it vacated so a
//! whole column collapses bottom-up over consecutive ticks. The
//! visual reads as a quick tumble; the net traffic is the same
//! `vx.edit` stream a manual dig / place produces.
//!
//! Trigger points (host-only, MC neighbour-update model):
//! - `apply_block_place`: a freshly placed falling block enqueues
//!   itself.
//! - `apply_block_break`: the cell directly above a removed block
//!   enqueues (it may have just lost its support).
//!
//! The tick re-queues to keep cascades + chains going. Worldgen
//! never enqueues, so naturally-floating sand stays put until the
//! player disturbs it - matching MC.

use crate::net::NetRole;
use crate::{BlockId, EditMsg, Voxel};

/// Per-tick cap on falling-cell relocations so a giant collapse
/// drains across several frames instead of stalling one. Cells over
/// the cap keep their slot for the next tick.
const MAX_FALLING_PER_TICK: usize = 2048;

/// W64: block ids that obey gravity. Sand (4), the 16
/// `concrete_powder` colours (201..=216), and gravel / red_sand
/// (W64.2, ids 644/645). `soul_sand` (374) is intentionally excluded
/// - it's a solid support block in MC, not a falling one.
pub(crate) fn is_falling_block(id: u16) -> bool {
    matches!(id, 4 | 201..=216)
        || id == crate::block::GRAVEL_ID
        || id == crate::block::RED_SAND_ID
}

/// Pure decision used by [`tick`]: the destination cell when the
/// block at `pos` is a falling block resting on air, else `None`.
/// The `pos[1] >= 1` guard keeps a block from falling past the world
/// floor (`y = 0`) into the unloaded void, where `World::set` would
/// silently drop it.
pub(crate) fn fall_target(
    world: &infengine_voxel::World,
    pos: [i32; 3],
) -> Option<[i32; 3]> {
    if pos[1] < 1 || !is_falling_block(world.get(pos).0) {
        return None;
    }
    let below = [pos[0], pos[1] - 1, pos[2]];
    if world.get(below).is_air() {
        Some(below)
    } else {
        None
    }
}

/// Host-side falling-block tick. Drops every queued unsupported
/// falling cell one voxel toward the ground, re-queuing so the fall
/// continues next tick. No-op on clients (they receive the resulting
/// `vx.edit` traffic and apply it like any other edit).
pub(crate) fn tick(vx: &mut Voxel) {
    if matches!(vx.net, NetRole::Client { .. }) {
        // A client should never accumulate a queue (its place /
        // break hooks skip the insert), but stay defensive.
        vx.falling_pending.clear();
        return;
    }
    if vx.falling_pending.is_empty() {
        return;
    }
    // Drain this frame's batch up front so destinations re-queued
    // below only move on the *next* tick (one voxel per tick).
    let batch: Vec<[i32; 3]> = vx.falling_pending.drain().collect();
    let mut processed = 0usize;
    for pos in batch {
        if processed >= MAX_FALLING_PER_TICK {
            // Over budget: hold the cell for the next tick.
            vx.falling_pending.insert(pos);
            continue;
        }
        let Some(below) = fall_target(&vx.world, pos) else {
            continue;
        };
        processed += 1;
        let moved = vx.world.get(pos);
        // Relocate one voxel down through the shared edit funnel.
        emit_edit(vx, below, moved);
        emit_edit(vx, pos, BlockId::AIR);
        // Keep the fall going + wake the cell that just lost its
        // floor (it may be a falling block too).
        vx.falling_pending.insert(below);
        vx.falling_pending.insert([pos[0], pos[1] + 1, pos[2]]);
    }
}

/// Apply one block edit the same way a manual dig / place does:
/// write the cell with side-table bookkeeping via
/// `world_set_tracked`, remesh the affected chunks, broadcast
/// `vx.edit` on the server, and record it in `edit_history`.
/// Host-only - the caller ([`tick`]) gates on `NetRole`.
fn emit_edit(vx: &mut Voxel, pos: [i32; 3], block: BlockId) {
    vx.world_set_tracked(pos, block);
    let coords = Voxel::affected_chunks(pos);
    vx.rebuild_chunks(&coords);
    let edit = EditMsg {
        x: pos[0],
        y: pos[1],
        z: pos[2],
        block: block.0,
    };
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
            server.broadcast_bytes("vx.edit", buf);
        }
    }
    vx.edit_history.push(edit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{Chunk, World};

    /// 2x2x2 chunk grid around origin so `set` / `get` work over a
    /// generous `[0..32)` cube of writable coords.
    fn test_world() -> World {
        let mut w = World::new();
        for cx in 0..=1 {
            for cy in 0..=1 {
                for cz in 0..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
        w
    }

    #[test]
    fn sand_and_concrete_powder_fall_dirt_and_stone_do_not() {
        assert!(is_falling_block(4)); // sand
        assert!(is_falling_block(201)); // white_concrete_powder
        assert!(is_falling_block(216)); // black_concrete_powder
        assert!(is_falling_block(crate::block::GRAVEL_ID)); // W64.2
        assert!(is_falling_block(crate::block::RED_SAND_ID)); // W64.2
        assert!(!is_falling_block(1)); // stone
        assert!(!is_falling_block(2)); // dirt
        assert!(!is_falling_block(374)); // soul_sand - solid in MC
    }

    #[test]
    fn fall_target_air_below_returns_below() {
        let mut w = test_world();
        w.set([5, 10, 5], BlockId(4)); // sand with air beneath
        assert_eq!(fall_target(&w, [5, 10, 5]), Some([5, 9, 5]));
    }

    #[test]
    fn fall_target_supported_returns_none() {
        let mut w = test_world();
        w.set([5, 10, 5], BlockId(4)); // sand
        w.set([5, 9, 5], BlockId(1)); // stone support below
        assert_eq!(fall_target(&w, [5, 10, 5]), None);
    }

    #[test]
    fn fall_target_non_falling_block_returns_none() {
        let mut w = test_world();
        w.set([5, 10, 5], BlockId(1)); // stone never falls
        assert_eq!(fall_target(&w, [5, 10, 5]), None);
    }

    #[test]
    fn fall_target_respects_world_floor() {
        let mut w = test_world();
        w.set([5, 0, 5], BlockId(4)); // sand on the floor row
        // Even with air "below" (y = -1, void) the guard rests it.
        assert_eq!(fall_target(&w, [5, 0, 5]), None);
    }
}
