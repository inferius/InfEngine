//! W255 - jungle temple (`JungleTemplePiece.java` port).
//!
//! Placement: `structure_set/jungle_temples.json` random_spread
//! (spacing 32, separation 8, salt 14357619), jungle + bamboo_jungle
//! tag, the SinglePieceStructure sea-level corner gate, random
//! orientation, `updateAverageGroundHeight` anchor - all shared with
//! the swamp hut via `scattered.rs`.
//!
//! The 12x10x15 pyramid carves block-for-block in source order with
//! the `MossStoneSelector`: every selector cell rolls 40% cobblestone
//! / 60% mossy cobblestone. MC rolls the sequential piece RNG; ours
//! is pure per world cell (the standard replay-invariance trade-off).
//!
//! The trap wiring places for real - redstone dust runs, both STICKY
//! PISTONS, the REPEATER, the three puzzle LEVERS and both DISPENSERS
//! exist as engine blocks. Honest gaps: tripwire + tripwire hooks
//! have no blocks (the trigger cells stay open - the dispenser traps
//! are duds exactly like the pyramid's TNT pit), dispensers place
//! empty (no dispenser container mint yet - the
//! JUNGLE_TEMPLE_DISPENSER arrows wait for it), facings (stairs,
//! lever, vine, piston, dispenser) drop to engine defaults, and the
//! `book` chest drop is unenchanted.

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

/// `structure_set/jungle_temples.json`.
const SPACING: i32 = 32;
const SEPARATION: i32 = 8;
const SALT_TEMPLE_SET: u64 = 14_357_619;
const SALT_TEMPLE_LAYOUT: u64 = 0xD4E6_0090;
/// Per-cell MossStoneSelector stream.
const SALT_TEMPLE_MOSS: u64 = 0xD4E6_0091;

const WIDTH: i32 = 12;
const HEIGHT: i32 = 10;
const DEPTH: i32 = 15;
const SEA_LEVEL: i32 = 63;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// Lay out (and cache) the temple of a start chunk.
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
    if !matches!(
        mc_full_biome_at(ctx, west + 8, north + 8),
        McBiome::Jungle | McBiome::BambooJungle
    ) {
        return None;
    }
    let mut rng =
        SplitMix64::new(stream_seed(ctx.effective_seed(), scx, scz, SALT_TEMPLE_LAYOUT));
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
        && (0..256).contains(&pos[1])
    {
        w.place(pos, id);
    }
}

/// `MossStoneSelector.next`: 40% cobblestone, else mossy - pure per
/// world cell.
fn moss_pick(seed: u32, pos: [i32; 3], cobble: u16, mossy: u16) -> u16 {
    let s = stream_seed(seed, pos[0], pos[2], SALT_TEMPLE_MOSS)
        ^ (pos[1] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let roll = (SplitMix64::new(s).next_u64() >> 40) as f32 / (1u64 << 24) as f32;
    if roll < 0.4 { cobble } else { mossy }
}

/// `generateBox(..., STONE_SELECTOR)`.
#[allow(clippy::too_many_arguments)]
fn moss_box(
    w: &mut dyn ShaftWorld,
    p: &ScatteredPiece,
    seed: u32,
    (x0, y0, z0): (i32, i32, i32),
    (x1, y1, z1): (i32, i32, i32),
    cobble: u16,
    mossy: u16,
) {
    let (cx0, cz0, cx1, cz1) = w.column();
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                let pos = p.world(x, y, z);
                if pos[0] >= cx0
                    && pos[0] <= cx1
                    && pos[2] >= cz0
                    && pos[2] <= cz1
                    && (0..256).contains(&pos[1])
                {
                    w.place(pos, moss_pick(seed, pos, cobble, mossy));
                }
            }
        }
    }
}

fn air_box(
    w: &mut dyn ShaftWorld,
    p: &ScatteredPiece,
    (x0, y0, z0): (i32, i32, i32),
    (x1, y1, z1): (i32, i32, i32),
) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                put(w, p, x, y, z, 0);
            }
        }
    }
}

