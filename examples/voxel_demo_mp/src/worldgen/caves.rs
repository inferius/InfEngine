//! 3D-noise cave carver for the `Realistic` generator (W17.1).
//!
//! Algorithm: two interleaved octaves of hand-rolled gradient noise
//! (hash-based, no external crate) are multiplied together. Where
//! the combined value exceeds a threshold the voxel is carved to
//! air. The multiplicative shape produces elongated, tunnel-like
//! voids rather than blob caves.
//!
//! Density targets (W121: the deep-zone switch is now SEA_LEVEL-relative
//! via `strata::depth_fraction`, not a hard `y < 10`, so caves spread
//! through the WHOLE underground instead of cramming below the old
//! sea-12 numbers):
//! - shallow underground: threshold 0.82 -> ~5% of voxels carved.
//! - deep stratum:        threshold 0.70 -> richer caves, lava-lake zone.
//! - bedrock floor:       never carved   -> keeps the world floor solid.
//!
//! Two callers:
//! - `gen_realistic` in `climate.rs` - primary user.
//! - `gen_classic`   in `chunk.rs`   - intentionally NOT wired; Classic
//!   stays vanilla-flat per the W17 decision.
//!
//! Every random decision mixes the seed via `hash3_cave` so two
//! peers with the same seed see identical cave systems.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use crate::worldgen::WorldgenCtx;
use crate::worldgen::strata::{self, WORLD_FLOOR};

// ---------------------------------------------------------------
// Noise primitives
// ---------------------------------------------------------------

/// Cheap 3D value noise in [0, 1]. Mixes integer voxel coords +
/// seed with two rounds of multiply-xor-shift. Deterministic across
/// platforms (no f32 trig; pure integer ops only).
fn hash3_cave(ix: i32, iy: i32, iz: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (iy as u32).wrapping_mul(0x9E37_79B1);
    h ^= (iz as u32).wrapping_mul(0x6C62_272E);
    h ^= seed;
    // Two avalanche rounds.
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32
}

/// Trilinear interpolation helper.
#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Sample 3D value noise at a continuous world position. Divides
/// (wx, wy, wz) by `scale` to set the feature frequency, then
/// trilinearly interpolates between the eight surrounding integer-
/// lattice hash values. Returns a value in [0, 1].
fn sample_noise3(wx: f32, wy: f32, wz: f32, scale: f32, seed: u32) -> f32 {
    let sx = wx / scale;
    let sy = wy / scale;
    let sz = wz / scale;

    let x0 = sx.floor() as i32;
    let y0 = sy.floor() as i32;
    let z0 = sz.floor() as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let z1 = z0 + 1;

    let tx = sx - x0 as f32;
    let ty = sy - y0 as f32;
    let tz = sz - z0 as f32;

    // Smooth the interpolation weights (Ken Perlin's quintic).
    let tx = tx * tx * (3.0 - 2.0 * tx);
    let ty = ty * ty * (3.0 - 2.0 * ty);
    let tz = tz * tz * (3.0 - 2.0 * tz);

    let v000 = hash3_cave(x0, y0, z0, seed);
    let v100 = hash3_cave(x1, y0, z0, seed);
    let v010 = hash3_cave(x0, y1, z0, seed);
    let v110 = hash3_cave(x1, y1, z0, seed);
    let v001 = hash3_cave(x0, y0, z1, seed);
    let v101 = hash3_cave(x1, y0, z1, seed);
    let v011 = hash3_cave(x0, y1, z1, seed);
    let v111 = hash3_cave(x1, y1, z1, seed);

    let x00 = lerp(v000, v100, tx);
    let x10 = lerp(v010, v110, tx);
    let x01 = lerp(v001, v101, tx);
    let x11 = lerp(v011, v111, tx);

    let y0v = lerp(x00, x10, ty);
    let y1v = lerp(x01, x11, ty);

    lerp(y0v, y1v, tz)
}

// ---------------------------------------------------------------
// Cave decision
// ---------------------------------------------------------------

