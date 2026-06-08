//! Ravine carver for the `Realistic` generator (W17.2).
//!
//! A ravine is a narrow, tall vertical slash through the terrain.
//! MC's canyon carver produces ravines as elongated ellipsoidal
//! carves; this implementation replicates the visual + gameplay
//! feel without literally porting `CanyonWorldCarver.java`.
//!
//! Algorithm:
//! 1. Per-chunk hash decides if a ravine centre lands in this chunk.
//!    Probability: 1/64. Only x-z position is hashed; the ravine
//!    spans multiple chunk-Y slices vertically.
//! 2. The ravine is an axis-aligned ellipsoid:
//!    - Length (x or z axis): 16-32 blocks.
//!    - Width (perpendicular axis): 4-8 blocks (half-axis).
//!    - Height: 16-40 blocks.
//!    - Random rotation: either x-aligned or z-aligned.
//! 3. All stone-like blocks inside the ellipsoid are replaced with
//!    air. The carve pass runs after `caves::carve` in `gen_realistic`.
//!
//! Ravines are seeded only from their (chunk_x, chunk_z) so the
//! same ravine centre produces consistent slices across all Y chunks.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use crate::worldgen::climate::SEA_LEVEL;
use crate::worldgen::strata::WORLD_FLOOR;
use crate::worldgen::{WorldgenCtx, hash3_for};

// ---------------------------------------------------------------
// Constants
// ---------------------------------------------------------------

/// Inverse probability that a ravine spawns anchored in a given
/// (chunk_x, chunk_z) column. 64 -> ~1/64 chunks.
const RAVINE_CHUNK_RATE: u32 = 64;

/// Minimum ravine half-length along the long axis (blocks).
const HALF_LENGTH_MIN: i32 = 8;
/// Amplitude added on top of the minimum (blocks). Range: 0..16
/// -> total half-length 8..24 (diameter 16..48).
const HALF_LENGTH_AMP: u32 = 16;

/// Minimum ravine half-width along the short axis (blocks).
const HALF_WIDTH_MIN: i32 = 2;
/// Amplitude for half-width variation (blocks). Range: 0..4
/// -> total half-width 2..6 (diameter 4..12).
const HALF_WIDTH_AMP: u32 = 4;

/// Minimum ravine half-height in blocks.
const HALF_HEIGHT_MIN: i32 = 8;
/// Amplitude for half-height variation. Range: 0..16
/// -> total half-height 8..24 (full height 16..48).
const HALF_HEIGHT_AMP: u32 = 16;

/// World-Y floor below which ravines do not carve. Keeps the bedrock
/// floor band intact. W121: relative to `WORLD_FLOOR` instead of the
/// old absolute `2`.
const RAVINE_FLOOR_Y: i32 = WORLD_FLOOR + 2;

// ---------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------

/// Salts used to derive independent random fields from the same
/// hash call site.
const SALT_SPAWN: u32 = 0xB1CA_55A1;
const SALT_CX: u32 = 0xDE4D_BEEF;
const SALT_CZ: u32 = 0xFACE_F00D;
const SALT_LEN: u32 = 0x1234_5678;
const SALT_WID: u32 = 0xABCD_EF01;
const SALT_HGT: u32 = 0xC0FF_EE11;
const SALT_ROT: u32 = 0xD1CE_D100;
const SALT_Y: u32 = 0xF1A7_0000;

/// Describes one ravine slice visible from a given chunk.
struct Ravine {
    /// World-X centre of the ravine.
    cx: i32,
    /// World-Z centre of the ravine.
    cz: i32,
    /// World-Y centre of the ravine.
    cy: i32,
    /// Half-length along the primary axis.
    hl: f32,
    /// Half-width along the perpendicular axis.
    hw: f32,
    /// Half-height.
    hh: f32,
    /// true = long axis is X, false = long axis is Z.
    x_aligned: bool,
}

