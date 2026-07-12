//! W252 - shipwreck (`ShipwreckStructure` + `ShipwreckPieces.java`
//! port): one template piece per start, picked from the Java
//! hardcoded lists (no template pools - the lists live in
//! `ShipwreckPieces`, mirrored here as consts).
//!
//! Placement (`structure_set/shipwrecks.json`): random_spread spacing
//! 24 / separation 4 / salt 165745295, two set entries with disjoint
//! biome gates - `#is_ocean` picks the OCEAN template list (sunken,
//! anchored to the heightmap MEAN like `postProcess`), `#is_beach`
//! picks the BEACHED list (anchored `minY - sizeY/2 - rand(3)`).
//! The heightmap sample walks the template's UNROTATED footprint
//! rectangle from the anchor - a faithful port of the Java loop,
//! which iterates `templatePosition..+size` regardless of rotation.
//!
//! Templates rotate around the shared PIVOT (4, 0, 15) (the
//! `makeSettings` rotation pivot), reusing the igloo's pivot-rotation
//! helper. Loot: the templates carry `structure_block` DATA markers
//! (`map_chest` / `treasure_chest` / `supply_chest`) one cell ABOVE
//! each chest (`handleDataMarker` sets the loot on
//! `position.below()`); `chest_kind_at` re-derives the marker for a
//! chest position so the gen worker picks the right table.
//!
//! Honest gaps: OCEAN_FLOOR_WG and our post-carve floor oracle agree
//! (both measure the seabed); WORLD_SURFACE_WG for the beached
//! variant would measure water surfaces, ours measures the seabed on
//! the few wet columns of a beach footprint - beached wrecks can sit
//! marginally lower there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::{Mapped, map_state};
use super::features::stream_seed;
use super::mineshaft::Bb;
use super::noise::SplitMix64;
use super::scattered::random_spread_start;
use super::terrain::{mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;

/// `structure_set/shipwrecks.json`.
const SPACING: i32 = 24;
const SEPARATION: i32 = 4;
const SALT_SHIPWRECK_SET: u64 = 165_745_295;
const SALT_SHIPWRECK_LAYOUT: u64 = 0xD4E6_0060;
/// `ShipwreckPieces.PIVOT`.
const PIVOT: [i32; 3] = [4, 0, 15];

/// `ShipwreckPieces.STRUCTURE_LOCATION_BEACHED`.
const TEMPLATES_BEACHED: &[&str] = &[
    "minecraft:shipwreck/with_mast",
    "minecraft:shipwreck/sideways_full",
    "minecraft:shipwreck/sideways_fronthalf",
    "minecraft:shipwreck/sideways_backhalf",
    "minecraft:shipwreck/rightsideup_full",
    "minecraft:shipwreck/rightsideup_fronthalf",
    "minecraft:shipwreck/rightsideup_backhalf",
    "minecraft:shipwreck/with_mast_degraded",
    "minecraft:shipwreck/rightsideup_full_degraded",
    "minecraft:shipwreck/rightsideup_fronthalf_degraded",
    "minecraft:shipwreck/rightsideup_backhalf_degraded",
];

/// `ShipwreckPieces.STRUCTURE_LOCATION_OCEAN`.
const TEMPLATES_OCEAN: &[&str] = &[
    "minecraft:shipwreck/with_mast",
    "minecraft:shipwreck/upsidedown_full",
    "minecraft:shipwreck/upsidedown_fronthalf",
    "minecraft:shipwreck/upsidedown_backhalf",
    "minecraft:shipwreck/sideways_full",
    "minecraft:shipwreck/sideways_fronthalf",
    "minecraft:shipwreck/sideways_backhalf",
    "minecraft:shipwreck/rightsideup_full",
    "minecraft:shipwreck/rightsideup_fronthalf",
    "minecraft:shipwreck/rightsideup_backhalf",
    "minecraft:shipwreck/with_mast_degraded",
    "minecraft:shipwreck/upsidedown_full_degraded",
    "minecraft:shipwreck/upsidedown_fronthalf_degraded",
    "minecraft:shipwreck/upsidedown_backhalf_degraded",
    "minecraft:shipwreck/sideways_full_degraded",
    "minecraft:shipwreck/sideways_fronthalf_degraded",
    "minecraft:shipwreck/sideways_backhalf_degraded",
    "minecraft:shipwreck/rightsideup_full_degraded",
    "minecraft:shipwreck/rightsideup_fronthalf_degraded",
    "minecraft:shipwreck/rightsideup_backhalf_degraded",
];

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// `StructureTemplate.transform` with the shipwreck pivot (same math
/// as the igloo helper, shared salt-free).
fn rot_pivot(r: u8, p: [i32; 3]) -> [i32; 3] {
    let (px, pz) = (PIVOT[0], PIVOT[2]);
    match r % 4 {
        0 => p,
        1 => [px + pz - p[2], p[1], pz - px + p[0]],
        2 => [px + px - p[0], p[1], pz + pz - p[2]],
        _ => [px - pz + p[2], p[1], px + pz - p[0]],
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Shipwreck {
    pub(crate) template: &'static str,
    /// World position of the template's local origin (pre-pivot).
    pub(crate) position: [i32; 3],
    pub(crate) rotation: u8,
    /// World bounds of the rotated template, for clip + membership.
    pub(crate) hull: Bb,
}

/// The chest tables a shipwreck can hold, keyed by its data markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShipwreckChest {
    Map,
    Treasure,
    Supply,
}

fn marker_chest(marker: &str) -> Option<ShipwreckChest> {
    Some(match marker {
        "map_chest" => ShipwreckChest::Map,
        "treasure_chest" => ShipwreckChest::Treasure,
        "supply_chest" => ShipwreckChest::Supply,
        _ => return None,
    })
}

/// Lay out (and cache) the shipwreck of a start chunk.
pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Shipwreck>> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<Shipwreck>>>;
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
        if map.len() > 512 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

fn is_ocean(b: McBiome) -> bool {
    matches!(
        b,
        McBiome::DeepFrozenOcean
            | McBiome::DeepColdOcean
            | McBiome::DeepOcean
            | McBiome::DeepLukewarmOcean
            | McBiome::WarmOcean
            | McBiome::FrozenOcean
            | McBiome::ColdOcean
            | McBiome::Ocean
            | McBiome::LukewarmOcean
    )
}

fn is_beach(b: McBiome) -> bool {
    matches!(b, McBiome::Beach | McBiome::SnowyBeach)
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Shipwreck> {
    let cs = CHUNK_SIZE as i32;
    let west = scx * cs;
    let north = scz * cs;
    let biome = mc_full_biome_at(ctx, west + 8, north + 8);
    let (beached, list) = if is_ocean(biome) {
        (false, TEMPLATES_OCEAN)
    } else if is_beach(biome) {
        (true, TEMPLATES_BEACHED)
    } else {
        return None;
    };
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        scx,
        scz,
        SALT_SHIPWRECK_LAYOUT,
    ));
    let rotation = next_int(&mut rng, 4) as u8;
    let template = list[(rng.next_u64() % list.len() as u64) as usize];
    let t = super::village_data::template(template)?;
    // postProcess height adjust: heightmap over the UNROTATED
    // footprint rectangle (the Java loop iterates raw size from the
    // anchor). "height" = first free y = solid floor + 1.
    let mut mean = 0i64;
    let mut min_y = i32::MAX;
    for dx in 0..t.size[0] {
        for dz in 0..t.size[2] {
            let h = mc_floor_at(ctx, west + dx, north + dz) + 1;
            mean += h as i64;
            min_y = min_y.min(h);
        }
    }
    let base = (t.size[0] * t.size[2]).max(1) as i64;
    let mean = (mean / base) as i32;
    let y = if beached {
        // calculateBeachedPosition: minY - sizeY/2 - rand(3).
        min_y - t.size[1] / 2 - next_int(&mut rng, 3)
    } else {
        mean
    };
    let position = [west, y.max(1), north];
    // Rotated world bounds from the two local corners.
    let a = rot_pivot(rotation, [0, 0, 0]);
    let b = rot_pivot(rotation, [t.size[0] - 1, t.size[1] - 1, t.size[2] - 1]);
    let hull = Bb {
        x0: position[0] + a[0].min(b[0]),
        y0: position[1] + a[1].min(b[1]),
        z0: position[2] + a[2].min(b[2]),
        x1: position[0] + a[0].max(b[0]),
        y1: position[1] + a[1].max(b[1]),
        z1: position[2] + a[2].max(b[2]),
    };
    Some(Shipwreck { template, position, rotation, hull })
}

/// Grid reach in chunks: the pivot throws rotated cells up to ~35
/// blocks from the anchor (32-long hulls around (4, 15)).
const REACH: i32 = 3;

/// The per-chunk shipwreck pass (surface_structures).
pub(crate) fn place_shipwrecks(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let cube = Bb {
        x0: origin[0],
        y0: origin[1],
        z0: origin[2],
        x1: origin[0] + cs - 1,
        y1: origin[1] + cs - 1,
        z1: origin[2] + cs - 1,
    };
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] =
                random_spread_start(seed, gx, gz, SPACING, SEPARATION, SALT_SHIPWRECK_SET);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(wreck) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            if !wreck.hull.intersects(&cube) {
                continue;
            }
            let Some(t) = super::village_data::template(wreck.template) else {
                continue;
            };
            for cell in &t.cells {
                let rp = rot_pivot(wreck.rotation, cell.pos);
                let w = [
                    wreck.position[0] + rp[0],
                    wreck.position[1] + rp[1],
                    wreck.position[2] + rp[2],
                ];
                let (lx, ly, lz) =
                    (w[0] - origin[0], w[1] - origin[1], w[2] - origin[2]);
                if !(0..cs).contains(&lx)
                    || !(0..cs).contains(&ly)
                    || !(0..cs).contains(&lz)
                {
                    continue;
                }
                if let Mapped::Id(id) = map_state(&t.palette[cell.state]) {
                    chunk.set(
                        lx as usize,
                        ly as usize,
                        lz as usize,
                        infengine_voxel::BlockId(id),
                    );
                }
            }
        }
    }
}

