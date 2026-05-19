//! BFS engine tests for the block-light propagator.
//!
//! Each test builds a tiny `World` of one (or a few) 16^3 chunks,
//! flips the relevant cells via `set`, then runs the propagator
//! over the chunk set and asserts on the resulting `LightGrid`
//! contents.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use super::block_engine::propagate_block_light;
use super::LightingState;
use crate::block::{FIRE_ID, GLOWSTONE_ID, LAVA_ID, TORCH_ID};

const ZERO_CHUNK: ChunkCoord = [0, 0, 0];

fn empty_world_with_one_chunk() -> World {
    let mut w = World::new();
    w.insert_chunk(ZERO_CHUNK, Chunk::empty());
    w
}

fn get_block_local(state: &LightingState, cc: ChunkCoord, lc: [u32; 3]) -> u8 {
    state.get(cc).map(|g| g.get_block(lc)).unwrap_or(0)
}

#[test]
fn single_torch_lights_its_own_cell_at_14() {
    let mut w = empty_world_with_one_chunk();
    w.set([5, 5, 5], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [5, 5, 5]), 14);
}

#[test]
fn single_torch_neighbour_one_step_away_is_13() {
    let mut w = empty_world_with_one_chunk();
    w.set([5, 5, 5], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [6, 5, 5]), 13);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [5, 6, 5]), 13);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [4, 5, 5]), 13);
}

#[test]
fn single_torch_decays_per_step_through_air() {
    let mut w = empty_world_with_one_chunk();
    w.set([0, 5, 5], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    // Along +X axis, expect 14, 13, 12, ..., down to 1 at the
    // far edge (distance 13).
    for d in 0..=13u32 {
        let expected = 14u8.saturating_sub(d as u8);
        assert_eq!(
            get_block_local(&s, ZERO_CHUNK, [d, 5, 5]),
            expected,
            "expected {} at distance {}",
            expected,
            d
        );
    }
    // Beyond distance 14 the light dies (14 - 14 = 0).
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [14, 5, 5]), 0);
}

#[test]
fn glowstone_emits_15_one_step_brighter_than_torch() {
    let mut w = empty_world_with_one_chunk();
    w.set([5, 5, 5], BlockId(GLOWSTONE_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [5, 5, 5]), 15);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [6, 5, 5]), 14);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [10, 5, 5]), 10);
}

#[test]
fn lava_and_fire_each_emit_15() {
    for id in [LAVA_ID, FIRE_ID] {
        let mut w = empty_world_with_one_chunk();
        w.set([8, 8, 8], BlockId(id));
        let mut s = LightingState::new();
        propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
        assert_eq!(get_block_local(&s, ZERO_CHUNK, [8, 8, 8]), 15);
    }
}

#[test]
fn two_torches_max_wins_at_overlap() {
    let mut w = empty_world_with_one_chunk();
    w.set([2, 5, 5], BlockId(TORCH_ID));
    w.set([12, 5, 5], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    // Cell at (7, 5, 5) sits 5 away from each torch -> level 9
    // from either, max stays 9.
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [7, 5, 5]), 9);
}

#[test]
fn opaque_wall_blocks_block_light_propagation() {
    let mut w = empty_world_with_one_chunk();
    // Box the torch in a 1-block-thick opaque shell so the BFS
    // has no path to a far-side cell. The torch sits at (4, 8,
    // 8); stone covers x=5 at y in 7..=9 and z in 7..=9, plus a
    // y-cap at y=10 and a floor at y=6 to seal the box. Light
    // must not exit the box.
    for y in 6..=10i32 {
        for z in 6..=10i32 {
            w.set([5, y, z], BlockId(1)); // +X wall
        }
    }
    // Ceiling + floor over the open region near the torch.
    for x in 3..=5i32 {
        for z in 6..=10i32 {
            w.set([x, 6, z], BlockId(1)); // floor
            w.set([x, 10, z], BlockId(1)); // ceiling
        }
    }
    // Z walls (front + back) and -X wall.
    for x in 3..=5i32 {
        for y in 6..=10i32 {
            w.set([x, y, 6], BlockId(1));
            w.set([x, y, 10], BlockId(1));
        }
    }
    for y in 6..=10i32 {
        for z in 6..=10i32 {
            w.set([3, y, z], BlockId(1));
        }
    }
    // Torch inside the sealed box.
    w.set([4, 8, 8], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    // Inside the box the torch's own cell + the neighbouring air
    // get lit.
    assert!(get_block_local(&s, ZERO_CHUNK, [4, 8, 8]) > 0);
    // The opaque shell cells stay at 0 (the BFS writes the
    // pushed value only if it beats the existing one, and the
    // path lands at level - 15 = 0).
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [5, 8, 8]), 0);
    // Outside the sealed shell the BFS has no path -> 0.
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [6, 8, 8]), 0);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [4, 11, 8]), 0);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [4, 5, 8]), 0);
}

