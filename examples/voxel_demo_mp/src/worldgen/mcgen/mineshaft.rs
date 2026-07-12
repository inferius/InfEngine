//! W243 - MC mineshaft structure port.
//!
//! Sources (decompiled 26.1.2):
//! - `worldgen/structure_set/mineshafts.json` - random_spread placement
//!   with spacing 1 / separation 0 / salt 0 / frequency 0.004: every
//!   chunk is a candidate start at 0.4% (the `legacy_type_3` frequency
//!   reduction reduces to "roll < 0.004" per chunk; we roll our own
//!   SplitMix64 stream instead of the Java LCG - same probability,
//!   different stream, consistent with the W242 monster-room policy).
//! - `MineshaftStructure.java` - the start room spawns at chunk block
//!   (2, y=50, 2); NORMAL variants sink the whole piece tree below sea
//!   level (`moveBelowSeaLevel(63, minY, rand, 10)`), MESA variants
//!   lift it toward the surface. The variant follows the biome
//!   (`has_structure/mineshaft_mesa` = badlands family).
//! - `MineshaftPieces.java` - the piece tree (room -> corridors /
//!   crossings / stairs, depth cap 8, +-80 block reach) and the
//!   per-chunk `postProcess` carving: plank floors, fence+beam
//!   supports, 1/3 rail corridors, 1/23 cave-spider corridors with
//!   cobwebs + a spawner, 1/100-per-section loot chests.
//!
//! Replay model (the W226.1 rules, extended for a multi-chunk
//! structure): the piece LAYOUT is a pure function of (effective
//! seed, start chunk) - `findCollisionPiece` only consults the
//! structure's own pieces, so every chunk that replays a start
//! reaches the identical tree. Each generated chunk scans candidate
//! start chunks within `START_RADIUS` and applies the pieces that
//! intersect its column. Carving mirrors MC's per-chunk semantics:
//! MC hands `postProcess` a full-height single-chunk clip box and a
//! per-chunk RNG; we hand it a full-height single-COLUMN clip and a
//! per-(column, start) stream, so every vertical cube of a column
//! replays the same roll sequence and applies only its own cells.
//!
//! Honest gaps vs MC (documented, deliberate):
//! - Chest MINECARTS become chest BLOCKS (no cart-container entity
//!   yet); the cell + cadence (1/100 per section, air-above-floor
//!   rule) match, the loot is `abandoned_mineshaft` via the W242
//!   interpreter at chest delivery.
//! - Cave-spider spawners spawn regular spiders until the cave
//!   spider mob is ported (`mob::spawner` maps corridor spawners).
//! - `iron_chain` hanging supports substitute the fence block; the
//!   5% wall-torch decoration is skipped (worldgen cannot reach the
//!   torch-orient map).
//! - Reads outside the live 16^3 cube fall back to the post-carve
//!   solidity oracle: aquifer water out of cube reads as air (the
//!   liquid-abort guard only sees in-cube water), and cross-cube
//!   support/rail probes can disagree with the sibling cube's real
//!   content at vertical cube borders - bounded, deterministic seams
//!   of the same class MC itself has at horizontal chunk borders.
//! - MESA wood: dark-oak planks/log blocks don't exist yet - mesa
//!   shafts use oak planks/log with the dark-oak FENCE (id 547).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::features::stream_seed;
use super::noise::SplitMix64;
use super::terrain::{column_is_solid, mc_floor_at, mc_full_biome_at};
use crate::block::{
    CHEST_ID, COBWEB_ID, DARK_OAK_FENCE_ID, LAVA_ID, OAK_FENCE_ID, PLANKS_ID,
    RAIL_ID, SPAWNER_ID, WATER_ID, WOOD_ID,
};
use crate::worldgen::WorldgenCtx;

/// Layout stream per start chunk (piece tree).
const SALT_MINESHAFT_START: u64 = 0xD4E6_0010;
/// Carve stream per (target column, start chunk).
const SALT_MINESHAFT_DECOR: u64 = 0xD4E6_0011;

/// `structure_set/mineshafts.json` frequency.
const FREQUENCY: f64 = 0.004;

/// Pieces reach at most +-80 blocks from the start room
/// (`generateAndAddPiece`), i.e. 6 chunks once piece extents are in.
const START_RADIUS: i32 = 6;

/// `MineshaftStructure.MAGIC_START_Y`.
const MAGIC_START_Y: i32 = 50;

/// Our world's sea level + build floor (worldgen minY). W260: the
/// floor is MC's -64, so `moveBelowSeaLevel` sinks normal shafts
/// into the deepslate depth exactly like MC.
const SEA_LEVEL: i32 = 63;
const WORLD_MIN_Y: i32 = -64;

/// Stand-in for "some solid rock" when a read leaves the live cube
/// and only the solidity oracle answers. Id 1 = stone.
const ORACLE_SOLID_ID: u16 = 1;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_f32(r: &mut SplitMix64) -> f32 {
    (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

fn next_bool(r: &mut SplitMix64) -> bool {
    r.next_u64() & 1 == 1
}

// ----------------------------------------------------------------
// Geometry.
// ----------------------------------------------------------------

/// MC `BoundingBox`: inclusive on every axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bb {
    pub(crate) x0: i32,
    pub(crate) y0: i32,
    pub(crate) z0: i32,
    pub(crate) x1: i32,
    pub(crate) y1: i32,
    pub(crate) z1: i32,
}

impl Bb {
    fn new(x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> Self {
        Self { x0, y0, z0, x1, y1, z1 }
    }

    pub(crate) fn moved(mut self, dx: i32, dy: i32, dz: i32) -> Self {
        self.x0 += dx;
        self.x1 += dx;
        self.y0 += dy;
        self.y1 += dy;
        self.z0 += dz;
        self.z1 += dz;
        self
    }

    pub(crate) fn intersects(&self, o: &Bb) -> bool {
        self.x1 >= o.x0
            && self.x0 <= o.x1
            && self.z1 >= o.z0
            && self.z0 <= o.z1
            && self.y1 >= o.y0
            && self.y0 <= o.y1
    }

    /// Horizontal-only overlap - the per-column clip test.
    fn intersects_column(&self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        self.x1 >= min_x && self.x0 <= max_x && self.z1 >= min_z && self.z0 <= max_z
    }

    pub(crate) fn contains(&self, p: [i32; 3]) -> bool {
        p[0] >= self.x0
            && p[0] <= self.x1
            && p[1] >= self.y0
            && p[1] <= self.y1
            && p[2] >= self.z0
            && p[2] <= self.z1
    }

    fn x_span(&self) -> i32 {
        self.x1 - self.x0 + 1
    }

    pub(crate) fn y_span(&self) -> i32 {
        self.y1 - self.y0 + 1
    }

    fn z_span(&self) -> i32 {
        self.z1 - self.z0 + 1
    }

    fn center(&self) -> [i32; 3] {
        [
            self.x0 + (self.x1 - self.x0 + 1) / 2,
            self.y0 + (self.y1 - self.y0 + 1) / 2,
            self.z0 + (self.z1 - self.z0 + 1) / 2,
        ]
    }
}

/// Horizontal facing of a piece (MC `Direction`, 2D subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    North,
    South,
    West,
    East,
}

/// Wood set per `MineshaftStructure.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShaftKind {
    Normal,
    Mesa,
}

impl ShaftKind {
    fn planks(self) -> u16 {
        PLANKS_ID
    }

    fn wood(self) -> u16 {
        WOOD_ID
    }

    fn fence(self) -> u16 {
        match self {
            ShaftKind::Normal => OAK_FENCE_ID,
            ShaftKind::Mesa => DARK_OAK_FENCE_ID,
        }
    }
}

#[derive(Debug, Clone)]
enum PieceKind {
    /// `MineShaftRoom` + the entrance cutouts recorded while children
    /// spawned. Orientation-less (world coordinates).
    Room { entrances: Vec<Bb> },
    /// `MineShaftCorridor`.
    Corridor { has_rails: bool, spider: bool, num_sections: i32 },
    /// `MineShaftCrossing`. Orientation-less in MC (the stored
    /// `direction` field only drives child spawning).
    Crossing { dir: Dir },
    /// `MineShaftStairs`.
    Stairs,
}

#[derive(Debug, Clone)]
pub(crate) struct Piece {
    kind: PieceKind,
    pub(crate) bb: Bb,
    /// `setOrientation` - corridors + stairs transform their local
    /// coordinates through this; rooms + crossings don't set it.
    orientation: Option<Dir>,
}

impl Piece {
    /// `Piece::is_corridor` - the cave-spider stand-in mapping keys
    /// on this from the spawner tick.
    pub(crate) fn is_corridor(&self) -> bool {
        matches!(self.kind, PieceKind::Corridor { .. })
    }

