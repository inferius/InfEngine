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
fn rebuild_all_lighting_seeds_empty_state_from_block_grid() {
    // Phase L.7 entry point: re-derive both light layers across
    // every loaded chunk from scratch. Used after load_world +
    // switch_dim. Simulates a save round-trip by setting up a
    // torch via the world directly (not world_set_tracked) and
    // confirming `rebuild_all_lighting` puts the torch's light
    // back on the next read.
    let mut v = fresh_voxel_with_one_chunk();
    v.world.set([5, 5, 5], BlockId(TORCH_ID));
    // No dirty marks, no ticks - lighting state is empty.
    assert!(v.lighting.get(ZERO_CHUNK).is_none());
    v.rebuild_all_lighting();
    let g = v.lighting.get(ZERO_CHUNK).unwrap();
    assert_eq!(g.get_block([5, 5, 5]), 14);
    assert_eq!(g.get_block([6, 5, 5]), 13);
}

#[test]
fn rebuild_all_lighting_seeds_sky_layer_too() {
    let mut v = fresh_voxel_with_one_chunk();
    v.rebuild_all_lighting();
    let g = v.lighting.get(ZERO_CHUNK).unwrap();
    // Open chunk -> every cell at sky=15.
    assert_eq!(g.get_sky([0, 0, 0]), 15);
    assert_eq!(g.get_sky([15, 15, 15]), 15);
}

#[test]
fn rebuild_all_lighting_clears_dirty_set() {
    let mut v = fresh_voxel_with_one_chunk();
    v.lighting.mark_dirty([1, 2, 3]);
    v.rebuild_all_lighting();
    // After full re-derive the dirty set is empty (the
    // re-derive already covered every loaded chunk - no point
    // queuing them again).
    let drained = v.lighting.take_dirty();
    assert!(drained.is_empty());
}

#[test]
fn rebuild_all_lighting_idempotent_when_no_chunks_loaded() {
    let mut v = Voxel::new();
    // No chunks at all.
    v.rebuild_all_lighting();
    // No panic, no grids created.
    assert!(v.lighting.get(ZERO_CHUNK).is_none());
}

#[test]
fn rebuild_all_lighting_recovers_after_save_round_trip_shape() {
    // Smoke test for the "save -> reload -> derive" shape.
    // Stamp a torch (with tracker), simulate a save by snapshot-
    // ting the block grid, then simulate load by wiping the
    // light state + calling rebuild_all_lighting. Light values
    // must match what we'd see immediately after the place.
    let mut v = fresh_voxel_with_one_chunk();
    v.world_set_tracked([3, 8, 7], BlockId(TORCH_ID));
    let _ = v.tick_lighting();
    let before = v
        .lighting
        .get(ZERO_CHUNK)
        .unwrap()
        .get_block([3, 8, 7]);
    // Simulate save -> reload: drop the lighting state, keep
    // the block grid.
    v.lighting.clear();
    v.rebuild_all_lighting();
    let after = v
        .lighting
        .get(ZERO_CHUNK)
        .unwrap()
        .get_block([3, 8, 7]);
    assert_eq!(before, after);
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

/// W457: an underground edit must NOT light the cave. The old
/// incremental relight took the dirty chunk + its 3x3x3 halo, and
/// the sky engine started its column walk at the halo's top with
/// sky=15 - as if open sky sat right above the cave (the R31/R34
/// bright pit). tick_lighting now expands every touched column to
/// its full loaded vertical run, so the walk starts above the real
/// roof.
#[test]
fn underground_edit_keeps_cave_dark() {
    let mut v = Voxel::new();
    // Column of three chunks: cave air in y=0..1, a solid stone
    // roof chunk on top (y=2).
    v.world.insert_chunk([0, 0, 0], Chunk::empty());
    v.world.insert_chunk([0, 1, 0], Chunk::empty());
    let mut roof = Chunk::empty();
    for z in 0..infengine_voxel::CHUNK_SIZE {
        for y in 0..infengine_voxel::CHUNK_SIZE {
            for x in 0..infengine_voxel::CHUNK_SIZE {
                roof.set(x, y, z, BlockId(1));
            }
        }
    }
    v.world.insert_chunk([0, 2, 0], roof);
    v.rebuild_all_lighting();
    assert_eq!(
        v.lighting.get(ZERO_CHUNK).unwrap().get_sky([8, 8, 8]),
        0,
        "roofed cave must derive dark",
    );
    // Dig-style edits deep in the cave.
    v.world_set_tracked([8, 8, 8], BlockId(1));
    let _ = v.tick_lighting();
    v.world_set_tracked([8, 8, 8], BlockId::AIR);
    let _ = v.tick_lighting();
    assert_eq!(
        v.lighting.get(ZERO_CHUNK).unwrap().get_sky([8, 9, 8]),
        0,
        "cave must stay dark after an underground edit",
    );
    assert_eq!(
        v.lighting.get([0, 1, 0]).unwrap().get_sky([8, 8, 8]),
        0,
        "upper cave chunk must stay dark too",
    );
}
