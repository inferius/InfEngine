//! W256 - stronghold (`StrongholdStructure` + `StrongholdPieces.java`
//! port) - the End-portal structure, and the first CONCENTRIC RINGS
//! placement.
//!
//! Placement (`structure_set/strongholds.json`): 128 positions on
//! rings around the origin - distance 32, spread 3 (the ring math is
//! ported verbatim; the biome snap searches the nearest
//! `#stronghold_biased_to` biome within 112 blocks on a 16-block
//! lattice where MC spiral-samples quarts - own-search approximation
//! on top of the own-streams doctrine).
//!
//! Layout (`generatePieces`): retry loop bumping the seed until the
//! PORTAL ROOM lands; the start spiral staircase at (chunk+2, y 64),
//! children drawn from the weighted piece table (weights + max place
//! counts + the library>4 / portal>5 depth gates + the no-repeat
//! rule + the imposed FiveCrossing after the source stairs), pending
//! pieces expanded in RANDOM order, collisions against every placed
//! box, the FillerCorridor fallback, then `moveBelowSeaLevel(63, 10)`
//! drops the whole maze under y 52.
//!
//! Carving mirrors the Java pieces block-for-block. The
//! SmoothStoneSelector rolls per cell (<0.2 cracked / <0.5 mossy /
//! <0.55 infested -> plain bricks / else stone bricks; interior =
//! air) - pure per world cell like every structure stream. The
//! `generateBox(..., true, ...)` "skip where already air" semantics
//! read the live cube cell, which is exactly how strongholds keep
//! cave openings in MC. End-portal frames place (eye states dropped -
//! no block state yet); the all-12-eyes pre-lit portal (p = 1e-12)
//! is skipped with the End itself. The portal-room spawner is
//! SILVERFISH in MC - the worker kind probe hands out the spider
//! stand-in until the mob exists.
//!
//! Honest gaps: wall torches -> floor torches (orient map),
//! stairs/door/button facings drop to engine defaults, iron-bar and
//! fence connection states drop (the engine derives them), the
//! `bury` terrain adaptation is unnecessary (pieces sit below y 52,
//! already underground).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::registry_id;
use super::features::stream_seed;
use super::mineshaft::{Bb, ChunkShaftWorld, ShaftWorld};
use super::noise::SplitMix64;
use super::scattered::ScatteredPiece;
use super::terrain::mc_full_biome_at;
use crate::worldgen::WorldgenCtx;

/// `structure_set/strongholds.json`.
const COUNT: usize = 128;
const DISTANCE: i32 = 32;
const SPREAD_START: i32 = 3;
const SALT_RINGS: u64 = 0xD4E6_00A0;
const SALT_LAYOUT: u64 = 0xD4E6_00A1;
const SALT_STONE: u64 = 0xD4E6_00A2;
const SALT_MAYBE: u64 = 0xD4E6_00A3;
/// `MAX_DEPTH` / the 112-block envelope / `LOWEST_Y_POSITION`.
const MAX_DEPTH: i32 = 50;
const ENVELOPE: i32 = 112;
const LOWEST_Y: i32 = 10;
const MAGIC_START_Y: i32 = 64;
const SEA_LEVEL: i32 = 63;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_f32(r: &mut SplitMix64) -> f32 {
    (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

fn next_f64(r: &mut SplitMix64) -> f64 {
    (r.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

fn next_bool(r: &mut SplitMix64) -> bool {
    r.next_u64() & 1 == 1
}

// ----------------------------------------------------------------
// Concentric rings placement.
// ----------------------------------------------------------------

/// `#stronghold_biased_to`: every land biome (the tag excludes
/// oceans, rivers, beaches, shores and swamps).
fn biome_biased_to(b: McBiome) -> bool {
    !matches!(
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
            | McBiome::River
            | McBiome::FrozenRiver
            | McBiome::Beach
            | McBiome::SnowyBeach
            | McBiome::StonyShore
            | McBiome::Swamp
            | McBiome::MangroveSwamp
    )
}

/// `generateRingPositions`: the 128 stronghold start chunks of a
/// seed. Cached per effective seed - the list is world-global.
pub(crate) fn stronghold_positions(ctx: &WorldgenCtx) -> Rc<Vec<[i32; 2]>> {
    thread_local! {
        static CACHE: RefCell<HashMap<u32, Rc<Vec<[i32; 2]>>>> =
            RefCell::new(HashMap::new());
    }
    let seed = ctx.effective_seed();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&seed).cloned()) {
        return hit;
    }
    let built = Rc::new(generate_ring_positions(ctx));
    CACHE.with(|c| c.borrow_mut().insert(seed, built.clone()));
    built
}

fn generate_ring_positions(ctx: &WorldgenCtx) -> Vec<[i32; 2]> {
    let seed = ctx.effective_seed();
    let mut rng = SplitMix64::new(stream_seed(seed, 0, 0, SALT_RINGS));
    let mut angle = next_f64(&mut rng) * std::f64::consts::TAU;
    let mut spread = SPREAD_START;
    let mut in_circle = 0;
    let mut circle = 0;
    let mut out = Vec::with_capacity(COUNT);
    for i in 0..COUNT {
        let dist = (4 * DISTANCE) as f64
            + (DISTANCE * circle * 6) as f64
            + (next_f64(&mut rng) - 0.5) * (DISTANCE as f64 * 2.5);
        let mut cx = (angle.cos() * dist).round() as i32;
        let mut cz = (angle.sin() * dist).round() as i32;
        // findBiomeHorizontal: the nearest biased biome within 112
        // blocks of the chunk centre (16-block lattice scan).
        'snap: for radius in 0i32..=7 {
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if dx.abs().max(dz.abs()) != radius {
                        continue;
                    }
                    let bx = cx * 16 + 8 + dx * 16;
                    let bz = cz * 16 + 8 + dz * 16;
                    if biome_biased_to(mc_full_biome_at(ctx, bx, bz)) {
                        cx = bx.div_euclid(16);
                        cz = bz.div_euclid(16);
                        break 'snap;
                    }
                }
            }
        }
        out.push([cx, cz]);
        angle += std::f64::consts::TAU / spread as f64;
        in_circle += 1;
        if in_circle == spread {
            circle += 1;
            in_circle = 0;
            spread += 2 * spread / (circle + 1);
            spread = spread.min((COUNT - i) as i32);
            angle += next_f64(&mut rng) * std::f64::consts::TAU;
        }
    }
    out
}

// ----------------------------------------------------------------
// Directions + oriented boxes (StructurePiece conventions).
// ----------------------------------------------------------------

/// 0 = north, 1 = south, 2 = west, 3 = east (the ScatteredPiece
/// convention - its `world()` IS `getWorldX/Y/Z`).
const N: u8 = 0;
const S: u8 = 1;
const W: u8 = 2;
const E: u8 = 3;

fn random_dir(r: &mut SplitMix64) -> u8 {
    next_int(r, 4) as u8
}