    /// `StructurePiece.getWorldX/Y/Z` - local -> world through the
    /// piece bb + orientation.
    fn world(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        match self.orientation {
            None => [x, y, z],
            Some(Dir::North) => [self.bb.x0 + x, self.bb.y0 + y, self.bb.z1 - z],
            Some(Dir::South) => [self.bb.x0 + x, self.bb.y0 + y, self.bb.z0 + z],
            Some(Dir::West) => [self.bb.x1 - z, self.bb.y0 + y, self.bb.z0 + x],
            Some(Dir::East) => [self.bb.x0 + z, self.bb.y0 + y, self.bb.z0 + x],
        }
    }
}

/// One laid-out mineshaft: the deterministic piece tree of a start.
#[derive(Debug)]
pub(crate) struct Mineshaft {
    pub(crate) kind: ShaftKind,
    pub(crate) pieces: Vec<Piece>,
}

// ----------------------------------------------------------------
// Layout (MineshaftStructure.findGenerationPoint + piece recursion).
// ----------------------------------------------------------------

/// True when the badlands biome family owns this column
/// (`has_structure/mineshaft_mesa`).
fn is_mesa_biome(b: McBiome) -> bool {
    matches!(
        b,
        McBiome::Badlands | McBiome::WoodedBadlands | McBiome::ErodedBadlands
    )
}

/// `createRandomShaftPiece`: 80% corridor, 10% stairs, 20%.. wait -
/// roll >= 80 crossing (20%), >= 70 stairs (10%), else corridor (70%).
fn create_random_piece(
    pieces: &[Piece],
    rng: &mut SplitMix64,
    foot: [i32; 3],
    dir: Dir,
) -> Option<Piece> {
    let sel = next_int(rng, 100);
    if sel >= 80 {
        let bb = find_crossing(pieces, rng, foot, dir)?;
        Some(Piece { kind: PieceKind::Crossing { dir }, bb, orientation: None })
    } else if sel >= 70 {
        let bb = find_stairs(pieces, foot, dir)?;
        Some(Piece { kind: PieceKind::Stairs, bb, orientation: Some(dir) })
    } else {
        let bb = find_corridor(pieces, rng, foot, dir)?;
        let has_rails = next_int(rng, 3) == 0;
        // Java short-circuits: the 1/23 spider roll is only consumed
        // when the rails roll failed.
        let spider = !has_rails && next_int(rng, 23) == 0;
        let num_sections = if matches!(dir, Dir::North | Dir::South) {
            bb.z_span() / 5
        } else {
            bb.x_span() / 5
        };
        Some(Piece {
            kind: PieceKind::Corridor { has_rails, spider, num_sections },
            bb,
            orientation: Some(dir),
        })
    }
}

fn collides(pieces: &[Piece], bb: &Bb) -> bool {
    pieces.iter().any(|p| p.bb.intersects(bb))
}

/// `MineShaftCorridor.findCorridorSize`.
fn find_corridor(pieces: &[Piece], rng: &mut SplitMix64, foot: [i32; 3], dir: Dir) -> Option<Bb> {
    let mut len_sections = next_int(rng, 3) + 2;
    while len_sections > 0 {
        let l = len_sections * 5;
        let raw = match dir {
            Dir::North => Bb::new(0, 0, -(l - 1), 2, 2, 0),
            Dir::South => Bb::new(0, 0, 0, 2, 2, l - 1),
            Dir::West => Bb::new(-(l - 1), 0, 0, 0, 2, 2),
            Dir::East => Bb::new(0, 0, 0, l - 1, 2, 2),
        };
        let bb = raw.moved(foot[0], foot[1], foot[2]);
        if !collides(pieces, &bb) {
            return Some(bb);
        }
        len_sections -= 1;
    }
    None
}

/// `MineShaftCrossing.findCrossing`.
fn find_crossing(pieces: &[Piece], rng: &mut SplitMix64, foot: [i32; 3], dir: Dir) -> Option<Bb> {
    let y1 = if next_int(rng, 4) == 0 { 6 } else { 2 };
    let raw = match dir {
        Dir::North => Bb::new(-1, 0, -4, 3, y1, 0),
        Dir::South => Bb::new(-1, 0, 0, 3, y1, 4),
        Dir::West => Bb::new(-4, 0, -1, 0, y1, 3),
        Dir::East => Bb::new(0, 0, -1, 4, y1, 3),
    };
    let bb = raw.moved(foot[0], foot[1], foot[2]);
    if collides(pieces, &bb) { None } else { Some(bb) }
}

/// `MineShaftStairs.findStairs`.
fn find_stairs(pieces: &[Piece], foot: [i32; 3], dir: Dir) -> Option<Bb> {
    let raw = match dir {
        Dir::North => Bb::new(0, -5, -8, 2, 2, 0),
        Dir::South => Bb::new(0, -5, 0, 2, 2, 8),
        Dir::West => Bb::new(-8, -5, 0, 0, 2, 2),
        Dir::East => Bb::new(0, -5, 0, 8, 2, 2),
    };
    let bb = raw.moved(foot[0], foot[1], foot[2]);
    if collides(pieces, &bb) { None } else { Some(bb) }
}

/// `generateAndAddPiece`: depth cap 8, +-80 block clamp against the
/// start room, then create + recurse. Returns the new piece's index.
fn generate_and_add(
    pieces: &mut Vec<Piece>,
    rng: &mut SplitMix64,
    foot: [i32; 3],
    dir: Dir,
    depth: i32,
) -> Option<usize> {
    if depth > 8 {
        return None;
    }
    let start_bb = pieces[0].bb;
    if (foot[0] - start_bb.x0).abs() > 80 || (foot[2] - start_bb.z0).abs() > 80 {
        return None;
    }
    let piece = create_random_piece(pieces, rng, foot, dir)?;
    let idx = pieces.len();
    pieces.push(piece);
    add_children(pieces, idx, rng, depth + 1);
    Some(idx)
}

/// The per-kind `addChildren` bodies. `depth` is the freshly-created
/// piece's genDepth (parent depth + 1), matching the Java field.
fn add_children(pieces: &mut Vec<Piece>, idx: usize, rng: &mut SplitMix64, depth: i32) {
    let (kind, bb, orientation) = {
        let p = &pieces[idx];
        (p.kind.clone(), p.bb, p.orientation)
    };
    match kind {
        PieceKind::Corridor { .. } => {
            corridor_children(pieces, bb, orientation, rng, depth);
        }
        PieceKind::Crossing { dir } => {
            crossing_children(pieces, bb, dir, rng, depth);
        }
        PieceKind::Stairs => {
            let dir = orientation.expect("stairs carry orientation");
            let foot = match dir {
                Dir::North => [bb.x0, bb.y0, bb.z0 - 1],
                Dir::South => [bb.x0, bb.y0, bb.z1 + 1],
                Dir::West => [bb.x0 - 1, bb.y0, bb.z0],
                Dir::East => [bb.x1 + 1, bb.y0, bb.z0],
            };
            generate_and_add(pieces, rng, foot, dir, depth);
        }
        PieceKind::Room { .. } => {
            unreachable!("room children spawn from layout_for_start");
        }
    }
}

/// `MineShaftCorridor.addChildren`.
fn corridor_children(
    pieces: &mut Vec<Piece>,
    bb: Bb,
    orientation: Option<Dir>,
    rng: &mut SplitMix64,
    depth: i32,
) {
    let end_selection = next_int(rng, 4);
    if let Some(dir) = orientation {
        match dir {
            Dir::North => {
                if end_selection <= 1 {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x0, y, bb.z0 - 1], Dir::North, depth);
                } else if end_selection == 2 {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x0 - 1, y, bb.z0], Dir::West, depth);
                } else {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x1 + 1, y, bb.z0], Dir::East, depth);
                }
            }
            Dir::South => {
                if end_selection <= 1 {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x0, y, bb.z1 + 1], Dir::South, depth);
                } else if end_selection == 2 {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x0 - 1, y, bb.z1 - 3], Dir::West, depth);
                } else {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x1 + 1, y, bb.z1 - 3], Dir::East, depth);
                }
            }
            Dir::West => {
                if end_selection <= 1 {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x0 - 1, y, bb.z0], Dir::West, depth);
                } else if end_selection == 2 {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x0, y, bb.z0 - 1], Dir::North, depth);
                } else {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x0, y, bb.z1 + 1], Dir::South, depth);
                }
            }
            Dir::East => {
                if end_selection <= 1 {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x1 + 1, y, bb.z0], Dir::East, depth);
                } else if end_selection == 2 {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x1 - 3, y, bb.z0 - 1], Dir::North, depth);
                } else {
                    let y = bb.y0 - 1 + next_int(rng, 3);
                    generate_and_add(pieces, rng, [bb.x1 - 3, y, bb.z1 + 1], Dir::South, depth);
                }
            }
        }
    }
    // Side branches every 5 blocks, 1/5 chance each side.
    if depth < 8 {
        match orientation {
            Some(Dir::North) | Some(Dir::South) => {
                let mut z = bb.z0 + 3;
                while z + 3 <= bb.z1 {
                    let sel = next_int(rng, 5);
                    if sel == 0 {
                        generate_and_add(pieces, rng, [bb.x0 - 1, bb.y0, z], Dir::West, depth + 1);
                    } else if sel == 1 {
                        generate_and_add(pieces, rng, [bb.x1 + 1, bb.y0, z], Dir::East, depth + 1);
                    }
                    z += 5;
                }
            }
            _ => {
                let mut x = bb.x0 + 3;
                while x + 3 <= bb.x1 {
                    let sel = next_int(rng, 5);
                    if sel == 0 {
                        generate_and_add(pieces, rng, [x, bb.y0, bb.z0 - 1], Dir::North, depth + 1);
                    } else if sel == 1 {
                        generate_and_add(pieces, rng, [x, bb.y0, bb.z1 + 1], Dir::South, depth + 1);
                    }
                    x += 5;
                }
            }
        }
    }
}

