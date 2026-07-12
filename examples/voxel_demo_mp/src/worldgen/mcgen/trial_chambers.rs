//! W263 - trial chambers, ported from `structure/trial_chambers.json`
//! + `structure_set/trial_chambers.json` (26.1.2).
//!
//! Placement: random_spread spacing 34 / separation 12 (salt 94251327
//! as the engine stream salt). The `has_structure/trial_chambers` tag
//! lists every overworld biome EXCEPT deep_dark, so the biome gate
//! collapses to "not the deep-dark region" at the start point (the
//! 3D check - dripstone/lush caves and surface biomes all qualify).
//!
//! Layout: the W245.4 jigsaw solver with size 20, max distance 116,
//! a start height rolled uniform -40..=-20 (no heightmap projection,
//! no start jigsaw, no expansion hack), and the W263 POOL ALIASES -
//! the trial-spawner content pools bind per structure instance
//! (`PoolAliasLookup`): one random_group pick ties ranged+slow_ranged
//! to the same mob family (skeleton / stray / poison_skeleton), two
//! random picks bind melee (zombie / husk / spider) and small_melee
//! (slime / cave_spider / silverfish / baby_zombie).
//!
//! Honest gaps: trial spawners + vaults carve as decorative shells
//! (the trial-wave gameplay + reward loot is a mob-parity follow-up);
//! `terrain_adaptation: encapsulate` is not modelled; dispensers /
//! pots / tripwire traps follow their KNOWN_SKIPS status; chest loot
//! keys off the containing piece's template path (corridor /
//! entrance / intersection / supply) instead of the per-chest NBT.

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

/// `structure_set/trial_chambers.json`.
const SPACING: i32 = 34;
const SEPARATION: i32 = 12;
const SALT_CHAMBERS_SET: u64 = 94_251_327;
/// Layout stream per start chunk.
const SALT_CHAMBERS_LAYOUT: u64 = 0xD4E6_00F0;

/// `structure/trial_chambers.json`.
const START_POOL: &str = "minecraft:trial_chambers/chamber/end";
const START_Y_MIN: i32 = -40;
const START_Y_MAX: i32 = -20;
const MAX_DEPTH: i32 = 20;
const MAX_DISTANCE: i32 = 116;

/// The `pool_aliases` tables: the random_group ranged families (the
/// pair binds together) and the two independent random picks.
const RANGED_FAMILIES: [(&str, &str); 3] = [
    (
        "minecraft:trial_chambers/spawner/ranged/skeleton",
        "minecraft:trial_chambers/spawner/slow_ranged/skeleton",
    ),
    (
        "minecraft:trial_chambers/spawner/ranged/stray",
        "minecraft:trial_chambers/spawner/slow_ranged/stray",
    ),
    (
        "minecraft:trial_chambers/spawner/ranged/poison_skeleton",
        "minecraft:trial_chambers/spawner/slow_ranged/poison_skeleton",
    ),
];
const MELEE: [&str; 3] = [
    "minecraft:trial_chambers/spawner/melee/zombie",
    "minecraft:trial_chambers/spawner/melee/husk",
    "minecraft:trial_chambers/spawner/melee/spider",
];
const SMALL_MELEE: [&str; 4] = [
    "minecraft:trial_chambers/spawner/small_melee/slime",
    "minecraft:trial_chambers/spawner/small_melee/cave_spider",
    "minecraft:trial_chambers/spawner/small_melee/silverfish",
    "minecraft:trial_chambers/spawner/small_melee/baby_zombie",
];

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// The candidate start chunk of a 34x34 grid cell.
pub(crate) fn potential_start(seed: u32, grid_x: i32, grid_z: i32) -> [i32; 2] {
    super::scattered::random_spread_start(
        seed,
        grid_x,
        grid_z,
        SPACING,
        SEPARATION,
        SALT_CHAMBERS_SET,
    )
}