/// Which shipwreck chest (if any) sits at this world position: the
/// template's DATA marker one cell above decides the table
/// (`handleDataMarker` sets loot on `position.below()`). Worker-side.
pub(crate) fn chest_kind_at(ctx: &WorldgenCtx, pos: [i32; 3]) -> Option<ShipwreckChest> {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return None;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING) {
            let [scx, scz] = random_spread_start(
                ctx.effective_seed(),
                gx,
                gz,
                SPACING,
                SEPARATION,
                SALT_SHIPWRECK_SET,
            );
            let Some(wreck) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            if !wreck.hull.contains(pos) {
                continue;
            }
            let Some(t) = super::village_data::template(wreck.template) else {
                continue;
            };
            for cell in &t.cells {
                let Some(marker) = cell.data_marker.as_deref() else {
                    continue;
                };
                let Some(kind) = marker_chest(marker) else {
                    continue;
                };
                let rp = rot_pivot(wreck.rotation, cell.pos);
                let chest_w = [
                    wreck.position[0] + rp[0],
                    wreck.position[1] + rp[1] - 1,
                    wreck.position[2] + rp[2],
                ];
                if chest_w == pos {
                    return Some(kind);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipwreck_templates_carry_chest_markers() {
        // Every full-hull template holds the treasure marker; the
        // with_mast supply chest sits mid-deck. Marker cells must
        // parse with their metadata + have a chest below.
        let t = super::super::village_data::template("minecraft:shipwreck/with_mast")
            .expect("template in the zip");
        let markers: Vec<(&str, [i32; 3])> = t
            .cells
            .iter()
            .filter_map(|c| c.data_marker.as_deref().map(|m| (m, c.pos)))
            .collect();
        assert!(
            markers.iter().any(|(m, _)| marker_chest(m).is_some()),
            "with_mast without chest markers: {markers:?}"
        );
        for (m, mpos) in &markers {
            if marker_chest(m).is_none() {
                continue;
            }
            let below = [mpos[0], mpos[1] - 1, mpos[2]];
            let chest_below = t.cells.iter().any(|c| {
                c.pos == below
                    && t.palette[c.state].name == "minecraft:chest"
            });
            assert!(chest_below, "marker {m} at {mpos:?} without a chest below");
        }
    }

    #[test]
    fn layouts_are_deterministic_and_biome_gated() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        // Same start -> same wreck (cache-independent purity).
        let a = build_layout(&ctx, 310, 310);
        let b = build_layout(&ctx, 310, 310);
        match (&a, &b) {
            (Some(x), Some(y)) => {
                assert_eq!(x.template, y.template);
                assert_eq!(x.position, y.position);
                assert_eq!(x.rotation, y.rotation);
            }
            (None, None) => {}
            _ => panic!("determinism broke"),
        }
    }

    /// Probe: shipwreck starts near a seed/centre + full-pipeline
    /// verify the first one (planks must materialise + chests must
    /// resolve their marker tables).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_shipwrecks -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_shipwrecks() {
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
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 64);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<Rc<Shipwreck>> = Vec::new();
        for gx in (ccx - radius).div_euclid(SPACING)..=(ccx + radius).div_euclid(SPACING)
        {
            for gz in
                (ccz - radius).div_euclid(SPACING)..=(ccz + radius).div_euclid(SPACING)
            {
                let [sx, sz] = random_spread_start(
                    ctx.effective_seed(),
                    gx,
                    gz,
                    SPACING,
                    SEPARATION,
                    SALT_SHIPWRECK_SET,
                );
                if let Some(w) = layout_for_start(&ctx, sx, sz) {
                    println!(
                        "shipwreck at start chunk ({sx}, {sz}): {} rot{} hull {:?}",
                        w.template, w.rotation, w.hull
                    );
                    found.push(w);
                }
            }
        }
        println!("total: {} shipwrecks in +-{radius} chunks", found.len());
        if let Some(w) = found.first() {
            // Chest markers -> world chest cells + their tables.
            let t = super::super::village_data::template(w.template).expect("template");
            let mut chests: Vec<([i32; 3], ShipwreckChest)> = Vec::new();
            for cell in &t.cells {
                if let Some(k) = cell.data_marker.as_deref().and_then(marker_chest) {
                    let rp = rot_pivot(w.rotation, cell.pos);
                    chests.push((
                        [
                            w.position[0] + rp[0],
                            w.position[1] + rp[1] - 1,
                            w.position[2] + rp[2],
                        ],
                        k,
                    ));
                }
            }
            println!("chests: {chests:?}");
            for (cpos, kind) in &chests {
                assert_eq!(
                    chest_kind_at(&ctx, *cpos),
                    Some(*kind),
                    "marker table must re-derive at {cpos:?}"
                );
            }
            // Full-pipeline: the hull centre cube holds ship planks.
            let cx = (w.hull.x0 + w.hull.x1) / 2;
            let cy = (w.hull.y0 + w.hull.y1) / 2;
            let cz = (w.hull.z0 + w.hull.z1) / 2;
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let mut ship_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if crate::block::block_type(id).is_some_and(|bt| {
                            bt.name.contains("planks")
                                || bt.name.contains("log")
                                || bt.name == "planks"
                                || bt.name == "wood"
                                || bt.name.contains("fence")
                                || bt.name.contains("trapdoor")
                        }) {
                            ship_blocks += 1;
                        }
                    }
                }
            }
            println!("full-pipeline hull cube {coord:?}: {ship_blocks} ship blocks");
            assert!(ship_blocks > 0, "the hull cube generated no ship blocks");
        }
    }
}
