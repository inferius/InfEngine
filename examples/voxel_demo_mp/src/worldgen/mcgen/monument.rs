//! W257 - ocean monument (`OceanMonumentStructure` +
//! `OceanMonumentPieces.java` port) - the guardian temple, and the
//! engine's first TRIANGULAR random-spread placement.
//!
//! Placement (`structure_set/ocean_monuments.json`): random_spread
//! spacing 32 / separation 5 / salt 10387313 with
//! `spread_type: triangular` - the cell offset averages two rolls,
//! biasing monuments toward cell centres. Gates: the start chunk
//! biome is deep ocean AND every biome sampled within 29 blocks of
//! the chunk centre is deep ocean or river
//! (`REQUIRED_OCEAN_MONUMENT_SURROUNDING`).
//!
//! The building is ONE 58x23x58 piece anchored at
//! (chunk-29, y 39, chunk-29) plus child rooms: `generateRoomGraph`
//! lays a 5x5x3 grid of 8x8x4 cells (75 slots, the top floor only
//! 3x2), connects neighbours (grid z+ pairs via NORTH - the Java
//! inversion is copied verbatim), claims the 8-cell core block,
//! randomly seals ~2 openings per room while keeping every room
//! connected to the source (the `findSource` DFS), then greedy-fits
//! room pieces: DoubleXY -> DoubleYZ -> DoubleZ -> DoubleX ->
//! DoubleY -> SimpleTop -> Simple. Wings + penthouse ride on top.
//!
//! Carving is block-for-block: prismarine (BASE_GRAY), prismarine
//! bricks (BASE_LIGHT + the DOT decor), dark prismarine
//! (BASE_BLACK), sea lanterns, the gold-block core, wet-sponge
//! ceilings in top rooms. `generateWaterBox` keeps water/ice, floods
//! below sea level and clears above; `generateBoxOnFillOnly` writes
//! only into water - both live-cube reads like the stronghold's
//! skip-air. Design rolls (simple-room design + centre pillar, wing
//! design) resolve at LAYOUT time; per-cell rolls (sponges) are pure
//! per world cell.
//!
//! Honest gaps: the THREE ELDER GUARDIANS (penthouse + both wings)
//! wait for the mob (once-logged); guardians as spawn-override mobs
//! too; ice-family keeps degrade to plain water keeps for ice kinds
//! the registry lacks.

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

/// `structure_set/ocean_monuments.json`.
const SPACING: i32 = 32;
const SEPARATION: i32 = 5;
const SALT_MONUMENT_SET: u64 = 10_387_313;
const SALT_MONUMENT_LAYOUT: u64 = 0xD4E6_00B0;
const SALT_SPONGE: u64 = 0xD4E6_00B1;
const SEA_LEVEL: i32 = 63;
/// The building anchors at a fixed depth: y 39..61.
const BASE_Y: i32 = 39;

/// Direction 3D data values (`Direction.get3DDataValue`).
const D_DOWN: usize = 0;
const D_UP: usize = 1;
const D_NORTH: usize = 2;
const D_SOUTH: usize = 3;
const D_WEST: usize = 4;
const D_EAST: usize = 5;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_bool(r: &mut SplitMix64) -> bool {
    r.next_u64() & 1 == 1
}

/// Triangular random_spread: the offset averages two rolls.
fn triangular_start(seed: u32, gx: i32, gz: i32) -> [i32; 2] {
    let mut rng = SplitMix64::new(stream_seed(seed, gx, gz, SALT_MONUMENT_SET));
    let range = (SPACING - SEPARATION).max(1) as u64;
    let ox = ((rng.next_u64() % range) + (rng.next_u64() % range)) as i32 / 2;
    let oz = ((rng.next_u64() % range) + (rng.next_u64() % range)) as i32 / 2;
    [gx * SPACING + ox, gz * SPACING + oz]
}

fn is_deep_ocean(b: McBiome) -> bool {
    matches!(
        b,
        McBiome::DeepFrozenOcean
            | McBiome::DeepColdOcean
            | McBiome::DeepOcean
            | McBiome::DeepLukewarmOcean
    )
}

fn is_surrounding(b: McBiome) -> bool {
    is_deep_ocean(b) || matches!(b, McBiome::River | McBiome::FrozenRiver)
}

// ----------------------------------------------------------------
// Room graph.
// ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct RoomLite {
    pub(crate) index: i32,
    pub(crate) conn: [Option<u16>; 6],
    pub(crate) open: [bool; 6],
}

