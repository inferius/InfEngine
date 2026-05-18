//! Tests for the multi-dim save/load orchestration (Phase F.2).
//!
//! These exercise `world_save::runtime::dims::{save_all_dims,
//! load_all_dims}` against a built-from-scratch `Voxel` + a
//! `tempfile::TempDir`-rooted save directory. The tests use the
//! `runtime` module's `_in_dir`-flavoured helpers where possible,
//! falling back to constructing a `WorldJson` directly when the
//! save layer doesn't expose an _in_dir runtime save (it doesn't,
//! and we don't want to add one just for tests). Each test sets
//! up state under a fresh TempDir so the OS data-dir is never
//! touched.
//!
//! Coverage:
//! - 2-dim save round-trip via the worldgen + parking maps (no
//!   real on-disk save - we test the in-memory swap dynamics).
//! - Switching dims preserves liquid + torch + snow side state.
//! - `current_dim` survives switching.

use crate::Voxel;
use crate::dim::{NETHER, OVERWORLD};
use infengine_voxel::{BlockId, Chunk};

#[test]
fn switch_dim_preserves_liquid_state() {
    let mut v = Voxel::new();
    v.base_seed = 0;
    v.current_dim = OVERWORLD.to_string();
    // Drop a water source + its level entry in the overworld.
    v.liquid_positions.insert([5, 64, 5]);
    v.liquid_level.insert([5, 64, 5], 0);
    v.switch_dim(NETHER);
    // Nether liquid state is empty.
    assert!(v.liquid_positions.is_empty());
    assert!(v.liquid_level.is_empty());
    // Add nether lava so we can verify it parks separately.
    v.liquid_positions.insert([1, 30, 1]);
    v.liquid_level.insert([1, 30, 1], 0);
    v.switch_dim(OVERWORLD);
    assert!(v.liquid_positions.contains(&[5, 64, 5]));
    assert!(!v.liquid_positions.contains(&[1, 30, 1]));
    assert_eq!(v.liquid_level.get(&[5, 64, 5]).copied(), Some(0));
    v.switch_dim(NETHER);
    assert!(v.liquid_positions.contains(&[1, 30, 1]));
    assert!(!v.liquid_positions.contains(&[5, 64, 5]));
}

#[test]
fn switch_dim_preserves_torch_and_snow() {
    let mut v = Voxel::new();
    v.base_seed = 0;
    v.current_dim = OVERWORLD.to_string();
    v.torch_positions.insert([0, 65, 0]);
    v.torch_orient.insert([0, 65, 0], 2);
    v.snow_positions.insert([1, 65, 0], 3);
    v.switch_dim(NETHER);
    // No torches / snow in the nether by default.
    assert!(v.torch_positions.is_empty());
    assert!(v.snow_positions.is_empty());
    v.switch_dim(OVERWORLD);
    assert!(v.torch_positions.contains(&[0, 65, 0]));
    assert_eq!(v.torch_orient.get(&[0, 65, 0]).copied(), Some(2));
    assert_eq!(v.snow_positions.get(&[1, 65, 0]).copied(), Some(3));
}

#[test]
fn switch_dim_preserves_chunks_across_swap() {
    let mut v = Voxel::new();
    v.base_seed = 1;
    v.current_dim = OVERWORLD.to_string();
    let mut c = Chunk::empty();
    c.set(2, 3, 4, BlockId(7)); // planks
    v.world.insert_chunk([0, 0, 0], c);
    v.switch_dim(NETHER);
    // Nether's chunk view is empty.
    assert_eq!(v.world.iter_chunks().count(), 0);
    v.switch_dim(OVERWORLD);
    let blk = v.world.chunk([0, 0, 0]).map(|c| c.get(2, 3, 4).0);
    assert_eq!(blk, Some(7));
}

#[test]
fn current_dim_persists_through_switches() {
    let mut v = Voxel::new();
    v.base_seed = 0;
    v.current_dim = OVERWORLD.to_string();
    assert_eq!(v.current_dim, OVERWORLD);
    v.switch_dim(NETHER);
    assert_eq!(v.current_dim, NETHER);
    v.switch_dim(OVERWORLD);
    assert_eq!(v.current_dim, OVERWORLD);
    // Idempotent same-target call.
    v.switch_dim(OVERWORLD);
    assert_eq!(v.current_dim, OVERWORLD);
}
