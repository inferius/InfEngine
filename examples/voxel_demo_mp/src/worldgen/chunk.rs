//! Top-level chunk generator + the `Flat` / `Islands` flat-noise
//! profiles. Dispatches into the per-profile implementation based
//! on the `WorldgenCtx`. The Classic + Amplified paths share
//! `gen_classic` with an amplitude scale; Realistic lives in a
//! sibling module (see Phase E.3).
//!
//! W5.1: the legacy `generate_chunk_at(coord)` bridge that read
//! `WORLD_SEED` / `WORLDGEN` atomics is gone. Every caller passes
//! a ctx explicitly.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

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
