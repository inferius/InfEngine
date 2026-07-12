//! W245.5 - villages in the worldgen pipeline.
//!
//! Placement (`structure_set/villages.json`): random_spread with
//! spacing 34 / separation 8 / salt 10387312 - every 34x34-chunk grid
//! cell rolls one candidate start chunk (`origin + rand(26)` on both
//! axes, the LINEAR spread), and the variant follows the biome at the
//! start anchor (`has_structure/village_*`: plains+meadow, desert,
//! savanna, snowy_plains, taiga - the five weight-1 set entries carry
//! disjoint biome tags, so the weighted shuffle collapses to a direct
//! biome match).
//!
//! The layout comes from the W245.4 jigsaw solver, cached per start
//! chunk (pure in the effective seed). Carving mirrors MC placement:
//! pieces in generation order; every template cell in NBT order maps
//! through the W245.3 blockmap; jigsaw cells resolve to their
//! `final_state`. RIGID pieces sit at their solved Y; terrain_matching
//! pieces drape per column like the GravityProcessor(WORLD_SURFACE_WG,
//! -1): world y = floor(x, z) + local y, so a street's y=0 layer
//! replaces the turf block of each column it crosses.
//!
//! `beard_thin` terrain adaptation is approximated: rigid pieces get
//! a dirt foundation from their box floor down to the terrain (capped
//! at 12 - taller stilts read as cliff overhangs and stay open, like
//! the fading MC beard). Honest gaps: the drape reads the NATURAL
//! heightmap (sibling pieces are invisible to it - MC's live
//! heightmap sees earlier writes); feature sockets place no trees
//! yet; village chests roll the simple_dungeon table until per-
//! building tables arrive (better stocked than empty).

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::biome::McBiome;
use super::blockmap::{Mapped, map_state, map_state_string};
use super::features::stream_seed;
use super::jigsaw::{self, PlacedPiece, template_bb};
use super::noise::SplitMix64;
use super::terrain::{mc_floor_at, mc_full_biome_at};
use super::village_data;
use crate::worldgen::WorldgenCtx;

/// `structure_set/villages.json`.
const SPACING: i32 = 34;
const SEPARATION: i32 = 8;
const SALT_VILLAGE_SET: u64 = 10_387_312;
/// Layout stream per start chunk.
const SALT_VILLAGE_LAYOUT: u64 = 0xD4E6_0020;

/// Dirt for the beard-thin foundation approximation.
const FOUNDATION_ID: u16 = 2;
/// Foundations taller than this stay open (the MC beard fades out).
const FOUNDATION_MAX: i32 = 12;

/// The candidate start chunk of a 34x34 grid cell.
fn potential_start(seed: u32, grid_x: i32, grid_z: i32) -> [i32; 2] {
    let mut rng =
        SplitMix64::new(stream_seed(seed, grid_x, grid_z, SALT_VILLAGE_SET));
    let range = SPACING - SEPARATION;
    let ox = (rng.next_u64() % range as u64) as i32;
    let oz = (rng.next_u64() % range as u64) as i32;
    [grid_x * SPACING + ox, grid_z * SPACING + oz]
}

/// The start pool for the biome at the start anchor, per the five
/// disjoint `has_structure/village_*` tags.
fn start_pool_for(biome: McBiome) -> Option<&'static str> {
    Some(match biome {
        McBiome::Plains | McBiome::Meadow => "minecraft:village/plains/town_centers",
        McBiome::Desert => "minecraft:village/desert/town_centers",
        McBiome::Savanna => "minecraft:village/savanna/town_centers",
        McBiome::SnowyPlains => "minecraft:village/snowy/town_centers",
        McBiome::Taiga => "minecraft:village/taiga/town_centers",
        _ => return None,
    })
}

