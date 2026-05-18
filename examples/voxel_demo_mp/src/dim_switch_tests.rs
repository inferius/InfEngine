//! Tests for `Voxel::switch_dim` (Phase F.1).
//!
//! Lives in its own file so the main.rs body stays under the
//! file-size policy. Mounted via `#[cfg(test)] #[path]` from
//! main.rs.

use crate::Voxel;
use crate::dim::{NETHER, OVERWORLD};
use crate::worldgen::{self, WORLDGEN, WorldGen, dim_seed};
use infengine_voxel::{BlockId, Chunk, ChunkCoord};

// Tests touch the WORLD_SEED + WORLDGEN atomics; share the
// worldgen test-lock so we don't race other generator tests.
fn lock_gen() -> std::sync::MutexGuard<'static, ()> {
    worldgen::TEST_GEN_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Helper: build a Voxel + place a single non-air block in a
/// known chunk so we can assert preservation across switches.
fn build_voxel_with_stone(coord: ChunkCoord, x: usize, y: usize, z: usize) -> Voxel {
    let mut v = Voxel::new();
    v.base_seed = 1234;
    v.current_dim = OVERWORLD.to_string();
    let mut chunk = Chunk::empty();
    chunk.set(x, y, z, BlockId(1));
    v.world.insert_chunk(coord, chunk);
    v
}

#[test]
fn switch_dim_is_idempotent_when_target_matches() {
    let _g = lock_gen();
    let mut v = build_voxel_with_stone([0, 0, 0], 1, 1, 1);
    let before = v
        .world
        .chunk([0, 0, 0])
        .map(|c| c.get(1, 1, 1).0);
    v.switch_dim(OVERWORLD);
    let after = v
        .world
        .chunk([0, 0, 0])
        .map(|c| c.get(1, 1, 1).0);
    assert_eq!(before, after);
    assert_eq!(v.current_dim, OVERWORLD);
}

#[test]
fn switch_dim_preserves_overworld_voxel_state() {
    let _g = lock_gen();
    let mut v = build_voxel_with_stone([2, 0, 3], 4, 5, 6);
    // Bounce overworld -> nether -> overworld and verify the
    // stone block is back where we left it.
    v.switch_dim(NETHER);
    assert_eq!(v.current_dim, NETHER);
    assert!(v.world.chunk([2, 0, 3]).is_none(), "nether should be empty");
    v.switch_dim(OVERWORLD);
    assert_eq!(v.current_dim, OVERWORLD);
    let blk = v.world.chunk([2, 0, 3]).map(|c| c.get(4, 5, 6).0);
    assert_eq!(blk, Some(1));
}

#[test]
fn switch_dim_updates_world_seed_atomic() {
    let _g = lock_gen();
    let mut v = Voxel::new();
    v.base_seed = 0xDEAD_BEEF;
    v.current_dim = OVERWORLD.to_string();
    // Force a known starting seed so we can detect the swap.
    worldgen::set_world_seed(0);
    v.switch_dim(NETHER);
    assert_eq!(worldgen::world_seed(), dim_seed(0xDEAD_BEEF, NETHER));
    v.switch_dim(OVERWORLD);
    assert_eq!(worldgen::world_seed(), dim_seed(0xDEAD_BEEF, OVERWORLD));
}

#[test]
fn switch_dim_updates_worldgen_atomic_from_map() {
    let _g = lock_gen();
    let mut v = Voxel::new();
    v.base_seed = 42;
    v.current_dim = OVERWORLD.to_string();
    v.dim_worldgens
        .insert(OVERWORLD.to_string(), WorldGen::Realistic);
    v.dim_worldgens
        .insert(NETHER.to_string(), WorldGen::Nether);
    // Park the WORLDGEN atomic at Classic; switch_dim should
    // rewrite it from the per-dim map.
    WORLDGEN.store(WorldGen::Classic as u8, std::sync::atomic::Ordering::Relaxed);
    v.switch_dim(NETHER);
    assert_eq!(WorldGen::current(), WorldGen::Nether);
    v.switch_dim(OVERWORLD);
    assert_eq!(WorldGen::current(), WorldGen::Realistic);
}

