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
//!
//! W5.1b: switched from the deleted `generate_chunk_at` atomic
//! bridge to `generate_chunk_at_ctx` with an explicit
//! `WorldgenCtx` per snapshot. Empty `dim` preserves the
//! pre-Wave-5 effective seed (= base_seed) so the snapshot
//! hashes don't shift.

#![cfg(test)]

use crate::worldgen::{
    WorldGen, WorldgenCtx, generate_chunk_at_ctx,
};
use infengine_voxel::ChunkCoord;

fn fold_chunk(coord: ChunkCoord, ctx: &WorldgenCtx) -> u64 {
    let chunk = generate_chunk_at_ctx(coord, ctx);
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
    // W5.1b: pure-fn ctx, no globals to lock.
    let ctx = WorldgenCtx {
        base_seed: 0,
        dim: String::new(),
        profile,
    };
    let cs = coords();
    let mut out = [0u64; 7];
    for (i, c) in cs.iter().enumerate() {
        out[i] = fold_chunk(*c, &ctx);
    }
    out
}

/// Captured against `worldgen.rs` HEAD prior to the Phase E split.
/// If this test fails after the worldgen folder migration, the
/// split changed terrain output -> rollback and find the diff.
///
/// W5.1b: no global lock needed; each snapshot uses its own ctx.
#[test]
fn all_profiles_seed_0_byte_identical_after_split() {
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
// W121: Classic + Amplified re-snapshotted after the modern-MC
// underground (ore depth bands + deepslate stratum + bedrock floor).
// Both profiles share `pick_stone_or_ore`, and their terrain sits below
// SEA_LEVEL, so the new strata change their deep-block content. The
// height curves are untouched. Flat + Islands (which never call the ore
// picker) are byte-identical and were left as-is.
// W149: Classic + Amplified re-snapshotted again after the underground
// variety blobs (dirt / gravel / granite / diorite / andesite / tuff
// via `strata::blob_at`) - same shared-picker reasoning as W121; Flat +
// Islands verified byte-identical against the previous dump.
const CLASSIC_SEED_0: [u64; 7] = [
    0xb633d73168685d09,
    0x64ae3483fa398118,
    0xae07ac57270e8e31,
    0x9a7e997bc461acd7,
    0x7591fcc7a14e587e,
    0xe5c77cb70ded7cb8,
    0x7aba9185434ec05a,
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
    0xa33b6196709cda6b,
    0x0094c725e278651d,
    0xb1e6b3be018b24fb,
    0xe4c1742b1b1b9338,
    0x7f046276b96df596,
    0x6a098145a6fead5e,
    0x7e94028ef7b7a41e,
];

/// One-shot capture: run with `cargo test dump_snapshots -- --ignored
/// --nocapture` to print fresh hash arrays. Used once during the
/// Phase E split to seed the const arrays above. Marked ignored so
/// CI never prints into the regular output.
#[test]
#[ignore]
#[allow(clippy::print_stdout)]
fn dump_snapshots() {
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