/// `BoundingBox.orientBox(foot, off, size, dir)`.
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

/// `SmallDoorType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Door {
    Opening,
    Wood,
    Grates,
    Iron,
}

fn random_door(r: &mut SplitMix64) -> Door {
    match next_int(r, 5) {
        2 => Door::Wood,
        3 => Door::Grates,
        4 => Door::Iron,
        _ => Door::Opening,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Straight { left: bool, right: bool },
    PrisonHall,
    LeftTurn,
    RightTurn,
    RoomCrossing { ty: i32 },
    StraightStairsDown,
    StairsDown { source: bool },
    FiveCrossing { ll: bool, lh: bool, rl: bool, rh: bool },
    ChestCorridor,
    Library { tall: bool },
    PortalRoom,
    Filler { steps: i32 },
}

#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub(crate) kind: Kind,
    pub(crate) bb: Bb,
    pub(crate) dir: u8,
    pub(crate) door: Door,
}

impl Piece {
    fn frame(&self) -> ScatteredPiece {
        ScatteredPiece { bb: self.bb, dir: self.dir }
    }
}

/// The weighted piece table (class, weight, maxPlaceCount).
const WEIGHT_TABLE: &[(u8, i32, i32)] = &[
    (0, 40, 0),  // Straight
    (1, 5, 5),   // PrisonHall
    (2, 20, 0),  // LeftTurn
    (3, 20, 0),  // RightTurn
    (4, 10, 6),  // RoomCrossing
    (5, 5, 5),   // StraightStairsDown
    (6, 5, 5),   // StairsDown
    (7, 5, 4),   // FiveCrossing
    (8, 5, 4),   // ChestCorridor
    (9, 10, 2),  // Library (depth > 4)
    (10, 20, 1), // PortalRoom (depth > 5)
];

struct LayoutState {
    pieces: Vec<Piece>,
    depths: Vec<i32>,
    place_counts: [i32; 11],
    imposed: Option<u8>,
    previous: Option<u8>,
    portal_room: bool,
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

/// `createPiece` per class id: oriented box + collision + validity.
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
        0 => (
            orient_box(fx, fy, fz, -1, -1, 0, 5, 5, 7, dir),
            Kind::Straight { left: next_int(rng, 2) == 0, right: next_int(rng, 2) == 0 },
        ),
        1 => (orient_box(fx, fy, fz, -1, -1, 0, 9, 5, 11, dir), Kind::PrisonHall),
        2 => (orient_box(fx, fy, fz, -1, -1, 0, 5, 5, 5, dir), Kind::LeftTurn),
        3 => (orient_box(fx, fy, fz, -1, -1, 0, 5, 5, 5, dir), Kind::RightTurn),
        4 => (
            orient_box(fx, fy, fz, -4, -1, 0, 11, 7, 11, dir),
            Kind::RoomCrossing { ty: next_int(rng, 5) },
        ),
        5 => (orient_box(fx, fy, fz, -1, -7, 0, 5, 11, 8, dir), Kind::StraightStairsDown),
        6 => (orient_box(fx, fy, fz, -1, -7, 0, 5, 11, 5, dir), Kind::StairsDown { source: false }),
        7 => (
            orient_box(fx, fy, fz, -4, -3, 0, 10, 9, 11, dir),
            Kind::FiveCrossing {
                ll: next_bool(rng),
                lh: next_bool(rng),
                rl: next_bool(rng),
                rh: next_int(rng, 3) > 0,
            },
        ),
        8 => (orient_box(fx, fy, fz, -1, -1, 0, 5, 5, 7, dir), Kind::ChestCorridor),
        9 => {
            let tall = orient_box(fx, fy, fz, -4, -1, 0, 14, 11, 15, dir);
            if is_ok_box(&tall) && !st.collides(&tall) {
                (tall, Kind::Library { tall: true })
            } else {
                let low = orient_box(fx, fy, fz, -4, -1, 0, 14, 6, 15, dir);
                (low, Kind::Library { tall: false })
            }
        }
        _ => (orient_box(fx, fy, fz, -4, -1, 0, 11, 8, 16, dir), Kind::PortalRoom),
    };
    if !is_ok_box(&bb) || st.collides(&bb) {
        return None;
    }
    // Doors roll AFTER the box succeeds (constructor order); the
    // portal room's door is fixed GRATES.
    let door = if class == 10 { Door::Grates } else { random_door(rng) };
    Some(Piece { kind, bb, dir, door })
}

/// `FillerCorridor.findPieceBox`: shrink until the corridor stops
/// short of the colliding piece.
fn filler_box(st: &LayoutState, fx: i32, fy: i32, fz: i32, dir: u8) -> Option<Bb> {
    let full = orient_box(fx, fy, fz, -1, -1, 0, 5, 5, 4, dir);
    let hit = st.pieces.iter().find(|p| p.bb.intersects(&full))?;
    if hit.bb.y0 != full.y0 {
        return None;
    }
    for depth in (1..=2).rev() {
        let bb = orient_box(fx, fy, fz, -1, -1, 0, 5, 5, depth, dir);
        if !hit.bb.intersects(&bb) {
            return Some(orient_box(fx, fy, fz, -1, -1, 0, 5, 5, depth + 1, dir));
        }
    }
    None
}

/// `generatePieceFromSmallDoor` + `generateAndAddPiece`: returns the
/// new piece index when one lands.
#[allow(clippy::too_many_arguments)]
fn generate_and_add(
    st: &mut LayoutState,
    rng: &mut SplitMix64,
    fx: i32,
    fy: i32,
    fz: i32,
    dir: u8,
    depth: i32,
) -> Option<usize> {
    if depth > MAX_DEPTH {
        return None;
    }
    if (fx - st.start_x0).abs() > ENVELOPE || (fz - st.start_z0).abs() > ENVELOPE {
        return None;
    }
    let child_depth = depth + 1;
    // updatePieceWeight: any capped class still open?
    let any_open = WEIGHT_TABLE
        .iter()
        .any(|(c, _, max)| *max > 0 && st.place_counts[*c as usize] < *max);
    if !any_open {
        return None;
    }
    let mut created: Option<Piece> = None;
    let mut created_class = 0u8;
    if let Some(imp) = st.imposed.take()
        && let Some(p) = try_create(st, rng, imp, fx, fy, fz, dir)
    {
        created = Some(p);
        created_class = imp;
    }
    if created.is_none() {
        let total: i32 = WEIGHT_TABLE.iter().map(|(_, w, _)| *w).sum();
        'attempts: for _ in 0..5 {
            let mut pick = next_int(rng, total);
            for (class, weight, max) in WEIGHT_TABLE {
                pick -= weight;
                if pick < 0 {
                    let count = st.place_counts[*class as usize];
                    let do_place = (*max == 0 || count < *max)
                        && match class {
                            9 => child_depth > 4,
                            10 => child_depth > 5,
                            _ => true,
                        };
                    if !do_place || st.previous == Some(*class) {
                        break;
                    }
                    if let Some(p) = try_create(st, rng, *class, fx, fy, fz, dir) {
                        st.place_counts[*class as usize] += 1;
                        st.previous = Some(*class);
                        created = Some(p);
                        created_class = *class;
                        break 'attempts;
                    }
                }
            }
        }
    }
    let piece = match created {
        Some(p) => p,
        None => {
            let bb = filler_box(st, fx, fy, fz, dir)?;
            if bb.y0 <= 1 {
                return None;
            }
            let steps = if dir == N || dir == S {
                bb.z1 - bb.z0 + 1
            } else {
                bb.x1 - bb.x0 + 1
            };
            created_class = 255;
            Piece { kind: Kind::Filler { steps }, bb, dir, door: Door::Opening }
        }
    };
    if piece.kind == Kind::PortalRoom {
        st.portal_room = true;
    }
    let _ = created_class;
    let idx = st.pieces.len();
    st.pieces.push(piece);
    st.depths.push(child_depth);
    Some(idx)
}

