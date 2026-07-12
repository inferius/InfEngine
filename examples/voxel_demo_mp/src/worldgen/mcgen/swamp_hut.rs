//! W247 - swamp hut (`SwampHutPiece.java` port).
//!
//! Placement: `structure_set/swamp_huts.json` random_spread (spacing
//! 32, separation 8, salt 14357620), swamp-only biome tag, the
//! SinglePieceStructure sea-level gate, random orientation, and
//! `updateAverageGroundHeight`: the box floor sits at the AVERAGE
//! ground height of the footprint (MC averages per generating chunk
//! and keeps the first result - order-dependent; the full-footprint
//! oracle average is the deterministic stand-in).
//!
//! The 7x7x9 hut carves block-for-block: spruce plank floor + walls,
//! oak-log corner posts with stilts filled down to the swamp floor,
//! the spruce stair roof, fences and the crafting table. Honest
//! gaps: the cauldron + potted red mushroom have no blocks yet
//! (cells stay open), stair facings/shapes drop to the engine
//! default, and the resident witch + cat wait for their mobs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::registry_id;
use super::features::stream_seed;
use super::mineshaft::{ChunkShaftWorld, ShaftWorld};
use super::noise::SplitMix64;
use super::scattered::{ScatteredPiece, random_spread_start};
use super::terrain::{mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;

/// `structure_set/swamp_huts.json`.
const SPACING: i32 = 32;
const SEPARATION: i32 = 8;
const SALT_HUT_SET: u64 = 14_357_620;
const SALT_HUT_LAYOUT: u64 = 0xD4E6_0040;

const WIDTH: i32 = 7;
const HEIGHT: i32 = 7;
const DEPTH: i32 = 9;
const SEA_LEVEL: i32 = 63;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// Lay out (and cache) the hut of a start chunk.
pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<ScatteredPiece> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<ScatteredPiece>>>;
    thread_local! {
        static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
    }
    let seed = ctx.effective_seed();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned()) {
        return hit.map(|p| *p);
    }
    let built = build_layout(ctx, scx, scz).map(Rc::new);
    CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() > 512 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built.map(|p| *p)
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<ScatteredPiece> {
    let cs = CHUNK_SIZE as i32;
    let west = scx * cs;
    let north = scz * cs;
    if mc_full_biome_at(ctx, west + 8, north + 8) != McBiome::Swamp {
        return None;
    }
    let mut rng =
        SplitMix64::new(stream_seed(ctx.effective_seed(), scx, scz, SALT_HUT_LAYOUT));
    let dir = next_int(&mut rng, 4) as u8;
    let piece = ScatteredPiece::new(west, 64, north, dir, WIDTH, HEIGHT, DEPTH);
    // SinglePieceStructure gate: lowest corner at or above sea level.
    let corners = [
        [piece.bb.x0, piece.bb.z0],
        [piece.bb.x1, piece.bb.z0],
        [piece.bb.x0, piece.bb.z1],
        [piece.bb.x1, piece.bb.z1],
    ];
    if corners
        .iter()
        .map(|c| mc_floor_at(ctx, c[0], c[1]) + 1)
        .min()
        .unwrap()
        < SEA_LEVEL
    {
        return None;
    }
    // updateAverageGroundHeight(offset 0) over the full footprint.
    let mut total = 0i64;
    let mut count = 0i64;
    for x in piece.bb.x0..=piece.bb.x1 {
        for z in piece.bb.z0..=piece.bb.z1 {
            total += (mc_floor_at(ctx, x, z) + 1) as i64;
            count += 1;
        }
    }
    let avg = (total / count) as i32;
    let dy = avg - piece.bb.y0;
    Some(ScatteredPiece { bb: piece.bb.moved(0, dy, 0), dir: piece.dir })
}

fn put(w: &mut dyn ShaftWorld, p: &ScatteredPiece, x: i32, y: i32, z: i32, id: u16) {
    let pos = p.world(x, y, z);
    let (cx0, cz0, cx1, cz1) = w.column();
    if pos[0] >= cx0
        && pos[0] <= cx1
        && pos[2] >= cz0
        && pos[2] <= cz1
        && (-64..320).contains(&pos[1])
    {
        w.place(pos, id);
    }
}

fn boxed(
    w: &mut dyn ShaftWorld,
    p: &ScatteredPiece,
    (x0, y0, z0): (i32, i32, i32),
    (x1, y1, z1): (i32, i32, i32),
    id: u16,
) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                put(w, p, x, y, z, id);
            }
        }
    }
}