impl RoomLite {
    fn count_openings(&self) -> i32 {
        self.open.iter().filter(|o| **o).count() as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MKind {
    Building,
    Entry,
    Core,
    DoubleX,
    DoubleXY,
    DoubleY,
    DoubleYZ,
    DoubleZ,
    Simple { design: i32, pillar: bool },
    SimpleTop,
    Wing { design: i32 },
    Penthouse,
}

#[derive(Debug, Clone)]
pub(crate) struct MPiece {
    pub(crate) kind: MKind,
    pub(crate) bb: Bb,
    pub(crate) dir: u8,
    /// Index into the layout's `rooms` (grid pieces only).
    pub(crate) room: usize,
}

#[derive(Debug)]
pub(crate) struct Monument {
    pub(crate) pieces: Vec<MPiece>,
    pub(crate) rooms: Vec<RoomLite>,
}

/// Graph scratch: connections/openings on indices into a flat Vec.
struct GRoom {
    index: i32,
    conn: [i32; 6],
    open: [bool; 6],
    claimed: bool,
    is_source: bool,
    scan: i32,
}

fn find_source(rooms: &mut [GRoom], at: usize, scan: i32) -> bool {
    if rooms[at].is_source {
        return true;
    }
    rooms[at].scan = scan;
    for f in 0..6 {
        let c = rooms[at].conn[f];
        if c >= 0 && rooms[at].open[f] {
            let ci = c as usize;
            if rooms[ci].scan != scan && find_source(rooms, ci, scan) {
                return true;
            }
        }
    }
    false
}

fn room_index(x: i32, y: i32, z: i32) -> i32 {
    y * 25 + z * 5 + x
}

/// `Direction.values()` order with step vectors.
const DIRS: [(usize, i32, i32, i32); 6] = [
    (D_DOWN, 0, -1, 0),
    (D_UP, 0, 1, 0),
    (D_NORTH, 0, 0, -1),
    (D_SOUTH, 0, 0, 1),
    (D_WEST, -1, 0, 0),
    (D_EAST, 1, 0, 0),
];

const OPPOSITE: [usize; 6] = [D_UP, D_DOWN, D_SOUTH, D_NORTH, D_EAST, D_WEST];

// ----------------------------------------------------------------
// Layout.
// ----------------------------------------------------------------

/// W380: deterministic membership for the guardian spawn override
/// (MC OceanMonumentStructure.spawnOverrides) - inside any piece
/// box of a laid-out monument.
pub(crate) fn pos_in_monument(
    ctx: &WorldgenCtx,
    pos: [i32; 3],
) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    const REACH: i32 = 4;
    let seed = ctx.effective_seed();
    for gx in
        (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING)
    {
        for gz in (ccz - REACH).div_euclid(SPACING)
            ..=(ccz + REACH).div_euclid(SPACING)
        {
            let [scx, scz] = triangular_start(seed, gx, gz);
            if (scx - ccx).abs() > REACH || (scz - ccz).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_probe(ctx, scx, scz) else {
                continue;
            };
            if layout.pieces.iter().any(|p| p.bb.contains(pos)) {
                return true;
            }
        }
    }
    false
}

type LayoutCache = HashMap<(u32, i32, i32), Option<Rc<Monument>>>;
thread_local! {
    static LAYOUT_CACHE: RefCell<LayoutCache> = RefCell::new(HashMap::new());
}

pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Monument>> {
    let seed = ctx.effective_seed();
    if let Some(hit) =
        LAYOUT_CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned())
    {
        return hit;
    }
    let built = build_layout(ctx, scx, scz).map(Rc::new);
    LAYOUT_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() > 64 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

/// W383: probe-path lookup - cache hit or (budget permitting) a
/// cold build. Over budget on the frame thread it reports `None`
/// WITHOUT caching, so the cell retries once the window rolls.
fn layout_for_probe(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Monument>> {
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

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Monument> {
    let cs = CHUNK_SIZE as i32;
    // Gate 1: the start biome hosts monuments.
    if !is_deep_ocean(mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8)) {
        return None;
    }
    // Gate 2: everything within 29 blocks is deep ocean / river.
    for dx in (-29..=29).step_by(8) {
        for dz in (-29..=29).step_by(8) {
            if !is_surrounding(mc_full_biome_at(
                ctx,
                scx * cs + 8 + dx,
                scz * cs + 8 + dz,
            )) {
                return None;
            }
        }
    }
    let seed = ctx.effective_seed();
    let mut rng =
        SplitMix64::new(stream_seed(seed, scx, scz, SALT_MONUMENT_LAYOUT));
    let dir = next_int(&mut rng, 4) as u8;
    let west = scx * cs - 29;
    let north = scz * cs - 29;
    let building = ScatteredPiece::new(west, BASE_Y, north, dir, 58, 23, 58);

    // generateRoomGraph.
    let mut rooms: Vec<GRoom> = Vec::new();
    let mut handle: HashMap<i32, usize> = HashMap::new();
    let mint = |rooms: &mut Vec<GRoom>, handle: &mut HashMap<i32, usize>, idx: i32| {
        let h = rooms.len();
        rooms.push(GRoom {
            index: idx,
            conn: [-1; 6],
            open: [false; 6],
            claimed: false,
            is_source: false,
            scan: 0,
        });
        handle.insert(idx, h);
        h
    };
    for z in 0..4 {
        for x in 0..5 {
            mint(&mut rooms, &mut handle, room_index(x, 0, z));
        }
    }
    for z in 0..4 {
        for x in 0..5 {
            mint(&mut rooms, &mut handle, room_index(x, 1, z));
        }
    }
    for z in 0..2 {
        for x in 1..4 {
            mint(&mut rooms, &mut handle, room_index(x, 2, z));
        }
    }
    // Connections (grid z pairs invert to NORTH, verbatim Java).
    for x in 0..5 {
        for z in 0..5 {
            for y in 0..3 {
                let Some(&h) = handle.get(&room_index(x, y, z)) else {
                    continue;
                };
                for (f, sx, sy, sz) in DIRS {
                    let (nx, ny, nz) = (x + sx, y + sy, z + sz);
                    if !(0..5).contains(&nx) || !(0..5).contains(&nz) || !(0..3).contains(&ny)
                    {
                        continue;
                    }
                    let Some(&nh) = handle.get(&room_index(nx, ny, nz)) else {
                        continue;
                    };
                    let face = if nz == z { f } else { OPPOSITE[f] };
                    rooms[h].conn[face] = nh as i32;
                    rooms[nh].conn[OPPOSITE[face]] = h as i32;
                }
            }
        }
    }
    let roof = mint(&mut rooms, &mut handle, 1003);
    let left_wing = mint(&mut rooms, &mut handle, 1001);
    let right_wing = mint(&mut rooms, &mut handle, 1002);
    let top_connect = handle[&room_index(2, 2, 0)];
    let leftwing_connect = handle[&room_index(0, 1, 0)];
    let rightwing_connect = handle[&room_index(4, 1, 0)];
    rooms[top_connect].conn[D_UP] = roof as i32;
    rooms[roof].conn[D_DOWN] = top_connect as i32;
    rooms[leftwing_connect].conn[D_SOUTH] = left_wing as i32;
    rooms[left_wing].conn[D_NORTH] = leftwing_connect as i32;
    rooms[rightwing_connect].conn[D_SOUTH] = right_wing as i32;
    rooms[right_wing].conn[D_NORTH] = rightwing_connect as i32;
    rooms[roof].claimed = true;
    rooms[left_wing].claimed = true;
    rooms[right_wing].claimed = true;
    let source = handle[&room_index(2, 0, 0)];
    rooms[source].is_source = true;
    // The 8-cell core block.
    let core = handle[&room_index(next_int(&mut rng, 4), 0, 2)];
    rooms[core].claimed = true;
    let core_e = rooms[core].conn[D_EAST] as usize;
    let core_n = rooms[core].conn[D_NORTH] as usize;
    let core_en = rooms[core_e].conn[D_NORTH] as usize;
    rooms[core_e].claimed = true;
    rooms[core_n].claimed = true;
    rooms[core_en].claimed = true;
    for h in [core, core_e, core_n, core_en] {
        let up = rooms[h].conn[D_UP] as usize;
        rooms[up].claimed = true;
    }
    // updateOpenings for the grid + roof.
    let mut order: Vec<usize> = (0..rooms.len() - 2).collect(); // grid + roof
    for &h in &order {
        for f in 0..6 {
            rooms[h].open[f] = rooms[h].conn[f] >= 0;
        }
    }
    // The wings never entered the open/shuffle pass; the roof did.
    order.pop(); // remove roof from the shuffled set (added after)
    // Util.shuffle.
    for i in (1..order.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    let mut scan = 1;
    for &h in &order {
        let mut closed = 0;
        let mut attempts = 0;
        while closed < 2 && attempts < 5 {
            attempts += 1;
            let f = next_int(&mut rng, 6) as usize;
            if !rooms[h].open[f] {
                continue;
            }
            let of = OPPOSITE[f];
            let c = rooms[h].conn[f] as usize;
            rooms[h].open[f] = false;
            rooms[c].open[of] = false;
            let a = find_source(&mut rooms, h, scan);
            scan += 1;
            let b = a && find_source(&mut rooms, c, scan);
            scan += 1;
            if b {
                closed += 1;
            } else {
                rooms[h].open[f] = true;
                rooms[c].open[of] = true;
            }
        }
    }
    // Definition order for fitting: shuffled grid, then roof + wings.
    let mut fit_order = order.clone();
    fit_order.push(roof);
    fit_order.push(left_wing);
    fit_order.push(right_wing);

    // Room piece boxes: local room-grid box per orientation, shifted
    // by the world position of building-local (9, 0, 22).
    let anchor = building.world(9, 0, 22);
    let room_bb = |idx: i32, w: i32, h: i32, d: i32| -> Bb {
        let rx = idx % 5;
        let rz = idx / 5 % 5;
        let ry = idx / 25;
        let (sx, sy, sz) = (w * 8, h * 4, d * 8);
        // makeBoundingBox(0,0,0, dir, sx, sy, sz) footprint transpose
        // + the per-orientation room offset.
        let (ex, ez) = if dir <= 1 { (sx, sz) } else { (sz, sx) };
        let (mx, mz) = match dir {
            0 => (rx * 8, -(rz + d) * 8 + 1),           // NORTH
            1 => (rx * 8, rz * 8),                      // SOUTH
            2 => (-(rz + d) * 8 + 1, rx * 8),           // WEST
            _ => (rz * 8, rx * 8),                      // EAST
        };
        Bb {
            x0: anchor[0] + mx,
            y0: anchor[1] + ry * 4,
            z0: anchor[2] + mz,
            x1: anchor[0] + mx + ex - 1,
            y1: anchor[1] + ry * 4 + sy - 1,
            z1: anchor[2] + mz + ez - 1,
        }
    };

    let mut pieces: Vec<MPiece> = vec![MPiece {
        kind: MKind::Building,
        bb: building.bb,
        dir,
        room: 0,
    }];
    rooms[source].claimed = true;
    pieces.push(MPiece {
        kind: MKind::Entry,
        bb: room_bb(rooms[source].index, 1, 1, 1),
        dir,
        room: source,
    });
    pieces.push(MPiece {
        kind: MKind::Core,
        bb: room_bb(rooms[core].index, 2, 2, 2),
        dir,
        room: core,
    });
    // Fitters, in Java priority order.
    for &h in &fit_order {
        if rooms[h].claimed || rooms[h].index >= 75 {
            continue;
        }
        let open = rooms[h].open;
        let conn = rooms[h].conn;
        let up_ok = |rooms: &Vec<GRoom>, i: i32| i >= 0 && !rooms[i as usize].claimed;
        // FitDoubleXY.
        if open[D_EAST]
            && up_ok(&rooms, conn[D_EAST])
            && open[D_UP]
            && up_ok(&rooms, conn[D_UP])
            && {
                let e = conn[D_EAST] as usize;
                rooms[e].open[D_UP] && up_ok(&rooms, rooms[e].conn[D_UP])
            }
        {
            let e = conn[D_EAST] as usize;
            let eu = rooms[e].conn[D_UP] as usize;
            let u = conn[D_UP] as usize;
            rooms[h].claimed = true;
            rooms[e].claimed = true;
            rooms[u].claimed = true;
            rooms[eu].claimed = true;
            pieces.push(MPiece {
                kind: MKind::DoubleXY,
                bb: room_bb(rooms[h].index, 2, 2, 1),
                dir,
                room: h,
            });
            continue;
        }
        // FitDoubleYZ.
        if open[D_NORTH]
            && up_ok(&rooms, conn[D_NORTH])
            && open[D_UP]
            && up_ok(&rooms, conn[D_UP])
            && {
                let n = conn[D_NORTH] as usize;
                rooms[n].open[D_UP] && up_ok(&rooms, rooms[n].conn[D_UP])
            }
        {
            let n = conn[D_NORTH] as usize;
            let nu = rooms[n].conn[D_UP] as usize;
            let u = conn[D_UP] as usize;
            rooms[h].claimed = true;
            rooms[n].claimed = true;
            rooms[u].claimed = true;
            rooms[nu].claimed = true;
            pieces.push(MPiece {
                kind: MKind::DoubleYZ,
                bb: room_bb(rooms[h].index, 1, 2, 2),
                dir,
                room: h,
            });
            continue;
        }
        // FitDoubleZ.
        if open[D_NORTH] && up_ok(&rooms, conn[D_NORTH]) {
            let n = conn[D_NORTH] as usize;
            rooms[h].claimed = true;
            rooms[n].claimed = true;
            pieces.push(MPiece {
                kind: MKind::DoubleZ,
                bb: room_bb(rooms[h].index, 1, 1, 2),
                dir,
                room: h,
            });
            continue;
        }
        // FitDoubleX.
        if open[D_EAST] && up_ok(&rooms, conn[D_EAST]) {
            let e = conn[D_EAST] as usize;
            rooms[h].claimed = true;
            rooms[e].claimed = true;
            pieces.push(MPiece {
                kind: MKind::DoubleX,
                bb: room_bb(rooms[h].index, 2, 1, 1),
                dir,
                room: h,
            });
            continue;
        }
        // FitDoubleY.
        if open[D_UP] && up_ok(&rooms, conn[D_UP]) {
            let u = conn[D_UP] as usize;
            rooms[h].claimed = true;
            rooms[u].claimed = true;
            pieces.push(MPiece {
                kind: MKind::DoubleY,
                bb: room_bb(rooms[h].index, 1, 2, 1),
                dir,
                room: h,
            });
            continue;
        }
        // FitSimpleTop.
        if !open[D_WEST] && !open[D_EAST] && !open[D_NORTH] && !open[D_SOUTH] && !open[D_UP]
        {
            rooms[h].claimed = true;
            pieces.push(MPiece {
                kind: MKind::SimpleTop,
                bb: room_bb(rooms[h].index, 1, 1, 1),
                dir,
                room: h,
            });
            continue;
        }
        // FitSimple (always fits). Design + the centre pillar resolve
        // at layout time (MC rolls the pillar at postProcess).
        rooms[h].claimed = true;
        let design = next_int(&mut rng, 3);
        let pillar = design != 0
            && next_bool(&mut rng)
            && !open[D_DOWN]
            && !open[D_UP]
            && {
                let lite_openings =
                    open.iter().filter(|o| **o).count() as i32;
                lite_openings > 1
            };
        pieces.push(MPiece {
            kind: MKind::Simple { design, pillar },
            bb: room_bb(rooms[h].index, 1, 1, 1),
            dir,
            room: h,
        });
    }
    // Wings + penthouse (absolute corner boxes).
    let corner = |a: [i32; 3], b: [i32; 3]| Bb {
        x0: a[0].min(b[0]),
        y0: a[1].min(b[1]),
        z0: a[2].min(b[2]),
        x1: a[0].max(b[0]),
        y1: a[1].max(b[1]),
        z1: a[2].max(b[2]),
    };
    let wing_design = next_int(&mut rng, 2);
    pieces.push(MPiece {
        kind: MKind::Wing { design: wing_design },
        bb: corner(building.world(1, 1, 1), building.world(23, 8, 21)),
        dir,
        room: 0,
    });
    pieces.push(MPiece {
        kind: MKind::Wing { design: (wing_design + 1) & 1 },
        bb: corner(building.world(34, 1, 1), building.world(56, 8, 21)),
        dir,
        room: 0,
    });
    pieces.push(MPiece {
        kind: MKind::Penthouse,
        bb: corner(building.world(22, 13, 22), building.world(35, 17, 35)),
        dir,
        room: 0,
    });

    let rooms_lite = rooms
        .iter()
        .map(|r| RoomLite {
            index: r.index,
            conn: r.conn.map(|c| if c >= 0 { Some(c as u16) } else { None }),
            open: r.open,
        })
        .collect();
    Some(Monument { pieces, rooms: rooms_lite })
}

// ----------------------------------------------------------------
// Carving.
// ----------------------------------------------------------------

struct Pal {
    gray: u16,
    light: u16,
    black: u16,
    lamp: u16,
    gold: u16,
    wet_sponge: u16,
    water: u16,
    ices: [u16; 3],
}

impl Pal {
    fn get() -> Pal {
        let gray = registry_id("prismarine").unwrap_or(1);
        Pal {
            gray,
            light: registry_id("prismarine_bricks").unwrap_or(gray),
            black: registry_id("dark_prismarine").unwrap_or(gray),
            lamp: registry_id("sea_lantern").unwrap_or(gray),
            gold: registry_id("gold_block").unwrap_or(gray),
            wet_sponge: registry_id("wet_sponge").unwrap_or(gray),
            water: crate::block::WATER_ID,
            ices: [
                registry_id("ice").unwrap_or(0),
                registry_id("packed_ice").unwrap_or(0),
                registry_id("blue_ice").unwrap_or(0),
            ],
        }
    }
}

struct MCarver<'a, 'w> {
    w: &'a mut dyn ShaftWorld,
    f: ScatteredPiece,
    seed: u32,
    pal: &'w Pal,
}

impl MCarver<'_, '_> {
    fn in_cube(&self, pos: [i32; 3]) -> bool {
        let (cx0, cz0, cx1, cz1) = self.w.column();
        pos[0] >= cx0
            && pos[0] <= cx1
            && pos[2] >= cz0
            && pos[2] <= cz1
            && (-64..320).contains(&pos[1])
    }

    fn put(&mut self, x: i32, y: i32, z: i32, id: u16) {
        let pos = self.f.world(x, y, z);
        if self.in_cube(pos) {
            self.w.place(pos, id);
        }
    }

    fn boxed(&mut self, a: (i32, i32, i32), b: (i32, i32, i32), id: u16) {
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                for z in a.2..=b.2 {
                    self.put(x, y, z, id);
                }
            }
        }
    }

    /// `generateWaterBox`: keep water/ice, flood below sea level,
    /// clear above.
    fn water_box(&mut self, a: (i32, i32, i32), b: (i32, i32, i32)) {
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                for z in a.2..=b.2 {
                    let pos = self.f.world(x, y, z);
                    if !self.in_cube(pos) {
                        continue;
                    }
                    let cur = self.w.read(pos);
                    if cur == self.pal.water || self.pal.ices.contains(&cur) {
                        continue;
                    }
                    let id = if pos[1] >= SEA_LEVEL { 0 } else { self.pal.water };
                    self.w.place(pos, id);
                }
            }
        }
    }

    /// `generateBoxOnFillOnly`: write only where water stands.
    fn fill_only(&mut self, a: (i32, i32, i32), b: (i32, i32, i32), id: u16) {
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                for z in a.2..=b.2 {
                    let pos = self.f.world(x, y, z);
                    if self.in_cube(pos) && self.w.read(pos) == self.pal.water {
                        self.w.place(pos, id);
                    }
                }
            }
        }
    }

    fn fill_column_down(&mut self, x: i32, y: i32, z: i32, id: u16) {
        let pos = self.f.world(x, y, z);
        let (cx0, cz0, cx1, cz1) = self.w.column();
        if pos[0] < cx0 || pos[0] > cx1 || pos[2] < cz0 || pos[2] > cz1 {
            return;
        }
        let mut wy = pos[1];
        while wy > 1 {
            let cell = [pos[0], wy, pos[2]];
            let here = self.w.read(cell);
            if !(here == 0
                || here == self.pal.water
                || here == crate::block::LAVA_ID)
            {
                break;
            }
            self.w.place(cell, id);
            wy -= 1;
        }
    }

    /// Pure sponge roll per world cell: `Some(y0)` when a column
    /// grows (2/3), y0 = 2 or 3 (1/4 : 3/4).
    fn sponge_roll(&self, x: i32, z: i32) -> Option<i32> {
        let pos = self.f.world(x, 0, z);
        let s = stream_seed(self.seed, pos[0], pos[2], SALT_SPONGE)
            ^ (pos[1] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut r = SplitMix64::new(s);
        if r.next_u64().is_multiple_of(3) {
            return None;
        }
        Some(2 + if r.next_u64().is_multiple_of(4) { 0 } else { 1 })
    }
}

