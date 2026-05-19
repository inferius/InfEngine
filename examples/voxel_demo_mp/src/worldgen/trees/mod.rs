//! Tree decoration pass. Splits into per-species sub-modules:
//! oak (Classic + Realistic Plains/Forest), birch (Forest accent),
//! pine (Taiga), sequoia (rare in Forest), jungle bigleaf
//! (Jungle), and acacia (Savanna). Phase E.4 brings the new
//! species online; E.1 kept the legacy oak as the only species
//! so Classic stayed byte-identical.
//!
//! Each species exposes `place_at(chunk, local_x, local_z,
//! ground, origin_y, rng_hash)`. The outer placement loop in
//! `place_realistic_from_coord` walks a margin-padded box around
//! the chunk, checks per-biome density via `hash2(...)`, then
//! dispatches to the chosen species. Each species clips its
//! writes to the current chunk (no global writes) so trees that
//! straddle a chunk boundary just drop the visible portion -
//! the next-chunk pass picks up the rest.

use infengine_voxel::{CHUNK_SIZE, Chunk, World};

use crate::worldgen::WorldgenCtx;
use crate::worldgen::biome::Biome;
use crate::worldgen::hash2_for;
use crate::worldgen::height::height_at_classic;

pub(crate) mod acacia;
pub(crate) mod birch;
pub(crate) mod jungle;
pub(crate) mod oak;
pub(crate) mod pine;
pub(crate) mod sequoia;
pub(crate) mod shapes;

/// Margin radius around the chunk that the tree placement loop
/// walks. Must be >= the maximum tree footprint half-width;
/// sequoia + jungle bigleaf are the widest (~4) so 5 leaves
/// headroom for any future species without losing tree blocks
/// near chunk boundaries.
const TREE_MARGIN: i32 = 5;

