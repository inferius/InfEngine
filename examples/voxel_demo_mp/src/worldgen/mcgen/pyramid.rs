//! W246 - desert pyramid (`DesertPyramidPiece.java` port).
//!
//! Placement: `structure_set/desert_pyramids.json` random_spread
//! (spacing 32, separation 8, salt 14357617), desert-only biome tag,
//! and the `SinglePieceStructure` gate: the LOWEST corner of the
//! 21x21 footprint must sit at or above sea level. The piece anchors
//! at the start chunk's min block, oriented randomly, and drops to
//! the lowest ground height of its footprint minus rand(3)
//! (`updateHeightPositionToLowestGroundHeight`).
//!
//! Carving mirrors the Java `postProcess` block-for-block on the
//! shared `ShaftWorld` view (in-cube exact reads/writes, column
//! oracle elsewhere): the layered body, both towers with their stair
//! crowns, the hieroglyph facades, the orange/blue terracotta floor
//! seal, the treasure pit with its four `desert_pyramid` chests, and
//! the 26.1.2 cellar (collapsed sand roof, stair shaft, sand-flooded
//! chamber). The suspicious-sand archaeology pass (`afterPlace`)
//! becomes plain sand at every potential position - the same
//! stand-in the desert well uses; MC itself converts the unpicked
//! remainder to plain sand.
//!
//! Honest gaps: the TNT trap floor is left uncarved (no TNT block in
//! the roster yet - the pressure plate sits over a dud pit),
//! chiseled sandstone substitutes cut sandstone, stairs render with
//! the engine's default facing, and chest loot-seed rolls are
//! consumed unconditionally (Java only rolls in the owning chunk;
//! the unconditional form keeps sibling-column streams aligned).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::registry_id;
use super::features::stream_seed;
use super::mineshaft::{Bb, ChunkShaftWorld, ShaftWorld};
use super::noise::SplitMix64;
use super::terrain::{mc_floor_at, mc_full_biome_at};
use crate::block::CHEST_ID;
use crate::worldgen::WorldgenCtx;

/// `structure_set/desert_pyramids.json`.
const SPACING: i32 = 32;
const SEPARATION: i32 = 8;
const SALT_PYRAMID_SET: u64 = 14_357_617;
/// Orientation + height-offset stream per start chunk.
const SALT_PYRAMID_LAYOUT: u64 = 0xD4E6_0030;
/// Per-(column, start) carve stream.
const SALT_PYRAMID_DECOR: u64 = 0xD4E6_0031;

const WIDTH: i32 = 21;
const DEPTH: i32 = 21;
const SEA_LEVEL: i32 = 63;

