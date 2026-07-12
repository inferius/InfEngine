//! W261 - the ancient city, ported from `structure/ancient_city.json`
//! + `structure_set/ancient_cities.json` (26.1.2).
//!
//! Placement: random_spread spacing 24 / separation 8 (salt 20083232
//! reused as the engine stream salt, the villages convention), gated
//! on the deep-dark region check (`has_structure/ancient_city` = the
//! deep_dark biome; `OverworldBiomeBuilder.isDeepDarkRegion` = 3D
//! erosion/depth thresholds). The layout is the W245.4 jigsaw solver
//! with the W261 config: absolute start height -27, no heightmap
//! projection, `start_jigsaw_name` city_anchor (the start template
//! shifts so that jigsaw lands on the anchor column), size 7,
//! max_distance_from_center 116, no expansion hack.
//!
//! Degradation: the pool elements reference the ancient_city_*
//! processor lists; the shared carve pass executes them per cell
//! (block_rot 0.95 over #ancient_city_replaceable -> ruin holes,
//! cracked-brick / snuffed-lantern rules).
//!
//! Honest gaps: `feature_pool_element` sockets (sculk patches) are
//! consumed without placing - the sculk growth is a follow-up wave;
//! the deep-dark gate samples the anchor column at the start height
//! (MC samples the solved centre piece's middle - a few blocks off);
//! `terrain_adaptation: beard_box` is not modelled (city rooms carve
//! their own space out of the deep stone; the beard fill would only
//! smooth the outer shell seam).

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::features::stream_seed;
use super::jigsaw::{self, PlacedPiece, SolveCfg};
use super::noise::SplitMix64;
use super::terrain::mc_is_deep_dark;
use super::village;
use crate::worldgen::WorldgenCtx;

/// `structure_set/ancient_cities.json`.
const SPACING: i32 = 24;
const SEPARATION: i32 = 8;
const SALT_CITY_SET: u64 = 20_083_232;
/// Layout stream per start chunk.
const SALT_CITY_LAYOUT: u64 = 0xD4E6_00E0;

/// `structure/ancient_city.json`.
const START_POOL: &str = "minecraft:ancient_city/city_center";
const START_JIGSAW: &str = "minecraft:city_anchor";
const START_Y: i32 = -27;
const MAX_DEPTH: i32 = 7;
const MAX_DISTANCE: i32 = 116;

/// The candidate start chunk of a 24x24 grid cell.
pub(crate) fn potential_start(seed: u32, grid_x: i32, grid_z: i32) -> [i32; 2] {
    super::scattered::random_spread_start(
        seed,
        grid_x,
        grid_z,
        SPACING,
        SEPARATION,
        SALT_CITY_SET,
    )
}

/// Solve (and cache) the city layout for a start chunk. `None` when
/// the anchor column is not deep-dark region or the pool rolls empty.
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
        if map.len() > 128 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Vec<PlacedPiece>> {
    let cs = CHUNK_SIZE as i32;
    // The deep-dark gate at the anchor column, at the start height.
    if !mc_is_deep_dark(ctx, scx * cs + 8, START_Y, scz * cs + 8) {
        return None;
    }
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        scx,
        scz,
        SALT_CITY_LAYOUT,
    ));
    jigsaw::solve_cfg(
        ctx,
        [scx, scz],
        START_POOL,
        &SolveCfg {
            max_depth: MAX_DEPTH,
            max_distance: MAX_DISTANCE,
            start_y: Some(START_Y),
            start_jigsaw: Some(START_JIGSAW),
            aliases: std::collections::HashMap::new(),
            // W261 shipped with the hack active (established layout);
            // the city JSON says false - noted divergence.
            expansion_hack: true,
        },
        &mut rng,
    )
}

/// The per-chunk ancient-city pass (underground_decoration step).
pub(crate) fn place_ancient_cities(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    // Pieces reach 116 blocks + a piece extent from the anchor:
    // 10 chunks covers it.
    const REACH: i32 = 10;
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
            village::carve_layout(&layout, chunk, ctx, origin);
        }
    }
}

/// W261: is `pos` inside an ancient-city piece? The worker-side chest
/// membership probe (city chests roll the ancient_city table; the
/// ice-box chest rolls ancient_city_ice_box).
pub(crate) enum CityChest {
    Regular,
    IceBox,
}

