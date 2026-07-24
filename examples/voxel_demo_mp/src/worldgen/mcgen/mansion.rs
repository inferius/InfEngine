//! W259 - woodland mansion (`WoodlandMansionStructure` +
//! `WoodlandMansionPieces.java` port) - the illager estate, built
//! from 73 NBT templates over a procedurally traced 11x11x3 grid.
//!
//! Placement (`structure_set/woodland_mansions.json`): TRIANGULAR
//! random_spread spacing 80 / separation 20 / salt 10387319,
//! dark-forest start biome; the anchor Y is the LOWEST surface in the
//! mansion's footprint area (16-block lattice stand-in for MC's 5x5
//! corner sample), rejected below y 60.
//!
//! Layout is the verbatim MansionGrid: the entrance stamp, four
//! recursive corridor walks, the cleanEdges fixpoint, room
//! identification (2x2 > 1x2 > 1x1 with the door-corner dance), the
//! third floor grown from a random door-flagged 1x2 of floor two.
//! The MansionPiecePlacer then emits template pieces: the entrance,
//! the outer-wall traversal of floors one and two, the third-floor
//! wall ring, both roofs (tiles, fronts, corners, inner corners,
//! small walls), corridor floors + carpets, indoor walls/doors, and
//! the FloorRoomCollection room picks (1x1 / secret / 1x2 side +
//! front + stairs / 2x2 + secret) with their rotation/mirror tables.
//!
//! Templates place with pivot-ZERO rotations + LEFT_RIGHT/FRONT_BACK
//! mirrors (coordinate negation, exactly `StructureTemplate
//! .transform`); air cells DO write (STRUCTURE_BLOCK-only ignore -
//! that is how rooms carve their interiors into the hill). DATA
//! markers: `Chest*` becomes the WOODLAND_MANSION loot chest (facing
//! drops to the engine default); `Mage` / `Warrior` /
//! `Group of Allays` wait for the evoker/vindicator/allay mobs
//! (once-logged, the cell opens to air like MC after the spawn).
//! `afterPlace` grows the cobblestone foundation under every piece
//! column.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::{Mapped, map_state};
use super::features::stream_seed;
use super::mineshaft::Bb;
use super::noise::SplitMix64;
use super::terrain::{column_is_solid, mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;

/// `structure_set/woodland_mansions.json`.
const SPACING: i32 = 80;
const SEPARATION: i32 = 20;
const SALT_MANSION_SET: u64 = 10_387_319;
const SALT_MANSION_LAYOUT: u64 = 0xD4E6_00D0;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_bool(r: &mut SplitMix64) -> bool {
    r.next_u64() & 1 == 1
}

fn triangular_start(seed: u32, gx: i32, gz: i32) -> [i32; 2] {
    let mut rng = SplitMix64::new(stream_seed(seed, gx, gz, SALT_MANSION_SET));
    let range = (SPACING - SEPARATION).max(1) as u64;
    let ox = ((rng.next_u64() % range) + (rng.next_u64() % range)) as i32 / 2;
    let oz = ((rng.next_u64() % range) + (rng.next_u64() % range)) as i32 / 2;
    [gx * SPACING + ox, gz * SPACING + oz]
}

// ----------------------------------------------------------------
// Directions + rotations (grid space; the placer works in N/S/W/E).
// ----------------------------------------------------------------

/// 0 = north (-z), 1 = south (+z), 2 = west (-x), 3 = east (+x).
const N: u8 = 0;
const S: u8 = 1;
const W: u8 = 2;
const E: u8 = 3;

fn step(d: u8) -> [i32; 2] {
    match d {
        N => [0, -1],
        S => [0, 1],
        W => [-1, 0],
        _ => [1, 0],
    }
}

fn opposite(d: u8) -> u8 {
    match d {
        N => S,
        S => N,
        W => E,
        _ => W,
    }
}

fn clockwise(d: u8) -> u8 {
    match d {
        N => E,
        E => S,
        S => W,
        _ => N,
    }
}

fn counter_clockwise(d: u8) -> u8 {
    clockwise(clockwise(clockwise(d)))
}

/// `Direction.from2DDataValue`: 0=S, 1=W, 2=N, 3=E.
fn from_2d(v: i32) -> u8 {
    match v & 3 {
        0 => S,
        1 => W,
        2 => N,
        _ => E,
    }
}

/// Rotate a direction by `rot` clockwise quarter turns.
fn rot_dir(rot: u8, d: u8) -> u8 {
    let mut out = d;
    for _ in 0..(rot % 4) {
        out = clockwise(out);
    }
    out
}

/// Pivot-zero rotation of a position vector (matches `rot_pos`).
fn rot_vec(rot: u8, p: [i32; 3]) -> [i32; 3] {
    match rot % 4 {
        0 => p,
        1 => [-p[2], p[1], p[0]],
        2 => [-p[0], p[1], -p[2]],
        _ => [p[2], p[1], -p[0]],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MMirror {
    None,
    LeftRight,
    FrontBack,
}

/// `StructureTemplate.transform` with pivot ZERO: the mirror negates
/// one axis, the rotation turns the (possibly negative) vector.
fn transform(p: [i32; 3], rot: u8, mirror: MMirror) -> [i32; 3] {
    let m = match mirror {
        MMirror::None => p,
        MMirror::LeftRight => [p[0], p[1], -p[2]],
        MMirror::FrontBack => [-p[0], p[1], p[2]],
    };
    rot_vec(rot, m)
}

/// `StructureTemplate.getZeroPositionWithTransform` (7x7 rooms use
/// this to re-anchor their rotated footprint).
fn zero_position(p: [i32; 3], mirror: MMirror, rot: u8, sx: i32, sz: i32) -> [i32; 3] {
    let (sx, sz) = (sx - 1, sz - 1);
    let i = if mirror == MMirror::FrontBack { sx } else { 0 };
    let j = if mirror == MMirror::LeftRight { sz } else { 0 };
    match rot % 4 {
        3 => [p[0] + j, p[1], p[2] + sx - i],       // CCW90
        1 => [p[0] + sz - j, p[1], p[2] + i],       // CW90
        2 => [p[0] + sx - i, p[1], p[2] + sz - j],  // 180
        _ => [p[0] + i, p[1], p[2] + j],            // NONE
    }
}

// ----------------------------------------------------------------
// SimpleGrid + MansionGrid (verbatim).
// ----------------------------------------------------------------

struct SimpleGrid {
    grid: Vec<i32>,
    width: i32,
    height: i32,
    outside: i32,
}

impl SimpleGrid {
    fn new(width: i32, height: i32, outside: i32) -> SimpleGrid {
        SimpleGrid { grid: vec![0; (width * height) as usize], width, height, outside }
    }

    fn get(&self, x: i32, y: i32) -> i32 {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.grid[(x * self.height + y) as usize]
        } else {
            self.outside
        }
    }

    fn set(&mut self, x: i32, y: i32, v: i32) {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.grid[(x * self.height + y) as usize] = v;
        }
    }

    fn set_box(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, v);
            }
        }
    }

    fn set_if(&mut self, x: i32, y: i32, expect: i32, v: i32) {
        if self.get(x, y) == expect {
            self.set(x, y, v);
        }
    }

    fn edges_to(&self, x: i32, y: i32, v: i32) -> bool {
        self.get(x - 1, y) == v
            || self.get(x + 1, y) == v
            || self.get(x, y + 1) == v
            || self.get(x, y - 1) == v
    }
}

