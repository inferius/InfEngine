//! W271 - nether fortress (`NetherFortressStructure` +
//! `NetherFortressPieces.java` port) - the bridge/castle maze.
//!
//! Placement: the SHARED nether_complexes grid (see `bastion.rs`) -
//! spacing 27 / separation 4, weighted fortress(2) : bastion(3) per
//! cell; the fortress claims the `NetherComplex::Fortress` cells.
//! Biome gate: `#has_structure/nether_fortress` = the whole nether.
//!
//! Layout (`generatePieces`): the StartPiece is a BridgeCrossing at
//! (chunk*16+2, y 64) with a random horizontal orientation; children
//! draw from TWO weighted tables (bridge pieces vs castle pieces -
//! the CastleEntrance switches the active table), with per-class max
//! place counts, the no-repeat rule (`allowInRow` exempts straights
//! and corridor stairs), 5 pick attempts, the BridgeEndFiller
//! fallback, depth cap 30, a 112-block envelope from the start, and
//! pending pieces expanded in RANDOM order. `moveInsideHeights(48,
//! 70)` then drops the whole maze to a random band start.
//!
//! Carving mirrors the Java `postProcess` bodies block-for-block
//! (nether bricks, fence railings, the stalk-room soul-sand + nether
//! wart gardens, the entrance lava well, the throne's blaze
//! spawner). The BridgeEndFiller re-seeds its own rng from a stored
//! per-piece seed - exactly why MC serializes it - so every chunk
//! replays identical ragged bridge ends.
//!
//! Honest gaps: fence connection states + stairs facings drop to the
//! engine's single-id blocks (the engine derives fence connections);
//! the blaze spawner uses the SKELETON stand-in until the mob is
//! ported (logged once, same doctrine as cave spider / silverfish);
//! fortress mob spawn zones (blaze/wither-skeleton piece spawns) are
//! the mob-parity follow-up.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::bastion::{self, NetherComplex};
use super::features::stream_seed;
use super::mineshaft::Bb;
use super::nether_mc;
use super::noise::SplitMix64;
use super::scattered::ScatteredPiece;
use crate::worldgen::WorldgenCtx;

const SALT_FORTRESS_LAYOUT: u64 = 0xD4E6_0310;
/// `MAX_DEPTH` / `LOWEST_Y_POSITION` / `MAGIC_START_Y` / the
/// `generateAndAddPiece` envelope.
const MAX_DEPTH: i32 = 30;
const LOWEST_Y: i32 = 10;
const MAGIC_START_Y: i32 = 64;
const ENVELOPE: i32 = 112;
/// `moveInsideHeights(random, 48, 70)`.
const HEIGHTS_MIN: i32 = 48;
const HEIGHTS_MAX: i32 = 70;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

// ----------------------------------------------------------------
// Directions (the ScatteredPiece convention).
// ----------------------------------------------------------------

const N: u8 = 0;
const S: u8 = 1;
const W: u8 = 2;
const E: u8 = 3;

/// `BoundingBox.orientBox(foot, off, size, dir)` (the stronghold's
/// helper, same Java source).
#[allow(clippy::too_many_arguments)]
fn orient_box(fx: i32, fy: i32, fz: i32, ox: i32, oy: i32, oz: i32, sx: i32, sy: i32, sz: i32, dir: u8) -> Bb {
    match dir {
        S => Bb { x0: fx + ox, y0: fy + oy, z0: fz + oz, x1: fx + ox + sx - 1, y1: fy + oy + sy - 1, z1: fz + oz + sz - 1 },
        N => Bb { x0: fx + ox, y0: fy + oy, z0: fz - sz + 1 - oz, x1: fx + ox + sx - 1, y1: fy + oy + sy - 1, z1: fz - oz },
        W => Bb { x0: fx - sz + 1 - oz, y0: fy + oy, z0: fz + ox, x1: fx - oz, y1: fy + oy + sy - 1, z1: fz + ox + sx - 1 },
        _ => Bb { x0: fx + oz, y0: fy + oy, z0: fz + ox, x1: fx + oz + sz - 1, y1: fy + oy + sy - 1, z1: fz + ox + sx - 1 },
    }
}

// ----------------------------------------------------------------
// Pieces.
// ----------------------------------------------------------------

/// The piece classes. `Start` carves as a BridgeCrossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    BridgeStraight,
    BridgeCrossing,
    RoomCrossing,
    StairsRoom,
    MonsterThrone,
    CastleEntrance,
    SmallCorridor,
    CorridorCrossing,
    LeftTurn { chest: bool },
    RightTurn { chest: bool },
    CorridorStairs,
    TBalcony { left: bool, right: bool },
    StalkRoom,
    EndFiller { self_seed: u64 },
}

#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub(crate) kind: Kind,
    pub(crate) bb: Bb,
    pub(crate) dir: u8,
}

impl Piece {
    fn frame(&self) -> ScatteredPiece {
        ScatteredPiece { bb: self.bb, dir: self.dir }
    }
}

/// One weighted-table row: (class id, weight, maxPlaceCount,
/// allowInRow). Class ids index `try_create`.
#[derive(Clone, Copy)]
struct PieceW {
    class: u8,
    weight: i32,
    max: i32,
    allow_in_row: bool,
    count: i32,
}

/// `BRIDGE_PIECE_WEIGHTS`.
fn bridge_weights() -> Vec<PieceW> {
    vec![
        PieceW { class: 0, weight: 30, max: 0, allow_in_row: true, count: 0 }, // BridgeStraight
        PieceW { class: 1, weight: 10, max: 4, allow_in_row: false, count: 0 }, // BridgeCrossing
        PieceW { class: 2, weight: 10, max: 4, allow_in_row: false, count: 0 }, // RoomCrossing
        PieceW { class: 3, weight: 10, max: 3, allow_in_row: false, count: 0 }, // StairsRoom
        PieceW { class: 4, weight: 5, max: 2, allow_in_row: false, count: 0 },  // MonsterThrone
        PieceW { class: 5, weight: 5, max: 1, allow_in_row: false, count: 0 },  // CastleEntrance
    ]
}

