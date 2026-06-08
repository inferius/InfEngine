//! Dungeon room placer for the `Realistic` generator (W17.3).
//!
//! A dungeon is a 7x7x5 (width x depth x height) hollow room of
//! cobblestone walls, placed underground. The interior is air with:
//! - A single cobblestone block at the centre-floor tagged as a
//!   placeholder "spawner" (TODO: real tile-entity spawner block in
//!   a future wave).
//! - 1-2 chest blocks on the interior floor (empty for now; loot
//!   tables are a deferred feature).
//!
//! Spawn rate: ~1/200 chunks (per chunk_x, chunk_z column).
//!
//! Y placement: between y=8 and y=32 (sub-surface, never too deep
//! for the player to find, never too shallow to pierce the surface).
//!
//! Note on chests: `CHEST_ID` (41) blocks are placed in the Chunk
//! directly. The chest's Container (inventory) is created lazily
//! the first time a player opens it - placing the block is enough.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use crate::block::COBBLE_ID;
use crate::worldgen::climate::SEA_LEVEL;
use crate::worldgen::strata::WORLD_FLOOR;
use crate::worldgen::{WorldgenCtx, hash3_for};

// ---------------------------------------------------------------
// Constants
// ---------------------------------------------------------------

/// Inverse spawn probability (per chunk column). 200 -> ~1/200.
const DUNGEON_RATE: u32 = 200;

/// Dungeon floor Y band, SEA_LEVEL-relative (W121). Was a fixed y8..32
/// pinned below the old sea-12 surface, which buried every dungeon under
/// the waterline. Now spans from just above the bedrock floor to a safe
/// margin below sea level, so dungeons appear throughout the underground
/// the player actually explores without ever piercing the surface.
const DUNGEON_FLOOR_MIN_Y: i32 = WORLD_FLOOR + 6;
/// Top of the dungeon floor band: leave headroom under the sea so the
/// 5-tall room never pokes through a shallow seabed.
const DUNGEON_FLOOR_MAX_Y: i32 = SEA_LEVEL - 12;

/// Interior room width (X) and depth (Z) in blocks. MC uses 9x9
/// inner; we use 5x5 inner (7x7 outer) as a slightly smaller but
/// still noticeable room.
const ROOM_INNER_W: i32 = 5;
const ROOM_INNER_D: i32 = 5;
/// Interior room height (Y). 3 blocks of air inside, 1 floor, 1 ceiling.
const ROOM_INNER_H: i32 = 3;
/// Outer dimensions include a 1-block wall on every face.
const ROOM_OUTER_W: i32 = ROOM_INNER_W + 2;
const ROOM_OUTER_D: i32 = ROOM_INNER_D + 2;
const ROOM_OUTER_H: i32 = ROOM_INNER_H + 2;

/// Block id for dungeon walls/floor/ceiling.
const WALL_BLOCK: u16 = COBBLE_ID;
/// Block id placed at room centre as a spawner placeholder.
/// TODO: replace with a real tile-entity spawner block.
const SPAWNER_PLACEHOLDER: u16 = COBBLE_ID;
/// Chest block id (CHEST_ID = 41).
const CHEST_BLOCK: u16 = 41;

// ---------------------------------------------------------------
// Salts for hash derivation
// ---------------------------------------------------------------

const SALT_SPAWN: u32 = 0xD001_D001;
const SALT_OX: u32 = 0xBEEF_CAFE;
const SALT_OZ: u32 = 0x1337_0000;
const SALT_Y: u32 = 0xF00D_BABE;
const SALT_CHESTS: u32 = 0xABBA_ABBA;
const SALT_C1X: u32 = 0x1111_2222;
const SALT_C1Z: u32 = 0x3333_4444;
const SALT_C2X: u32 = 0x5555_6666;
const SALT_C2Z: u32 = 0x7777_8888;

// ---------------------------------------------------------------
// Dungeon descriptor
// ---------------------------------------------------------------

struct Dungeon {
    /// World-space origin (min-corner) of the outer cobblestone box.
    ox: i32,
    oy: i32,
    oz: i32,
    /// Number of chests to place (1 or 2).
    num_chests: u32,
    /// Interior-local X offset for each chest (0..ROOM_INNER_W).
    chest_x: [i32; 2],
    /// Interior-local Z offset for each chest (0..ROOM_INNER_D).
    chest_z: [i32; 2],
}