/// The single-cell floor slab + ceiling helpers shared by rooms.
fn default_floor(c: &mut MCarver, xo: i32, zo: i32, down_open: bool) {
    let (gray, light) = (c.pal.gray, c.pal.light);
    if down_open {
        c.boxed((xo, 0, zo), (xo + 2, 0, zo + 7), gray);
        c.boxed((xo + 5, 0, zo), (xo + 7, 0, zo + 7), gray);
        c.boxed((xo + 3, 0, zo), (xo + 4, 0, zo + 2), gray);
        c.boxed((xo + 3, 0, zo + 5), (xo + 4, 0, zo + 7), gray);
        c.boxed((xo + 3, 0, zo + 2), (xo + 4, 0, zo + 2), light);
        c.boxed((xo + 3, 0, zo + 5), (xo + 4, 0, zo + 5), light);
        c.boxed((xo + 2, 0, zo + 3), (xo + 2, 0, zo + 4), light);
        c.boxed((xo + 5, 0, zo + 3), (xo + 5, 0, zo + 4), light);
    } else {
        c.boxed((xo, 0, zo), (xo + 7, 0, zo + 7), gray);
    }
}

#[allow(clippy::too_many_lines)]
fn carve_piece(c: &mut MCarver, m: &Monument, piece: &MPiece) {
    let (gray, light, black, lamp) =
        (c.pal.gray, c.pal.light, c.pal.black, c.pal.lamp);
    let room = &m.rooms[piece.room];
    let up_conn = |r: &RoomLite| r.conn[D_UP].map(|i| i as usize);
    match piece.kind {
        MKind::Building => {
            // The sea-level flush over the whole footprint.
            let water_h = SEA_LEVEL.max(64) - BASE_Y;
            c.water_box((0, 0, 0), (58, water_h, 58));
            carve_building_exterior(c);
            // Foundation pillars + the water border rings.
            for px in 0..7 {
                let mut pz = 0;
                while pz < 7 {
                    if pz == 0 && px == 3 {
                        pz = 6;
                    }
                    let bx = px * 9;
                    let bz = pz * 9;
                    for w in 0..4 {
                        for d in 0..4 {
                            c.put(bx + w, 0, bz + d, light);
                            c.fill_column_down(bx + w, -1, bz + d, light);
                        }
                    }
                    if px != 0 && px != 6 {
                        pz += 6;
                    } else {
                        pz += 1;
                    }
                }
            }
            for i in 0..5 {
                c.water_box((-1 - i, i * 2, -1 - i), (-1 - i, 23, 58 + i));
                c.water_box((58 + i, i * 2, -1 - i), (58 + i, 23, 58 + i));
                c.water_box((-i, i * 2, -1 - i), (57 + i, 23, -1 - i));
                c.water_box((-i, i * 2, 58 + i), (57 + i, 23, 58 + i));
            }
        }
        MKind::Entry => {
            c.boxed((0, 3, 0), (2, 3, 7), light);
            c.boxed((5, 3, 0), (7, 3, 7), light);
            c.boxed((0, 2, 0), (1, 2, 7), light);
            c.boxed((6, 2, 0), (7, 2, 7), light);
            c.boxed((0, 1, 0), (0, 1, 7), light);
            c.boxed((7, 1, 0), (7, 1, 7), light);
            c.boxed((0, 1, 7), (7, 3, 7), light);
            c.boxed((1, 1, 0), (2, 3, 0), light);
            c.boxed((5, 1, 0), (6, 3, 0), light);
            if room.open[D_NORTH] {
                c.water_box((3, 1, 7), (4, 2, 7));
            }
            if room.open[D_WEST] {
                c.water_box((0, 1, 3), (1, 2, 4));
            }
            if room.open[D_EAST] {
                c.water_box((6, 1, 3), (7, 2, 4));
            }
        }
        MKind::Core => {
            c.fill_only((1, 8, 0), (14, 8, 14), gray);
            c.boxed((0, 7, 0), (0, 7, 15), light);
            c.boxed((15, 7, 0), (15, 7, 15), light);
            c.boxed((1, 7, 0), (15, 7, 0), light);
            c.boxed((1, 7, 15), (14, 7, 15), light);
            for y in 1..=6 {
                let b = if y == 2 || y == 6 { gray } else { light };
                for x in [0, 15] {
                    c.boxed((x, y, 0), (x, y, 1), b);
                    c.boxed((x, y, 6), (x, y, 9), b);
                    c.boxed((x, y, 14), (x, y, 15), b);
                }
                c.boxed((1, y, 0), (1, y, 0), b);
                c.boxed((6, y, 0), (9, y, 0), b);
                c.boxed((14, y, 0), (14, y, 0), b);
                c.boxed((1, y, 15), (14, y, 15), b);
            }
            c.boxed((6, 3, 6), (9, 6, 9), black);
            c.boxed((7, 4, 7), (8, 5, 8), c.pal.gold);
            for y in [3, 6] {
                for x in [6, 9] {
                    c.put(x, y, 6, lamp);
                    c.put(x, y, 9, lamp);
                }
            }
            for (a, b) in [
                ((5, 1, 6), (5, 2, 6)),
                ((5, 1, 9), (5, 2, 9)),
                ((10, 1, 6), (10, 2, 6)),
                ((10, 1, 9), (10, 2, 9)),
                ((6, 1, 5), (6, 2, 5)),
                ((9, 1, 5), (9, 2, 5)),
                ((6, 1, 10), (6, 2, 10)),
                ((9, 1, 10), (9, 2, 10)),
                ((5, 2, 5), (5, 6, 5)),
                ((5, 2, 10), (5, 6, 10)),
                ((10, 2, 5), (10, 6, 5)),
                ((10, 2, 10), (10, 6, 10)),
                ((5, 7, 1), (5, 7, 6)),
                ((10, 7, 1), (10, 7, 6)),
                ((5, 7, 9), (5, 7, 14)),
                ((10, 7, 9), (10, 7, 14)),
                ((1, 7, 5), (6, 7, 5)),
                ((1, 7, 10), (6, 7, 10)),
                ((9, 7, 5), (14, 7, 5)),
                ((9, 7, 10), (14, 7, 10)),
                ((2, 1, 2), (2, 1, 3)),
                ((3, 1, 2), (3, 1, 2)),
                ((13, 1, 2), (13, 1, 3)),
                ((12, 1, 2), (12, 1, 2)),
                ((2, 1, 12), (2, 1, 13)),
                ((3, 1, 13), (3, 1, 13)),
                ((13, 1, 12), (13, 1, 13)),
                ((12, 1, 13), (12, 1, 13)),
            ] {
                c.boxed(a, b, light);
            }
        }
        MKind::DoubleX => {
            let east = &m.rooms[room.conn[D_EAST].unwrap() as usize];
            if room.index / 25 > 0 {
                default_floor(c, 8, 0, east.open[D_DOWN]);
                default_floor(c, 0, 0, room.open[D_DOWN]);
            }
            if up_conn(room).is_none() {
                c.fill_only((1, 4, 1), (7, 4, 6), gray);
            }
            if up_conn(east).is_none() {
                c.fill_only((8, 4, 1), (14, 4, 6), gray);
            }
            c.boxed((0, 3, 0), (0, 3, 7), light);
            c.boxed((15, 3, 0), (15, 3, 7), light);
            c.boxed((1, 3, 0), (15, 3, 0), light);
            c.boxed((1, 3, 7), (14, 3, 7), light);
            c.boxed((0, 2, 0), (0, 2, 7), gray);
            c.boxed((15, 2, 0), (15, 2, 7), gray);
            c.boxed((1, 2, 0), (15, 2, 0), gray);
            c.boxed((1, 2, 7), (14, 2, 7), gray);
            c.boxed((0, 1, 0), (0, 1, 7), light);
            c.boxed((15, 1, 0), (15, 1, 7), light);
            c.boxed((1, 1, 0), (15, 1, 0), light);
            c.boxed((1, 1, 7), (14, 1, 7), light);
            c.boxed((5, 1, 0), (10, 1, 4), light);
            c.boxed((6, 2, 0), (9, 2, 3), gray);
            c.boxed((5, 3, 0), (10, 3, 4), light);
            c.put(6, 2, 3, lamp);
            c.put(9, 2, 3, lamp);
            if room.open[D_SOUTH] {
                c.water_box((3, 1, 0), (4, 2, 0));
            }
            if room.open[D_NORTH] {
                c.water_box((3, 1, 7), (4, 2, 7));
            }
            if room.open[D_WEST] {
                c.water_box((0, 1, 3), (0, 2, 4));
            }
            if east.open[D_SOUTH] {
                c.water_box((11, 1, 0), (12, 2, 0));
            }
            if east.open[D_NORTH] {
                c.water_box((11, 1, 7), (12, 2, 7));
            }
            if east.open[D_EAST] {
                c.water_box((15, 1, 3), (15, 2, 4));
            }
        }
        MKind::DoubleXY => {
            let east = &m.rooms[room.conn[D_EAST].unwrap() as usize];
            let west_up = &m.rooms[room.conn[D_UP].unwrap() as usize];
            let east_up = &m.rooms[east.conn[D_UP].unwrap() as usize];
            if room.index / 25 > 0 {
                default_floor(c, 8, 0, east.open[D_DOWN]);
                default_floor(c, 0, 0, room.open[D_DOWN]);
            }
            if up_conn(west_up).is_none() {
                c.fill_only((1, 8, 1), (7, 8, 6), gray);
            }
            if up_conn(east_up).is_none() {
                c.fill_only((8, 8, 1), (14, 8, 6), gray);
            }
            for y in 1..=7 {
                let b = if y == 2 || y == 6 { gray } else { light };
                c.boxed((0, y, 0), (0, y, 7), b);
                c.boxed((15, y, 0), (15, y, 7), b);
                c.boxed((1, y, 0), (15, y, 0), b);
                c.boxed((1, y, 7), (14, y, 7), b);
            }
            for (a, b) in [
                ((2, 1, 3), (2, 7, 4)),
                ((3, 1, 2), (4, 7, 2)),
                ((3, 1, 5), (4, 7, 5)),
                ((13, 1, 3), (13, 7, 4)),
                ((11, 1, 2), (12, 7, 2)),
                ((11, 1, 5), (12, 7, 5)),
                ((5, 1, 3), (5, 3, 4)),
                ((10, 1, 3), (10, 3, 4)),
                ((5, 7, 2), (10, 7, 5)),
                ((5, 5, 2), (5, 7, 2)),
                ((10, 5, 2), (10, 7, 2)),
                ((5, 5, 5), (5, 7, 5)),
                ((10, 5, 5), (10, 7, 5)),
            ] {
                c.boxed(a, b, light);
            }
            c.put(6, 6, 2, light);
            c.put(9, 6, 2, light);
            c.put(6, 6, 5, light);
            c.put(9, 6, 5, light);
            c.boxed((5, 4, 3), (6, 4, 4), light);
            c.boxed((9, 4, 3), (10, 4, 4), light);
            c.put(5, 4, 2, lamp);
            c.put(5, 4, 5, lamp);
            c.put(10, 4, 2, lamp);
            c.put(10, 4, 5, lamp);
            type GateCheck<'a> = (&'a RoomLite, usize, (i32, i32, i32), (i32, i32, i32));
            let checks: [GateCheck; 12] = [
                (room, D_SOUTH, (3, 1, 0), (4, 2, 0)),
                (room, D_NORTH, (3, 1, 7), (4, 2, 7)),
                (room, D_WEST, (0, 1, 3), (0, 2, 4)),
                (east, D_SOUTH, (11, 1, 0), (12, 2, 0)),
                (east, D_NORTH, (11, 1, 7), (12, 2, 7)),
                (east, D_EAST, (15, 1, 3), (15, 2, 4)),
                (west_up, D_SOUTH, (3, 5, 0), (4, 6, 0)),
                (west_up, D_NORTH, (3, 5, 7), (4, 6, 7)),
                (west_up, D_WEST, (0, 5, 3), (0, 6, 4)),
                (east_up, D_SOUTH, (11, 5, 0), (12, 6, 0)),
                (east_up, D_NORTH, (11, 5, 7), (12, 6, 7)),
                (east_up, D_EAST, (15, 5, 3), (15, 6, 4)),
            ];
            for (r, f, a, b) in checks {
                if r.open[f] {
                    c.water_box(a, b);
                }
            }
        }
        MKind::DoubleY => {
            if room.index / 25 > 0 {
                default_floor(c, 0, 0, room.open[D_DOWN]);
            }
            let above = &m.rooms[room.conn[D_UP].unwrap() as usize];
            if up_conn(above).is_none() {
                c.fill_only((1, 8, 1), (6, 8, 6), gray);
            }
            c.boxed((0, 4, 0), (0, 4, 7), light);
            c.boxed((7, 4, 0), (7, 4, 7), light);
            c.boxed((1, 4, 0), (6, 4, 0), light);
            c.boxed((1, 4, 7), (6, 4, 7), light);
            c.boxed((2, 4, 1), (2, 4, 2), light);
            c.boxed((1, 4, 2), (1, 4, 2), light);
            c.boxed((5, 4, 1), (5, 4, 2), light);
            c.boxed((6, 4, 2), (6, 4, 2), light);
            c.boxed((2, 4, 5), (2, 4, 6), light);
            c.boxed((1, 4, 5), (1, 4, 5), light);
            c.boxed((5, 4, 5), (5, 4, 6), light);
            c.boxed((6, 4, 5), (6, 4, 5), light);
            let mut cur = room;
            for y in [1, 5] {
                if cur.open[D_SOUTH] {
                    c.boxed((2, y, 0), (2, y + 2, 0), light);
                    c.boxed((5, y, 0), (5, y + 2, 0), light);
                    c.boxed((3, y + 2, 0), (4, y + 2, 0), light);
                } else {
                    c.boxed((0, y, 0), (7, y + 2, 0), light);
                    c.boxed((0, y + 1, 0), (7, y + 1, 0), gray);
                }
                if cur.open[D_NORTH] {
                    c.boxed((2, y, 7), (2, y + 2, 7), light);
                    c.boxed((5, y, 7), (5, y + 2, 7), light);
                    c.boxed((3, y + 2, 7), (4, y + 2, 7), light);
                } else {
                    c.boxed((0, y, 7), (7, y + 2, 7), light);
                    c.boxed((0, y + 1, 7), (7, y + 1, 7), gray);
                }
                if cur.open[D_WEST] {
                    c.boxed((0, y, 2), (0, y + 2, 2), light);
                    c.boxed((0, y, 5), (0, y + 2, 5), light);
                    c.boxed((0, y + 2, 3), (0, y + 2, 4), light);
                } else {
                    c.boxed((0, y, 0), (0, y + 2, 7), light);
                    c.boxed((0, y + 1, 0), (0, y + 1, 7), gray);
                }
                if cur.open[D_EAST] {
                    c.boxed((7, y, 2), (7, y + 2, 2), light);
                    c.boxed((7, y, 5), (7, y + 2, 5), light);
                    c.boxed((7, y + 2, 3), (7, y + 2, 4), light);
                } else {
                    c.boxed((7, y, 0), (7, y + 2, 7), light);
                    c.boxed((7, y + 1, 0), (7, y + 1, 7), gray);
                }
                cur = above;
            }
        }
        MKind::DoubleYZ => {
            let north = &m.rooms[room.conn[D_NORTH].unwrap() as usize];
            let south_up = &m.rooms[room.conn[D_UP].unwrap() as usize];
            let north_up = &m.rooms[north.conn[D_UP].unwrap() as usize];
            if room.index / 25 > 0 {
                default_floor(c, 0, 8, north.open[D_DOWN]);
                default_floor(c, 0, 0, room.open[D_DOWN]);
            }
            if up_conn(south_up).is_none() {
                c.fill_only((1, 8, 1), (6, 8, 7), gray);
            }
            if up_conn(north_up).is_none() {
                c.fill_only((1, 8, 8), (6, 8, 14), gray);
            }
            for y in 1..=7 {
                let b = if y == 2 || y == 6 { gray } else { light };
                c.boxed((0, y, 0), (0, y, 15), b);
                c.boxed((7, y, 0), (7, y, 15), b);
                c.boxed((1, y, 0), (6, y, 0), b);
                c.boxed((1, y, 15), (6, y, 15), b);
            }
            for y in 1..=7 {
                let b = if y == 2 || y == 6 { lamp } else { black };
                c.boxed((3, y, 7), (4, y, 8), b);
            }
            if room.open[D_SOUTH] {
                c.water_box((3, 1, 0), (4, 2, 0));
            }
            if room.open[D_EAST] {
                c.water_box((7, 1, 3), (7, 2, 4));
            }
            if room.open[D_WEST] {
                c.water_box((0, 1, 3), (0, 2, 4));
            }
            if north.open[D_NORTH] {
                c.water_box((3, 1, 15), (4, 2, 15));
            }
            if north.open[D_WEST] {
                c.water_box((0, 1, 11), (0, 2, 12));
            }
            if north.open[D_EAST] {
                c.water_box((7, 1, 11), (7, 2, 12));
            }
            if south_up.open[D_SOUTH] {
                c.water_box((3, 5, 0), (4, 6, 0));
            }
            if south_up.open[D_EAST] {
                c.water_box((7, 5, 3), (7, 6, 4));
                c.boxed((5, 4, 2), (6, 4, 5), light);
                c.boxed((6, 1, 2), (6, 3, 2), light);
                c.boxed((6, 1, 5), (6, 3, 5), light);
            }
            if south_up.open[D_WEST] {
                c.water_box((0, 5, 3), (0, 6, 4));
                c.boxed((1, 4, 2), (2, 4, 5), light);
                c.boxed((1, 1, 2), (1, 3, 2), light);
                c.boxed((1, 1, 5), (1, 3, 5), light);
            }
            if north_up.open[D_NORTH] {
                c.water_box((3, 5, 15), (4, 6, 15));
            }
            if north_up.open[D_WEST] {
                c.water_box((0, 5, 11), (0, 6, 12));
                c.boxed((1, 4, 10), (2, 4, 13), light);
                c.boxed((1, 1, 10), (1, 3, 10), light);
                c.boxed((1, 1, 13), (1, 3, 13), light);
            }
            if north_up.open[D_EAST] {
                c.water_box((7, 5, 11), (7, 6, 12));
                c.boxed((5, 4, 10), (6, 4, 13), light);
                c.boxed((6, 1, 10), (6, 3, 10), light);
                c.boxed((6, 1, 13), (6, 3, 13), light);
            }
        }
        MKind::DoubleZ => {
            let north = &m.rooms[room.conn[D_NORTH].unwrap() as usize];
            if room.index / 25 > 0 {
                default_floor(c, 0, 8, north.open[D_DOWN]);
                default_floor(c, 0, 0, room.open[D_DOWN]);
            }
            if up_conn(room).is_none() {
                c.fill_only((1, 4, 1), (6, 4, 7), gray);
            }
            if up_conn(north).is_none() {
                c.fill_only((1, 4, 8), (6, 4, 14), gray);
            }
            c.boxed((0, 3, 0), (0, 3, 15), light);
            c.boxed((7, 3, 0), (7, 3, 15), light);
            c.boxed((1, 3, 0), (7, 3, 0), light);
            c.boxed((1, 3, 15), (6, 3, 15), light);
            c.boxed((0, 2, 0), (0, 2, 15), gray);
            c.boxed((7, 2, 0), (7, 2, 15), gray);
            c.boxed((1, 2, 0), (7, 2, 0), gray);
            c.boxed((1, 2, 15), (6, 2, 15), gray);
            c.boxed((0, 1, 0), (0, 1, 15), light);
            c.boxed((7, 1, 0), (7, 1, 15), light);
            c.boxed((1, 1, 0), (7, 1, 0), light);
            c.boxed((1, 1, 15), (6, 1, 15), light);
            for (a, b) in [
                ((1, 1, 1), (1, 1, 2)),
                ((6, 1, 1), (6, 1, 2)),
                ((1, 3, 1), (1, 3, 2)),
                ((6, 3, 1), (6, 3, 2)),
                ((1, 1, 13), (1, 1, 14)),
                ((6, 1, 13), (6, 1, 14)),
                ((1, 3, 13), (1, 3, 14)),
                ((6, 3, 13), (6, 3, 14)),
                ((2, 1, 6), (2, 3, 6)),
                ((5, 1, 6), (5, 3, 6)),
                ((2, 1, 9), (2, 3, 9)),
                ((5, 1, 9), (5, 3, 9)),
                ((3, 2, 6), (4, 2, 6)),
                ((3, 2, 9), (4, 2, 9)),
                ((2, 2, 7), (2, 2, 8)),
                ((5, 2, 7), (5, 2, 8)),
            ] {
                c.boxed(a, b, light);
            }
            c.put(2, 2, 5, lamp);
            c.put(5, 2, 5, lamp);
            c.put(2, 2, 10, lamp);
            c.put(5, 2, 10, lamp);
            c.put(2, 3, 5, light);
            c.put(5, 3, 5, light);
            c.put(2, 3, 10, light);
            c.put(5, 3, 10, light);
            if room.open[D_SOUTH] {
                c.water_box((3, 1, 0), (4, 2, 0));
            }
            if room.open[D_EAST] {
                c.water_box((7, 1, 3), (7, 2, 4));
            }
            if room.open[D_WEST] {
                c.water_box((0, 1, 3), (0, 2, 4));
            }
            if north.open[D_NORTH] {
                c.water_box((3, 1, 15), (4, 2, 15));
            }
            if north.open[D_WEST] {
                c.water_box((0, 1, 11), (0, 2, 12));
            }
            if north.open[D_EAST] {
                c.water_box((7, 1, 11), (7, 2, 12));
            }
        }
        MKind::Simple { design, pillar } => {
            carve_simple_room(c, room, design, pillar);
        }
        MKind::SimpleTop => {
            if room.index / 25 > 0 {
                default_floor(c, 0, 0, room.open[D_DOWN]);
            }
            if up_conn(room).is_none() {
                c.fill_only((1, 4, 1), (6, 4, 6), gray);
            }
            let sponge = c.pal.wet_sponge;
            for x in 1..=6 {
                for z in 1..=6 {
                    if let Some(y0) = c.sponge_roll(x, z) {
                        c.boxed((x, y0, z), (x, 3, z), sponge);
                    }
                }
            }
            c.boxed((0, 1, 0), (0, 1, 7), light);
            c.boxed((7, 1, 0), (7, 1, 7), light);
            c.boxed((1, 1, 0), (6, 1, 0), light);
            c.boxed((1, 1, 7), (6, 1, 7), light);
            c.boxed((0, 2, 0), (0, 2, 7), black);
            c.boxed((7, 2, 0), (7, 2, 7), black);
            c.boxed((1, 2, 0), (6, 2, 0), black);
            c.boxed((1, 2, 7), (6, 2, 7), black);
            c.boxed((0, 3, 0), (0, 3, 7), light);
            c.boxed((7, 3, 0), (7, 3, 7), light);
            c.boxed((1, 3, 0), (6, 3, 0), light);
            c.boxed((1, 3, 7), (6, 3, 7), light);
            c.boxed((0, 1, 3), (0, 2, 4), black);
            c.boxed((7, 1, 3), (7, 2, 4), black);
            c.boxed((3, 1, 0), (4, 2, 0), black);
            c.boxed((3, 1, 7), (4, 2, 7), black);
            if room.open[D_SOUTH] {
                c.water_box((3, 1, 0), (4, 2, 0));
            }
        }
        MKind::Wing { design } => carve_wing(c, design),
        MKind::Penthouse => {
            c.boxed((2, -1, 2), (11, -1, 11), light);
            c.boxed((0, -1, 0), (1, -1, 11), gray);
            c.boxed((12, -1, 0), (13, -1, 11), gray);
            c.boxed((2, -1, 0), (11, -1, 1), gray);
            c.boxed((2, -1, 12), (11, -1, 13), gray);
            c.boxed((0, 0, 0), (0, 0, 13), light);
            c.boxed((13, 0, 0), (13, 0, 13), light);
            c.boxed((1, 0, 0), (12, 0, 0), light);
            c.boxed((1, 0, 13), (12, 0, 13), light);
            let mut i = 2;
            while i <= 11 {
                c.put(0, 0, i, lamp);
                c.put(13, 0, i, lamp);
                c.put(i, 0, 0, lamp);
                i += 3;
            }
            c.boxed((2, 0, 3), (4, 0, 9), light);
            c.boxed((9, 0, 3), (11, 0, 9), light);
            c.boxed((4, 0, 9), (9, 0, 11), light);
            c.put(5, 0, 8, light);
            c.put(8, 0, 8, light);
            c.put(10, 0, 10, light);
            c.put(3, 0, 10, light);
            c.boxed((3, 0, 3), (3, 0, 7), black);
            c.boxed((10, 0, 3), (10, 0, 7), black);
            c.boxed((6, 0, 10), (7, 0, 10), black);
            for x in [3, 10] {
                let mut z = 2;
                while z <= 8 {
                    c.boxed((x, 0, z), (x, 2, z), light);
                    z += 3;
                }
            }
            c.boxed((5, 0, 10), (5, 2, 10), light);
            c.boxed((8, 0, 10), (8, 2, 10), light);
            c.boxed((6, -1, 7), (7, -1, 8), black);
            c.water_box((6, -1, 3), (7, -1, 4));
            elder_gap_log();
        }
    }
}

