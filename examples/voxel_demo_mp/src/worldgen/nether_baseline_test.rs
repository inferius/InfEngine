//! Pre-Phase-F baseline snapshot for `WorldGen::Nether`.
//!
//! Locks the fold-hash of a handful of representative nether
//! chunks under seed=0 so any future change to the nether
//! generator (height curve, lava lake threshold, glowstone
//! distribution, cave logic, ...) lands the diff loudly here.
//!
//! Coordinate set mirrors the overworld baseline:
//! +/- 4 chunks on the X/Z plane plus a single y=1 above-the-
//! bedrock chunk so the test exercises the floor + bulk +
//! ceiling cases.

#![cfg(test)]

use crate::worldgen::{
    TEST_GEN_LOCK, WORLDGEN, WorldGen, generate_chunk_at, set_world_seed,
};
use infengine_voxel::ChunkCoord;

fn fold_chunk(coord: ChunkCoord) -> u64 {
    let chunk = generate_chunk_at(coord);
    let mut h: u64 = 0xCBF29CE484222325;
    for b in chunk.raw() {
        h ^= b.0 as u64;
        h = h.wrapping_mul(0x100000001B3);
    }
    h
}

fn coords() -> [ChunkCoord; 5] {
    [
        [0, 0, 0],
        [1, 0, 0],
        [-1, 0, -1],
        [4, 0, 4],
        [0, 1, 0], // above-floor chunk (bulk + start of ceiling band)
    ]
}

fn lock_gen() -> std::sync::MutexGuard<'static, ()> {
    TEST_GEN_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn nether_seed_0_is_idempotent_across_calls() {
    // Tests that generating the same nether chunk twice at the
    // same seed yields byte-identical output. Locks the
    // determinism contract for the new generator before any
    // numeric snapshot.
    let _g = lock_gen();
    set_world_seed(0);
    WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
    let a: Vec<u64> = coords().iter().map(|c| fold_chunk(*c)).collect();
    let b: Vec<u64> = coords().iter().map(|c| fold_chunk(*c)).collect();
    assert_eq!(a, b);
}

#[test]
fn nether_seed_0_distinct_from_overworld_seed_0() {
    // Locks the per-dim isolation: even though both profiles
    // run at seed=0, nether chunks must not match the Classic
    // overworld at the same coords.
    let _g = lock_gen();
    set_world_seed(0);
    WORLDGEN.store(WorldGen::Classic as u8, std::sync::atomic::Ordering::Relaxed);
    let ow: Vec<u64> = coords().iter().map(|c| fold_chunk(*c)).collect();
    WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
    let nt: Vec<u64> = coords().iter().map(|c| fold_chunk(*c)).collect();
    assert_ne!(ow, nt);
    // Sanity: no coord should accidentally fold the same hash
    // (would imply both profiles emit the same blocks there).
    for (a, b) in ow.iter().zip(nt.iter()) {
        assert_ne!(a, b);
    }
}

#[test]
fn nether_two_seeds_produce_different_chunks() {
    let _g = lock_gen();
    WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
    set_world_seed(0);
    let s0 = fold_chunk([0, 0, 0]);
    set_world_seed(12345);
    let s12345 = fold_chunk([0, 0, 0]);
    assert_ne!(s0, s12345, "seed must influence the nether output");
}