impl Dungeon {
    /// Returns the block that should occupy world position (wx, wy, wz),
    /// or `None` if this position is outside the dungeon box.
    fn block_at(&self, wx: i32, wy: i32, wz: i32) -> Option<BlockId> {
        let lx = wx - self.ox;
        let ly = wy - self.oy;
        let lz = wz - self.oz;

        // Outside the outer box entirely.
        if !(0..ROOM_OUTER_W).contains(&lx)
            || !(0..ROOM_OUTER_H).contains(&ly)
            || !(0..ROOM_OUTER_D).contains(&lz)
        {
            return None;
        }

        // Is this voxel on the outer shell (wall / floor / ceiling)?
        let on_shell = lx == 0
            || lx == ROOM_OUTER_W - 1
            || ly == 0
            || ly == ROOM_OUTER_H - 1
            || lz == 0
            || lz == ROOM_OUTER_D - 1;

        if on_shell {
            return Some(BlockId(WALL_BLOCK));
        }

        // Interior coordinates (relative to inner room, 0-indexed).
        // ly==1 is the inner floor row (ly==0 is the outer floor/wall).
        let ix = lx - 1; // 0..ROOM_INNER_W
        let iz = lz - 1; // 0..ROOM_INNER_D
        let iy = ly - 1; // 0..ROOM_INNER_H; 0 = floor row

        // Floor row: spawner placeholder at centre, chests, else air.
        if iy == 0 {
            let cx = ROOM_INNER_W / 2;
            let cz = ROOM_INNER_D / 2;
            if ix == cx && iz == cz {
                // Centre floor = spawner placeholder.
                return Some(BlockId(SPAWNER_PLACEHOLDER));
            }
            // Chest positions.
            for i in 0..self.num_chests as usize {
                if ix == self.chest_x[i] && iz == self.chest_z[i] {
                    return Some(BlockId(CHEST_BLOCK));
                }
            }
            return Some(BlockId::AIR);
        }

        // Upper interior rows are air.
        Some(BlockId::AIR)
    }
}

/// Derive a Dungeon from a (chunk_x, chunk_z) column. Returns `None`
/// if the spawn roll fails.
fn try_dungeon_at_column(chunk_x: i32, chunk_z: i32, seed: u32) -> Option<Dungeon> {
    let spawn_h = hash3_for(chunk_x, 0, chunk_z, seed ^ SALT_SPAWN);
    if !spawn_h.is_multiple_of(DUNGEON_RATE) {
        return None;
    }
    use infengine_voxel::CHUNK_SIZE;
    let cs = CHUNK_SIZE as i32;

    // Offset within chunk for the dungeon's outer-box min-corner (x, z).
    // Must fit: outer box is ROOM_OUTER_W wide, so offset range 0..(cs - ROOM_OUTER_W).
    let x_range = (cs - ROOM_OUTER_W).max(1) as u32;
    let z_range = (cs - ROOM_OUTER_D).max(1) as u32;
    let ox_local = (hash3_for(chunk_x, 1, chunk_z, seed ^ SALT_OX) % x_range) as i32;
    let oz_local = (hash3_for(chunk_x, 2, chunk_z, seed ^ SALT_OZ) % z_range) as i32;
    let ox = chunk_x * cs + ox_local;
    let oz = chunk_z * cs + oz_local;

    // Y: SEA_LEVEL-relative floor band (W121) for the outer box.
    let y_range = (DUNGEON_FLOOR_MAX_Y - DUNGEON_FLOOR_MIN_Y + 1).max(1) as u32;
    let oy = DUNGEON_FLOOR_MIN_Y + (hash3_for(chunk_x, 3, chunk_z, seed ^ SALT_Y) % y_range) as i32;

    // Number of chests: 1 or 2.
    let chest_h = hash3_for(chunk_x, 4, chunk_z, seed ^ SALT_CHESTS);
    let num_chests = 1 + (chest_h % 2); // 1 or 2

    // Chest positions (interior, clamped to avoid centre spawner).
    let inner_range_x = ROOM_INNER_W as u32;
    let inner_range_z = ROOM_INNER_D as u32;
    let cx_spawner = ROOM_INNER_W / 2;
    let cz_spawner = ROOM_INNER_D / 2;

    // Pick first chest position, avoiding the spawner centre.
    let c1x_raw = (hash3_for(chunk_x, 5, chunk_z, seed ^ SALT_C1X) % inner_range_x) as i32;
    let c1z_raw = (hash3_for(chunk_x, 6, chunk_z, seed ^ SALT_C1Z) % inner_range_z) as i32;
    // If it lands on the spawner, shift by 1.
    let c1x = if c1x_raw == cx_spawner && c1z_raw == cz_spawner {
        (c1x_raw + 1) % ROOM_INNER_W
    } else {
        c1x_raw
    };
    let c1z = c1z_raw;

    // Second chest (only used if num_chests == 2).
    let c2x_raw = (hash3_for(chunk_x, 7, chunk_z, seed ^ SALT_C2X) % inner_range_x) as i32;
    let c2z_raw = (hash3_for(chunk_x, 8, chunk_z, seed ^ SALT_C2Z) % inner_range_z) as i32;
    let c2x = if (c2x_raw == cx_spawner && c2z_raw == cz_spawner)
        || (c2x_raw == c1x && c2z_raw == c1z)
    {
        (c2x_raw + 2) % ROOM_INNER_W
    } else {
        c2x_raw
    };
    let c2z = c2z_raw;

    Some(Dungeon {
        ox,
        oy,
        oz,
        num_chests,
        chest_x: [c1x, c2x],
        chest_z: [c1z, c2z],
    })
}

