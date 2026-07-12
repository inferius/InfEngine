//! W248 - igloo (`IglooPieces.java` port) - the first TEMPLATE-based
//! scattered structure, riding the W245 template loader.
//!
//! Placement: `structure_set/igloos.json` random_spread (spacing 32,
//! separation 8, salt 14357618), snowy biomes. Pieces: the top dome
//! always; a 50% roll adds the laboratory basement at `rand(8)+4`
//! ladder segments down, with `igloo/middle` shaft pieces every 3
//! blocks. Each template rotates around its own PIVOT
//! (`StructureTemplate.transform`), and every piece anchors to the
//! HEIGHTMAP at the shared entrance column (the Java pieces each
//! re-derive the same column through their offsets and land at
//! `height - 1` relative to the 90-line).
//!
//! Template cells map through the W245.3 blockmap (the igloo palette
//! added: infested bricks -> plain, structure_block -> air per the
//! data-marker cleanup, oak_wall_sign skipped). The basement chest
//! rolls `igloo_chest` loot at delivery via membership; without a
//! basement the entrance trapdoor seals with a snow block, exactly
//! like the Java `!belowState.is(LADDER)` arm resolves when no shaft
//! was generated.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::{Mapped, map_state, registry_id};
use super::features::stream_seed;
use super::mineshaft::Bb;
use super::noise::SplitMix64;
use super::scattered::random_spread_start;
use super::terrain::{mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;

/// `structure_set/igloos.json`.
const SPACING: i32 = 32;
const SEPARATION: i32 = 8;
const SALT_IGLOO_SET: u64 = 14_357_618;
const SALT_IGLOO_LAYOUT: u64 = 0xD4E6_0050;

/// Template ids + their rotation pivots and piece offsets
/// (`IglooPieces.PIVOTS` / `OFFSETS`).
const TOP: &str = "minecraft:igloo/top";
const MIDDLE: &str = "minecraft:igloo/middle";
const BOTTOM: &str = "minecraft:igloo/bottom";

fn pivot(template: &str) -> [i32; 3] {
    match template {
        TOP => [3, 5, 5],
        MIDDLE => [1, 3, 1],
        _ => [3, 6, 7],
    }
}

fn offset(template: &str) -> [i32; 3] {
    match template {
        TOP => [0, 0, 0],
        MIDDLE => [2, -3, 4],
        _ => [0, -3, -2],
    }
}

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// `StructureTemplate.transform` with a pivot: rotate `p` by `r`
/// clockwise quarter turns around `(pivot.x, pivot.z)`.
fn rot_pivot(r: u8, p: [i32; 3], pv: [i32; 3]) -> [i32; 3] {
    let (px, pz) = (pv[0], pv[2]);
    match r % 4 {
        0 => p,
        1 => [px + pz - p[2], p[1], pz - px + p[0]],
        2 => [px + px - p[0], p[1], pz + pz - p[2]],
        _ => [px - pz + p[2], p[1], px + pz - p[0]],
    }
}

/// One placed igloo piece: template + world anchor (the template's
/// local origin BEFORE pivot rotation).
#[derive(Debug, Clone)]
pub(crate) struct IglooPiece {
    pub(crate) template: &'static str,
    pub(crate) position: [i32; 3],
    pub(crate) rotation: u8,
}

#[derive(Debug, Clone)]
pub(crate) struct Igloo {
    pub(crate) pieces: Vec<IglooPiece>,
    pub(crate) has_basement: bool,
    /// World cell of the entrance trapdoor (snow-sealed when no
    /// basement exists).
    pub(crate) trapdoor: [i32; 3],
    /// Conservative world bounds for membership tests.
    pub(crate) hull: Bb,
}

/// Lay out (and cache) the igloo of a start chunk.
pub(crate) fn layout_for_start(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Rc<Igloo>> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<Igloo>>>;
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

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Igloo> {
    let cs = CHUNK_SIZE as i32;
    let west = scx * cs;
    let north = scz * cs;
    if !matches!(
        mc_full_biome_at(ctx, west + 8, north + 8),
        McBiome::SnowyPlains | McBiome::SnowyTaiga | McBiome::SnowySlopes
    ) {
        return None;
    }
    let mut rng =
        SplitMix64::new(stream_seed(ctx.effective_seed(), scx, scz, SALT_IGLOO_LAYOUT));
    let rotation = next_int(&mut rng, 4) as u8;
    // addPieces: 50% basement, depth = rand(8) + 4 ladder segments.
    let has_basement = rng.next_f64() < 0.5;
    let depth = if has_basement { next_int(&mut rng, 8) + 4 } else { 0 };
    // Every piece lands relative to the heightmap at the shared
    // entrance column: local (3 - off.x, 0, -off.z) of the TOP piece
    // (offset zero) rotated around the top pivot.
    let anchor = [west, 90, north];
    let entrance =
        rot_pivot(rotation, [3, 0, 0], pivot(TOP));
    let entrance_col = [anchor[0] + entrance[0], anchor[2] + entrance[2]];
    let height = mc_floor_at(ctx, entrance_col[0], entrance_col[1]) + 1;
    let base_y = height - 1;
    let mut pieces = Vec::new();
    if has_basement {
        let off = offset(BOTTOM);
        pieces.push(IglooPiece {
            template: BOTTOM,
            position: [
                anchor[0] + off[0],
                base_y + off[1] - depth * 3,
                anchor[2] + off[2],
            ],
            rotation,
        });
        let off = offset(MIDDLE);
        for i in 0..(depth - 1) {
            pieces.push(IglooPiece {
                template: MIDDLE,
                position: [
                    anchor[0] + off[0],
                    base_y + off[1] - i * 3,
                    anchor[2] + off[2],
                ],
                rotation,
            });
        }
    }
    pieces.push(IglooPiece {
        template: TOP,
        position: [anchor[0], base_y, anchor[2]],
        rotation,
    });
    // Trapdoor at TOP local (3, 0, 5), pivot-rotated.
    let td = rot_pivot(rotation, [3, 0, 5], pivot(TOP));
    let trapdoor = [anchor[0] + td[0], base_y, anchor[2] + td[2]];
    // Conservative hull: the top footprint inflated + the shaft down.
    let hull = Bb {
        x0: west - 8,
        y0: (base_y - depth * 3 - 8).max(0),
        z0: north - 8,
        x1: west + 16,
        y1: base_y + 6,
        z1: north + 16,
    };
    Some(Igloo { pieces, has_basement, trapdoor, hull })
}

/// The per-chunk igloo pass (surface_structures).
pub(crate) fn place_igloos(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    const REACH: i32 = 1;
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] =
                random_spread_start(seed, gx, gz, SPACING, SEPARATION, SALT_IGLOO_SET);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(igloo) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            for piece in &igloo.pieces {
                let Some(t) = super::village_data::template(piece.template) else {
                    continue;
                };
                let pv = pivot(piece.template);
                for cell in &t.cells {
                    let rp = rot_pivot(piece.rotation, cell.pos, pv);
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
            // No basement: the trapdoor seals with a snow block (the
            // Java below-is-not-ladder arm).
            if !igloo.has_basement {
                let td = igloo.trapdoor;
                let (lx, ly, lz) =
                    (td[0] - origin[0], td[1] - origin[1], td[2] - origin[2]);
                if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz)
                {
                    let snow = registry_id("snow_block").unwrap_or(1);
                    chunk.set(
                        lx as usize,
                        ly as usize,
                        lz as usize,
                        infengine_voxel::BlockId(snow),
                    );
                }
            }
        }
    }
}

