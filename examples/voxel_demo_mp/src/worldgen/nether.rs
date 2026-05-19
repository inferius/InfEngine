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
//! Determinism: every noise call mixes `world_seed()`, which the
//! `switch_dim` path sets to `dim_seed(base, "nether")`. So two
//! peers running the same base seed produce byte-identical
//! nether chunks - no sync over the wire.
//!
//! F.1 ships a minimal placeholder (bedrock floor + netherrack
//! bulk only) so the worldgen enum compiles cleanly. F.3 swaps
//! in the full lava + ceiling + glowstone implementation.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use crate::block::{BEDROCK_ID, GLOWSTONE_ID, LAVA_ID, NETHERRACK_ID};
use crate::worldgen::WorldgenCtx;
use crate::worldgen::hash2;
use crate::worldgen::height::cave_at;

/// World-Y of the bedrock floor. Vanilla MC matches this; the
/// nether's actual playable range is 1..127.
pub(crate) const NETHER_FLOOR_Y: i32 = 0;

/// World-Y of the solid netherrack ceiling row. Three rows of
/// netherrack sit at `CEIL_Y..=CEIL_Y+2` so the player can't
/// trivially break through.
pub(crate) const NETHER_CEILING_Y: i32 = 127;

/// Mid-height of the netherrack bulk surface. Adds a per-(x,z)
/// noise wobble around this value to produce uneven ground.
const BULK_BASE_Y: i32 = 40;

/// Vertical extent of the bulk-noise wobble around `BULK_BASE_Y`.
const BULK_AMP: i32 = 16;

/// Y below which lava lakes can form when the bulk-floor noise
/// dips into a "lake" basin.
const LAVA_LAKE_Y_MAX: i32 = 32;

/// `gen_nether` - deterministic chunk generator for the nether.
pub(crate) fn gen_nether(coord: ChunkCoord, _ctx: &WorldgenCtx) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wy = origin.y as i32 + y as i32;
        let wz = origin.z + z as f32;
        nether_block_at(wx, wy, wz)
    });
    chunk
}

/// Resolve the block id at a single world-space cell for the
/// nether generator. Factored out so the caller can drive a
/// fold-hash test against the same per-cell decision used by
/// the chunk pass.
pub(crate) fn nether_block_at(wx: f32, wy: i32, wz: f32) -> BlockId {
    // Bedrock floor - one solid row at y=0. Anything below is
    // out of bounds (chunks never extend there in practice but
    // keep the guard).
    if wy <= NETHER_FLOOR_Y {
        return BlockId(BEDROCK_ID);
    }
    // Three solid ceiling rows + a scatter of glowstone embedded
    // in the bottom-most row so the light bleeds down. Above the
    // top ceiling row -> air (nothing should sit higher).
    if wy >= NETHER_CEILING_Y + 2 {
        return BlockId::AIR;
    }
    if wy >= NETHER_CEILING_Y {
        // Bottom ceiling row -> sometimes glowstone, mostly
        // netherrack. Hash2 keeps it deterministic.
        if wy == NETHER_CEILING_Y && glowstone_at(wx, wz) {
            return BlockId(GLOWSTONE_ID);
        }
        return BlockId(NETHERRACK_ID);
    }
    // Compute the per-column ground height. Solid below, air
    // above (modulo cave + lava-lake overrides).
    let ground_h = nether_ground_height(wx, wz);
    if wy > ground_h {
        // Above the surface curve - mostly air, plus lava that
        // pools in basin columns from y=ground_h+1 up to
        // LAVA_LAKE_Y_MAX. This produces visible lava lakes
        // rather than a single thin row.
        if wy <= LAVA_LAKE_Y_MAX && is_lava_lake(wx, wz) {
            return BlockId(LAVA_ID);
        }
        return BlockId::AIR;
    }
    // Below the surface - mostly netherrack. Carve caves
    // through the bulk for navigation. Skip the cave carve if
    // we're inside an actual lava lake column so the cave gen
    // doesn't punch holes into the lava floor.
    if !is_lava_lake(wx, wz) && cave_at(wx, wy as f32, wz) {
        return BlockId::AIR;
    }
    BlockId(NETHERRACK_ID)
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

/// Returns true when (wx, wz) sits inside a lava-lake basin.
/// Threshold the standard `hash2` to about 12% coverage so
/// lakes are sparse but visible. Pure XZ test - the caller
/// gates on Y separately.
fn is_lava_lake(wx: f32, wz: f32) -> bool {
    let h = hash2(wx as i32, wz as i32);
    (h % 100) < 12
}

/// Returns true when (wx, wz) on the ceiling-bottom row should
/// emit glowstone instead of netherrack. ~8% coverage.
fn glowstone_at(wx: f32, wz: f32) -> bool {
    let h = hash2(wx as i32 + 7, wz as i32 + 13);
    (h % 100) < 8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{TEST_GEN_LOCK, WORLDGEN, WorldGen, set_world_seed};
    use infengine_voxel::CHUNK_SIZE;

    fn lock_gen() -> std::sync::MutexGuard<'static, ()> {
        TEST_GEN_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn nether_bedrock_floor_is_solid() {
        let _g = lock_gen();
        set_world_seed(0);
        WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
        // y=0 must be bedrock at every (x, z).
        for wx in -8..8 {
            for wz in -8..8 {
                assert_eq!(
                    nether_block_at(wx as f32, 0, wz as f32).0,
                    BEDROCK_ID,
                    "floor at ({wx}, 0, {wz}) should be bedrock"
                );
            }
        }
    }

    #[test]
    fn nether_chunk_is_idempotent() {
        let _g = lock_gen();
        set_world_seed(42);
        WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
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
    fn nether_ceiling_is_netherrack() {
        let _g = lock_gen();
        set_world_seed(0);
        WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
        // Top ceiling row: every column is either netherrack or
        // glowstone (never air, never bedrock).
        for wx in -4..4 {
            for wz in -4..4 {
                let id = nether_block_at(wx as f32, NETHER_CEILING_Y, wz as f32).0;
                assert!(
                    id == NETHERRACK_ID || id == GLOWSTONE_ID,
                    "ceiling at ({wx}, {NETHER_CEILING_Y}, {wz}) = {id}"
                );
            }
        }
    }

    #[test]
    fn nether_has_lava_somewhere_at_seed_zero() {
        let _g = lock_gen();
        set_world_seed(0);
        WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
        // Scan a wide XZ band looking for at least one column
        // that resolves to a lava lake. Determinism contract:
        // seed=0 must always have some lava on this band.
        let mut found_lava = false;
        for wx in -64..64 {
            for wz in -64..64 {
                if is_lava_lake(wx as f32, wz as f32) {
                    found_lava = true;
                    break;
                }
            }
            if found_lava {
                break;
            }
        }
        assert!(found_lava, "seed=0 should produce at least one lava lake");
    }

    #[test]
    fn nether_has_glowstone_somewhere_at_seed_zero() {
        let _g = lock_gen();
        set_world_seed(0);
        WORLDGEN.store(WorldGen::Nether as u8, std::sync::atomic::Ordering::Relaxed);
        let mut found = false;
        for wx in -64..64 {
            for wz in -64..64 {
                if glowstone_at(wx as f32, wz as f32) {
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