// ---------------------------------------------------------------
// Public placement pass
// ---------------------------------------------------------------

/// Place dungeon rooms into an already-filled chunk. Called from
/// `gen_realistic` after decoration passes (so cobblestone walls
/// write over any misplaced decorations).
///
/// A dungeon anchored in a neighbouring column may extend into
/// this chunk, so we check a neighbourhood of columns. The outer
/// box is at most ROOM_OUTER_W = 7 blocks wide, fitting within a
/// single chunk column.
pub(crate) fn place(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    use infengine_voxel::CHUNK_SIZE;

    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let cs = CHUNK_SIZE as i32;

    // A dungeon fits within one chunk column (7 wide, cs=16 wide);
    // but it may be anchored in the immediate neighbouring column
    // and still overhang into this chunk. 1-block neighbourhood
    // is sufficient given ROOM_OUTER_W < cs.
    let chunk_radius = 1;
    let cx0 = coord[0];
    let cz0 = coord[2];

    let mut writes: Vec<(usize, usize, usize, BlockId)> = Vec::new();

    for dcx in -chunk_radius..=chunk_radius {
        for dcz in -chunk_radius..=chunk_radius {
            let dungeon = match try_dungeon_at_column(cx0 + dcx, cz0 + dcz, seed) {
                Some(d) => d,
                None => continue,
            };

            // Bounding box of the dungeon in world space.
            let wx_min = dungeon.ox;
            let wx_max = dungeon.ox + ROOM_OUTER_W - 1;
            let wy_min = dungeon.oy;
            let wy_max = dungeon.oy + ROOM_OUTER_H - 1;
            let wz_min = dungeon.oz;
            let wz_max = dungeon.oz + ROOM_OUTER_D - 1;

            // Intersect with this chunk's world-space extent.
            let chunk_wx_min = origin.x as i32;
            let chunk_wx_max = chunk_wx_min + cs - 1;
            let chunk_wy_min = origin.y as i32;
            let chunk_wy_max = chunk_wy_min + cs - 1;
            let chunk_wz_min = origin.z as i32;
            let chunk_wz_max = chunk_wz_min + cs - 1;

            let ix_min = wx_min.max(chunk_wx_min);
            let ix_max = wx_max.min(chunk_wx_max);
            let iy_min = wy_min.max(chunk_wy_min);
            let iy_max = wy_max.min(chunk_wy_max);
            let iz_min = wz_min.max(chunk_wz_min);
            let iz_max = wz_max.min(chunk_wz_max);

            if ix_min > ix_max || iy_min > iy_max || iz_min > iz_max {
                continue; // No overlap with this chunk.
            }

            for wz in iz_min..=iz_max {
                for wy in iy_min..=iy_max {
                    for wx in ix_min..=ix_max {
                        if let Some(block) = dungeon.block_at(wx, wy, wz) {
                            let lx = (wx - chunk_wx_min) as usize;
                            let ly = (wy - chunk_wy_min) as usize;
                            let lz = (wz - chunk_wz_min) as usize;
                            writes.push((lx, ly, lz, block));
                        }
                    }
                }
            }
        }
    }

    for (x, y, z, block) in writes {
        chunk.set(x, y, z, block);
    }
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{WorldGen, WorldgenCtx};
    use infengine_voxel::{BlockId, CHUNK_SIZE};

    fn ctx(seed: u32) -> WorldgenCtx {
        WorldgenCtx {
            base_seed: seed,
            dim: String::new(),
            profile: WorldGen::Realistic,
        }
    }

    fn stone_chunk() -> Chunk {
        let mut c = Chunk::empty();
        let n = CHUNK_SIZE;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    c.set(x, y, z, BlockId(1));
                }
            }
        }
        c
    }

    /// Find a chunk column that spawns a dungeon under `seed`.
    fn find_dungeon_column(seed: u32) -> Option<(i32, i32)> {
        for cx in -200..200_i32 {
            for cz in -200..200_i32 {
                if try_dungeon_at_column(cx, cz, seed).is_some() {
                    return Some((cx, cz));
                }
            }
        }
        None
    }

    // ---------------------------------------------------------------
    // W17.3 dungeon tests
    // ---------------------------------------------------------------

    /// A chunk column selected by the hash spawns a dungeon.
    #[test]
    fn dungeon_spawns_in_marked_chunk() {
        let seed = 0;
        let (cx, cz) = find_dungeon_column(seed).expect("no dungeon found in search area");
        let d = try_dungeon_at_column(cx, cz, seed).unwrap();
        // W121: the dungeon origin Y now sits in the SEA_LEVEL-relative
        // band (WORLD_FLOOR+6 .. SEA_LEVEL-12), not the old absolute
        // 8..=32.
        assert!(
            (DUNGEON_FLOOR_MIN_Y..=DUNGEON_FLOOR_MAX_Y).contains(&d.oy),
            "dungeon Y={} out of expected range {DUNGEON_FLOOR_MIN_Y}..={DUNGEON_FLOOR_MAX_Y}",
            d.oy
        );
    }

    /// The outer shell of a placed dungeon consists of cobblestone.
    #[test]
    fn dungeon_has_cobblestone_walls() {
        let seed = 0;
        let (cx, cz) = find_dungeon_column(seed).expect("no dungeon found");
        let d = try_dungeon_at_column(cx, cz, seed).unwrap();

        // Check all 6 faces of the outer shell.
        let ox = d.ox;
        let oy = d.oy;
        let oz = d.oz;

        // Bottom floor row (ly=0).
        for lx in 0..ROOM_OUTER_W {
            for lz in 0..ROOM_OUTER_D {
                let b = d.block_at(ox + lx, oy, oz + lz).unwrap();
                assert_eq!(b.0, WALL_BLOCK, "floor not cobble at ({lx},0,{lz})");
            }
        }
        // Top ceiling row (ly=ROOM_OUTER_H-1).
        for lx in 0..ROOM_OUTER_W {
            for lz in 0..ROOM_OUTER_D {
                let b = d
                    .block_at(ox + lx, oy + ROOM_OUTER_H - 1, oz + lz)
                    .unwrap();
                assert_eq!(
                    b.0,
                    WALL_BLOCK,
                    "ceiling not cobble at ({lx},{},{lz})",
                    ROOM_OUTER_H - 1
                );
            }
        }
        // North/south walls (lz=0 and lz=ROOM_OUTER_D-1).
        for ly in 0..ROOM_OUTER_H {
            for lx in 0..ROOM_OUTER_W {
                assert_eq!(
                    d.block_at(ox + lx, oy + ly, oz).unwrap().0,
                    WALL_BLOCK
                );
                assert_eq!(
                    d.block_at(ox + lx, oy + ly, oz + ROOM_OUTER_D - 1)
                        .unwrap()
                        .0,
                    WALL_BLOCK
                );
            }
        }
    }

    /// The interior of a placed dungeon (above the floor row) is air.
    #[test]
    fn dungeon_interior_is_air() {
        let seed = 0;
        let (cx, cz) = find_dungeon_column(seed).expect("no dungeon found");
        let d = try_dungeon_at_column(cx, cz, seed).unwrap();
        let ox = d.ox;
        let oy = d.oy;
        let oz = d.oz;

        // Inner rows above floor: lx 1..ROOM_OUTER_W-1, lz 1..ROOM_OUTER_D-1,
        // ly 2..ROOM_OUTER_H-1  (ly=1 is floor row, ly=2+ is open air).
        for ly in 2..ROOM_OUTER_H - 1 {
            for lz in 1..ROOM_OUTER_D - 1 {
                for lx in 1..ROOM_OUTER_W - 1 {
                    let b = d.block_at(ox + lx, oy + ly, oz + lz).unwrap();
                    assert_eq!(
                        b,
                        BlockId::AIR,
                        "interior not air at ({lx},{ly},{lz}), got {}",
                        b.0
                    );
                }
            }
        }
    }

    /// A dungeon is placed into a fully-stone chunk (walls appear).
    #[test]
    fn dungeon_writes_to_chunk() {
        let seed = 0;
        let (cx, cz) = find_dungeon_column(seed).expect("no dungeon found");
        // The dungeon oy is 8..=32; use chunk Y=0 (world-Y 0..16) or Y=1 (16..32).
        let ctx = ctx(seed);
        let d = try_dungeon_at_column(cx, cz, seed).unwrap();
        let chy = d.oy / 16; // Which chunk Y slice.
        let mut chunk = stone_chunk();
        place(&mut chunk, [cx, chy, cz], &ctx);
        // Count cobblestone blocks (should be > 0 from walls).
        let n = CHUNK_SIZE;
        let cobble_count = (0..n)
            .flat_map(|z| (0..n).flat_map(move |y| (0..n).map(move |x| (x, y, z))))
            .filter(|&(x, y, z)| chunk.get(x, y, z).0 == COBBLE_ID)
            .count();
        // Wall volume > 0.
        assert!(cobble_count > 0, "no cobblestone written to chunk");
    }

    /// The dungeon placement is deterministic.
    #[test]
    fn dungeon_deterministic_per_seed() {
        let seed = 42;
        let ctx = ctx(seed);
        let (cx, cz) = find_dungeon_column(seed).unwrap_or((0, 0));
        let a = {
            let mut c = stone_chunk();
            place(&mut c, [cx, 0, cz], &ctx);
            c
        };
        let b = {
            let mut c = stone_chunk();
            place(&mut c, [cx, 0, cz], &ctx);
            c
        };
        let n = CHUNK_SIZE;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    assert_eq!(a.get(x, y, z), b.get(x, y, z));
                }
            }
        }
    }

    /// Dungeon spawn rate is in the expected ballpark (~1/200 columns).
    #[test]
    fn dungeon_spawn_rate_approximately_correct() {
        let seed = 0;
        let mut spawned = 0;
        let total = 2000_i32;
        for cx in 0..total {
            if try_dungeon_at_column(cx, 0, seed).is_some() {
                spawned += 1;
            }
        }
        // Expected ~10; allow wide variance.
        assert!(
            spawned > 0 && spawned < 200,
            "unusual dungeon rate: {spawned}/{total}"
        );
    }

    /// A dungeon's floor row contains at least one chest.
    #[test]
    fn dungeon_floor_has_a_chest() {
        let seed = 0;
        let (cx, cz) = find_dungeon_column(seed).expect("no dungeon found");
        let d = try_dungeon_at_column(cx, cz, seed).unwrap();
        let ox = d.ox;
        let oy = d.oy;
        let oz = d.oz;
        // Floor row: ly=1 in outer coords.
        let mut found_chest = false;
        for lz in 1..ROOM_OUTER_D - 1 {
            for lx in 1..ROOM_OUTER_W - 1 {
                if matches!(d.block_at(ox + lx, oy + 1, oz + lz), Some(b) if b.0 == CHEST_BLOCK) {
                    found_chest = true;
                }
            }
        }
        assert!(found_chest, "dungeon floor has no chest");
    }

    /// Rooms outside the block's range return `None` from `block_at`.
    #[test]
    fn dungeon_block_at_returns_none_outside() {
        let d = Dungeon {
            ox: 10,
            oy: 10,
            oz: 10,
            num_chests: 1,
            chest_x: [1, 1],
            chest_z: [1, 1],
        };
        assert!(d.block_at(9, 10, 10).is_none());
        assert!(d.block_at(10 + ROOM_OUTER_W, 10, 10).is_none());
        assert!(d.block_at(10, 9, 10).is_none());
        assert!(d.block_at(10, 10 + ROOM_OUTER_H, 10).is_none());
    }
}
