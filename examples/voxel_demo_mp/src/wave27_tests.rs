//! W27 tests: loader counter fix + lighting BFS off main thread.
//!
//! W27.1: `build_world` target matches the live `view_distance`-based count.
//! W27.2: `ChunkSnapshot` BFS produces the same values as the live `World` BFS.
//! W27.3: `tick_lighting` without a worker pool (synchronous fallback) still
//!         returns affected chunks and computes correct light values.

use std::sync::Arc;
use infengine_voxel::{BlockId, Chunk, CHUNK_SIZE};

use crate::Voxel;
use crate::block::TORCH_ID;
use crate::lighting::{ChunkSnapshot, LightingState};
use crate::lighting::block_engine::propagate_block_light;
use crate::lighting::sky_engine::propagate_sky_light;

// ---------------------------------------------------------------------------
// W27.1 - Loader counter tests
// ---------------------------------------------------------------------------

/// After `build_world`, the enqueued count equals (2*view_distance+1)^2 *
/// visible_y_slices where visible_y_slices comes from vertical_chunk_range.
/// W231: that range is now the FULL vertical world (16 slices, MC-style
/// full columns) regardless of player height.
#[test]
fn build_world_target_matches_view_distance_footprint() {
    let mut v = Voxel::new();
    // view_distance defaults to VIEW_DISTANCE_CHUNKS (=4) set in Settings.
    // Clamp mirrors what build_world does: clamp(2..=32).
    let vd = v.settings.view_distance.clamp(2, crate::VIEW_DISTANCE_MAX as u8) as i32;
    let (cy_min, cy_max) = v.vertical_chunk_range();
    let y_slices = (cy_max - cy_min + 1) as u32;
    let expected: u32 = ((2 * vd + 1) * (2 * vd + 1)) as u32 * y_slices;
    let got = v.build_world();
    assert_eq!(
        got, expected,
        "build_world enqueued {got} chunks but expected {expected} \
         (view_distance={vd}, y_slices={y_slices})"
    );
}

/// `build_world` returns a non-zero target even for the minimum view_distance.
#[test]
fn build_world_nonzero_target() {
    let mut v = Voxel::new();
    // Force the minimum (2) so the test is stable regardless of defaults.
    v.settings.view_distance = 2;
    let target = v.build_world();
    assert!(target > 0, "build_world must return a positive target");
}

/// Two consecutive `build_world` calls return the same target (the function
/// clears stale queues and re-enqueues from scratch each time).
#[test]
fn build_world_idempotent_target() {
    let mut v = Voxel::new();
    let a = v.build_world();
    let b = v.build_world();
    assert_eq!(a, b, "build_world must return the same target on repeated calls");
}

// ---------------------------------------------------------------------------
// W27.2 - ChunkSnapshot BFS produces same results as World BFS
// ---------------------------------------------------------------------------

/// Block-light BFS over a `ChunkSnapshot` gives identical results to the
/// same BFS over a live `World` - same seed chunk, same torch position.
#[test]
fn chunk_snapshot_block_bfs_matches_world_bfs() {
    let coord = [0i32, 0, 0];
    let torch_lc = [5u32, 5, 5];
    let cs = CHUNK_SIZE as i32;

    // Build a one-chunk world with a torch.
    let mut chunk = Chunk::empty();
    chunk.set(
        torch_lc[0] as usize,
        torch_lc[1] as usize,
        torch_lc[2] as usize,
        BlockId(TORCH_ID),
    );

    // --- BFS via live World ---
    let mut world = infengine_voxel::World::new();
    world.insert_chunk(coord, chunk.clone());
    let mut state_world = LightingState::new();
    propagate_block_light(&world, &mut state_world, &[coord]);
    let grid_world = state_world.get(coord).expect("world grid").clone();

    // --- BFS via ChunkSnapshot ---
    let snap = ChunkSnapshot::from_iter([(coord, chunk)]);
    let mut state_snap = LightingState::new();
    propagate_block_light(&snap, &mut state_snap, &[coord]);
    let grid_snap = state_snap.get(coord).expect("snapshot grid").clone();

    // Both must agree for every cell in the chunk.
    for z in 0..CHUNK_SIZE as u32 {
        for y in 0..CHUNK_SIZE as u32 {
            for x in 0..CHUNK_SIZE as u32 {
                let lc = [x, y, z];
                assert_eq!(
                    grid_world.get_block(lc),
                    grid_snap.get_block(lc),
                    "block-light mismatch at local {:?}", lc
                );
            }
        }
    }

    // Spot-check the torch cell and its immediate +X neighbour.
    assert_eq!(grid_snap.get_block(torch_lc), 14, "torch self = 14");
    let px_lc = [torch_lc[0] + 1, torch_lc[1], torch_lc[2]];
    assert_eq!(grid_snap.get_block(px_lc), 13, "torch +X neighbour = 13");

    // Verify that a torch at world coord [5,5,5] maps correctly.
    let torch_world = [
        coord[0] * cs + torch_lc[0] as i32,
        coord[1] * cs + torch_lc[1] as i32,
        coord[2] * cs + torch_lc[2] as i32,
    ];
    // LightingWorldAccess for snapshot should return the torch block.
    use crate::lighting::LightingWorldAccess;
    assert_eq!(snap.get_block(torch_world), BlockId(TORCH_ID));
}

