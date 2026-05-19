//! Phase L.6 tests for `Voxel::tick_lighting` + the
//! `world_set_tracked` dirty-set wiring.
//!
//! Block edits flag the touched voxel as dirty in
//! `voxel.lighting.dirty`; the next `tick_lighting` call drains
//! that set, re-floods every dirty voxel's owning chunk + the
//! 26-neighbour halo, and returns the affected chunk coords for
//! the caller to remesh.

use infengine_voxel::{BlockId, Chunk};

use crate::Voxel;
use crate::block::{GLOWSTONE_ID, TORCH_ID};

const ZERO_CHUNK: [i32; 3] = [0, 0, 0];

fn fresh_voxel_with_one_chunk() -> Voxel {
    let mut v = Voxel::new();
    v.world.insert_chunk(ZERO_CHUNK, Chunk::empty());
    v
}

#[test]
fn world_set_tracked_marks_dirty() {
    // Any block edit through `world_set_tracked` must stamp the
    // dirty set so the next lighting tick picks it up.
    let mut v = fresh_voxel_with_one_chunk();
    v.world_set_tracked([5, 5, 5], BlockId(TORCH_ID));
    let drained = v.lighting.take_dirty();
    assert!(drained.contains(&[5, 5, 5]));
}

#[test]
fn tick_lighting_propagates_block_light_after_edit() {
    let mut v = fresh_voxel_with_one_chunk();
    v.world_set_tracked([5, 5, 5], BlockId(TORCH_ID));
    let affected = v.tick_lighting();
    assert!(affected.contains(&ZERO_CHUNK));
    // After tick, the lighting grid has the torch at 14 + its
    // neighbour at 13.
    let g = v.lighting.get(ZERO_CHUNK).unwrap();
    assert_eq!(g.get_block([5, 5, 5]), 14);
    assert_eq!(g.get_block([6, 5, 5]), 13);
}

#[test]
fn tick_lighting_returns_empty_when_dirty_set_empty() {
    let mut v = fresh_voxel_with_one_chunk();
    let affected = v.tick_lighting();
    assert!(affected.is_empty());
}

#[test]
fn placing_then_removing_torch_zeros_block_light_via_two_ticks() {
    let mut v = fresh_voxel_with_one_chunk();
    v.world_set_tracked([5, 5, 5], BlockId(TORCH_ID));
    let _ = v.tick_lighting();
    assert!(v.lighting.get(ZERO_CHUNK).unwrap().get_block([5, 5, 5]) > 0);
    v.world_set_tracked([5, 5, 5], BlockId::AIR);
    let _ = v.tick_lighting();
    // After the second tick the cell + every cell it had reached
    // are back to 0.
    let g = v.lighting.get(ZERO_CHUNK).unwrap();
    for x in 0..16u32 {
        for y in 0..16u32 {
            for z in 0..16u32 {
                assert_eq!(g.get_block([x, y, z]), 0);
            }
        }
    }
}

#[test]
fn tick_lighting_affects_neighbour_chunks_via_halo() {
    // Place a torch at (15, 5, 5) of chunk [0,0,0] - one cell off
    // the +X boundary. The 3x3x3 chunk halo around [0,0,0]
    // includes [1,0,0]. tick_lighting must include both chunks
    // in its affected list AS LONG AS both are loaded.
    let mut v = fresh_voxel_with_one_chunk();
    v.world.insert_chunk([1, 0, 0], Chunk::empty());
    v.world_set_tracked([15, 5, 5], BlockId(TORCH_ID));
    let affected = v.tick_lighting();
    assert!(affected.contains(&ZERO_CHUNK));
    assert!(affected.contains(&[1, 0, 0]));
    // Light bled across: cell at (0, 5, 5) of chunk [1,0,0] sees
    // a single decay step (1 cell away from the torch).
    let g = v.lighting.get([1, 0, 0]).unwrap();
    assert_eq!(g.get_block([0, 5, 5]), 13);
}

#[test]
fn tick_lighting_skips_unloaded_neighbours() {
    // Only chunk [0,0,0] is loaded. A torch at (15, 5, 5) would
    // try to bleed into the missing [1,0,0]; tick_lighting must
    // filter it out + return only the loaded chunk.
    let mut v = fresh_voxel_with_one_chunk();
    v.world_set_tracked([15, 5, 5], BlockId(TORCH_ID));
    let affected = v.tick_lighting();
    assert_eq!(affected.len(), 1);
    assert!(affected.contains(&ZERO_CHUNK));
}

#[test]
fn glowstone_block_emits_via_tick_path() {
    let mut v = fresh_voxel_with_one_chunk();
    v.world_set_tracked([8, 8, 8], BlockId(GLOWSTONE_ID));
    let _ = v.tick_lighting();
    let g = v.lighting.get(ZERO_CHUNK).unwrap();
    assert_eq!(g.get_block([8, 8, 8]), 15);
}

#[test]
fn dirty_set_drains_on_tick() {
    let mut v = fresh_voxel_with_one_chunk();
    v.world_set_tracked([1, 1, 1], BlockId(TORCH_ID));
    let _ = v.tick_lighting();
    // After the tick the dirty set is empty.
    let drained = v.lighting.take_dirty();
    assert!(drained.is_empty());
}

#[test]
fn tick_lighting_sky_layer_uncovers_on_break() {
    // Carve a vertical shaft by breaking a ceiling cell. Sky
    // should reach the floor of the shaft after one lighting
    // tick.
    let mut v = fresh_voxel_with_one_chunk();
    // Build a ceiling at y=10 across the whole chunk.
    for x in 0..16i32 {
        for z in 0..16i32 {
            v.world.set([x, 10, z], BlockId(1));
        }
    }
    // Initial lighting pass - covered cells stay dark.
    v.lighting.mark_dirty([0, 10, 0]);
    let _ = v.tick_lighting();
    let g = v.lighting.get(ZERO_CHUNK).unwrap();
    assert_eq!(g.get_sky([5, 5, 5]), 0);
    // Now break the ceiling cell at (5, 10, 5). The next tick
    // should let sky reach the column below.
    v.world_set_tracked([5, 10, 5], BlockId::AIR);
    let _ = v.tick_lighting();
    let g = v.lighting.get(ZERO_CHUNK).unwrap();
    assert_eq!(g.get_sky([5, 5, 5]), 15);
}

#[test]
fn multiple_dirty_voxels_coalesce_into_single_chunk_re_flood() {
    // Edit two cells in the same chunk - the affected set should
    // contain the chunk once (HashSet dedup).
    let mut v = fresh_voxel_with_one_chunk();
    v.world_set_tracked([1, 1, 1], BlockId(TORCH_ID));
    v.world_set_tracked([14, 14, 14], BlockId(TORCH_ID));
    let affected = v.tick_lighting();
    // 3x3 chunk halo around each dirty voxel coalesces - only
    // the loaded chunk [0,0,0] survives the existence filter.
    assert_eq!(affected.len(), 1);
    assert!(affected.contains(&ZERO_CHUNK));
}