/// `MineShaftCrossing.addChildren` - runs right after the crossing is
/// pushed (the Java object keeps `direction`; we thread it through).
fn crossing_children(pieces: &mut Vec<Piece>, bb: Bb, dir: Dir, rng: &mut SplitMix64, depth: i32) {
    let two_floors = bb.y_span() > 3;
    match dir {
        Dir::North => {
            generate_and_add(pieces, rng, [bb.x0 + 1, bb.y0, bb.z0 - 1], Dir::North, depth);
            generate_and_add(pieces, rng, [bb.x0 - 1, bb.y0, bb.z0 + 1], Dir::West, depth);
            generate_and_add(pieces, rng, [bb.x1 + 1, bb.y0, bb.z0 + 1], Dir::East, depth);
        }
        Dir::South => {
            generate_and_add(pieces, rng, [bb.x0 + 1, bb.y0, bb.z1 + 1], Dir::South, depth);
            generate_and_add(pieces, rng, [bb.x0 - 1, bb.y0, bb.z0 + 1], Dir::West, depth);
            generate_and_add(pieces, rng, [bb.x1 + 1, bb.y0, bb.z0 + 1], Dir::East, depth);
        }
        Dir::West => {
            generate_and_add(pieces, rng, [bb.x0 + 1, bb.y0, bb.z0 - 1], Dir::North, depth);
            generate_and_add(pieces, rng, [bb.x0 + 1, bb.y0, bb.z1 + 1], Dir::South, depth);
            generate_and_add(pieces, rng, [bb.x0 - 1, bb.y0, bb.z0 + 1], Dir::West, depth);
        }
        Dir::East => {
            generate_and_add(pieces, rng, [bb.x0 + 1, bb.y0, bb.z0 - 1], Dir::North, depth);
            generate_and_add(pieces, rng, [bb.x0 + 1, bb.y0, bb.z1 + 1], Dir::South, depth);
            generate_and_add(pieces, rng, [bb.x1 + 1, bb.y0, bb.z0 + 1], Dir::East, depth);
        }
    }
    if two_floors {
        if next_bool(rng) {
            generate_and_add(pieces, rng, [bb.x0 + 1, bb.y0 + 4, bb.z0 - 1], Dir::North, depth);
        }
        if next_bool(rng) {
            generate_and_add(pieces, rng, [bb.x0 - 1, bb.y0 + 4, bb.z0 + 1], Dir::West, depth);
        }
        if next_bool(rng) {
            generate_and_add(pieces, rng, [bb.x1 + 1, bb.y0 + 4, bb.z0 + 1], Dir::East, depth);
        }
        if next_bool(rng) {
            generate_and_add(pieces, rng, [bb.x0 + 1, bb.y0 + 4, bb.z1 + 1], Dir::South, depth);
        }
    }
}

/// `MineShaftRoom.addChildren` - four wall sweeps spawning children
/// with entrance cutouts recorded on the room.
fn room_children(pieces: &mut Vec<Piece>, rng: &mut SplitMix64) {
    let room_bb = pieces[0].bb;
    let mut entrances: Vec<Bb> = Vec::new();
    let height_space = (room_bb.y_span() - 3 - 1).max(1);
    let depth = 0;

    let mut pos = 0;
    while pos < room_bb.x_span() {
        pos += next_int(rng, room_bb.x_span());
        if pos + 3 > room_bb.x_span() {
            break;
        }
        let y = room_bb.y0 + next_int(rng, height_space) + 1;
        let child = generate_and_add(
            pieces,
            rng,
            [room_bb.x0 + pos, y, room_bb.z0 - 1],
            Dir::North,
            depth,
        );
        if let Some(ci) = child {
            let cb = pieces[ci].bb;
            entrances.push(Bb::new(cb.x0, cb.y0, room_bb.z0, cb.x1, cb.y1, room_bb.z0 + 1));
        }
        pos += 4;
    }

    pos = 0;
    while pos < room_bb.x_span() {
        pos += next_int(rng, room_bb.x_span());
        if pos + 3 > room_bb.x_span() {
            break;
        }
        let y = room_bb.y0 + next_int(rng, height_space) + 1;
        let child = generate_and_add(
            pieces,
            rng,
            [room_bb.x0 + pos, y, room_bb.z1 + 1],
            Dir::South,
            depth,
        );
        if let Some(ci) = child {
            let cb = pieces[ci].bb;
            entrances.push(Bb::new(cb.x0, cb.y0, room_bb.z1 - 1, cb.x1, cb.y1, room_bb.z1));
        }
        pos += 4;
    }

    pos = 0;
    while pos < room_bb.z_span() {
        pos += next_int(rng, room_bb.z_span());
        if pos + 3 > room_bb.z_span() {
            break;
        }
        let y = room_bb.y0 + next_int(rng, height_space) + 1;
        let child = generate_and_add(
            pieces,
            rng,
            [room_bb.x0 - 1, y, room_bb.z0 + pos],
            Dir::West,
            depth,
        );
        if let Some(ci) = child {
            let cb = pieces[ci].bb;
            entrances.push(Bb::new(room_bb.x0, cb.y0, cb.z0, room_bb.x0 + 1, cb.y1, cb.z1));
        }
        pos += 4;
    }

    pos = 0;
    while pos < room_bb.z_span() {
        pos += next_int(rng, room_bb.z_span());
        if pos + 3 > room_bb.z_span() {
            break;
        }
        let y = room_bb.y0 + next_int(rng, height_space) + 1;
        let child = generate_and_add(
            pieces,
            rng,
            [room_bb.x1 + 1, y, room_bb.z0 + pos],
            Dir::East,
            depth,
        );
        if let Some(ci) = child {
            let cb = pieces[ci].bb;
            entrances.push(Bb::new(room_bb.x1 - 1, cb.y0, cb.z0, room_bb.x1, cb.y1, cb.z1));
        }
        pos += 4;
    }

    if let PieceKind::Room { entrances: e } = &mut pieces[0].kind {
        *e = entrances;
    }
}

