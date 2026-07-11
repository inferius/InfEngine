//! W242 - MC underground structures, starting with the monster room
//! ("dungeon"): a small cobblestone cellar carved into cave walls,
//! with a spawner at its centre and up to two loot chests.
//!
//! Ported from `MonsterRoomFeature.java` (decompiled 26.1.2) +
//! `worldgen/placed_feature/monster_room.json`: 10 attempts per
//! chunk, in_square, uniform y in [0, world top], every biome. The
//! `monster_room_deep` variant (count 4, negative-y only) has no
//! domain in our 0..256 world - skipped, noted as the world-height
//! parity gap.
//!
//! Determinism / replay (the W226.1 rules): one SplitMix64 stream per
//! source column, so every chunk that replays the 3x3 neighbourhood
//! (rooms span up to +/-4 blocks horizontally and cross vertical
//! slices freely) reaches identical decisions. Reads go through the
//! post-carve column bitsets (`column_is_solid`) overlaid with the
//! cells already written by rooms earlier in the same pass - NEVER
//! through chunk-local neighbour state. Honest gaps vs MC: (1) other
//! features' writes (lava lakes, geodes) are invisible to the reads,
//! (2) "empty" means "not solid", so water-flooded cells count as
//! open - an occasional room MC would reject can open into an
//! aquifer.

use std::collections::HashMap;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::features::stream_seed;
use super::noise::SplitMix64;
use super::terrain::column_is_solid;
use crate::block::{CHEST_ID, COBBLE_ID, MOSSY_COBBLESTONE_ID, SPAWNER_ID};
use crate::worldgen::WorldgenCtx;

const SALT_MONSTER_ROOM: u64 = 0xD4E6_0000;

/// Placement pipeline count (placed_feature/monster_room.json).
const ATTEMPTS_PER_CHUNK: u32 = 10;

/// Uniform height range top: below_top 0 = the world build limit
/// (0..256 here; terrain.rs hardcodes the same span).
const WORLD_TOP_Y: i32 = 256;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// Cells written by ONE room while it builds. The Java build loop
/// reads the level it is writing (shell drop-outs, the chest
/// neighbour rule see this room's own cobble/air), so reads overlay
/// the scratch on base terrain. Deliberately NOT shared between
/// rooms: each room decides from base terrain + itself only, so
/// every chunk replaying the 3x3 neighbourhood reaches identical
/// decisions no matter which neighbours its replay window covers
/// (rooms from column C-1 can reach column C, but C+1's window
/// never replays C-1 - shared reads would desync those targets).
/// The rare MC deviation: two overlapping rooms both build where MC
/// would let the first reshape the second's validity scan; the
/// deterministic apply order (column-major, later wins) keeps every
/// chunk agreeing on the merged result.
pub(crate) struct RoomScratch {
    writes: HashMap<[i32; 3], u16>,
}

impl RoomScratch {
    fn new() -> Self {
        Self { writes: HashMap::new() }
    }

    fn set(&mut self, x: i32, y: i32, z: i32, id: u16) {
        self.writes.insert([x, y, z], id);
    }

    fn get(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        self.writes.get(&[x, y, z]).copied()
    }
}

/// `BlockState.isSolid` over overlay-then-base. Chests are NOT solid
/// in MC (non-full block); spawners are.
fn cell_solid(
    scratch: &RoomScratch,
    base_solid: &dyn Fn(i32, i32, i32) -> bool,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    match scratch.get(x, y, z) {
        Some(id) => {
            id == COBBLE_ID || id == MOSSY_COBBLESTONE_ID || id == SPAWNER_ID
        }
        None => base_solid(x, y, z),
    }
}

/// `Level.isEmptyBlock` over overlay-then-base. The base read is the
/// honest approximation "not solid" (see the module header).
fn cell_empty(
    scratch: &RoomScratch,
    base_solid: &dyn Fn(i32, i32, i32) -> bool,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    match scratch.get(x, y, z) {
        Some(id) => id == 0,
        None => !base_solid(x, y, z),
    }
}