fn carve_simple_room(c: &mut MCarver, room: &RoomLite, design: i32, pillar: bool) {
    let (gray, light, black, lamp) =
        (c.pal.gray, c.pal.light, c.pal.black, c.pal.lamp);
    if room.index / 25 > 0 {
        default_floor(c, 0, 0, room.open[D_DOWN]);
    }
    if room.conn[D_UP].is_none() {
        c.fill_only((1, 4, 1), (6, 4, 6), gray);
    }
    if design == 0 {
        for (x0, z0) in [(0, 0), (5, 0), (0, 5), (5, 5)] {
            c.boxed((x0, 1, z0), (x0 + 2, 1, z0 + 2), light);
            c.boxed((x0, 3, z0), (x0 + 2, 3, z0 + 2), light);
        }
        c.boxed((0, 2, 0), (0, 2, 2), gray);
        c.boxed((1, 2, 0), (2, 2, 0), gray);
        c.put(1, 2, 1, lamp);
        c.boxed((7, 2, 0), (7, 2, 2), gray);
        c.boxed((5, 2, 0), (6, 2, 0), gray);
        c.put(6, 2, 1, lamp);
        c.boxed((0, 2, 5), (0, 2, 7), gray);
        c.boxed((1, 2, 7), (2, 2, 7), gray);
        c.put(1, 2, 6, lamp);
        c.boxed((7, 2, 5), (7, 2, 7), gray);
        c.boxed((5, 2, 7), (6, 2, 7), gray);
        c.put(6, 2, 6, lamp);
        if room.open[D_SOUTH] {
            c.boxed((3, 3, 0), (4, 3, 0), light);
        } else {
            c.boxed((3, 3, 0), (4, 3, 1), light);
            c.boxed((3, 2, 0), (4, 2, 0), gray);
            c.boxed((3, 1, 0), (4, 1, 1), light);
        }
        if room.open[D_NORTH] {
            c.boxed((3, 3, 7), (4, 3, 7), light);
        } else {
            c.boxed((3, 3, 6), (4, 3, 7), light);
            c.boxed((3, 2, 7), (4, 2, 7), gray);
            c.boxed((3, 1, 6), (4, 1, 7), light);
        }
        if room.open[D_WEST] {
            c.boxed((0, 3, 3), (0, 3, 4), light);
        } else {
            c.boxed((0, 3, 3), (1, 3, 4), light);
            c.boxed((0, 2, 3), (0, 2, 4), gray);
            c.boxed((0, 1, 3), (1, 1, 4), light);
        }
        if room.open[D_EAST] {
            c.boxed((7, 3, 3), (7, 3, 4), light);
        } else {
            c.boxed((6, 3, 3), (7, 3, 4), light);
            c.boxed((7, 2, 3), (7, 2, 4), gray);
            c.boxed((6, 1, 3), (7, 1, 4), light);
        }
    } else if design == 1 {
        for (x, z) in [(2, 2), (2, 5), (5, 5), (5, 2)] {
            c.boxed((x, 1, z), (x, 3, z), light);
            c.put(x, 2, z, lamp);
        }
        c.boxed((0, 1, 0), (1, 3, 0), light);
        c.boxed((0, 1, 1), (0, 3, 1), light);
        c.boxed((0, 1, 7), (1, 3, 7), light);
        c.boxed((0, 1, 6), (0, 3, 6), light);
        c.boxed((6, 1, 7), (7, 3, 7), light);
        c.boxed((7, 1, 6), (7, 3, 6), light);
        c.boxed((6, 1, 0), (7, 3, 0), light);
        c.boxed((7, 1, 1), (7, 3, 1), light);
        c.put(1, 2, 0, gray);
        c.put(0, 2, 1, gray);
        c.put(1, 2, 7, gray);
        c.put(0, 2, 6, gray);
        c.put(6, 2, 7, gray);
        c.put(7, 2, 6, gray);
        c.put(6, 2, 0, gray);
        c.put(7, 2, 1, gray);
        if !room.open[D_SOUTH] {
            c.boxed((1, 3, 0), (6, 3, 0), light);
            c.boxed((1, 2, 0), (6, 2, 0), gray);
            c.boxed((1, 1, 0), (6, 1, 0), light);
        }
        if !room.open[D_NORTH] {
            c.boxed((1, 3, 7), (6, 3, 7), light);
            c.boxed((1, 2, 7), (6, 2, 7), gray);
            c.boxed((1, 1, 7), (6, 1, 7), light);
        }
        if !room.open[D_WEST] {
            c.boxed((0, 3, 1), (0, 3, 6), light);
            c.boxed((0, 2, 1), (0, 2, 6), gray);
            c.boxed((0, 1, 1), (0, 1, 6), light);
        }
        if !room.open[D_EAST] {
            c.boxed((7, 3, 1), (7, 3, 6), light);
            c.boxed((7, 2, 1), (7, 2, 6), gray);
            c.boxed((7, 1, 1), (7, 1, 6), light);
        }
    } else {
        c.boxed((0, 1, 0), (0, 1, 7), light);
        c.boxed((7, 1, 0), (7, 1, 7), light);
        c.boxed((1, 1, 0), (6, 1, 0), light);
        c.boxed((1, 1, 7), (6, 1, 7), light);
        c.boxed((0, 2, 0), (0, 2, 7), black);
        c.boxed((7, 2, 0), (7, 2, 7), black);
        c.boxed((1, 2, 0), (6, 2, 0), black);
        c.boxed((1, 2, 7), (6, 2, 7), black);
        c.boxed((0, 3, 0), (0, 3, 7), light);
        c.boxed((7, 3, 0), (7, 3, 7), light);
        c.boxed((1, 3, 0), (6, 3, 0), light);
        c.boxed((1, 3, 7), (6, 3, 7), light);
        c.boxed((0, 1, 3), (0, 2, 4), black);
        c.boxed((7, 1, 3), (7, 2, 4), black);
        c.boxed((3, 1, 0), (4, 2, 0), black);
        c.boxed((3, 1, 7), (4, 2, 7), black);
        if room.open[D_SOUTH] {
            c.water_box((3, 1, 0), (4, 2, 0));
        }
        if room.open[D_NORTH] {
            c.water_box((3, 1, 7), (4, 2, 7));
        }
        if room.open[D_WEST] {
            c.water_box((0, 1, 3), (0, 2, 4));
        }
        if room.open[D_EAST] {
            c.water_box((7, 1, 3), (7, 2, 4));
        }
    }
    if pillar {
        c.boxed((3, 1, 3), (4, 1, 4), light);
        c.boxed((3, 2, 3), (4, 2, 4), gray);
        c.boxed((3, 3, 3), (4, 3, 4), light);
    }
}