/// Build the full deterministic layout for a start chunk, or `None`
/// when the 0.4% roll misses. Pure in (effective seed, start chunk).
pub(crate) fn layout_for_start(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Rc<Mineshaft>> {
    /// Memoized layouts (incl. negative rolls) per worker thread,
    /// keyed by (effective seed, start chunk).
    type LayoutCache = HashMap<(u32, i32, i32), Option<Rc<Mineshaft>>>;
    thread_local! {
        static CACHE: RefCell<LayoutCache> = RefCell::new(HashMap::new());
    }
    let seed = ctx.effective_seed();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned()) {
        return hit;
    }
    let built = build_layout(ctx, scx, scz).map(Rc::new);
    CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() > 4096 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Mineshaft> {
    let seed = ctx.effective_seed();
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_MINESHAFT_START));
    if rng.next_f64() >= FREQUENCY {
        return None;
    }
    let cs = CHUNK_SIZE as i32;
    // Variant: the badlands family gets the mesa shaft
    // (has_structure/mineshaft vs mineshaft_mesa biome tags).
    let kind = if is_mesa_biome(mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8)) {
        ShaftKind::Mesa
    } else {
        ShaftKind::Normal
    };
    // MineShaftRoom: west/north corner at chunk block (2, MAGIC_Y, 2).
    let west = scx * cs + 2;
    let north = scz * cs + 2;
    let room_bb = Bb::new(
        west,
        MAGIC_START_Y,
        north,
        west + 7 + next_int(&mut rng, 6),
        MAGIC_START_Y + 4 + next_int(&mut rng, 6),
        north + 7 + next_int(&mut rng, 6),
    );
    let mut pieces = vec![Piece {
        kind: PieceKind::Room { entrances: Vec::new() },
        bb: room_bb,
        orientation: None,
    }];
    room_children(&mut pieces, &mut rng);

    // Whole-tree vertical shift.
    let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
    for p in &pieces {
        min_y = min_y.min(p.bb.y0);
        max_y = max_y.max(p.bb.y1);
    }
    let dy = match kind {
        ShaftKind::Normal => {
            // StructurePiecesBuilder.moveBelowSeaLevel(63, 0, rand, 10).
            let clamp_max = SEA_LEVEL - 10;
            let mut target_top = (max_y - min_y + 1) + WORLD_MIN_Y + 1;
            if target_top < clamp_max {
                target_top += next_int(&mut rng, clamp_max - target_top);
            }
            target_top - max_y
        }
        ShaftKind::Mesa => {
            // MineshaftStructure mesa arm: lift the tree centre to a
            // random y in [sea level, surface].
            let center = pieces
                .iter()
                .fold(Bb::new(i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN), |a, p| {
                    Bb::new(
                        a.x0.min(p.bb.x0),
                        a.y0.min(p.bb.y0),
                        a.z0.min(p.bb.z0),
                        a.x1.max(p.bb.x1),
                        a.y1.max(p.bb.y1),
                        a.z1.max(p.bb.z1),
                    )
                })
                .center();
            let surface = mc_floor_at(ctx, center[0], center[2]);
            let target = if surface <= SEA_LEVEL {
                SEA_LEVEL
            } else {
                SEA_LEVEL + next_int(&mut rng, surface - SEA_LEVEL + 1)
            };
            target - center[1]
        }
    };
    for p in &mut pieces {
        p.bb = p.bb.moved(0, dy, 0);
        if let PieceKind::Room { entrances } = &mut p.kind {
            for e in entrances {
                *e = e.moved(0, dy, 0);
            }
        }
    }
    Some(Mineshaft { kind, pieces })
}

// ----------------------------------------------------------------
// Carving (per-column postProcess).
// ----------------------------------------------------------------

/// The world surface a mineshaft piece reads + writes through. Prod
/// impl wraps the live chunk cube + the column oracles; tests use a
/// hash-map world. Mirrors MC's chunk-clipped `WorldGenLevel` view.
pub(crate) trait ShaftWorld {
    /// Block id at a world cell. MC semantics: outside the clip
    /// column -> air; inside the live cube -> exact; elsewhere in
    /// the column -> a solidity approximation.
    fn read(&self, pos: [i32; 3]) -> u16;
    /// Write clipped to the live cube.
    fn place(&mut self, pos: [i32; 3], id: u16);
    /// `OCEAN_FLOOR_WG` heightmap stand-in (isInterior).
    fn floor_at(&self, wx: i32, wz: i32) -> i32;
    /// The x/z clip box (inclusive), i.e. "the chunk" in MC terms.
    fn column(&self) -> (i32, i32, i32, i32);
}

fn in_column(w: &dyn ShaftWorld, pos: [i32; 3]) -> bool {
    let (x0, z0, x1, z1) = w.column();
    pos[0] >= x0 && pos[0] <= x1 && pos[2] >= z0 && pos[2] <= z1 && pos[1] >= WORLD_MIN_Y && pos[1] < 320
}

/// Live-pipeline `ShaftWorld` over the chunk being generated. Shared
/// with the other hardcoded-piece structures (W246 desert pyramid).
pub(crate) struct ChunkShaftWorld<'a> {
    pub(crate) chunk: &'a mut Chunk,
    pub(crate) origin: [i32; 3],
    pub(crate) ctx: &'a WorldgenCtx,
}

impl ShaftWorld for ChunkShaftWorld<'_> {
    fn read(&self, pos: [i32; 3]) -> u16 {
        let cs = CHUNK_SIZE as i32;
        let (lx, ly, lz) = (
            pos[0] - self.origin[0],
            pos[1] - self.origin[1],
            pos[2] - self.origin[2],
        );
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            return self.chunk.get(lx as usize, ly as usize, lz as usize).0;
        }
        // Outside the column -> air (the MC chunk clip). Inside the
        // column but off this cube -> post-carve solidity oracle.
        if pos[0] < self.origin[0]
            || pos[0] >= self.origin[0] + cs
            || pos[2] < self.origin[2]
            || pos[2] >= self.origin[2] + cs
            || !(-64..320).contains(&pos[1])
        {
            return 0;
        }
        if column_is_solid(self.ctx, pos[0], pos[1], pos[2]) {
            ORACLE_SOLID_ID
        } else {
            0
        }
    }

    fn place(&mut self, pos: [i32; 3], id: u16) {
        let cs = CHUNK_SIZE as i32;
        let (lx, ly, lz) = (
            pos[0] - self.origin[0],
            pos[1] - self.origin[1],
            pos[2] - self.origin[2],
        );
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            self.chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
        }
    }

    fn floor_at(&self, wx: i32, wz: i32) -> i32 {
        mc_floor_at(self.ctx, wx, wz)
    }

    fn column(&self) -> (i32, i32, i32, i32) {
        let cs = CHUNK_SIZE as i32;
        (
            self.origin[0],
            self.origin[2],
            self.origin[0] + cs - 1,
            self.origin[2] + cs - 1,
        )
    }
}

/// Solid-looking read: not air, not a liquid, not a decoration we
/// know is thin. Approximates `BlockState.isSolidRender` for floor
/// probes.
fn read_solid(w: &dyn ShaftWorld, pos: [i32; 3]) -> bool {
    let id = w.read(pos);
    id != 0
        && id != WATER_ID
        && id != LAVA_ID
        && crate::block::block_type(id).is_none_or(|bt| !bt.transparent)
}

fn read_liquid(w: &dyn ShaftWorld, pos: [i32; 3]) -> bool {
    let id = w.read(pos);
    id == WATER_ID || id == LAVA_ID
}

/// `MineShaftPiece.canBeReplaced` - never overwrite the shaft's own
/// woodwork (planks / log / either fence; the chain substitute is a
/// fence, so it's covered).
fn guarded(w: &dyn ShaftWorld, pos: [i32; 3]) -> bool {
    let id = w.read(pos);
    id != PLANKS_ID && id != WOOD_ID && id != OAK_FENCE_ID && id != DARK_OAK_FENCE_ID
}

/// `StructurePiece.placeBlock`: clip to column + replace guard.
fn place_block(w: &mut dyn ShaftWorld, piece: &Piece, x: i32, y: i32, z: i32, id: u16) {
    let pos = piece.world(x, y, z);
    if in_column(w, pos) && guarded(w, pos) {
        w.place(pos, id);
    }
}

/// `StructurePiece.getBlock`: air outside the clip column.
fn get_block(w: &dyn ShaftWorld, piece: &Piece, x: i32, y: i32, z: i32) -> u16 {
    let pos = piece.world(x, y, z);
    if in_column(w, pos) { w.read(pos) } else { 0 }
}

/// `StructurePiece.isInterior`: below the WG ocean-floor heightmap.
fn is_interior(w: &dyn ShaftWorld, piece: &Piece, x: i32, y: i32, z: i32) -> bool {
    let pos = piece.world(x, y + 1, z);
    in_column(w, pos) && pos[1] < w.floor_at(pos[0], pos[2])
}

/// `generateBox(edge, fill, skipAir=false)` with a single state.
fn generate_box(
    w: &mut dyn ShaftWorld,
    piece: &Piece,
    (x0, y0, z0): (i32, i32, i32),
    (x1, y1, z1): (i32, i32, i32),
    id: u16,
) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                place_block(w, piece, x, y, z, id);
            }
        }
    }
}

/// `generateMaybeBox`: rolls for EVERY cell (stream parity), writes
/// the survivors.
#[allow(clippy::too_many_arguments)]
fn generate_maybe_box(
    w: &mut dyn ShaftWorld,
    piece: &Piece,
    rng: &mut SplitMix64,
    probability: f32,
    (x0, y0, z0): (i32, i32, i32),
    (x1, y1, z1): (i32, i32, i32),
    id: u16,
    has_to_be_inside: bool,
) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if next_f32(rng) > probability {
                    continue;
                }
                if has_to_be_inside && !is_interior(w, piece, x, y, z) {
                    continue;
                }
                place_block(w, piece, x, y, z, id);
            }
        }
    }
}

