//! Tests for `Voxel::switch_dim` (Phase F.1).
//!
//! Lives in its own file so the main.rs body stays under the
//! file-size policy. Mounted via `#[cfg(test)] #[path]` from
//! main.rs.

use crate::Voxel;
use crate::dim::{NETHER, OVERWORLD};
use crate::worldgen::{self, WorldGen, dim_seed};
use infengine_voxel::{BlockId, Chunk, ChunkCoord};

// W5.1b: the worldgen globals are gone, so these tests no longer
// need a serialising mutex. `switch_dim` operates purely on
// `Voxel` fields; each test is naturally isolated.

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
fn switch_dim_preserves_base_seed_across_swaps() {
    // W5.1b: switch_dim must not touch voxel.base_seed - it's
    // the world's invariant. The per-dim mix happens at
    // `current_worldgen_ctx()` build time.
    let mut v = Voxel::new();
    v.base_seed = 0xDEAD_BEEF;
    v.current_dim = OVERWORLD.to_string();
    let before = v.base_seed;
    v.switch_dim(NETHER);
    assert_eq!(v.base_seed, before, "switch_dim must not touch base_seed");
    v.switch_dim(OVERWORLD);
    assert_eq!(v.base_seed, before, "switch_dim must not touch base_seed");
}

#[test]
fn switch_dim_preserves_dim_worldgens_table() {
    // W5.1b: switch_dim must not rewrite the `dim_worldgens`
    // entries it consults. The map is the world's per-dim
    // profile table; only `/worldgen` should mutate it.
    let mut v = Voxel::new();
    v.base_seed = 42;
    v.current_dim = OVERWORLD.to_string();
    v.dim_worldgens
        .insert(OVERWORLD.to_string(), WorldGen::Realistic);
    v.dim_worldgens
        .insert(NETHER.to_string(), WorldGen::Nether);
    v.switch_dim(NETHER);
    assert_eq!(
        v.dim_worldgens.get(OVERWORLD).copied(),
        Some(WorldGen::Realistic),
    );
    assert_eq!(
        v.dim_worldgens.get(NETHER).copied(),
        Some(WorldGen::Nether),
    );
    v.switch_dim(OVERWORLD);
    assert_eq!(
        v.dim_worldgens.get(OVERWORLD).copied(),
        Some(WorldGen::Realistic),
    );
    assert_eq!(
        v.dim_worldgens.get(NETHER).copied(),
        Some(WorldGen::Nether),
    );
}

#[test]
fn switch_dim_updates_current_dim_for_ctx_consumers() {
    // F.2e replaces the atomic-write contract with this: after
    // `switch_dim(target)`, `current_worldgen_ctx()` returns a
    // ctx that describes the TARGET dim. Pure-fn callers (the
    // worker pool) build their jobs from this and so see the
    // right seed + profile.
let mut v = Voxel::new();
    v.base_seed = 0xDEAD_BEEF;
    v.current_dim = OVERWORLD.to_string();
    v.dim_worldgens
        .insert(OVERWORLD.to_string(), WorldGen::Classic);
    v.dim_worldgens
        .insert(NETHER.to_string(), WorldGen::Nether);
    v.switch_dim(NETHER);
    let ctx = v.current_worldgen_ctx();
    assert_eq!(ctx.dim, NETHER);
    assert_eq!(ctx.profile, WorldGen::Nether);
    assert_eq!(ctx.effective_seed(), dim_seed(0xDEAD_BEEF, NETHER));
    v.switch_dim(OVERWORLD);
    let ctx = v.current_worldgen_ctx();
    assert_eq!(ctx.dim, OVERWORLD);
    assert_eq!(ctx.profile, WorldGen::Classic);
    assert_eq!(ctx.effective_seed(), dim_seed(0xDEAD_BEEF, OVERWORLD));
}