/// `JungleTemplePiece.postProcess`, in source order.
fn carve(w: &mut dyn ShaftWorld, p: &ScatteredPiece, seed: u32) {
    let cobble = crate::block::COBBLE_ID;
    let mossy = registry_id("mossy_cobblestone").unwrap_or(cobble);
    let stairs = registry_id("cobblestone_stairs").unwrap_or(cobble);
    let chiseled = registry_id("chiseled_stone_bricks").unwrap_or(cobble);
    let lever = registry_id("lever").unwrap_or(0);
    let wire = registry_id("redstone_dust").unwrap_or(0);
    let piston = registry_id("sticky_piston").unwrap_or(0);
    let repeater = registry_id("repeater").unwrap_or(0);
    let dispenser = registry_id("dispenser").unwrap_or(cobble);
    let vine = registry_id("vine").unwrap_or(0);
    let chest = crate::block::CHEST_ID;
    let mbox = |w: &mut dyn ShaftWorld, a, b| moss_box(w, p, seed, a, b, cobble, mossy);

    // The stepped pyramid shell.
    mbox(w, (0, -4, 0), (WIDTH - 1, 0, DEPTH - 1));
    mbox(w, (2, 1, 2), (9, 2, 2));
    mbox(w, (2, 1, 12), (9, 2, 12));
    mbox(w, (2, 1, 3), (2, 2, 11));
    mbox(w, (9, 1, 3), (9, 2, 11));
    mbox(w, (1, 3, 1), (10, 6, 1));
    mbox(w, (1, 3, 13), (10, 6, 13));
    mbox(w, (1, 3, 2), (1, 6, 12));
    mbox(w, (10, 3, 2), (10, 6, 12));
    mbox(w, (2, 3, 2), (9, 3, 12));
    mbox(w, (2, 6, 2), (9, 6, 12));
    mbox(w, (3, 7, 3), (8, 7, 11));
    mbox(w, (4, 8, 4), (7, 8, 10));
    air_box(w, p, (3, 1, 3), (8, 2, 11));
    air_box(w, p, (4, 3, 6), (7, 3, 9));
    air_box(w, p, (2, 4, 2), (9, 5, 12));
    air_box(w, p, (4, 6, 5), (7, 6, 9));
    air_box(w, p, (5, 7, 6), (6, 7, 8));
    air_box(w, p, (5, 1, 2), (6, 2, 2));
    air_box(w, p, (5, 2, 12), (6, 2, 12));
    air_box(w, p, (5, 5, 1), (6, 5, 1));
    air_box(w, p, (5, 5, 13), (6, 5, 13));
    put(w, p, 1, 5, 5, 0);
    put(w, p, 10, 5, 5, 0);
    put(w, p, 1, 5, 9, 0);
    put(w, p, 10, 5, 9, 0);
    // Window pillars on the north + south faces.
    for z in [0, 14] {
        mbox(w, (2, 4, z), (2, 5, z));
        mbox(w, (4, 4, z), (4, 5, z));
        mbox(w, (7, 4, z), (7, 5, z));
        mbox(w, (9, 4, z), (9, 5, z));
    }
    mbox(w, (5, 6, 0), (6, 6, 0));
    for x in [0, 11] {
        let mut z = 2;
        while z <= 12 {
            mbox(w, (x, 4, z), (x, 5, z));
            z += 2;
        }
        mbox(w, (x, 6, 5), (x, 6, 5));
        mbox(w, (x, 6, 9), (x, 6, 9));
    }
    // Roof corner towers + the crown.
    mbox(w, (2, 7, 2), (2, 9, 2));
    mbox(w, (9, 7, 2), (9, 9, 2));
    mbox(w, (2, 7, 12), (2, 9, 12));
    mbox(w, (9, 7, 12), (9, 9, 12));
    mbox(w, (4, 9, 4), (4, 9, 4));
    mbox(w, (7, 9, 4), (7, 9, 4));
    mbox(w, (4, 9, 10), (4, 9, 10));
    mbox(w, (7, 9, 10), (7, 9, 10));
    mbox(w, (5, 9, 7), (6, 9, 7));
    // Stairs (facings drop to the engine default).
    put(w, p, 5, 9, 6, stairs);
    put(w, p, 6, 9, 6, stairs);
    put(w, p, 5, 9, 8, stairs);
    put(w, p, 6, 9, 8, stairs);
    put(w, p, 4, 0, 0, stairs);
    put(w, p, 5, 0, 0, stairs);
    put(w, p, 6, 0, 0, stairs);
    put(w, p, 7, 0, 0, stairs);
    put(w, p, 4, 1, 8, stairs);
    put(w, p, 4, 2, 9, stairs);
    put(w, p, 4, 3, 10, stairs);
    put(w, p, 7, 1, 8, stairs);
    put(w, p, 7, 2, 9, stairs);
    put(w, p, 7, 3, 10, stairs);
    mbox(w, (4, 1, 9), (4, 1, 9));
    mbox(w, (7, 1, 9), (7, 1, 9));
    mbox(w, (4, 1, 10), (7, 2, 10));
    mbox(w, (5, 4, 5), (6, 4, 5));
    put(w, p, 4, 4, 5, stairs);
    put(w, p, 7, 4, 5, stairs);
    // The staircase down into the basement.
    for i in 0..4 {
        put(w, p, 5, -i, 6 + i, stairs);
        put(w, p, 6, -i, 6 + i, stairs);
        air_box(w, p, (5, -i, 7 + i), (6, -i, 9 + i));
    }
    air_box(w, p, (1, -3, 12), (10, -1, 13));
    air_box(w, p, (1, -3, 1), (3, -1, 13));
    air_box(w, p, (1, -3, 1), (9, -1, 5));
    let mut z = 1;
    while z <= 13 {
        mbox(w, (1, -3, z), (1, -2, z));
        z += 2;
    }
    let mut z = 2;
    while z <= 12 {
        mbox(w, (1, -1, z), (3, -1, z));
        z += 2;
    }
    mbox(w, (2, -2, 1), (5, -2, 1));
    mbox(w, (7, -2, 1), (9, -2, 1));
    mbox(w, (6, -3, 1), (6, -3, 1));
    mbox(w, (6, -1, 1), (6, -1, 1));
    // Trap 1: tripwire + hooks have no blocks (dud trigger); the
    // wiring + dispenser place for real.
    put(w, p, 5, -3, 7, wire);
    put(w, p, 5, -3, 6, wire);
    put(w, p, 5, -3, 5, wire);
    put(w, p, 5, -3, 4, wire);
    put(w, p, 5, -3, 3, wire);
    put(w, p, 5, -3, 2, wire);
    put(w, p, 5, -3, 1, wire);
    put(w, p, 4, -3, 1, wire);
    put(w, p, 3, -3, 1, mossy);
    put(w, p, 3, -2, 1, dispenser);
    put(w, p, 3, -2, 2, vine);
    // Trap 2.
    put(w, p, 8, -3, 6, wire);
    put(w, p, 9, -3, 6, wire);
    put(w, p, 9, -3, 5, wire);
    put(w, p, 9, -3, 4, mossy);
    put(w, p, 9, -2, 4, wire);
    put(w, p, 9, -2, 3, dispenser);
    put(w, p, 8, -1, 3, vine);
    put(w, p, 8, -2, 3, vine);
    // The main chest.
    put(w, p, 8, -3, 3, chest);
    put(w, p, 9, -3, 2, mossy);
    put(w, p, 8, -3, 1, mossy);
    put(w, p, 4, -3, 5, mossy);
    put(w, p, 5, -2, 5, mossy);
    put(w, p, 5, -1, 5, mossy);
    put(w, p, 6, -3, 5, mossy);
    put(w, p, 7, -2, 5, mossy);
    put(w, p, 7, -1, 5, mossy);
    put(w, p, 8, -3, 5, mossy);
    mbox(w, (9, -1, 1), (9, -1, 5));
    // The lever puzzle room + the piston vault.
    air_box(w, p, (8, -3, 8), (10, -1, 10));
    put(w, p, 8, -2, 11, chiseled);
    put(w, p, 9, -2, 11, chiseled);
    put(w, p, 10, -2, 11, chiseled);
    put(w, p, 8, -2, 12, lever);
    put(w, p, 9, -2, 12, lever);
    put(w, p, 10, -2, 12, lever);
    mbox(w, (8, -3, 8), (8, -3, 10));
    mbox(w, (10, -3, 8), (10, -3, 10));
    put(w, p, 10, -2, 9, mossy);
    put(w, p, 8, -2, 9, wire);
    put(w, p, 8, -2, 10, wire);
    put(w, p, 10, -1, 9, wire);
    put(w, p, 9, -2, 8, piston);
    put(w, p, 10, -2, 8, piston);
    put(w, p, 10, -1, 8, piston);
    put(w, p, 10, -2, 10, repeater);
    // The hidden chest behind the pistons.
    put(w, p, 9, -3, 10, chest);
}

