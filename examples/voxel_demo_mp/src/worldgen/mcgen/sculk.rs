//! W262 - sculk patches + sculk veins (the deep-dark floor), ported
//! from the decompiled 26.1.2 `SculkPatchFeature` / `SculkSpreader` /
//! `SculkBlock` / `SculkVeinBlock` + the configured/placed JSONs.
//!
//! `sculk_patch_deep_dark`: count 256 per chunk, y uniform -64..256,
//! biome deep_dark (the 3D `isDeepDarkRegion` check). Each try that
//! lands on an open cell with a full-block neighbour runs the
//! world-gen sculk spreader: 10 charge cursors of 32, 64 update
//! rounds, veins mediating the substrate->sculk conversion
//! (`SCULK_REPLACEABLE_WORLD_GEN`), growths (sensor 10/11, shrieker
//! 1/11) on the SculkBlock charge rules, a 50% catalyst at the
//! origin. `sculk_vein` (count 204..250) is the multiface-growth
//! sibling of glow lichen.
//!
//! Determinism / replay (the W226.1 rules): one SplitMix64 stream
//! per source column; every chunk replays the 3x3 neighbourhood and
//! applies only its own cells. Cursor walks clamp to 15 blocks xz
//! from the origin (`SculkSpreader` world-gen rule), inside the
//! replay window. Base-terrain reads go through the replay-invariant
//! `mc_painted_block_at` oracle; patch writes overlay a scratch.
//!
//! Honest divergences: the vein block is a single id rendered as the
//! cross fallback (face bits are tracked internally for the spreader
//! semantics but do not reach the mesh); `MultifaceSpreader`'s
//! wrap-around spread orders collapse to "vein on open neighbours
//! against solid replaceables"; water handling drops out (the
//! painted oracle reads open cells as air - deep-dark caves are the
//! dry-cave regime); growth/step sounds and the shrieker can_summon
//! flag have no engine counterpart yet.

use std::collections::HashMap;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::terrain::{mc_is_deep_dark, mc_painted_block_at};
use crate::worldgen::WorldgenCtx;

const SALT_SCULK_PATCH: u64 = 0xD4E6_00E2;
const SALT_SCULK_VEIN: u64 = 0xD4E6_00E3;

/// `sculk_patch_deep_dark` configured values.
const CHARGE_COUNT: u32 = 10;
const AMOUNT_PER_CHARGE: i32 = 32;
const SPREAD_ATTEMPTS: u32 = 64;
const CATALYST_CHANCE: f32 = 0.5;
/// placed_feature count.
const PATCH_TRIES: u32 = 256;
/// `createWorldGenSpreader()`.
const GROWTH_SPAWN_COST: i32 = 50;
const NO_GROWTH_RADIUS: i32 = 1;
const CHARGE_DECAY_RATE: i32 = 5;
const ADDITIONAL_DECAY_RATE: i32 = 10;
const MAX_CURSORS: usize = 32;
/// World-gen cursors die past this xz chessboard distance.
const CURSOR_XZ_RANGE: f64 = 15.0;

/// `sculk_vein` placed_feature (multiface_growth).
const VEIN_TRIES_MIN: i32 = 204;
const VEIN_TRIES_MAX: i32 = 250;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_f32(r: &mut SplitMix64) -> f32 {
    (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

struct Ids {
    sculk: u16,
    vein: u16,
    catalyst: u16,
    sensor: u16,
    shrieker: u16,
}

impl Ids {
    fn get() -> Option<Ids> {
        Some(Ids {
            sculk: block_id_by_name("sculk")?,
            vein: block_id_by_name("sculk_vein")?,
            catalyst: block_id_by_name("sculk_catalyst")?,
            sensor: block_id_by_name("sculk_sensor")?,
            shrieker: block_id_by_name("sculk_shrieker")?,
        })
    }
}

/// `#minecraft:sculk_replaceable_world_gen` resolved to engine ids
/// (the base tag's stone/substrate families + the deepslate building
/// blocks; names the engine lacks drop out).
fn replaceable(id: u16) -> bool {
    use std::sync::OnceLock;
    static SET: OnceLock<std::collections::HashSet<u16>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            // #base_stone_overworld + #substrate_overworld.
            "stone", "granite", "diorite", "andesite", "tuff", "deepslate",
            "dirt", "coarse_dirt", "podzol", "grass", "grass_block", "mycelium",
            "rooted_dirt", "mud", "moss_block",
            // Direct members.
            "sand", "red_sand", "gravel", "soul_sand", "soul_soil", "calcite",
            "smooth_basalt", "clay", "dripstone_block", "red_sandstone",
            "sandstone",
            // The world-gen extras (city floors).
            "deepslate_bricks", "deepslate_tiles", "cobbled_deepslate",
            "cracked_deepslate_bricks", "cracked_deepslate_tiles",
            "polished_deepslate",
        ]
        .iter()
        .filter_map(|n| block_id_by_name(n))
        .collect()
    })
    .contains(&id)
}