/// Deterministic membership for the chest-loot pick.
pub(crate) fn pos_in_igloo(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    const REACH: i32 = 1;
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING) {
            let [scx, scz] = random_spread_start(
                ctx.effective_seed(),
                gx,
                gz,
                SPACING,
                SEPARATION,
                SALT_IGLOO_SET,
            );
            if let Some(igloo) = layout_for_start(ctx, scx, scz)
                && igloo.hull.contains(pos)
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pivot_rotation_round_trips() {
        // Rotating the pivot cell is a fixed point.
        for r in 0..4 {
            assert_eq!(rot_pivot(r, [3, 2, 5], [3, 0, 5]), [3, 2, 5]);
        }
        // CW90 around (3, 5): (0,0) -> (3+5-0, 5-3+0) = (8, 2).
        assert_eq!(rot_pivot(1, [0, 7, 0], [3, 0, 5]), [8, 7, 2]);
    }

    /// Probe: igloo starts near a seed/centre + a full-pipeline
    /// slice of the first one (snow dome must materialise).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_igloos -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_igloos() {
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
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 160);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<(i32, i32, Rc<Igloo>)> = Vec::new();
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
                    SALT_IGLOO_SET,
                );
                if let Some(igloo) = layout_for_start(&ctx, sx, sz) {
                    println!(
                        "igloo at start chunk ({sx}, {sz}): {} pieces, basement {}",
                        igloo.pieces.len(),
                        igloo.has_basement,
                    );
                    found.push((sx, sz, igloo));
                }
            }
        }
        println!("total: {} igloos", found.len());
        if let Some((sx, sz, igloo)) = found.first() {
            let top = igloo.pieces.last().expect("top piece");
            let centre = [top.position[0] + 3, top.position[1] + 1, top.position[2] + 4];
            let coord = [
                centre[0].div_euclid(cs),
                centre[1].div_euclid(cs),
                centre[2].div_euclid(cs),
            ];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let snow_block = registry_id("snow_block").unwrap_or(0);
            let mut snow = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        if chunk.get(lx, ly, lz).0 == snow_block {
                            snow += 1;
                        }
                    }
                }
            }
            println!(
                "igloo ({sx}, {sz}) dome cube {coord:?}: {snow} snow blocks, \
                 membership {}",
                pos_in_igloo(&ctx, centre)
            );
            assert!(snow > 0, "the dome cube generated no snow blocks");
            assert!(pos_in_igloo(&ctx, centre));
        }
    }
}