const ROOM_1X1: i32 = 65536;
const ROOM_1X2: i32 = 131072;
const ROOM_2X2: i32 = 262144;
const ORIGIN_FLAG: i32 = 1048576;
const DOOR_FLAG: i32 = 2097152;
const STAIRS_FLAG: i32 = 4194304;
const CORRIDOR_FLAG: i32 = 8388608;
const TYPE_MASK: i32 = 983040;
const ID_MASK: i32 = 65535;

struct MansionGrid {
    base: SimpleGrid,
    third: SimpleGrid,
    floor_rooms: [SimpleGrid; 3],
    entrance_x: i32,
    entrance_y: i32,
}

fn is_house(grid: &SimpleGrid, x: i32, y: i32) -> bool {
    matches!(grid.get(x, y), 1..=4)
}

impl MansionGrid {
    fn new(rng: &mut SplitMix64) -> MansionGrid {
        let (ex, ey) = (7, 4);
        let mut base = SimpleGrid::new(11, 11, 5);
        base.set_box(ex, ey, ex + 1, ey + 1, 3);
        base.set_box(ex - 1, ey, ex - 1, ey + 1, 2);
        base.set_box(ex + 2, ey - 2, ex + 3, ey + 3, 5);
        base.set_box(ex + 1, ey - 2, ex + 1, ey - 1, 1);
        base.set_box(ex + 1, ey + 2, ex + 1, ey + 3, 1);
        base.set(ex - 1, ey - 1, 1);
        base.set(ex - 1, ey + 2, 1);
        base.set_box(0, 0, 11, 1, 5);
        base.set_box(0, 9, 11, 11, 5);
        Self::recursive_corridor(&mut base, rng, ex, ey - 2, W, 6);
        Self::recursive_corridor(&mut base, rng, ex, ey + 3, W, 6);
        Self::recursive_corridor(&mut base, rng, ex - 2, ey - 1, W, 3);
        Self::recursive_corridor(&mut base, rng, ex - 2, ey + 2, W, 3);
        while Self::clean_edges(&mut base) {}
        let mut floor_rooms = [
            SimpleGrid::new(11, 11, 5),
            SimpleGrid::new(11, 11, 5),
            SimpleGrid::new(11, 11, 5),
        ];
        Self::identify_rooms(rng, &base, &mut floor_rooms[0]);
        Self::identify_rooms(rng, &base, &mut floor_rooms[1]);
        floor_rooms[0].set_box(ex + 1, ey, ex + 1, ey + 1, CORRIDOR_FLAG);
        floor_rooms[1].set_box(ex + 1, ey, ex + 1, ey + 1, CORRIDOR_FLAG);
        let mut third = SimpleGrid::new(base.width, base.height, 5);
        let mut grid = MansionGrid { base, third, floor_rooms, entrance_x: ex, entrance_y: ey };
        grid.setup_third_floor(rng);
        third = std::mem::replace(&mut grid.third, SimpleGrid::new(1, 1, 5));
        Self::identify_rooms(rng, &third, &mut grid.floor_rooms[2]);
        grid.third = third;
        grid
    }

    fn recursive_corridor(
        grid: &mut SimpleGrid,
        rng: &mut SplitMix64,
        x: i32,
        y: i32,
        heading: u8,
        depth: i32,
    ) {
        if depth <= 0 {
            return;
        }
        grid.set(x, y, 1);
        let h = step(heading);
        grid.set_if(x + h[0], y + h[1], 0, 1);
        for _ in 0..8 {
            let next = from_2d(next_int(rng, 4));
            if next == opposite(heading) || (next == E && next_bool(rng)) {
                continue;
            }
            let (nx, ny) = (x + h[0], y + h[1]);
            let n = step(next);
            if grid.get(nx + n[0], ny + n[1]) == 0
                && grid.get(nx + n[0] * 2, ny + n[1] * 2) == 0
            {
                Self::recursive_corridor(
                    grid,
                    rng,
                    x + h[0] + n[0],
                    y + h[1] + n[1],
                    next,
                    depth - 1,
                );
                break;
            }
        }
        let cw = step(clockwise(heading));
        let ccw = step(counter_clockwise(heading));
        grid.set_if(x + cw[0], y + cw[1], 0, 2);
        grid.set_if(x + ccw[0], y + ccw[1], 0, 2);
        grid.set_if(x + h[0] + cw[0], y + h[1] + cw[1], 0, 2);
        grid.set_if(x + h[0] + ccw[0], y + h[1] + ccw[1], 0, 2);
        grid.set_if(x + h[0] * 2, y + h[1] * 2, 0, 2);
        grid.set_if(x + cw[0] * 2, y + cw[1] * 2, 0, 2);
        grid.set_if(x + ccw[0] * 2, y + ccw[1] * 2, 0, 2);
    }

    fn clean_edges(grid: &mut SimpleGrid) -> bool {
        let mut touched = false;
        for y in 0..grid.height {
            for x in 0..grid.width {
                if grid.get(x, y) != 0 {
                    continue;
                }
                let direct = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .filter(|(dx, dy)| is_house(grid, x + dx, y + dy))
                    .count();
                if direct >= 3 {
                    grid.set(x, y, 2);
                    touched = true;
                } else if direct == 2 {
                    let diag = [(1, 1), (-1, 1), (1, -1), (-1, -1)]
                        .iter()
                        .filter(|(dx, dy)| is_house(grid, x + dx, y + dy))
                        .count();
                    if diag <= 1 {
                        grid.set(x, y, 2);
                        touched = true;
                    }
                }
            }
        }
        touched
    }