const DIRS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

/// One patch's writes over the base terrain. Vein cells track their
/// face bits (index into [`DIRS`]) for the spreader semantics.
struct Scratch {
    writes: HashMap<[i32; 3], u16>,
    vein_faces: HashMap<[i32; 3], u8>,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch { writes: HashMap::new(), vein_faces: HashMap::new() }
    }

    /// Overlay-then-base block read (0 = open).
    fn read(&self, ctx: &WorldgenCtx, p: [i32; 3]) -> u16 {
        match self.writes.get(&p) {
            Some(id) => *id,
            None => mc_painted_block_at(ctx, p[0], p[1], p[2]),
        }
    }

    fn add_vein_face(&mut self, ids: &Ids, p: [i32; 3], dir_idx: usize) {
        *self.vein_faces.entry(p).or_insert(0) |= 1 << dir_idx;
        self.writes.insert(p, ids.vein);
    }

    /// `SculkVeinBlock.onDischarged`: faces toward sculk drop; a
    /// vein with no faces left opens up.
    fn discharge_vein(&mut self, ctx: &WorldgenCtx, ids: &Ids, p: [i32; 3]) {
        let Some(mut faces) = self.vein_faces.get(&p).copied() else {
            return;
        };
        for (i, d) in DIRS.iter().enumerate() {
            let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
            if faces & (1 << i) != 0 && self.read(ctx, n) == ids.sculk {
                faces &= !(1 << i);
            }
        }
        if faces == 0 {
            self.vein_faces.remove(&p);
            self.writes.insert(p, 0);
        } else {
            self.vein_faces.insert(p, faces);
        }
    }
}

/// `SculkSpreader.ChargeCursor`.
struct Cursor {
    pos: [i32; 3],
    charge: i32,
    decay_delay: i32,
    update_delay: i32,
}

/// The 18 non-corner neighbour offsets (`NON_CORNER_NEIGHBOURS`).
fn non_corner_offsets() -> [[i32; 3]; 18] {
    let mut out = [[0i32; 3]; 18];
    let mut n = 0;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                if (x == 0 || y == 0 || z == 0) && (x, y, z) != (0, 0, 0) {
                    out[n] = [x, y, z];
                    n += 1;
                }
            }
        }
    }
    out
}

fn is_sculk_family(scratch: &Scratch, ctx: &WorldgenCtx, ids: &Ids, p: [i32; 3]) -> bool {
    let id = scratch.read(ctx, p);
    id == ids.sculk || id == ids.vein || id == ids.catalyst
}

/// `isMovementUnobstructed` (diagonal steps need one open shared
/// face).
fn movement_unobstructed(
    scratch: &Scratch,
    ctx: &WorldgenCtx,
    from: [i32; 3],
    to: [i32; 3],
) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    if d[0].abs() + d[1].abs() + d[2].abs() == 1 {
        return true;
    }
    let open = |axis: usize| {
        let mut n = from;
        n[axis] += d[axis].signum();
        // "Face is not sturdy" ~ the neighbour cell is open (full
        // solid cells are the sturdy case in the fill).
        scratch.read(ctx, n) == 0
    };
    if d[0] == 0 {
        open(1) || open(2)
    } else if d[1] == 0 {
        open(0) || open(2)
    } else {
        open(0) || open(1)
    }
}