const SANDSTONE: u16 = 26;
const SAND: u16 = 4;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_f32(r: &mut SplitMix64) -> f32 {
    (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

/// The candidate start chunk of a 32x32 grid cell.
fn potential_start(seed: u32, grid_x: i32, grid_z: i32) -> [i32; 2] {
    super::scattered::random_spread_start(
        seed,
        grid_x,
        grid_z,
        SPACING,
        SEPARATION,
        SALT_PYRAMID_SET,
    )
}

/// One laid-out pyramid: bounding box + orientation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pyramid {
    pub(crate) bb: Bb,
    /// 0 = north, 1 = south, 2 = west, 3 = east (the piece
    /// transform's four arms).
    dir: u8,
}

impl Pyramid {
    /// `StructurePiece.getWorldX/Y/Z` for the oriented piece. Local
    /// y may be negative (the treasure pit + cellar hang below the
    /// box floor, exactly like the Java piece).
    fn world(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        let b = &self.bb;
        match self.dir {
            0 => [b.x0 + x, b.y0 + y, b.z1 - z],
            1 => [b.x0 + x, b.y0 + y, b.z0 + z],
            2 => [b.x1 - z, b.y0 + y, b.z0 + x],
            _ => [b.x0 + z, b.y0 + y, b.z0 + x],
        }
    }
}

/// Lay out (and cache) the pyramid of a start chunk: biome + sea-
/// level gates, then the height drop. Pure in the effective seed.
pub(crate) fn layout_for_start(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Pyramid> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<Pyramid>>>;
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

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Pyramid> {
    let cs = CHUNK_SIZE as i32;
    let west = scx * cs;
    let north = scz * cs;
    if mc_full_biome_at(ctx, west + 8, north + 8) != McBiome::Desert {
        return None;
    }
    // SinglePieceStructure: the lowest 21x21 corner must clear sea
    // level (the heightmap probe is floor + 1).
    let corners = [
        [west, north],
        [west + WIDTH - 1, north],
        [west, north + DEPTH - 1],
        [west + WIDTH - 1, north + DEPTH - 1],
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
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        scx,
        scz,
        SALT_PYRAMID_LAYOUT,
    ));
    let dir = next_int(&mut rng, 4) as u8;
    // updateHeightPositionToLowestGroundHeight(-rand(3)): the box
    // floor lands on the LOWEST ground of the whole footprint.
    let mut lowest = i32::MAX;
    for x in west..(west + WIDTH) {
        for z in north..(north + DEPTH) {
            lowest = lowest.min(mc_floor_at(ctx, x, z) + 1);
        }
    }
    let y0 = lowest - next_int(&mut rng, 3);
    Some(Pyramid {
        bb: Bb {
            x0: west,
            y0,
            z0: north,
            x1: west + WIDTH - 1,
            y1: y0 + 14,
            z1: north + DEPTH - 1,
        },
        dir,
    })
}

fn rid(name: &str) -> u16 {
    registry_id(name).unwrap_or(SANDSTONE)
}

/// `placeBlock` with the piece transform (no replace guard - the
/// pyramid overwrites everything in its way, like the Java piece).
fn put(w: &mut dyn ShaftWorld, p: &Pyramid, x: i32, y: i32, z: i32, id: u16) {
    let pos = p.world(x, y, z);
    let (cx0, cz0, cx1, cz1) = w.column();
    if pos[0] >= cx0 && pos[0] <= cx1 && pos[2] >= cz0 && pos[2] <= cz1 && (0..256).contains(&pos[1]) {
        w.place(pos, id);
    }
}

/// `generateBox(edge, fill)` - single-id boxes cover every call the
/// pyramid makes (edge == fill everywhere except the tower shells,
/// which pass fill = air).
#[allow(clippy::too_many_arguments)]
fn boxed(
    w: &mut dyn ShaftWorld,
    p: &Pyramid,
    (x0, y0, z0): (i32, i32, i32),
    (x1, y1, z1): (i32, i32, i32),
    edge: u16,
    fill: u16,
    skip_air: bool,
) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if skip_air {
                    let pos = p.world(x, y, z);
                    let (cx0, cz0, cx1, cz1) = w.column();
                    let inside = pos[0] >= cx0
                        && pos[0] <= cx1
                        && pos[2] >= cz0
                        && pos[2] <= cz1;
                    if !inside || w.read(pos) == 0 {
                        continue;
                    }
                }
                let on_edge =
                    y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                put(w, p, x, y, z, if on_edge { edge } else { fill });
            }
        }
    }
}

/// `fillColumnDown`: write while the cell is replaceable, descending.
fn fill_column_down(w: &mut dyn ShaftWorld, p: &Pyramid, x: i32, y: i32, z: i32, id: u16) {
    let pos = p.world(x, y, z);
    let (cx0, cz0, cx1, cz1) = w.column();
    if pos[0] < cx0 || pos[0] > cx1 || pos[2] < cz0 || pos[2] > cz1 {
        return;
    }
    let mut wy = pos[1];
    while wy > 1 {
        let cell = [pos[0], wy, pos[2]];
        let id_here = w.read(cell);
        let replaceable = id_here == 0
            || id_here == crate::block::WATER_ID
            || id_here == crate::block::LAVA_ID;
        if !replaceable {
            break;
        }
        w.place(cell, id);
        wy -= 1;
    }
}