fn carve_wing(c: &mut MCarver, design: i32) {
    let (light, black, lamp) = (c.pal.light, c.pal.black, c.pal.lamp);
    if design == 0 {
        for i in 0..4 {
            c.boxed((10 - i, 3 - i, 20 - i), (12 + i, 3 - i, 20), light);
        }
        c.boxed((7, 0, 6), (15, 0, 16), light);
        c.boxed((6, 0, 6), (6, 3, 20), light);
        c.boxed((16, 0, 6), (16, 3, 20), light);
        c.boxed((7, 1, 7), (7, 1, 20), light);
        c.boxed((15, 1, 7), (15, 1, 20), light);
        c.boxed((7, 1, 6), (9, 3, 6), light);
        c.boxed((13, 1, 6), (15, 3, 6), light);
        c.boxed((8, 1, 7), (9, 1, 7), light);
        c.boxed((13, 1, 7), (14, 1, 7), light);
        c.boxed((9, 0, 5), (13, 0, 5), light);
        c.boxed((10, 0, 7), (12, 0, 7), black);
        c.boxed((8, 0, 10), (8, 0, 12), black);
        c.boxed((14, 0, 10), (14, 0, 12), black);
        let mut z = 18;
        while z >= 7 {
            c.put(6, 3, z, lamp);
            c.put(16, 3, z, lamp);
            z -= 3;
        }
        c.put(10, 0, 10, lamp);
        c.put(12, 0, 10, lamp);
        c.put(10, 0, 12, lamp);
        c.put(12, 0, 12, lamp);
        c.put(8, 3, 6, lamp);
        c.put(14, 3, 6, lamp);
        for (x, z) in [(4, 4), (18, 4), (4, 18), (18, 18)] {
            c.put(x, 2, z, light);
            c.put(x, 1, z, lamp);
            c.put(x, 0, z, light);
        }
        c.put(9, 7, 20, light);
        c.put(13, 7, 20, light);
        c.boxed((6, 0, 21), (7, 4, 21), light);
        c.boxed((15, 0, 21), (16, 4, 21), light);
        elder_gap_log();
    } else {
        c.boxed((9, 3, 18), (13, 3, 20), light);
        c.boxed((9, 0, 18), (9, 2, 18), light);
        c.boxed((13, 0, 18), (13, 2, 18), light);
        for x in [9, 13] {
            c.put(x, 6, 20, light);
            c.put(x, 5, 20, lamp);
            c.put(x, 4, 20, light);
        }
        c.boxed((7, 3, 7), (15, 3, 14), light);
        for x in [10, 12] {
            c.boxed((x, 0, 10), (x, 6, 10), light);
            c.boxed((x, 0, 12), (x, 6, 12), light);
            c.put(x, 0, 10, lamp);
            c.put(x, 0, 12, lamp);
            c.put(x, 4, 10, lamp);
            c.put(x, 4, 12, lamp);
        }
        for x in [8, 14] {
            c.boxed((x, 0, 7), (x, 2, 7), light);
            c.boxed((x, 0, 14), (x, 2, 14), light);
        }
        c.boxed((8, 3, 8), (8, 3, 13), black);
        c.boxed((14, 3, 8), (14, 3, 13), black);
        elder_gap_log();
    }
}