/// `MineShaftPiece.isInInvalidLocation`: any liquid on the clipped
/// hull (the deep-dark biome arm has no domain in our biome set).
fn is_in_invalid_location(w: &dyn ShaftWorld, bb: &Bb) -> bool {
    let (cx0, cz0, cx1, cz1) = w.column();
    let x0 = (bb.x0 - 1).max(cx0);
    let y0 = (bb.y0 - 1).max(WORLD_MIN_Y);
    let z0 = (bb.z0 - 1).max(cz0);
    let x1 = (bb.x1 + 1).min(cx1);
    let y1 = (bb.y1 + 1).min(319);
    let z1 = (bb.z1 + 1).min(cz1);
    for x in x0..=x1 {
        for z in z0..=z1 {
            if read_liquid(w, [x, y0, z]) || read_liquid(w, [x, y1, z]) {
                return true;
            }
        }
    }
    for x in x0..=x1 {
        for y in y0..=y1 {
            if read_liquid(w, [x, y, z0]) || read_liquid(w, [x, y, z1]) {
                return true;
            }
        }
    }
    for z in z0..=z1 {
        for y in y0..=y1 {
            if read_liquid(w, [x0, y, z]) || read_liquid(w, [x1, y, z]) {
                return true;
            }
        }
    }
    false
}

/// `setPlanksBlock`: replace non-sturdy floor cells, interior only.
fn set_planks_block(w: &mut dyn ShaftWorld, piece: &Piece, planks: u16, x: i32, y: i32, z: i32) {
    if !is_interior(w, piece, x, y, z) {
        return;
    }
    let pos = piece.world(x, y, z);
    if !in_column(w, pos) {
        return;
    }
    if !read_solid(w, pos) {
        w.place(pos, planks);
    }
}

/// `MineShaftCorridor.postProcess` + its helpers.
#[allow(clippy::too_many_arguments)]
fn corridor_post_process(
    w: &mut dyn ShaftWorld,
    piece: &Piece,
    kind: ShaftKind,
    has_rails: bool,
    spider: bool,
    num_sections: i32,
    rng: &mut SplitMix64,
) {
    if is_in_invalid_location(w, &piece.bb) {
        return;
    }
    let len = num_sections * 5 - 1;
    let planks = kind.planks();
    generate_box(w, piece, (0, 0, 0), (2, 1, len), 0);
    generate_maybe_box(w, piece, rng, 0.8, (0, 2, 0), (2, 2, len), 0, false);
    if spider {
        generate_maybe_box(w, piece, rng, 0.6, (0, 0, 0), (2, 1, len), COBWEB_ID, true);
    }
    let mut spider_placed = false;
    for section in 0..num_sections {
        let z = 2 + section * 5;
        place_support(w, piece, kind, 0, 0, z, 2, 2, rng);
        maybe_place_cobweb(w, piece, rng, 0.1, 0, 2, z - 1);
        maybe_place_cobweb(w, piece, rng, 0.1, 2, 2, z - 1);
        maybe_place_cobweb(w, piece, rng, 0.1, 0, 2, z + 1);
        maybe_place_cobweb(w, piece, rng, 0.1, 2, 2, z + 1);
        maybe_place_cobweb(w, piece, rng, 0.05, 0, 2, z - 2);
        maybe_place_cobweb(w, piece, rng, 0.05, 2, 2, z - 2);
        maybe_place_cobweb(w, piece, rng, 0.05, 0, 2, z + 2);
        maybe_place_cobweb(w, piece, rng, 0.05, 2, 2, z + 2);
        if next_int(rng, 100) == 0 {
            create_chest(w, piece, rng, 2, 0, z - 1);
        }
        if next_int(rng, 100) == 0 {
            create_chest(w, piece, rng, 0, 0, z + 1);
        }
        if spider && !spider_placed {
            let new_z = z - 1 + next_int(rng, 3);
            let pos = piece.world(1, 0, new_z);
            if in_column(w, pos) && is_interior(w, piece, 1, 0, new_z) {
                spider_placed = true;
                w.place(pos, SPAWNER_ID);
            }
        }
    }
    for x in 0..=2 {
        for z in 0..=len {
            set_planks_block(w, piece, planks, x, -1, z);
        }
    }
    place_double_support(w, piece, kind, 0, -1, 2);
    if num_sections > 1 {
        place_double_support(w, piece, kind, 0, -1, len - 2);
    }
    if has_rails {
        for z in 0..=len {
            let floor = piece.world(1, -1, z);
            if in_column(w, floor) && read_solid(w, floor) {
                let probability = if is_interior(w, piece, 1, 0, z) { 0.7 } else { 0.9 };
                if next_f32(rng) < probability {
                    place_block(w, piece, 1, 0, z, RAIL_ID);
                }
            }
        }
    }
}

/// Corridor `createChest`: rail-under-cart becomes a chest block (the
/// documented minecart deviation). Loot rolls at chest delivery.
fn create_chest(w: &mut dyn ShaftWorld, piece: &Piece, rng: &mut SplitMix64, x: i32, y: i32, z: i32) {
    let pos = piece.world(x, y, z);
    // MC: target air + below NOT air (any non-air state carries the
    // cart, water included).
    if in_column(w, pos)
        && w.read(pos) == 0
        && w.read([pos[0], pos[1] - 1, pos[2]]) != 0
    {
        // MC rolls the rail's N-S/E-W shape here; our rails derive
        // shape from neighbours, but consume the roll for parity of
        // the stream with the Java flow.
        let _ = next_bool(rng);
        w.place(pos, CHEST_ID);
    }
}

/// `maybePlaceCobWeb`: interior gate short-circuits BEFORE the roll
/// (Java `&&` order), sturdy-neighbour check after.
fn maybe_place_cobweb(
    w: &mut dyn ShaftWorld,
    piece: &Piece,
    rng: &mut SplitMix64,
    probability: f32,
    x: i32,
    y: i32,
    z: i32,
) {
    if !is_interior(w, piece, x, y, z) {
        return;
    }
    if next_f32(rng) >= probability {
        return;
    }
    let pos = piece.world(x, y, z);
    let mut sturdy = 0;
    for d in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
        let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
        if in_column(w, n) && read_solid(w, n) {
            sturdy += 1;
            if sturdy >= 2 {
                break;
            }
        }
    }
    if sturdy >= 2 {
        place_block(w, piece, x, y, z, COBWEB_ID);
    }
}

/// `placeSupport`: fence legs + plank beam, 5% wall torches skipped.
#[allow(clippy::too_many_arguments)]
fn place_support(
    w: &mut dyn ShaftWorld,
    piece: &Piece,
    kind: ShaftKind,
    x0: i32,
    y0: i32,
    z: i32,
    y1: i32,
    x1: i32,
    rng: &mut SplitMix64,
) {
    // isSupportingBox: the row above the ceiling must be fully solid.
    for x in x0..=x1 {
        if get_block(w, piece, x, y1 + 1, z) == 0 {
            return;
        }
    }
    let planks = kind.planks();
    let fence = kind.fence();
    generate_box(w, piece, (x0, y0, z), (x0, y1 - 1, z), fence);
    generate_box(w, piece, (x1, y0, z), (x1, y1 - 1, z), fence);
    if next_int(rng, 4) == 0 {
        generate_box(w, piece, (x0, y1, z), (x0, y1, z), planks);
        generate_box(w, piece, (x1, y1, z), (x1, y1, z), planks);
    } else {
        generate_box(w, piece, (x0, y1, z), (x1, y1, z), planks);
        // MC hangs 5% wall torches beside the beam here - skipped
        // (no worldgen path into the torch-orient map yet).
        let _ = (next_f32(rng), next_f32(rng));
    }
}

/// `placeDoubleLowerOrUpperSupport` + `fillPillarDownOrChainUp`.
fn place_double_support(w: &mut dyn ShaftWorld, piece: &Piece, kind: ShaftKind, x: i32, y: i32, z: i32) {
    let planks = kind.planks();
    if get_block(w, piece, x, y, z) == planks {
        fill_pillar_or_chain(w, piece, kind, x, y, z);
    }
    if get_block(w, piece, x + 2, y, z) == planks {
        fill_pillar_or_chain(w, piece, kind, x + 2, y, z);
    }
}