/// The whole `DesertPyramidPiece.postProcess` body, in source order.
fn carve(w: &mut dyn ShaftWorld, p: &Pyramid, rng: &mut SplitMix64) {
    let width = WIDTH;
    let depth = DEPTH;
    let cut = rid("cut_sandstone");
    // Chiseled sandstone is not in the roster - cut stands in.
    let chiseled = cut;
    let stairs = rid("sandstone_stairs");
    let slab = rid("sandstone_slab");
    let orange = rid("orange_terracotta");
    let blue = rid("blue_terracotta");
    let plate = rid("stone_pressure_plate");

    boxed(w, p, (0, -4, 0), (width - 1, 0, depth - 1), SANDSTONE, SANDSTONE, false);
    for i in 1..=9 {
        boxed(w, p, (i, i, i), (width - 1 - i, i, depth - 1 - i), SANDSTONE, SANDSTONE, false);
        boxed(w, p, (i + 1, i, i + 1), (width - 2 - i, i, depth - 2 - i), 0, 0, false);
    }
    for x in 0..width {
        for z in 0..depth {
            fill_column_down(w, p, x, -5, z, SANDSTONE);
        }
    }
    // Both entrance towers (the stair ids drop their facing - the
    // engine renders stairs east-facing).
    boxed(w, p, (0, 0, 0), (4, 9, 4), SANDSTONE, 0, false);
    boxed(w, p, (1, 10, 1), (3, 10, 3), SANDSTONE, SANDSTONE, false);
    put(w, p, 2, 10, 0, stairs);
    put(w, p, 2, 10, 4, stairs);
    put(w, p, 0, 10, 2, stairs);
    put(w, p, 4, 10, 2, stairs);
    boxed(w, p, (width - 5, 0, 0), (width - 1, 9, 4), SANDSTONE, 0, false);
    boxed(w, p, (width - 4, 10, 1), (width - 2, 10, 3), SANDSTONE, SANDSTONE, false);
    put(w, p, width - 3, 10, 0, stairs);
    put(w, p, width - 3, 10, 4, stairs);
    put(w, p, width - 5, 10, 2, stairs);
    put(w, p, width - 1, 10, 2, stairs);
    boxed(w, p, (8, 0, 0), (12, 4, 4), SANDSTONE, 0, false);
    boxed(w, p, (9, 1, 0), (11, 3, 4), 0, 0, false);
    for (x, y) in [(9, 1), (9, 2), (9, 3), (10, 3), (11, 3), (11, 2), (11, 1)] {
        put(w, p, x, y, 1, cut);
    }
    boxed(w, p, (4, 1, 1), (8, 3, 3), SANDSTONE, 0, false);
    boxed(w, p, (4, 1, 2), (8, 2, 2), 0, 0, false);
    boxed(w, p, (12, 1, 1), (16, 3, 3), SANDSTONE, 0, false);
    boxed(w, p, (12, 1, 2), (16, 2, 2), 0, 0, false);
    boxed(w, p, (5, 4, 5), (width - 6, 4, depth - 6), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (9, 4, 9), (11, 4, 11), 0, 0, false);
    boxed(w, p, (8, 1, 8), (8, 3, 8), cut, cut, false);
    boxed(w, p, (12, 1, 8), (12, 3, 8), cut, cut, false);
    boxed(w, p, (8, 1, 12), (8, 3, 12), cut, cut, false);
    boxed(w, p, (12, 1, 12), (12, 3, 12), cut, cut, false);
    boxed(w, p, (1, 1, 5), (4, 4, 11), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (width - 5, 1, 5), (width - 2, 4, 11), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (6, 7, 9), (6, 7, 11), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (width - 7, 7, 9), (width - 7, 7, 11), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (5, 5, 9), (5, 7, 11), cut, cut, false);
    boxed(w, p, (width - 6, 5, 9), (width - 6, 7, 11), cut, cut, false);
    put(w, p, 5, 5, 10, 0);
    put(w, p, 5, 6, 10, 0);
    put(w, p, 6, 6, 10, 0);
    put(w, p, width - 6, 5, 10, 0);
    put(w, p, width - 6, 6, 10, 0);
    put(w, p, width - 7, 6, 10, 0);
    boxed(w, p, (2, 4, 4), (2, 6, 4), 0, 0, false);
    boxed(w, p, (width - 3, 4, 4), (width - 3, 6, 4), 0, 0, false);
    put(w, p, 2, 4, 5, stairs);
    put(w, p, 2, 3, 4, stairs);
    put(w, p, width - 3, 4, 5, stairs);
    put(w, p, width - 3, 3, 4, stairs);
    boxed(w, p, (1, 1, 3), (2, 2, 3), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (width - 3, 1, 3), (width - 2, 2, 3), SANDSTONE, SANDSTONE, false);
    put(w, p, 1, 1, 2, SANDSTONE);
    put(w, p, width - 2, 1, 2, SANDSTONE);
    put(w, p, 1, 2, 2, slab);
    put(w, p, width - 2, 2, 2, slab);
    put(w, p, 2, 1, 2, stairs);
    put(w, p, width - 3, 1, 2, stairs);
    boxed(w, p, (4, 3, 5), (4, 3, 17), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (width - 5, 3, 5), (width - 5, 3, 17), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (3, 1, 5), (4, 2, 16), 0, 0, false);
    boxed(w, p, (width - 6, 1, 5), (width - 5, 2, 16), 0, 0, false);
    let mut z = 5;
    while z <= 17 {
        put(w, p, 4, 1, z, cut);
        put(w, p, 4, 2, z, chiseled);
        put(w, p, width - 5, 1, z, cut);
        put(w, p, width - 5, 2, z, chiseled);
        z += 2;
    }
    // The terracotta floor seal over the treasure pit.
    for (x, z) in [
        (10, 7),
        (10, 8),
        (9, 9),
        (11, 9),
        (8, 10),
        (12, 10),
        (7, 10),
        (13, 10),
        (9, 11),
        (11, 11),
        (10, 12),
        (10, 13),
    ] {
        put(w, p, x, 0, z, orange);
    }
    put(w, p, 10, 0, 10, blue);
    // Hieroglyph facades on both side walls...
    let mut x = 0;
    while x < width {
        for (y, id) in [
            (2, cut),
            (3, cut),
            (4, orange),
            (5, cut),
            (6, orange),
            (7, orange),
            (8, cut),
        ] {
            put(w, p, x, y, 1, id);
            put(
                w,
                p,
                x,
                y,
                2,
                match y {
                    2 | 3 | 5 => orange,
                    4 | 6 => chiseled,
                    _ => if y == 7 { orange } else { cut },
                },
            );
            put(w, p, x, y, 3, id);
        }
        if x == width - 1 {
            break;
        }
        x += width - 1;
    }
    // ...and around both entrances on the front wall.
    let mut x = 2;
    while x <= width - 3 {
        for (y, mid) in [
            (2, orange),
            (3, orange),
            (4, chiseled),
            (5, orange),
            (6, chiseled),
            (7, orange),
            (8, cut),
        ] {
            let side = match y {
                4 | 6 | 7 => orange,
                _ => cut,
            };
            put(w, p, x - 1, y, 0, side);
            put(w, p, x, y, 0, mid);
            put(w, p, x + 1, y, 0, side);
        }
        if x == width - 3 {
            break;
        }
        x += width - 3 - 2;
    }
    boxed(w, p, (8, 4, 0), (12, 6, 0), cut, cut, false);
    put(w, p, 8, 6, 0, 0);
    put(w, p, 12, 6, 0, 0);
    put(w, p, 9, 5, 0, orange);
    put(w, p, 10, 5, 0, chiseled);
    put(w, p, 11, 5, 0, orange);
    // The treasure pit.
    boxed(w, p, (8, -14, 8), (12, -11, 12), cut, cut, false);
    boxed(w, p, (8, -10, 8), (12, -10, 12), chiseled, chiseled, false);
    boxed(w, p, (8, -9, 8), (12, -9, 12), cut, cut, false);
    boxed(w, p, (8, -8, 8), (12, -1, 12), SANDSTONE, SANDSTONE, false);
    boxed(w, p, (9, -11, 9), (11, -1, 11), 0, 0, false);
    put(w, p, 10, -11, 10, plate);
    // MC rings the floor below with TNT (generateBox 9,-13,9 ->
    // 11,-13,11); no TNT block exists yet, so the trap stays a dud
    // pit - deliberately NOT substituted.
    for (ax, az, bx, bz) in [(8, 10, 7, 10), (12, 10, 13, 10), (10, 8, 10, 7), (10, 12, 10, 13)] {
        put(w, p, ax, -11, az, 0);
        put(w, p, ax, -10, az, 0);
        put(w, p, bx, -10, bz, chiseled);
        put(w, p, bx, -11, bz, cut);
    }
    // Four loot chests around the plate. The loot-seed roll is
    // consumed unconditionally to keep sibling columns aligned.
    for (dx, dz) in [(0, -2), (0, 2), (-2, 0), (2, 0)] {
        let _loot_seed = rng.next_u64();
        let pos = p.world(10 + dx, -11, 10 + dz);
        let (cx0, cz0, cx1, cz1) = w.column();
        if pos[0] >= cx0 && pos[0] <= cx1 && pos[2] >= cz0 && pos[2] <= cz1 {
            w.place(pos, CHEST_ID);
        }
    }
    add_cellar(w, p, rng);
}