#[test]
fn switch_dim_preserves_player_pose_and_inventory() {
    let _g = lock_gen();
    let mut v = Voxel::new();
    v.base_seed = 7;
    v.current_dim = OVERWORLD.to_string();
    v.player_pos = glam::Vec3::new(12.5, 64.0, -7.5);
    v.player_yaw = 1.234;
    v.player_pitch = -0.42;
    v.player_health = 13.0;
    v.inventory[0] = Some((BlockId(1), 32));
    v.inventory[5] = Some((BlockId(7), 64));
    v.switch_dim(NETHER);
    // Pose + inventory must NOT swap with the dim. The MC
    // contract has the player carry their stuff through the
    // portal.
    assert!((v.player_pos.x - 12.5).abs() < 1e-3);
    assert!((v.player_pos.y - 64.0).abs() < 1e-3);
    assert!((v.player_pos.z + 7.5).abs() < 1e-3);
    assert!((v.player_yaw - 1.234).abs() < 1e-3);
    assert!((v.player_pitch + 0.42).abs() < 1e-3);
    assert_eq!(v.player_health, 13.0);
    assert_eq!(v.inventory[0], Some((BlockId(1), 32)));
    assert_eq!(v.inventory[5], Some((BlockId(7), 64)));
}

#[test]
fn switch_dim_swaps_chests_per_dim() {
    let _g = lock_gen();
    let mut v = Voxel::new();
    v.base_seed = 99;
    v.current_dim = OVERWORLD.to_string();
    // Drop a chest in the overworld.
    let mut chest = crate::container::Container::with_size(27);
    chest.set_slot(0, Some((1, 5)));
    v.chests.insert([10, 20, 30], chest);
    // Switch to nether: the chest map should be empty there.
    v.switch_dim(NETHER);
    assert!(
        v.chests.is_empty(),
        "nether's chest map should start empty"
    );
    // Add a nether chest at a different pos so we can verify
    // it parks separately when we go back.
    let mut nether_chest = crate::container::Container::with_size(27);
    nether_chest.set_slot(0, Some((44, 7))); // 44 = netherrack
    v.chests.insert([1, 2, 3], nether_chest);
    // Bounce back to overworld: our original chest should be
    // back, the nether chest should be gone (parked).
    v.switch_dim(OVERWORLD);
    assert!(v.chests.contains_key(&[10, 20, 30]));
    assert!(!v.chests.contains_key(&[1, 2, 3]));
    assert_eq!(v.chests.get(&[10, 20, 30]).unwrap().slot(0), Some((1, 5)));
    // Round-trip back to nether: the netherrack chest survives.
    v.switch_dim(NETHER);
    assert!(v.chests.contains_key(&[1, 2, 3]));
    assert_eq!(v.chests.get(&[1, 2, 3]).unwrap().slot(0), Some((44, 7)));
}

#[test]
fn switch_dim_swaps_mobs_per_dim() {
    let _g = lock_gen();
    let mut v = Voxel::new();
    v.base_seed = 17;
    v.current_dim = OVERWORLD.to_string();
    // Drop a mob in the overworld.
    v.mobs.push(crate::mob_ai::MobState {
        id: 1,
        kind: 0,
        pos: [0.0, 32.0, 0.0],
        target: [0.0, 32.0, 0.0],
        yaw: 0.0,
        retarget_at: 0.0,
        health: 10.0,
        dying: 0.0,
        attack_cd: 0.0,
    });
    v.switch_dim(NETHER);
    assert!(v.mobs.is_empty(), "nether starts with no mobs");
    v.switch_dim(OVERWORLD);
    assert_eq!(v.mobs.len(), 1);
    assert_eq!(v.mobs[0].id, 1);
}