/// The three door-child spawners (foot coordinates per orientation,
/// verbatim from the Java switch tables).
fn child_forward(st: &mut LayoutState, rng: &mut SplitMix64, idx: usize, x_off: i32, y_off: i32) -> Option<usize> {
    let p = st.pieces[idx].clone();
    let d = st.depths[idx];
    match p.dir {
        N => generate_and_add(st, rng, p.bb.x0 + x_off, p.bb.y0 + y_off, p.bb.z0 - 1, N, d),
        S => generate_and_add(st, rng, p.bb.x0 + x_off, p.bb.y0 + y_off, p.bb.z1 + 1, S, d),
        W => generate_and_add(st, rng, p.bb.x0 - 1, p.bb.y0 + y_off, p.bb.z0 + x_off, W, d),
        _ => generate_and_add(st, rng, p.bb.x1 + 1, p.bb.y0 + y_off, p.bb.z0 + x_off, E, d),
    }
}

fn child_left(st: &mut LayoutState, rng: &mut SplitMix64, idx: usize, y_off: i32, z_off: i32) -> Option<usize> {
    let p = st.pieces[idx].clone();
    let d = st.depths[idx];
    match p.dir {
        N | S => generate_and_add(st, rng, p.bb.x0 - 1, p.bb.y0 + y_off, p.bb.z0 + z_off, W, d),
        _ => generate_and_add(st, rng, p.bb.x0 + z_off, p.bb.y0 + y_off, p.bb.z0 - 1, N, d),
    }
}

fn child_right(st: &mut LayoutState, rng: &mut SplitMix64, idx: usize, y_off: i32, z_off: i32) -> Option<usize> {
    let p = st.pieces[idx].clone();
    let d = st.depths[idx];
    match p.dir {
        N | S => generate_and_add(st, rng, p.bb.x1 + 1, p.bb.y0 + y_off, p.bb.z0 + z_off, E, d),
        _ => generate_and_add(st, rng, p.bb.x0 + z_off, p.bb.y0 + y_off, p.bb.z1 + 1, S, d),
    }
}

/// `addChildren` per kind - pushes new pending indices.
fn add_children(st: &mut LayoutState, rng: &mut SplitMix64, idx: usize, pending: &mut Vec<usize>) {
    let kind = st.pieces[idx].kind;
    let push = |o: Option<usize>, pending: &mut Vec<usize>| {
        if let Some(i) = o {
            pending.push(i);
        }
    };
    match kind {
        Kind::Straight { left, right } => {
            push(child_forward(st, rng, idx, 1, 1), pending);
            if left {
                push(child_left(st, rng, idx, 1, 2), pending);
            }
            if right {
                push(child_right(st, rng, idx, 1, 2), pending);
            }
        }
        Kind::PrisonHall | Kind::ChestCorridor | Kind::StraightStairsDown => {
            push(child_forward(st, rng, idx, 1, 1), pending);
        }
        Kind::LeftTurn => {
            let dir = st.pieces[idx].dir;
            if dir != N && dir != E {
                push(child_right(st, rng, idx, 1, 1), pending);
            } else {
                push(child_left(st, rng, idx, 1, 1), pending);
            }
        }
        Kind::RightTurn => {
            let dir = st.pieces[idx].dir;
            if dir != N && dir != E {
                push(child_left(st, rng, idx, 1, 1), pending);
            } else {
                push(child_right(st, rng, idx, 1, 1), pending);
            }
        }
        Kind::RoomCrossing { .. } => {
            push(child_forward(st, rng, idx, 4, 1), pending);
            push(child_left(st, rng, idx, 1, 4), pending);
            push(child_right(st, rng, idx, 1, 4), pending);
        }
        Kind::StairsDown { source } => {
            if source {
                st.imposed = Some(7); // FiveCrossing
            }
            push(child_forward(st, rng, idx, 1, 1), pending);
        }
        Kind::FiveCrossing { ll, lh, rl, rh } => {
            let dir = st.pieces[idx].dir;
            let (mut a, mut b) = (3, 5);
            if dir == W || dir == N {
                a = 8 - a;
                b = 8 - b;
            }
            push(child_forward(st, rng, idx, 5, 1), pending);
            if ll {
                push(child_left(st, rng, idx, a, 1), pending);
            }
            if lh {
                push(child_left(st, rng, idx, b, 7), pending);
            }
            if rl {
                push(child_right(st, rng, idx, a, 1), pending);
            }
            if rh {
                push(child_right(st, rng, idx, b, 7), pending);
            }
        }
        Kind::Library { .. } | Kind::PortalRoom | Kind::Filler { .. } => {}
    }
}