/// `CASTLE_PIECE_WEIGHTS`.
fn castle_weights() -> Vec<PieceW> {
    vec![
        PieceW { class: 6, weight: 25, max: 0, allow_in_row: true, count: 0 },  // SmallCorridor
        PieceW { class: 7, weight: 15, max: 5, allow_in_row: false, count: 0 }, // CorridorCrossing
        PieceW { class: 8, weight: 5, max: 10, allow_in_row: false, count: 0 }, // RightTurn
        PieceW { class: 9, weight: 5, max: 10, allow_in_row: false, count: 0 }, // LeftTurn
        PieceW { class: 10, weight: 10, max: 3, allow_in_row: true, count: 0 }, // CorridorStairs
        PieceW { class: 11, weight: 7, max: 2, allow_in_row: false, count: 0 }, // TBalcony
        PieceW { class: 12, weight: 5, max: 2, allow_in_row: false, count: 0 }, // StalkRoom
    ]
}

struct LayoutState {
    pieces: Vec<Piece>,
    /// (piece index -> genDepth, is_castle_child flag rides along).
    depths: Vec<i32>,
    bridge: Vec<PieceW>,
    castle: Vec<PieceW>,
    /// `startPiece.previousPiece` (a class id).
    previous: Option<u8>,
    start_x0: i32,
    start_z0: i32,
}

impl LayoutState {
    fn collides(&self, bb: &Bb) -> bool {
        self.pieces.iter().any(|p| p.bb.intersects(bb))
    }
}

fn is_ok_box(bb: &Bb) -> bool {
    bb.y0 > LOWEST_Y
}

/// `findAndCreateBridgePieceFactory` (createPiece per class): the
/// oriented box + collision + validity, plus the ctor rolls.
#[allow(clippy::too_many_arguments)]
fn try_create(
    st: &LayoutState,
    rng: &mut SplitMix64,
    class: u8,
    fx: i32,
    fy: i32,
    fz: i32,
    dir: u8,
) -> Option<Piece> {
    let (bb, kind) = match class {
        0 => (orient_box(fx, fy, fz, -1, -3, 0, 5, 10, 19, dir), Kind::BridgeStraight),
        1 => (orient_box(fx, fy, fz, -8, -3, 0, 19, 10, 19, dir), Kind::BridgeCrossing),
        2 => (orient_box(fx, fy, fz, -2, 0, 0, 7, 9, 7, dir), Kind::RoomCrossing),
        3 => (orient_box(fx, fy, fz, -2, 0, 0, 7, 11, 7, dir), Kind::StairsRoom),
        4 => (orient_box(fx, fy, fz, -2, 0, 0, 7, 8, 9, dir), Kind::MonsterThrone),
        5 => (orient_box(fx, fy, fz, -5, -3, 0, 13, 14, 13, dir), Kind::CastleEntrance),
        6 => (orient_box(fx, fy, fz, -1, 0, 0, 5, 7, 5, dir), Kind::SmallCorridor),
        7 => (orient_box(fx, fy, fz, -1, 0, 0, 5, 7, 5, dir), Kind::CorridorCrossing),
        8 => (orient_box(fx, fy, fz, -1, 0, 0, 5, 7, 5, dir), Kind::RightTurn { chest: false }),
        9 => (orient_box(fx, fy, fz, -1, 0, 0, 5, 7, 5, dir), Kind::LeftTurn { chest: false }),
        10 => (orient_box(fx, fy, fz, -1, -7, 0, 5, 14, 10, dir), Kind::CorridorStairs),
        11 => (orient_box(fx, fy, fz, -3, 0, 0, 9, 7, 9, dir), Kind::TBalcony { left: false, right: false }),
        _ => (orient_box(fx, fy, fz, -5, -3, 0, 13, 14, 13, dir), Kind::StalkRoom),
    };
    if !is_ok_box(&bb) || st.collides(&bb) {
        return None;
    }
    // Constructor rolls after the box succeeds (Java ctor order).
    let kind = match kind {
        Kind::LeftTurn { .. } => Kind::LeftTurn { chest: next_int(rng, 3) == 0 },
        Kind::RightTurn { .. } => Kind::RightTurn { chest: next_int(rng, 3) == 0 },
        other => other,
    };
    Some(Piece { kind, bb, dir })
}

/// `BridgeEndFiller.createPiece` (the fallback; rolls its self seed).
fn end_filler(st: &LayoutState, rng: &mut SplitMix64, fx: i32, fy: i32, fz: i32, dir: u8) -> Option<Piece> {
    let bb = orient_box(fx, fy, fz, -1, -3, 0, 5, 10, 8, dir);
    if !is_ok_box(&bb) || st.collides(&bb) {
        return None;
    }
    Some(Piece { kind: Kind::EndFiller { self_seed: rng.next_u64() }, bb, dir })
}

/// `generatePiece` + `generateAndAddPiece`: weighted pick with 5
/// attempts, the no-repeat rule, per-class caps (maxed classes leave
/// the table), the end-filler fallback, the 112-block envelope.
#[allow(clippy::too_many_arguments)]
fn generate_and_add(
    st: &mut LayoutState,
    rng: &mut SplitMix64,
    fx: i32,
    fy: i32,
    fz: i32,
    dir: u8,
    depth: i32,
    castle: bool,
) -> Option<usize> {
    if (fx - st.start_x0).abs() > ENVELOPE || (fz - st.start_z0).abs() > ENVELOPE {
        // Out of the envelope: Java creates a filler and DISCARDS it
        // (never added to the builder) - so nothing lands here.
        return None;
    }
    let child_depth = depth + 1;
    let table = if castle { &st.castle } else { &st.bridge };
    // updatePieceWeight: any capped class still open + the total.
    let any_open = table.iter().any(|p| p.max > 0 && p.count < p.max);
    let total: i32 = table.iter().map(|p| p.weight).sum();
    let mut created: Option<(Piece, u8)> = None;
    if any_open && child_depth <= MAX_DEPTH {
        'attempts: for _ in 0..5 {
            let mut pick = next_int(rng, total);
            let rows: Vec<PieceW> =
                if castle { st.castle.clone() } else { st.bridge.clone() };
            for row in rows {
                pick -= row.weight;
                if pick < 0 {
                    let do_place = row.max == 0 || row.count < row.max;
                    if !do_place || (st.previous == Some(row.class) && !row.allow_in_row) {
                        break;
                    }
                    if let Some(p) = try_create(st, rng, row.class, fx, fy, fz, dir) {
                        let tab = if castle { &mut st.castle } else { &mut st.bridge };
                        if let Some(r) = tab.iter_mut().find(|r| r.class == row.class) {
                            r.count += 1;
                            let maxed = r.max > 0 && r.count >= r.max;
                            if maxed {
                                tab.retain(|r| r.class != row.class);
                            }
                        }
                        st.previous = Some(row.class);
                        created = Some((p, row.class));
                        break 'attempts;
                    }
                }
            }
        }
    }
    let piece = match created {
        Some((p, _)) => p,
        None => end_filler(st, rng, fx, fy, fz, dir)?,
    };
    let idx = st.pieces.len();
    st.pieces.push(piece);
    st.depths.push(child_depth);
    Some(idx)
}