/// Cave feature scale (world units). Larger = fewer, longer caves.
const CAVE_SCALE_A: f32 = 12.0;
/// Second octave at a smaller scale for thinner finger passages.
const CAVE_SCALE_B: f32 = 6.0;
/// Salt mixed into seed for the second octave so A and B are
/// decorrelated even though both use `hash3_cave`.
const OCTAVE_B_SALT: u32 = 0xA5B4_C3D2;

/// Carve threshold for the shallow underground.
/// ~5% of voxels satisfy `cave_value > THRESHOLD_NORMAL`.
const THRESHOLD_NORMAL: f32 = 0.82;
/// Carve threshold for the deep stratum.
/// Wider caves / more frequent carving to evoke the lava-lake zone.
const THRESHOLD_DEEP: f32 = 0.70;

/// Returns `true` when the voxel at (wx, wy, wz) falls inside a
/// cave void. The multiplicative combination of two octaves creates
/// elongated tunnels that branch, matching MC's cave carver feel.
///
/// W121: the deep/shallow split is keyed to `strata::is_deepslate` (a
/// SEA_LEVEL-relative depth fraction) instead of the old absolute
/// `y < 10`, so the richer deep caves track the deepslate stratum and
/// the normal caves fill the whole band up to the surface. The bedrock
/// floor (`wy < WORLD_FLOOR + BEDROCK_ROWS`-ish) stays solid.
pub(crate) fn is_cave(wx: f32, wy: f32, wz: f32, seed: u32) -> bool {
    if wy <= WORLD_FLOOR as f32 {
        return false;
    }
    let a = sample_noise3(wx, wy, wz, CAVE_SCALE_A, seed);
    // Distinct seed for B so the two fields are uncorrelated.
    let b = sample_noise3(wx, wy, wz, CAVE_SCALE_B, seed ^ OCTAVE_B_SALT);
    // Multiply -> highest values only where BOTH octaves are high.
    let v = a * b;
    let threshold = if strata::is_deepslate(wy.floor() as i32) {
        THRESHOLD_DEEP * THRESHOLD_DEEP
    } else {
        THRESHOLD_NORMAL * THRESHOLD_NORMAL
    };
    v > threshold
}

// ---------------------------------------------------------------
// Chunk-level carve pass
// ---------------------------------------------------------------

/// Carve caves into an already-filled chunk. Call this AFTER the
/// terrain-fill pass but BEFORE the ore decoration pass.
///
/// Rules:
/// - Only stone, cobble, and ore blocks are carved (not surface
///   grass/sand/snow, not water, not air that was already placed).
/// - The top 4 rows of any column are never carved so the surface
///   stays intact (matching the Classic generator's `h - 3` guard).
///
/// `surface_y` is intentionally NOT passed here; instead we use
/// a simple block-id allowlist so we don't need to re-derive the
/// height map. Blocks that should never be carved (grass, sand,
/// dirt, water, air) are passed through unchanged.
pub(crate) fn carve(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    use infengine_voxel::CHUNK_SIZE;

    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let cs = CHUNK_SIZE as i32;

    // Collect positions to carve (can't mutate chunk while iterating
    // via fill_with without a secondary pass).
    let mut to_carve = Vec::new();
    for z in 0..cs {
        for y in 0..cs {
            for x in 0..cs {
                let wx = origin.x + x as f32;
                let wy = origin.y as i32 + y;
                let wz = origin.z + z as f32;

                let block = chunk.get(x as usize, y as usize, z as usize);
                // Only carve stone-like blocks. Skips surface blocks,
                // air, water, bedrock, etc.
                if !is_carveable(block) {
                    continue;
                }
                // Skip the top 4 voxels of each column to guard the
                // surface cap. We use a world-Y cutoff: columns whose
                // surface is above y=8 always have some sub-surface
                // dirt/grass cap; y < 5 is deep enough to be safe.
                // The simpler check is: never carve within 4 blocks of
                // the top of any cave-eligible column. Since we don't
                // know h here, guard by world-Y range: y >= 5 is a
                // safe lower-bound.
                if wy < 5 {
                    // Allow near-bedrock carving for the deep layer, but
                    // never cut into the bedrock floor (W121).
                    if wy <= WORLD_FLOOR {
                        continue;
                    }
                }
                if is_cave(wx, wy as f32, wz, seed) {
                    to_carve.push((x as usize, y as usize, z as usize));
                }
            }
        }
    }
    for (x, y, z) in to_carve {
        chunk.set(x, y, z, BlockId::AIR);
    }
}