#[test]
fn two_dim_round_trip_byte_identical_overworld_chunks() {
    // F.2e integration: gen 3 overworld chunks, jump to nether
    // and back, re-gen the same 3 overworld chunks. They must be
    // byte-identical the second time around.
let mut v = Voxel::new();
    v.base_seed = 0xDEAD_BEEF;
    v.current_dim = OVERWORLD.to_string();
    v.dim_worldgens
        .insert(OVERWORLD.to_string(), WorldGen::Classic);
    v.dim_worldgens
        .insert(NETHER.to_string(), WorldGen::Nether);

    let coords: [ChunkCoord; 3] = [[0, 0, 0], [1, 0, 0], [-1, 0, -1]];
    let ctx_ow = v.current_worldgen_ctx();
    let before: Vec<Chunk> = coords
        .iter()
        .map(|c| worldgen::generate_chunk_at_ctx(*c, &ctx_ow))
        .collect();

    // Jump to nether, gen some chunks, jump back.
    v.switch_dim(NETHER);
    let ctx_nt = v.current_worldgen_ctx();
    let _ = worldgen::generate_chunk_at_ctx([0, 0, 0], &ctx_nt);
    let _ = worldgen::generate_chunk_at_ctx([1, 0, 0], &ctx_nt);
    v.switch_dim(OVERWORLD);

    let ctx_ow2 = v.current_worldgen_ctx();
    let after: Vec<Chunk> = coords
        .iter()
        .map(|c| worldgen::generate_chunk_at_ctx(*c, &ctx_ow2))
        .collect();

    for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    assert_eq!(
                        b.get(x, y, z).0,
                        a.get(x, y, z).0,
                        "chunk {i} differs at ({x},{y},{z})",
                    );
                }
            }
        }
    }
}

#[test]
fn switch_dim_preserves_player_pose_and_inventory() {
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
fn f2f_chest_plus_chunk_survive_full_dim_round_trip() {
    // F.2f integration: drop a chest with a unique block id in
    // the overworld, also place a marker block in a chunk, then
    // switch to nether, generate a chunk there, switch back.
    // The chest + marker + the chunk's terrain hash must all
    // match what we left. Mirrors the user-reported bug exactly:
    // the player walks away through a dim swap and the world
    // they come back to must be identical.
let mut v = Voxel::new();
    v.base_seed = 0xCAFE_F00D;
    v.current_dim = OVERWORLD.to_string();
    v.dim_worldgens
        .insert(OVERWORLD.to_string(), WorldGen::Classic);
    v.dim_worldgens
        .insert(NETHER.to_string(), WorldGen::Nether);

    // Generate three overworld chunks via the ctx path + capture
    // a fold hash of each. These represent saved terrain a
    // player would walk away from.
    let coords: [ChunkCoord; 3] = [[0, 0, 0], [2, 0, -1], [-3, 0, 2]];
    let ctx_ow_before = v.current_worldgen_ctx();
    let folds_before: Vec<u64> = coords
        .iter()
        .map(|c| {
            let chunk = worldgen::generate_chunk_at_ctx(*c, &ctx_ow_before);
            fold_chunk(&chunk)
        })
        .collect();

    // Drop a chest with a unique block-id payload.
    let chest_pos: [i32; 3] = [10, 20, 30];
    let mut chest = crate::container::Container::with_size(27);
    chest.set_slot(0, Some((42, 17)));
    chest.set_slot(5, Some((99, 3)));
    v.chests.insert(chest_pos, chest);

    // Walk away to the nether: switch dim, generate a chunk
    // there (forces ctx swap + worker path), then walk back.
    v.switch_dim(NETHER);
    assert!(v.chests.is_empty(), "nether chest map starts empty");
    let ctx_nt = v.current_worldgen_ctx();
    let _ = worldgen::generate_chunk_at_ctx([0, 0, 0], &ctx_nt);
    let _ = worldgen::generate_chunk_at_ctx([1, 0, 0], &ctx_nt);
    v.switch_dim(OVERWORLD);

    // Chest should be back, content intact.
    assert!(v.chests.contains_key(&chest_pos));
    let restored = v.chests.get(&chest_pos).unwrap();
    assert_eq!(restored.slot(0), Some((42, 17)));
    assert_eq!(restored.slot(5), Some((99, 3)));

    // Terrain regen must match the original gen byte-for-byte.
    // This is the regression that broke before F.2 - the user
    // saw mountains-turned-to-snow because the noise pipeline
    // sampled fresh atomics on the second gen.
    let ctx_ow_after = v.current_worldgen_ctx();
    let folds_after: Vec<u64> = coords
        .iter()
        .map(|c| {
            let chunk = worldgen::generate_chunk_at_ctx(*c, &ctx_ow_after);
            fold_chunk(&chunk)
        })
        .collect();
    assert_eq!(
        folds_before, folds_after,
        "terrain must regenerate byte-identical after a dim round trip",
    );
}

fn fold_chunk(chunk: &Chunk) -> u64 {
    let mut h: u64 = 0xCBF29CE484222325;
    for b in chunk.raw() {
        h ^= b.0 as u64;
        h = h.wrapping_mul(0x100000001B3);
    }
    h
}

#[test]
fn switch_dim_swaps_mobs_per_dim() {
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