    fn setup_third_floor(&mut self, rng: &mut SplitMix64) {
        let mut potential: Vec<(i32, i32)> = Vec::new();
        for y in 0..self.third.height {
            for x in 0..self.third.width {
                let data = self.floor_rooms[1].get(x, y);
                if data & TYPE_MASK == ROOM_1X2 && data & DOOR_FLAG == DOOR_FLAG {
                    potential.push((x, y));
                }
            }
        }
        if potential.is_empty() {
            self.third.set_box(0, 0, self.third.width, self.third.height, 5);
            return;
        }
        let (rx, ry) = potential[next_int(rng, potential.len() as i32) as usize];
        let data = self.floor_rooms[1].get(rx, ry);
        self.floor_rooms[1].set(rx, ry, data | STAIRS_FLAG);
        let room_id = data & ID_MASK;
        let mut dir = None;
        for d in [N, S, W, E] {
            let s = step(d);
            if self.floor_rooms[1].get(rx + s[0], ry + s[1]) & ID_MASK == room_id {
                dir = Some(d);
                break;
            }
        }
        let Some(dir) = dir else {
            self.third.set_box(0, 0, self.third.width, self.third.height, 5);
            self.floor_rooms[1].set(rx, ry, data);
            return;
        };
        let s = step(dir);
        let (endx, endy) = (rx + s[0], ry + s[1]);
        for y in 0..self.third.height {
            for x in 0..self.third.width {
                if !is_house(&self.base, x, y) {
                    self.third.set(x, y, 5);
                } else if x == rx && y == ry {
                    self.third.set(x, y, 3);
                } else if x == endx && y == endy {
                    self.third.set(x, y, 3);
                    self.floor_rooms[2].set(x, y, CORRIDOR_FLAG);
                }
            }
        }
        let mut corridors: Vec<u8> = Vec::new();
        for d in [N, S, W, E] {
            let s = step(d);
            if self.third.get(endx + s[0], endy + s[1]) == 0 {
                corridors.push(d);
            }
        }
        if corridors.is_empty() {
            self.third.set_box(0, 0, self.third.width, self.third.height, 5);
            self.floor_rooms[1].set(rx, ry, data);
            return;
        }
        let cd = corridors[next_int(rng, corridors.len() as i32) as usize];
        let cs = step(cd);
        Self::recursive_corridor(&mut self.third, rng, endx + cs[0], endy + cs[1], cd, 4);
        while Self::clean_edges(&mut self.third) {}
    }

    fn identify_rooms(rng: &mut SplitMix64, from: &SimpleGrid, rooms: &mut SimpleGrid) {
        let mut pos: Vec<(i32, i32)> = Vec::new();
        for y in 0..from.height {
            for x in 0..from.width {
                if from.get(x, y) == 2 {
                    pos.push((x, y));
                }
            }
        }
        // Util.shuffle.
        for i in (1..pos.len()).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            pos.swap(i, j);
        }
        let mut room_id = 10;
        for (x, y) in pos {
            if rooms.get(x, y) != 0 {
                continue;
            }
            let (mut x0, mut x1, mut y0, mut y1) = (x, x, y, y);
            let mut ty = ROOM_1X1;
            if rooms.get(x + 1, y) == 0
                && rooms.get(x, y + 1) == 0
                && rooms.get(x + 1, y + 1) == 0
                && from.get(x + 1, y) == 2
                && from.get(x, y + 1) == 2
                && from.get(x + 1, y + 1) == 2
            {
                x1 += 1;
                y1 += 1;
                ty = ROOM_2X2;
            } else if rooms.get(x - 1, y) == 0
                && rooms.get(x, y + 1) == 0
                && rooms.get(x - 1, y + 1) == 0
                && from.get(x - 1, y) == 2
                && from.get(x, y + 1) == 2
                && from.get(x - 1, y + 1) == 2
            {
                x0 -= 1;
                y1 += 1;
                ty = ROOM_2X2;
            } else if rooms.get(x - 1, y) == 0
                && rooms.get(x, y - 1) == 0
                && rooms.get(x - 1, y - 1) == 0
                && from.get(x - 1, y) == 2
                && from.get(x, y - 1) == 2
                && from.get(x - 1, y - 1) == 2
            {
                x0 -= 1;
                y0 -= 1;
                ty = ROOM_2X2;
            } else if rooms.get(x + 1, y) == 0 && from.get(x + 1, y) == 2 {
                x1 += 1;
                ty = ROOM_1X2;
            } else if rooms.get(x, y + 1) == 0 && from.get(x, y + 1) == 2 {
                y1 += 1;
                ty = ROOM_1X2;
            } else if rooms.get(x - 1, y) == 0 && from.get(x - 1, y) == 2 {
                x0 -= 1;
                ty = ROOM_1X2;
            } else if rooms.get(x, y - 1) == 0 && from.get(x, y - 1) == 2 {
                y0 -= 1;
                ty = ROOM_1X2;
            }
            let mut dx = if next_bool(rng) { x0 } else { x1 };
            let mut dy = if next_bool(rng) { y0 } else { y1 };
            let mut door = DOOR_FLAG;
            if !from.edges_to(dx, dy, 1) {
                dx = if dx == x0 { x1 } else { x0 };
                dy = if dy == y0 { y1 } else { y0 };
                if !from.edges_to(dx, dy, 1) {
                    dy = if dy == y0 { y1 } else { y0 };
                    if !from.edges_to(dx, dy, 1) {
                        dx = if dx == x0 { x1 } else { x0 };
                        dy = if dy == y0 { y1 } else { y0 };
                        if !from.edges_to(dx, dy, 1) {
                            door = 0;
                            dx = x0;
                            dy = y0;
                        }
                    }
                }
            }
            for ry in y0..=y1 {
                for rx in x0..=x1 {
                    if rx == dx && ry == dy {
                        rooms.set(rx, ry, ORIGIN_FLAG | door | ty | room_id);
                    } else {
                        rooms.set(rx, ry, ty | room_id);
                    }
                }
            }
            room_id += 1;
        }
    }

    fn is_room_id(&self, floor: usize, x: i32, y: i32, room_id: i32) -> bool {
        self.floor_rooms[floor].get(x, y) & ID_MASK == room_id
    }

    fn room_direction(&self, floor: usize, x: i32, y: i32, room_id: i32) -> Option<u8> {
        for d in [N, S, W, E] {
            let s = step(d);
            if self.is_room_id(floor, x + s[0], y + s[1], room_id) {
                return Some(d);
            }
        }
        None
    }
}

// ----------------------------------------------------------------
// Piece placer.
// ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct MansionTemplate {
    pub(crate) name: String,
    pub(crate) pos: [i32; 3],
    pub(crate) rot: u8,
    pub(crate) mirror: MMirror,
}

/// `pos.relative(rotation.rotate(dir), n)`.
fn rel(pos: [i32; 3], rot: u8, dir: u8, n: i32) -> [i32; 3] {
    let d = step(rot_dir(rot, dir));
    [pos[0] + d[0] * n, pos[1], pos[2] + d[1] * n]
}

struct Placer<'r> {
    rng: &'r mut SplitMix64,
    start_x: i32,
    start_y: i32,
    pieces: Vec<MansionTemplate>,
}