fn fill_pillar_or_chain(w: &mut dyn ShaftWorld, piece: &Piece, kind: ShaftKind, x: i32, y: i32, z: i32) {
    let pos = piece.world(x, y, z);
    if !in_column(w, pos) {
        return;
    }
    let wood = kind.wood();
    let fence = kind.fence();
    let world_y = pos[1];
    let mut dist = 1;
    let mut check_below = true;
    let mut check_above = true;
    while check_below || check_above {
        if check_below {
            let p = [pos[0], world_y - dist, pos[2]];
            let state = w.read(p);
            let empty_below = (state == 0 || state == WATER_ID) && state != LAVA_ID;
            if !empty_below && read_solid(w, p) {
                for py in (world_y - dist + 1)..world_y {
                    w.place([pos[0], py, pos[2]], wood);
                }
                return;
            }
            check_below = dist <= 20 && empty_below && p[1] > WORLD_MIN_Y + 1;
        }
        if check_above {
            let p = [pos[0], world_y + dist, pos[2]];
            let state = w.read(p);
            let empty_above = state == 0 || state == WATER_ID;
            if !empty_above && read_solid(w, p) {
                w.place([pos[0], world_y + 1, pos[2]], fence);
                // MC hangs IRON_CHAIN here; the fence substitutes
                // until a chain block exists.
                for py in (world_y + 2)..(world_y + dist) {
                    w.place([pos[0], py, pos[2]], fence);
                }
                return;
            }
            check_above = dist <= 50 && empty_above && p[1] < 255;
        }
        dist += 1;
    }
}

/// `MineShaftCrossing.postProcess` (world coordinates - orientation
/// is unset on crossings).
fn crossing_post_process(w: &mut dyn ShaftWorld, piece: &Piece, kind: ShaftKind) {
    if is_in_invalid_location(w, &piece.bb) {
        return;
    }
    let bb = piece.bb;
    let planks = kind.planks();
    let two_floors = bb.y_span() > 3;
    if two_floors {
        generate_box(w, piece, (bb.x0 + 1, bb.y0, bb.z0), (bb.x1 - 1, bb.y0 + 2, bb.z1), 0);
        generate_box(w, piece, (bb.x0, bb.y0, bb.z0 + 1), (bb.x1, bb.y0 + 2, bb.z1 - 1), 0);
        generate_box(w, piece, (bb.x0 + 1, bb.y1 - 2, bb.z0), (bb.x1 - 1, bb.y1, bb.z1), 0);
        generate_box(w, piece, (bb.x0, bb.y1 - 2, bb.z0 + 1), (bb.x1, bb.y1, bb.z1 - 1), 0);
        generate_box(w, piece, (bb.x0 + 1, bb.y0 + 3, bb.z0 + 1), (bb.x1 - 1, bb.y0 + 3, bb.z1 - 1), 0);
    } else {
        generate_box(w, piece, (bb.x0 + 1, bb.y0, bb.z0), (bb.x1 - 1, bb.y1, bb.z1), 0);
        generate_box(w, piece, (bb.x0, bb.y0, bb.z0 + 1), (bb.x1, bb.y1, bb.z1 - 1), 0);
    }
    crossing_pillar(w, piece, planks, bb.x0 + 1, bb.y0, bb.z0 + 1, bb.y1);
    crossing_pillar(w, piece, planks, bb.x0 + 1, bb.y0, bb.z1 - 1, bb.y1);
    crossing_pillar(w, piece, planks, bb.x1 - 1, bb.y0, bb.z0 + 1, bb.y1);
    crossing_pillar(w, piece, planks, bb.x1 - 1, bb.y0, bb.z1 - 1, bb.y1);
    let y = bb.y0 - 1;
    for x in bb.x0..=bb.x1 {
        for z in bb.z0..=bb.z1 {
            set_planks_block(w, piece, planks, x, y, z);
        }
    }
}

fn crossing_pillar(w: &mut dyn ShaftWorld, piece: &Piece, planks: u16, x: i32, y0: i32, z: i32, y1: i32) {
    if get_block(w, piece, x, y1 + 1, z) != 0 {
        generate_box(w, piece, (x, y0, z), (x, y1, z), planks);
    }
}

/// `MineShaftRoom.postProcess`.
fn room_post_process(w: &mut dyn ShaftWorld, piece: &Piece, entrances: &[Bb]) {
    if is_in_invalid_location(w, &piece.bb) {
        return;
    }
    let bb = piece.bb;
    generate_box(
        w,
        piece,
        (bb.x0, bb.y0 + 1, bb.z0),
        (bb.x1, (bb.y0 + 3).min(bb.y1), bb.z1),
        0,
    );
    for e in entrances {
        generate_box(w, piece, (e.x0, e.y1 - 2, e.z0), (e.x1, e.y1, e.z1), 0);
    }
    // generateUpperHalfSphere: dome the cavern roof.
    let (x0, y0, z0, x1, y1, z1) = (bb.x0, bb.y0 + 4, bb.z0, bb.x1, bb.y1, bb.z1);
    if y0 <= y1 {
        let dx = (x1 - x0 + 1) as f32;
        let dy = (y1 - y0 + 1) as f32;
        let dz = (z1 - z0 + 1) as f32;
        let cx = x0 as f32 + dx / 2.0;
        let cz = z0 as f32 + dz / 2.0;
        for y in y0..=y1 {
            let ny = (y - y0) as f32 / dy;
            for x in x0..=x1 {
                let nx = (x as f32 - cx) / (dx * 0.5);
                for z in z0..=z1 {
                    let nz = (z as f32 - cz) / (dz * 0.5);
                    if nx * nx + ny * ny + nz * nz <= 1.05 {
                        place_block(w, piece, x, y, z, 0);
                    }
                }
            }
        }
    }
}

/// `MineShaftStairs.postProcess` (local coordinates).
fn stairs_post_process(w: &mut dyn ShaftWorld, piece: &Piece) {
    if is_in_invalid_location(w, &piece.bb) {
        return;
    }
    generate_box(w, piece, (0, 5, 0), (2, 7, 1), 0);
    generate_box(w, piece, (0, 0, 7), (2, 2, 8), 0);
    for i in 0..5 {
        let dip = if i < 4 { 1 } else { 0 };
        generate_box(w, piece, (0, 5 - i - dip, 2 + i), (2, 7 - i, 2 + i), 0);
    }
}

/// Apply one laid-out mineshaft to the live chunk column: pieces in
/// list order, sharing one per-(column, start) stream.
fn apply_mineshaft(ms: &Mineshaft, w: &mut dyn ShaftWorld, rng: &mut SplitMix64) {
    let (cx0, cz0, cx1, cz1) = w.column();
    for piece in &ms.pieces {
        if !piece.bb.intersects_column(cx0, cz0, cx1, cz1) {
            continue;
        }
        match &piece.kind {
            PieceKind::Room { entrances } => room_post_process(w, piece, entrances),
            PieceKind::Corridor { has_rails, spider, num_sections } => corridor_post_process(
                w,
                piece,
                ms.kind,
                *has_rails,
                *spider,
                *num_sections,
                rng,
            ),
            PieceKind::Crossing { .. } => crossing_post_process(w, piece, ms.kind),
            PieceKind::Stairs => stairs_post_process(w, piece),
        }
    }
}

/// The per-chunk mineshaft pass: scan candidate start chunks in the
/// reach radius, lay out hits (cached), carve the intersecting
/// pieces into this chunk. Runs in UNDERGROUND_STRUCTURES before the
/// monster-room feature (structures precede features in the step).
pub(crate) fn place_mineshafts(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    for scx in (coord[0] - START_RADIUS)..=(coord[0] + START_RADIUS) {
        for scz in (coord[2] - START_RADIUS)..=(coord[2] + START_RADIUS) {
            let Some(ms) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            let mut rng = SplitMix64::new(
                stream_seed(seed, coord[0], coord[2], SALT_MINESHAFT_DECOR)
                    ^ stream_seed(seed, scx, scz, SALT_MINESHAFT_START),
            );
            let mut world = ChunkShaftWorld { chunk, origin, ctx };
            apply_mineshaft(&ms, &mut world, &mut rng);
        }
    }
}

/// Deterministic structure-membership probe: is this position inside
/// any mineshaft piece? Drives the chest-loot table pick at delivery
/// (abandoned_mineshaft vs simple_dungeon) without persisting any
/// provenance.
pub(crate) fn pos_in_mineshaft(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    pos_in_mineshaft_piece(ctx, pos, false)
}

/// Same probe restricted to CORRIDOR pieces - the cave-spider
/// spawner stand-in mapping keys on this from the spawner tick.
pub(crate) fn pos_in_mineshaft_corridor(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    pos_in_mineshaft_piece(ctx, pos, true)
}