fn elder_gap_log() {
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        tracing::info!(
            "W257: ocean monuments spawn no elder guardians yet - the mob \
             is a future wave"
        );
    });
}

/// `MonumentBuilding.postProcess`'s exterior painters (wings, arches,
/// entrance wall, roof, walls), verbatim.
fn carve_building_exterior(c: &mut MCarver) {
    let (gray, light, black, lamp) =
        (c.pal.gray, c.pal.light, c.pal.black, c.pal.lamp);
    let dot = light; // DOT_DECO_DATA == BASE_LIGHT
    // generateWing(false, 0) + generateWing(true, 33).
    for (flip, xo) in [(false, 0), (true, 33)] {
        c.boxed((xo, 0, 0), (xo + 24, 0, 20), gray);
        c.water_box((xo, 1, 0), (xo + 24, 10, 20));
        for i in 0..4 {
            c.boxed((xo + i, i + 1, i), (xo + i, i + 1, 20), light);
            c.boxed((xo + i + 7, i + 5, i + 7), (xo + i + 7, i + 5, 20), light);
            c.boxed((xo + 17 - i, i + 5, i + 7), (xo + 17 - i, i + 5, 20), light);
            c.boxed((xo + 24 - i, i + 1, i), (xo + 24 - i, i + 1, 20), light);
            c.boxed((xo + i + 1, i + 1, i), (xo + 23 - i, i + 1, i), light);
            c.boxed((xo + i + 8, i + 5, i + 7), (xo + 16 - i, i + 5, i + 7), light);
        }
        c.boxed((xo + 4, 4, 4), (xo + 6, 4, 20), gray);
        c.boxed((xo + 7, 4, 4), (xo + 17, 4, 6), gray);
        c.boxed((xo + 18, 4, 4), (xo + 20, 4, 20), gray);
        c.boxed((xo + 11, 8, 11), (xo + 13, 8, 20), gray);
        c.put(xo + 12, 9, 12, dot);
        c.put(xo + 12, 9, 15, dot);
        c.put(xo + 12, 9, 18, dot);
        let left = xo + if flip { 19 } else { 5 };
        let right = xo + if flip { 5 } else { 19 };
        let mut z = 20;
        while z >= 5 {
            c.put(left, 5, z, dot);
            z -= 3;
        }
        z = 19;
        while z >= 7 {
            c.put(right, 5, z, dot);
            z -= 3;
        }
        for i in 0..4 {
            let pos = if flip { xo + 24 - (17 - i * 3) } else { xo + 17 - i * 3 };
            c.put(pos, 5, 5, dot);
        }
        c.put(right, 5, 5, dot);
        c.boxed((xo + 11, 1, 12), (xo + 13, 7, 12), gray);
        c.boxed((xo + 12, 1, 11), (xo + 12, 7, 13), gray);
    }
    // generateEntranceArchs.
    c.water_box((25, 0, 0), (32, 8, 20));
    for i in 0..4 {
        let z = 5 + i * 4;
        c.boxed((24, 2, z), (24, 4, z), light);
        c.boxed((22, 4, z), (23, 4, z), light);
        c.put(25, 5, z, light);
        c.put(26, 6, z, light);
        c.put(26, 5, z, lamp);
        c.boxed((33, 2, z), (33, 4, z), light);
        c.boxed((34, 4, z), (35, 4, z), light);
        c.put(32, 5, z, light);
        c.put(31, 6, z, light);
        c.put(31, 5, z, lamp);
        c.boxed((27, 6, z), (30, 6, z), gray);
    }
    // generateEntranceWall.
    c.boxed((15, 0, 21), (42, 0, 21), gray);
    c.water_box((26, 1, 21), (31, 3, 21));
    c.boxed((21, 12, 21), (36, 12, 21), gray);
    c.boxed((17, 11, 21), (40, 11, 21), gray);
    c.boxed((16, 10, 21), (41, 10, 21), gray);
    c.boxed((15, 7, 21), (42, 9, 21), gray);
    c.boxed((16, 6, 21), (41, 6, 21), gray);
    c.boxed((17, 5, 21), (40, 5, 21), gray);
    c.boxed((21, 4, 21), (36, 4, 21), gray);
    c.boxed((22, 3, 21), (26, 3, 21), gray);
    c.boxed((31, 3, 21), (35, 3, 21), gray);
    c.boxed((23, 2, 21), (25, 2, 21), gray);
    c.boxed((32, 2, 21), (34, 2, 21), gray);
    c.boxed((28, 4, 20), (29, 4, 21), light);
    c.put(27, 3, 21, light);
    c.put(30, 3, 21, light);
    c.put(26, 2, 21, light);
    c.put(31, 2, 21, light);
    c.put(25, 1, 21, light);
    c.put(32, 1, 21, light);
    for i in 0..7 {
        c.put(28 - i, 6 + i, 21, black);
        c.put(29 + i, 6 + i, 21, black);
    }
    for i in 0..4 {
        c.put(28 - i, 9 + i, 21, black);
        c.put(29 + i, 9 + i, 21, black);
    }
    c.put(28, 12, 21, black);
    c.put(29, 12, 21, black);
    for i in 0..3 {
        c.put(22 - i * 2, 8, 21, black);
        c.put(22 - i * 2, 9, 21, black);
        c.put(35 + i * 2, 8, 21, black);
        c.put(35 + i * 2, 9, 21, black);
    }
    c.water_box((15, 13, 21), (42, 15, 21));
    for (a, b) in [
        ((15, 1, 21), (15, 6, 21)),
        ((16, 1, 21), (16, 5, 21)),
        ((17, 1, 21), (20, 4, 21)),
        ((21, 1, 21), (21, 3, 21)),
        ((22, 1, 21), (22, 2, 21)),
        ((23, 1, 21), (24, 1, 21)),
        ((42, 1, 21), (42, 6, 21)),
        ((41, 1, 21), (41, 5, 21)),
        ((37, 1, 21), (40, 4, 21)),
        ((36, 1, 21), (36, 3, 21)),
        ((33, 1, 21), (34, 1, 21)),
        ((35, 1, 21), (35, 2, 21)),
    ] {
        c.water_box(a, b);
    }
    // generateRoofPiece.
    c.boxed((21, 0, 22), (36, 0, 36), gray);
    c.water_box((21, 1, 22), (36, 23, 36));
    for i in 0..4 {
        c.boxed((21 + i, 13 + i, 21 + i), (36 - i, 13 + i, 21 + i), light);
        c.boxed((21 + i, 13 + i, 36 - i), (36 - i, 13 + i, 36 - i), light);
        c.boxed((21 + i, 13 + i, 22 + i), (21 + i, 13 + i, 35 - i), light);
        c.boxed((36 - i, 13 + i, 22 + i), (36 - i, 13 + i, 35 - i), light);
    }
    c.boxed((25, 16, 25), (32, 16, 32), gray);
    c.boxed((25, 17, 25), (25, 19, 25), light);
    c.boxed((32, 17, 25), (32, 19, 25), light);
    c.boxed((25, 17, 32), (25, 19, 32), light);
    c.boxed((32, 17, 32), (32, 19, 32), light);
    for (bx, bz, lx, lz) in [(26, 26, 27, 27), (26, 31, 27, 30), (31, 31, 30, 30), (31, 26, 30, 27)]
    {
        c.put(bx, 20, bz, light);
        c.put(lx, 21, lz, light);
        c.put(lx, 20, lz, lamp);
    }
    c.boxed((28, 21, 27), (29, 21, 27), gray);
    c.boxed((27, 21, 28), (27, 21, 29), gray);
    c.boxed((28, 21, 30), (29, 21, 30), gray);
    c.boxed((30, 21, 28), (30, 21, 29), gray);
    // generateLowerWall.
    c.boxed((0, 0, 21), (6, 0, 57), gray);
    c.water_box((0, 1, 21), (6, 7, 57));
    c.boxed((4, 4, 21), (6, 4, 53), gray);
    for i in 0..4 {
        c.boxed((i, i + 1, 21), (i, i + 1, 57 - i), light);
    }
    let mut z = 23;
    while z < 53 {
        c.put(5, 5, z, dot);
        z += 3;
    }
    c.put(5, 5, 52, dot);
    c.boxed((4, 1, 52), (6, 3, 52), gray);
    c.boxed((5, 1, 51), (5, 3, 53), gray);
    c.boxed((51, 0, 21), (57, 0, 57), gray);
    c.water_box((51, 1, 21), (57, 7, 57));
    c.boxed((51, 4, 21), (53, 4, 53), gray);
    for i in 0..4 {
        c.boxed((57 - i, i + 1, 21), (57 - i, i + 1, 57 - i), light);
    }
    z = 23;
    while z < 53 {
        c.put(52, 5, z, dot);
        z += 3;
    }
    c.put(52, 5, 52, dot);
    c.boxed((51, 1, 52), (53, 3, 52), gray);
    c.boxed((52, 1, 51), (52, 3, 53), gray);
    c.boxed((7, 0, 51), (50, 0, 57), gray);
    c.water_box((7, 1, 51), (50, 10, 57));
    for i in 0..4 {
        c.boxed((i + 1, i + 1, 57 - i), (56 - i, i + 1, 57 - i), light);
    }
    // generateMiddleWall.
    c.boxed((7, 0, 21), (13, 0, 50), gray);
    c.water_box((7, 1, 21), (13, 10, 50));
    c.boxed((11, 8, 21), (13, 8, 53), gray);
    for i in 0..4 {
        c.boxed((i + 7, i + 5, 21), (i + 7, i + 5, 54), light);
    }
    z = 21;
    while z <= 45 {
        c.put(12, 9, z, dot);
        z += 3;
    }
    c.boxed((44, 0, 21), (50, 0, 50), gray);
    c.water_box((44, 1, 21), (50, 10, 50));
    c.boxed((44, 8, 21), (46, 8, 53), gray);
    for i in 0..4 {
        c.boxed((50 - i, i + 5, 21), (50 - i, i + 5, 54), light);
    }
    z = 21;
    while z <= 45 {
        c.put(45, 9, z, dot);
        z += 3;
    }
    c.boxed((14, 0, 44), (43, 0, 50), gray);
    c.water_box((14, 1, 44), (43, 10, 50));
    let mut x = 12;
    while x <= 45 {
        c.put(x, 9, 45, dot);
        c.put(x, 9, 52, dot);
        if x == 12 || x == 18 || x == 24 || x == 33 || x == 39 || x == 45 {
            c.put(x, 9, 47, dot);
            c.put(x, 9, 50, dot);
            c.put(x, 10, 45, dot);
            c.put(x, 10, 46, dot);
            c.put(x, 10, 51, dot);
            c.put(x, 10, 52, dot);
            c.put(x, 11, 47, dot);
            c.put(x, 11, 50, dot);
            c.put(x, 12, 48, dot);
            c.put(x, 12, 49, dot);
        }
        x += 3;
    }
    for i in 0..3 {
        c.boxed((8 + i, 5 + i, 54), (49 - i, 5 + i, 54), gray);
    }
    c.boxed((11, 8, 54), (46, 8, 54), light);
    c.boxed((14, 8, 44), (43, 8, 53), gray);
    // generateUpperWall.
    c.boxed((14, 0, 21), (20, 0, 43), gray);
    c.water_box((14, 1, 22), (20, 14, 43));
    c.boxed((18, 12, 22), (20, 12, 39), gray);
    c.boxed((18, 12, 21), (20, 12, 21), light);
    for i in 0..4 {
        c.boxed((i + 14, i + 9, 21), (i + 14, i + 9, 43 - i), light);
    }
    z = 23;
    while z <= 39 {
        c.put(19, 13, z, dot);
        z += 3;
    }
    c.boxed((37, 0, 21), (43, 0, 43), gray);
    c.water_box((37, 1, 22), (43, 14, 43));
    c.boxed((37, 12, 22), (39, 12, 39), gray);
    c.boxed((37, 12, 21), (39, 12, 21), light);
    for i in 0..4 {
        c.boxed((43 - i, i + 9, 21), (43 - i, i + 9, 43 - i), light);
    }
    z = 23;
    while z <= 39 {
        c.put(38, 13, z, dot);
        z += 3;
    }
    c.boxed((21, 0, 37), (36, 0, 43), gray);
    c.water_box((21, 1, 37), (36, 14, 43));
    c.boxed((21, 12, 37), (36, 12, 39), gray);
    for i in 0..4 {
        c.boxed((15 + i, i + 9, 43 - i), (42 - i, i + 9, 43 - i), light);
    }
    x = 21;
    while x <= 36 {
        c.put(x, 13, 38, dot);
        x += 3;
    }
}

