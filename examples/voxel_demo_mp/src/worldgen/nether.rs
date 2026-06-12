//! `WorldGen::Nether` chunk generator. Hellish dimension paired
//! with the overworld (Phase F).
//!
//! Layout summary:
//! - **Bedrock floor** at y=0 (block id `BEDROCK_ID`, unbreakable).
//! - **Netherrack bulk** from y=1 up to a noisy ground curve
//!   (~y=24..64). Caves carved through the bulk by 3D noise.
//! - **Lava lakes** below y=32 wherever the noise dips into a
//!   "lake" basin (uses the existing `LAVA_ID` so the standard
//!   liquid pipeline applies).
//! - **Netherrack ceiling** at y=127 (a few rows of solid rock).
//! - **Glowstone scatter** along the ceiling for ambient light.
//!
//! Determinism: every noise call mixes `ctx.effective_seed()`
//! where ctx is built per-job from `base_seed` + `"nether"`
//! (Phase F.2 ctx-first refactor). So two peers running the
//! same base seed produce byte-identical nether chunks - no
//! sync over the wire.
//!
//! F.1 ships a minimal placeholder (bedrock floor + netherrack
//! bulk only) so the worldgen enum compiles cleanly. F.3 swaps
//! in the full lava + ceiling + glowstone implementation.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use crate::block::{BEDROCK_ID, GLOWSTONE_ID, LAVA_ID, NETHERRACK_ID};
use crate::worldgen::WorldgenCtx;
use crate::worldgen::hash2_for;
use crate::worldgen::height::cave_at;
use crate::worldgen::ore::ore_hash;

/// World-Y of the bedrock floor. Vanilla MC matches this; the
/// nether's actual playable range is 1..127.
pub(crate) const NETHER_FLOOR_Y: i32 = 0;

/// World-Y of the bedrock CEILING row (W155: MC roofs the nether
/// with bedrock at y=127, jagged a few rows down - players cannot
/// break through).
pub(crate) const NETHER_CEILING_Y: i32 = 127;

/// Bottom of the solid netherrack roof band under the bedrock cap.
const ROOF_BAND_Y: i32 = 119;

/// Mid-height of the netherrack bulk surface. Adds a per-(x,z)
/// noise wobble around this value to produce uneven ground.
const BULK_BASE_Y: i32 = 40;

/// Vertical extent of the bulk-noise wobble around `BULK_BASE_Y`.
const BULK_AMP: i32 = 16;

/// W155: the LAVA SEA level. MC floods everything open at y<=31
/// with lava - a sea, not scattered per-column lakes.
const LAVA_SEA_Y: i32 = 31;

/// W155 - nether filler blocks (ids from the block registry).
const QUARTZ_ORE: u16 = 250;
const NETHER_GOLD_ORE: u16 = 251;
const SOUL_SAND: u16 = 149;
const MAGMA_BLOCK: u16 = 58;

/// `gen_nether` - deterministic chunk generator for the nether.
pub(crate) fn gen_nether(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wy = origin.y as i32 + y as i32;
        let wz = origin.z + z as f32;
        nether_block_at(wx, wy, wz, seed)
    });
    chunk
}