/// The per-chunk jungle-temple pass (surface_structures).
pub(crate) fn place_jungle_temples(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    const REACH: i32 = 2;
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] =
                random_spread_start(seed, gx, gz, SPACING, SEPARATION, SALT_TEMPLE_SET);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(piece) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            let mut world = ChunkShaftWorld { chunk, origin, ctx };
            carve(&mut world, &piece, seed);
        }
    }
}

/// Deterministic membership for the chest-loot pick: inside the
/// temple box (basement included - it hangs 4 below the box floor).
pub(crate) fn pos_in_jungle_temple(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    const REACH: i32 = 2;
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING) {
            let [scx, scz] = random_spread_start(
                ctx.effective_seed(),
                gx,
                gz,
                SPACING,
                SEPARATION,
                SALT_TEMPLE_SET,
            );
            let Some(piece) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            let hull = super::mineshaft::Bb { y0: piece.bb.y0 - 4, ..piece.bb };
            if hull.contains(pos) {
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
    fn moss_pick_is_pure_and_mostly_mossy() {
        let cobble = crate::block::COBBLE_ID;
        let mossy = registry_id("mossy_cobblestone").unwrap();
        let mut cobbles = 0usize;
        for i in 0..4000 {
            let pos = [i, 60 + (i % 9), -i * 3];
            assert_eq!(
                moss_pick(5, pos, cobble, mossy),
                moss_pick(5, pos, cobble, mossy),
            );
            if moss_pick(5, pos, cobble, mossy) == cobble {
                cobbles += 1;
            }
        }
        let ratio = cobbles as f32 / 4000.0;
        assert!((ratio - 0.4).abs() < 0.03, "cobble share drifted: {ratio}");
    }

    /// Probe: temple starts near a seed/centre + a full-pipeline
    /// slice of the first one (basement chest incl. membership).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_jungle_temples -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_jungle_temples() {
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
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 256);
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
                    SALT_TEMPLE_SET,
                );
                if let Some(piece) = layout_for_start(&ctx, sx, sz) {
                    println!("jungle temple at start chunk ({sx}, {sz}): {:?}", piece.bb);
                    found.push(piece);
                }
            }
        }
        println!("total: {} temples in +-{radius} chunks", found.len());
        if let Some(piece) = found.first() {
            // The main chest at local (8, -3, 3) must materialise +
            // probe as temple membership.
            let chest_w = piece.world(8, -3, 3);
            let coord = [
                chest_w[0].div_euclid(cs),
                chest_w[1].div_euclid(cs),
                chest_w[2].div_euclid(cs),
            ];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let got = chunk
                .get(
                    chest_w[0].rem_euclid(cs) as usize,
                    chest_w[1].rem_euclid(cs) as usize,
                    chest_w[2].rem_euclid(cs) as usize,
                )
                .0;
            println!("main chest at {chest_w:?} holds id {got}");
            assert_eq!(got, crate::block::CHEST_ID, "the main chest must materialise");
            assert!(pos_in_jungle_temple(&ctx, chest_w), "membership re-derives");
            // The shell around the chest must be the moss mix.
            let mut moss_mix = 0usize;
            let cobble = crate::block::COBBLE_ID;
            let mossy = registry_id("mossy_cobblestone").unwrap();
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if id == cobble || id == mossy {
                            moss_mix += 1;
                        }
                    }
                }
            }
            println!("temple cube {coord:?}: {moss_mix} cobble/mossy blocks");
            assert!(moss_mix > 50, "the shell mix must materialise: {moss_mix}");
        }
    }
}
