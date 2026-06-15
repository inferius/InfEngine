//! Top-level chunk generator + the `Flat` / `Islands` flat-noise
//! profiles. Dispatches into the per-profile implementation based
//! on the `WorldgenCtx`. The Classic + Amplified paths share
//! `gen_classic` with an amplitude scale; Realistic lives in a
//! sibling module (see Phase E.3).
//!
//! W5.1: the legacy `generate_chunk_at(coord)` bridge that read
//! `WORLD_SEED` / `WORLDGEN` atomics is gone. Every caller passes
//! a ctx explicitly.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

use super::biome::{Biome, biome_at};
use super::height::{cave_at, height_at_classic};
use super::ore::pick_stone_or_ore;
use super::{WorldGen, WorldgenCtx, cactus, flowers, trees};

/// Pure-function chunk generator. Determined entirely by `ctx` +
/// `coord`; never reads any global state.
///
/// F.2c-3 finalised the migration: every noise leaf
/// (`biome_at`, `climate_at`, `ore_hash`, `hash2_for`,
/// `seed_offset_for`) takes the seed as a parameter. W5.1 deleted
/// the last globals (`WORLD_SEED` + `WORLDGEN`).
pub fn generate_chunk_at_ctx(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    match ctx.profile {
        WorldGen::Classic => gen_classic(coord, ctx, 1.0),
        WorldGen::Amplified => gen_classic(coord, ctx, 2.5),
        WorldGen::Flat => gen_flat(coord, ctx),
        WorldGen::Islands => gen_islands(coord, ctx),
        WorldGen::Realistic => super::climate::gen_realistic(coord, ctx),
        WorldGen::Nether => super::nether::gen_nether(coord, ctx),
        WorldGen::Natural => super::natural::gen_natural(coord, ctx),
        WorldGen::Mc => super::mcgen::terrain::gen_mc(coord, ctx),
    }
}

/// Generator surface height (the un-carved terrain top) at world
/// column `(wx, wz)` for the active profile. Mirrors the per-profile
/// height that `generate_chunk_at_ctx` fills, so a spawn placed at
/// `column_height + 1` sits on the real ground rather than the floor
/// of a cave / ravine carved into the column afterwards (which also
/// has clear air above it and otherwise reads as a valid surface).
pub(crate) fn column_height(ctx: &WorldgenCtx, wx: i32, wz: i32) -> i32 {
    let seed = ctx.effective_seed();
    let fx = wx as f32;
    let fz = wz as f32;
    match ctx.profile {
        WorldGen::Flat => 8,
        WorldGen::Islands => {
            // Mirror gen_islands' mask -> height; <= threshold is ocean.
            let a = (fx * 0.04).sin() + (fz * 0.05).cos();
            let b = (fx * 0.11 + 17.0).sin() * 0.5 + (fz * 0.13 + 9.0).cos() * 0.5;
            let m = a + b * 0.7;
            if m <= 0.55 {
                0
            } else {
                10 + ((m - 0.55) * 18.0).round() as i32
            }
        }
        WorldGen::Classic => {
            height_at_classic(fx, fz, biome_at(fx, fz, seed), 1.0)
        }
        WorldGen::Amplified => {
            height_at_classic(fx, fz, biome_at(fx, fz, seed), 2.5)
        }
        WorldGen::Realistic => {
            let climate = super::climate::climate_at(fx, fz, seed);
            let biome = super::biome_table::biome_from_climate(climate);
            super::climate::realistic_height(climate, biome)
        }
        WorldGen::Natural => {
            let biome =
                super::biome_table::biome_from_climate(super::climate::climate_at(fx, fz, seed));
            super::natural::natural_height(fx, fz, seed, biome)
        }
        WorldGen::Mc => super::mcgen::terrain::mc_column_height(ctx, wx, wz),
        // Nether is roofed + lava-floored; its spawn is handled
        // separately. A mid-height fallback keeps the dispatch total.
        WorldGen::Nether => 32,
    }
}

