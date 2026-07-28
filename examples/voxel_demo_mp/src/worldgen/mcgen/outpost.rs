//! W251 - pillager outposts in the worldgen pipeline.
//!
//! Placement (`structure_set/pillager_outposts.json`): random_spread
//! with spacing 32 / separation 8 / salt 165745296, PLUS two gates
//! villages don't have:
//! - `frequency 0.2` (`legacy_type_1`): only a fifth of grid cells
//!   host an outpost. MC seeds that roll with
//!   `setLargeFeatureSeed(levelSeed, x, z)`; we draw it from our own
//!   per-cell stream - same trade-off as every structure stream (own
//!   streams, same algorithm).
//! - `exclusion_zone`: no outpost within 10 chunks of any VILLAGES
//!   placement position. MC tests the other set's potential placement
//!   grid (not whether the village actually generated), so this probes
//!   `village::potential_start` over the overlapping grid cells.
//!
//! The structure itself (`pillager_outpost.json`) is a jigsaw: start
//! pool `pillager_outpost/base_plates`, size 7 (vs the village 6),
//! same +-80 envelope, WORLD_SURFACE_WG projection, beard_thin. The
//! towers pool is the engine's first `list_pool_element` user: the
//! watchtower template stacked with an `outpost_rot` (block_rot 0.05)
//! overgrown overlay. Carving is shared with villages
//! (`village::carve_layout`).
//!
//! Biome gate `#has_structure/pillager_outpost`: desert, plains,
//! savanna, snowy_plains, taiga, grove + `#is_mountain` (meadow,
//! frozen/jagged/stony peaks, snowy slopes, cherry grove).

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::biome::McBiome;
use super::features::stream_seed;
use super::jigsaw::{self, PlacedPiece, template_bb};
use super::noise::SplitMix64;
use super::terrain::mc_full_biome_at;
use super::village;
use crate::worldgen::WorldgenCtx;

/// `structure_set/pillager_outposts.json`.
pub(crate) const SPACING: i32 = 32;
const SEPARATION: i32 = 8;
const SALT_OUTPOST_SET: u64 = 165_745_296;
/// `frequency: 0.2` with `legacy_type_1` reduction.
const FREQUENCY: f32 = 0.2;
/// Own stream for the frequency roll (MC: setLargeFeatureSeed).
const SALT_OUTPOST_FREQ: u64 = 0xD4E6_0031;
/// Layout stream per start chunk.
const SALT_OUTPOST_LAYOUT: u64 = 0xD4E6_0032;
/// `exclusion_zone.chunk_count` against the villages set.
const VILLAGE_EXCLUSION: i32 = 10;
/// `pillager_outpost.json` `size`.
const MAX_DEPTH: i32 = 7;

/// The candidate start chunk of a 32x32 grid cell.
pub(crate) fn potential_start(seed: u32, grid_x: i32, grid_z: i32) -> [i32; 2] {
    super::scattered::random_spread_start(
        seed,
        grid_x,
        grid_z,
        SPACING,
        SEPARATION,
        SALT_OUTPOST_SET,
    )
}

/// The legacy_type_1 frequency roll for a candidate start chunk.
fn frequency_passes(seed: u32, scx: i32, scz: i32) -> bool {
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_OUTPOST_FREQ));
    let roll = (rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
    roll < FREQUENCY
}

/// `exclusion_zone`: forbidden when any villages-set placement lands
/// within 10 chunks (Chebyshev) of the candidate start.
fn near_village_placement(seed: u32, scx: i32, scz: i32) -> bool {
    let gx0 = (scx - VILLAGE_EXCLUSION).div_euclid(village::SPACING);
    let gx1 = (scx + VILLAGE_EXCLUSION).div_euclid(village::SPACING);
    let gz0 = (scz - VILLAGE_EXCLUSION).div_euclid(village::SPACING);
    let gz1 = (scz + VILLAGE_EXCLUSION).div_euclid(village::SPACING);
    for gx in gx0..=gx1 {
        for gz in gz0..=gz1 {
            let [vx, vz] = village::potential_start(seed, gx, gz);
            if (vx - scx).abs() <= VILLAGE_EXCLUSION
                && (vz - scz).abs() <= VILLAGE_EXCLUSION
            {
                return true;
            }
        }
    }
    false
}