#[test]
fn water_decays_block_light_per_cell_like_air() {
    let mut w = empty_world_with_one_chunk();
    // Fill a 5x1x1 row with water, drop a torch one cell to the
    // left so the BFS walks through water.
    for x in 6..=10i32 {
        w.set([x, 5, 5], BlockId(36)); // water
    }
    w.set([5, 5, 5], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    // Water is transparent for the block-light decay (per L
    // spec). At distance 5 (x=10) we expect 14 - 5 = 9.
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [10, 5, 5]), 9);
}

#[test]
fn placing_then_removing_torch_zeros_the_lit_region() {
    let mut w = empty_world_with_one_chunk();
    w.set([5, 5, 5], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    assert!(get_block_local(&s, ZERO_CHUNK, [5, 5, 5]) > 0);
    // Remove the torch and re-flood.
    w.set([5, 5, 5], BlockId::AIR);
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    // Without an emitter the entire chunk goes back to 0.
    for x in 0..16u32 {
        for y in 0..16u32 {
            for z in 0..16u32 {
                assert_eq!(get_block_local(&s, ZERO_CHUNK, [x, y, z]), 0);
            }
        }
    }
}

#[test]
fn re_flood_block_layer_is_idempotent() {
    let mut w = empty_world_with_one_chunk();
    w.set([5, 5, 5], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    let after_first = get_block_local(&s, ZERO_CHUNK, [5, 5, 5]);
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    let after_second = get_block_local(&s, ZERO_CHUNK, [5, 5, 5]);
    assert_eq!(after_first, after_second);
}

#[test]
fn cross_chunk_propagation_respects_affected_set() {
    // Place a torch at the boundary of chunk [0,0,0] (x=15) and
    // ensure neighbour chunk [1,0,0] only receives light when it
    // is in the affected set.
    let mut w = World::new();
    w.insert_chunk([0, 0, 0], Chunk::empty());
    w.insert_chunk([1, 0, 0], Chunk::empty());
    w.set([15, 5, 5], BlockId(TORCH_ID));
    // With ONLY chunk [0,0,0] in the affected set the neighbour
    // chunk's grid stays at 0.
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[[0, 0, 0]]);
    assert_eq!(get_block_local(&s, [1, 0, 0], [0, 5, 5]), 0);
    // With BOTH chunks in the affected set the neighbour gets
    // a step of decay.
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[[0, 0, 0], [1, 0, 0]]);
    assert_eq!(get_block_local(&s, [1, 0, 0], [0, 5, 5]), 13);
}

#[test]
fn empty_world_no_emitter_block_layer_stays_zero() {
    let w = empty_world_with_one_chunk();
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    for x in 0..16u32 {
        for y in 0..16u32 {
            for z in 0..16u32 {
                assert_eq!(get_block_local(&s, ZERO_CHUNK, [x, y, z]), 0);
            }
        }
    }
}

#[test]
fn diagonal_neighbour_two_axis_decay_is_distance_squared_friendly() {
    // Manhattan distance (axis-aligned BFS) gives a 14-d falloff
    // for a torch. At a diagonal-2 cell (1, 1, 0) from the torch
    // the Manhattan distance is 2 so level = 12. The BFS doesn't
    // shortcut diagonals.
    let mut w = empty_world_with_one_chunk();
    w.set([5, 5, 5], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [6, 6, 5]), 12);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [7, 7, 5]), 10);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [6, 6, 6]), 11);
}

#[test]
fn torch_block_emits_through_its_own_neighbour_face() {
    // The emitter cell is treated as TRANSPARENT (torch
    // F_DEDICATED_RENDER block) so its own neighbour faces
    // decay by 1 per step, NOT by 15.
    let mut w = empty_world_with_one_chunk();
    w.set([0, 8, 8], BlockId(TORCH_ID));
    let mut s = LightingState::new();
    propagate_block_light(&w, &mut s, &[ZERO_CHUNK]);
    // 14 at torch cell, 13 at (1, 8, 8) - no extra penalty for
    // leaving the emitter's own voxel.
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [0, 8, 8]), 14);
    assert_eq!(get_block_local(&s, ZERO_CHUNK, [1, 8, 8]), 13);
}

#[test]
fn step_decay_air_is_one() {
    use super::block_engine::{step_decay, TRANSPARENT_DECAY};
    assert_eq!(step_decay(BlockId::AIR), TRANSPARENT_DECAY);
}

#[test]
fn step_decay_opaque_is_fifteen() {
    use super::block_engine::{step_decay, OPAQUE_DECAY};
    // Stone (id=1) is opaque.
    assert_eq!(step_decay(BlockId(1)), OPAQUE_DECAY);
}

#[test]
fn step_decay_transparent_voxel_is_one() {
    use super::block_engine::{step_decay, TRANSPARENT_DECAY};
    // Glass (id=9) is transparent.
    assert_eq!(step_decay(BlockId(9)), TRANSPARENT_DECAY);
    // Water (id=36) is transparent.
    assert_eq!(step_decay(BlockId(36)), TRANSPARENT_DECAY);
    // Leaves (id=6) is transparent (alpha-cutout). MC's leaves
    // are "non-occluding" for block-light decay (they propagate
    // with a -1 decay just like air).
    assert_eq!(step_decay(BlockId(6)), TRANSPARENT_DECAY);
}