/// Resolve the block id at a single world-space cell for the
/// nether generator. Factored out so the caller can drive a
/// fold-hash test against the same per-cell decision used by
/// the chunk pass.
///
/// F.2c-2: takes an explicit seed so the noise calls are pure;
/// callers thread `ctx.effective_seed()` through.
pub(crate) fn nether_block_at(wx: f32, wy: i32, wz: f32, seed: u32) -> BlockId {
    let xi = wx as i32;
    let zi = wz as i32;
    // Bedrock floor: solid row at y=0 + a jagged ramp above it,
    // exactly like the overworld floor treatment.
    if wy <= NETHER_FLOOR_Y {
        return BlockId(BEDROCK_ID);
    }
    if wy <= NETHER_FLOOR_Y + 3 {
        let keep = (4 - wy) as f32 / 4.0;
        if ore_hash(xi, wy, zi, 0xBED0_F100, seed) < keep {
            return BlockId(BEDROCK_ID);
        }
        // fall through to bulk fill below.
    }
    // W155: bedrock CEILING. Solid cap at y=127, jagged bedrock
    // mixed into the rows below (MC's 123..127 roof), then a
    // netherrack roof band down to ROOF_BAND_Y with glowstone
    // scattered along its underside.
    if wy > NETHER_CEILING_Y {
        return BlockId::AIR;
    }
    if wy == NETHER_CEILING_Y {
        return BlockId(BEDROCK_ID);
    }
    if wy >= NETHER_CEILING_Y - 4 {
        let keep = (wy - (NETHER_CEILING_Y - 5)) as f32 / 5.0;
        if ore_hash(xi, wy, zi, 0xBED0_C311, seed) < keep {
            return BlockId(BEDROCK_ID);
        }
        return BlockId(NETHERRACK_ID);
    }
    if wy >= ROOF_BAND_Y {
        if wy == ROOF_BAND_Y && glowstone_at(wx, wz, seed) {
            return BlockId(GLOWSTONE_ID);
        }
        return BlockId(NETHERRACK_ID);
    }
    // W155: glowstone clusters hanging in the open space under the
    // roof (coarse-cell blobs, chunk-independent like the W149
    // underground blobs).
    if wy >= 96 && glowstone_blob_at(xi, wy, zi, seed) {
        return BlockId(GLOWSTONE_ID);
    }
    // Per-column ground height. Solid below, air above (modulo
    // the cave carve + the lava sea).
    let ground_h = nether_ground_height(wx, wz);
    if wy > ground_h {
        // W155: the LAVA SEA - everything open at y<=31 floods
        // with lava, like MC's nether ocean.
        if wy <= LAVA_SEA_Y {
            return BlockId(LAVA_ID);
        }
        return BlockId::AIR;
    }
    // Caves through the bulk; below the sea level a carved cell
    // floods with lava instead of air (MC caves there are lava
    // tubes, never dry).
    if cave_at(wx, wy as f32, wz) {
        return if wy <= LAVA_SEA_Y {
            BlockId(LAVA_ID)
        } else {
            BlockId::AIR
        };
    }
    // W155: surface dressing + bulk fillers.
    // Soul sand patches sit in the top rows of low-lying ground
    // (MC soul sand valleys hug the lava sea level).
    if wy >= ground_h - 1 && ground_h <= LAVA_SEA_Y + 6 && soul_patch(xi, zi, seed) {
        return BlockId(SOUL_SAND);
    }
    // Magma sprinkles near the lava shoreline.
    if wy >= ground_h - 1
        && wy <= LAVA_SEA_Y + 2
        && ore_hash(xi, wy, zi, 0x3A63_0001, seed) < 0.10
    {
        return BlockId(MAGMA_BLOCK);
    }
    // Quartz (common) + nether gold (rare) through the bulk.
    if ore_hash(xi, wy, zi, 0x90A2_7200, seed) < 0.045 {
        return BlockId(QUARTZ_ORE);
    }
    if ore_hash(xi, wy, zi, 0x901D_7201, seed) < 0.012 {
        return BlockId(NETHER_GOLD_ORE);
    }
    BlockId(NETHERRACK_ID)
}

/// W155: coarse-cell hanging glowstone clusters (8^3 cells, ~1 in
/// 14 cells in the under-roof band hosts a small blob).
fn glowstone_blob_at(wx: i32, wy: i32, wz: i32, seed: u32) -> bool {
    let cx = wx.div_euclid(8);
    let cy = wy.div_euclid(8);
    let cz = wz.div_euclid(8);
    let h = ore_hash(cx, cy, cz, 0x910A_5703, seed);
    if h > 0.07 {
        return false;
    }
    let bits = (h * 16_777_216.0) as u32;
    let centre = [
        cx * 8 + 3 + (bits & 1) as i32,
        cy * 8 + 3 + ((bits >> 1) & 1) as i32,
        cz * 8 + 3 + ((bits >> 2) & 1) as i32,
    ];
    let dx = wx - centre[0];
    let dy = wy - centre[1];
    let dz = wz - centre[2];
    dx * dx + dy * dy + dz * dz <= 4
}