impl Placer<'_> {
    fn add(&mut self, name: impl Into<String>, pos: [i32; 3], rot: u8, mirror: MMirror) {
        self.pieces.push(MansionTemplate { name: name.into(), pos, rot, mirror });
    }

    fn create(&mut self, origin: [i32; 3], rotation: u8, grid: &MansionGrid) {
        struct Data {
            position: [i32; 3],
            rotation: u8,
            wall: &'static str,
        }
        let mut data = Data { position: origin, rotation, wall: "wall_flat" };
        // entrance().
        self.add("entrance", rel(data.position, rotation, W, 9), rotation, MMirror::None);
        data.position = rel(data.position, rotation, S, 16);
        let mut second = Data {
            position: [data.position[0], data.position[1] + 8, data.position[2]],
            rotation,
            wall: "wall_window",
        };
        self.start_x = grid.entrance_x + 1;
        self.start_y = grid.entrance_y + 1;
        let (endx, endy) = (grid.entrance_x + 1, grid.entrance_y);
        let (sx, sy) = (self.start_x, self.start_y);
        self.traverse_outer(
            &mut data.position,
            &mut data.rotation,
            data.wall,
            &grid.base,
            sx,
            sy,
            endx,
            endy,
        );
        self.traverse_outer(
            &mut second.position,
            &mut second.rotation,
            second.wall,
            &grid.base,
            sx,
            sy,
            endx,
            endy,
        );
        // The third-floor wall ring starts at its first house cell.
        let mut third_pos = [origin[0], origin[1] + 19, origin[2]];
        'third: for y in 0..grid.third.height {
            for x in (0..grid.third.width).rev() {
                if is_house(&grid.third, x, y) {
                    third_pos = rel(third_pos, rotation, S, 8 + (y - self.start_y) * 8);
                    third_pos = rel(third_pos, rotation, E, (x - self.start_x) * 8);
                    let mut rot3 = rotation;
                    self.wall_piece(&mut third_pos, rot3, "wall_window");
                    self.traverse_outer(
                        &mut third_pos,
                        &mut rot3,
                        "wall_window",
                        &grid.third,
                        x,
                        y,
                        x,
                        y,
                    );
                    break 'third;
                }
            }
        }
        self.create_roof(
            [origin[0], origin[1] + 16, origin[2]],
            rotation,
            &grid.base,
            Some(&grid.third),
        );
        self.create_roof([origin[0], origin[1] + 27, origin[2]], rotation, &grid.third, None);
        // Rooms per floor.
        for floor in 0..3usize {
            let floor_origin = [
                origin[0],
                origin[1] + 8 * floor as i32 + if floor == 2 { 3 } else { 0 },
                origin[2],
            ];
            let rooms = &grid.floor_rooms[floor];
            let g = if floor == 2 { &grid.third } else { &grid.base };
            let south_piece = if floor == 0 { "carpet_south_1" } else { "carpet_south_2" };
            let west_piece = if floor == 0 { "carpet_west_1" } else { "carpet_west_2" };
            for y in 0..g.height {
                for x in 0..g.width {
                    if g.get(x, y) != 1 {
                        continue;
                    }
                    let mut pos =
                        rel(floor_origin, rotation, S, 8 + (y - self.start_y) * 8);
                    pos = rel(pos, rotation, E, (x - self.start_x) * 8);
                    self.add("corridor_floor", pos, rotation, MMirror::None);
                    if g.get(x, y - 1) == 1
                        || rooms.get(x, y - 1) & CORRIDOR_FLAG == CORRIDOR_FLAG
                    {
                        let mut p = rel(pos, rotation, E, 1);
                        p[1] += 1;
                        self.add("carpet_north", p, rotation, MMirror::None);
                    }
                    if g.get(x + 1, y) == 1
                        || rooms.get(x + 1, y) & CORRIDOR_FLAG == CORRIDOR_FLAG
                    {
                        let mut p = rel(rel(pos, rotation, S, 1), rotation, E, 5);
                        p[1] += 1;
                        self.add("carpet_east", p, rotation, MMirror::None);
                    }
                    if g.get(x, y + 1) == 1
                        || rooms.get(x, y + 1) & CORRIDOR_FLAG == CORRIDOR_FLAG
                    {
                        let p = rel(rel(pos, rotation, S, 5), rotation, W, 1);
                        self.add(south_piece, p, rotation, MMirror::None);
                    }
                    if g.get(x - 1, y) == 1
                        || rooms.get(x - 1, y) & CORRIDOR_FLAG == CORRIDOR_FLAG
                    {
                        let p = rel(rel(pos, rotation, W, 1), rotation, N, 1);
                        self.add(west_piece, p, rotation, MMirror::None);
                    }
                }
            }
            let wall_piece = if floor == 0 { "indoors_wall_1" } else { "indoors_wall_2" };
            let door_piece = if floor == 0 { "indoors_door_1" } else { "indoors_door_2" };
            for y in 0..g.height {
                for x in 0..g.width {
                    let third_start = floor == 2 && g.get(x, y) == 3;
                    if g.get(x, y) != 2 && !third_start {
                        continue;
                    }
                    let data_v = rooms.get(x, y);
                    let ty = data_v & TYPE_MASK;
                    let id = data_v & ID_MASK;
                    let third_start = third_start && data_v & CORRIDOR_FLAG == CORRIDOR_FLAG;
                    // Door direction.
                    let mut door_dirs: Vec<u8> = Vec::new();
                    if data_v & DOOR_FLAG == DOOR_FLAG {
                        for d in [N, S, W, E] {
                            let s = step(d);
                            if g.get(x + s[0], y + s[1]) == 1 {
                                door_dirs.push(d);
                            }
                        }
                    }
                    // 255 = UP (the secret-room marker).
                    let door: Option<u8> = if !door_dirs.is_empty() {
                        Some(door_dirs[next_int(self.rng, door_dirs.len() as i32) as usize])
                    } else if data_v & ORIGIN_FLAG == ORIGIN_FLAG {
                        Some(255)
                    } else {
                        None
                    };
                    let mut room_pos =
                        rel(floor_origin, rotation, S, 8 + (y - self.start_y) * 8);
                    room_pos = rel(room_pos, rotation, E, -1 + (x - self.start_x) * 8);
                    if is_house(g, x - 1, y) && !grid.is_room_id(floor, x - 1, y, id) {
                        let name = if door == Some(W) { door_piece } else { wall_piece };
                        self.add(name, room_pos, rotation, MMirror::None);
                    }
                    if g.get(x + 1, y) == 1 && !third_start {
                        let p = rel(room_pos, rotation, E, 8);
                        let name = if door == Some(E) { door_piece } else { wall_piece };
                        self.add(name, p, rotation, MMirror::None);
                    }
                    if is_house(g, x, y + 1) && !grid.is_room_id(floor, x, y + 1, id) {
                        let p = rel(rel(room_pos, rotation, S, 7), rotation, E, 7);
                        let name = if door == Some(S) { door_piece } else { wall_piece };
                        self.add(name, p, (rotation + 1) % 4, MMirror::None);
                    }
                    if g.get(x, y - 1) == 1 && !third_start {
                        let p = rel(rel(room_pos, rotation, N, 1), rotation, E, 7);
                        let name = if door == Some(N) { door_piece } else { wall_piece };
                        self.add(name, p, (rotation + 1) % 4, MMirror::None);
                    }
                    if ty == ROOM_1X1 {
                        self.room_1x1(room_pos, rotation, door, floor);
                    } else if ty == ROOM_1X2 && door.is_some() {
                        let room_dir = grid.room_direction(floor, x, y, id);
                        let stairs = data_v & STAIRS_FLAG == STAIRS_FLAG;
                        if let Some(rd) = room_dir {
                            self.room_1x2(room_pos, rotation, rd, door.unwrap(), floor, stairs);
                        }
                    } else if ty == ROOM_2X2
                        && let Some(d) = door
                        && d != 255
                    {
                        let mut rd = clockwise(d);
                        let s = step(rd);
                        if !grid.is_room_id(floor, x + s[0], y + s[1], id) {
                            rd = opposite(rd);
                        }
                        self.room_2x2(room_pos, rotation, rd, d, floor);
                    } else if ty == ROOM_2X2 && door == Some(255) {
                        let p = rel(room_pos, rotation, E, 1);
                        self.add(room_2x2_secret(floor), p, rotation, MMirror::None);
                    }
                }
            }
        }
    }

    fn wall_piece(&mut self, position: &mut [i32; 3], rotation: u8, wall: &str) {
        let p = rel(*position, rotation, E, 7);
        self.add(wall.to_string(), p, rotation, MMirror::None);
        *position = rel(*position, rotation, S, 8);
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse_outer(
        &mut self,
        position: &mut [i32; 3],
        rotation: &mut u8,
        wall: &str,
        grid: &SimpleGrid,
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
    ) {
        let mut gx = start_x;
        let mut gy = start_y;
        let mut dir = S;
        let start_dir = dir;
        // The Java do-while always closes the wall loop; the cap is
        // a worker-hang guard should a port bug ever break that.
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 512 {
                tracing::warn!("W259: mansion wall traversal guard tripped");
                break;
            }
            let s = step(dir);
            if !is_house(grid, gx + s[0], gy + s[1]) {
                // traverseTurn.
                *position = rel(*position, *rotation, S, -1);
                self.add("wall_corner", *position, *rotation, MMirror::None);
                *position = rel(*position, *rotation, S, -7);
                *position = rel(*position, *rotation, W, -6);
                *rotation = (*rotation + 1) % 4;
                dir = clockwise(dir);
                if gx != end_x || gy != end_y || start_dir != dir {
                    self.wall_piece(position, *rotation, wall);
                }
            } else {
                let ccw = step(counter_clockwise(dir));
                if is_house(grid, gx + s[0] + ccw[0], gy + s[1] + ccw[1]) {
                    // traverseInnerTurn.
                    *position = rel(*position, *rotation, S, 6);
                    *position = rel(*position, *rotation, E, 8);
                    *rotation = (*rotation + 3) % 4;
                    gx += s[0];
                    gy += s[1];
                    dir = counter_clockwise(dir);
                } else {
                    gx += s[0];
                    gy += s[1];
                    if gx != end_x || gy != end_y || start_dir != dir {
                        self.wall_piece(position, *rotation, wall);
                    }
                }
            }
            if gx == end_x && gy == end_y && start_dir == dir {
                break;
            }
        }
    }

    fn create_roof(
        &mut self,
        roof_origin: [i32; 3],
        rotation: u8,
        grid: &SimpleGrid,
        above: Option<&SimpleGrid>,
    ) {
        let at = |x: i32, y: i32, base: [i32; 3], placer: &Placer| {
            let p = rel(base, rotation, S, 8 + (y - placer.start_y) * 8);
            rel(p, rotation, E, (x - placer.start_x) * 8)
        };
        for y in 0..grid.height {
            for x in 0..grid.width {
                let position = at(x, y, roof_origin, self);
                let is_above = above.is_some_and(|a| is_house(a, x, y));
                if !is_house(grid, x, y) || is_above {
                    continue;
                }
                let mut p = position;
                p[1] += 3;
                self.add("roof", p, rotation, MMirror::None);
                if !is_house(grid, x + 1, y) {
                    let p2 = rel(position, rotation, E, 6);
                    self.add("roof_front", p2, rotation, MMirror::None);
                }
                if !is_house(grid, x - 1, y) {
                    let p2 = rel(rel(position, rotation, E, 0), rotation, S, 7);
                    self.add("roof_front", p2, (rotation + 2) % 4, MMirror::None);
                }
                if !is_house(grid, x, y - 1) {
                    let p2 = rel(position, rotation, W, 1);
                    self.add("roof_front", p2, (rotation + 3) % 4, MMirror::None);
                }
                if !is_house(grid, x, y + 1) {
                    let p2 = rel(rel(position, rotation, E, 6), rotation, S, 6);
                    self.add("roof_front", p2, (rotation + 1) % 4, MMirror::None);
                }
            }
        }
        if let Some(above) = above {
            for y in 0..grid.height {
                for x in 0..grid.width {
                    let position = at(x, y, roof_origin, self);
                    if !is_house(grid, x, y) || !is_house(above, x, y) {
                        continue;
                    }
                    if !is_house(grid, x + 1, y) {
                        let p = rel(position, rotation, E, 7);
                        self.add("small_wall", p, rotation, MMirror::None);
                    }
                    if !is_house(grid, x - 1, y) {
                        let p = rel(rel(position, rotation, W, 1), rotation, S, 6);
                        self.add("small_wall", p, (rotation + 2) % 4, MMirror::None);
                    }
                    if !is_house(grid, x, y - 1) {
                        let p = rel(rel(position, rotation, W, 0), rotation, N, 1);
                        self.add("small_wall", p, (rotation + 3) % 4, MMirror::None);
                    }
                    if !is_house(grid, x, y + 1) {
                        let p = rel(rel(position, rotation, E, 6), rotation, S, 7);
                        self.add("small_wall", p, (rotation + 1) % 4, MMirror::None);
                    }
                    if !is_house(grid, x + 1, y) {
                        if !is_house(grid, x, y - 1) {
                            let p = rel(rel(position, rotation, E, 7), rotation, N, 2);
                            self.add("small_wall_corner", p, rotation, MMirror::None);
                        }
                        if !is_house(grid, x, y + 1) {
                            let p = rel(rel(position, rotation, E, 8), rotation, S, 7);
                            self.add("small_wall_corner", p, (rotation + 1) % 4, MMirror::None);
                        }
                    }
                    if !is_house(grid, x - 1, y) {
                        if !is_house(grid, x, y - 1) {
                            let p = rel(rel(position, rotation, W, 2), rotation, N, 1);
                            self.add("small_wall_corner", p, (rotation + 3) % 4, MMirror::None);
                        }
                        if !is_house(grid, x, y + 1) {
                            let p = rel(rel(position, rotation, W, 1), rotation, S, 8);
                            self.add("small_wall_corner", p, (rotation + 2) % 4, MMirror::None);
                        }
                    }
                }
            }
        }
        for y in 0..grid.height {
            for x in 0..grid.width {
                let position = at(x, y, roof_origin, self);
                let is_above = above.is_some_and(|a| is_house(a, x, y));
                if !is_house(grid, x, y) || is_above {
                    continue;
                }
                if !is_house(grid, x + 1, y) {
                    let p2 = rel(position, rotation, E, 6);
                    if !is_house(grid, x, y + 1) {
                        let p3 = rel(p2, rotation, S, 6);
                        self.add("roof_corner", p3, rotation, MMirror::None);
                    } else if is_house(grid, x + 1, y + 1) {
                        let p3 = rel(p2, rotation, S, 5);
                        self.add("roof_inner_corner", p3, rotation, MMirror::None);
                    }
                    if !is_house(grid, x, y - 1) {
                        let p3 = rel(p2, rotation, N, 0);
                        self.add("roof_corner", p3, (rotation + 3) % 4, MMirror::None);
                    } else if is_house(grid, x + 1, y - 1) {
                        let p3 = rel(rel(position, rotation, E, 9), rotation, N, 2);
                        self.add("roof_inner_corner", p3, (rotation + 1) % 4, MMirror::None);
                    }
                }
                if !is_house(grid, x - 1, y) {
                    let p2 = rel(rel(position, rotation, E, 0), rotation, S, 0);
                    if !is_house(grid, x, y + 1) {
                        let p3 = rel(p2, rotation, S, 6);
                        self.add("roof_corner", p3, (rotation + 1) % 4, MMirror::None);
                    } else if is_house(grid, x - 1, y + 1) {
                        let p3 = rel(rel(p2, rotation, S, 8), rotation, W, 3);
                        self.add("roof_inner_corner", p3, (rotation + 3) % 4, MMirror::None);
                    }
                    if !is_house(grid, x, y - 1) {
                        self.add("roof_corner", p2, (rotation + 2) % 4, MMirror::None);
                    } else if is_house(grid, x - 1, y - 1) {
                        let p3 = rel(p2, rotation, S, 1);
                        self.add("roof_inner_corner", p3, (rotation + 2) % 4, MMirror::None);
                    }
                }
            }
        }
    }

    fn room_1x1(&mut self, room_pos: [i32; 3], rotation: u8, door: Option<u8>, floor: usize) {
        let mut piece_rot = 0u8;
        let mut name = room_1x1(self.rng, floor);
        match door {
            Some(E) => {}
            Some(N) => piece_rot = 3,
            Some(W) => piece_rot = 2,
            Some(S) => piece_rot = 1,
            _ => name = room_1x1_secret(self.rng, floor),
        }
        let orientation = zero_position([1, 0, 0], MMirror::None, piece_rot, 7, 7);
        let final_rot = (piece_rot + rotation) % 4;
        let o = rot_vec(rotation, orientation);
        let pos = [room_pos[0] + o[0], room_pos[1], room_pos[2] + o[2]];
        self.add(name, pos, final_rot, MMirror::None);
    }

    #[allow(clippy::too_many_arguments)]
    fn room_1x2(
        &mut self,
        room_pos: [i32; 3],
        rotation: u8,
        room_dir: u8,
        door: u8,
        floor: usize,
        stairs: bool,
    ) {
        let side = |placer: &mut Placer| room_1x2_side(placer.rng, floor, stairs);
        let front = |placer: &mut Placer| room_1x2_front(placer.rng, floor, stairs);
        match (door, room_dir) {
            (E, S) => {
                let p = rel(room_pos, rotation, E, 1);
                let n = side(self);
                self.add(n, p, rotation, MMirror::None);
            }
            (E, N) => {
                let p = rel(rel(room_pos, rotation, E, 1), rotation, S, 6);
                let n = side(self);
                self.add(n, p, rotation, MMirror::LeftRight);
            }
            (W, N) => {
                let p = rel(rel(room_pos, rotation, E, 7), rotation, S, 6);
                let n = side(self);
                self.add(n, p, (rotation + 2) % 4, MMirror::None);
            }
            (W, S) => {
                let p = rel(room_pos, rotation, E, 7);
                let n = side(self);
                self.add(n, p, rotation, MMirror::FrontBack);
            }
            (S, E) => {
                let p = rel(room_pos, rotation, E, 1);
                let n = side(self);
                self.add(n, p, (rotation + 1) % 4, MMirror::LeftRight);
            }
            (S, W) => {
                let p = rel(room_pos, rotation, E, 7);
                let n = side(self);
                self.add(n, p, (rotation + 1) % 4, MMirror::None);
            }
            (N, W) => {
                let p = rel(rel(room_pos, rotation, E, 7), rotation, S, 6);
                let n = side(self);
                self.add(n, p, (rotation + 1) % 4, MMirror::FrontBack);
            }
            (N, E) => {
                let p = rel(rel(room_pos, rotation, E, 1), rotation, S, 6);
                let n = side(self);
                self.add(n, p, (rotation + 3) % 4, MMirror::None);
            }
            (S, N) => {
                let p = rel(rel(room_pos, rotation, E, 1), rotation, N, 8);
                let n = front(self);
                self.add(n, p, rotation, MMirror::None);
            }
            (N, S) => {
                let p = rel(rel(room_pos, rotation, E, 7), rotation, S, 14);
                let n = front(self);
                self.add(n, p, (rotation + 2) % 4, MMirror::None);
            }
            (W, E) => {
                let p = rel(room_pos, rotation, E, 15);
                let n = front(self);
                self.add(n, p, (rotation + 1) % 4, MMirror::None);
            }
            (E, W) => {
                let p = rel(rel(room_pos, rotation, W, 7), rotation, S, 6);
                let n = front(self);
                self.add(n, p, (rotation + 3) % 4, MMirror::None);
            }
            (255, E) => {
                let p = rel(room_pos, rotation, E, 15);
                let n = room_1x2_secret(self.rng, floor);
                self.add(n, p, (rotation + 1) % 4, MMirror::None);
            }
            (255, S) => {
                let p = rel(room_pos, rotation, E, 1);
                let n = room_1x2_secret(self.rng, floor);
                self.add(n, p, rotation, MMirror::None);
            }
            _ => {}
        }
    }

    fn room_2x2(&mut self, room_pos: [i32; 3], rotation: u8, room_dir: u8, door: u8, floor: usize) {
        let mut east = 0;
        let mut south = 0;
        let mut rot = rotation;
        let mut mirror = MMirror::None;
        match (door, room_dir) {
            (E, S) => east = -7,
            (E, N) => {
                east = -7;
                south = 6;
                mirror = MMirror::LeftRight;
            }
            (N, E) => {
                east = 1;
                south = 14;
                rot = (rotation + 3) % 4;
            }
            (N, W) => {
                east = 7;
                south = 14;
                rot = (rotation + 3) % 4;
                mirror = MMirror::LeftRight;
            }
            (S, W) => {
                east = 7;
                south = -8;
                rot = (rotation + 1) % 4;
            }
            (S, E) => {
                east = 1;
                south = -8;
                rot = (rotation + 1) % 4;
                mirror = MMirror::LeftRight;
            }
            (W, N) => {
                east = 15;
                south = 6;
                rot = (rotation + 2) % 4;
            }
            (W, S) => {
                east = 15;
                mirror = MMirror::FrontBack;
            }
            _ => {}
        }
        let p = rel(rel(room_pos, rotation, E, east), rotation, S, south);
        let n = room_2x2(self.rng, floor);
        self.add(n, p, rot, mirror);
    }
}