/// Solve (and cache) the chambers layout for a start chunk.
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
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        scx,
        scz,
        SALT_CHAMBERS_LAYOUT,
    ));
    // start_height uniform [-40..=-20] (rolled before the biome gate
    // so the stream stays aligned whatever the gate says).
    let start_y = START_Y_MIN + next_int(&mut rng, START_Y_MAX - START_Y_MIN + 1);
    // has_structure/trial_chambers = every overworld biome except
    // deep_dark: the 3D deep-dark check at the start point IS the
    // tag test.
    if mc_is_deep_dark(ctx, scx * cs + 8, start_y, scz * cs + 8) {
        return None;
    }
    // pool_aliases: one family group + two independent picks.
    let fam = RANGED_FAMILIES[next_int(&mut rng, 3) as usize];
    let melee = MELEE[next_int(&mut rng, 3) as usize];
    let small = SMALL_MELEE[next_int(&mut rng, 4) as usize];
    let mut aliases = HashMap::new();
    aliases.insert(
        "minecraft:trial_chambers/spawner/contents/ranged".to_string(),
        fam.0.to_string(),
    );
    aliases.insert(
        "minecraft:trial_chambers/spawner/contents/slow_ranged".to_string(),
        fam.1.to_string(),
    );
    aliases.insert(
        "minecraft:trial_chambers/spawner/contents/melee".to_string(),
        melee.to_string(),
    );
    aliases.insert(
        "minecraft:trial_chambers/spawner/contents/small_melee".to_string(),
        small.to_string(),
    );
    jigsaw::solve_cfg(
        ctx,
        [scx, scz],
        START_POOL,
        &SolveCfg {
            max_depth: MAX_DEPTH,
            max_distance: MAX_DISTANCE,
            start_y: Some(start_y),
            start_jigsaw: None,
            aliases,
            expansion_hack: false,
        },
        &mut rng,
    )
}

/// The per-chunk trial-chambers pass (underground_structures step).
pub(crate) fn place_trial_chambers(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
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

/// Which chest table `pos` rolls, by the containing piece's template
/// path (`None` = not inside the chambers).
pub(crate) enum ChamberChest {
    Corridor,
    Entrance,
    Intersection,
    Supply,
}

pub(crate) fn chamber_chest_at(ctx: &WorldgenCtx, pos: [i32; 3]) -> Option<ChamberChest> {
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
                let Some(t) = super::village_data::template(&piece.template) else {
                    continue;
                };
                let bb = jigsaw::template_bb(t.size, piece.position, piece.rotation);
                if !bb.contains(pos) {
                    continue;
                }
                let tpl = piece.template.as_str();
                return Some(if tpl.contains("corridor") {
                    ChamberChest::Corridor
                } else if tpl.contains("entrance") {
                    ChamberChest::Entrance
                } else if tpl.contains("intersection") {
                    ChamberChest::Intersection
                } else {
                    ChamberChest::Supply
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
            dim: "overworld".into(),
            profile: WorldGen::Mc,
        }
    }

    /// The chambers pools + templates resolve (aliases included) and
    /// the solver produces a large layout.
    #[test]
    fn a_chambers_layout_solves() {
        let c = ctx();
        let mut found = None;
        'search: for gx in -6..6 {
            for gz in -6..6 {
                let [sx, sz] = potential_start(c.effective_seed(), gx, gz);
                if let Some(layout) = layout_for_start(&c, sx, sz) {
                    found = Some(((sx, sz), layout.len()));
                    break 'search;
                }
            }
        }
        let ((sx, sz), pieces) = found.expect("no trial chambers in a 12x12 grid scan");
        assert!(pieces > 20, "chambers at ({sx},{sz}) too small: {pieces} pieces");
    }

    /// Probe: scan + full-pipeline verify the first chambers.
    /// `cargo test -p voxel_demo_mp --release --lib
    /// dump_trial_chambers -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_trial_chambers() {
        let c = ctx();
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<[i32; 2]> = Vec::new();
        for gx in -10..10 {
            for gz in -10..10 {
                let [sx, sz] = potential_start(c.effective_seed(), gx, gz);
                let Some(layout) = layout_for_start(&c, sx, sz) else {
                    continue;
                };
                let spawners = layout
                    .iter()
                    .filter(|p| p.template.contains("spawner"))
                    .count();
                println!(
                    "chambers at start chunk ({sx},{sz}): {} pieces, {spawners} spawner pieces, centre bb {:?}",
                    layout.len(),
                    layout[0].bb
                );
                found.push([sx, sz]);
            }
        }
        println!("total: {} chambers in the 20x20 grid scan", found.len());
        if let Some([sx, sz]) = found.first() {
            let layout = layout_for_start(&c, *sx, *sz).expect("layout");
            let centre = layout[0].bb;
            let cx = (centre.x0 + centre.x1) / 2;
            let cz = (centre.z0 + centre.z1) / 2;
            let cy = (centre.y0 + centre.y1) / 2;
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &c);
            let mut chamber_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if crate::block::block_type(id).is_some_and(|bt| {
                            matches!(
                                bt.name,
                                "tuff_bricks"
                                    | "polished_tuff"
                                    | "chiseled_tuff_bricks"
                                    | "cut_copper"
                                    | "oxidized_cut_copper"
                                    | "copper_grate"
                                    | "copper_bulb"
                                    | "trial_spawner"
                                    | "vault"
                            )
                        }) {
                            chamber_blocks += 1;
                        }
                    }
                }
            }
            println!(
                "full-pipeline centre cube {coord:?}: {chamber_blocks} chamber blocks"
            );
            assert!(
                chamber_blocks > 0,
                "the chambers centre cube generated no chamber blocks"
            );
        }
    }
}