pub(crate) fn city_chest_at(ctx: &WorldgenCtx, pos: [i32; 3]) -> Option<CityChest> {
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    const REACH: i32 = 10;
    let seed = ctx.effective_seed();
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING)
        {
            let [scx, scz] = potential_start(seed, gx, gz);
            if (scx - ccx).abs() > REACH || (scz - ccz).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            for piece in layout.iter() {
                let t = super::village_data::template(&piece.template);
                let Some(t) = t else { continue };
                let real_bb =
                    jigsaw::template_bb(t.size, piece.position, piece.rotation);
                if real_bb.contains(pos) {
                    return Some(if piece.template.contains("ice_box") {
                        CityChest::IceBox
                    } else {
                        CityChest::Regular
                    });
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::WorldGen;

    fn ctx() -> WorldgenCtx {
        WorldgenCtx {
            base_seed: 3592732596u32,
            dim: "overworld".into(),
            profile: WorldGen::Mc,
        }
    }

    #[test]
    fn potential_starts_stay_in_their_cells() {
        for gx in -3..3 {
            for gz in -3..3 {
                let [sx, sz] = potential_start(7, gx, gz);
                assert!(sx >= gx * SPACING && sx < gx * SPACING + SPACING - SEPARATION);
                assert!(sz >= gz * SPACING && sz < gz * SPACING + SPACING - SEPARATION);
                assert_eq!(potential_start(7, gx, gz), [sx, sz], "deterministic");
            }
        }
    }

    /// The city pools + templates resolve from the zip and the
    /// solver produces a multi-piece layout somewhere in a deep-dark
    /// search area (pure layout check, no chunk gen).
    #[test]
    fn a_city_layout_solves_in_a_deep_dark_cell() {
        let c = ctx();
        let mut found = None;
        'search: for gx in -20..20 {
            for gz in -20..20 {
                let [sx, sz] = potential_start(c.effective_seed(), gx, gz);
                if let Some(layout) = layout_for_start(&c, sx, sz) {
                    found = Some(((sx, sz), layout.len()));
                    break 'search;
                }
            }
        }
        let ((sx, sz), pieces) = found.expect("no ancient city in a 40x40 grid scan");
        assert!(pieces > 10, "city at ({sx},{sz}) too small: {pieces} pieces");
    }

    /// Probe: scan for cities + full-pipeline verify the first one.
    /// `cargo test -p voxel_demo_mp --release --lib
    /// dump_ancient_cities -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_ancient_cities() {
        let c = ctx();
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<[i32; 2]> = Vec::new();
        for gx in -20..20 {
            for gz in -20..20 {
                let [sx, sz] = potential_start(c.effective_seed(), gx, gz);
                let Some(layout) = layout_for_start(&c, sx, sz) else {
                    continue;
                };
                let centre = layout[0].bb;
                println!(
                    "city at start chunk ({sx},{sz}): {} pieces, centre bb {:?}",
                    layout.len(),
                    centre
                );
                found.push([sx, sz]);
            }
        }
        println!("total: {} cities in the 40x40 grid scan", found.len());
        if let Some([sx, sz]) = found.first() {
            let layout = layout_for_start(&c, *sx, *sz).expect("layout");
            let centre = layout[0].bb;
            let cx = (centre.x0 + centre.x1) / 2;
            let cz = (centre.z0 + centre.z1) / 2;
            let cy = (centre.y0 + centre.y1) / 2;
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &c);
            let mut city_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if crate::block::block_type(id).is_some_and(|bt| {
                            matches!(
                                bt.name,
                                "deepslate_bricks"
                                    | "deepslate_tiles"
                                    | "cracked_deepslate_bricks"
                                    | "cracked_deepslate_tiles"
                                    | "polished_deepslate"
                                    | "polished_basalt"
                                    | "soul_lantern"
                            )
                        }) {
                            city_blocks += 1;
                        }
                    }
                }
            }
            println!("full-pipeline centre cube {coord:?}: {city_blocks} city blocks");
            assert!(city_blocks > 0, "the city centre cube generated no city blocks");
        }
    }
}