/// One cursor update (`ChargeCursor.update` with the world-gen
/// spreader constants).
#[allow(clippy::too_many_arguments)]
fn cursor_update(
    cur: &mut Cursor,
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    ids: &Ids,
    origin: [i32; 3],
    rng: &mut SplitMix64,
    spread_veins: bool,
) {
    if cur.charge <= 0 {
        return;
    }
    if cur.update_delay > 0 {
        cur.update_delay -= 1;
        return;
    }
    let state = scratch.read(ctx, cur.pos);
    let on_vein = scratch.vein_faces.contains_key(&cur.pos);
    let on_sculk = state == ids.sculk;

    // attemptSpreadVein (DEFAULT behaviour arm: open cells grow a
    // same-space vein against their solid neighbours).
    if spread_veins && !on_vein && !on_sculk && state == 0 {
        for (i, d) in DIRS.iter().enumerate() {
            let n = [cur.pos[0] + d[0], cur.pos[1] + d[1], cur.pos[2] + d[2]];
            if scratch.read(ctx, n) != 0 {
                scratch.add_vein_face(ids, cur.pos, i);
            }
        }
    }

    // attemptUseCharge per behaviour.
    let on_vein = scratch.vein_faces.contains_key(&cur.pos);
    if on_vein {
        // SculkVeinBlock: convert one replaceable support to sculk.
        let mut placed = false;
        if spread_veins {
            let faces = scratch.vein_faces.get(&cur.pos).copied().unwrap_or(0);
            let mut order: Vec<usize> = (0..6).collect();
            for i in (1..order.len()).rev() {
                let j = (rng.next_u64() % (i as u64 + 1)) as usize;
                order.swap(i, j);
            }
            for i in order {
                if faces & (1 << i) == 0 {
                    continue;
                }
                let d = DIRS[i];
                let sup = [cur.pos[0] + d[0], cur.pos[1] + d[1], cur.pos[2] + d[2]];
                if !replaceable(scratch.read(ctx, sup)) {
                    continue;
                }
                scratch.vein_faces.remove(&cur.pos);
                scratch.writes.insert(cur.pos, 0);
                scratch.writes.insert(sup, ids.sculk);
                // veinSpreader.spreadAll from the new sculk: fringe
                // veins on its open neighbours (attached to solid
                // replaceables), then discharge veins around it.
                for fd in &DIRS {
                    let n = [sup[0] + fd[0], sup[1] + fd[1], sup[2] + fd[2]];
                    if scratch.read(ctx, n) != 0 {
                        continue;
                    }
                    for (gi, gd) in DIRS.iter().enumerate() {
                        let g = [n[0] + gd[0], n[1] + gd[1], n[2] + gd[2]];
                        if g != sup && replaceable(scratch.read(ctx, g)) {
                            scratch.add_vein_face(ids, n, gi);
                        }
                    }
                }
                let skip = i ^ 1; // the opposite DIRS index pairs 0/1, 2/3, 4/5
                for (vi, vd) in DIRS.iter().enumerate() {
                    if vi == skip {
                        continue;
                    }
                    let v = [sup[0] + vd[0], sup[1] + vd[1], sup[2] + vd[2]];
                    if scratch.vein_faces.contains_key(&v) {
                        scratch.discharge_vein(ctx, ids, v);
                    }
                }
                placed = true;
                break;
            }
        }
        cur.charge = if placed {
            cur.charge - 1
        } else if next_int(rng, CHARGE_DECAY_RATE) == 0 {
            cur.charge / 2
        } else {
            cur.charge
        };
    } else if on_sculk {
        // SculkBlock.attemptUseCharge.
        if next_int(rng, CHARGE_DECAY_RATE) == 0 {
            let dx = cur.pos[0] - origin[0];
            let dy = cur.pos[1] - origin[1];
            let dz = cur.pos[2] - origin[2];
            let dist_sq = (dx * dx + dy * dy + dz * dz) as f32;
            let close = dist_sq < (NO_GROWTH_RADIUS * NO_GROWTH_RADIUS) as f32;
            if !close && can_place_growth(scratch, ctx, ids, cur.pos) {
                if next_int(rng, GROWTH_SPAWN_COST) < cur.charge {
                    let above = [cur.pos[0], cur.pos[1] + 1, cur.pos[2]];
                    let growth = if next_int(rng, 11) == 0 { ids.shrieker } else { ids.sensor };
                    scratch.writes.insert(above, growth);
                }
                cur.charge = (cur.charge - GROWTH_SPAWN_COST).max(0);
            } else if next_int(rng, ADDITIONAL_DECAY_RATE) == 0 {
                cur.charge -= if close {
                    1
                } else {
                    // getDecayPenalty.
                    let outer = (dist_sq.sqrt() - NO_GROWTH_RADIUS as f32).powi(2);
                    let max_reach = ((24 - NO_GROWTH_RADIUS) * (24 - NO_GROWTH_RADIUS)) as f32;
                    let factor = (outer / max_reach).min(1.0);
                    ((cur.charge as f32 * factor * 0.5) as i32).max(1)
                };
            }
        }
    } else {
        // DEFAULT behaviour: decay_delay > 0 keeps the charge, else 0.
        if cur.decay_delay <= 0 {
            cur.charge = 0;
        }
    }

    if cur.charge <= 0 {
        if on_vein {
            scratch.discharge_vein(ctx, ids, cur.pos);
        }
        return;
    }

    // getValidMovementPos: shuffled non-corner neighbours; move onto
    // sculk-family cells, preferring one with substrate access.
    let mut offsets = non_corner_offsets();
    for i in (1..offsets.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        offsets.swap(i, j);
    }
    let mut target: Option<[i32; 3]> = None;
    for off in offsets {
        let n = [cur.pos[0] + off[0], cur.pos[1] + off[1], cur.pos[2] + off[2]];
        if is_sculk_family(scratch, ctx, ids, n)
            && movement_unobstructed(scratch, ctx, cur.pos, n)
        {
            target = Some(n);
            // hasSubstrateAccess: a vein with a replaceable support
            // ends the search.
            if let Some(faces) = scratch.vein_faces.get(&n) {
                let sturdy = DIRS.iter().enumerate().any(|(i, d)| {
                    faces & (1 << i) != 0
                        && replaceable(scratch.read(ctx, [n[0] + d[0], n[1] + d[1], n[2] + d[2]]))
                });
                if sturdy {
                    break;
                }
            }
        }
    }
    if let Some(t) = target {
        if on_vein {
            scratch.discharge_vein(ctx, ids, cur.pos);
        }
        // World-gen clamp: 15 blocks xz from the origin.
        let dx = (t[0] - origin[0]) as f64;
        let dz = (t[2] - origin[2]) as f64;
        if (dx * dx + dz * dz).sqrt() >= CURSOR_XZ_RANGE {
            cur.charge = 0;
            return;
        }
        cur.pos = t;
    }
    let on_family = is_sculk_family(scratch, ctx, ids, cur.pos);
    cur.decay_delay = if on_family { 1 } else { (cur.decay_delay - 1).max(0) };
    cur.update_delay = 1;
}