fn fill_column_down(
    w: &mut dyn ShaftWorld,
    p: &ScatteredPiece,
    x: i32,
    y: i32,
    z: i32,
    id: u16,
) {
    let pos = p.world(x, y, z);
    let (cx0, cz0, cx1, cz1) = w.column();
    if pos[0] < cx0 || pos[0] > cx1 || pos[2] < cz0 || pos[2] > cz1 {
        return;
    }
    let mut wy = pos[1];
    while wy > 1 {
        let cell = [pos[0], wy, pos[2]];
        let here = w.read(cell);
        if !(here == 0 || here == crate::block::WATER_ID || here == crate::block::LAVA_ID)
        {
            break;
        }
        w.place(cell, id);
        wy -= 1;
    }
}

/// `SwampHutPiece.postProcess`, in source order.
fn carve(w: &mut dyn ShaftWorld, p: &ScatteredPiece) {
    let planks = registry_id("spruce_planks").unwrap_or(7);
    let log = crate::block::WOOD_ID;
    let fence = crate::block::OAK_FENCE_ID;
    let stairs = registry_id("spruce_stairs").unwrap_or(planks);
    let crafting = registry_id("crafting_table").unwrap_or(planks);

    boxed(w, p, (1, 1, 1), (5, 1, 7), planks);
    boxed(w, p, (1, 4, 2), (5, 4, 7), planks);
    boxed(w, p, (2, 1, 0), (4, 1, 0), planks);
    boxed(w, p, (2, 2, 2), (3, 3, 2), planks);
    boxed(w, p, (1, 2, 3), (1, 3, 6), planks);
    boxed(w, p, (5, 2, 3), (5, 3, 6), planks);
    boxed(w, p, (2, 2, 7), (4, 3, 7), planks);
    boxed(w, p, (1, 0, 2), (1, 3, 2), log);
    boxed(w, p, (5, 0, 2), (5, 3, 2), log);
    boxed(w, p, (1, 0, 7), (1, 3, 7), log);
    boxed(w, p, (5, 0, 7), (5, 3, 7), log);
    put(w, p, 2, 3, 2, fence);
    put(w, p, 3, 3, 7, fence);
    put(w, p, 1, 3, 4, 0);
    put(w, p, 5, 3, 4, 0);
    put(w, p, 5, 3, 5, 0);
    // Potted red mushroom + cauldron have no blocks yet - the cells
    // stay open (module docs).
    put(w, p, 3, 2, 6, crafting);
    put(w, p, 1, 2, 1, fence);
    put(w, p, 5, 2, 1, fence);
    // The stair roof (facings drop to the engine default).
    boxed(w, p, (0, 4, 1), (6, 4, 1), stairs);
    boxed(w, p, (0, 4, 2), (0, 4, 7), stairs);
    boxed(w, p, (6, 4, 2), (6, 4, 7), stairs);
    boxed(w, p, (0, 4, 8), (6, 4, 8), stairs);
    // Stilts down to the swamp floor.
    let mut z = 2;
    while z <= 7 {
        let mut x = 1;
        while x <= 5 {
            fill_column_down(w, p, x, -1, z, log);
            x += 4;
        }
        z += 5;
    }
    // The resident witch + cat spawn here in MC - both mobs are
    // future waves.
}

/// The per-chunk swamp-hut pass (surface_structures).
pub(crate) fn place_swamp_huts(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    const REACH: i32 = 1;
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] =
                random_spread_start(seed, gx, gz, SPACING, SEPARATION, SALT_HUT_SET);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(piece) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            let mut world = ChunkShaftWorld { chunk, origin, ctx };
            carve(&mut world, &piece);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Probe: hut starts near a seed/centre + a full-pipeline slice
    /// of the first one.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_swamp_huts -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_swamp_huts() {
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
        let mut found: Vec<ScatteredPiece> = Vec::new();
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
                    SALT_HUT_SET,
                );
                if let Some(p) = layout_for_start(&ctx, sx, sz) {
                    println!("swamp hut at start chunk ({sx}, {sz}): {:?}", p.bb);
                    found.push(p);
                }
            }
        }
        println!("total: {} huts", found.len());
        if let Some(p) = found.first() {
            let centre = p.world(3, 1, 4);
            let coord = [
                centre[0].div_euclid(cs),
                centre[1].div_euclid(cs),
                centre[2].div_euclid(cs),
            ];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let planks = registry_id("spruce_planks").unwrap_or(0);
            let mut hut_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if id == planks || id == crate::block::OAK_FENCE_ID {
                            hut_blocks += 1;
                        }
                    }
                }
            }
            println!("hut cube {coord:?}: {hut_blocks} hut blocks");
            assert!(hut_blocks > 0, "the hut cube generated no hut blocks");
        }
    }
}