// FloorRoomCollection template pickers.
fn room_1x1(rng: &mut SplitMix64, floor: usize) -> String {
    if floor == 0 {
        format!("1x1_a{}", next_int(rng, 5) + 1)
    } else {
        format!("1x1_b{}", next_int(rng, 5) + 1)
    }
}

fn room_1x1_secret(rng: &mut SplitMix64, _floor: usize) -> String {
    format!("1x1_as{}", next_int(rng, 4) + 1)
}

fn room_1x2_side(rng: &mut SplitMix64, floor: usize, stairs: bool) -> String {
    if floor == 0 {
        format!("1x2_a{}", next_int(rng, 9) + 1)
    } else if stairs {
        "1x2_c_stairs".to_string()
    } else {
        format!("1x2_c{}", next_int(rng, 4) + 1)
    }
}

fn room_1x2_front(rng: &mut SplitMix64, floor: usize, stairs: bool) -> String {
    if floor == 0 {
        format!("1x2_b{}", next_int(rng, 5) + 1)
    } else if stairs {
        "1x2_d_stairs".to_string()
    } else {
        format!("1x2_d{}", next_int(rng, 5) + 1)
    }
}

fn room_1x2_secret(rng: &mut SplitMix64, floor: usize) -> String {
    if floor == 0 {
        format!("1x2_s{}", next_int(rng, 2) + 1)
    } else {
        format!("1x2_se{}", next_int(rng, 1) + 1)
    }
}