/// Solve (and cache) the stronghold layout of a start chunk.
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

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Vec<Piece>> {
    // Only ring positions host strongholds.
    if !stronghold_positions(ctx).contains(&[scx, scz]) {
        return None;
    }
    let seed = ctx.effective_seed();
    // The generatePieces retry loop: bump the seed until a portal
    // room lands (capped - the Java loop practically always
    // terminates within a few tries).
    for tries in 0..16u64 {
        let mut rng =
            SplitMix64::new(stream_seed(seed, scx, scz, SALT_LAYOUT).wrapping_add(tries));
        let dir = random_dir(&mut rng);
        let west = scx * 16 + 2;
        let north = scz * 16 + 2;
        // StairsDown start: makeBoundingBox(west, 64, north, dir, 5, 11, 5)
        // = footprint transpose exactly like ScatteredPiece::new.
        let start_frame = ScatteredPiece::new(west, MAGIC_START_Y, north, dir, 5, 11, 5);
        let mut st = LayoutState {
            pieces: vec![Piece {
                kind: Kind::StairsDown { source: true },
                bb: start_frame.bb,
                dir,
                door: Door::Opening,
            }],
            depths: vec![0],
            place_counts: [0; 11],
            imposed: None,
            previous: None,
            portal_room: false,
            start_x0: start_frame.bb.x0,
            start_z0: start_frame.bb.z0,
        };
        let mut pending: Vec<usize> = Vec::new();
        add_children(&mut st, &mut rng, 0, &mut pending);
        while !pending.is_empty() {
            let pick = next_int(&mut rng, pending.len() as i32) as usize;
            let idx = pending.swap_remove(pick);
            add_children(&mut st, &mut rng, idx, &mut pending);
        }
        if !st.portal_room {
            continue;
        }
        // moveBelowSeaLevel(63, minY, rand, 10): the whole maze tops
        // out below y 52, buried at a random depth.
        let mut y1 = i32::MIN;
        let mut span_min = i32::MAX;
        for p in &st.pieces {
            y1 = y1.max(p.bb.y1);
            span_min = span_min.min(p.bb.y0);
        }
        let ceiling = SEA_LEVEL - LOWEST_Y;
        let mut target_top = (y1 - span_min) + 1 + 1;
        if target_top < ceiling {
            target_top += next_int(&mut rng, ceiling - target_top);
        }
        let dy = target_top - y1;
        for p in &mut st.pieces {
            p.bb = p.bb.moved(0, dy, 0);
        }
        return Some(st.pieces);
    }
    None
}

// ----------------------------------------------------------------
// Carving.
// ----------------------------------------------------------------

struct Palette {
    stone: u16,
    mossy: u16,
    cracked: u16,
    slab_stone_brick: u16,
    slab_smooth: u16,
    stairs_stone_brick: u16,
    stairs_cobble: u16,
    cobble: u16,
    planks: u16,
    bookshelf: u16,
    fence: u16,
    ladder: u16,
    torch: u16,
    bars: u16,
    oak_door: u16,
    iron_door: u16,
    button: u16,
    cobweb: u16,
    chest: u16,
    spawner: u16,
    frame: u16,
    water: u16,
    lava: u16,
}

impl Palette {
    fn get() -> Palette {
        let stone = registry_id("stone_bricks").unwrap_or(1);
        Palette {
            stone,
            mossy: registry_id("mossy_stone_bricks").unwrap_or(stone),
            cracked: registry_id("cracked_stone_bricks").unwrap_or(stone),
            slab_stone_brick: registry_id("stone_brick_slab").unwrap_or(stone),
            slab_smooth: registry_id("smooth_stone_slab").unwrap_or(stone),
            stairs_stone_brick: registry_id("stone_brick_stairs").unwrap_or(stone),
            stairs_cobble: registry_id("cobblestone_stairs").unwrap_or(stone),
            cobble: crate::block::COBBLE_ID,
            planks: crate::block::PLANKS_ID,
            bookshelf: registry_id("bookshelf").unwrap_or(0),
            fence: crate::block::OAK_FENCE_ID,
            ladder: registry_id("ladder").unwrap_or(0),
            torch: crate::block::TORCH_ID,
            bars: registry_id("iron_bars").unwrap_or(0),
            oak_door: registry_id("oak_door").unwrap_or(0),
            iron_door: registry_id("iron_door").unwrap_or(0),
            button: registry_id("stone_button").unwrap_or(0),
            cobweb: registry_id("cobweb").unwrap_or(0),
            chest: crate::block::CHEST_ID,
            spawner: crate::block::SPAWNER_ID,
            frame: registry_id("end_portal_frame").unwrap_or(stone),
            water: crate::block::WATER_ID,
            lava: crate::block::LAVA_ID,
        }
    }
}

/// `SmoothStoneSelector.next` for an EDGE cell, pure per world cell.
fn stone_pick(seed: u32, pos: [i32; 3], pal: &Palette) -> u16 {
    let s = stream_seed(seed, pos[0], pos[2], SALT_STONE)
        ^ (pos[1] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let roll = (SplitMix64::new(s).next_u64() >> 40) as f32 / (1u64 << 24) as f32;
    if roll < 0.2 {
        pal.cracked
    } else if roll < 0.5 {
        pal.mossy
    } else {
        // 0.5..0.55 is INFESTED_STONE_BRICKS - the silverfish block
        // maps to the plain variant until the mob exists.
        pal.stone
    }
}

/// `maybeGenerateBlock` roll, pure per world cell.
fn maybe_keeps(seed: u32, pos: [i32; 3], p: f32) -> bool {
    let s = stream_seed(seed, pos[0], pos[2], SALT_MAYBE)
        ^ (pos[1] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let roll = (SplitMix64::new(s).next_u64() >> 40) as f32 / (1u64 << 24) as f32;
    roll < p
}

struct Carver<'a, 'w> {
    w: &'a mut dyn ShaftWorld,
    f: ScatteredPiece,
    seed: u32,
    pal: &'w Palette,
}

impl Carver<'_, '_> {
    fn put(&mut self, x: i32, y: i32, z: i32, id: u16) {
        let pos = self.f.world(x, y, z);
        let (cx0, cz0, cx1, cz1) = self.w.column();
        if pos[0] >= cx0
            && pos[0] <= cx1
            && pos[2] >= cz0
            && pos[2] <= cz1
            && (-64..320).contains(&pos[1])
        {
            self.w.place(pos, id);
        }
    }

    /// `generateBox` with a constant fill.
    fn boxed(&mut self, a: (i32, i32, i32), b: (i32, i32, i32), id: u16) {
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                for z in a.2..=b.2 {
                    self.put(x, y, z, id);
                }
            }
        }
    }

    /// `generateBox(..., skip_air, random, SMOOTH_STONE_SELECTOR)`.
    /// Edge cells roll the brick mix, interior cells carve air;
    /// `skip_air` leaves already-open world cells alone (that is how
    /// strongholds keep their cave holes).
    fn stone_box(&mut self, a: (i32, i32, i32), b: (i32, i32, i32), skip_air: bool) {
        let (cx0, cz0, cx1, cz1) = self.w.column();
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                for z in a.2..=b.2 {
                    let pos = self.f.world(x, y, z);
                    if pos[0] < cx0
                        || pos[0] > cx1
                        || pos[2] < cz0
                        || pos[2] > cz1
                        || !(-64..320).contains(&pos[1])
                    {
                        continue;
                    }
                    if skip_air && self.w.read(pos) == 0 {
                        continue;
                    }
                    let edge = x == a.0 || x == b.0 || y == a.1 || y == b.1 || z == a.2 || z == b.2;
                    let id = if edge { stone_pick(self.seed, pos, self.pal) } else { 0 };
                    self.w.place(pos, id);
                }
            }
        }
    }

    fn maybe(&mut self, p: f32, x: i32, y: i32, z: i32, id: u16) {
        let pos = self.f.world(x, y, z);
        if maybe_keeps(self.seed, pos, p) {
            self.put(x, y, z, id);
        }
    }

    /// `generateSmallDoor`.
    fn door(&mut self, door: Door, x: i32, y: i32, z: i32) {
        let pal = self.pal;
        match door {
            Door::Opening => self.boxed((x, y, z), (x + 2, y + 2, z), 0),
            Door::Wood | Door::Iron => {
                let d = if door == Door::Wood { pal.oak_door } else { pal.iron_door };
                let (stone, button) = (pal.stone, pal.button);
                self.put(x, y, z, stone);
                self.put(x, y + 1, z, stone);
                self.put(x, y + 2, z, stone);
                self.put(x + 1, y + 2, z, stone);
                self.put(x + 2, y + 2, z, stone);
                self.put(x + 2, y + 1, z, stone);
                self.put(x + 2, y, z, stone);
                // The engine door anchors at the lower cell; the
                // upper half opens (matching the blockmap rule).
                self.put(x + 1, y, z, d);
                self.put(x + 1, y + 1, z, 0);
                if door == Door::Iron {
                    self.put(x + 2, y + 1, z + 1, button);
                    self.put(x + 2, y + 1, z - 1, button);
                }
            }
            Door::Grates => {
                let bars = pal.bars;
                self.put(x + 1, y, z, 0);
                self.put(x + 1, y + 1, z, 0);
                self.put(x, y, z, bars);
                self.put(x, y + 1, z, bars);
                self.put(x, y + 2, z, bars);
                self.put(x + 1, y + 2, z, bars);
                self.put(x + 2, y + 2, z, bars);
                self.put(x + 2, y + 1, z, bars);
                self.put(x + 2, y, z, bars);
            }
        }
    }
}

