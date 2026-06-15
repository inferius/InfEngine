//! Cactus pass: sparse 2-3 block columns on desert sand. Same
//! surface + air checks as flowers; height varies with the cell
//! hash so dunes don't look uniform.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, World};

use super::WorldgenCtx;
use super::biome::{Biome, biome_at};
use super::hash2_for;
use super::height::height_at_classic;

/// Sand block id - the only valid cactus support (matches MC's
/// `BlockTags.SAND` check in `CactusBlock.canSurvive`).
const SAND_ID: u16 = 4;
const CACTUS_ID: u16 = 27;

/// Place one cactus column standing on `ground` (the world-Y of the
/// support block) at local `(x, z)`.
///
/// W188: ports MC's `CactusBlock.canSurvive` instead of an ad-hoc
/// "skip if underwater" gate: the support must be sand, and the column
/// grows UP only through air, stopping at the first non-air cell. A
/// submerged desert seabed therefore grows nothing - the cell above the
/// sand is water, not air, so the column never starts - and the same
/// rule rejects a column blocked by terrain. No special-case Y checks.
fn place_cactus_column(chunk: &mut Chunk, x: usize, z: usize, ground: i32, origin_y: i32, hash: u32) {
    let cs = CHUNK_SIZE as i32;
    // Support must be sand. Only checkable when `ground` lies in this
    // chunk; if it sits one chunk below, the desert surface is sand by
    // construction, so fall through to the air checks.
    let support_local = ground - origin_y;
    if (0..cs).contains(&support_local) && chunk.get(x, support_local as usize, z).0 != SAND_ID {
        return;
    }
    let height_voxels = 2 + ((hash >> 4) % 2) as i32; // 2 or 3
    for step in 1..=height_voxels {
        let local_y = ground + step - origin_y;
        if !(0..cs).contains(&local_y) {
            continue;
        }
        // Grow only through air: water (a submerged column) or any solid
        // stops the cactus, exactly like canSurvive failing over an
        // ocean seabed or against terrain.
        if !chunk.get(x, local_y as usize, z).is_air() {
            break;
        }
        chunk.set(x, local_y as usize, z, BlockId(CACTUS_ID));
    }
}

pub(crate) fn place_classic(
    chunk: &mut Chunk,
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    amp_scale: f32,
    seed: u32,
) {
    let cs = CHUNK_SIZE as i32;
    for x in 0..cs {
        for z in 0..cs {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let biome = biome_at(wx as f32 + 0.5, wz as f32 + 0.5, seed);
            if biome != Biome::Desert {
                continue;
            }
            let h = hash2_for(wx ^ 0xCAC7, wz ^ 0x1A55, seed);
            if h % 24 != 5 {
                continue;
            }
            let ground = height_at_classic(wx as f32 + 0.5, wz as f32 + 0.5, biome, amp_scale);
            place_cactus_column(chunk, x as usize, z as usize, ground, origin_y, h);
        }
    }
}

pub(crate) fn place_classic_from_coord(
    chunk: &mut Chunk,
    coord: infengine_voxel::ChunkCoord,
    amp_scale: f32,
    ctx: &WorldgenCtx,
) {
    let origin = World::chunk_world_origin(coord);
    place_classic(
        chunk,
        origin.x as i32,
        origin.y as i32,
        origin.z as i32,
        amp_scale,
        ctx.effective_seed(),
    );
}

/// Realistic-profile cactus pass. Cactus columns sit on Desert
/// sand (climate-driven). Skips Savanna for now - acacia trees
/// in E.4b will handle that biome's vertical decoration.
pub(crate) fn place_realistic_from_coord(
    chunk: &mut Chunk,
    coord: infengine_voxel::ChunkCoord,
    ctx: &WorldgenCtx,
) {
    use super::chunk::column_height;
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let origin_x = origin.x as i32;
    let origin_y = origin.y as i32;
    let origin_z = origin.z as i32;
    let cs = CHUNK_SIZE as i32;
    for x in 0..cs {
        for z in 0..cs {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let biome = super::realistic_biome_at(ctx, wx as f32 + 0.5, wz as f32 + 0.5);
            if biome != Biome::Desert {
                continue;
            }
            let h = hash2_for(wx ^ 0xCAC7, wz ^ 0x1A55, seed);
            if h % 24 != 5 {
                continue;
            }
            // W78: profile-correct column height so cacti sit on the
            // real terrain (Natural fills with natural_height, not
            // realistic_height).
            let ground = column_height(ctx, wx, wz);
            place_cactus_column(chunk, x as usize, z as usize, ground, origin_y, h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MC `CactusBlock.canSurvive` port: cactus grows on sand into air,
    /// but a submerged column (water above the sand) grows nothing - no
    /// ad-hoc sea-level check needed.
    #[test]
    fn cactus_grows_on_dry_sand_not_underwater() {
        // Dry: sand at y=5, air above -> cactus appears at y=6.
        let mut dry = Chunk::empty();
        dry.set(5, 5, 5, BlockId(SAND_ID));
        place_cactus_column(&mut dry, 5, 5, 5, 0, 0);
        assert_eq!(dry.get(5, 6, 5).0, CACTUS_ID, "cactus should grow on dry sand");

        // Submerged: same sand, but water above -> the column never
        // starts, the water stays put.
        let mut wet = Chunk::empty();
        wet.set(5, 5, 5, BlockId(SAND_ID));
        wet.set(5, 6, 5, BlockId(36)); // water
        place_cactus_column(&mut wet, 5, 5, 5, 0, 0);
        assert_eq!(wet.get(5, 6, 5).0, 36, "underwater column must stay water");

        // Wrong support: stone, not sand -> nothing grows.
        let mut stone = Chunk::empty();
        stone.set(5, 5, 5, BlockId(1));
        place_cactus_column(&mut stone, 5, 5, 5, 0, 0);
        assert_eq!(stone.get(5, 6, 5).0, 0, "cactus needs sand support");
    }
}
