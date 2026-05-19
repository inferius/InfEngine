//! F.2a regression test: pure-function worldgen stability.
//!
//! Originally guarded against the `WORLD_SEED` + `WORLDGEN` global
//! atomics leaking into the gen tree (a `switch_dim` or `/worldgen`
//! between two gens of the same chunk would produce different
//! terrain). W5.1b deleted the atomics entirely; these tests now
//! assert the pure-ctx contract directly:
//!
//! 1. Same ctx + same coord -> byte-identical chunk.
//! 2. A second ctx with different content runs concurrently and
//!    does NOT influence the first ctx's result.
//! 3. Worker thread carrying a cloned ctx produces the same bytes
//!    as a main-thread gen.

#![cfg(test)]

use crate::worldgen::{WorldGen, WorldgenCtx, generate_chunk_at_ctx};
use infengine_voxel::ChunkCoord;

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
fn pure_worldgen_is_stable_under_unrelated_concurrent_gens() {
    let ctx = WorldgenCtx {
        base_seed: 12345,
        dim: "overworld".to_string(),
        profile: WorldGen::Classic,
    };

    // Step 1: gen the chunk via the pure entry point.
    let hash_pre = fold_chunk_ctx([0, 0, 0], &ctx);

    // Step 2: run an unrelated gen with a completely different
    // ctx (different seed + profile). With the atomics gone, this
    // can no longer corrupt the original ctx's gen path.
    let other_ctx = WorldgenCtx {
        base_seed: 999,
        dim: "nether".to_string(),
        profile: WorldGen::Realistic,
    };
    let _other = fold_chunk_ctx([5, 0, 5], &other_ctx);

    // Step 3: re-gen the SAME chunk via the SAME ctx.
    let hash_post = fold_chunk_ctx([0, 0, 0], &ctx);

    assert_eq!(
        hash_pre, hash_post,
        "pure WorldgenCtx-driven gen must be stable under unrelated concurrent gens",
    );
}

#[test]
fn pure_worldgen_is_stable_across_worker_thread_unrelated_gen() {
    // The F.2d worker-thread race used to require atomic stomps.
    // With no atomics, the test reduces to: spawn a worker doing
    // an unrelated gen; verify the main thread's ctx-driven gen
    // is unaffected.
    let ctx = WorldgenCtx {
        base_seed: 0x1234_5678,
        dim: "overworld".to_string(),
        profile: WorldGen::Classic,
    };

    // Control: gen the chunk on the main thread.
    let control = fold_chunk_ctx([2, 0, 3], &ctx);

    // Spawn a worker that gens with completely unrelated ctx.
    let unrelated = WorldgenCtx {
        base_seed: 0xFFFF_FFFF,
        dim: "nether".to_string(),
        profile: WorldGen::Realistic,
    };
    let _ = std::thread::spawn(move || fold_chunk_ctx([7, 0, 7], &unrelated))
        .join()
        .expect("worker thread");

    let ctx_for_worker = ctx.clone();
    let worker_hash = std::thread::spawn(move || {
        fold_chunk_ctx([2, 0, 3], &ctx_for_worker)
    })
    .join()
    .expect("worker thread");

    assert_eq!(
        control, worker_hash,
        "worker-thread gen with a cloned ctx must match main-thread gen",
    );
}

#[test]
fn pure_worldgen_two_dim_round_trip_is_byte_identical() {
    // 2-dim round-trip: gen 3 overworld chunks, gen some nether
    // chunks with a separate ctx, then re-gen the SAME 3
    // overworld chunks via their original ctx. Byte-identical or
    // bust.
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

    // Run nether gens in the meantime. With no atomics this can
    // no longer corrupt the overworld path.
    let _ = fold_chunk_ctx([0, 0, 0], &nt_ctx);
    let _ = fold_chunk_ctx([1, 0, 0], &nt_ctx);

    let ow_post: Vec<u64> = coords
        .iter()
        .map(|c| fold_chunk_ctx(*c, &ow_ctx))
        .collect();

    assert_eq!(
        ow_pre, ow_post,
        "overworld chunks must regenerate identical after intervening nether gens",
    );
}