/// Carve one piece into the cube (Java `postProcess`, in order).
fn carve_piece(c: &mut Carver, piece: &Piece) {
    let pal_stone = c.pal.stone;
    match piece.kind {
        Kind::Straight { left, right } => {
            c.stone_box((0, 0, 0), (4, 4, 6), true);
            c.door(piece.door, 1, 1, 0);
            c.door(Door::Opening, 1, 1, 6);
            let torch = c.pal.torch;
            c.maybe(0.1, 1, 2, 1, torch);
            c.maybe(0.1, 3, 2, 1, torch);
            c.maybe(0.1, 1, 2, 5, torch);
            c.maybe(0.1, 3, 2, 5, torch);
            if left {
                c.boxed((0, 1, 2), (0, 3, 4), 0);
            }
            if right {
                c.boxed((4, 1, 2), (4, 3, 4), 0);
            }
        }
        Kind::PrisonHall => {
            c.stone_box((0, 0, 0), (8, 4, 10), true);
            c.door(piece.door, 1, 1, 0);
            c.boxed((1, 1, 10), (3, 3, 10), 0);
            c.stone_box((4, 1, 1), (4, 3, 1), false);
            c.stone_box((4, 1, 3), (4, 3, 3), false);
            c.stone_box((4, 1, 7), (4, 3, 7), false);
            c.stone_box((4, 1, 9), (4, 3, 9), false);
            let bars = c.pal.bars;
            for y in 1..=3 {
                c.put(4, y, 4, bars);
                c.put(4, y, 5, bars);
                c.put(4, y, 6, bars);
                c.put(5, y, 5, bars);
                c.put(6, y, 5, bars);
                c.put(7, y, 5, bars);
            }
            c.put(4, 3, 2, bars);
            c.put(4, 3, 8, bars);
            let door = c.pal.iron_door;
            c.put(4, 1, 2, door);
            c.put(4, 2, 2, 0);
            c.put(4, 1, 8, door);
            c.put(4, 2, 8, 0);
        }
        Kind::LeftTurn | Kind::RightTurn => {
            c.stone_box((0, 0, 0), (4, 4, 4), true);
            c.door(piece.door, 1, 1, 0);
            let dir = piece.dir;
            let west_side = match piece.kind {
                Kind::LeftTurn => dir != N && dir != E,
                _ => !(dir != N && dir != E),
            };
            if west_side {
                c.boxed((4, 1, 1), (4, 3, 3), 0);
            } else {
                c.boxed((0, 1, 1), (0, 3, 3), 0);
            }
        }
        Kind::RoomCrossing { ty } => {
            c.stone_box((0, 0, 0), (10, 6, 10), true);
            c.door(piece.door, 4, 1, 0);
            c.boxed((4, 1, 10), (6, 3, 10), 0);
            c.boxed((0, 1, 4), (0, 3, 6), 0);
            c.boxed((10, 1, 4), (10, 3, 6), 0);
            let torch = c.pal.torch;
            match ty {
                0 => {
                    c.put(5, 1, 5, pal_stone);
                    c.put(5, 2, 5, pal_stone);
                    c.put(5, 3, 5, pal_stone);
                    c.put(4, 3, 5, torch);
                    c.put(6, 3, 5, torch);
                    c.put(5, 3, 4, torch);
                    c.put(5, 3, 6, torch);
                    let slab = c.pal.slab_smooth;
                    c.put(4, 1, 4, slab);
                    c.put(4, 1, 5, slab);
                    c.put(4, 1, 6, slab);
                    c.put(6, 1, 4, slab);
                    c.put(6, 1, 5, slab);
                    c.put(6, 1, 6, slab);
                    c.put(5, 1, 4, slab);
                    c.put(5, 1, 6, slab);
                }
                1 => {
                    for i in 0..5 {
                        c.put(3, 1, 3 + i, pal_stone);
                        c.put(7, 1, 3 + i, pal_stone);
                        c.put(3 + i, 1, 3, pal_stone);
                        c.put(3 + i, 1, 7, pal_stone);
                    }
                    c.put(5, 1, 5, pal_stone);
                    c.put(5, 2, 5, pal_stone);
                    c.put(5, 3, 5, pal_stone);
                    c.put(5, 4, 5, c.pal.water);
                }
                2 => {
                    let cobble = c.pal.cobble;
                    for z in 1..=9 {
                        c.put(1, 3, z, cobble);
                        c.put(9, 3, z, cobble);
                    }
                    for x in 1..=9 {
                        c.put(x, 3, 1, cobble);
                        c.put(x, 3, 9, cobble);
                    }
                    c.put(5, 1, 4, cobble);
                    c.put(5, 1, 6, cobble);
                    c.put(5, 3, 4, cobble);
                    c.put(5, 3, 6, cobble);
                    c.put(4, 1, 5, cobble);
                    c.put(6, 1, 5, cobble);
                    c.put(4, 3, 5, cobble);
                    c.put(6, 3, 5, cobble);
                    for y in 1..=3 {
                        c.put(4, y, 4, cobble);
                        c.put(6, y, 4, cobble);
                        c.put(4, y, 6, cobble);
                        c.put(6, y, 6, cobble);
                    }
                    c.put(5, 3, 5, torch);
                    let planks = c.pal.planks;
                    for z in 2..=8 {
                        c.put(2, 3, z, planks);
                        c.put(3, 3, z, planks);
                        if z <= 3 || z >= 7 {
                            c.put(4, 3, z, planks);
                            c.put(5, 3, z, planks);
                            c.put(6, 3, z, planks);
                        }
                        c.put(7, 3, z, planks);
                        c.put(8, 3, z, planks);
                    }
                    let ladder = c.pal.ladder;
                    c.put(9, 1, 3, ladder);
                    c.put(9, 2, 3, ladder);
                    c.put(9, 3, 3, ladder);
                    c.put(3, 4, 8, c.pal.chest);
                }
                _ => {}
            }
        }
        Kind::StraightStairsDown => {
            c.stone_box((0, 0, 0), (4, 10, 7), true);
            c.door(piece.door, 1, 7, 0);
            c.door(Door::Opening, 1, 1, 7);
            let stairs = c.pal.stairs_cobble;
            for i in 0..6 {
                c.put(1, 6 - i, 1 + i, stairs);
                c.put(2, 6 - i, 1 + i, stairs);
                c.put(3, 6 - i, 1 + i, stairs);
                if i < 5 {
                    c.put(1, 5 - i, 1 + i, pal_stone);
                    c.put(2, 5 - i, 1 + i, pal_stone);
                    c.put(3, 5 - i, 1 + i, pal_stone);
                }
            }
        }
        Kind::StairsDown { .. } => {
            c.stone_box((0, 0, 0), (4, 10, 4), true);
            c.door(piece.door, 1, 7, 0);
            c.door(Door::Opening, 1, 1, 4);
            let slab = c.pal.slab_smooth;
            c.put(2, 6, 1, pal_stone);
            c.put(1, 5, 1, pal_stone);
            c.put(1, 6, 1, slab);
            c.put(1, 5, 2, pal_stone);
            c.put(1, 4, 3, pal_stone);
            c.put(1, 5, 3, slab);
            c.put(2, 4, 3, pal_stone);
            c.put(3, 3, 3, pal_stone);
            c.put(3, 4, 3, slab);
            c.put(3, 3, 2, pal_stone);
            c.put(3, 2, 1, pal_stone);
            c.put(3, 3, 1, slab);
            c.put(2, 2, 1, pal_stone);
            c.put(1, 1, 1, pal_stone);
            c.put(1, 2, 1, slab);
            c.put(1, 1, 2, pal_stone);
            c.put(1, 1, 3, slab);
        }
        Kind::FiveCrossing { ll, lh, rl, rh } => {
            c.stone_box((0, 0, 0), (9, 8, 10), true);
            c.door(piece.door, 4, 3, 0);
            if ll {
                c.boxed((0, 3, 1), (0, 5, 3), 0);
            }
            if rl {
                c.boxed((9, 3, 1), (9, 5, 3), 0);
            }
            if lh {
                c.boxed((0, 5, 7), (0, 7, 9), 0);
            }
            if rh {
                c.boxed((9, 5, 7), (9, 7, 9), 0);
            }
            c.boxed((5, 1, 10), (7, 3, 10), 0);
            c.stone_box((1, 2, 1), (8, 2, 6), false);
            c.stone_box((4, 1, 5), (4, 4, 9), false);
            c.stone_box((8, 1, 5), (8, 4, 9), false);
            c.stone_box((1, 4, 7), (3, 4, 9), false);
            c.stone_box((1, 3, 5), (3, 3, 6), false);
            let slab = c.pal.slab_smooth;
            c.boxed((1, 3, 4), (3, 3, 4), slab);
            c.boxed((1, 4, 6), (3, 4, 6), slab);
            c.stone_box((5, 1, 7), (7, 1, 8), false);
            c.boxed((5, 1, 9), (7, 1, 9), slab);
            c.boxed((5, 2, 7), (7, 2, 7), slab);
            c.boxed((4, 5, 7), (4, 5, 9), slab);
            c.boxed((8, 5, 7), (8, 5, 9), slab);
            // The double slab centre reads as the full smooth block -
            // the plain slab stands in (no double-slab state).
            c.boxed((5, 5, 7), (7, 5, 9), slab);
            c.put(6, 5, 6, c.pal.torch);
        }
        Kind::ChestCorridor => {
            c.stone_box((0, 0, 0), (4, 4, 6), true);
            c.door(piece.door, 1, 1, 0);
            c.door(Door::Opening, 1, 1, 6);
            c.boxed((3, 1, 2), (3, 1, 4), pal_stone);
            let slab = c.pal.slab_stone_brick;
            c.put(3, 1, 1, slab);
            c.put(3, 1, 5, slab);
            c.put(3, 2, 2, slab);
            c.put(3, 2, 4, slab);
            for z in 2..=4 {
                c.put(2, 1, z, slab);
            }
            c.put(3, 2, 3, c.pal.chest);
        }
        Kind::Library { tall } => {
            let h = if tall { 11 } else { 6 };
            c.stone_box((0, 0, 0), (13, h - 1, 14), true);
            c.door(piece.door, 4, 1, 0);
            // Cobwebs at 7%.
            let web = c.pal.cobweb;
            for y in 1..=4 {
                for x in 2..=11 {
                    for z in 1..=13 {
                        let pos = c.f.world(x, y, z);
                        if maybe_keeps(c.seed, pos, 0.07) {
                            c.put(x, y, z, web);
                        }
                    }
                }
            }
            let planks = c.pal.planks;
            let shelf = c.pal.bookshelf;
            let torch = c.pal.torch;
            for d in 1..=13 {
                let post = (d - 1) % 4 == 0;
                let fill = if post { planks } else { shelf };
                c.boxed((1, 1, d), (1, 4, d), fill);
                c.boxed((12, 1, d), (12, 4, d), fill);
                if post {
                    c.put(2, 3, d, torch);
                    c.put(11, 3, d, torch);
                }
                if tall {
                    c.boxed((1, 6, d), (1, 9, d), fill);
                    c.boxed((12, 6, d), (12, 9, d), fill);
                }
            }
            let mut d = 3;
            while d < 12 {
                c.boxed((3, 1, d), (4, 3, d), shelf);
                c.boxed((6, 1, d), (7, 3, d), shelf);
                c.boxed((9, 1, d), (10, 3, d), shelf);
                d += 2;
            }
            if tall {
                c.boxed((1, 5, 1), (3, 5, 13), planks);
                c.boxed((10, 5, 1), (12, 5, 13), planks);
                c.boxed((4, 5, 1), (9, 5, 2), planks);
                c.boxed((4, 5, 12), (9, 5, 13), planks);
                c.put(9, 5, 11, planks);
                c.put(8, 5, 11, planks);
                c.put(9, 5, 10, planks);
                let fence = c.pal.fence;
                c.boxed((3, 6, 3), (3, 6, 11), fence);
                c.boxed((10, 6, 3), (10, 6, 9), fence);
                c.boxed((4, 6, 2), (9, 6, 2), fence);
                c.boxed((4, 6, 12), (7, 6, 12), fence);
                c.put(3, 6, 2, fence);
                c.put(3, 6, 12, fence);
                c.put(10, 6, 2, fence);
                for i in 0..=2 {
                    c.put(8 + i, 6, 12 - i, fence);
                    if i != 2 {
                        c.put(8 + i, 6, 11 - i, fence);
                    }
                }
                let ladder = c.pal.ladder;
                for y in 1..=7 {
                    c.put(10, y, 13, ladder);
                }
                // The chandelier.
                c.put(6, 9, 7, fence);
                c.put(7, 9, 7, fence);
                c.put(6, 8, 7, fence);
                c.put(7, 8, 7, fence);
                c.put(6, 7, 7, fence);
                c.put(7, 7, 7, fence);
                c.put(5, 7, 7, fence);
                c.put(8, 7, 7, fence);
                c.put(6, 7, 6, fence);
                c.put(6, 7, 8, fence);
                c.put(7, 7, 6, fence);
                c.put(7, 7, 8, fence);
                c.put(5, 8, 7, torch);
                c.put(8, 8, 7, torch);
                c.put(6, 8, 6, torch);
                c.put(6, 8, 8, torch);
                c.put(7, 8, 6, torch);
                c.put(7, 8, 8, torch);
            }
            c.put(3, 3, 5, c.pal.chest);
            if tall {
                c.put(12, 9, 1, 0);
                c.put(12, 8, 1, c.pal.chest);
            }
        }
        Kind::PortalRoom => {
            c.stone_box((0, 0, 0), (10, 7, 15), false);
            c.door(Door::Grates, 4, 1, 0);
            c.stone_box((1, 6, 1), (1, 6, 14), false);
            c.stone_box((9, 6, 1), (9, 6, 14), false);
            c.stone_box((2, 6, 1), (8, 6, 2), false);
            c.stone_box((2, 6, 14), (8, 6, 14), false);
            c.stone_box((1, 1, 1), (2, 1, 4), false);
            c.stone_box((8, 1, 1), (9, 1, 4), false);
            let lava = c.pal.lava;
            c.boxed((1, 1, 1), (1, 1, 3), lava);
            c.boxed((9, 1, 1), (9, 1, 3), lava);
            c.stone_box((3, 1, 8), (7, 1, 12), false);
            c.boxed((4, 1, 9), (6, 1, 11), lava);
            let bars = c.pal.bars;
            let mut z = 3;
            while z < 14 {
                c.boxed((0, 3, z), (0, 4, z), bars);
                c.boxed((10, 3, z), (10, 4, z), bars);
                z += 2;
            }
            let mut x = 2;
            while x < 9 {
                c.boxed((x, 3, 15), (x, 4, 15), bars);
                x += 2;
            }
            let stairs = c.pal.stairs_stone_brick;
            c.stone_box((4, 1, 5), (6, 1, 7), false);
            c.stone_box((4, 2, 6), (6, 2, 7), false);
            c.stone_box((4, 3, 7), (6, 3, 7), false);
            for x in 4..=6 {
                c.put(x, 1, 4, stairs);
                c.put(x, 2, 5, stairs);
                c.put(x, 3, 6, stairs);
            }
            // The 12 END PORTAL FRAMES (eye states dropped; the
            // all-eyes pre-lit portal has p = 1e-12 and waits for the
            // End itself).
            let frame = c.pal.frame;
            c.put(4, 3, 8, frame);
            c.put(5, 3, 8, frame);
            c.put(6, 3, 8, frame);
            c.put(4, 3, 12, frame);
            c.put(5, 3, 12, frame);
            c.put(6, 3, 12, frame);
            c.put(3, 3, 9, frame);
            c.put(3, 3, 10, frame);
            c.put(3, 3, 11, frame);
            c.put(7, 3, 9, frame);
            c.put(7, 3, 10, frame);
            c.put(7, 3, 11, frame);
            // The silverfish spawner (worker kind probe hands out the
            // stand-in).
            c.put(5, 3, 6, c.pal.spawner);
        }
        Kind::Filler { steps } => {
            for i in 0..steps {
                for x in 0..=4 {
                    c.put(x, 0, i, pal_stone);
                    c.put(x, 4, i, pal_stone);
                }
                for y in 1..=3 {
                    c.put(0, y, i, pal_stone);
                    c.put(1, y, i, 0);
                    c.put(2, y, i, 0);
                    c.put(3, y, i, 0);
                    c.put(4, y, i, pal_stone);
                }
            }
        }
    }
}

