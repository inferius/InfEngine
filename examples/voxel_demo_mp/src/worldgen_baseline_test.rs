//! Pre-split baseline snapshot test for `worldgen`. Computes a
//! deterministic 64-bit FNV-like fold over the block ids in a
//! handful of representative chunks under seed=0, the default
//! `Classic` generator. The numbers were captured against the
//! pre-Phase-E `worldgen.rs` so any structural change to the
//! generator (even a "pure refactor" split) is caught here.
//!
//! Also covers `Flat`, `Islands`, and `Amplified` so the same
//! guarantee holds across every shipped profile.
//!
//! The chunk coordinates are spread across +/- 4 chunks on the
//! x/z plane plus a single mid-air y=1 chunk so the test exercises
//! both the surface terrain pass and the under/over-y=0 cases.

#![cfg(test)]

use crate::worldgen::{
    WORLDGEN, WorldGen, generate_chunk_at, set_world_seed,
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

fn coords() -> [ChunkCoord; 7] {
    [
        [0, 0, 0],
        [1, 0, 0],
        [-1, 0, -1],
        [4, 0, 4],
        [-4, 0, 4],
        [0, 1, 0], // above-surface chunk
        [3, 0, -2],
    ]
}

fn snapshot(profile: WorldGen) -> [u64; 7] {
    set_world_seed(0);
    WORLDGEN.store(profile as u8, std::sync::atomic::Ordering::Relaxed);
    let cs = coords();
    let mut out = [0u64; 7];
    for (i, c) in cs.iter().enumerate() {
        out[i] = fold_chunk(*c);
    }
    out
}

// `WORLDGEN` + `WORLD_SEED` are global atomics shared with worker
// threads. The regression suite cycles through profiles in series,
// so it MUST serialise against any other test that touches them.
// Re-uses the shared `TEST_GEN_LOCK` from `worldgen::mod`.
use crate::worldgen::TEST_GEN_LOCK;

fn lock_gen() -> std::sync::MutexGuard<'static, ()> {
    // Poisoned-state recovery: another test panicked while
    // holding the lock; that's fine for our purposes since each
    // test re-seeds the world before reading.
    TEST_GEN_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Captured against `worldgen.rs` HEAD prior to the Phase E split.
/// If this test fails after the worldgen folder migration, the
/// split changed terrain output -> rollback and find the diff.
///
/// Runs all four profiles in series under a shared mutex so the
/// global `WORLDGEN` atomic can't be stomped by a sibling test
/// running in parallel.
#[test]
fn all_profiles_seed_0_byte_identical_after_split() {
    let _g = lock_gen();
    assert_eq!(
        snapshot(WorldGen::Classic),
        CLASSIC_SEED_0,
        "Classic seed=0 differs from pre-split snapshot",
    );
    assert_eq!(
        snapshot(WorldGen::Flat),
        FLAT_SEED_0,
        "Flat seed=0 differs from pre-split snapshot",
    );
    assert_eq!(
        snapshot(WorldGen::Islands),
        ISLANDS_SEED_0,
        "Islands seed=0 differs from pre-split snapshot",
    );
    assert_eq!(
        snapshot(WorldGen::Amplified),
        AMPLIFIED_SEED_0,
        "Amplified seed=0 differs from pre-split snapshot",
    );
}

// Snapshot values captured against `worldgen.rs` HEAD (commit
// 8a79fc2, "C.4f RMB-open furnace UI") prior to the Phase E split
// via `cargo test dump_snapshots -- --ignored --nocapture`.
const CLASSIC_SEED_0: [u64; 7] = [
    0x17a329954876e943,
    0x91d82ba3e14b165e,
    0x4b9c0cc38123e1db,
    0x5189420585774da1,
    0x64c0ccb8ca8d2abc,
    0xb68bcb4088dc1aa5,
    0x461492b8fbe40a5b,
];
const FLAT_SEED_0: [u64; 7] = [
    0xcecbf0fbda0ee525,
    0xcecbf0fbda0ee525,
    0xcecbf0fbda0ee525,
    0xcecbf0fbda0ee525,
    0xcecbf0fbda0ee525,
    0xb93a0c83ce3b6325,
    0xcecbf0fbda0ee525,
];
const ISLANDS_SEED_0: [u64; 7] = [
    0x033b79ef8f824613,
    0x13411b19e5157325,
    0x1c6cbefcb63c4cb0,
    0xb93a0c83ce3b6325,
    0xb93a0c83ce3b6325,
    0x969fc8c3d4a108f4,
    0x2dd75833bf438d8e,
];
const AMPLIFIED_SEED_0: [u64; 7] = [
    0x5809f0984e3685e2,
    0x94b0e032fec247db,
    0x5e45d458088a0745,
    0x641ef80644e17981,
    0x86e358c4b8b4cf68,
    0x1197d549b3b0d6c0,
    0xd052ddcd01ad8ff1,
];

/// One-shot capture: run with `cargo test dump_snapshots -- --ignored
/// --nocapture` to print fresh hash arrays. Used once during the
/// Phase E split to seed the const arrays above. Marked ignored so
/// CI never prints into the regular output.
#[test]
#[ignore]
#[allow(clippy::print_stdout)]
fn dump_snapshots() {
    let _g = lock_gen();
    for (name, profile) in [
        ("CLASSIC", WorldGen::Classic),
        ("FLAT", WorldGen::Flat),
        ("ISLANDS", WorldGen::Islands),
        ("AMPLIFIED", WorldGen::Amplified),
    ] {
        let s = snapshot(profile);
        println!("const {name}_SEED_0: [u64; 7] = [");
        for v in s {
            println!("    0x{v:016x},");
        }
        println!("];");
    }
}
