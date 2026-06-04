//! W71 - "Natural" worldgen profile: MC-inspired fractal terrain.
//!
//! The Classic + Realistic profiles shape their height with a handful
//! of sin/cos terms, which reads as smooth, visibly periodic rolling
//! hills. Natural swaps that for multi-octave value noise (fBm) -
//! broad continentalness + biome-scaled hills + fine detail + a ridged
//! term for mountain peaks - so the terrain looks like MC's irregular,
//! non-repeating landscape. Biome selection, surface blocks, caves,
//! ravines, ores, trees, flowers and dungeons all reuse the Realistic
//! machinery; only the height curve changes.
//!
//! MC mirror: `net/minecraft/world/level/levelgen` density functions
//! stack octave noise into continentalness / erosion / peaks-and-
//! valleys; this approximates that layering with plain value-noise fBm.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use super::biome::Biome;
use super::biome_table::biome_from_climate;
use super::climate::{SEA_LEVEL, climate_at, near_surface_block, surface_block};
use super::ore::pick_stone_or_ore;
use super::{WorldgenCtx, cactus, caves, dungeons, flowers, ravines, trees};

/// Integer-grid hash -> `[0, 1)`. Stable per `(ix, iz, seed)`.
fn hash2(ix: i32, iz: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x8DA6_B343)
        ^ (iz as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0x9E37_79B1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smooth value noise at `(x, z)` in `[-1, 1]`: bilinear over the unit
/// integer grid with a smoothstep fade on each axis.
fn value_noise2(x: f32, z: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let fx = x - ix as f32;
    let fz = z - iz as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sz = fz * fz * (3.0 - 2.0 * fz);
    let v00 = hash2(ix, iz, seed);
    let v10 = hash2(ix + 1, iz, seed);
    let v01 = hash2(ix, iz + 1, seed);
    let v11 = hash2(ix + 1, iz + 1, seed);
    let a = lerp(v00, v10, sx);
    let b = lerp(v01, v11, sx);
    lerp(a, b, sz) * 2.0 - 1.0
}

/// Fractal Brownian motion: `octaves` of value noise summed at
/// doubling frequency / halving amplitude, normalised to `[-1, 1]`.
fn fbm2(x: f32, z: f32, seed: u32, octaves: u32) -> f32 {
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for o in 0..octaves {
        let s = seed.wrapping_add(o.wrapping_mul(0x9E37));
        sum += value_noise2(x * freq, z * freq, s) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// MC-inspired terrain height at `(wx, wz)`. Layers broad
/// continentalness, biome-scaled hills, fine detail, and a ridged
/// term that sharpens mountain peaks.
pub(crate) fn natural_height(wx: f32, wz: f32, seed: u32, biome: Biome) -> i32 {
    let cont = fbm2(wx * 0.0035, wz * 0.0035, seed ^ 0x00C0_FFEE, 5);
    let hills = fbm2(wx * 0.013, wz * 0.013, seed ^ 0xB117_0000, 4);
    let detail = fbm2(wx * 0.05, wz * 0.05, seed ^ 0xD7A1_5EED, 3);
    let (bias, amp, ridged) = match biome {
        Biome::Mountain | Biome::SnowyMountain => (14.0, 28.0, true),
        Biome::Jungle => (5.0, 10.0, false),
        Biome::Forest | Biome::Taiga => (4.0, 8.0, false),
        Biome::Desert => (2.0, 5.0, false),
        Biome::Tundra => (2.0, 5.0, false),
        Biome::Plains | Biome::Savanna => (2.0, 4.0, false),
        Biome::Swamp => (-2.0, 2.0, false),
        Biome::Beach => (-1.0, 1.5, false),
    };
    let mut elev = cont * 10.0 + hills * amp + detail * 2.0 + bias;
    if ridged {
        // `1 - |hills|` folds the noise into ridge lines for peaks.
        elev += (1.0 - hills.abs()) * 14.0;
    }
    (SEA_LEVEL as f32 + elev).round() as i32
}

/// Natural chunk generator: the Realistic biome / surface / carve /
/// decorate pipeline driven by the fBm height curve above.
pub(crate) fn gen_natural(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let climate = climate_at(wx, wz, seed);
        let biome = biome_from_climate(climate);
        let h = natural_height(wx, wz, seed, biome);
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            if wy <= SEA_LEVEL && h < SEA_LEVEL {
                return BlockId(36); // water fills below sea level
            }
            return BlockId::AIR;
        }
        if wy == h {
            surface_block(biome, h)
        } else if wy >= h - 3 {
            near_surface_block(biome)
        } else {
            pick_stone_or_ore(wx, wy, wz, biome, seed)
        }
    });
    caves::carve(&mut chunk, coord, ctx);
    ravines::carve(&mut chunk, coord, ctx);
    trees::place_realistic_from_coord(&mut chunk, coord, ctx);
    flowers::place_realistic_from_coord(&mut chunk, coord, ctx);
    cactus::place_realistic_from_coord(&mut chunk, coord, ctx);
    dungeons::place(&mut chunk, coord, ctx);
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fbm_stays_in_unit_range() {
        for i in 0..1000 {
            let x = i as f32 * 1.7;
            let z = i as f32 * 0.9;
            let n = fbm2(x * 0.01, z * 0.01, 42, 5);
            assert!((-1.05..=1.05).contains(&n), "fbm out of range: {n}");
        }
    }

    #[test]
    fn natural_height_varies_and_is_deterministic() {
        let a = natural_height(100.0, 200.0, 7, Biome::Plains);
        let b = natural_height(100.0, 200.0, 7, Biome::Plains);
        assert_eq!(a, b, "deterministic");
        // Sweep a line; the surface must not be constant (proves the
        // noise actually shapes terrain rather than a flat plane).
        let mut seen = std::collections::HashSet::new();
        for wx in 0..300 {
            seen.insert(natural_height(wx as f32, 0.0, 7, Biome::Plains));
        }
        assert!(seen.len() > 4, "terrain too flat: {} distinct", seen.len());
    }

    #[test]
    fn mountains_outclimb_plains() {
        // Summed over a patch, the mountain biome sits higher than
        // plains under the same noise field.
        let mut mh = 0i64;
        let mut ph = 0i64;
        for i in 0..64 {
            let wx = (i * 7) as f32;
            let wz = (i * 13) as f32;
            mh += natural_height(wx, wz, 5, Biome::Mountain) as i64;
            ph += natural_height(wx, wz, 5, Biome::Plains) as i64;
        }
        assert!(mh > ph, "mountains {mh} should outclimb plains {ph}");
    }
}