fn room_2x2(rng: &mut SplitMix64, floor: usize) -> String {
    if floor == 0 {
        format!("2x2_a{}", next_int(rng, 4) + 1)
    } else {
        format!("2x2_b{}", next_int(rng, 5) + 1)
    }
}

fn room_2x2_secret(_floor: usize) -> &'static str {
    "2x2_s1"
}

// ----------------------------------------------------------------
// Layout + carve.
// ----------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct Mansion {
    pub(crate) pieces: Vec<MansionTemplate>,
    pub(crate) hull: Bb,
}

type LayoutCache = HashMap<(u32, i32, i32), Option<Rc<Mansion>>>;
thread_local! {
    static LAYOUT_CACHE: RefCell<LayoutCache> = RefCell::new(HashMap::new());
}

pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Mansion>> {
    let seed = ctx.effective_seed();
    if let Some(hit) =
        LAYOUT_CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned())
    {
        return hit;
    }
    let built = build_layout(ctx, scx, scz).map(Rc::new);
    LAYOUT_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        // W383: 16 -> 64. The probe's REACH window sweeps more start
        // cells than 16 while circling a dark forest; the tiny cap
        // thrashed the cache and re-ran the expensive gates.
        if map.len() > 64 {
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
) -> Option<Rc<Mansion>> {
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

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Mansion> {
    let cs = CHUNK_SIZE as i32;
    let west = scx * cs;
    let north = scz * cs;
    if mc_full_biome_at(ctx, west + 8, north + 8) != McBiome::DarkForest {
        return None;
    }
    let seed = ctx.effective_seed();
    let mut rng =
        SplitMix64::new(stream_seed(seed, scx, scz, SALT_MANSION_LAYOUT));
    let rotation = next_int(&mut rng, 4) as u8;
    // getLowestYIn5by5BoxOffset7Blocks: the LOWEST surface over the
    // footprint area (16-block lattice stand-in), reject below 60.
    let mut min_y = i32::MAX;
    for dx in (-32..=48).step_by(16) {
        for dz in (-8..=72).step_by(16) {
            let o = rot_vec(rotation, [dx, 0, dz]);
            min_y = min_y.min(mc_floor_at(ctx, west + 8 + o[0], north + 8 + o[2]) + 1);
        }
    }
    if min_y < 60 {
        return None;
    }
    let origin = [west + 8, min_y, north + 8];
    let grid = MansionGrid::new(&mut rng);
    let mut placer = Placer { rng: &mut rng, start_x: 0, start_y: 0, pieces: Vec::new() };
    placer.create(origin, rotation, &grid);
    let pieces = placer.pieces;
    // Hull from the transformed template extents.
    let mut hull: Option<Bb> = None;
    for piece in &pieces {
        let Some(t) = template_of(&piece.name) else { continue };
        let a = transform([0, 0, 0], piece.rot, piece.mirror);
        let b = transform(
            [t.size[0] - 1, t.size[1] - 1, t.size[2] - 1],
            piece.rot,
            piece.mirror,
        );
        let bb = Bb {
            x0: piece.pos[0] + a[0].min(b[0]),
            y0: piece.pos[1] + a[1].min(b[1]),
            z0: piece.pos[2] + a[2].min(b[2]),
            x1: piece.pos[0] + a[0].max(b[0]),
            y1: piece.pos[1] + a[1].max(b[1]),
            z1: piece.pos[2] + a[2].max(b[2]),
        };
        hull = Some(match hull {
            None => bb,
            Some(h) => Bb {
                x0: h.x0.min(bb.x0),
                y0: h.y0.min(bb.y0),
                z0: h.z0.min(bb.z0),
                x1: h.x1.max(bb.x1),
                y1: h.y1.max(bb.y1),
                z1: h.z1.max(bb.z1),
            },
        });
    }
    Some(Mansion { pieces, hull: hull? })
}

fn template_of(name: &str) -> Option<Rc<super::template::StructureTemplate>> {
    super::village_data::template(&format!("minecraft:woodland_mansion/{name}"))
}

fn mob_gap_log() {
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        tracing::info!(
            "W259: mansions spawn no evokers/vindicators/allays yet - the \
             mobs are a future wave"
        );
    });
}