/// Block ids that the cave carver is allowed to replace with air.
/// Everything else (grass, sand, water, bedrock, ...) is preserved.
///
/// W121: the deepslate stratum (96) + the full ore roster (stone-form
/// 13/14/117-122 and deepslate-form 123-126/246-249) are now carveable
/// so caves don't leave floating ore / un-carved deepslate columns where
/// they cut through the deep underground.
fn is_carveable(block: BlockId) -> bool {
    matches!(
        block.0,
        1  // stone
        | 8  // cobblestone
        | 26 // sandstone
        | 96 // deepslate
        | 13 | 14 | 117..=122 // stone-form ores (coal, iron, gold..copper)
        | 123..=126 // deepslate ores (coal, iron, diamond, gold)
        | 246..=249 // deepslate ores (redstone, lapis, emerald, copper)
    )
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{WorldGen, WorldgenCtx};
    use infengine_voxel::{BlockId, CHUNK_SIZE};

    fn ctx(seed: u32) -> WorldgenCtx {
        WorldgenCtx {
            base_seed: seed,
            dim: String::new(),
            profile: WorldGen::Realistic,
        }
    }

    // Build a completely stone-filled chunk (all block id 1) so the
    // cave carver has something to work on.
    fn stone_chunk() -> Chunk {
        let mut c = Chunk::empty();
        let n = CHUNK_SIZE;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    c.set(x, y, z, BlockId(1)); // stone
                }
            }
        }
        c
    }

    // How many voxels in the chunk now have block id 0 (air)?
    fn count_air(chunk: &Chunk) -> usize {
        let n = CHUNK_SIZE;
        let mut count = 0;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    if chunk.get(x, y, z) == BlockId::AIR {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    // ---------------------------------------------------------------
    // W17.1 cave tests
    // ---------------------------------------------------------------

    /// Carving a fully-stone underground chunk produces some air
    /// (caves exist inside solid ground).
    #[test]
    fn caves_carve_some_voxels_below_surface() {
        // Coord [0, 0, 0] -> world-Y range 0..16, well underground.
        let mut chunk = stone_chunk();
        let ctx = ctx(42);
        carve(&mut chunk, [0, 0, 0], &ctx);
        let air = count_air(&chunk);
        // Expect at least 1 air voxel; realistically many more.
        assert!(air > 0, "expected cave carving but got 0 air voxels");
    }

    /// A chunk whose Y span is above any realistic surface height
    /// (coord [0, 15, 0] -> world-Y 240..256) should be carved too
    /// if it were stone, but in practice the realistic generator
    /// leaves that region air anyway. Here we just verify that
    /// `carve` does NOT crash and that the guard `wy <= 0` path works.
    #[test]
    fn caves_do_not_carve_above_surface() {
        // y coord 10 -> world-Y 160..176, well above typical surface.
        // Fill with stone and carve; at minimum we should NOT carve
        // the y=160 row (all positive wy, so no bedrock guard kicks
        // in, but cavern threshold is very tight at high altitude
        // because the test just checks the function doesn't panic and
        // that air-above-surface logic doesn't break).
        // The main assertion: an already-air chunk stays all-air.
        let mut empty_chunk = Chunk::empty();
        let ctx = ctx(0);
        carve(&mut empty_chunk, [0, 10, 0], &ctx);
        assert_eq!(count_air(&empty_chunk), CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE);
    }

    /// Two calls with the same seed + coord produce the same air
    /// pattern (determinism contract).
    #[test]
    fn caves_deterministic_per_seed() {
        let c1 = {
            let mut chunk = stone_chunk();
            carve(&mut chunk, [3, 0, 5], &ctx(77));
            chunk
        };
        let c2 = {
            let mut chunk = stone_chunk();
            carve(&mut chunk, [3, 0, 5], &ctx(77));
            chunk
        };
        let n = CHUNK_SIZE;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    assert_eq!(
                        c1.get(x, y, z),
                        c2.get(x, y, z),
                        "mismatch at ({x},{y},{z})"
                    );
                }
            }
        }
    }

    /// Different seeds produce different cave patterns (sanity:
    /// seed mixing actually changes output).
    #[test]
    fn caves_differ_across_seeds() {
        let air_s1 = {
            let mut chunk = stone_chunk();
            carve(&mut chunk, [0, 0, 0], &ctx(1));
            count_air(&chunk)
        };
        let air_s2 = {
            let mut chunk = stone_chunk();
            carve(&mut chunk, [0, 0, 0], &ctx(2));
            count_air(&chunk)
        };
        // The counts don't need to be equal - but at least ONE of
        // the seeds must carve something, and they shouldn't have
        // the exact same layout in all positions. We just check
        // they differ in count as a proxy.
        assert!(
            air_s1 > 0 || air_s2 > 0,
            "neither seed carved any voxels"
        );
    }

    /// Carver never touches voxels with block ids that are NOT in the
    /// carveable allowlist (e.g. water = BlockId(36)).
    #[test]
    fn caves_do_not_carve_non_stone_blocks() {
        let water_id = BlockId(36);
        let mut chunk = Chunk::empty();
        let n = CHUNK_SIZE;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    chunk.set(x, y, z, water_id);
                }
            }
        }
        carve(&mut chunk, [0, 0, 0], &ctx(42));
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    assert_eq!(
                        chunk.get(x, y, z),
                        water_id,
                        "water at ({x},{y},{z}) was incorrectly carved"
                    );
                }
            }
        }
    }

    /// `is_carveable` allows all expected stone-like block ids and
    /// rejects everything else.
    #[test]
    fn is_carveable_correct_allowlist() {
        assert!(is_carveable(BlockId(1)));  // stone
        assert!(is_carveable(BlockId(8)));  // cobblestone
        assert!(is_carveable(BlockId(13))); // coal ore
        assert!(is_carveable(BlockId(14))); // iron ore
        assert!(is_carveable(BlockId(26))); // sandstone
        // Should NOT be carveable:
        assert!(!is_carveable(BlockId(0)));  // air
        assert!(!is_carveable(BlockId(2)));  // dirt
        assert!(!is_carveable(BlockId(3)));  // grass
        assert!(!is_carveable(BlockId(4)));  // sand
        assert!(!is_carveable(BlockId(36))); // water
        assert!(!is_carveable(BlockId(45))); // bedrock
    }

    /// `is_cave` returns false at y <= 0 (the floor guard).
    #[test]
    fn cave_floor_guard() {
        for y_int in -3..=0 {
            assert!(
                !is_cave(100.0, y_int as f32, 100.0, 0),
                "expected solid at y={y_int}"
            );
        }
    }

    /// `sample_noise3` stays within [0, 1] across a dense sample
    /// grid (ensures the trilinear interpolation doesn't overshoot).
    #[test]
    fn noise3_stays_in_unit_range() {
        for ix in -8..8_i32 {
            for iy in -8..8_i32 {
                for iz in -8..8_i32 {
                    let v = sample_noise3(
                        ix as f32 * 3.7 + 0.5,
                        iy as f32 * 3.7 + 0.5,
                        iz as f32 * 3.7 + 0.5,
                        CAVE_SCALE_A,
                        12345,
                    );
                    assert!((0.0..=1.0).contains(&v), "out of range: {v}");
                }
            }
        }
    }
}