/// `addCellar` + stairs + room (the 26.1.2 archaeology cellar).
fn add_cellar(w: &mut dyn ShaftWorld, p: &Pyramid, rng: &mut SplitMix64) {
    let (x, y, z) = (16, -4, 13);
    let cut = rid("cut_sandstone");
    let chiseled = cut;
    let stairs = rid("sandstone_stairs");
    // Stair shaft down.
    put(w, p, 13, -1, 17, stairs);
    put(w, p, 14, -2, 17, stairs);
    put(w, p, 15, -3, 17, stairs);
    let variant = rng.next_u64() & 1 == 1;
    for dx in -4..=0 {
        put(w, p, x + dx, y + 4, z + 4, SAND);
    }
    put(w, p, x - 2, y + 3, z + 4, SAND);
    put(w, p, x - 1, y + 3, z + 4, if variant { SAND } else { SANDSTONE });
    put(w, p, x, y + 3, z + 4, if !variant { SAND } else { SANDSTONE });
    put(w, p, x - 1, y + 2, z + 4, SAND);
    put(w, p, x, y + 2, z + 4, SANDSTONE);
    put(w, p, x, y + 1, z + 4, SAND);
    // The chamber walls (skipAir like the Java boxes).
    boxed(w, p, (x - 3, y + 1, z - 3), (x - 3, y + 1, z + 2), cut, cut, true);
    boxed(w, p, (x + 3, y + 1, z - 3), (x + 3, y + 1, z + 2), cut, cut, true);
    boxed(w, p, (x - 3, y + 1, z - 3), (x + 3, y + 1, z - 2), cut, cut, true);
    boxed(w, p, (x - 3, y + 1, z + 3), (x + 3, y + 1, z + 3), cut, cut, true);
    boxed(w, p, (x - 3, y + 2, z - 3), (x - 3, y + 2, z + 2), chiseled, chiseled, true);
    boxed(w, p, (x + 3, y + 2, z - 3), (x + 3, y + 2, z + 2), chiseled, chiseled, true);
    boxed(w, p, (x - 3, y + 2, z - 3), (x + 3, y + 2, z - 2), chiseled, chiseled, true);
    boxed(w, p, (x - 3, y + 2, z + 3), (x + 3, y + 2, z + 3), chiseled, chiseled, true);
    boxed(w, p, (x - 3, -1, z - 3), (x - 3, -1, z + 2), cut, cut, true);
    boxed(w, p, (x + 3, -1, z - 3), (x + 3, -1, z + 2), cut, cut, true);
    boxed(w, p, (x - 3, -1, z - 3), (x + 3, -1, z - 2), cut, cut, true);
    boxed(w, p, (x - 3, -1, z + 3), (x + 3, -1, z + 3), cut, cut, true);
    // The sand-flooded chamber: every potential suspicious-sand cell
    // becomes plain sand (the archaeology stand-in).
    for sy in (y + 1)..=(y + 3) {
        for sx in (x - 2)..=(x + 2) {
            for sz in (z - 2)..=(z + 2) {
                put(w, p, sx, sy, sz, SAND);
            }
        }
    }
    // Collapsed roof: 1/3 sandstone, else sand, per cell.
    for rx in (x - 2)..=(x + 2) {
        for rz in (z - 2)..=(z + 2) {
            let id = if next_f32(rng) < 0.33 { SANDSTONE } else { SAND };
            put(w, p, rx, y + 4, rz, id);
        }
    }
    let orange = rid("orange_terracotta");
    let blue = rid("blue_terracotta");
    put(w, p, x, y, z, blue);
    for (dx, dz) in [(1, -1), (1, 1), (-1, -1), (-1, 1), (2, 0), (-2, 0), (0, 2), (0, -2), (3, 0)] {
        put(w, p, x + dx, y, z + dz, orange);
    }
    put(w, p, x + 3, y + 1, z, SAND);
    put(w, p, x + 3, y + 2, z, SAND);
    put(w, p, x + 4, y + 1, z, cut);
    put(w, p, x + 4, y + 2, z, chiseled);
    put(w, p, x - 3, y, z, orange);
    put(w, p, x - 3, y + 1, z, SAND);
    put(w, p, x - 3, y + 2, z, SAND);
    put(w, p, x - 4, y + 1, z, cut);
    put(w, p, x - 4, y + 2, z, chiseled);
    put(w, p, x, y, z + 3, orange);
    put(w, p, x, y + 1, z + 3, SAND);
    put(w, p, x, y + 2, z + 3, SAND);
    put(w, p, x, y, z - 3, orange);
    put(w, p, x, y + 1, z - 3, SAND);
    put(w, p, x, y + 2, z - 3, SAND);
    put(w, p, x, y + 1, z - 4, cut);
    put(w, p, x, -2, z - 4, chiseled);
}