/// One `MonsterRoomFeature.place` attempt at `(ox, oy, oz)`. Mirrors
/// the Java flow (and its rng call order) exactly; writes land in
/// `scratch`. Returns whether a room was built.
pub(crate) fn try_monster_room(
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
    base_solid: &dyn Fn(i32, i32, i32) -> bool,
    scratch: &mut RoomScratch,
) -> bool {
    let xr = next_int(rng, 2) + 2;
    let zr = next_int(rng, 2) + 2;
    let (min_x, max_x) = (-xr - 1, xr + 1);
    let (min_z, max_z) = (-zr - 1, zr + 1);

    // Validity scan: full floor + full ceiling solid, and 1..=5 ring
    // openings into a cave at origin height.
    let mut holes = 0u32;
    for dx in min_x..=max_x {
        for dy in -1..=4 {
            for dz in min_z..=max_z {
                let (x, y, z) = (ox + dx, oy + dy, oz + dz);
                let solid = cell_solid(scratch, base_solid, x, y, z);
                if dy == -1 && !solid {
                    return false;
                }
                if dy == 4 && !solid {
                    return false;
                }
                if (dx == min_x || dx == max_x || dz == min_z || dz == max_z)
                    && dy == 0
                    && cell_empty(scratch, base_solid, x, y, z)
                    && cell_empty(scratch, base_solid, x, y + 1, z)
                {
                    holes += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&holes) {
        return false;
    }

    // Build, top-down (dy 3 -> -1; the ceiling at dy 4 is never
    // touched - the Java `dy == 4` arm in this loop is dead code).
    for dx in min_x..=max_x {
        for dy in (-1..=3).rev() {
            for dz in min_z..=max_z {
                let (x, y, z) = (ox + dx, oy + dy, oz + dz);
                let on_shell =
                    dx == min_x || dy == -1 || dz == min_z || dx == max_x || dz == max_z;
                if on_shell {
                    if y >= 1 && !cell_solid(scratch, base_solid, x, y - 1, z) {
                        // Shell over a drop: open it up (continues the
                        // cave the room grew out of).
                        scratch.set(x, y, z, 0);
                    } else if cell_solid(scratch, base_solid, x, y, z)
                        && scratch.get(x, y, z) != Some(CHEST_ID)
                    {
                        if dy == -1 && next_int(rng, 4) != 0 {
                            scratch.set(x, y, z, MOSSY_COBBLESTONE_ID);
                        } else {
                            scratch.set(x, y, z, COBBLE_ID);
                        }
                    }
                } else if scratch.get(x, y, z) != Some(CHEST_ID)
                    && scratch.get(x, y, z) != Some(SPAWNER_ID)
                {
                    scratch.set(x, y, z, 0);
                }
            }
        }
    }

    // Chests: 2 slots x 3 attempts each, inside the interior span at
    // origin height, on cells with exactly ONE solid horizontal
    // neighbour. (MC reorients the chest toward the open side; our
    // delivery arm derives the facing from the world instead.)
    for _slot in 0..2 {
        for _try in 0..3 {
            let xc = ox + next_int(rng, xr * 2 + 1) - xr;
            let zc = oz + next_int(rng, zr * 2 + 1) - zr;
            if !cell_empty(scratch, base_solid, xc, oy, zc) {
                continue;
            }
            let solid_neighbours = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .filter(|(hx, hz)| {
                    cell_solid(scratch, base_solid, xc + hx, oy, zc + hz)
                })
                .count();
            if solid_neighbours == 1 {
                scratch.set(xc, oy, zc, CHEST_ID);
                break;
            }
        }
    }

    scratch.set(ox, oy, oz, SPAWNER_ID);
    // MC rolls the spawner entity here (skeleton/zombie/zombie/spider).
    // The entity kind is re-derived deterministically from world seed +
    // position wherever it's needed (W242.2 ticking), so the roll isn't
    // consumed from this stream - the block write is the whole output.
    true
}

/// The per-chunk monster-room pass: replay the 3x3 source-column
/// neighbourhood and keep the cells that land in this chunk.
pub(crate) fn place_monster_rooms(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let base_solid =
        |x: i32, y: i32, z: i32| -> bool { column_is_solid(ctx, x, y, z) };
    // Ordered room list: rooms apply column-major (the replay
    // iteration order), later rooms overwriting earlier ones where
    // they overlap. Within one room the map holds the final per-cell
    // state, so its iteration order is irrelevant.
    let mut rooms: Vec<RoomScratch> = Vec::new();
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mut rng =
                SplitMix64::new(stream_seed(seed, scx, scz, SALT_MONSTER_ROOM));
            for _ in 0..ATTEMPTS_PER_CHUNK {
                // in_square then height_range, like the placement
                // pipeline orders its modifiers.
                let ox = scx * cs + next_int(&mut rng, 16);
                let oz = scz * cs + next_int(&mut rng, 16);
                let oy = next_int(&mut rng, WORLD_TOP_Y);
                let mut scratch = RoomScratch::new();
                if try_monster_room(
                    &mut rng, ox, oy, oz, &base_solid, &mut scratch,
                ) {
                    rooms.push(scratch);
                }
            }
        }
    }
    for room in &rooms {
        for ([x, y, z], id) in &room.writes {
            let (lx, ly, lz) = (x - origin[0], y - origin[1], z - origin[2]);
            if (0..cs).contains(&lx)
                && (0..cs).contains(&ly)
                && (0..cs).contains(&lz)
            {
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(*id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic cave: a 3x3 pocket (y 10..=13) + a 2-tall tunnel cell
    /// on the ring, inside otherwise solid rock. Exactly one ring
    /// opening -> the room must build.
    fn pocket_solid(x: i32, y: i32, z: i32) -> bool {
        let pocket = (-1..=1).contains(&x)
            && (-1..=1).contains(&z)
            && (10..=13).contains(&y);
        let tunnel = x == 3 && z == 0 && (10..=11).contains(&y);
        !(pocket || tunnel)
    }

    /// Find an rng seed whose first two rolls give xr = zr = 2 (the
    /// tunnel at |dx| = 3 must sit ON the ring for the test cave).
    fn seed_with_radius_2() -> u64 {
        (0..2000u64)
            .find(|&s| {
                let mut r = SplitMix64::new(s);
                next_int(&mut r, 2) == 0 && next_int(&mut r, 2) == 0
            })
            .expect("a radius-2 seed exists")
    }

    #[test]
    fn room_builds_into_a_one_hole_cave() {
        let mut scratch = RoomScratch::new();
        let mut rng = SplitMix64::new(seed_with_radius_2());
        let built =
            try_monster_room(&mut rng, 0, 10, 0, &pocket_solid, &mut scratch);
        assert!(built, "1 ring hole is within 1..=5");
        assert_eq!(scratch.get(0, 10, 0), Some(SPAWNER_ID));
        // Floor under the spawner is cobblestone family.
        let floor = scratch.get(0, 9, 0).expect("floor written");
        assert!(floor == COBBLE_ID || floor == MOSSY_COBBLESTONE_ID);
        // Interior cells opened to air.
        assert_eq!(scratch.get(1, 12, 1), Some(0));
        // Walls turned to cobblestone (mossy is floor-only).
        assert_eq!(scratch.get(3, 11, 1), Some(COBBLE_ID));
        let chests = scratch
            .writes
            .values()
            .filter(|id| **id == CHEST_ID)
            .count();
        assert!(chests <= 2);
    }

    /// A fully sealed pocket (zero ring holes) must be rejected with
    /// no writes - MC only digs dungeons that open into a cave.
    #[test]
    fn sealed_pocket_is_rejected() {
        let sealed = |x: i32, y: i32, z: i32| -> bool {
            !((-1..=1).contains(&x)
                && (-1..=1).contains(&z)
                && (10..=13).contains(&y))
        };
        let mut scratch = RoomScratch::new();
        let mut rng = SplitMix64::new(seed_with_radius_2());
        let built = try_monster_room(&mut rng, 0, 10, 0, &sealed, &mut scratch);
        assert!(!built);
        assert!(scratch.writes.is_empty());
    }

    /// Missing floor/ceiling rejects (a room can't hang over a chasm).
    #[test]
    fn open_floor_is_rejected() {
        let no_floor = |_x: i32, y: i32, _z: i32| -> bool { y > 9 };
        let mut scratch = RoomScratch::new();
        let mut rng = SplitMix64::new(seed_with_radius_2());
        assert!(!try_monster_room(&mut rng, 0, 10, 0, &no_floor, &mut scratch));
        assert!(scratch.writes.is_empty());
    }

    /// Slice probe: renders one y-layer of REAL generated chunks
    /// around a room origin as ASCII (S=spawner, C=chest, m=mossy,
    /// c=cobble, .=air, #=other solid) - proves the feature survives
    /// the full worldgen pipeline, not just the unit harness.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_SLICE_X/Y/Z=<world pos>
    /// cargo test -p voxel_demo_mp --release --lib
    /// dump_monster_room_slice -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_monster_room_slice() {
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
        let (wx, wy, wz) = (
            env_i32("INFENGINE_SLICE_X", 4788),
            env_i32("INFENGINE_SLICE_Y", 9),
            env_i32("INFENGINE_SLICE_Z", 4838),
        );
        // The LIVE overworld runs with dim "overworld", which XORs
        // into the effective seed - a probe with dim "" scans a
        // different world.
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let r = 8;
        for y in [wy - 1, wy, wy + 1] {
            println!("--- y = {y} ---");
            for z in (wz - r)..=(wz + r) {
                let mut line = String::new();
                for x in (wx - r)..=(wx + r) {
                    let (ccx, ccy, ccz) = (
                        x.div_euclid(cs),
                        y.div_euclid(cs),
                        z.div_euclid(cs),
                    );
                    let chunk = crate::worldgen::generate_chunk_at_ctx(
                        [ccx, ccy, ccz],
                        &ctx,
                    );
                    let id = chunk
                        .get(
                            x.rem_euclid(cs) as usize,
                            y.rem_euclid(cs) as usize,
                            z.rem_euclid(cs) as usize,
                        )
                        .0;
                    line.push(match id {
                        0 => '.',
                        x if x == SPAWNER_ID => 'S',
                        x if x == CHEST_ID => 'C',
                        x if x == MOSSY_COBBLESTONE_ID => 'm',
                        x if x == COBBLE_ID => 'c',
                        _ => '#',
                    });
                }
                println!("{line}");
            }
        }
    }

    /// Loot verification probe: reads a REAL world save's container
    /// file and prints every chest with its slot contents - proves
    /// the DungeonChest delivery arm minted + persisted loot without
    /// needing the in-game chest UI.
    /// `INFENGINE_DUMP_WORLD=test-mc-9 cargo test -p voxel_demo_mp
    /// --release --lib dump_saved_chests -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_saved_chests() {
        let slug = std::env::var("INFENGINE_DUMP_WORLD")
            .unwrap_or_else(|_| "test-mc-9".into());
        let dim = std::env::var("INFENGINE_DUMP_DIM")
            .unwrap_or_else(|_| "overworld".into());
        let dc = crate::world_save::containers::read_containers(&slug, &dim)
            .expect("containers file reads")
            .expect("containers file exists");
        println!("{} chest(s) in {slug}/{dim}", dc.chests.len());
        for ch in &dc.chests {
            let items: Vec<String> = ch
                .slots
                .iter()
                .flatten()
                .map(|(id, n)| {
                    let name = crate::item::item_type(*id)
                        .map(|it| it.name)
                        .unwrap_or("?");
                    format!("{name} x{n}")
                })
                .collect();
            println!("chest at {:?}: [{}]", ch.pos, items.join(", "));
        }
    }

    /// In-game verification probe: prints every monster-room origin
    /// within `radius` columns of a centre column for a seed, so a
    /// live session can /tppos straight to a room.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX=<chunk x>
    /// INFENGINE_DUMP_CZ=<chunk z> cargo test -p voxel_demo_mp
    /// --release --lib dump_monster_rooms -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_monster_rooms() {
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
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 8);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 0), env_i32("INFENGINE_DUMP_CZ", 0));
        // Match the live overworld's dim-mixed effective seed (see
        // dump_monster_room_slice).
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        // Streams must roll from the same DIM-MIXED seed the live
        // pipeline uses, not the raw base seed.
        let eff = ctx.effective_seed();
        let base_solid =
            |x: i32, y: i32, z: i32| -> bool { column_is_solid(&ctx, x, y, z) };
        let mut found = 0usize;
        for scx in (ccx - radius)..=(ccx + radius) {
            for scz in (ccz - radius)..=(ccz + radius) {
                let mut rng = SplitMix64::new(stream_seed(
                    eff,
                    scx,
                    scz,
                    SALT_MONSTER_ROOM,
                ));
                for _ in 0..ATTEMPTS_PER_CHUNK {
                    let ox = scx * cs + next_int(&mut rng, 16);
                    let oz = scz * cs + next_int(&mut rng, 16);
                    let oy = next_int(&mut rng, WORLD_TOP_Y);
                    let mut scratch = RoomScratch::new();
                    if try_monster_room(
                        &mut rng,
                        ox,
                        oy,
                        oz,
                        &base_solid,
                        &mut scratch,
                    ) {
                        let chests = scratch
                            .writes
                            .values()
                            .filter(|id| **id == CHEST_ID)
                            .count();
                        println!(
                            "monster room at ({ox}, {oy}, {oz}) - {chests} chest(s)"
                        );
                        found += 1;
                    }
                }
            }
        }
        println!(
            "total: {found} rooms in +/-{radius} columns around \
             ({ccx}, {ccz}), seed {seed}"
        );
    }
}
