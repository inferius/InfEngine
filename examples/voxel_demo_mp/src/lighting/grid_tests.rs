//! Additional `LightGrid` integration tests that exercise the
//! grid against `LightingState` rather than the bare struct.

use super::{LightGrid, LightingState};

#[test]
fn lighting_state_ensure_creates_empty_grid() {
    let mut s = LightingState::new();
    let g = s.ensure([1, 2, 3]);
    assert_eq!(g.get_block([0, 0, 0]), 0);
    assert_eq!(g.get_sky([0, 0, 0]), 0);
}

#[test]
fn lighting_state_get_returns_none_for_missing_chunk() {
    let s = LightingState::new();
    assert!(s.get([1, 2, 3]).is_none());
}

#[test]
fn lighting_state_ensure_reuses_existing_grid() {
    let mut s = LightingState::new();
    s.ensure([0, 0, 0]).set_block([1, 2, 3], 11);
    // A second `ensure` call must reuse the same grid, not mint a
    // fresh empty one over the top.
    let g = s.ensure([0, 0, 0]);
    assert_eq!(g.get_block([1, 2, 3]), 11);
}

#[test]
fn lighting_state_clear_drops_all_grids_and_dirty() {
    let mut s = LightingState::new();
    s.ensure([0, 0, 0]).set_sky([4, 5, 6], 9);
    s.mark_dirty([10, 20, 30]);
    s.clear();
    assert!(s.get([0, 0, 0]).is_none());
    let drained = s.take_dirty();
    assert!(drained.is_empty());
}

#[test]
fn lighting_state_mark_dirty_accumulates_until_taken() {
    let mut s = LightingState::new();
    s.mark_dirty([1, 2, 3]);
    s.mark_dirty([4, 5, 6]);
    s.mark_dirty([1, 2, 3]); // duplicate
    let drained = s.take_dirty();
    assert_eq!(drained.len(), 2);
    assert!(drained.contains(&[1, 2, 3]));
    assert!(drained.contains(&[4, 5, 6]));
    // Re-draining returns nothing.
    let again = s.take_dirty();
    assert!(again.is_empty());
}

#[test]
fn lighting_state_forget_drops_only_target_chunk() {
    let mut s = LightingState::new();
    s.ensure([0, 0, 0]).set_block([0, 0, 0], 7);
    s.ensure([1, 0, 0]).set_block([0, 0, 0], 8);
    s.forget([0, 0, 0]);
    assert!(s.get([0, 0, 0]).is_none());
    assert_eq!(s.get([1, 0, 0]).unwrap().get_block([0, 0, 0]), 8);
}

#[test]
fn light_grid_independent_layers_within_one_grid() {
    // Crank up block at one cell; sky must not bleed across.
    let mut g = LightGrid::empty();
    g.set_block([5, 5, 5], 14);
    g.set_sky([5, 5, 5], 7);
    assert_eq!(g.get_block([5, 5, 5]), 14);
    assert_eq!(g.get_sky([5, 5, 5]), 7);
    g.set_block([5, 5, 5], 0);
    assert_eq!(g.get_sky([5, 5, 5]), 7);
}

#[test]
fn block_emission_lookup_table() {
    use super::block_emission;
    use crate::block::{FIRE_ID, GLOWSTONE_ID, LAVA_ID, TORCH_ID};
    assert_eq!(block_emission(0), 0); // air
    assert_eq!(block_emission(TORCH_ID), 14);
    assert_eq!(block_emission(GLOWSTONE_ID), 15);
    assert_eq!(block_emission(LAVA_ID), 15);
    assert_eq!(block_emission(FIRE_ID), 15);
    // Random non-emitter must return 0.
    assert_eq!(block_emission(1), 0); // stone
    assert_eq!(block_emission(36), 0); // water
}