impl Ravine {
    /// Returns `true` if (wx, wy, wz) is inside this ravine ellipsoid.
    fn contains(&self, wx: i32, wy: i32, wz: i32) -> bool {
        if wy <= RAVINE_FLOOR_Y {
            return false;
        }
        let dx = (wx - self.cx) as f32;
        let dy = (wy - self.cy) as f32;
        let dz = (wz - self.cz) as f32;

        let (dl, ds) = if self.x_aligned {
            (dx, dz)
        } else {
            (dz, dx)
        };

        // Ellipsoid equation: (dl/hl)^2 + (ds/hw)^2 + (dy/hh)^2 < 1
        let el = (dl / self.hl).powi(2);
        let es = (ds / self.hw).powi(2);
        let ey = (dy / self.hh).powi(2);
        el + es + ey < 1.0
    }
}

/// Attempt to generate a ravine anchored at chunk column (cx, cz).
/// Returns `None` if the hash doesn't select a ravine there.
///
/// The `seed` mixes into every hash so different world seeds produce
/// different ravine placements.
fn try_ravine_at_column(chunk_x: i32, chunk_z: i32, seed: u32) -> Option<Ravine> {
    let spawn_h = hash3_for(chunk_x, 0, chunk_z, seed ^ SALT_SPAWN);
    if !spawn_h.is_multiple_of(RAVINE_CHUNK_RATE) {
        return None;
    }
    use infengine_voxel::CHUNK_SIZE;
    let cs = CHUNK_SIZE as i32;

    // Offset within the chunk (0..cs) for the ravine centre x,z.
    let ox = (hash3_for(chunk_x, 1, chunk_z, seed ^ SALT_CX) % cs as u32) as i32;
    let oz = (hash3_for(chunk_x, 2, chunk_z, seed ^ SALT_CZ) % cs as u32) as i32;
    let cx = chunk_x * cs + ox;
    let cz = chunk_z * cs + oz;

    // Dimensions.
    let hl = HALF_LENGTH_MIN
        + (hash3_for(chunk_x, 3, chunk_z, seed ^ SALT_LEN) % HALF_LENGTH_AMP) as i32;
    let hw = HALF_WIDTH_MIN
        + (hash3_for(chunk_x, 4, chunk_z, seed ^ SALT_WID) % HALF_WIDTH_AMP) as i32;
    let hh = HALF_HEIGHT_MIN
        + (hash3_for(chunk_x, 5, chunk_z, seed ^ SALT_HGT) % HALF_HEIGHT_AMP) as i32;

    // Y centre: spread through the WHOLE underground (W121). Was a fixed
    // y10..50 band pinned below the old sea-12 surface, which crammed
    // every ravine under the waterline. Now relative to SEA_LEVEL /
    // WORLD_FLOOR: from just above the bedrock floor up to a few blocks
    // under sea level, so ravines slash the band where players dig.
    let cy_lo = WORLD_FLOOR + 8;
    let cy_hi = SEA_LEVEL - 6;
    let cy_range = (cy_hi - cy_lo).max(1) as u32;
    let cy = cy_lo + (hash3_for(chunk_x, 6, chunk_z, seed ^ SALT_Y) % cy_range) as i32;

    // Rotation.
    let x_aligned = hash3_for(chunk_x, 7, chunk_z, seed ^ SALT_ROT).is_multiple_of(2);

    Some(Ravine {
        cx,
        cz,
        cy,
        hl: hl as f32,
        hw: hw as f32,
        hh: hh as f32,
        x_aligned,
    })
}

// ---------------------------------------------------------------
// Block allowlist
// ---------------------------------------------------------------