/// Pieces stay within +-112 blocks of the start + one piece extent.
const REACH: i32 = 9;

/// The per-chunk stronghold pass.
pub(crate) fn place_strongholds(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let positions = stronghold_positions(ctx);
    let near: Vec<[i32; 2]> = positions
        .iter()
        .filter(|p| (p[0] - coord[0]).abs() <= REACH && (p[1] - coord[2]).abs() <= REACH)
        .copied()
        .collect();
    if near.is_empty() {
        return;
    }
    let cs = CHUNK_SIZE as i32;
    let cube = Bb {
        x0: origin[0],
        y0: origin[1],
        z0: origin[2],
        x1: origin[0] + cs - 1,
        y1: origin[1] + cs - 1,
        z1: origin[2] + cs - 1,
    };
    let pal = Palette::get();
    let seed = ctx.effective_seed();
    for start in near {
        let Some(layout) = layout_for_start(ctx, start[0], start[1]) else {
            continue;
        };
        for piece in layout.iter() {
            if !piece.bb.intersects(&cube) {
                continue;
            }
            let mut world = ChunkShaftWorld { chunk, origin, ctx };
            let mut carver = Carver { w: &mut world, f: piece.frame(), seed, pal: &pal };
            carve_piece(&mut carver, piece);
        }
    }
}

