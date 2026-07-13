//! W268 - nether vegetal decoration (step 9 of the biome lists):
//! huge fungi (`HugeFungusFeature`), the nylium ground cover
//! (`NetherForestVegetationFeature` - crimson/warped forest
//! vegetation + nether sprouts), weeping vines (`WeepingVinesFeature`
//! incl. the roof wart blobs) and twisting vines
//! (`TwistingVinesFeature`). Same determinism scheme as
//! `nether_features` (source-column streams, 3x3 replay, terrain +
//! own-scratch reads).

use std::sync::OnceLock;

use infengine_voxel::CHUNK_SIZE;

use crate::worldgen::WorldgenCtx;

use super::features::{block_id_by_name, stream_seed};
use super::nether_features::{
    Ids, SALT_BASE, Scratch, every_layer_positions, next_between, next_int,
};
use super::nether_mc::TOP_Y;
use super::noise::SplitMix64;

/// The vegetal block set (all verified in the registry).
pub(crate) struct VegIds {
    shroomlight: u16,
    nether_wart_block: u16,
    warped_wart_block: u16,
    crimson_stem: u16,
    warped_stem: u16,
    crimson_fungus: u16,
    warped_fungus: u16,
    crimson_roots: u16,
    warped_roots: u16,
    nether_sprouts: u16,
    weeping_vines: u16,
    twisting_vines: u16,
}

impl VegIds {
    fn get() -> Option<&'static VegIds> {
        static IDS: OnceLock<Option<VegIds>> = OnceLock::new();
        IDS.get_or_init(|| {
            Some(VegIds {
                shroomlight: block_id_by_name("shroomlight")?,
                nether_wart_block: block_id_by_name("nether_wart_block")?,
                warped_wart_block: block_id_by_name("warped_wart_block")?,
                crimson_stem: block_id_by_name("crimson_stem")?,
                warped_stem: block_id_by_name("warped_stem")?,
                crimson_fungus: block_id_by_name("crimson_fungus")?,
                warped_fungus: block_id_by_name("warped_fungus")?,
                crimson_roots: block_id_by_name("crimson_roots")?,
                warped_roots: block_id_by_name("warped_roots")?,
                nether_sprouts: block_id_by_name("nether_sprouts")?,
                weeping_vines: block_id_by_name("weeping_vines")?,
                twisting_vines: block_id_by_name("twisting_vines")?,
            })
        })
        .as_ref()
    }
}

/// The small-plant survival floor (FungusBlock / RootsBlock /
/// NetherSproutsBlock `mayPlaceOn`): nylium or soul soil.
fn plant_floor(id: u16, ids: &Ids) -> bool {
    id == ids.crimson_nylium || id == ids.warped_nylium || id == ids.soul_soil
}

// ---------------------------------------------------------------------------
// Huge fungi.
// ---------------------------------------------------------------------------