/// W155: soul-sand patch mask (XZ cells ~12% near the sea level).
fn soul_patch(wx: i32, wz: i32, seed: u32) -> bool {
    hash2_for(wx ^ 0x50A1, wz ^ 0x5A2D, seed) % 100 < 12
}

/// Per-column ground height for the nether bulk. Same family
/// of sin/cos noise as the overworld height curve so the
/// determinism contract carries over.
fn nether_ground_height(wx: f32, wz: f32) -> i32 {
    let a = (wx * 0.07).sin() * 0.6 + (wz * 0.09).cos() * 0.6;
    let b = ((wx + wz) * 0.05 + 11.0).sin() * 0.5;
    let h = BULK_BASE_Y as f32 + (a + b) * BULK_AMP as f32 * 0.5;
    h.round() as i32
}

/// Returns true when (wx, wz) on the ceiling-bottom row should
/// emit glowstone instead of netherrack. ~8% coverage.
fn glowstone_at(wx: f32, wz: f32, seed: u32) -> bool {
    let h = hash2_for(wx as i32 + 7, wz as i32 + 13, seed);
    (h % 100) < 8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::WorldGen;
    use infengine_voxel::CHUNK_SIZE;
    // W5.1b: nether helpers take seed as a parameter; no globals
    // to lock or seed.

    #[test]
    fn nether_bedrock_floor_is_solid() {
        // y=0 must be bedrock at every (x, z).
        for wx in -8..8 {
            for wz in -8..8 {
                assert_eq!(
                    nether_block_at(wx as f32, 0, wz as f32, 0).0,
                    BEDROCK_ID,
                    "floor at ({wx}, 0, {wz}) should be bedrock"
                );
            }
        }
    }

    #[test]
    fn nether_chunk_is_idempotent() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 42,
            dim: String::new(),
            profile: WorldGen::Nether,
        };
        let a = gen_nether([0, 0, 0], &ctx);
        let b = gen_nether([0, 0, 0], &ctx);
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
                }
            }
        }
    }

    #[test]
    fn nether_ceiling_is_bedrock() {
        // W155: the roof cap row is solid bedrock (unbreakable),
        // and the row above it is out-of-world air.
        for wx in -4..4 {
            for wz in -4..4 {
                let id = nether_block_at(wx as f32, NETHER_CEILING_Y, wz as f32, 0).0;
                assert_eq!(id, BEDROCK_ID, "cap at ({wx},{NETHER_CEILING_Y},{wz})");
                let above =
                    nether_block_at(wx as f32, NETHER_CEILING_Y + 1, wz as f32, 0);
                assert!(above.is_air(), "above the cap must be air");
            }
        }
    }

    #[test]
    fn nether_has_a_lava_sea_at_seed_zero() {
        // W155: every OPEN cell at y<=31 floods with lava. Find a
        // low-ground column and assert the cell above its surface
        // is lava, plus that mid-air above the sea level is air.
        let mut checked = false;
        for wx in -96..96 {
            for wz in -96..96 {
                let g = nether_ground_height(wx as f32, wz as f32);
                if g < LAVA_SEA_Y - 2 {
                    let id = nether_block_at(wx as f32, g + 1, wz as f32, 0).0;
                    assert_eq!(id, LAVA_ID, "open cell above low ground = lava");
                    checked = true;
                    break;
                }
            }
            if checked {
                break;
            }
        }
        assert!(checked, "seed-independent terrain must dip under the sea");
    }

    #[test]
    fn nether_has_glowstone_somewhere_at_seed_zero() {
        let mut found = false;
        for wx in -64..64 {
            for wz in -64..64 {
                if glowstone_at(wx as f32, wz as f32, 0) {
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
        assert!(found, "seed=0 should produce at least one glowstone tile");
    }
}