/// Solve (and cache) the village layout for a start chunk. `None`
/// when the cell's biome hosts no village or the start pool rolls
/// empty.
pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Vec<PlacedPiece>>> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<Vec<PlacedPiece>>>>;
    thread_local! {
        static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
    }
    let seed = ctx.effective_seed();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned()) {
        return hit;
    }
    let built = build_layout(ctx, scx, scz).map(Rc::new);
    CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() > 256 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Vec<PlacedPiece>> {
    let cs = CHUNK_SIZE as i32;
    let biome = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
    let pool = start_pool_for(biome)?;
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        scx,
        scz,
        SALT_VILLAGE_LAYOUT,
    ));
    jigsaw::solve(ctx, [scx, scz], pool, &mut rng)
}

/// The per-chunk village pass: find grid cells whose candidate start
/// is in reach, solve their layouts, carve the intersecting pieces.
/// Runs in SURFACE_STRUCTURES before the desert-well feature.
pub(crate) fn place_villages(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    // Pieces reach at most 80 blocks + a piece extent from the start
    // anchor: 7 chunks covers it.
    const REACH: i32 = 7;
    let grid_x0 = (coord[0] - REACH).div_euclid(SPACING);
    let grid_x1 = (coord[0] + REACH).div_euclid(SPACING);
    let grid_z0 = (coord[2] - REACH).div_euclid(SPACING);
    let grid_z1 = (coord[2] + REACH).div_euclid(SPACING);
    for gx in grid_x0..=grid_x1 {
        for gz in grid_z0..=grid_z1 {
            let [scx, scz] = potential_start(seed, gx, gz);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            carve_layout(&layout, chunk, ctx, origin);
        }
    }
}