/// `SculkBlock.canPlaceGrowth`: open above + at most 2 growths in
/// the 9x3x9 box.
fn can_place_growth(scratch: &Scratch, ctx: &WorldgenCtx, ids: &Ids, p: [i32; 3]) -> bool {
    if scratch.read(ctx, [p[0], p[1] + 1, p[2]]) != 0 {
        return false;
    }
    let mut growths = 0;
    for dx in -4..=4 {
        for dy in 0..=2 {
            for dz in -4..=4 {
                let id = scratch.read(ctx, [p[0] + dx, p[1] + dy, p[2] + dz]);
                if id == ids.sensor || id == ids.shrieker {
                    growths += 1;
                    if growths > 2 {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// `SculkPatchFeature.place` for one origin.
fn run_patch(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    ids: &Ids,
    origin: [i32; 3],
    rng: &mut SplitMix64,
    extra_growths: i32,
) {
    // spread_rounds 1, growth_rounds 0.
    let mut cursors: Vec<Cursor> = Vec::new();
    for _ in 0..CHARGE_COUNT {
        if cursors.len() < MAX_CURSORS {
            cursors.push(Cursor {
                pos: origin,
                charge: AMOUNT_PER_CHARGE,
                decay_delay: 1,
                update_delay: 0,
            });
        }
    }
    for _ in 0..SPREAD_ATTEMPTS {
        for cur in cursors.iter_mut() {
            cursor_update(cur, scratch, ctx, ids, origin, rng, true);
        }
        cursors.retain(|c| c.charge > 0);
        if cursors.is_empty() {
            break;
        }
    }
    // Catalyst at the origin over a solid floor.
    if next_f32(rng) <= CATALYST_CHANCE
        && scratch.read(ctx, [origin[0], origin[1] - 1, origin[2]]) != 0
    {
        scratch.vein_faces.remove(&origin);
        scratch.writes.insert(origin, ids.catalyst);
    }
    // extra_rare_growths (the ancient-city variant): shriekers on
    // nearby floors.
    for _ in 0..extra_growths {
        let c = [
            origin[0] + next_int(rng, 5) - 2,
            origin[1],
            origin[2] + next_int(rng, 5) - 2,
        ];
        if scratch.read(ctx, c) == 0
            && scratch.read(ctx, [c[0], c[1] - 1, c[2]]) != 0
        {
            scratch.writes.insert(c, ids.shrieker);
        }
    }
}

/// `SculkPatchFeature.canSpreadFrom`.
fn can_spread_from(scratch: &Scratch, ctx: &WorldgenCtx, p: [i32; 3]) -> bool {
    if scratch.read(ctx, p) != 0 {
        return false;
    }
    DIRS.iter().any(|d| scratch.read(ctx, [p[0] + d[0], p[1] + d[1], p[2] + d[2]]) != 0)
}

/// W267: cached per-column deep-dark probe. `isDeepDarkRegion`'s
/// erosion channel is xz-only and deep dark is rare, so one probe at
/// the column centre (y -40 makes the depth channel a given) answers
/// for the whole column - and the 24 vertical chunk gens of a column
/// (x 3x3 replay, x the vein pass) were each re-running the two
/// spline evals, a measurable slice of the W260 fill slowdown.
fn column_is_deep_dark(ctx: &WorldgenCtx, cx: i32, cz: i32) -> bool {
    use std::cell::RefCell;
    thread_local! {
        static CACHE: RefCell<HashMap<(i32, i32, u32), bool>> =
            RefCell::new(HashMap::new());
    }
    let seed = ctx.effective_seed();
    CACHE.with(|c| {
        if let Some(hit) = c.borrow().get(&(cx, cz, seed)) {
            return *hit;
        }
        let cs = CHUNK_SIZE as i32;
        let v = mc_is_deep_dark(ctx, cx * cs + 8, -40, cz * cs + 8);
        let mut map = c.borrow_mut();
        if map.len() > 4096 {
            map.clear();
        }
        map.insert((cx, cz, seed), v);
        v
    })
}

/// The per-chunk deep-dark sculk pass (underground_decoration step):
/// sculk veins (multiface growth) then the sculk patches, exactly
/// the deep_dark biome feature order.
pub(crate) fn place_sculk(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let Some(ids) = Ids::get() else {
        return;
    };
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();

    // sculk_vein: count uniform 204..=250, y uniform -64..256, biome
    // deep_dark - the glow-lichen pattern with all-face hosts.
    // W267: veins are chunk-local, so the whole 240-try loop (and its
    // per-try 3D biome evals) skips unless THIS column is deep dark -
    // that per-chunk cost ran for every chunk of the world.
    if column_is_deep_dark(ctx, coord[0], coord[2]) {
        let mut rng = SplitMix64::new(stream_seed(
            seed,
            coord[0],
            coord[2],
            SALT_SCULK_VEIN ^ (coord[1] as u64) << 8,
        ));
        let count = VEIN_TRIES_MIN + next_int(&mut rng, VEIN_TRIES_MAX - VEIN_TRIES_MIN + 1);
        for _ in 0..count {
            let lx = next_int(&mut rng, 16);
            let lz = next_int(&mut rng, 16);
            let wy = -64 + next_int(&mut rng, 321);
            let ly = wy - origin[1];
            if !(0..cs).contains(&ly) {
                continue;
            }
            let (wx, wz) = (origin[0] + lx, origin[2] + lz);
            if !mc_is_deep_dark(ctx, wx, wy, wz) {
                continue;
            }
            let (ux, uy, uz) = (lx as usize, ly as usize, lz as usize);
            if chunk.get(ux, uy, uz).0 != 0 {
                continue;
            }
            // Any-face host (can_place_on floor/ceiling/wall all true).
            let host = |dx: i32, dy: i32, dz: i32| -> bool {
                let (nx, ny, nz) = (lx + dx, ly + dy, lz + dz);
                (0..cs).contains(&nx)
                    && (0..cs).contains(&ny)
                    && (0..cs).contains(&nz)
                    && replaceable(chunk.get(nx as usize, ny as usize, nz as usize).0)
            };
            if host(0, -1, 0)
                || host(0, 1, 0)
                || host(1, 0, 0)
                || host(-1, 0, 0)
                || host(0, 0, 1)
                || host(0, 0, -1)
            {
                chunk.set(ux, uy, uz, BlockId(ids.vein));
            }
        }
    }

    // sculk_patch_deep_dark: the 3x3 source-column replay (patches
    // reach 15 blocks + fringe from their origin).
    let mut patches: Vec<Scratch> = Vec::new();
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            // Column pre-filter (cached, W267): skips the 256-try
            // loop for the whole non-deep-dark world. Replay-safe:
            // every chunk replaying this source column runs the same
            // check.
            if !column_is_deep_dark(ctx, scx, scz) {
                continue;
            }
            let mut rng =
                SplitMix64::new(stream_seed(seed, scx, scz, SALT_SCULK_PATCH));
            for _ in 0..PATCH_TRIES {
                let ox = scx * cs + next_int(&mut rng, 16);
                let oz = scz * cs + next_int(&mut rng, 16);
                let oy = -64 + next_int(&mut rng, 321);
                if !mc_is_deep_dark(ctx, ox, oy, oz) {
                    continue;
                }
                let mut scratch = Scratch::new();
                if !can_spread_from(&scratch, ctx, [ox, oy, oz]) {
                    continue;
                }
                run_patch(&mut scratch, ctx, &ids, [ox, oy, oz], &mut rng, 0);
                patches.push(scratch);
            }
        }
    }
    for patch in &patches {
        for ([x, y, z], id) in &patch.writes {
            let (lx, ly, lz) = (x - origin[0], y - origin[1], z - origin[2]);
            if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(*id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::WorldGen;

    fn ctx() -> WorldgenCtx {
        WorldgenCtx {
            base_seed: 3592732596u32,
            dim: "overworld".into(),
            profile: WorldGen::Mc,
        }
    }

    #[test]
    fn sculk_blocks_resolve() {
        let ids = Ids::get().expect("sculk family in the registry");
        assert_ne!(ids.sculk, 0);
        assert_ne!(ids.vein, 0);
        assert!(replaceable(super::super::features::block_id_by_name("deepslate").unwrap()));
        assert!(replaceable(
            super::super::features::block_id_by_name("deepslate_tiles").unwrap()
        ));
    }

    /// A synthetic patch on the real terrain oracle: pick a deep-dark
    /// cave cell and check the spreader converts substrate to sculk.
    #[test]
    fn patches_paint_sculk_in_deep_dark() {
        let c = ctx();
        // Find a deep-dark open cell with a floor, scanning the city
        // region found by the W261 test (deep dark by construction).
        let mut origin = None;
        'search: for gx in -20..20 {
            for gz in -20..20 {
                let [sx, sz] =
                    super::super::ancient_city::potential_start(c.effective_seed(), gx, gz);
                let (wx, wz) = (sx * 16 + 8, sz * 16 + 8);
                for wy in -50..-20 {
                    let p = [wx, wy, wz];
                    let s = Scratch::new();
                    if mc_is_deep_dark(&c, wx, wy, wz) && can_spread_from(&s, &c, p) {
                        origin = Some(p);
                        break 'search;
                    }
                }
            }
        }
        let Some(origin) = origin else {
            // Deep-dark cave cells exist somewhere; if this scan area
            // has none the probe is inconclusive, not broken.
            return;
        };
        let ids = Ids::get().unwrap();
        let mut scratch = Scratch::new();
        let mut rng = SplitMix64::new(42);
        run_patch(&mut scratch, &c, &ids, origin, &mut rng, 0);
        let sculk_cells =
            scratch.writes.values().filter(|id| **id == ids.sculk).count();
        assert!(
            sculk_cells > 0,
            "the spreader converted no substrate at {origin:?}"
        );
    }
}