/// Mansions reach ~11 grid cells (88 blocks) from the anchor.
const REACH: i32 = 7;

/// The per-chunk mansion pass (surface_structures).
pub(crate) fn place_mansions(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    let cs = CHUNK_SIZE as i32;
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] = triangular_start(seed, gx, gz);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(mansion) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            let cube = Bb {
                x0: origin[0],
                y0: origin[1],
                z0: origin[2],
                x1: origin[0] + cs - 1,
                y1: origin[1] + cs - 1,
                z1: origin[2] + cs - 1,
            };
            let slack = Bb { y0: 0, y1: 255, ..mansion.hull };
            if !slack.intersects(&cube) {
                continue;
            }
            carve(chunk, ctx, origin, &mansion);
        }
    }
}

fn carve(chunk: &mut Chunk, ctx: &WorldgenCtx, origin: [i32; 3], mansion: &Mansion) {
    let cs = CHUNK_SIZE as i32;
    let chest_id = crate::block::CHEST_ID;
    let cobble = crate::block::COBBLE_ID;
    for piece in &mansion.pieces {
        let Some(t) = template_of(&piece.name) else { continue };
        for cell in &t.cells {
            let lp = transform(cell.pos, piece.rot, piece.mirror);
            let w = [
                piece.pos[0] + lp[0],
                piece.pos[1] + lp[1],
                piece.pos[2] + lp[2],
            ];
            let (lx, ly, lz) = (w[0] - origin[0], w[1] - origin[1], w[2] - origin[2]);
            if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
                continue;
            }
            let state = &t.palette[cell.state];
            // DATA markers: chests + the illager/allay spawns.
            if let Some(marker) = cell.data_marker.as_deref() {
                let id = if marker.starts_with("Chest") {
                    chest_id
                } else {
                    mob_gap_log();
                    0
                };
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
                continue;
            }
            // STRUCTURE_BLOCK-only ignore: air cells DO write.
            if let Mapped::Id(id) = map_state(state) {
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
            }
        }
    }
    // afterPlace: the cobblestone foundation under the footprint.
    let hull = &mansion.hull;
    let x0 = hull.x0.max(origin[0]);
    let x1 = hull.x1.min(origin[0] + cs - 1);
    let z0 = hull.z0.max(origin[2]);
    let z1 = hull.z1.min(origin[2] + cs - 1);
    for x in x0..=x1 {
        for z in z0..=z1 {
            // Only columns the mansion floor actually covers: the
            // base level holds a written cell.
            let by = hull.y0;
            let (lx, lz) = (x - origin[0], z - origin[2]);
            let base_in_cube = (0..cs).contains(&(by - origin[1]));
            let covered = if base_in_cube {
                chunk
                    .get(lx as usize, (by - origin[1]) as usize, lz as usize)
                    .0
                    != 0
            } else {
                // Cubes below the base can't read it - fall back to
                // the hull test (a slightly wider foundation there).
                true
            };
            if !covered {
                continue;
            }
            let mut y = by - 1;
            while y > 1 && !column_is_solid(ctx, x, y, z) {
                let ly = y - origin[1];
                if (0..cs).contains(&ly) {
                    chunk.set(lx as usize, ly as usize, lz as usize, BlockId(cobble));
                }
                y -= 1;
            }
        }
    }
}