/// HugeFungusFeature (not planted): stem + wart hat with shroomlight
/// decor; 6% double-size. `count_on_every_layer 8` per forest.
#[allow(clippy::too_many_arguments)]
fn huge_fungus(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
    ids: &Ids,
    veg: &VegIds,
    warped: bool,
) {
    let base = if warped { ids.warped_nylium } else { ids.crimson_nylium };
    if scratch.get(ctx, ox, oy - 1, oz) != base {
        return;
    }
    let mut height = next_between(rng, 4, 13);
    if next_int(rng, 12) == 0 {
        height *= 2;
    }
    if oy + height + 1 > TOP_Y {
        return;
    }
    let huge = rng.next_f64() < 0.06;
    let (stem, hat) = if warped {
        (veg.warped_stem, veg.warped_wart_block)
    } else {
        (veg.crimson_stem, veg.nether_wart_block)
    };
    // The vines only hang off nether-wart hats (crimson).
    let vines = !warped;

    // Stem.
    let stem_r: i32 = if huge { 1 } else { 0 };
    for dx in -stem_r..=stem_r {
        for dz in -stem_r..=stem_r {
            let corner = huge && dx.abs() == stem_r && dz.abs() == stem_r;
            for dy in 0..height {
                let (x, y, z) = (ox + dx, oy + dy, oz + dz);
                if !scratch.is_empty(ctx, x, y, z) {
                    continue;
                }
                if corner {
                    if rng.next_f64() < 0.1 {
                        scratch.set(x, y, z, stem);
                    }
                } else {
                    scratch.set(x, y, z, stem);
                }
            }
        }
    }

    // Hat.
    let hat_height = (next_int(rng, 1 + height / 3) + 5).min(height);
    let hat_start = height - hat_height;
    for dy in hat_start..=height {
        let mut radius = if dy < height - next_int(rng, 3) { 2 } else { 1 };
        if hat_height > 8 && dy < hat_start + 4 {
            radius = 3;
        }
        if huge {
            radius += 1;
        }
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let edge_x = dx == -radius || dx == radius;
                let edge_z = dz == -radius || dz == radius;
                let inside = !edge_x && !edge_z && dy != height;
                let corner = edge_x && edge_z;
                let hat_bottom = dy < hat_start + 3;
                let (x, y, z) = (ox + dx, oy + dy, oz + dz);
                if !scratch.is_empty(ctx, x, y, z) {
                    continue;
                }
                if hat_bottom {
                    if !inside {
                        // placeHatDropBlock: continue a wart run from
                        // above, else a sparse 15% fringe.
                        if scratch.get(ctx, x, y - 1, z) == hat {
                            scratch.set(x, y, z, hat);
                        } else if rng.next_f64() < 0.15 {
                            scratch.set(x, y, z, hat);
                            if vines && next_int(rng, 11) == 0 {
                                weeping_column_below(scratch, ctx, rng, x, y - 1, z, veg);
                            }
                        }
                    }
                } else {
                    let (decor_p, hat_p, vine_p) = if inside {
                        (0.1, 0.2, 0.1)
                    } else if corner {
                        (0.01, 0.7, 0.083)
                    } else {
                        (5.0e-4, 0.98, 0.07)
                    };
                    // placeHatBlock.
                    if rng.next_f64() < decor_p {
                        scratch.set(x, y, z, veg.shroomlight);
                    } else if rng.next_f64() < hat_p {
                        scratch.set(x, y, z, hat);
                        if vines && rng.next_f64() < vine_p {
                            weeping_column_below(scratch, ctx, rng, x, y - 1, z, veg);
                        }
                    }
                }
            }
        }
    }
}

/// `HugeFungusFeature.tryPlaceWeepingVines`: a 1..5 (1-in-7 doubled)
/// vine strand below a hat block.
fn weeping_column_below(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    x: i32,
    y: i32,
    z: i32,
    veg: &VegIds,
) {
    if !scratch.is_empty(ctx, x, y, z) {
        return;
    }
    let mut goal = next_between(rng, 1, 5);
    if next_int(rng, 7) == 0 {
        goal *= 2;
    }
    place_vine_strand(scratch, ctx, x, y, z, goal, -1, veg.weeping_vines);
}

/// A weeping (dir -1) / twisting (dir +1) strand: fill empty cells,
/// stop at the goal or the first obstruction.
#[allow(clippy::too_many_arguments)]
fn place_vine_strand(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    x: i32,
    mut y: i32,
    z: i32,
    goal: i32,
    dir: i32,
    block: u16,
) {
    for _ in 0..=goal {
        if !scratch.is_empty(ctx, x, y, z) {
            break;
        }
        scratch.set(x, y, z, block);
        y += dir;
    }
}

// ---------------------------------------------------------------------------
// Nylium ground cover + vines.
// ---------------------------------------------------------------------------