/// `generateChildForward` (xOff, yOff).
#[allow(clippy::too_many_arguments)]
fn child_forward(st: &mut LayoutState, rng: &mut SplitMix64, idx: usize, x_off: i32, y_off: i32, castle: bool) -> Option<usize> {
    let p = st.pieces[idx].clone();
    let d = st.depths[idx];
    match p.dir {
        N => generate_and_add(st, rng, p.bb.x0 + x_off, p.bb.y0 + y_off, p.bb.z0 - 1, N, d, castle),
        S => generate_and_add(st, rng, p.bb.x0 + x_off, p.bb.y0 + y_off, p.bb.z1 + 1, S, d, castle),
        W => generate_and_add(st, rng, p.bb.x0 - 1, p.bb.y0 + y_off, p.bb.z0 + x_off, W, d, castle),
        _ => generate_and_add(st, rng, p.bb.x1 + 1, p.bb.y0 + y_off, p.bb.z0 + x_off, E, d, castle),
    }
}

/// `generateChildLeft` (yOff, zOff).
#[allow(clippy::too_many_arguments)]
fn child_left(st: &mut LayoutState, rng: &mut SplitMix64, idx: usize, y_off: i32, z_off: i32, castle: bool) -> Option<usize> {
    let p = st.pieces[idx].clone();
    let d = st.depths[idx];
    match p.dir {
        N | S => generate_and_add(st, rng, p.bb.x0 - 1, p.bb.y0 + y_off, p.bb.z0 + z_off, W, d, castle),
        _ => generate_and_add(st, rng, p.bb.x0 + z_off, p.bb.y0 + y_off, p.bb.z0 - 1, N, d, castle),
    }
}

/// `generateChildRight` (yOff, zOff).
#[allow(clippy::too_many_arguments)]
fn child_right(st: &mut LayoutState, rng: &mut SplitMix64, idx: usize, y_off: i32, z_off: i32, castle: bool) -> Option<usize> {
    let p = st.pieces[idx].clone();
    let d = st.depths[idx];
    match p.dir {
        N | S => generate_and_add(st, rng, p.bb.x1 + 1, p.bb.y0 + y_off, p.bb.z0 + z_off, E, d, castle),
        _ => generate_and_add(st, rng, p.bb.x0 + z_off, p.bb.y0 + y_off, p.bb.z1 + 1, S, d, castle),
    }
}

/// `addChildren` per class - pushes new pending indices.
fn add_children(st: &mut LayoutState, rng: &mut SplitMix64, idx: usize, pending: &mut Vec<usize>) {
    let kind = st.pieces[idx].kind;
    let dir = st.pieces[idx].dir;
    let push = |o: Option<usize>, pending: &mut Vec<usize>| {
        if let Some(i) = o {
            pending.push(i);
        }
    };
    match kind {
        Kind::BridgeCrossing => {
            push(child_forward(st, rng, idx, 8, 3, false), pending);
            push(child_left(st, rng, idx, 3, 8, false), pending);
            push(child_right(st, rng, idx, 3, 8, false), pending);
        }
        Kind::BridgeStraight => {
            push(child_forward(st, rng, idx, 1, 3, false), pending);
        }
        Kind::RoomCrossing => {
            push(child_forward(st, rng, idx, 2, 0, false), pending);
            push(child_left(st, rng, idx, 0, 2, false), pending);
            push(child_right(st, rng, idx, 0, 2, false), pending);
        }
        Kind::StairsRoom => {
            push(child_right(st, rng, idx, 6, 2, false), pending);
        }
        Kind::MonsterThrone | Kind::EndFiller { .. } => {}
        Kind::CastleEntrance => {
            push(child_forward(st, rng, idx, 5, 3, true), pending);
        }
        Kind::SmallCorridor | Kind::CorridorStairs => {
            push(child_forward(st, rng, idx, 1, 0, true), pending);
        }
        Kind::CorridorCrossing => {
            push(child_forward(st, rng, idx, 1, 0, true), pending);
            push(child_left(st, rng, idx, 0, 1, true), pending);
            push(child_right(st, rng, idx, 0, 1, true), pending);
        }
        Kind::LeftTurn { .. } => {
            push(child_left(st, rng, idx, 0, 1, true), pending);
        }
        Kind::RightTurn { .. } => {
            push(child_right(st, rng, idx, 0, 1, true), pending);
        }
        Kind::TBalcony { .. } => {
            // zOff 1, or 5 when facing W/N; each side gated 7-in-8.
            let z_off = if dir == W || dir == N { 5 } else { 1 };
            let left_roll = next_int(rng, 8) > 0;
            if left_roll {
                push(child_left(st, rng, idx, 0, z_off, true), pending);
            }
            let right_roll = next_int(rng, 8) > 0;
            if right_roll {
                push(child_right(st, rng, idx, 0, z_off, true), pending);
            }
        }
        Kind::StalkRoom => {
            push(child_forward(st, rng, idx, 5, 3, true), pending);
            push(child_forward(st, rng, idx, 5, 11, true), pending);
        }
    }
}