/// Classic tree pass: plains-only oak. Density: ~1 per 32 cells.
pub(crate) fn place_classic(
    chunk: &mut Chunk,
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    amp_scale: f32,
    seed: u32,
) {
    let cs = CHUNK_SIZE as i32;
    for x in -2..(cs + 2) {
        for z in -2..(cs + 2) {
            let wx = origin_x + x;
            let wz = origin_z + z;
            // Classic uses `biome_at` directly via
            // `crate::worldgen::biome::biome_at`, but `height_at_classic`
            // takes the biome enum so we route through it once.
            let biome = crate::worldgen::biome::biome_at(wx as f32 + 0.5, wz as f32 + 0.5, seed);
            if biome != Biome::Plains {
                continue;
            }
            if hash2_for(wx, wz, seed) % 32 != 7 {
                continue;
            }
            let ground = height_at_classic(wx as f32 + 0.5, wz as f32 + 0.5, biome, amp_scale);
            oak::place_at(chunk, x, z, ground, origin_y);
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

/// Realistic tree pass: walks a margin-padded box, picks a
/// species via biome, and runs that species' `place_at`. Density
/// plus species are biome-driven so Forest is thick with
/// oak / birch, Taiga is pine-only, Jungle is sparse-but-tall.
pub(crate) fn place_realistic_from_coord(
    chunk: &mut Chunk,
    coord: infengine_voxel::ChunkCoord,
    ctx: &WorldgenCtx,
) {
    use crate::worldgen::biome_table::biome_from_climate;
    use crate::worldgen::climate::{climate_at, realistic_height};
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let origin_x = origin.x as i32;
    let origin_y = origin.y as i32;
    let origin_z = origin.z as i32;
    let cs = CHUNK_SIZE as i32;
    for x in -TREE_MARGIN..(cs + TREE_MARGIN) {
        for z in -TREE_MARGIN..(cs + TREE_MARGIN) {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let climate = climate_at(wx as f32 + 0.5, wz as f32 + 0.5, seed);
            let biome = biome_from_climate(climate);
            let rng = hash2_for(wx, wz, seed);
            let Some((modulus, species)) = pick_species(biome, rng) else {
                continue;
            };
            if rng % modulus != 7 {
                continue;
            }
            let ground = realistic_height(climate, biome);
            dispatch(species, chunk, x, z, ground, origin_y, rng);
        }
    }
}

/// Tree species tag. Used by `dispatch` to pick a `place_at`
/// implementation. Kept separate from `Biome` so a biome can roll
/// for one of several species at placement time.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Species {
    Oak,
    Birch,
    Pine,
    Sequoia,
    Jungle,
    Acacia,
}

/// Returns `(density_modulus, species)` for a biome roll, or
/// `None` if the biome is treeless. The modulus is the divisor
/// used in `hash % modulus == 7` so smaller = denser; the species
/// is the dispatch tag.
///
/// `rng` lets one biome pick between species (e.g. Forest =
/// 50/50 oak/birch + a 7% sequoia chance).
pub(crate) fn pick_species(biome: Biome, rng: u32) -> Option<(u32, Species)> {
    match biome {
        // Forest: 7% sequoia, 50/50 oak/birch on the rest.
        Biome::Forest => {
            // Use a separate slice of the rng so the species roll
            // doesn't bias the density check below.
            let species_roll = (rng >> 8) % 100;
            let species = if species_roll < 7 {
                Species::Sequoia
            } else if species_roll < 53 {
                Species::Oak
            } else {
                Species::Birch
            };
            Some((10, species))
        }
        // Plains: legacy oak only, legacy density.
        Biome::Plains => Some((32, Species::Oak)),
        // Taiga: dense pines.
        Biome::Taiga => Some((9, Species::Pine)),
        // Jungle: less dense, but bigger trees. 70/30 jungle/oak.
        Biome::Jungle => {
            let species_roll = (rng >> 8) % 10;
            let species = if species_roll < 7 {
                Species::Jungle
            } else {
                Species::Oak
            };
            Some((14, species))
        }
        // Savanna: sparse acacia.
        Biome::Savanna => Some((48, Species::Acacia)),
        // Swamp: sparse oak.
        Biome::Swamp => Some((40, Species::Oak)),
        // Other biomes: treeless. (Desert handled by cactus pass;
        // Beach/Mountain/SnowyMountain/Tundra are vegetation-free.)
        _ => None,
    }
}

fn dispatch(
    species: Species,
    chunk: &mut Chunk,
    local_x: i32,
    local_z: i32,
    ground: i32,
    origin_y: i32,
    rng: u32,
) {
    match species {
        Species::Oak => oak::place_at(chunk, local_x, local_z, ground, origin_y),
        Species::Birch => birch::place_at(chunk, local_x, local_z, ground, origin_y, rng),
        Species::Pine => pine::place_at(chunk, local_x, local_z, ground, origin_y, rng),
        Species::Sequoia => sequoia::place_at(chunk, local_x, local_z, ground, origin_y),
        Species::Jungle => jungle::place_at(chunk, local_x, local_z, ground, origin_y, rng),
        Species::Acacia => acacia::place_at(chunk, local_x, local_z, ground, origin_y, rng),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_species_is_total_over_biome() {
        // Every biome must either return Some species or None;
        // panic = bug.
        for b in [
            Biome::Desert,
            Biome::Plains,
            Biome::Mountain,
            Biome::Tundra,
            Biome::Taiga,
            Biome::Forest,
            Biome::Jungle,
            Biome::Swamp,
            Biome::Savanna,
            Biome::SnowyMountain,
            Biome::Beach,
        ] {
            let _ = pick_species(b, 0);
            let _ = pick_species(b, 0xFFFF_FFFF);
        }
    }

    #[test]
    fn pick_species_forest_uses_three_species() {
        // Sweep the rng. Forest should pick at least one of each
        // species across the 65k roll range.
        let mut saw_oak = false;
        let mut saw_birch = false;
        let mut saw_sequoia = false;
        for rng in 0u32..0x1_0000 {
            let species = pick_species(Biome::Forest, rng).unwrap().1;
            match species {
                Species::Oak => saw_oak = true,
                Species::Birch => saw_birch = true,
                Species::Sequoia => saw_sequoia = true,
                _ => {}
            }
        }
        assert!(saw_oak && saw_birch && saw_sequoia);
    }

    #[test]
    fn treeless_biomes_return_none() {
        for b in [
            Biome::Desert,
            Biome::Mountain,
            Biome::Tundra,
            Biome::SnowyMountain,
            Biome::Beach,
        ] {
            assert!(pick_species(b, 0).is_none(), "biome {b:?} should be treeless");
        }
    }
}