// ----------------------------------------------------------------
// Membership (loot + spawner stand-in).
// ----------------------------------------------------------------

/// Which stronghold chest table a chest at `pos` rolls (worker-side).
pub(crate) fn chest_kind_at(ctx: &WorldgenCtx, pos: [i32; 3]) -> Option<StrongholdChest> {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return None;
    }
    for_piece_at(ctx, pos, |piece, frame| {
        let hit = |l: (i32, i32, i32)| frame.world(l.0, l.1, l.2) == pos;
        match piece.kind {
            Kind::ChestCorridor if hit((3, 2, 3)) => Some(StrongholdChest::Corridor),
            Kind::RoomCrossing { ty: 2 } if hit((3, 4, 8)) => Some(StrongholdChest::Crossing),
            Kind::Library { tall } => {
                if hit((3, 3, 5)) || (tall && hit((12, 8, 1))) {
                    Some(StrongholdChest::Library)
                } else {
                    None
                }
            }
            _ => None,
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrongholdChest {
    Corridor,
    Crossing,
    Library,
}

/// Is `pos` the portal room's silverfish spawner cell? (Worker-side -
/// picks the stand-in kind.)
pub(crate) fn is_portal_spawner(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    for_piece_at(ctx, pos, |piece, frame| {
        (piece.kind == Kind::PortalRoom && frame.world(5, 3, 6) == pos).then_some(())
    })
    .is_some()
}

fn for_piece_at<T>(
    ctx: &WorldgenCtx,
    pos: [i32; 3],
    mut f: impl FnMut(&Piece, &ScatteredPiece) -> Option<T>,
) -> Option<T> {
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    let positions = stronghold_positions(ctx);
    for start in positions.iter() {
        if (start[0] - ccx).abs() > REACH || (start[1] - ccz).abs() > REACH {
            continue;
        }
        let Some(layout) = layout_for_start(ctx, start[0], start[1]) else {
            continue;
        };
        for piece in layout.iter() {
            if !piece.bb.contains(pos) {
                continue;
            }
            let frame = piece.frame();
            if let Some(t) = f(piece, &frame) {
                return Some(t);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_positions_are_deterministic_and_ringed() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let a = stronghold_positions(&ctx);
        let b = stronghold_positions(&ctx);
        assert_eq!(a.len(), 128);
        assert_eq!(a[0], b[0], "cached + deterministic");
        // Ring 1 (first 3): 4*32 +- 40 chunks from origin.
        for p in a.iter().take(3) {
            let d = ((p[0] * p[0] + p[1] * p[1]) as f64).sqrt();
            assert!(
                (60.0..=220.0).contains(&d),
                "ring-1 stronghold drifted: {p:?} (d = {d:.0})"
            );
        }
    }

    #[test]
    fn orient_box_matches_the_java_corners() {
        // SOUTH: straight extents.
        let s = orient_box(10, 64, 20, -1, -1, 0, 5, 5, 7, S);
        assert_eq!((s.x0, s.y0, s.z0, s.x1, s.y1, s.z1), (9, 63, 20, 13, 67, 26));
        // NORTH: depth extends toward -z.
        let n = orient_box(10, 64, 20, -1, -1, 0, 5, 5, 7, N);
        assert_eq!((n.z0, n.z1), (14, 20));
        assert_eq!((n.x0, n.x1), (9, 13));
        // WEST: width along +z, depth toward -x.
        let w = orient_box(10, 64, 20, -1, -1, 0, 5, 5, 7, W);
        assert_eq!((w.x0, w.x1), (4, 10));
        assert_eq!((w.z0, w.z1), (19, 23));
        // EAST: depth toward +x.
        let e = orient_box(10, 64, 20, -1, -1, 0, 5, 5, 7, E);
        assert_eq!((e.x0, e.x1), (10, 16));
        assert_eq!((e.z0, e.z1), (19, 23));
    }

    #[test]
    fn layouts_hold_a_portal_room_below_sea_level() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let positions = stronghold_positions(&ctx);
        let p = positions[0];
        let layout = layout_for_start(&ctx, p[0], p[1]).expect("ring position solves");
        assert!(layout.len() > 10, "stronghold came out tiny: {}", layout.len());
        let portal = layout
            .iter()
            .find(|p| p.kind == Kind::PortalRoom)
            .expect("the retry loop guarantees a portal room");
        let top = layout.iter().map(|p| p.bb.y1).max().unwrap();
        assert!(top <= 52, "moveBelowSeaLevel must bury the maze: top {top}");
        assert!(portal.bb.y0 >= 1);
        // Determinism.
        let again = layout_for_start(&ctx, p[0], p[1]).expect("cached");
        assert_eq!(again.len(), layout.len());
        // Non-ring chunks never host one.
        assert!(layout_for_start(&ctx, p[0] + 1, p[1]).is_none());
    }

    /// Probe: the nearest stronghold to the test region +
    /// full-pipeline verify (portal frames + spawner + a chest with
    /// its table membership).
    /// `INFENGINE_DUMP_SEED=<seed> cargo test -p voxel_demo_mp
    /// --release --lib dump_strongholds -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_strongholds() {
        let seed: u32 = std::env::var("INFENGINE_DUMP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3592732596);
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let positions = stronghold_positions(&ctx);
        println!("first ring: {:?}", &positions[..3.min(positions.len())]);
        let p = positions[0];
        let layout = layout_for_start(&ctx, p[0], p[1]).expect("layout");
        let mut kinds: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for piece in layout.iter() {
            let name = match piece.kind {
                Kind::Straight { .. } => "straight",
                Kind::PrisonHall => "prison",
                Kind::LeftTurn => "left_turn",
                Kind::RightTurn => "right_turn",
                Kind::RoomCrossing { .. } => "room_crossing",
                Kind::StraightStairsDown => "straight_stairs",
                Kind::StairsDown { .. } => "stairs",
                Kind::FiveCrossing { .. } => "five_crossing",
                Kind::ChestCorridor => "chest_corridor",
                Kind::Library { .. } => "library",
                Kind::PortalRoom => "portal_room",
                Kind::Filler { .. } => "filler",
            };
            *kinds.entry(name).or_insert(0) += 1;
        }
        println!(
            "stronghold at chunk ({}, {}): {} pieces {kinds:?}",
            p[0],
            p[1],
            layout.len()
        );
        let portal = layout.iter().find(|q| q.kind == Kind::PortalRoom).unwrap();
        let frame_pos = portal.frame().world(5, 3, 8);
        let spawner_pos = portal.frame().world(5, 3, 6);
        println!("portal room bb {:?}, frame cell {frame_pos:?}", portal.bb);
        let coord = [
            frame_pos[0].div_euclid(cs),
            frame_pos[1].div_euclid(cs),
            frame_pos[2].div_euclid(cs),
        ];
        let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
        let frame_id = registry_id("end_portal_frame").unwrap();
        let mut frames = 0usize;
        for lx in 0..CHUNK_SIZE {
            for ly in 0..CHUNK_SIZE {
                for lz in 0..CHUNK_SIZE {
                    if chunk.get(lx, ly, lz).0 == frame_id {
                        frames += 1;
                    }
                }
            }
        }
        println!("full-pipeline portal cube {coord:?}: {frames} end portal frames");
        assert!(frames > 0, "portal frames must materialise");
        assert!(is_portal_spawner(&ctx, spawner_pos), "spawner membership");
        // A chest piece must resolve its table.
        for piece in layout.iter() {
            let f = piece.frame();
            match piece.kind {
                Kind::ChestCorridor => {
                    assert_eq!(
                        chest_kind_at(&ctx, f.world(3, 2, 3)),
                        Some(StrongholdChest::Corridor)
                    );
                }
                Kind::Library { .. } => {
                    assert_eq!(
                        chest_kind_at(&ctx, f.world(3, 3, 5)),
                        Some(StrongholdChest::Library)
                    );
                }
                _ => {}
            }
        }
        println!("chest membership re-derives");
    }
}