/// `#has_structure/pillager_outpost` (incl. the `#is_mountain` tag).
fn biome_hosts_outpost(biome: McBiome) -> bool {
    matches!(
        biome,
        McBiome::Desert
            | McBiome::Plains
            | McBiome::Savanna
            | McBiome::SnowyPlains
            | McBiome::Taiga
            | McBiome::Grove
            | McBiome::Meadow
            | McBiome::FrozenPeaks
            | McBiome::JaggedPeaks
            | McBiome::StonyPeaks
            | McBiome::SnowySlopes
            | McBiome::CherryGrove
    )
}

/// Solve (and cache) the outpost layout for a start chunk. `None`
/// when any placement gate fails (frequency, village exclusion,
/// biome) or the start pool rolls empty.
type LayoutCache = HashMap<(u32, i32, i32), Option<Rc<Vec<PlacedPiece>>>>;
thread_local! {
    static LAYOUT_CACHE: RefCell<LayoutCache> = RefCell::new(HashMap::new());
}

pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Vec<PlacedPiece>>> {
    let seed = ctx.effective_seed();
    if let Some(hit) =
        LAYOUT_CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned())
    {
        return hit;
    }
    let built = {
        // W444: same probe-guard rule as the village solve.
        let _probe = super::terrain::LayoutProbeGuard::enter();
        build_layout(ctx, scx, scz)
    }
    .map(Rc::new);
    LAYOUT_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() > 256 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

/// W383: probe-path lookup (see `probe_budget`) - cache hit or one
/// budgeted cold build; over budget reports absent without caching.
fn layout_for_probe(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Vec<PlacedPiece>>> {
    let seed = ctx.effective_seed();
    if let Some(hit) =
        LAYOUT_CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned())
    {
        return hit;
    }
    if !super::probe_budget::allow_build() {
        return None;
    }
    layout_for_start(ctx, scx, scz)
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Vec<PlacedPiece>> {
    let seed = ctx.effective_seed();
    if !frequency_passes(seed, scx, scz) || near_village_placement(seed, scx, scz) {
        return None;
    }
    let cs = CHUNK_SIZE as i32;
    let biome = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
    if !biome_hosts_outpost(biome) {
        return None;
    }
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_OUTPOST_LAYOUT));
    jigsaw::solve(
        ctx,
        [scx, scz],
        "minecraft:pillager_outpost/base_plates",
        MAX_DEPTH,
        &mut rng,
    )
}

/// The per-chunk outpost pass, mirroring `place_villages`.
pub(crate) fn place_outposts(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    // +-80 envelope + piece extent, same reach as villages.
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
            village::carve_layout(&layout, chunk, ctx, origin);
        }
    }
}

