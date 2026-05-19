//! Top-level chunk generator + the `Flat` / `Islands` flat-noise
//! profiles. Dispatches into the per-profile implementation based
//! on the `WorldgenCtx` (Phase F.2) or the currently-loaded
//! `WORLDGEN` atomic (legacy path). The Classic + Amplified
//! paths share `gen_classic` with an amplitude scale; Realistic
//! lives in a sibling module (see Phase E.3).

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use super::biome::{Biome, biome_at};
use super::height::{cave_at, height_at_classic};
use super::ore::pick_stone_or_ore;
use super::{WorldGen, WorldgenCtx, cactus, flowers, trees};

/// Top-level chunk generator. Dispatches to the per-profile
/// implementation based on the currently-loaded `WORLDGEN`.
///
/// Legacy entry point - reads the `WORLD_SEED` + `WORLDGEN` global
/// atomics. Kept as a thin wrapper around `generate_chunk_at_ctx`
/// for headless callers + tests that still build chunks against
/// the active atomic state. New code should prefer
/// `generate_chunk_at_ctx`, which is race-free across atomic
/// writes.
#[allow(dead_code)] // headless lib + tests still reach for it.
pub fn generate_chunk_at(coord: ChunkCoord) -> Chunk {
    // Build a ctx snapshot from the atomics so the dispatch goes
    // through the new path. The sub-modules (biome / climate / ore
    // / trees / decorations) still read the atomics until F.2c, so
    // the wrapper isn't race-free yet - the ctx is mostly along for
    // the ride. Once F.2c plumbs ctx into the noise leaves, the
    // legacy atomics fall away and this fn picks them up only as
    // the "active state" cache.
    let ctx = WorldgenCtx {
        // The `WORLD_SEED` atomic already holds the effective seed
        // (dim_seed(base, current_dim) per Phase F.1). Recovering
        // the base + dim from it isn't possible, so we stash the
        // effective value as the base and leave dim empty - it
        // produces the same `effective_seed()` and behaves
        // identically against any pure-fn that only consumes the
        // mixed value.
        base_seed: super::world_seed(),
        dim: String::new(),
        profile: WorldGen::current(),
    };
    generate_chunk_at_ctx(coord, &ctx)
}

/// Pure-function chunk generator. Determined entirely by `ctx` +
/// `coord`; never reads the global `WORLD_SEED` / `WORLDGEN`
/// atomics.
///
/// F.2c-3 finalised the migration: every noise leaf
/// (`biome_at`, `climate_at`, `ore_hash`, `hash2_for`,
/// `seed_offset_for`) takes the seed as a parameter, so a
/// concurrent stomp of the global atomics on another thread can
/// no longer poison this call.
pub fn generate_chunk_at_ctx(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    match ctx.profile {
        WorldGen::Classic => gen_classic(coord, ctx, 1.0),
        WorldGen::Amplified => gen_classic(coord, ctx, 2.5),
        WorldGen::Flat => gen_flat(coord, ctx),
        WorldGen::Islands => gen_islands(coord, ctx),
        WorldGen::Realistic => super::climate::gen_realistic(coord, ctx),
        WorldGen::Nether => super::nether::gen_nether(coord, ctx),
    }
}

/// Pancake-flat terrain at y=8: one row of grass, three rows of
/// dirt, stone below. Empty above. Trees disabled.
///
/// Flat is deterministic regardless of seed - it doesn't sample
/// any noise. `ctx` is unused but kept on the signature for
/// uniform dispatch.
fn gen_flat(coord: ChunkCoord, _ctx: &WorldgenCtx) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|_x, y, _z| {
        let wy = origin.y as i32 + y as i32;
        match wy {
            n if n > 8 => BlockId::AIR,
            8 => BlockId(3),     // grass
            5..=7 => BlockId(2), // dirt
            _ => BlockId(1),     // stone
        }
    });
    chunk
}

/// Sparse-island terrain: only where 2D noise exceeds a threshold
/// does land rise; everywhere else is bottomless air. Trees grow
/// on the island tops.
///
/// Islands uses pure sin/cos noise of (wx, wz) - no seed mixing -
/// so the result is independent of `ctx`. The argument is kept for
/// uniform dispatch.
fn gen_islands(coord: ChunkCoord, _ctx: &WorldgenCtx) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    let mask = |wx: f32, wz: f32| -> f32 {
        // Two octaves of sin/cos; treat the result as height
        // potential. Anything < threshold = ocean (air).
        let a = (wx * 0.04).sin() + (wz * 0.05).cos();
        let b = (wx * 0.11 + 17.0).sin() * 0.5 + (wz * 0.13 + 9.0).cos() * 0.5;
        a + b * 0.7
    };
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let m = mask(wx, wz);
        if m <= 0.55 {
            return BlockId::AIR;
        }
        let h = 10 + ((m - 0.55) * 18.0).round() as i32;
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            return BlockId::AIR;
        }
        if wy == h {
            BlockId(3) // grass crown
        } else if wy >= h - 2 {
            BlockId(2) // dirt
        } else {
            BlockId(1) // stone core
        }
    });
    chunk
}

