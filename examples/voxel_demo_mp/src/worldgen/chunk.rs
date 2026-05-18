//! Top-level chunk generator + the `Flat` / `Islands` flat-noise
//! profiles. Dispatches into the per-profile implementation based
//! on the currently-loaded `WORLDGEN`. The Classic + Amplified
//! paths share `gen_classic` with an amplitude scale; Realistic
//! lives in a sibling module (see Phase E.3).

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use super::biome::{Biome, biome_at};
use super::height::{cave_at, height_at_classic};
use super::ore::pick_stone_or_ore;
use super::{WorldGen, cactus, flowers, trees};

/// Top-level chunk generator. Dispatches to the per-profile
/// implementation based on the currently-loaded `WORLDGEN`.
pub fn generate_chunk_at(coord: ChunkCoord) -> Chunk {
    match WorldGen::current() {
        WorldGen::Classic => gen_classic(coord, 1.0),
        WorldGen::Amplified => gen_classic(coord, 2.5),
        WorldGen::Flat => gen_flat(coord),
        WorldGen::Islands => gen_islands(coord),
        WorldGen::Realistic => super::climate::gen_realistic(coord),
        WorldGen::Nether => super::nether::gen_nether(coord),
    }
}

/// Pancake-flat terrain at y=8: one row of grass, three rows of
/// dirt, stone below. Empty above. Trees disabled.
fn gen_flat(coord: ChunkCoord) -> Chunk {
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
fn gen_islands(coord: ChunkCoord) -> Chunk {
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
fn gen_classic(coord: ChunkCoord, amp_scale: f32) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let biome = biome_at(wx, wz);
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
            pick_stone_or_ore(wx, wy, wz, biome)
        }
    });
    // Decoration passes follow the same order as the pre-split
    // implementation so the chunk-fold hash stays unchanged:
    // trees -> flowers -> (no ponds) -> cactus.
    trees::place_classic_from_coord(&mut chunk, coord, amp_scale);
    flowers::place_classic_from_coord(&mut chunk, coord, amp_scale);
    cactus::place_classic_from_coord(&mut chunk, coord, amp_scale);
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

    fn itertools_xyz() -> impl Iterator<Item = (usize, usize, usize)> {
        let n = CHUNK_SIZE;
        (0..n).flat_map(move |z| {
            (0..n).flat_map(move |y| (0..n).map(move |x| (x, y, z)))
        })
    }
}
