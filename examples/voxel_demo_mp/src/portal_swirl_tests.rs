//! W5.2a regression suite: portal block renders an animated swirl.
//!
//! The actual draw call goes through `renderer.draw_mesh_pbr` so
//! we can't exercise the GPU path in a unit test. These tests
//! instead lock the contract that drives the per-frame render
//! pass: the portal block carries the right flag set, and
//! `world_set_tracked` keeps the `portal_positions` index in sync
//! so the render loop sees the right cells.

use infengine_voxel::{BlockId, Chunk};

use crate::block::{
    F_AABB_PASS, F_DEDICATED_RENDER, F_RAY_PASS, F_TRANSPARENT_RENDER,
    PORTAL_ID, block_has_flag,
};
use crate::Voxel;

/// W5.2a: the portal block sits on a fixed contract:
/// - `F_RAY_PASS`: rays don't stop at the portal cell so the
///   player can target blocks behind it.
/// - `F_AABB_PASS`: player AABB walks through; required so the
///   portal-overlap tick can fire.
/// - `F_DEDICATED_RENDER`: chunk mesher skips the cell; the
///   animated swirl pass paints it instead.
/// - `F_TRANSPARENT_RENDER`: alpha-blend pipeline, no shadow.
#[test]
fn portal_block_carries_swirl_render_flags() {
    let b = BlockId(PORTAL_ID);
    assert!(block_has_flag(b, F_RAY_PASS));
    assert!(block_has_flag(b, F_AABB_PASS));
    assert!(block_has_flag(b, F_DEDICATED_RENDER));
    assert!(block_has_flag(b, F_TRANSPARENT_RENDER));
}

/// Placing a portal voxel via `world_set_tracked` indexes the
/// cell in `portal_positions`; the render pass walks this set so
/// every lit cell gets a draw call.
#[test]
fn world_set_tracked_indexes_portal_position() {
    let mut v = Voxel::new();
    // Pre-insert the chunk so world.set has somewhere to write.
    v.world.insert_chunk([0, 0, 0], Chunk::empty());
    let pos = [3, 5, 7];
    v.world_set_tracked(pos, BlockId(PORTAL_ID));
    assert!(v.portal_positions.contains(&pos));
}

/// Replacing a portal cell with air via `world_set_tracked` evicts
/// the entry so the render pass stops drawing at that pos.
#[test]
fn world_set_tracked_evicts_portal_on_air() {
    let mut v = Voxel::new();
    v.world.insert_chunk([0, 0, 0], Chunk::empty());
    let pos = [3, 5, 7];
    v.world_set_tracked(pos, BlockId(PORTAL_ID));
    assert!(v.portal_positions.contains(&pos));
    v.world_set_tracked(pos, BlockId::AIR);
    assert!(!v.portal_positions.contains(&pos));
}

/// `reset_world` clears the portal tracker so a fresh world load
/// doesn't render leftover swirls from a previous session.
#[test]
fn reset_world_clears_portal_positions() {
    let mut v = Voxel::new();
    v.world.insert_chunk([0, 0, 0], Chunk::empty());
    v.world_set_tracked([1, 1, 1], BlockId(PORTAL_ID));
    assert!(!v.portal_positions.is_empty());
    v.reset_world();
    assert!(v.portal_positions.is_empty());
}

/// Multiple portal cells (a typical 2x3 frame interior) all land
/// in the tracker. The render pass walks the set so all 6 cells
/// get a swirl panel.
#[test]
fn six_cell_portal_frame_indexed() {
    let mut v = Voxel::new();
    v.world.insert_chunk([0, 0, 0], Chunk::empty());
    for x in 0..=1 {
        for y in 0..=2 {
            v.world_set_tracked([x, y, 5], BlockId(PORTAL_ID));
        }
    }
    assert_eq!(v.portal_positions.len(), 6);
    for x in 0..=1 {
        for y in 0..=2 {
            assert!(v.portal_positions.contains(&[x, y, 5]));
        }
    }
}