fn pos_in_mineshaft_piece(ctx: &WorldgenCtx, pos: [i32; 3], corridors_only: bool) -> bool {
    // Mineshafts only exist in the MC worldgen profile; bail before
    // the layout path builds the MC noise stack for a classic world.
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    for scx in (ccx - START_RADIUS)..=(ccx + START_RADIUS) {
        for scz in (ccz - START_RADIUS)..=(ccz + START_RADIUS) {
            let Some(ms) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            for p in &ms.pieces {
                if (!corridors_only || p.is_corridor()) && p.bb.contains(pos) {
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
    use std::collections::HashMap as StdMap;

    /// Hash-map ShaftWorld with a solid floor plane and an unbounded
    /// column (tests want the whole structure applied at once).
    struct MapWorld {
        cells: StdMap<[i32; 3], u16>,
        floor_y: i32,
        surface_y: i32,
    }

    impl MapWorld {
        fn new(floor_y: i32, surface_y: i32) -> Self {
            Self { cells: StdMap::new(), floor_y, surface_y }
        }
    }

    impl ShaftWorld for MapWorld {
        fn read(&self, pos: [i32; 3]) -> u16 {
            if let Some(id) = self.cells.get(&pos) {
                return *id;
            }
            if pos[1] <= self.floor_y { ORACLE_SOLID_ID } else { 0 }
        }

        fn place(&mut self, pos: [i32; 3], id: u16) {
            self.cells.insert(pos, id);
        }

        fn floor_at(&self, _wx: i32, _wz: i32) -> i32 {
            self.surface_y
        }

        fn column(&self) -> (i32, i32, i32, i32) {
            (-10_000, -10_000, 10_000, 10_000)
        }
    }

    /// A hand-built straight corridor piece for the carve tests.
    fn test_corridor(sections: i32, dir: Dir) -> Piece {
        let l = sections * 5;
        let raw = match dir {
            Dir::North => Bb::new(0, 0, -(l - 1), 2, 2, 0),
            Dir::South => Bb::new(0, 0, 0, 2, 2, l - 1),
            Dir::West => Bb::new(-(l - 1), 0, 0, 0, 2, 2),
            Dir::East => Bb::new(0, 0, 0, l - 1, 2, 2),
        };
        Piece {
            kind: PieceKind::Corridor { has_rails: true, spider: false, num_sections: sections },
            bb: raw.moved(0, 40, 0),
            orientation: Some(dir),
        }
    }

    #[test]
    fn world_transform_matches_structure_piece() {
        // SOUTH: identity-ish (minX+x, minY+y, minZ+z).
        let p = test_corridor(1, Dir::South);
        assert_eq!(p.world(0, 0, 0), [0, 40, 0]);
        assert_eq!(p.world(2, 1, 4), [2, 41, 4]);
        // NORTH: z flips (maxZ - z).
        let p = test_corridor(1, Dir::North);
        assert_eq!(p.world(0, 0, 0), [0, 40, 0]);
        assert_eq!(p.world(0, 0, 4), [0, 40, -4]);
        // EAST: worldX = minX + z, worldZ = minZ + x.
        let p = test_corridor(1, Dir::East);
        assert_eq!(p.world(0, 0, 4), [4, 40, 0]);
        assert_eq!(p.world(2, 0, 0), [0, 40, 2]);
        // WEST: worldX = maxX - z.
        let p = test_corridor(1, Dir::West);
        assert_eq!(p.world(0, 0, 4), [-4, 40, 0]);
    }

    #[test]
    fn corridor_carves_air_and_rails_on_plank_floor() {
        // Floor plane right under the corridor (piece bb y0 = 40 ->
        // floor row y = 39), deep underground (surface at 200).
        let mut w = MapWorld::new(39, 200);
        let p = test_corridor(2, Dir::South);
        let mut rng = SplitMix64::new(7);
        corridor_post_process(&mut w, &p, ShaftKind::Normal, true, false, 2, &mut rng);
        // Interior carved (the x=1 floor line may carry rails, so
        // probe the side lane + the head-height row).
        assert_eq!(w.read([0, 40, 3]), 0);
        assert_eq!(w.read([1, 41, 3]), 0);
        // Some rails landed on the centre line (0.7/0.9 per cell over
        // 10 cells - statistically certain with any seed).
        let rails = (0..10).filter(|z| w.read([1, 40, *z]) == RAIL_ID).count();
        assert!(rails > 0, "rail corridor placed no rails");
        // Floor row stays solid (planks only replace non-sturdy).
        assert_eq!(w.read([1, 39, 3]), ORACLE_SOLID_ID);
    }

    #[test]
    fn spider_corridor_places_spawner_and_webs() {
        let mut w = MapWorld::new(39, 200);
        let p = Piece {
            kind: PieceKind::Corridor { has_rails: false, spider: true, num_sections: 3 },
            bb: Bb::new(0, 40, 0, 2, 42, 14),
            orientation: Some(Dir::South),
        };
        let mut rng = SplitMix64::new(99);
        corridor_post_process(&mut w, &p, ShaftKind::Normal, false, true, 3, &mut rng);
        let spawners = w.cells.values().filter(|id| **id == SPAWNER_ID).count();
        assert_eq!(spawners, 1, "exactly one spawner per spider corridor column");
        let webs = w.cells.values().filter(|id| **id == COBWEB_ID).count();
        assert!(webs > 0, "spider corridor without cobwebs");
    }

    #[test]
    fn support_needs_solid_ceiling() {
        // Ceiling above the corridor open -> no fence supports.
        let mut w = MapWorld::new(39, 200);
        // Poke the ceiling row open above section z=2.
        for x in 0..=2 {
            w.place([x, 43, 2], 0);
        }
        let p = test_corridor(1, Dir::South);
        let mut rng = SplitMix64::new(3);
        corridor_post_process(&mut w, &p, ShaftKind::Normal, false, false, 1, &mut rng);
        assert_eq!(w.read([0, 40, 2]), 0, "no fence when ceiling is open");
        // Solid ceiling -> fences + beam.
        let mut w2 = MapWorld::new(39, 200);
        for x in 0..=2 {
            w2.place([x, 43, 2], ORACLE_SOLID_ID);
        }
        let mut rng2 = SplitMix64::new(3);
        corridor_post_process(&mut w2, &p, ShaftKind::Normal, false, false, 1, &mut rng2);
        assert_eq!(w2.read([0, 40, 2]), OAK_FENCE_ID);
        assert_eq!(w2.read([2, 41, 2]), OAK_FENCE_ID);
        assert_eq!(w2.read([1, 42, 2]), PLANKS_ID, "beam spans the corridor");
    }

    #[test]
    fn liquid_on_hull_aborts_the_piece() {
        let mut w = MapWorld::new(39, 200);
        w.place([1, 43, 3], WATER_ID); // hull cell (y1+1 plane)
        let p = test_corridor(1, Dir::South);
        let mut rng = SplitMix64::new(3);
        corridor_post_process(&mut w, &p, ShaftKind::Normal, false, false, 1, &mut rng);
        assert_eq!(
            w.cells.len(),
            1,
            "flooded piece must not carve (only the water cell exists)"
        );
    }

    #[test]
    fn stairs_carve_descending_steps() {
        let mut w = MapWorld::new(30, 200);
        let p = Piece {
            kind: PieceKind::Stairs,
            bb: Bb::new(0, 35, 0, 2, 42, 8),
            orientation: Some(Dir::South),
        };
        stairs_post_process(&mut w, &p);
        // Top landing + bottom landing carved.
        assert_eq!(w.read([1, 40, 0]), 0);
        assert_eq!(w.read([1, 35, 8]), 0);
    }

    #[test]
    fn layout_is_deterministic_and_bounded() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        // Find a start within a scan window; layouts must replay
        // identically and stay within the +-80 reach clamp.
        let mut found = 0;
        for scx in 0..80 {
            for scz in 0..80 {
                let Some(a) = layout_for_start(&ctx, scx, scz) else {
                    continue;
                };
                found += 1;
                let b = build_layout(&ctx, scx, scz).expect("re-roll agrees");
                assert_eq!(a.pieces.len(), b.pieces.len(), "layout not deterministic");
                let start = a.pieces[0].bb;
                for p in &a.pieces {
                    assert!(
                        (p.bb.x0 - start.x0).abs() <= 95 && (p.bb.z0 - start.z0).abs() <= 95,
                        "piece beyond the 80-block reach clamp: {:?} vs start {:?}",
                        p.bb,
                        start
                    );
                    if a.kind == ShaftKind::Normal {
                        // moveBelowSeaLevel puts the tree TOP at most at
                        // seaLevel - 10; individual crossings poke a
                        // few blocks above their foot piece.
                        assert!(
                            p.bb.y1 <= SEA_LEVEL,
                            "normal shaft above sea level: {:?}",
                            p.bb
                        );
                    }
                }
            }
        }
        assert!(found > 0, "no mineshaft in 80x80 chunks at 0.4%/chunk (p ~ 1e-12)");
    }

    /// Full-pipeline probe: generate the REAL chunks over a laid-out
    /// mineshaft (default: the start nearest the test-mc-9 dungeon)
    /// and slice the first railed corridor - proves the layout +
    /// carve survive `place_from_coord` end to end. Asserts planks,
    /// rails and carved air materialised.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_MS_CX/CZ=<start chunk>
    /// cargo test -p voxel_demo_mp --release --lib
    /// dump_mineshaft_slice -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_mineshaft_slice() {
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
        let (scx, scz) = (env_i32("INFENGINE_MS_CX", 290), env_i32("INFENGINE_MS_CZ", 307));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM").unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let ms = layout_for_start(&ctx, scx, scz).expect("start chunk holds a mineshaft");
        let corridor = ms
            .pieces
            .iter()
            .find(|p| matches!(p.kind, PieceKind::Corridor { has_rails: true, .. }))
            .expect("a railed corridor exists");
        let bb = corridor.bb;
        println!("railed corridor {:?} (orientation {:?})", bb, corridor.orientation);
        // Pull REAL chunks across the corridor's rail plane.
        let cs = CHUNK_SIZE as i32;
        let y = bb.y0;
        let mut planks = 0usize;
        let mut rails = 0usize;
        let mut air = 0usize;
        let mut fences = 0usize;
        for z in (bb.z0 - 1)..=(bb.z1 + 1) {
            let mut line = String::new();
            for x in (bb.x0 - 1)..=(bb.x1 + 1) {
                let coord = [x.div_euclid(cs), y.div_euclid(cs), z.div_euclid(cs)];
                let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
                let id = chunk
                    .get(
                        x.rem_euclid(cs) as usize,
                        y.rem_euclid(cs) as usize,
                        z.rem_euclid(cs) as usize,
                    )
                    .0;
                match id {
                    0 => air += 1,
                    x if x == PLANKS_ID => planks += 1,
                    x if x == RAIL_ID => rails += 1,
                    x if x == OAK_FENCE_ID || x == DARK_OAK_FENCE_ID => fences += 1,
                    _ => {}
                }
                line.push(match id {
                    0 => '.',
                    x if x == RAIL_ID => 'r',
                    x if x == PLANKS_ID => 'p',
                    x if x == OAK_FENCE_ID || x == DARK_OAK_FENCE_ID => 'f',
                    x if x == COBWEB_ID => 'w',
                    x if x == SPAWNER_ID => 'S',
                    x if x == CHEST_ID => 'C',
                    x if x == WATER_ID => 'W',
                    x if x == LAVA_ID => 'L',
                    _ => '#',
                });
            }
            println!("{line}");
        }
        println!(
            "rail plane y={y}: {air} air, {planks} planks, {rails} rails, {fences} fences"
        );
        // Hull liquid scan: any water/lava on the piece hull explains
        // a per-cube abort (the faithful MC `isInInvalidLocation`).
        let mut liquids = 0usize;
        for x in (bb.x0 - 1)..=(bb.x1 + 1) {
            for yy in (bb.y0 - 1)..=(bb.y1 + 1) {
                for z in (bb.z0 - 1)..=(bb.z1 + 1) {
                    let on_hull = x == bb.x0 - 1
                        || x == bb.x1 + 1
                        || yy == bb.y0 - 1
                        || yy == bb.y1 + 1
                        || z == bb.z0 - 1
                        || z == bb.z1 + 1;
                    if !on_hull {
                        continue;
                    }
                    let coord = [x.div_euclid(cs), yy.div_euclid(cs), z.div_euclid(cs)];
                    let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
                    let id = chunk
                        .get(
                            x.rem_euclid(cs) as usize,
                            yy.rem_euclid(cs) as usize,
                            z.rem_euclid(cs) as usize,
                        )
                        .0;
                    if id == WATER_ID || id == LAVA_ID {
                        liquids += 1;
                        if liquids <= 8 {
                            println!(
                                "hull liquid {} at ({x}, {yy}, {z})",
                                if id == WATER_ID { "water" } else { "lava" }
                            );
                        }
                    }
                }
            }
        }
        println!("hull liquid cells: {liquids}");
        assert!(air > 0, "corridor interior did not carve");
        assert!(
            rails > 0,
            "railed corridor materialised no rails through the live pipeline"
        );
    }

    /// The chest-loot + spawner-kind membership probes agree with the
    /// layout geometry (drives abandoned_mineshaft vs simple_dungeon
    /// at delivery and the cave-spider stand-in at tick).
    #[test]
    fn pos_probes_match_layout() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let ms = layout_for_start(&ctx, 290, 307).expect("known start from the dump probe");
        let corridor = ms
            .pieces
            .iter()
            .find(|p| p.is_corridor())
            .expect("shaft has corridors");
        let c = corridor.bb.center();
        assert!(pos_in_mineshaft(&ctx, c));
        assert!(pos_in_mineshaft_corridor(&ctx, c));
        // Above the whole tree: nothing there.
        let top = ms.pieces.iter().map(|p| p.bb.y1).max().unwrap();
        let room = ms.pieces[0].bb;
        let above = [room.x0, top + 2, room.z0];
        assert!(!pos_in_mineshaft(&ctx, above));
        // Classic profiles never claim membership (the stream.rs
        // delivery arm calls this for every worldgen chest).
        let classic = crate::worldgen::WorldgenCtx {
            base_seed: 1,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Classic,
        };
        assert!(!pos_in_mineshaft(&classic, c));
    }

    /// Full-pipeline spider-corridor probe: generate the REAL chunks
    /// over every spider corridor of a laid-out shaft and count the
    /// spawners + cobwebs that materialised.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_MS_CX/CZ=<start chunk>
    /// cargo test -p voxel_demo_mp --release --lib
    /// dump_mineshaft_spiders -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_mineshaft_spiders() {
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
        let (scx, scz) = (env_i32("INFENGINE_MS_CX", 287), env_i32("INFENGINE_MS_CZ", 313));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM").unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let ms = layout_for_start(&ctx, scx, scz).expect("start chunk holds a mineshaft");
        let cs = CHUNK_SIZE as i32;
        let mut total_spawners = 0usize;
        let mut total_webs = 0usize;
        let mut total_chests = 0usize;
        for piece in ms.pieces.iter().filter(|p| {
            matches!(p.kind, PieceKind::Corridor { spider: true, .. })
        }) {
            let bb = piece.bb;
            let mut spawners = 0usize;
            let mut webs = 0usize;
            for x in bb.x0..=bb.x1 {
                for y in bb.y0..=bb.y1 {
                    for z in bb.z0..=bb.z1 {
                        let coord =
                            [x.div_euclid(cs), y.div_euclid(cs), z.div_euclid(cs)];
                        let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
                        match chunk
                            .get(
                                x.rem_euclid(cs) as usize,
                                y.rem_euclid(cs) as usize,
                                z.rem_euclid(cs) as usize,
                            )
                            .0
                        {
                            b if b == SPAWNER_ID => {
                                spawners += 1;
                                println!("  spawner at ({x}, {y}, {z})");
                            }
                            b if b == COBWEB_ID => webs += 1,
                            b if b == CHEST_ID => total_chests += 1,
                            _ => {}
                        }
                    }
                }
            }
            println!(
                "spider corridor {:?}: {spawners} spawner(s), {webs} cobwebs",
                bb
            );
            total_spawners += spawners;
            total_webs += webs;
        }
        println!(
            "shaft ({scx}, {scz}): {total_spawners} spawners, {total_webs} webs, \
             {total_chests} chests in spider corridors"
        );
        assert!(
            total_spawners > 0,
            "no spider corridor materialised a spawner through the live pipeline"
        );
        assert!(total_webs > 0, "spider corridors without cobwebs");
    }

    /// Probe: print mineshaft starts near a seed/centre so a live
    /// session can /tppos to one.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp --release
    /// --lib dump_mineshafts -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_mineshafts() {
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
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 24);
        let (ccx, ccz) = (env_i32("INFENGINE_DUMP_CX", 0), env_i32("INFENGINE_DUMP_CZ", 0));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM").unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let mut found = 0;
        for scx in (ccx - radius)..=(ccx + radius) {
            for scz in (ccz - radius)..=(ccz + radius) {
                let Some(ms) = layout_for_start(&ctx, scx, scz) else {
                    continue;
                };
                found += 1;
                let room = ms.pieces[0].bb;
                let corridors = ms.pieces.iter().filter(|p| p.is_corridor()).count();
                let spiders = ms
                    .pieces
                    .iter()
                    .filter(|p| matches!(p.kind, PieceKind::Corridor { spider: true, .. }))
                    .count();
                println!(
                    "mineshaft ({:?}) start chunk ({scx}, {scz}): room {:?}, {} pieces \
                     ({corridors} corridors, {spiders} spider), y {}..{}",
                    ms.kind,
                    room,
                    ms.pieces.len(),
                    ms.pieces.iter().map(|p| p.bb.y0).min().unwrap(),
                    ms.pieces.iter().map(|p| p.bb.y1).max().unwrap(),
                );
            }
        }
        println!("total: {found} mineshafts in +/-{radius} chunks around ({ccx}, {ccz})");
    }
}