/// NetherForestVegetationFeature: spread_width 8 / spread_height 4 -
/// 64 offset tries around an on-layer origin whose floor is nylium;
/// each try places a weighted plant on empty cells above a plantable
/// floor.
#[allow(clippy::too_many_arguments)]
fn forest_vegetation(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
    ids: &Ids,
    weights: &[(u16, u32)],
) {
    let below = scratch.get(ctx, ox, oy - 1, oz);
    if below != ids.crimson_nylium && below != ids.warped_nylium {
        return;
    }
    let total: u32 = weights.iter().map(|(_, w)| w).sum();
    for _ in 0..64 {
        let x = ox + next_int(rng, 8) - next_int(rng, 8);
        let y = oy + next_int(rng, 4) - next_int(rng, 4);
        let z = oz + next_int(rng, 8) - next_int(rng, 8);
        let mut roll = next_int(rng, total as i32) as u32;
        let mut state = weights[0].0;
        for (id, w) in weights {
            if roll < *w {
                state = *id;
                break;
            }
            roll -= w;
        }
        if y > 0
            && scratch.is_empty(ctx, x, y, z)
            && plant_floor(scratch.get(ctx, x, y - 1, z), ids)
        {
            scratch.set(x, y, z, state);
        }
    }
}

/// WeepingVinesFeature: a roof nether-wart blob (200 tries, exactly
/// one wart/netherrack neighbour) + 100 hanging strands under it.
#[allow(clippy::too_many_arguments)]
fn weeping_vines(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
    ids: &Ids,
    veg: &VegIds,
) {
    if !scratch.is_empty(ctx, ox, oy, oz) {
        return;
    }
    let above = scratch.get(ctx, ox, oy + 1, oz);
    if above != ids.netherrack && above != veg.nether_wart_block {
        return;
    }
    // placeRoofNetherWart.
    scratch.set(ox, oy, oz, veg.nether_wart_block);
    for _ in 0..200 {
        let x = ox + next_int(rng, 6) - next_int(rng, 6);
        let y = oy + next_int(rng, 2) - next_int(rng, 5);
        let z = oz + next_int(rng, 6) - next_int(rng, 6);
        if !scratch.is_empty(ctx, x, y, z) {
            continue;
        }
        let mut neighbours = 0;
        for (dx, dy, dz) in
            [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
        {
            let id = scratch.get(ctx, x + dx, y + dy, z + dz);
            if id == ids.netherrack || id == veg.nether_wart_block {
                neighbours += 1;
                if neighbours > 1 {
                    break;
                }
            }
        }
        if neighbours == 1 {
            scratch.set(x, y, z, veg.nether_wart_block);
        }
    }
    // placeRoofWeepingVines.
    for _ in 0..100 {
        let x = ox + next_int(rng, 8) - next_int(rng, 8);
        let y = oy + next_int(rng, 2) - next_int(rng, 7);
        let z = oz + next_int(rng, 8) - next_int(rng, 8);
        if !scratch.is_empty(ctx, x, y, z) {
            continue;
        }
        let above = scratch.get(ctx, x, y + 1, z);
        if above != ids.netherrack && above != veg.nether_wart_block {
            continue;
        }
        let mut goal = next_between(rng, 1, 8);
        if next_int(rng, 6) == 0 {
            goal *= 2;
        }
        if next_int(rng, 5) == 0 {
            goal = 1;
        }
        place_vine_strand(scratch, ctx, x, y, z, goal, -1, veg.weeping_vines);
    }
}

/// TwistingVinesFeature: 64 offset tries; each drops to the ground
/// and grows a 1..8 (doubling / stunting rolls) strand upward off
/// netherrack / warped nylium / warped wart.
#[allow(clippy::too_many_arguments)]
fn twisting_vines(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
    ids: &Ids,
    veg: &VegIds,
) {
    let invalid = |s: &mut Scratch, x: i32, y: i32, z: i32| -> bool {
        if !s.is_empty(ctx, x, y, z) {
            return true;
        }
        let below = s.get(ctx, x, y - 1, z);
        below != ids.netherrack && below != ids.warped_nylium && below != veg.warped_wart_block
    };
    if invalid(scratch, ox, oy, oz) {
        return;
    }
    for _ in 0..64 {
        let x = ox + next_between(rng, -8, 8);
        let mut y = oy + next_between(rng, -4, 4);
        let z = oz + next_between(rng, -8, 8);
        // findFirstAirBlockAboveGround.
        while scratch.is_empty(ctx, x, y - 1, z) {
            y -= 1;
            if y <= 0 {
                break;
            }
        }
        if y <= 0 || invalid(scratch, x, y, z) {
            continue;
        }
        let mut goal = next_between(rng, 1, 8);
        if next_int(rng, 6) == 0 {
            goal *= 2;
        }
        if next_int(rng, 5) == 0 {
            goal = 1;
        }
        place_vine_strand(scratch, ctx, x, y, z, goal, 1, veg.twisting_vines);
    }
}

// ---------------------------------------------------------------------------
// The step-9 dispatcher (per biome list).
// ---------------------------------------------------------------------------

/// Vegetal decoration for one source column, in biome-list order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_vegetal(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
    biome: &str,
) {
    let Some(veg) = VegIds::get() else {
        return;
    };
    let cs = CHUNK_SIZE as i32;
    match biome {
        "crimson_forest" => {
            // weeping_vines: count 10, y uniform 0..127.
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x90));
            for _ in 0..10 {
                let x = scx * cs + next_int(&mut rng, 16);
                let z = scz * cs + next_int(&mut rng, 16);
                let y = next_between(&mut rng, 0, TOP_Y);
                weeping_vines(scratch, ctx, &mut rng, x, y, z, ids, veg);
            }
            // crimson_fungi: 8 per layer.
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x91));
            for [x, y, z] in every_layer_positions(ctx, &mut rng, scx, scz, 8) {
                huge_fungus(scratch, ctx, &mut rng, x, y, z, ids, veg, false);
            }
            // crimson_forest_vegetation: 6 per layer, roots 87 /
            // crimson fungus 11 / warped fungus 1.
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x92));
            let weights = [
                (veg.crimson_roots, 87u32),
                (veg.crimson_fungus, 11),
                (veg.warped_fungus, 1),
            ];
            for [x, y, z] in every_layer_positions(ctx, &mut rng, scx, scz, 6) {
                forest_vegetation(scratch, ctx, &mut rng, x, y, z, ids, &weights);
            }
        }
        "warped_forest" => {
            // warped_fungi: 8 per layer.
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x93));
            for [x, y, z] in every_layer_positions(ctx, &mut rng, scx, scz, 8) {
                huge_fungus(scratch, ctx, &mut rng, x, y, z, ids, veg, true);
            }
            // warped_forest_vegetation: 5 per layer, warped roots 85 /
            // crimson roots 1 / warped fungus 13 / crimson fungus 1.
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x94));
            let weights = [
                (veg.warped_roots, 85u32),
                (veg.crimson_roots, 1),
                (veg.warped_fungus, 13),
                (veg.crimson_fungus, 1),
            ];
            for [x, y, z] in every_layer_positions(ctx, &mut rng, scx, scz, 5) {
                forest_vegetation(scratch, ctx, &mut rng, x, y, z, ids, &weights);
            }
            // nether_sprouts: 4 per layer, simple provider.
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x95));
            let weights = [(veg.nether_sprouts, 1u32)];
            for [x, y, z] in every_layer_positions(ctx, &mut rng, scx, scz, 4) {
                forest_vegetation(scratch, ctx, &mut rng, x, y, z, ids, &weights);
            }
            // twisting_vines: count 10, y uniform 0..127.
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x96));
            for _ in 0..10 {
                let x = scx * cs + next_int(&mut rng, 16);
                let z = scz * cs + next_int(&mut rng, 16);
                let y = next_between(&mut rng, 0, TOP_Y);
                twisting_vines(scratch, ctx, &mut rng, x, y, z, ids, veg);
            }
        }
        _ => {}
    }
}