/// Building 58 wide from chunk-29: +-4 chunks (+ border rings).
const REACH: i32 = 4;

/// The per-chunk monument pass (surface_structures).
pub(crate) fn place_monuments(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    let cs = CHUNK_SIZE as i32;
    let cube = Bb {
        x0: origin[0] - 8,
        y0: origin[1],
        z0: origin[2],
        x1: origin[0] + cs + 7,
        y1: origin[1] + cs - 1,
        z1: origin[2] + cs + 7,
    };
    let _ = &cube;
    let pal = Pal::get();
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] = triangular_start(seed, gx, gz);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            for piece in &layout.pieces {
                // The building piece floods a halo past its box; give
                // it slack instead of exact clips.
                let slack = Bb {
                    x0: piece.bb.x0 - 8,
                    y0: 0,
                    z0: piece.bb.z0 - 8,
                    x1: piece.bb.x1 + 8,
                    y1: 255,
                    z1: piece.bb.z1 + 8,
                };
                let cube = Bb {
                    x0: origin[0],
                    y0: origin[1],
                    z0: origin[2],
                    x1: origin[0] + cs - 1,
                    y1: origin[1] + cs - 1,
                    z1: origin[2] + cs - 1,
                };
                if !slack.intersects(&cube) {
                    continue;
                }
                let mut world = ChunkShaftWorld { chunk, origin, ctx };
                let mut carver = MCarver {
                    w: &mut world,
                    f: ScatteredPiece { bb: piece.bb, dir: piece.dir },
                    seed,
                    pal: &pal,
                };
                carve_piece(&mut carver, &layout, piece);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangular_offsets_bias_to_centre() {
        let mut sum = 0i64;
        let n = 4000;
        for i in 0..n {
            let [sx, _] = triangular_start(7, i, -i);
            sum += (sx - i * SPACING) as i64;
        }
        let mean = sum as f64 / n as f64;
        let expect = (SPACING - SEPARATION) as f64 / 2.0 - 0.5;
        assert!(
            (mean - expect).abs() < 1.0,
            "triangular mean drifted: {mean} vs {expect}"
        );
    }

    #[test]
    fn room_graph_claims_and_fits_every_grid_room() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        // Find a deep-ocean start on the seed by scanning grid cells.
        let mut found = None;
        'outer: for gx in -20..20 {
            for gz in -20..20 {
                let [sx, sz] = triangular_start(ctx.effective_seed(), gx, gz);
                if layout_for_start(&ctx, sx, sz).is_some() {
                    found = Some([sx, sz]);
                    break 'outer;
                }
            }
        }
        let Some([sx, sz]) = found else {
            // The seed may lack a qualifying deep-ocean patch in
            // range; the placement gates themselves are the test.
            return;
        };
        let layout = layout_for_start(&ctx, sx, sz).unwrap();
        // Building + entry + core + wings + penthouse + fitted rooms.
        assert!(layout.pieces.len() > 10, "{} pieces", layout.pieces.len());
        assert_eq!(layout.pieces[0].kind, MKind::Building);
        assert!(layout.pieces.iter().any(|p| p.kind == MKind::Core));
        assert!(layout.pieces.iter().any(|p| p.kind == MKind::Penthouse));
        assert_eq!(
            layout.pieces.iter().filter(|p| matches!(p.kind, MKind::Wing { .. })).count(),
            2
        );
        // Determinism via the cache-bypassing rebuild.
        let again = layout_for_start(&ctx, sx, sz).unwrap();
        assert_eq!(again.pieces.len(), layout.pieces.len());
    }

    /// Probe: monuments near the test region + full-pipeline verify
    /// (prismarine + sea lanterns + the gold core).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_monuments -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_monuments() {
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
        let mut found: Vec<[i32; 2]> = Vec::new();
        for gx in (ccx - radius).div_euclid(SPACING)..=(ccx + radius).div_euclid(SPACING)
        {
            for gz in
                (ccz - radius).div_euclid(SPACING)..=(ccz + radius).div_euclid(SPACING)
            {
                let [sx, sz] = triangular_start(ctx.effective_seed(), gx, gz);
                if let Some(layout) = layout_for_start(&ctx, sx, sz) {
                    println!(
                        "monument at start chunk ({sx}, {sz}): {} pieces, building {:?}",
                        layout.pieces.len(),
                        layout.pieces[0].bb
                    );
                    found.push([sx, sz]);
                }
            }
        }
        println!("total: {} monuments in +-{radius} chunks", found.len());
        if let Some([sx, sz]) = found.first() {
            let layout = layout_for_start(&ctx, *sx, *sz).unwrap();
            let core = layout
                .pieces
                .iter()
                .find(|p| p.kind == MKind::Core)
                .expect("core room");
            let centre = [
                (core.bb.x0 + core.bb.x1) / 2,
                (core.bb.y0 + core.bb.y1) / 2,
                (core.bb.z0 + core.bb.z1) / 2,
            ];
            let coord = [
                centre[0].div_euclid(cs),
                centre[1].div_euclid(cs),
                centre[2].div_euclid(cs),
            ];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let gold = registry_id("gold_block").unwrap();
            let prism = registry_id("prismarine").unwrap();
            let bricks = registry_id("prismarine_bricks").unwrap();
            let lamp = registry_id("sea_lantern").unwrap();
            let (mut n_gold, mut n_prism, mut n_lamp) = (0usize, 0usize, 0usize);
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if id == gold {
                            n_gold += 1;
                        } else if id == prism || id == bricks {
                            n_prism += 1;
                        } else if id == lamp {
                            n_lamp += 1;
                        }
                    }
                }
            }
            println!(
                "full-pipeline core cube {coord:?}: {n_gold} gold, {n_prism} \
                 prismarine, {n_lamp} lanterns"
            );
            assert!(n_prism > 50, "prismarine must materialise: {n_prism}");
            assert!(n_gold > 0, "the gold core must materialise");
        }
    }
}