/// The per-chunk desert-pyramid pass (surface_structures).
pub(crate) fn place_desert_pyramids(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    // A 21-block piece anchored at a chunk min reaches 2 chunks.
    const REACH: i32 = 2;
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] = potential_start(seed, gx, gz);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(pyramid) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            let mut rng = SplitMix64::new(
                stream_seed(seed, coord[0], coord[2], SALT_PYRAMID_DECOR)
                    ^ stream_seed(seed, scx, scz, SALT_PYRAMID_LAYOUT),
            );
            let mut world = ChunkShaftWorld { chunk, origin, ctx };
            carve(&mut world, &pyramid, &mut rng);
        }
    }
}

/// Deterministic membership for the chest-loot pick: the pyramid box
/// plus the treasure pit + cellar below it.
pub(crate) fn pos_in_desert_pyramid(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    const REACH: i32 = 2;
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING) {
            let [scx, scz] = potential_start(ctx.effective_seed(), gx, gz);
            let Some(p) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            let hull = Bb { y0: p.bb.y0 - 16, ..p.bb };
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
    fn potential_starts_are_deterministic_and_in_cell() {
        for gx in -2..2 {
            for gz in -2..2 {
                let [sx, sz] = potential_start(7, gx, gz);
                assert!(sx >= gx * SPACING && sx < gx * SPACING + SPACING - SEPARATION);
                assert!(sz >= gz * SPACING && sz < gz * SPACING + SPACING - SEPARATION);
                assert_eq!(potential_start(7, gx, gz), [sx, sz]);
            }
        }
    }

    #[test]
    fn orientation_transform_covers_the_square() {
        let p = Pyramid {
            bb: Bb { x0: 0, y0: 60, z0: 0, x1: 20, y1: 74, z1: 20 },
            dir: 0,
        };
        assert_eq!(p.world(0, 0, 0), [0, 60, 20]);
        assert_eq!(p.world(20, 0, 20), [20, 60, 0]);
        let p = Pyramid { dir: 2, ..p };
        assert_eq!(p.world(0, 0, 0), [20, 60, 0]);
        // Negative local y hangs below the box (the treasure pit).
        assert_eq!(p.world(0, -14, 0)[1], 46);
    }

    /// Probe: pyramid starts near a seed/centre + a full-pipeline
    /// slice of the first one (sandstone body + chests must
    /// materialise).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_desert_pyramids -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_desert_pyramids() {
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
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 96);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<Pyramid> = Vec::new();
        for gx in (ccx - radius).div_euclid(SPACING)..=(ccx + radius).div_euclid(SPACING)
        {
            for gz in
                (ccz - radius).div_euclid(SPACING)..=(ccz + radius).div_euclid(SPACING)
            {
                let [sx, sz] = potential_start(ctx.effective_seed(), gx, gz);
                if let Some(p) = layout_for_start(&ctx, sx, sz) {
                    println!("desert pyramid at start chunk ({sx}, {sz}): {:?}", p.bb);
                    found.push(p);
                }
            }
        }
        println!("total: {} pyramids", found.len());
        if let Some(p) = found.first() {
            // Full-pipeline: read the cube holding the treasure pit
            // (chests at local y -11).
            let centre = p.world(10, -11, 10);
            let coord = [
                centre[0].div_euclid(cs),
                centre[1].div_euclid(cs),
                centre[2].div_euclid(cs),
            ];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let mut chests = 0usize;
            let mut sandstone = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        match chunk.get(lx, ly, lz).0 {
                            b if b == CHEST_ID => chests += 1,
                            b if b == SANDSTONE => sandstone += 1,
                            _ => {}
                        }
                    }
                }
            }
            println!(
                "treasure cube {coord:?}: {chests} chests, {sandstone} sandstone, \
                 in-pyramid probe: {}",
                pos_in_desert_pyramid(&ctx, centre),
            );
            assert!(chests > 0, "the treasure pit generated no chests");
            assert!(sandstone > 0, "no pyramid body in the treasure cube");
            assert!(pos_in_desert_pyramid(&ctx, centre));
        }
    }
}