/// Sky-light BFS over a `ChunkSnapshot` gives identical results to the
/// same BFS over a live `World` for an open (all-transparent) chunk.
#[test]
fn chunk_snapshot_sky_bfs_matches_world_bfs_open_chunk() {
    let coord = [0i32, 0, 0];

    // Empty chunk = fully transparent -> sky=15 everywhere.
    let chunk = Chunk::empty();

    // --- BFS via live World ---
    let mut world = infengine_voxel::World::new();
    world.insert_chunk(coord, chunk.clone());
    let mut state_world = LightingState::new();
    propagate_sky_light(&world, &mut state_world, &[coord]);
    let grid_world = state_world.get(coord).expect("world grid").clone();

    // --- BFS via ChunkSnapshot ---
    let snap = ChunkSnapshot::from_iter([(coord, chunk)]);
    let mut state_snap = LightingState::new();
    propagate_sky_light(&snap, &mut state_snap, &[coord]);
    let grid_snap = state_snap.get(coord).expect("snapshot grid").clone();

    for z in 0..CHUNK_SIZE as u32 {
        for y in 0..CHUNK_SIZE as u32 {
            for x in 0..CHUNK_SIZE as u32 {
                let lc = [x, y, z];
                assert_eq!(
                    grid_world.get_sky(lc),
                    grid_snap.get_sky(lc),
                    "sky-light mismatch at local {:?}", lc
                );
                assert_eq!(grid_snap.get_sky(lc), 15, "open chunk -> sky=15 at {:?}", lc);
            }
        }
    }
}

/// `ChunkSnapshot::get_block` returns air for coordinates outside any
/// chunk in the snapshot.
#[test]
fn chunk_snapshot_out_of_bounds_returns_air() {
    use crate::lighting::LightingWorldAccess;

    let snap = ChunkSnapshot::from_iter(std::iter::empty());
    assert_eq!(
        snap.get_block([999, 999, 999]),
        BlockId::AIR,
        "missing chunk -> air"
    );
    assert_eq!(
        snap.get_block([0, 0, 0]),
        BlockId::AIR,
        "missing chunk [0,0,0] -> air"
    );
}

// ---------------------------------------------------------------------------
// W27.3 - tick_lighting synchronous fallback (no worker pool)
// ---------------------------------------------------------------------------

/// Without a worker pool, `tick_lighting` still runs the BFS synchronously
/// and returns the affected chunks, just as before W27.
#[test]
fn tick_lighting_sync_fallback_returns_affected_and_propagates() {
    // Voxel::new() leaves workers = None, so tick_lighting takes the
    // synchronous fallback path.
    let mut v = Voxel::new();
    v.world.insert_chunk([0, 0, 0], Chunk::empty());

    v.world_set_tracked([5, 5, 5], BlockId(TORCH_ID));
    let affected = v.tick_lighting();

    // Must return the affected chunk in the fallback path.
    assert!(
        affected.contains(&[0, 0, 0]),
        "tick_lighting sync fallback must return [0,0,0]"
    );
    // Light value must have been propagated.
    let g = v.lighting.get([0, 0, 0]).expect("grid populated");
    assert_eq!(g.get_block([5, 5, 5]), 14, "torch cell = 14");
    assert_eq!(g.get_block([6, 5, 5]), 13, "torch +X = 13");
}

/// `tick_lighting` with no dirty set returns an empty vec (fast path).
#[test]
fn tick_lighting_empty_dirty_returns_empty() {
    let mut v = Voxel::new();
    v.world.insert_chunk([0, 0, 0], Chunk::empty());
    let affected = v.tick_lighting();
    assert!(affected.is_empty(), "no dirty set -> empty return");
}

/// `LightingState::set_grid` replaces an existing grid with the new Arc.
#[test]
fn lighting_state_set_grid_replaces_existing() {
    let mut state = LightingState::new();
    let cc = [0i32, 0, 0];

    // Insert initial grid.
    state.ensure(cc).set_block([0, 0, 0], 5);
    assert_eq!(state.get(cc).unwrap().get_block([0, 0, 0]), 5);

    // Replace via set_grid.
    let mut fresh = crate::lighting::LightGrid::empty();
    fresh.set_block([0, 0, 0], 12);
    state.set_grid(cc, Arc::new(fresh));
    assert_eq!(
        state.get(cc).unwrap().get_block([0, 0, 0]),
        12,
        "set_grid must replace the existing grid"
    );
}

/// `LightingState::with_grids` pre-seeds the state with existing Arcs;
/// subsequent `get` returns them without copying.
#[test]
fn lighting_state_with_grids_seeds_state() {
    use std::collections::HashMap;

    let cc = [1i32, 2, 3];
    let mut g = crate::lighting::LightGrid::empty();
    g.set_block([0, 0, 0], 9);
    let arc = Arc::new(g);

    let mut map = HashMap::new();
    map.insert(cc, Arc::clone(&arc));

    let state = LightingState::with_grids(map);
    let got = state.get(cc).expect("seeded grid present");
    assert_eq!(got.get_block([0, 0, 0]), 9, "seeded value preserved");
}