/// Chest membership for the WOODLAND_MANSION loot pick (worker-side).
pub(crate) fn pos_in_mansion(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING) {
            let [scx, scz] = triangular_start(ctx.effective_seed(), gx, gz);
            // W383: distance pre-filter (monument/outpost had it,
            // mansion did not) - starts beyond REACH cannot contain
            // `pos`, so never pay their gate cost.
            if (scx - ccx).abs() > REACH || (scz - ccz).abs() > REACH {
                continue;
            }
            if let Some(m) = layout_for_probe(ctx, scx, scz)
                && m.hull.contains(pos)
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
    fn grid_produces_a_house_with_rooms() {
        let mut rng = SplitMix64::new(42);
        let grid = MansionGrid::new(&mut rng);
        let mut houses = 0;
        let mut rooms = 0;
        for y in 0..11 {
            for x in 0..11 {
                if is_house(&grid.base, x, y) {
                    houses += 1;
                }
                if grid.floor_rooms[0].get(x, y) & TYPE_MASK != 0 {
                    rooms += 1;
                }
            }
        }
        assert!(houses > 20, "mansion footprint too small: {houses}");
        assert!(rooms > 10, "first floor rooms: {rooms}");
        // Determinism.
        let mut rng2 = SplitMix64::new(42);
        let grid2 = MansionGrid::new(&mut rng2);
        for y in 0..11 {
            for x in 0..11 {
                assert_eq!(grid.base.get(x, y), grid2.base.get(x, y));
            }
        }
    }

    #[test]
    fn transforms_match_the_java_conventions() {
        // LEFT_RIGHT negates z; FRONT_BACK negates x; CW90 turns
        // (x, z) -> (-z, x).
        assert_eq!(transform([2, 1, 3], 0, MMirror::LeftRight), [2, 1, -3]);
        assert_eq!(transform([2, 1, 3], 0, MMirror::FrontBack), [-2, 1, 3]);
        assert_eq!(transform([2, 1, 3], 1, MMirror::None), [-3, 1, 2]);
        // getZeroPositionWithTransform for the 7x7 1x1 room.
        assert_eq!(zero_position([1, 0, 0], MMirror::None, 0, 7, 7), [1, 0, 0]);
        assert_eq!(zero_position([1, 0, 0], MMirror::None, 1, 7, 7), [7, 0, 0]);
        assert_eq!(zero_position([1, 0, 0], MMirror::None, 2, 7, 7), [7, 0, 6]);
        assert_eq!(zero_position([1, 0, 0], MMirror::None, 3, 7, 7), [1, 0, 6]);
    }

    /// Probe: mansions near the test region + full-pipeline verify
    /// (dark-oak mass + a chest membership when one lands).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_mansions -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_mansions() {
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
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 640);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<Rc<Mansion>> = Vec::new();
        for gx in (ccx - radius).div_euclid(SPACING)..=(ccx + radius).div_euclid(SPACING)
        {
            for gz in
                (ccz - radius).div_euclid(SPACING)..=(ccz + radius).div_euclid(SPACING)
            {
                let [sx, sz] = triangular_start(ctx.effective_seed(), gx, gz);
                if let Some(m) = layout_for_start(&ctx, sx, sz) {
                    println!(
                        "mansion at start chunk ({sx}, {sz}): {} pieces, hull {:?}",
                        m.pieces.len(),
                        m.hull
                    );
                    found.push(m);
                }
            }
        }
        println!("total: {} mansions in +-{radius} chunks", found.len());
        if let Some(m) = found.first() {
            let cx = (m.hull.x0 + m.hull.x1) / 2;
            let cy = m.hull.y0 + 2;
            let cz = (m.hull.z0 + m.hull.z1) / 2;
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let mut mansion_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if crate::block::block_type(id).is_some_and(|bt| {
                            bt.name.contains("dark_oak")
                                || bt.name == "cobble"
                                || bt.name.contains("birch")
                                || bt.name.contains("wool")
                                || bt.name.contains("carpet")
                        }) {
                            mansion_blocks += 1;
                        }
                    }
                }
            }
            println!(
                "full-pipeline mansion cube {coord:?}: {mansion_blocks} mansion blocks"
            );
            assert!(
                mansion_blocks > 50,
                "the mansion mass must materialise: {mansion_blocks}"
            );
            assert!(pos_in_mansion(&ctx, [cx, cy, cz]), "membership re-derives");
        }
    }
}
