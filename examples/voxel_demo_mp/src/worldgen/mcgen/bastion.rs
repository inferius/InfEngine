//! W270 - bastion remnants, ported from `structure/bastion_remnant.json`
//! + `structure_set/nether_complexes.json` (26.1.2).
//!
//! Placement: the nether_complexes set is a SHARED random_spread grid
//! (spacing 27 / separation 4, salt 30084232) whose cells roll a
//! weighted pick between the fortress (weight 2) and the bastion
//! (weight 3); only the picked structure may generate there, so the
//! two never share a cell (the fortress port consumes the same helper
//! when it lands). Biome gate per `has_structure/bastion_remnant`:
//! every nether biome EXCEPT basalt_deltas.
//!
//! Layout: the W245.4 jigsaw solver - start pool
//! `minecraft:bastion/starts` (one of the four bastion type anchors),
//! size 6, max distance 80, start height absolute 33, no expansion
//! hack, no aliases. The starts pool rolls the bastion type exactly
//! like MC (units / hoglin_stable / treasure / bridge).
//!
//! Honest gaps: piglin/hoglin spawns are a mob-parity follow-up;
//! chest loot keys off the containing template path (treasure /
//! hoglin_stable / bridge / other) instead of per-chest NBT.

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::features::stream_seed;
use super::jigsaw::{self, PlacedPiece, SolveCfg};
use super::noise::SplitMix64;
use super::village;
use crate::worldgen::WorldgenCtx;

/// `structure_set/nether_complexes.json`.
pub(crate) const SPACING: i32 = 27;
pub(crate) const SEPARATION: i32 = 4;
pub(crate) const SALT_COMPLEXES_SET: u64 = 30_084_232;
/// The per-cell weighted structure pick's stream salt.
const SALT_COMPLEX_TYPE: u64 = 0xD4E6_0300;
/// Layout stream per start chunk.
const SALT_BASTION_LAYOUT: u64 = 0xD4E6_0301;

/// `structure/bastion_remnant.json`.
const START_POOL: &str = "minecraft:bastion/starts";
const START_Y: i32 = 33;
const MAX_DEPTH: i32 = 6;
const MAX_DISTANCE: i32 = 80;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// Which nether complex a grid cell rolled (fortress 2 : bastion 3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NetherComplex {
    Fortress,
    Bastion,
}

/// The candidate start chunk of a 27x27 cell + its weighted pick.
pub(crate) fn potential_start(seed: u32, grid_x: i32, grid_z: i32) -> ([i32; 2], NetherComplex) {
    let start = super::scattered::random_spread_start(
        seed,
        grid_x,
        grid_z,
        SPACING,
        SEPARATION,
        SALT_COMPLEXES_SET,
    );
    let mut r = SplitMix64::new(stream_seed(seed, grid_x, grid_z, SALT_COMPLEX_TYPE));
    let kind = if next_int(&mut r, 5) < 2 {
        NetherComplex::Fortress
    } else {
        NetherComplex::Bastion
    };
    (start, kind)
}

/// Solve (and cache) the bastion layout for a start chunk.
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
        if map.len() > 64 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Vec<PlacedPiece>> {
    let cs = CHUNK_SIZE as i32;
    // has_structure/bastion_remnant: every nether biome except
    // basalt_deltas, at the start column.
    if super::nether_features::biome_of(ctx, scx, scz) == "basalt_deltas" {
        return None;
    }
    let _ = cs;
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        scx,
        scz,
        SALT_BASTION_LAYOUT,
    ));
    jigsaw::solve_cfg(
        ctx,
        [scx, scz],
        START_POOL,
        &SolveCfg {
            max_depth: MAX_DEPTH,
            max_distance: MAX_DISTANCE,
            start_y: Some(START_Y),
            start_jigsaw: None,
            aliases: HashMap::new(),
            expansion_hack: false,
        },
        &mut rng,
    )
}