/// Find a dry-land column for an initial spawn: a square-spiral out
/// from the origin (probing one column every couple of chunks) for the
/// nearest column whose generator surface height clears sea level.
/// Returns `(wx, wz, height)`, or `None` if the whole search radius is
/// ocean (the caller keeps its own fallback).
///
/// W186: generator-only - reads NO chunk data - so it works before the
/// target region has streamed in. The MC profile (W184) gives the world
/// real oceans, so a fresh spawn can land in open water whose nearest
/// shore is hundreds of blocks past the streamed region; `safe_spawn`'s
/// in-world spiral can't see that far. This points the spawn at real
/// land and lets streaming fill the chunks in afterwards.
pub(crate) fn find_land_spawn(ctx: &WorldgenCtx) -> Option<(i32, i32, i32)> {
    use super::climate::SEA_LEVEL;
    // Probe every 2 chunks; search out to ~768 blocks. Coarse on
    // purpose - we want *some* nearby shore, not the exact nearest
    // column, and each probe samples the (heavy) density column.
    const STEP: i32 = 2 * CHUNK_SIZE as i32;
    const MAX_R: i32 = 24;
    for r in 0..=MAX_R {
        for dz in -r..=r {
            for dx in -r..=r {
                // Ring perimeter only (skip the filled interior).
                if r > 0 && dx.abs() != r && dz.abs() != r {
                    continue;
                }
                let wx = dx * STEP;
                let wz = dz * STEP;
                let h = column_height(ctx, wx, wz);
                if h > SEA_LEVEL {
                    return Some((wx, wz, h));
                }
            }
        }
    }
    None
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
    use infengine_voxel::CHUNK_SIZE;

    fn ctx_for(profile: WorldGen) -> WorldgenCtx {
        // W5.1b: tests build an explicit ctx; no globals to write.
        // Empty dim keeps `effective_seed()` == `base_seed`, so
        // baseline hashes captured under seed=0 still line up.
        WorldgenCtx {
            base_seed: 0,
            dim: String::new(),
            profile,
        }
    }

    #[test]
    fn flat_chunk_has_y8_grass_at_origin() {
        let chunk = generate_chunk_at_ctx([0, 0, 0], &ctx_for(WorldGen::Flat));
        // y=8 must be grass; y=9 must be air.
        assert_eq!(chunk.get(0, 8, 0).0, 3);
        assert_eq!(chunk.get(0, 9, 0).0, 0);
    }

    #[test]
    fn column_height_matches_profiles() {
        // Flat: constant crown at y=8 (matches gen_flat's grass row).
        assert_eq!(column_height(&ctx_for(WorldGen::Flat), 0, 0), 8);
        // Natural: the origin is lifted to dry land (>= sea+3 = 15) by
        // the spawn land-floor, so a spawn placed there clears water.
        assert!(column_height(&ctx_for(WorldGen::Natural), 0, 0) >= 15);
        // Classic: a plausible overworld surface height.
        let ch = column_height(&ctx_for(WorldGen::Classic), 40, -24);
        assert!((1..=200).contains(&ch), "classic height {ch} out of range");
    }

    #[test]
    fn find_land_spawn_lands_on_dry_ground() {
        use crate::worldgen::climate::SEA_LEVEL;
        // For the MC profile (oceans exist), the generator land search
        // must return a column whose surface clears sea level, so a
        // fresh spawn never lands the player treading water.
        let ctx = ctx_for(WorldGen::Mc);
        let (wx, wz, h) = super::find_land_spawn(&ctx).expect("land within search radius");
        assert!(h > SEA_LEVEL, "spawn column not dry: h={h}");
        // And the reported height must match column_height at that spot
        // (the spawn Y is derived from it).
        assert_eq!(h, column_height(&ctx, wx, wz));
    }

    #[test]
    fn gen_classic_is_idempotent() {
        let ctx = ctx_for(WorldGen::Classic);
        let a = generate_chunk_at_ctx([0, 0, 0], &ctx);
        let b = generate_chunk_at_ctx([0, 0, 0], &ctx);
        for (x, y, z) in itertools_xyz() {
            assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
        }
    }

    /// W5.1b: the ctx is the only path. This test pins the
    /// determinism contract: same ctx + same coord -> identical
    /// chunk, even across two independently-built ctxes.
    #[test]
    fn ctx_path_is_deterministic_classic_seed_0() {
        let ctx1 = WorldgenCtx {
            base_seed: 0,
            dim: String::new(),
            profile: WorldGen::Classic,
        };
        let ctx2 = WorldgenCtx {
            base_seed: 0,
            dim: String::new(),
            profile: WorldGen::Classic,
        };
        let a = generate_chunk_at_ctx([0, 0, 0], &ctx1);
        let b = generate_chunk_at_ctx([0, 0, 0], &ctx2);
        for (x, y, z) in itertools_xyz() {
            assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
        }
    }

    fn itertools_xyz() -> impl Iterator<Item = (usize, usize, usize)> {
        let n = CHUNK_SIZE;
        (0..n).flat_map(move |z| {
            (0..n).flat_map(move |y| (0..n).map(move |x| (x, y, z)))
        })
    }
}
