//! F.2a regression test: pure-function worldgen stability across
//! global-atomic writes.
//!
//! Today the worldgen call tree reads the `WORLD_SEED` + `WORLDGEN`
//! global atomics on every call. A `switch_dim` (or any other write
//! to those atomics) between two gens of the SAME chunk yields
//! different terrain even though the caller intended an identical
//! result. The user hit this for real: walk away from a chunk while
//! the worldgen profile changes, walk back, the chunk regenerates
//! with the new noise, and the saved block layout no longer matches
//! the freshly-derived biome ("snow floating where the mountain
//! used to be").
//!
//! This test reproduces the bug as a regression marker:
//!
//! 1. Build a `WorldgenCtx { base_seed, dim, profile }` snapshot.
//! 2. Generate chunk (0,0,0) via the new pure entry point.
//! 3. Stomp `WORLD_SEED` + `WORLDGEN` with completely different
//!    values (simulating a dim swap or `/worldgen` command between
//!    two gens of the SAME chunk).
//! 4. Regenerate chunk (0,0,0) via the same `WorldgenCtx`.
//! 5. Assert byte-identical output.
//!
//! Marked `#[ignore]` for F.2a because the pure entry point does
//! not exist yet. F.2b adds `generate_chunk_at_ctx`; later phases
//! remove the ignore.

#![cfg(test)]

use crate::worldgen::{
    TEST_GEN_LOCK, WORLDGEN, WorldGen, WorldgenCtx, generate_chunk_at_ctx,
    set_world_seed,
};
use infengine_voxel::ChunkCoord;

fn lock_gen() -> std::sync::MutexGuard<'static, ()> {
    TEST_GEN_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn fold_chunk_ctx(coord: ChunkCoord, ctx: &WorldgenCtx) -> u64 {
    let chunk = generate_chunk_at_ctx(coord, ctx);
    let mut h: u64 = 0xCBF29CE484222325;
    for b in chunk.raw() {
        h ^= b.0 as u64;
        h = h.wrapping_mul(0x100000001B3);
    }
    h
}

#[test]
#[ignore = "F.2a regression marker - enabled once F.2b adds the pure ctx entry point"]
fn pure_worldgen_is_stable_across_global_atomic_writes() {
    let _g = lock_gen();

    let ctx = WorldgenCtx {
        base_seed: 12345,
        dim: "overworld".to_string(),
        profile: WorldGen::Classic,
    };

    // Step 1: generate the chunk via the pure entry point.
    let hash_pre = fold_chunk_ctx([0, 0, 0], &ctx);

    // Step 2: stomp the atomics with completely unrelated values,
    // simulating a `switch_dim("nether")` + `/worldgen realistic`
    // between two gens of the same chunk.
    set_world_seed(999);
    WORLDGEN.store(
        WorldGen::Realistic as u8,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Step 3: re-gen the SAME chunk via the SAME ctx. If the
    // gen tree still secretly reads the atomics, the result will
    // differ and this test fails. After F.2b-F.2e the result must
    // match step 1 byte-for-byte.
    let hash_post = fold_chunk_ctx([0, 0, 0], &ctx);

    assert_eq!(
        hash_pre, hash_post,
        "pure WorldgenCtx-driven gen must be stable across global atomic writes",
    );
}

#[test]
#[ignore = "F.2a regression marker - enabled once F.2b adds the pure ctx entry point"]
fn pure_worldgen_two_dim_round_trip_is_byte_identical() {
    // 2-dim round-trip: gen 3 overworld chunks, switch dim (write
    // atomics + gen some nether), then re-gen the SAME 3 overworld
    // chunks via their original ctx. Byte-identical or bust.
    let _g = lock_gen();

    let ow_ctx = WorldgenCtx {
        base_seed: 0xDEAD_BEEF,
        dim: "overworld".to_string(),
        profile: WorldGen::Classic,
    };
    let nt_ctx = WorldgenCtx {
        base_seed: 0xDEAD_BEEF,
        dim: "nether".to_string(),
        profile: WorldGen::Nether,
    };

    let coords: [ChunkCoord; 3] = [[0, 0, 0], [1, 0, 0], [-1, 0, -1]];

    let ow_pre: Vec<u64> = coords
        .iter()
        .map(|c| fold_chunk_ctx(*c, &ow_ctx))
        .collect();

    // Simulate `switch_dim("nether")`: atomic writes + gen the
    // other dim's chunks.
    set_world_seed(crate::worldgen::dim_seed(0xDEAD_BEEF, "nether"));
    WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
    let _ = fold_chunk_ctx([0, 0, 0], &nt_ctx);
    let _ = fold_chunk_ctx([1, 0, 0], &nt_ctx);

    // Walk back to overworld. The original ctx still describes
    // the overworld; the atomics are now nether-state.
    let ow_post: Vec<u64> = coords
        .iter()
        .map(|c| fold_chunk_ctx(*c, &ow_ctx))
        .collect();

    assert_eq!(
        ow_pre, ow_post,
        "overworld chunks must regenerate identical after a dim swap",
    );
}