/// The per-chunk bastion pass (surface_structures step, Nether only).
pub(crate) fn place_bastions(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    // Bastions span ~5-6 chunks from their anchor (max_distance 80).
    const REACH: i32 = 7;
    let grid_x0 = (coord[0] - REACH).div_euclid(SPACING);
    let grid_x1 = (coord[0] + REACH).div_euclid(SPACING);
    let grid_z0 = (coord[2] - REACH).div_euclid(SPACING);
    let grid_z1 = (coord[2] + REACH).div_euclid(SPACING);
    for gx in grid_x0..=grid_x1 {
        for gz in grid_z0..=grid_z1 {
            let ([scx, scz], kind) = potential_start(seed, gx, gz);
            if kind != NetherComplex::Bastion {
                continue;
            }
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

/// Which bastion loot family `pos` falls in (`None` = not inside a
/// bastion piece).
pub(crate) enum BastionChest {
    Treasure,
    HoglinStable,
    Bridge,
    Other,
}

pub(crate) fn bastion_chest_at(ctx: &WorldgenCtx, pos: [i32; 3]) -> Option<BastionChest> {
    // Nether-only structure - the loot chain probes every dimension.
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Nether) {
        return None;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    const REACH: i32 = 7;
    let seed = ctx.effective_seed();
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING) {
            let ([scx, scz], kind) = potential_start(seed, gx, gz);
            if kind != NetherComplex::Bastion {
                continue;
            }
            if (scx - ccx).abs() > REACH || (scz - ccz).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            // W477: the chest cell's own LootTable nbt rules - the
            // rampart/wall chests of a treasure or stable bastion
            // carry bastion_OTHER, which the path family got wrong.
            if let Some(id) = village::layout_loot_table_at(&layout, ctx, pos) {
                let stem = id.strip_prefix("minecraft:").unwrap_or(&id);
                match stem.strip_prefix("chests/") {
                    Some("bastion_treasure") => {
                        return Some(BastionChest::Treasure);
                    }
                    Some("bastion_hoglin_stable") => {
                        return Some(BastionChest::HoglinStable);
                    }
                    Some("bastion_bridge") => return Some(BastionChest::Bridge),
                    Some("bastion_other") => return Some(BastionChest::Other),
                    _ => {}
                }
            }
            for piece in layout.iter() {
                let Some(t) = super::village_data::template(&piece.template) else {
                    continue;
                };
                let bb = jigsaw::template_bb(t.size, piece.position, piece.rotation);
                if !bb.contains(pos) {
                    continue;
                }
                let tpl = piece.template.as_str();
                return Some(if tpl.contains("treasure") {
                    BastionChest::Treasure
                } else if tpl.contains("hoglin_stable") {
                    BastionChest::HoglinStable
                } else if tpl.contains("bridge") {
                    BastionChest::Bridge
                } else {
                    BastionChest::Other
                });
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
            dim: "nether".into(),
            profile: WorldGen::Nether,
        }
    }

    /// The bastion pools + templates resolve and the solver yields a
    /// multi-piece layout somewhere in a modest grid scan.
    #[test]
    fn a_bastion_layout_solves() {
        let c = ctx();
        let mut found = None;
        'search: for gx in -8..8 {
            for gz in -8..8 {
                let ([sx, sz], kind) = potential_start(c.effective_seed(), gx, gz);
                if kind != NetherComplex::Bastion {
                    continue;
                }
                if let Some(layout) = layout_for_start(&c, sx, sz) {
                    found = Some(((sx, sz), layout.len()));
                    break 'search;
                }
            }
        }
        let ((sx, sz), pieces) = found.expect("no bastion in a 16x16 grid scan");
        assert!(pieces >= 5, "bastion at ({sx},{sz}) too small: {pieces} pieces");
    }

    /// Both complex kinds roll across cells (2:3 weighting).
    #[test]
    fn complex_pick_rolls_both_kinds() {
        let mut fortress = 0;
        let mut bastion = 0;
        for gx in 0..20 {
            for gz in 0..20 {
                match potential_start(0, gx, gz).1 {
                    NetherComplex::Fortress => fortress += 1,
                    NetherComplex::Bastion => bastion += 1,
                }
            }
        }
        assert!(fortress > 100, "fortress cells: {fortress}");
        assert!(bastion > 150, "bastion cells: {bastion}");
    }
}