/// Classic biome-driven terrain. `amp_scale` lets the Amplified
/// preset reuse this generator with a 2.5x vertical multiplier.
///
/// F.2c-1: `biome_at` now takes an explicit seed, threaded through
/// via `ctx.effective_seed()`. The decoration helpers + ore
/// picker still read the atomic; F.2c-2 lifts those.
fn gen_classic(coord: ChunkCoord, ctx: &WorldgenCtx, amp_scale: f32) -> Chunk {
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let biome = biome_at(wx, wz, seed);
        let h = height_at_classic(wx, wz, biome, amp_scale);
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            return BlockId::AIR;
        }
        // Cave carve: only below the 3-block dirt / grass cap so
        // we don't punch holes through the surface.
        if wy < h - 3 && cave_at(wx, wy as f32, wz) {
            return BlockId::AIR;
        }
        // Surface block per biome. Mountains above the snow line
        // swap to snow (id 15) for visual identity.
        let surface = match biome {
            Biome::Desert => BlockId(4),
            Biome::Plains => BlockId(3),
            Biome::Mountain => {
                if h >= 26 {
                    BlockId(15)
                } else {
                    BlockId(1)
                }
            }
            _ => BlockId(3),
        };
        let near_surface = match biome {
            Biome::Desert => BlockId(26), // sandstone under sand cap
            Biome::Plains => BlockId(2),
            Biome::Mountain => BlockId(1),
            _ => BlockId(2),
        };
        if wy == h {
            surface
        } else if wy >= h - 3 {
            near_surface
        } else {
            pick_stone_or_ore(wx, wy, wz, biome, seed)
        }
    });
    // Decoration passes follow the same order as the pre-split
    // implementation so the chunk-fold hash stays unchanged:
    // trees -> flowers -> (no ponds) -> cactus.
    trees::place_classic_from_coord(&mut chunk, coord, amp_scale, ctx);
    flowers::place_classic_from_coord(&mut chunk, coord, amp_scale, ctx);
    cactus::place_classic_from_coord(&mut chunk, coord, amp_scale, ctx);
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{TEST_GEN_LOCK, WORLDGEN, set_world_seed};
    use infengine_voxel::CHUNK_SIZE;

    fn lock_gen() -> std::sync::MutexGuard<'static, ()> {
        TEST_GEN_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn flat_chunk_has_y8_grass_at_origin() {
        let _g = lock_gen();
        set_world_seed(0);
        WORLDGEN.store(WorldGen::Flat as u8, std::sync::atomic::Ordering::Relaxed);
        let chunk = generate_chunk_at([0, 0, 0]);
        // y=8 must be grass; y=9 must be air.
        assert_eq!(chunk.get(0, 8, 0).0, 3);
        assert_eq!(chunk.get(0, 9, 0).0, 0);
    }

    #[test]
    fn gen_classic_is_idempotent() {
        let _g = lock_gen();
        set_world_seed(0);
        WORLDGEN.store(WorldGen::Classic as u8, std::sync::atomic::Ordering::Relaxed);
        let a = generate_chunk_at([0, 0, 0]);
        let b = generate_chunk_at([0, 0, 0]);
        for (x, y, z) in itertools_xyz() {
            assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
        }
    }

    /// F.2b: legacy `generate_chunk_at` is now a thin wrapper
    /// around `generate_chunk_at_ctx` that builds a ctx from the
    /// atomics. Two calls (one through each entry point) with the
    /// matching seed + profile must produce byte-identical chunks.
    #[test]
    fn ctx_path_matches_atomic_path_classic_seed_0() {
        let _g = lock_gen();
        set_world_seed(0);
        WORLDGEN.store(WorldGen::Classic as u8, std::sync::atomic::Ordering::Relaxed);
        let legacy = generate_chunk_at([0, 0, 0]);
        let ctx = WorldgenCtx {
            base_seed: 0,
            dim: String::new(),
            profile: WorldGen::Classic,
        };
        let via_ctx = generate_chunk_at_ctx([0, 0, 0], &ctx);
        for (x, y, z) in itertools_xyz() {
            assert_eq!(legacy.get(x, y, z).0, via_ctx.get(x, y, z).0);
        }
    }

    fn itertools_xyz() -> impl Iterator<Item = (usize, usize, usize)> {
        let n = CHUNK_SIZE;
        (0..n).flat_map(move |z| {
            (0..n).flat_map(move |y| (0..n).map(move |x| (x, y, z)))
        })
    }
}