/// Solve (and cache) the fortress layout of a start chunk.
pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Vec<Piece>>> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<Vec<Piece>>>>;
    thread_local! {
        static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
    }
    let seed = ctx.effective_seed();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned()) {
        return hit;
    }
    let built = Some(Rc::new(build_layout(ctx, scx, scz)));
    CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() > 64 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Vec<Piece> {
    let seed = ctx.effective_seed();
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_FORTRESS_LAYOUT));
    let dir = next_int(&mut rng, 4) as u8;
    let west = scx * 16 + 2;
    let north = scz * 16 + 2;
    // StartPiece: BridgeCrossing.makeBoundingBox(west, 64, north, dir,
    // 19, 10, 19) - the footprint transpose ScatteredPiece::new does.
    let start_frame = ScatteredPiece::new(west, MAGIC_START_Y, north, dir, 19, 10, 19);
    let mut st = LayoutState {
        pieces: vec![Piece { kind: Kind::BridgeCrossing, bb: start_frame.bb, dir }],
        depths: vec![0],
        bridge: bridge_weights(),
        castle: castle_weights(),
        previous: None,
        start_x0: start_frame.bb.x0,
        start_z0: start_frame.bb.z0,
    };
    let mut pending: Vec<usize> = Vec::new();
    add_children(&mut st, &mut rng, 0, &mut pending);
    while !pending.is_empty() {
        let pick = next_int(&mut rng, pending.len() as i32) as usize;
        let idx = pending.remove(pick);
        add_children(&mut st, &mut rng, idx, &mut pending);
    }
    // moveInsideHeights(random, 48, 70): shift the whole maze so its
    // bb starts at a random y in [48, 70 - ySpan].
    let mut y0 = i32::MAX;
    let mut y1 = i32::MIN;
    for p in &st.pieces {
        y0 = y0.min(p.bb.y0);
        y1 = y1.max(p.bb.y1);
    }
    let span = y1 - y0 + 1;
    let room = HEIGHTS_MAX - HEIGHTS_MIN + 1 - span;
    let target = if room > 0 { HEIGHTS_MIN + next_int(&mut rng, room) } else { HEIGHTS_MIN };
    let delta = target - y0;
    for p in &mut st.pieces {
        p.bb.y0 += delta;
        p.bb.y1 += delta;
    }
    st.pieces
}

// ----------------------------------------------------------------
// Carving.
// ----------------------------------------------------------------

/// The fortress block palette (engine ids; fence connections and
/// stairs facings drop to the single engine block).
struct Palette {
    bricks: u16,
    fence: u16,
    stairs: u16,
    soul_sand: u16,
    wart: u16,
    chest: u16,
    spawner: u16,
    lava: u16,
}

impl Palette {
    fn get() -> Palette {
        let id = |n: &str| super::features::block_id_by_name(n).unwrap_or(0);
        Palette {
            bricks: id("nether_bricks"),
            fence: id("nether_brick_fence"),
            stairs: id("nether_brick_stairs"),
            soul_sand: id("soul_sand"),
            wart: id("nether_wart"),
            chest: crate::block::CHEST_ID,
            spawner: crate::block::SPAWNER_ID,
            lava: 37,
        }
    }
}

/// The chunk-clipped carve target (writes clip to the live cube;
/// `fillColumnDown` continuation reads the carver-aware terrain).
struct Sink<'a> {
    chunk: &'a mut Chunk,
    origin: [i32; 3],
    ctx: &'a WorldgenCtx,
    frame: ScatteredPiece,
    pal: &'a Palette,
}

impl Sink<'_> {
    fn put_world(&mut self, pos: [i32; 3], id: u16) {
        let cs = CHUNK_SIZE as i32;
        let (lx, ly, lz) =
            (pos[0] - self.origin[0], pos[1] - self.origin[1], pos[2] - self.origin[2]);
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            self.chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
        }
    }

    fn put(&mut self, x: i32, y: i32, z: i32, id: u16) {
        let pos = self.frame.world(x, y, z);
        self.put_world(pos, id);
    }

    /// `generateBox` with a constant fill (skipAir is never used by
    /// the fortress pieces).
    fn boxed(&mut self, a: (i32, i32, i32), b: (i32, i32, i32), id: u16) {
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                for z in a.2..=b.2 {
                    self.put(x, y, z, id);
                }
            }
        }
    }

    /// `fillColumnDown`: bricks from local y downward while the
    /// TERRAIN below is air or lava (the carver-aware oracle keeps
    /// every vertical slice's walk identical; fortress legs pierce
    /// the lava sea like MC's).
    fn fill_down(&mut self, x: i32, y: i32, z: i32) {
        let pos = self.frame.world(x, y, z);
        let mut wy = pos[1];
        while wy > 0 {
            let t = nether_mc::nether_block_at(self.ctx, pos[0], wy, pos[2]);
            if t != 0 && t != self.pal.lava {
                break;
            }
            self.put_world([pos[0], wy, pos[2]], self.pal.bricks);
            wy -= 1;
        }
    }
}