/// Deterministic membership: is this world position inside an outpost
/// piece's real footprint? Drives the watchtower chest's loot pick on
/// the gen worker.
pub(crate) fn pos_in_outpost(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    let seed = ctx.effective_seed();
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    const REACH: i32 = 7;
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in
            (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING)
        {
            let [scx, scz] = potential_start(seed, gx, gz);
            if (scx - ccx).abs() > REACH || (scz - ccz).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_probe(ctx, scx, scz) else {
                continue;
            };
            for piece in layout.iter() {
                let Some(t) = super::village_data::template(&piece.template) else {
                    continue;
                };
                let bb = template_bb(t.size, piece.position, piece.rotation);
                if bb.contains(pos) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_holds_a_fifth_of_cells() {
        let mut hosted = 0usize;
        let total = 4000;
        for i in 0..total {
            if frequency_passes(99, i * 13, -i * 7) {
                hosted += 1;
            }
        }
        let ratio = hosted as f32 / total as f32;
        assert!(
            (ratio - FREQUENCY).abs() < 0.03,
            "legacy_type_1 ratio drifted: {ratio}"
        );
    }

    #[test]
    fn village_exclusion_blocks_nearby_cells() {
        // A start ON a village placement must be excluded; one a full
        // spacing away from every village candidate must not.
        let seed = 1234u32;
        let [vx, vz] = village::potential_start(seed, 5, 5);
        assert!(near_village_placement(seed, vx, vz));
        assert!(near_village_placement(seed, vx + VILLAGE_EXCLUSION, vz));
        assert!(!near_village_placement(
            seed,
            vx + village::SPACING * 2 + VILLAGE_EXCLUSION + 1,
            vz + village::SPACING * 2 + VILLAGE_EXCLUSION + 1,
        ));
    }

    #[test]
    fn outpost_layout_solves_with_a_tower() {
        // The towers pool is a list element - the solved layout must
        // resolve it to the watchtower template with the overgrown
        // overlay attached.
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let mut rng = SplitMix64::new(0xB00B1E5);
        let pieces = jigsaw::solve(
            &ctx,
            [300, 300],
            "minecraft:pillager_outpost/base_plates",
            MAX_DEPTH,
            &mut rng,
        )
        .expect("outpost layout solves");
        assert!(
            pieces
                .iter()
                .any(|p| p.template.contains("base_plate")),
            "base plate missing"
        );
        let tower = pieces
            .iter()
            .find(|p| p.template.contains("watchtower"))
            .expect("watchtower attaches to the base plate");
        assert_eq!(tower.overlays.len(), 1, "overgrown overlay rides along");
        assert!(tower.overlays[0].location.contains("overgrown"));
        assert!(
            (tower.overlays[0].integrity - 0.05).abs() < 1e-6,
            "outpost_rot block_rot integrity is 0.05: {}",
            tower.overlays[0].integrity
        );
    }

    /// Probe: scan for outposts near a seed/centre + full-pipeline
    /// verify the first one (real chunks must hold its blocks).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp --release
    /// --lib dump_outposts -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_outposts() {
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
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 128);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<[i32; 2]> = Vec::new();
        for gx in (ccx - radius).div_euclid(SPACING)..=(ccx + radius).div_euclid(SPACING)
        {
            for gz in
                (ccz - radius).div_euclid(SPACING)..=(ccz + radius).div_euclid(SPACING)
            {
                let [sx, sz] = potential_start(ctx.effective_seed(), gx, gz);
                let Some(layout) = layout_for_start(&ctx, sx, sz) else {
                    continue;
                };
                let biome = mc_full_biome_at(&ctx, sx * cs + 8, sz * cs + 8);
                let tower = layout.iter().find(|p| p.template.contains("watchtower"));
                println!(
                    "outpost at start chunk ({sx}, {sz}) [{biome:?}]: {} pieces, tower {:?}",
                    layout.len(),
                    tower.map(|t| t.bb),
                );
                found.push([sx, sz]);
            }
        }
        println!("total: {} outposts in +-{radius} chunks", found.len());
        // Full-pipeline verify on the first: the watchtower cube must
        // contain outpost-written blocks after generation.
        if let Some([sx, sz]) = found.first() {
            let layout = layout_for_start(&ctx, *sx, *sz).expect("layout");
            let tower = layout
                .iter()
                .find(|p| p.template.contains("watchtower"))
                .expect("tower piece");
            let t = super::super::village_data::template(&tower.template)
                .expect("tower template");
            let bb = template_bb(t.size, tower.position, tower.rotation);
            let cx = (bb.x0 + bb.x1) / 2;
            let cz = (bb.z0 + bb.z1) / 2;
            let cy = (bb.y0 + bb.y1) / 2;
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let mut outpost_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if crate::block::block_type(id).is_some_and(|bt| {
                            matches!(
                                bt.name,
                                "planks" | "cobble" | "mossy_cobble" | "birch_planks"
                                    | "dark_oak_planks" | "dark_oak_log" | "oak_fence"
                            )
                        }) {
                            outpost_blocks += 1;
                        }
                    }
                }
            }
            println!(
                "full-pipeline watchtower cube {coord:?}: {outpost_blocks} outpost blocks"
            );
            assert!(
                outpost_blocks > 0,
                "the watchtower cube generated no outpost blocks"
            );
            // The tower loot chest must materialize AND probe as
            // outpost membership - that membership IS the worker's
            // loot-table pick (chest_loot_kind).
            let mut chest_positions: Vec<[i32; 3]> = Vec::new();
            for ccx in bb.x0.div_euclid(cs)..=bb.x1.div_euclid(cs) {
                for ccy in bb.y0.div_euclid(cs)..=bb.y1.div_euclid(cs) {
                    for ccz in bb.z0.div_euclid(cs)..=bb.z1.div_euclid(cs) {
                        let c = crate::worldgen::generate_chunk_at_ctx(
                            [ccx, ccy, ccz],
                            &ctx,
                        );
                        for lx in 0..CHUNK_SIZE {
                            for ly in 0..CHUNK_SIZE {
                                for lz in 0..CHUNK_SIZE {
                                    if c.get(lx, ly, lz).0 == crate::block::CHEST_ID {
                                        chest_positions.push([
                                            ccx * cs + lx as i32,
                                            ccy * cs + ly as i32,
                                            ccz * cs + lz as i32,
                                        ]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            println!("watchtower chests: {chest_positions:?}");
            assert!(
                !chest_positions.is_empty(),
                "the watchtower must hold its loot chest"
            );
            for p in &chest_positions {
                assert!(
                    pos_in_outpost(&ctx, *p),
                    "tower chest at {p:?} must probe as outpost membership"
                );
            }
        }
    }
}