/// Block ids the ravine carver may replace with air. Mirrors the
/// cave carver's allowlist (W121: + deepslate + full ore roster so
/// open ravine walls don't leave floating ore / un-carved deepslate).
fn is_carveable(block: BlockId) -> bool {
    matches!(
        block.0,
        1  // stone
        | 2  // dirt
        | 3  // grass
        | 8  // cobblestone
        | 26 // sandstone
        | 96 // deepslate
        | 13 | 14 | 117..=122 // stone-form ores
        | 123..=126 // deepslate ores (coal, iron, diamond, gold)
        | 246..=249 // deepslate ores (redstone, lapis, emerald, copper)
    )
}

// ---------------------------------------------------------------
// Public carve pass
// ---------------------------------------------------------------

/// Carve ravines into an already-filled chunk. Called from
/// `gen_realistic` after `caves::carve`.
///
/// A ravine anchored up to `HALF_LENGTH_MAX` blocks away in x or z
/// may extend into this chunk, so we check a neighbourhood of
/// columns around the current chunk.
pub(crate) fn carve(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    use infengine_voxel::CHUNK_SIZE;

    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let cs = CHUNK_SIZE as i32;

    // The furthest a ravine centre can be from this chunk and still
    // reach into it equals the maximum half-length + half-width.
    // HALF_LENGTH_MIN + HALF_LENGTH_AMP - 1 = 8 + 15 = 23
    // In chunk units: ceil(23/16) = 2.
    let chunk_radius = 2;
    let cx0 = coord[0];
    let cz0 = coord[2];

    let mut to_carve = Vec::new();

    for dcx in -chunk_radius..=chunk_radius {
        for dcz in -chunk_radius..=chunk_radius {
            let ravine = match try_ravine_at_column(cx0 + dcx, cz0 + dcz, seed) {
                Some(r) => r,
                None => continue,
            };

            // Check every voxel in the current chunk.
            for z in 0..cs {
                for y in 0..cs {
                    for x in 0..cs {
                        let wx = (origin.x as i32) + x;
                        let wy = (origin.y as i32) + y;
                        let wz = (origin.z as i32) + z;

                        if !ravine.contains(wx, wy, wz) {
                            continue;
                        }
                        let block =
                            chunk.get(x as usize, y as usize, z as usize);
                        if is_carveable(block) {
                            to_carve.push((x as usize, y as usize, z as usize));
                        }
                    }
                }
            }
        }
    }

    for (x, y, z) in to_carve {
        chunk.set(x, y, z, BlockId::AIR);
    }
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

    fn stone_chunk() -> Chunk {
        let mut c = Chunk::empty();
        let n = CHUNK_SIZE;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    c.set(x, y, z, BlockId(1));
                }
            }
        }
        c
    }

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

    /// Find a chunk column that DOES have a ravine for the given seed.
    fn find_ravine_column(seed: u32) -> Option<(i32, i32)> {
        for cx in -32..32_i32 {
            for cz in -32..32_i32 {
                if try_ravine_at_column(cx, cz, seed).is_some() {
                    return Some((cx, cz));
                }
            }
        }
        None
    }

    /// Find a chunk column that does NOT have a ravine for the given seed.
    fn find_no_ravine_column(seed: u32) -> Option<(i32, i32)> {
        for cx in -32..32_i32 {
            for cz in -32..32_i32 {
                if try_ravine_at_column(cx, cz, seed).is_none() {
                    return Some((cx, cz));
                }
            }
        }
        None
    }

    /// A chunk that contains a ravine centre has some voxels carved.
    #[test]
    fn ravine_carved_in_marked_chunk() {
        let seed = 42;
        let (cx, cz) = find_ravine_column(seed).expect("no ravine found in search area");
        // W121: the ravine Y centre now spreads across the whole
        // underground (y ~8..57), so sweep chunk-Y 0..=3 (world-Y 0..64)
        // to be sure we hit the slice that contains the carve.
        let ctx = ctx(seed);
        let mut total_air = 0usize;
        for chy in 0..=3 {
            let mut chunk = stone_chunk();
            carve(&mut chunk, [cx, chy, cz], &ctx);
            total_air += count_air(&chunk);
        }
        assert!(
            total_air > 0,
            "expected ravine carving at ({cx},{cz}) seed={seed} but got 0 air"
        );
    }

    /// A chunk that does not contain any ravine centre and is outside
    /// the reach of neighbouring ravines should not be carved.
    #[test]
    fn ravine_skipped_in_unmarked_chunk() {
        let seed = 42;
        // Use a column where there is no ravine, and make the coordinate
        // far from any neighbour by using a large offset.
        // We use an isolated column: pick one with no ravine where none
        // of the 5x5 neighbourhood has a ravine either.
        'outer: for cx in -200..200_i32 {
            for cz in -200..200_i32 {
                let mut any_near = false;
                'near: for dcx in -2..=2 {
                    for dcz in -2..=2 {
                        if try_ravine_at_column(cx + dcx, cz + dcz, seed).is_some() {
                            any_near = true;
                            break 'near;
                        }
                    }
                }
                if !any_near {
                    // Verify carve doesn't produce any air.
                    let ctx = ctx(seed);
                    let mut chunk = stone_chunk();
                    carve(&mut chunk, [cx, 0, cz], &ctx);
                    assert_eq!(
                        count_air(&chunk),
                        0,
                        "unexpected carving at chunk ({cx},0,{cz})"
                    );
                    break 'outer;
                }
            }
        }
    }

    /// Ravine carving is deterministic: two passes with the same
    /// ctx + coord produce the same result.
    #[test]
    fn ravine_deterministic_per_seed() {
        let seed = 7;
        let (cx, cz) =
            find_ravine_column(seed).unwrap_or((0, 0));
        let ctx = ctx(seed);
        let a = {
            let mut c = stone_chunk();
            carve(&mut c, [cx, 1, cz], &ctx);
            c
        };
        let b = {
            let mut c = stone_chunk();
            carve(&mut c, [cx, 1, cz], &ctx);
            c
        };
        let n = CHUNK_SIZE;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    assert_eq!(a.get(x, y, z), b.get(x, y, z));
                }
            }
        }
    }

    /// `try_ravine_at_column` returns `None` for columns that fail
    /// the 1/64 spawn roll.
    #[test]
    fn ravine_spawn_rate_approximately_correct() {
        let seed = 0;
        let mut spawned = 0;
        let total = 640_i32;
        for cx in 0..total {
            if try_ravine_at_column(cx, 0, seed).is_some() {
                spawned += 1;
            }
        }
        // Expected ~10; allow wide range for hash variance.
        assert!(
            spawned > 0 && spawned < 80,
            "unusual spawn count: {spawned}/640"
        );
    }

    /// The ravine floor guard prevents carving at y <= RAVINE_FLOOR_Y.
    #[test]
    fn ravine_floor_is_solid() {
        let r = Ravine {
            cx: 8,
            cz: 8,
            cy: 8,
            hl: 20.0,
            hw: 6.0,
            hh: 20.0,
            x_aligned: true,
        };
        // At y = RAVINE_FLOOR_Y, inside the ellipsoid horizontally,
        // but the floor guard should kick in.
        for y in -3..=RAVINE_FLOOR_Y {
            assert!(
                !r.contains(8, y, 8),
                "ravine carved at y={y}, should be solid"
            );
        }
    }

    /// Water blocks are not carved by ravines.
    #[test]
    fn ravine_does_not_carve_water() {
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
        let ctx = ctx(42);
        carve(&mut chunk, [0, 1, 0], &ctx);
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    assert_eq!(chunk.get(x, y, z), water_id);
                }
            }
        }
    }

    /// `find_no_ravine_column` finds a column that skips the spawn roll.
    #[test]
    fn no_ravine_column_exists() {
        let col = find_no_ravine_column(0);
        assert!(col.is_some(), "could not find any column without a ravine");
    }
}