/// Carve one piece (Java `postProcess`, boxes in source order).
fn carve_piece(s: &mut Sink<'_>, piece: &Piece) {
    let nb = s.pal.bricks;
    let fence = s.pal.fence;
    match piece.kind {
        Kind::BridgeStraight => {
            s.boxed((0, 3, 0), (4, 4, 18), nb);
            s.boxed((1, 5, 0), (3, 7, 18), 0);
            s.boxed((0, 5, 0), (0, 5, 18), nb);
            s.boxed((4, 5, 0), (4, 5, 18), nb);
            s.boxed((0, 2, 0), (4, 2, 5), nb);
            s.boxed((0, 2, 13), (4, 2, 18), nb);
            s.boxed((0, 0, 0), (4, 1, 3), nb);
            s.boxed((0, 0, 15), (4, 1, 18), nb);
            for x in 0..=4 {
                for z in 0..=2 {
                    s.fill_down(x, -1, z);
                    s.fill_down(x, -1, 18 - z);
                }
            }
            // The fence railings.
            s.boxed((0, 1, 1), (0, 4, 1), fence);
            s.boxed((0, 3, 4), (0, 4, 4), fence);
            s.boxed((0, 3, 14), (0, 4, 14), fence);
            s.boxed((0, 1, 17), (0, 4, 17), fence);
            s.boxed((4, 1, 1), (4, 4, 1), fence);
            s.boxed((4, 3, 4), (4, 4, 4), fence);
            s.boxed((4, 3, 14), (4, 4, 14), fence);
            s.boxed((4, 1, 17), (4, 4, 17), fence);
        }
        Kind::BridgeCrossing => {
            s.boxed((7, 3, 0), (11, 4, 18), nb);
            s.boxed((0, 3, 7), (18, 4, 11), nb);
            s.boxed((8, 5, 0), (10, 7, 18), 0);
            s.boxed((0, 5, 8), (18, 7, 10), 0);
            s.boxed((7, 5, 0), (7, 5, 7), nb);
            s.boxed((7, 5, 11), (7, 5, 18), nb);
            s.boxed((11, 5, 0), (11, 5, 7), nb);
            s.boxed((11, 5, 11), (11, 5, 18), nb);
            s.boxed((0, 5, 7), (7, 5, 7), nb);
            s.boxed((11, 5, 7), (18, 5, 7), nb);
            s.boxed((0, 5, 11), (7, 5, 11), nb);
            s.boxed((11, 5, 11), (18, 5, 11), nb);
            s.boxed((7, 2, 0), (11, 2, 5), nb);
            s.boxed((7, 2, 13), (11, 2, 18), nb);
            s.boxed((7, 0, 0), (11, 1, 3), nb);
            s.boxed((7, 0, 15), (11, 1, 18), nb);
            for x in 7..=11 {
                for z in 0..=2 {
                    s.fill_down(x, -1, z);
                    s.fill_down(x, -1, 18 - z);
                }
            }
            s.boxed((0, 2, 7), (5, 2, 11), nb);
            s.boxed((13, 2, 7), (18, 2, 11), nb);
            s.boxed((0, 0, 7), (3, 1, 11), nb);
            s.boxed((15, 0, 7), (18, 1, 11), nb);
            for x in 0..=2 {
                for z in 7..=11 {
                    s.fill_down(x, -1, z);
                    s.fill_down(18 - x, -1, z);
                }
            }
        }
        Kind::RoomCrossing => {
            s.boxed((0, 0, 0), (6, 1, 6), nb);
            s.boxed((0, 2, 0), (6, 7, 6), 0);
            s.boxed((0, 2, 0), (1, 6, 0), nb);
            s.boxed((0, 2, 6), (1, 6, 6), nb);
            s.boxed((5, 2, 0), (6, 6, 0), nb);
            s.boxed((5, 2, 6), (6, 6, 6), nb);
            s.boxed((0, 2, 0), (0, 6, 1), nb);
            s.boxed((0, 2, 5), (0, 6, 6), nb);
            s.boxed((6, 2, 0), (6, 6, 1), nb);
            s.boxed((6, 2, 5), (6, 6, 6), nb);
            s.boxed((2, 6, 0), (4, 6, 0), nb);
            s.boxed((2, 5, 0), (4, 5, 0), fence);
            s.boxed((2, 6, 6), (4, 6, 6), nb);
            s.boxed((2, 5, 6), (4, 5, 6), fence);
            s.boxed((0, 6, 2), (0, 6, 4), nb);
            s.boxed((0, 5, 2), (0, 5, 4), fence);
            s.boxed((6, 6, 2), (6, 6, 4), nb);
            s.boxed((6, 5, 2), (6, 5, 4), fence);
            for x in 0..=6 {
                for z in 0..=6 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::StairsRoom => {
            s.boxed((0, 0, 0), (6, 1, 6), nb);
            s.boxed((0, 2, 0), (6, 10, 6), 0);
            s.boxed((0, 2, 0), (1, 8, 0), nb);
            s.boxed((5, 2, 0), (6, 8, 0), nb);
            s.boxed((0, 2, 1), (0, 8, 6), nb);
            s.boxed((6, 2, 1), (6, 8, 6), nb);
            s.boxed((1, 2, 6), (5, 8, 6), nb);
            s.boxed((0, 3, 2), (0, 5, 4), fence);
            s.boxed((6, 3, 2), (6, 5, 2), fence);
            s.boxed((6, 3, 4), (6, 5, 4), fence);
            s.put(5, 2, 5, nb);
            s.boxed((4, 2, 5), (4, 3, 5), nb);
            s.boxed((3, 2, 5), (3, 4, 5), nb);
            s.boxed((2, 2, 5), (2, 5, 5), nb);
            s.boxed((1, 2, 5), (1, 6, 5), nb);
            s.boxed((1, 7, 1), (5, 7, 4), nb);
            s.boxed((6, 8, 2), (6, 8, 4), 0);
            s.boxed((2, 6, 0), (4, 8, 0), nb);
            s.boxed((2, 5, 0), (4, 5, 0), fence);
            for x in 0..=6 {
                for z in 0..=6 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::MonsterThrone => {
            s.boxed((0, 2, 0), (6, 7, 7), 0);
            s.boxed((1, 0, 0), (5, 1, 7), nb);
            s.boxed((1, 2, 1), (5, 2, 7), nb);
            s.boxed((1, 3, 2), (5, 3, 7), nb);
            s.boxed((1, 4, 3), (5, 4, 7), nb);
            s.boxed((1, 2, 0), (1, 4, 2), nb);
            s.boxed((5, 2, 0), (5, 4, 2), nb);
            s.boxed((1, 5, 2), (1, 5, 3), nb);
            s.boxed((5, 5, 2), (5, 5, 3), nb);
            s.boxed((0, 5, 3), (0, 5, 8), nb);
            s.boxed((6, 5, 3), (6, 5, 8), nb);
            s.boxed((1, 5, 8), (5, 5, 8), nb);
            s.put(1, 6, 3, fence);
            s.put(5, 6, 3, fence);
            s.put(0, 6, 3, fence);
            s.put(6, 6, 3, fence);
            s.boxed((0, 6, 4), (0, 6, 7), fence);
            s.boxed((6, 6, 4), (6, 6, 7), fence);
            s.put(0, 6, 8, fence);
            s.put(6, 6, 8, fence);
            s.boxed((1, 6, 8), (5, 6, 8), fence);
            s.put(1, 7, 8, fence);
            s.boxed((2, 7, 8), (4, 7, 8), fence);
            s.put(5, 7, 8, fence);
            s.put(2, 8, 8, fence);
            s.put(3, 8, 8, fence);
            s.put(4, 8, 8, fence);
            // The blaze spawner (chunk-clipped one-shot in MC; the
            // world position is unique, so the per-chunk replay is
            // equivalent).
            let spawner = s.pal.spawner;
            s.put(3, 5, 5, spawner);
            for x in 0..=6 {
                for z in 0..=6 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::CastleEntrance | Kind::StalkRoom => {
            s.boxed((0, 3, 0), (12, 4, 12), nb);
            s.boxed((0, 5, 0), (12, 13, 12), 0);
            s.boxed((0, 5, 0), (1, 12, 12), nb);
            s.boxed((11, 5, 0), (12, 12, 12), nb);
            s.boxed((2, 5, 11), (4, 12, 12), nb);
            s.boxed((8, 5, 11), (10, 12, 12), nb);
            s.boxed((5, 9, 11), (7, 12, 12), nb);
            s.boxed((2, 5, 0), (4, 12, 1), nb);
            s.boxed((8, 5, 0), (10, 12, 1), nb);
            s.boxed((5, 9, 0), (7, 12, 1), nb);
            s.boxed((2, 11, 2), (10, 12, 10), nb);
            if piece.kind == Kind::CastleEntrance {
                s.boxed((5, 8, 0), (7, 8, 0), fence);
            }
            // The window fences + roof crenellation ring.
            let mut i = 1;
            while i <= 11 {
                s.boxed((i, 10, 0), (i, 11, 0), fence);
                s.boxed((i, 10, 12), (i, 11, 12), fence);
                s.boxed((0, 10, i), (0, 11, i), fence);
                s.boxed((12, 10, i), (12, 11, i), fence);
                s.put(i, 13, 0, nb);
                s.put(i, 13, 12, nb);
                s.put(0, 13, i, nb);
                s.put(12, 13, i, nb);
                if i != 11 {
                    s.put(i + 1, 13, 0, fence);
                    s.put(i + 1, 13, 12, fence);
                    s.put(0, 13, i + 1, fence);
                    s.put(12, 13, i + 1, fence);
                }
                i += 2;
            }
            s.put(0, 13, 0, fence);
            s.put(0, 13, 12, fence);
            s.put(12, 13, 12, fence);
            s.put(12, 13, 0, fence);
            let mut z = 3;
            while z <= 9 {
                s.boxed((1, 7, z), (1, 8, z), fence);
                s.boxed((11, 7, z), (11, 8, z), fence);
                z += 2;
            }
            if piece.kind == Kind::StalkRoom {
                // The wart-garden stalk staircase.
                let stairs = s.pal.stairs;
                for i in 0..=6 {
                    let z = i + 4;
                    for x in 5..=7 {
                        s.put(x, 5 + i, z, stairs);
                    }
                    if (5..=8).contains(&z) {
                        s.boxed((5, 5, z), (7, i + 4, z), nb);
                    } else if (9..=10).contains(&z) {
                        s.boxed((5, 8, z), (7, i + 4, z), nb);
                    }
                    if i >= 1 {
                        s.boxed((5, 6 + i, z), (7, 9 + i, z), 0);
                    }
                }
                for x in 5..=7 {
                    s.put(x, 12, 11, stairs);
                }
                s.boxed((5, 6, 7), (5, 7, 7), fence);
                s.boxed((7, 6, 7), (7, 7, 7), fence);
                s.boxed((5, 13, 12), (7, 13, 12), 0);
                s.boxed((2, 5, 2), (3, 5, 3), nb);
                s.boxed((2, 5, 9), (3, 5, 10), nb);
                s.boxed((2, 5, 4), (2, 5, 8), nb);
                s.boxed((9, 5, 2), (10, 5, 3), nb);
                s.boxed((9, 5, 9), (10, 5, 10), nb);
                s.boxed((10, 5, 4), (10, 5, 8), nb);
                for (x, zs) in [(4, [2, 3, 9, 10]), (8, [2, 3, 9, 10])] {
                    for z in zs {
                        s.put(x, 5, z, stairs);
                    }
                }
                let (sand, wart) = (s.pal.soul_sand, s.pal.wart);
                s.boxed((3, 4, 4), (4, 4, 8), sand);
                s.boxed((8, 4, 4), (9, 4, 8), sand);
                s.boxed((3, 5, 4), (4, 5, 8), wart);
                s.boxed((8, 5, 4), (9, 5, 8), wart);
            }
            s.boxed((4, 2, 0), (8, 2, 12), nb);
            s.boxed((0, 2, 4), (12, 2, 8), nb);
            s.boxed((4, 0, 0), (8, 1, 3), nb);
            s.boxed((4, 0, 9), (8, 1, 12), nb);
            s.boxed((0, 0, 4), (3, 1, 8), nb);
            s.boxed((9, 0, 4), (12, 1, 8), nb);
            for x in 4..=8 {
                for z in 0..=2 {
                    s.fill_down(x, -1, z);
                    s.fill_down(x, -1, 12 - z);
                }
            }
            for x in 0..=2 {
                for z in 4..=8 {
                    s.fill_down(x, -1, z);
                    s.fill_down(12 - x, -1, z);
                }
            }
            if piece.kind == Kind::CastleEntrance {
                // The lava well.
                s.boxed((5, 5, 5), (7, 5, 7), nb);
                s.boxed((6, 1, 6), (6, 4, 6), 0);
                s.put(6, 0, 6, nb);
                let lava = s.pal.lava;
                s.put(6, 5, 6, lava);
            }
        }
        Kind::SmallCorridor => {
            s.boxed((0, 0, 0), (4, 1, 4), nb);
            s.boxed((0, 2, 0), (4, 5, 4), 0);
            s.boxed((0, 2, 0), (0, 5, 4), nb);
            s.boxed((4, 2, 0), (4, 5, 4), nb);
            s.boxed((0, 3, 1), (0, 4, 1), fence);
            s.boxed((0, 3, 3), (0, 4, 3), fence);
            s.boxed((4, 3, 1), (4, 4, 1), fence);
            s.boxed((4, 3, 3), (4, 4, 3), fence);
            s.boxed((0, 6, 0), (4, 6, 4), nb);
            for x in 0..=4 {
                for z in 0..=4 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::CorridorCrossing => {
            s.boxed((0, 0, 0), (4, 1, 4), nb);
            s.boxed((0, 2, 0), (4, 5, 4), 0);
            s.boxed((0, 2, 0), (0, 5, 0), nb);
            s.boxed((4, 2, 0), (4, 5, 0), nb);
            s.boxed((0, 2, 4), (0, 5, 4), nb);
            s.boxed((4, 2, 4), (4, 5, 4), nb);
            s.boxed((0, 6, 0), (4, 6, 4), nb);
            for x in 0..=4 {
                for z in 0..=4 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::LeftTurn { chest } => {
            s.boxed((0, 0, 0), (4, 1, 4), nb);
            s.boxed((0, 2, 0), (4, 5, 4), 0);
            s.boxed((4, 2, 0), (4, 5, 4), nb);
            s.boxed((4, 3, 1), (4, 4, 1), fence);
            s.boxed((4, 3, 3), (4, 4, 3), fence);
            s.boxed((0, 2, 0), (0, 5, 0), nb);
            s.boxed((0, 2, 4), (3, 5, 4), nb);
            s.boxed((1, 3, 4), (1, 4, 4), fence);
            s.boxed((3, 3, 4), (3, 4, 4), fence);
            if chest {
                let c = s.pal.chest;
                s.put(3, 2, 3, c);
            }
            s.boxed((0, 6, 0), (4, 6, 4), nb);
            for x in 0..=4 {
                for z in 0..=4 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::RightTurn { chest } => {
            s.boxed((0, 0, 0), (4, 1, 4), nb);
            s.boxed((0, 2, 0), (4, 5, 4), 0);
            s.boxed((0, 2, 0), (0, 5, 4), nb);
            s.boxed((0, 3, 1), (0, 4, 1), fence);
            s.boxed((0, 3, 3), (0, 4, 3), fence);
            s.boxed((4, 2, 0), (4, 5, 0), nb);
            s.boxed((1, 2, 4), (4, 5, 4), nb);
            s.boxed((1, 3, 4), (1, 4, 4), fence);
            s.boxed((3, 3, 4), (3, 4, 4), fence);
            if chest {
                let c = s.pal.chest;
                s.put(1, 2, 3, c);
            }
            s.boxed((0, 6, 0), (4, 6, 4), nb);
            for x in 0..=4 {
                for z in 0..=4 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::CorridorStairs => {
            let stairs = s.pal.stairs;
            for step in 0..=9 {
                let floor = (7 - step).max(1);
                let roof = ((floor + 5).max(14 - step)).min(13);
                let z = step;
                s.boxed((0, 0, z), (4, floor, z), nb);
                s.boxed((1, floor + 1, z), (3, roof - 1, z), 0);
                if step <= 6 {
                    s.put(1, floor + 1, z, stairs);
                    s.put(2, floor + 1, z, stairs);
                    s.put(3, floor + 1, z, stairs);
                }
                s.boxed((0, roof, z), (4, roof, z), nb);
                s.boxed((0, floor + 1, z), (0, roof - 1, z), nb);
                s.boxed((4, floor + 1, z), (4, roof - 1, z), nb);
                if step & 1 == 0 {
                    s.boxed((0, floor + 2, z), (0, floor + 3, z), fence);
                    s.boxed((4, floor + 2, z), (4, floor + 3, z), fence);
                }
                for x in 0..=4 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::TBalcony { .. } => {
            s.boxed((0, 0, 0), (8, 1, 8), nb);
            s.boxed((0, 2, 0), (8, 5, 8), 0);
            s.boxed((0, 6, 0), (8, 6, 5), nb);
            s.boxed((0, 2, 0), (2, 5, 0), nb);
            s.boxed((6, 2, 0), (8, 5, 0), nb);
            s.boxed((1, 3, 0), (1, 4, 0), fence);
            s.boxed((7, 3, 0), (7, 4, 0), fence);
            s.boxed((0, 2, 4), (8, 2, 8), nb);
            s.boxed((1, 1, 4), (2, 2, 4), 0);
            s.boxed((6, 1, 4), (7, 2, 4), 0);
            s.boxed((1, 3, 8), (7, 3, 8), fence);
            s.put(0, 3, 8, fence);
            s.put(8, 3, 8, fence);
            s.boxed((0, 3, 6), (0, 3, 7), fence);
            s.boxed((8, 3, 6), (8, 3, 7), fence);
            s.boxed((0, 3, 4), (0, 5, 5), nb);
            s.boxed((8, 3, 4), (8, 5, 5), nb);
            s.boxed((1, 3, 5), (2, 5, 5), nb);
            s.boxed((6, 3, 5), (7, 5, 5), nb);
            s.boxed((1, 4, 5), (1, 5, 5), fence);
            s.boxed((7, 4, 5), (7, 5, 5), fence);
            for z in 0..=5 {
                for x in 0..=8 {
                    s.fill_down(x, -1, z);
                }
            }
        }
        Kind::EndFiller { self_seed } => {
            // The stored per-piece seed replays the ragged bridge end
            // identically in every chunk (why MC serializes it).
            let mut r = SplitMix64::new(self_seed);
            for x in 0..=4 {
                for y in 3..=4 {
                    let z = next_int(&mut r, 8);
                    s.boxed((x, y, 0), (x, y, z), nb);
                }
            }
            let z = next_int(&mut r, 8);
            s.boxed((0, 5, 0), (0, 5, z), nb);
            let z = next_int(&mut r, 8);
            s.boxed((4, 5, 0), (4, 5, z), nb);
            for x in 0..=4 {
                let z = next_int(&mut r, 5);
                s.boxed((x, 2, 0), (x, 2, z), nb);
            }
            for x in 0..=4 {
                for y in 0..=1 {
                    let z = next_int(&mut r, 3);
                    s.boxed((x, y, 0), (x, y, z), nb);
                }
            }
        }
    }
}

// ----------------------------------------------------------------
// The per-chunk pass + probes.
// ----------------------------------------------------------------

/// Envelope 112 + the largest piece extent (19) in chunks.
const REACH: i32 = 9;

/// The per-chunk fortress pass (Nether only; called from
/// `place_from_coord_nether` before the decoration steps).
pub(crate) fn place_fortresses(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    let pal = Palette::get();
    let cs = CHUNK_SIZE as i32;
    let cube = Bb {
        x0: origin[0],
        y0: origin[1],
        z0: origin[2],
        x1: origin[0] + cs - 1,
        y1: origin[1] + cs - 1,
        z1: origin[2] + cs - 1,
    };
    for gx in (coord[0] - REACH).div_euclid(bastion::SPACING)
        ..=(coord[0] + REACH).div_euclid(bastion::SPACING)
    {
        for gz in (coord[2] - REACH).div_euclid(bastion::SPACING)
            ..=(coord[2] + REACH).div_euclid(bastion::SPACING)
        {
            let ([scx, scz], kind) = bastion::potential_start(seed, gx, gz);
            if kind != NetherComplex::Fortress {
                continue;
            }
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            for piece in layout.iter() {
                // fill_down runs below the bb - widen the vertical
                // clip to the whole band.
                let mut reach_bb = piece.bb;
                reach_bb.y0 = 0;
                if !reach_bb.intersects(&cube) {
                    continue;
                }
                let mut sink = Sink {
                    chunk,
                    origin,
                    ctx,
                    frame: piece.frame(),
                    pal: &pal,
                };
                carve_piece(&mut sink, piece);
            }
        }
    }
}

/// Loot probe: is `pos` one of the fortress corridor chests?
pub(crate) fn is_fortress_chest(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    fortress_probe(ctx, pos, |piece, frame| match piece.kind {
        Kind::LeftTurn { chest: true } => frame.world(3, 2, 3) == pos,
        Kind::RightTurn { chest: true } => frame.world(1, 2, 3) == pos,
        _ => false,
    })
}

/// Spawner probe: is `pos` a monster-throne blaze spawner?
pub(crate) fn is_throne_spawner(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    fortress_probe(ctx, pos, |piece, frame| {
        piece.kind == Kind::MonsterThrone && frame.world(3, 5, 5) == pos
    })
}

fn fortress_probe(
    ctx: &WorldgenCtx,
    pos: [i32; 3],
    hit: impl Fn(&Piece, &ScatteredPiece) -> bool,
) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Nether) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    let seed = ctx.effective_seed();
    for gx in (ccx - REACH).div_euclid(bastion::SPACING)
        ..=(ccx + REACH).div_euclid(bastion::SPACING)
    {
        for gz in (ccz - REACH).div_euclid(bastion::SPACING)
            ..=(ccz + REACH).div_euclid(bastion::SPACING)
        {
            let ([scx, scz], kind) = bastion::potential_start(seed, gx, gz);
            if kind != NetherComplex::Fortress {
                continue;
            }
            if (scx - ccx).abs() > REACH || (scz - ccz).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            for piece in layout.iter() {
                if !piece.bb.contains(pos) {
                    continue;
                }
                let frame = piece.frame();
                if hit(piece, &frame) {
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
    use crate::worldgen::WorldGen;

    fn ctx() -> WorldgenCtx {
        WorldgenCtx {
            base_seed: 3592732596u32,
            dim: "nether".into(),
            profile: WorldGen::Nether,
        }
    }

    /// A fortress layout solves with a real maze (bridges + castle
    /// interior + at least one throne or stalk room in a scan).
    #[test]
    fn a_fortress_layout_solves() {
        let c = ctx();
        let mut best: Option<((i32, i32), usize)> = None;
        let mut thrones = 0usize;
        let mut chests = 0usize;
        for gx in -8..8 {
            for gz in -8..8 {
                let ([sx, sz], kind) = bastion::potential_start(c.effective_seed(), gx, gz);
                if kind != NetherComplex::Fortress {
                    continue;
                }
                let layout = layout_for_start(&c, sx, sz).expect("layout");
                thrones += layout
                    .iter()
                    .filter(|p| p.kind == Kind::MonsterThrone)
                    .count();
                chests += layout
                    .iter()
                    .filter(|p| {
                        matches!(
                            p.kind,
                            Kind::LeftTurn { chest: true } | Kind::RightTurn { chest: true }
                        )
                    })
                    .count();
                if best.as_ref().is_none_or(|(_, n)| layout.len() > *n) {
                    best = Some(((sx, sz), layout.len()));
                }
                // Every piece stays inside the dimension's chunk band
                // (bridges chain +3 per hop, so rare layouts overtop
                // the y-127 terrain roof exactly like MC's do - the
                // nether's build band runs to 255).
                for p in layout.iter() {
                    assert!(p.bb.y0 > LOWEST_Y, "piece below the floor: {:?}", p.bb);
                    assert!(p.bb.y1 < 256, "piece past the build band: {:?}", p.bb);
                }
            }
        }
        let ((sx, sz), pieces) = best.expect("no fortress cells in the scan");
        assert!(pieces > 10, "fortress at ({sx},{sz}) too small: {pieces} pieces");
        assert!(thrones > 0, "no monster thrones across the scan");
        assert!(chests > 0, "no corridor chests across the scan");
    }

    /// Full pipeline: the start crossing materialises nether bricks
    /// in the generated chunks.
    #[test]
    fn fortress_carves_nether_bricks() {
        let c = ctx();
        let cs = CHUNK_SIZE as i32;
        let nb = super::super::features::block_id_by_name("nether_bricks").unwrap();
        let mut found = None;
        'search: for gx in -4..4 {
            for gz in -4..4 {
                let ([sx, sz], kind) = bastion::potential_start(c.effective_seed(), gx, gz);
                if kind == NetherComplex::Fortress {
                    found = Some([sx, sz]);
                    break 'search;
                }
            }
        }
        let [sx, sz] = found.expect("no fortress cell");
        let layout = layout_for_start(&c, sx, sz).expect("layout");
        let start_bb = layout[0].bb;
        let cx = (start_bb.x0 + start_bb.x1) / 2;
        let cy = (start_bb.y0 + start_bb.y1) / 2;
        let cz = (start_bb.z0 + start_bb.z1) / 2;
        let mut bricks = 0usize;
        for dy in -1..=1 {
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs) + dy, cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &c);
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        if chunk.get(lx, ly, lz).0 == nb {
                            bricks += 1;
                        }
                    }
                }
            }
        }
        assert!(bricks > 100, "the start crossing carved {bricks} bricks");
    }
}