/// Write every piece cell that lands in this 16^3 cube.
fn carve_layout(
    layout: &[PlacedPiece],
    chunk: &mut Chunk,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    let cube = super::mineshaft::Bb {
        x0: origin[0],
        y0: origin[1],
        z0: origin[2],
        x1: origin[0] + cs - 1,
        y1: origin[1] + cs - 1,
        z1: origin[2] + cs - 1,
    };
    for piece in layout {
        let Some(t) = village_data::template(&piece.template) else {
            continue;
        };
        // The REAL footprint (not the expansion-hack headroom box).
        let mut real_bb = template_bb(t.size, piece.position, piece.rotation);
        if piece.terrain_matching {
            // The drape can move columns anywhere on the terrain:
            // widen the y test to the full column.
            real_bb.y0 = 0;
            real_bb.y1 = 255;
        }
        // Foundations extend below the box - include them in the clip
        // test so a cube under a house still writes the dirt legs.
        let test_bb = super::mineshaft::Bb {
            y0: (real_bb.y0 - FOUNDATION_MAX).max(0),
            ..real_bb
        };
        if !test_bb.intersects(&cube) {
            continue;
        }
        for cell in &t.cells {
            let rp = jigsaw::rot_pos(piece.rotation, cell.pos);
            let wx = piece.position[0] + rp[0];
            let wz = piece.position[2] + rp[2];
            let wy = if piece.terrain_matching {
                // GravityProcessor(WORLD_SURFACE_WG, -1): the local
                // y=0 layer replaces each column's turf block.
                mc_floor_at(ctx, wx, wz) + cell.pos[1]
            } else {
                piece.position[1] + rp[1]
            };
            let (lx, ly, lz) =
                (wx - origin[0], wy - origin[1], wz - origin[2]);
            if !(0..cs).contains(&lx)
                || !(0..cs).contains(&ly)
                || !(0..cs).contains(&lz)
            {
                continue;
            }
            let mapped = match &cell.jigsaw {
                Some(j) => map_state_string(&j.final_state),
                None => map_state(&t.palette[cell.state]),
            };
            if let Mapped::Id(id) = mapped {
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
            }
        }
        // beard_thin approximation: rigid pieces stand on a dirt
        // foundation down to the terrain.
        if !piece.terrain_matching {
            let foot = template_bb(t.size, piece.position, piece.rotation);
            for wx in foot.x0..=foot.x1 {
                for wz in foot.z0..=foot.z1 {
                    if wx < cube.x0 || wx > cube.x1 || wz < cube.z0 || wz > cube.z1 {
                        continue;
                    }
                    let ground = mc_floor_at(ctx, wx, wz);
                    if ground >= foot.y0 || foot.y0 - ground > FOUNDATION_MAX {
                        continue;
                    }
                    for wy in (ground + 1)..foot.y0 {
                        let ly = wy - origin[1];
                        if (0..cs).contains(&ly) {
                            chunk.set(
                                (wx - origin[0]) as usize,
                                ly as usize,
                                (wz - origin[2]) as usize,
                                BlockId(FOUNDATION_ID),
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn potential_starts_stay_in_their_cells() {
        for gx in -3..3 {
            for gz in -3..3 {
                let [sx, sz] = potential_start(42, gx, gz);
                assert!(sx >= gx * SPACING && sx < gx * SPACING + SPACING - SEPARATION);
                assert!(sz >= gz * SPACING && sz < gz * SPACING + SPACING - SEPARATION);
                assert_eq!(potential_start(42, gx, gz), [sx, sz], "deterministic");
            }
        }
    }

    #[test]
    fn start_pools_follow_the_biome_tags() {
        assert!(start_pool_for(McBiome::Plains).unwrap().contains("plains"));
        assert!(start_pool_for(McBiome::Meadow).unwrap().contains("plains"));
        assert!(start_pool_for(McBiome::Desert).unwrap().contains("desert"));
        assert!(start_pool_for(McBiome::SnowyPlains).unwrap().contains("snowy"));
        assert!(start_pool_for(McBiome::Taiga).unwrap().contains("taiga"));
        assert_eq!(start_pool_for(McBiome::SavannaPlateau), None);
        assert_eq!(start_pool_for(McBiome::Badlands), None);
    }

    /// Probe: scan for villages near a seed/centre + full-pipeline
    /// verify the first one (real chunks must hold its blocks).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp --release
    /// --lib dump_villages -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_villages() {
        let env_i32 = |name: &str, default: i32| -> i32 {
            std::env::var(name)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        let seed: u32 = std::env::var("INFENGINE_DUMP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3592732596);
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 96);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<([i32; 2], usize)> = Vec::new();
        for gx in (ccx - radius).div_euclid(SPACING)..=(ccx + radius).div_euclid(SPACING)
        {
            for gz in
                (ccz - radius).div_euclid(SPACING)..=(ccz + radius).div_euclid(SPACING)
            {
                let [sx, sz] = potential_start(ctx.effective_seed(), gx, gz);
                let biome = mc_full_biome_at(&ctx, sx * cs + 8, sz * cs + 8);
                let Some(layout) = layout_for_start(&ctx, sx, sz) else {
                    println!("cell ({gx}, {gz}): start ({sx}, {sz}) biome {biome:?} - no village");
                    continue;
                };
                let houses = layout
                    .iter()
                    .filter(|p| p.template.contains("/houses/"))
                    .count();
                println!(
                    "village at start chunk ({sx}, {sz}) [{biome:?}]: {} pieces, {houses} houses, centre bb {:?}",
                    layout.len(),
                    layout[0].bb
                );
                found.push(([sx, sz], layout.len()));
            }
        }
        println!("total: {} villages", found.len());
        // Full-pipeline verify on the first: the town-centre cube must
        // contain SOME village-written block after generation.
        if let Some(([sx, sz], _)) = found.first() {
            let layout = layout_for_start(&ctx, *sx, *sz).expect("layout");
            let centre = layout[0].bb;
            let cx = (centre.x0 + centre.x1) / 2;
            let cz = (centre.z0 + centre.z1) / 2;
            let cy = centre.y0.max(1);
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let mut village_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        // Path blocks / planks / cobble / fences are
                        // unmistakable village artefacts on plains.
                        if crate::block::block_type(id).is_some_and(|bt| {
                            matches!(
                                bt.name,
                                "dirt_path" | "planks" | "cobble" | "oak_fence"
                                    | "spruce_log" | "glass_pane" | "hay_block"
                            )
                        }) {
                            village_blocks += 1;
                        }
                    }
                }
            }
            println!(
                "full-pipeline centre cube {coord:?}: {village_blocks} village blocks"
            );
            assert!(
                village_blocks > 0,
                "the town-centre cube generated no village blocks"
            );
        }
    }
}
