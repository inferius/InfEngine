//! W254 - ocean ruins (`OceanRuinStructure` + `OceanRuinPieces.java`
//! port). The template lists are Java consts, mirrored here.
//!
//! Placement (`structure_set/ocean_ruins.json`): random_spread
//! spacing 20 / separation 8 / salt 14357621; the two set entries
//! carry disjoint biome gates (cold oceans vs warm/lukewarm). One
//! start piece per grid cell: `isLarge` rolls at `large_probability
//! 0.3`; a large start also spreads a CLUSTER of 4-8 small ruins
//! (`cluster_probability 0.9`) into the 8 surrounding slots, each
//! dropped when its planar box clips the parent's.
//!
//! COLD ruins stack THREE templates at one anchor - bricks at the
//! base integrity (0.9 large / 0.8 small), the cracked variant at
//! 0.7, the mossy one at 0.5 - each cell surviving its own
//! `BlockRotProcessor` keep roll, which is what gives cold ruins the
//! blended weathering. WARM ruins are a single template. Air cells
//! never place (`STRUCTURE_AND_AIR` ignore - the ocean stays where
//! the template is hollow).
//!
//! Anchoring ports `postProcess` + `getHeight`: start at the ocean
//! floor of the anchor column; per footprint column the effective
//! floor is `min(anchor_floor, column_floor)` (the Java walk starts
//! below the anchor and stops immediately inside higher ground); when
//! the footprint is mostly a deep depression (span > 2 and deep
//! columns > width - 2) the whole piece drops to `minY + 1`.
//!
//! DATA markers: `chest` becomes the loot chest cell itself
//! (UNDERWATER_RUIN_BIG for large pieces, _SMALL otherwise, resolved
//! on the gen worker); `drowned` would spawn the mob - until drowneds
//! exist the marker resolves to water below sea level / air above,
//! exactly the block MC leaves behind after the spawn. Honest gaps:
//! no suspicious-sand/gravel archaeology processor (plain sand /
//! gravel stays), ice counts as floor for the depression scan (MC
//! walks through it).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::{Mapped, map_state, registry_id};
use super::features::stream_seed;
use super::mineshaft::Bb;
use super::noise::SplitMix64;
use super::scattered::random_spread_start;
use super::terrain::{mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;

/// `structure_set/ocean_ruins.json`.
const SPACING: i32 = 20;
const SEPARATION: i32 = 8;
const SALT_RUIN_SET: u64 = 14_357_621;
const SALT_RUIN_LAYOUT: u64 = 0xD4E6_0080;
/// `ocean_ruin_cold/warm.json`.
const LARGE_PROBABILITY: f32 = 0.3;
const CLUSTER_PROBABILITY: f32 = 0.9;
/// Engine sea level (the drowned-marker water/air split).
const SEA_LEVEL: i32 = 63;

const WARM_RUINS: &[&str] = &[
    "minecraft:underwater_ruin/warm_1",
    "minecraft:underwater_ruin/warm_2",
    "minecraft:underwater_ruin/warm_3",
    "minecraft:underwater_ruin/warm_4",
    "minecraft:underwater_ruin/warm_5",
    "minecraft:underwater_ruin/warm_6",
    "minecraft:underwater_ruin/warm_7",
    "minecraft:underwater_ruin/warm_8",
];
const BIG_WARM_RUINS: &[&str] = &[
    "minecraft:underwater_ruin/big_warm_4",
    "minecraft:underwater_ruin/big_warm_5",
    "minecraft:underwater_ruin/big_warm_6",
    "minecraft:underwater_ruin/big_warm_7",
];
const RUINS_BRICK: &[&str] = &[
    "minecraft:underwater_ruin/brick_1",
    "minecraft:underwater_ruin/brick_2",
    "minecraft:underwater_ruin/brick_3",
    "minecraft:underwater_ruin/brick_4",
    "minecraft:underwater_ruin/brick_5",
    "minecraft:underwater_ruin/brick_6",
    "minecraft:underwater_ruin/brick_7",
    "minecraft:underwater_ruin/brick_8",
];
const RUINS_CRACKED: &[&str] = &[
    "minecraft:underwater_ruin/cracked_1",
    "minecraft:underwater_ruin/cracked_2",
    "minecraft:underwater_ruin/cracked_3",
    "minecraft:underwater_ruin/cracked_4",
    "minecraft:underwater_ruin/cracked_5",
    "minecraft:underwater_ruin/cracked_6",
    "minecraft:underwater_ruin/cracked_7",
    "minecraft:underwater_ruin/cracked_8",
];
const RUINS_MOSSY: &[&str] = &[
    "minecraft:underwater_ruin/mossy_1",
    "minecraft:underwater_ruin/mossy_2",
    "minecraft:underwater_ruin/mossy_3",
    "minecraft:underwater_ruin/mossy_4",
    "minecraft:underwater_ruin/mossy_5",
    "minecraft:underwater_ruin/mossy_6",
    "minecraft:underwater_ruin/mossy_7",
    "minecraft:underwater_ruin/mossy_8",
];
const BIG_RUINS_BRICK: &[&str] = &[
    "minecraft:underwater_ruin/big_brick_1",
    "minecraft:underwater_ruin/big_brick_2",
    "minecraft:underwater_ruin/big_brick_3",
    "minecraft:underwater_ruin/big_brick_8",
];
const BIG_RUINS_CRACKED: &[&str] = &[
    "minecraft:underwater_ruin/big_cracked_1",
    "minecraft:underwater_ruin/big_cracked_2",
    "minecraft:underwater_ruin/big_cracked_3",
    "minecraft:underwater_ruin/big_cracked_8",
];
const BIG_RUINS_MOSSY: &[&str] = &[
    "minecraft:underwater_ruin/big_mossy_1",
    "minecraft:underwater_ruin/big_mossy_2",
    "minecraft:underwater_ruin/big_mossy_3",
    "minecraft:underwater_ruin/big_mossy_8",
];

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_f32(r: &mut SplitMix64) -> f32 {
    (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

/// `Mth.nextInt(random, min, max)` - INCLUSIVE bounds.
fn next_range(r: &mut SplitMix64, min: i32, max: i32) -> i32 {
    min + next_int(r, max - min + 1)
}

/// Origin-pivot rotation (`StructureTemplate.transform`, pivot ZERO).
fn rot_origin(r: u8, p: [i32; 3]) -> [i32; 3] {
    match r % 4 {
        0 => p,
        1 => [-p[2], p[1], p[0]],
        2 => [-p[0], p[1], -p[2]],
        _ => [p[2], p[1], -p[0]],
    }
}

/// One placed ruin piece: the layered templates share the anchor +
/// rotation; each layer keeps cells at its own integrity.
#[derive(Debug, Clone)]
pub(crate) struct RuinPiece {
    pub(crate) layers: Vec<(&'static str, f32)>,
    pub(crate) position: [i32; 3],
    pub(crate) rotation: u8,
    pub(crate) is_large: bool,
    pub(crate) hull: Bb,
}

fn is_cold_ocean(b: McBiome) -> bool {
    matches!(
        b,
        McBiome::FrozenOcean
            | McBiome::ColdOcean
            | McBiome::Ocean
            | McBiome::DeepFrozenOcean
            | McBiome::DeepColdOcean
            | McBiome::DeepOcean
    )
}

fn is_warm_ocean(b: McBiome) -> bool {
    matches!(
        b,
        McBiome::LukewarmOcean | McBiome::WarmOcean | McBiome::DeepLukewarmOcean
    )
}

/// `OceanRuinPiece.postProcess` + `getHeight`: resolve a piece's Y
/// from its anchor column + footprint depression scan.
fn piece_y(ctx: &WorldgenCtx, anchor: [i32; 2], size: [i32; 3], rot: u8) -> i32 {
    let h = mc_floor_at(ctx, anchor[0], anchor[1]) + 1;
    let corner_l = rot_origin(rot, [size[0] - 1, 0, size[2] - 1]);
    let (x0, x1) = (
        anchor[0].min(anchor[0] + corner_l[0]),
        anchor[0].max(anchor[0] + corner_l[0]),
    );
    let (z0, z1) = (
        anchor[1].min(anchor[1] + corner_l[2]),
        anchor[1].max(anchor[1] + corner_l[2]),
    );
    let top_y = h - 1;
    let mut min_y = i32::MAX;
    let mut area = 0;
    for x in x0..=x1 {
        for z in z0..=z1 {
            // The Java walk starts below the anchor height and stops
            // inside higher ground immediately: effective floor =
            // min(anchor floor, column floor).
            let floor = mc_floor_at(ctx, x, z).min(top_y);
            min_y = min_y.min(floor);
            if floor < top_y - 2 {
                area += 1;
            }
        }
    }
    let width = x1 - x0;
    if top_y - min_y > 2 && area > width - 2 {
        min_y + 1
    } else {
        h
    }
}

/// Build one piece at `anchor` with resolved Y + hull.
fn make_piece(
    ctx: &WorldgenCtx,
    layers: Vec<(&'static str, f32)>,
    anchor: [i32; 2],
    rotation: u8,
    is_large: bool,
) -> Option<RuinPiece> {
    let t = super::village_data::template(layers[0].0)?;
    let y = piece_y(ctx, anchor, t.size, rotation);
    let position = [anchor[0], y.max(1), anchor[1]];
    let a = rot_origin(rotation, [0, 0, 0]);
    let b = rot_origin(rotation, [t.size[0] - 1, t.size[1] - 1, t.size[2] - 1]);
    let hull = Bb {
        x0: position[0] + a[0].min(b[0]),
        y0: position[1] + a[1].min(b[1]),
        z0: position[2] + a[2].min(b[2]),
        x1: position[0] + a[0].max(b[0]),
        y1: position[1] + a[1].max(b[1]),
        z1: position[2] + a[2].max(b[2]),
    };
    Some(RuinPiece { layers, position, rotation, is_large, hull })
}

/// `addPiece`: warm = one template; cold = the brick/cracked/mossy
/// triple at one shared list index.
fn roll_piece(
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    warm: bool,
    anchor: [i32; 2],
    rotation: u8,
    is_large: bool,
    base_integrity: f32,
) -> Option<RuinPiece> {
    if warm {
        let list = if is_large { BIG_WARM_RUINS } else { WARM_RUINS };
        let t = list[(rng.next_u64() % list.len() as u64) as usize];
        make_piece(ctx, vec![(t, base_integrity)], anchor, rotation, is_large)
    } else {
        let (bricks, cracked, mossy) = if is_large {
            (BIG_RUINS_BRICK, BIG_RUINS_CRACKED, BIG_RUINS_MOSSY)
        } else {
            (RUINS_BRICK, RUINS_CRACKED, RUINS_MOSSY)
        };
        let idx = next_int(rng, bricks.len() as i32) as usize;
        make_piece(
            ctx,
            vec![
                (bricks[idx], base_integrity),
                (cracked[idx], 0.7),
                (mossy[idx], 0.5),
            ],
            anchor,
            rotation,
            is_large,
        )
    }
}

/// Lay out (and cache) the ocean ruin of a start chunk.
pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Vec<RuinPiece>>> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<Vec<RuinPiece>>>>;
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

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Vec<RuinPiece>> {
    let cs = CHUNK_SIZE as i32;
    let west = scx * cs;
    let north = scz * cs;
    let biome = mc_full_biome_at(ctx, west + 8, north + 8);
    let warm = if is_cold_ocean(biome) {
        false
    } else if is_warm_ocean(biome) {
        true
    } else {
        return None;
    };
    let mut rng =
        SplitMix64::new(stream_seed(ctx.effective_seed(), scx, scz, SALT_RUIN_LAYOUT));
    // generatePieces: rotation first, then addPieces' rolls.
    let rotation = next_int(&mut rng, 4) as u8;
    let is_large = next_f32(&mut rng) <= LARGE_PROBABILITY;
    let base_integrity = if is_large { 0.9 } else { 0.8 };
    let mut pieces = Vec::new();
    pieces.push(roll_piece(
        ctx,
        &mut rng,
        warm,
        [west, north],
        rotation,
        is_large,
        base_integrity,
    )?);
    if is_large && next_f32(&mut rng) <= CLUSTER_PROBABILITY {
        // addClusterRuins: the parent's planar box + the 8 slot list.
        let corner_l = rot_origin(rotation, [15, 0, 15]);
        let parent = Bb {
            x0: west.min(west + corner_l[0]),
            y0: 0,
            z0: north.min(north + corner_l[2]),
            x1: west.max(west + corner_l[0]),
            y1: 255,
            z1: north.max(north + corner_l[2]),
        };
        let (bx, bz) = (parent.x0, parent.z0);
        let mut slots: Vec<[i32; 2]> = vec![
            [bx - 16 + next_range(&mut rng, 1, 8), bz + 16 + next_range(&mut rng, 1, 7)],
            [bx - 16 + next_range(&mut rng, 1, 8), bz + next_range(&mut rng, 1, 7)],
            [
                bx - 16 + next_range(&mut rng, 1, 8),
                bz - 16 + next_range(&mut rng, 4, 8),
            ],
            [bx + next_range(&mut rng, 1, 7), bz + 16 + next_range(&mut rng, 1, 7)],
            [bx + next_range(&mut rng, 1, 7), bz - 16 + next_range(&mut rng, 4, 6)],
            [
                bx + 16 + next_range(&mut rng, 1, 7),
                bz + 16 + next_range(&mut rng, 3, 8),
            ],
            [bx + 16 + next_range(&mut rng, 1, 7), bz + next_range(&mut rng, 1, 7)],
            [
                bx + 16 + next_range(&mut rng, 1, 7),
                bz - 16 + next_range(&mut rng, 4, 8),
            ],
        ];
        let ruins = next_range(&mut rng, 4, 8);
        for _ in 0..ruins {
            if slots.is_empty() {
                break;
            }
            let idx = next_int(&mut rng, slots.len() as i32) as usize;
            let pos = slots.remove(idx);
            let next_rotation = next_int(&mut rng, 4) as u8;
            let nc = rot_origin(next_rotation, [5, 0, 6]);
            let next_bb = Bb {
                x0: pos[0].min(pos[0] + nc[0]),
                y0: 0,
                z0: pos[1].min(pos[1] + nc[2]),
                x1: pos[0].max(pos[0] + nc[0]),
                y1: 255,
                z1: pos[1].max(pos[1] + nc[2]),
            };
            if next_bb.intersects(&parent) {
                continue;
            }
            if let Some(p) =
                roll_piece(ctx, &mut rng, warm, pos, next_rotation, false, 0.8)
            {
                pieces.push(p);
            }
        }
    }
    Some(pieces)
}

/// Per-cell integrity keep roll (`BlockRotProcessor`), pure in
/// (seed, world cell, layer) so every replay slice agrees.
fn keeps(seed: u32, w: [i32; 3], layer: usize, integrity: f32) -> bool {
    if integrity >= 1.0 {
        return true;
    }
    let s = stream_seed(seed, w[0], w[2], SALT_RUIN_LAYOUT ^ (layer as u64 + 1))
        ^ (w[1] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let roll = (SplitMix64::new(s).next_u64() >> 40) as f32 / (1u64 << 24) as f32;
    roll < integrity
}

/// Grid reach in chunks: cluster slots throw pieces ~32 blocks out.
const REACH: i32 = 3;

/// The per-chunk ocean-ruin pass (surface_structures).
pub(crate) fn place_ocean_ruins(
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
    let chest_id = registry_id("chest").unwrap_or(0);
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] =
                random_spread_start(seed, gx, gz, SPACING, SEPARATION, SALT_RUIN_SET);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            for piece in layout.iter() {
                if !piece.hull.intersects(&cube) {
                    continue;
                }
                for (layer, (tid, integrity)) in piece.layers.iter().enumerate() {
                    let Some(t) = super::village_data::template(tid) else {
                        continue;
                    };
                    for cell in &t.cells {
                        let state = &t.palette[cell.state];
                        // STRUCTURE_AND_AIR: hollow cells never
                        // overwrite the ocean.
                        if state.name == "minecraft:air" {
                            continue;
                        }
                        let rp = rot_origin(piece.rotation, cell.pos);
                        let w = [
                            piece.position[0] + rp[0],
                            piece.position[1] + rp[1],
                            piece.position[2] + rp[2],
                        ];
                        let (lx, ly, lz) =
                            (w[0] - origin[0], w[1] - origin[1], w[2] - origin[2]);
                        if !(0..cs).contains(&lx)
                            || !(0..cs).contains(&ly)
                            || !(0..cs).contains(&lz)
                        {
                            continue;
                        }
                        // DATA markers resolve before the integrity
                        // roll (handleDataMarker runs unconditionally
                        // for placed marker cells; markers are never
                        // rotted away in practice - keep it simple
                        // and always resolve them).
                        if let Some(marker) = cell.data_marker.as_deref() {
                            let id = match marker {
                                "chest" => chest_id,
                                // "drowned" (and anything unknown):
                                // the post-spawn backfill.
                                _ if w[1] < SEA_LEVEL => 36,
                                _ => 0,
                            };
                            chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
                            continue;
                        }
                        if !keeps(seed, w, layer, *integrity) {
                            continue;
                        }
                        if let Mapped::Id(id) = map_state(state) {
                            chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
                        }
                    }
                }
            }
        }
    }
}

/// Which ruin chest (if any) sits at this world cell - `Some(true)`
/// for a large piece (UNDERWATER_RUIN_BIG), `Some(false)` for small.
pub(crate) fn chest_is_big_at(ctx: &WorldgenCtx, pos: [i32; 3]) -> Option<bool> {
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
                SALT_RUIN_SET,
            );
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            for piece in layout.iter() {
                if !piece.hull.contains(pos) {
                    continue;
                }
                for (tid, _) in &piece.layers {
                    let Some(t) = super::village_data::template(tid) else {
                        continue;
                    };
                    for cell in &t.cells {
                        if cell.data_marker.as_deref() != Some("chest") {
                            continue;
                        }
                        let rp = rot_origin(piece.rotation, cell.pos);
                        let w = [
                            piece.position[0] + rp[0],
                            piece.position[1] + rp[1],
                            piece.position[2] + rp[2],
                        ];
                        if w == pos {
                            return Some(piece.is_large);
                        }
                    }
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
    fn cold_pieces_stack_three_layers_warm_one() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let mut rng = SplitMix64::new(7);
        let cold = roll_piece(&ctx, &mut rng, false, [100, 100], 0, false, 0.8)
            .expect("cold piece builds");
        assert_eq!(cold.layers.len(), 3);
        assert_eq!(cold.layers[1].1, 0.7);
        assert_eq!(cold.layers[2].1, 0.5);
        // The triple shares its list index: brick_N / cracked_N /
        // mossy_N.
        let suffix = |s: &str| s.rsplit('_').next().unwrap().to_string();
        assert_eq!(suffix(cold.layers[0].0), suffix(cold.layers[1].0));
        assert_eq!(suffix(cold.layers[0].0), suffix(cold.layers[2].0));
        let warm = roll_piece(&ctx, &mut rng, true, [100, 100], 1, true, 0.9)
            .expect("warm piece builds");
        assert_eq!(warm.layers.len(), 1);
        assert!(warm.layers[0].0.contains("big_warm"));
    }

    #[test]
    fn integrity_roll_is_pure_and_proportional() {
        let mut kept = 0usize;
        for i in 0..4000 {
            let w = [i * 3, 40 + (i % 17), -i];
            assert_eq!(keeps(9, w, 1, 0.7), keeps(9, w, 1, 0.7), "pure");
            if keeps(9, w, 1, 0.7) {
                kept += 1;
            }
        }
        let ratio = kept as f32 / 4000.0;
        assert!((ratio - 0.7).abs() < 0.03, "keep ratio drifted: {ratio}");
    }

    /// Probe: ocean ruins near a seed/centre + full-pipeline verify
    /// the first one.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_ocean_ruins -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_ocean_ruins() {
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
        let mut found: Vec<Rc<Vec<RuinPiece>>> = Vec::new();
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
                    SALT_RUIN_SET,
                );
                if let Some(layout) = layout_for_start(&ctx, sx, sz) {
                    println!(
                        "ocean ruin at start chunk ({sx}, {sz}): {} piece(s), \
                         first {} rot{} at {:?} large {}",
                        layout.len(),
                        layout[0].layers[0].0,
                        layout[0].rotation,
                        layout[0].position,
                        layout[0].is_large,
                    );
                    found.push(layout);
                }
            }
        }
        println!("total: {} ocean ruins in +-{radius} chunks", found.len());
        if let Some(layout) = found.first() {
            let p = &layout[0];
            // Chest markers -> world cells + table sizes.
            let mut chests: Vec<([i32; 3], bool)> = Vec::new();
            for (tid, _) in &p.layers {
                let t = super::super::village_data::template(tid).expect("template");
                for cell in &t.cells {
                    if cell.data_marker.as_deref() == Some("chest") {
                        let rp = rot_origin(p.rotation, cell.pos);
                        chests.push((
                            [
                                p.position[0] + rp[0],
                                p.position[1] + rp[1],
                                p.position[2] + rp[2],
                            ],
                            p.is_large,
                        ));
                    }
                }
            }
            println!("chests: {chests:?}");
            for (cpos, big) in &chests {
                assert_eq!(
                    chest_is_big_at(&ctx, *cpos),
                    Some(*big),
                    "chest table must re-derive at {cpos:?}"
                );
            }
            // Full-pipeline: the hull cube holds ruin masonry.
            let cx = (p.hull.x0 + p.hull.x1) / 2;
            let cy = (p.hull.y0 + p.hull.y1) / 2;
            let cz = (p.hull.z0 + p.hull.z1) / 2;
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let mut ruin_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if crate::block::block_type(id).is_some_and(|bt| {
                            bt.name.contains("stone_brick")
                                || bt.name.contains("sandstone")
                                || bt.name == "chest"
                                || bt.name.contains("prismarine")
                                || bt.name.contains("magma")
                        }) {
                            ruin_blocks += 1;
                        }
                    }
                }
            }
            println!("full-pipeline hull cube {coord:?}: {ruin_blocks} ruin blocks");
            assert!(ruin_blocks > 0, "the hull cube generated no ruin blocks");
        }
    }
}
