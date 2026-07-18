//! W65 - MC-style random block tick.
//!
//! MC mirror: `ServerLevel.tickChunk` -> `randomTick`. For each
//! loaded chunk section, `randomTickSpeed` (default 3) random
//! positions per game tick call `Block.randomTick`, driving grass
//! spread, sapling growth, leaf decay, crop growth, fire, etc.
//!
//! We approximate with a per-frame sample over the chunks within
//! `RADIUS_CHUNKS` of the host player, bounded by `MAX_CELLS_PER_TICK`
//! so a large loaded world doesn't blow the frame budget. Host-only;
//! clients receive the resulting `vx.edit` traffic.
//!
//! W65.1 ships the grass-spread behaviour. Later sub-waves add
//! sapling -> tree growth, leaf decay, and crop growth under the same
//! `random_tick_cell` dispatch.

use infengine_voxel::CHUNK_SIZE;

use crate::block::block_is_opaque;
use crate::net::NetRole;
use crate::{BlockId, Voxel};

/// Dirt block id (the grass-spread target / death result).
const DIRT_ID: u16 = 2;
/// Grass-block id (the random-tick subject).
const GRASS_ID: u16 = 3;

/// Horizontal radius (in chunks) around the player whose loaded
/// chunks are eligible for random ticks. Vertical sections in those
/// columns are all eligible (grass / crops live near the surface but
/// the per-cell match keeps off-surface samples cheap).
/// W233: shared with the lava-ignition pass (fire_runtime) so both
/// "randomTick near the player" behaviours agree on the range.
pub(crate) const RADIUS_CHUNKS: i32 = 4;

/// Cells sampled per eligible chunk per frame, before the global cap.
const CELLS_PER_CHUNK: usize = 1;

/// Hard per-frame ceiling on random-tick cell visits, independent of
/// how many chunks are loaded. Most samples land on air / stone and
/// no-op, so this is generous.
const MAX_CELLS_PER_TICK: usize = 256;

/// Inverse chance a sampled sapling advances toward a tree on a given
/// random tick (1 in N). Higher = slower growth.
const SAPLING_GROW_DENOM: u32 = 8;

/// Inverse chance a decaying leaf drops an oak sapling (1 in N).
const LEAF_SAPLING_DROP_DENOM: u32 = 20;

/// MC random ticks advance at the 20 Hz game-tick rate. This used to
/// run per FRAME - at 400 fps that's 20x MC's sample rate for pure
/// waste, and each pass paid the origin scan below.
const RANDOM_TICK_HZ: f32 = 20.0;

/// Host-side random-tick driver. Samples random cells from the
/// loaded chunks near the player and dispatches each to a per-block
/// behaviour. No-op on clients.
pub(crate) fn tick(vx: &mut Voxel, dt: f32) {
    if matches!(vx.net, NetRole::Client { .. }) {
        return;
    }
    // W232: fixed 20 Hz cadence (the W104/W198.7 accumulator
    // pattern) instead of per-frame.
    vx.random_tick_accum += dt;
    let period = 1.0 / RANDOM_TICK_HZ;
    if vx.random_tick_accum < period {
        return;
    }
    // One pass per rollover; a long stall doesn't burst-run passes.
    vx.random_tick_accum = (vx.random_tick_accum - period).min(period);
    let cs = CHUNK_SIZE as i32;
    let pcx = (vx.player_pos.x / cs as f32).floor() as i32;
    let pcz = (vx.player_pos.z / cs as f32).floor() as i32;
    // Snapshot the in-radius loaded chunk origins first so the
    // mutable behaviour pass below doesn't alias the chunk map.
    //
    // W232: probe the RADIUS box with chunk() lookups instead of
    // filtering ALL loaded chunks - the old iter_chunks() pass was
    // O(loaded world) per call (10k chunks at VD 12 full columns,
    // 30k+ at VD 40) for a 9x9-column result.
    let mut origins: Vec<[i32; 3]> = Vec::new();
    let (cy_min, cy_max) = vx.vertical_chunk_range();
    for dz in -RADIUS_CHUNKS..=RADIUS_CHUNKS {
        for dx in -RADIUS_CHUNKS..=RADIUS_CHUNKS {
            for cy in cy_min..=cy_max {
                let cc = [pcx + dx, cy, pcz + dz];
                if vx.world.chunk(cc).is_some() {
                    origins.push([cc[0] * cs, cc[1] * cs, cc[2] * cs]);
                }
            }
        }
    }
    if origins.is_empty() {
        return;
    }
    let budget = (origins.len() * CELLS_PER_CHUNK).min(MAX_CELLS_PER_TICK);
    // Counter-based xorshift seeded from the net tick so the sample
    // pattern varies frame to frame without a wall clock.
    let mut seed = (vx.net_tick.wrapping_mul(0x9E37_79B1) ^ 0x5BD1_E995) as u32;
    for _ in 0..budget {
        let r = xorshift(&mut seed);
        let origin = origins[(r as usize) % origins.len()];
        let lx = (r & 15) as i32;
        let ly = ((r >> 4) & 15) as i32;
        let lz = ((r >> 8) & 15) as i32;
        let pos = [origin[0] + lx, origin[1] + ly, origin[2] + lz];
        random_tick_cell(vx, pos, xorshift(&mut seed));
    }
}

/// In-place xorshift32: advance `s` and return the new word.
fn xorshift(s: &mut u32) -> u32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}

/// Dispatch a single random-tick cell to its block behaviour. The
/// `rng` word lets a behaviour make a local random choice (e.g. the
/// grass-spread direction) without touching the scheduler's stream.
fn random_tick_cell(vx: &mut Voxel, pos: [i32; 3], rng: u32) {
    let id = vx.world.get(pos).0;
    if id == GRASS_ID {
        if let Some((p, b)) = grass_tick(&vx.world, pos, rng) {
            crate::world::edit::emit(vx, p, b);
        }
    } else if is_sapling(id) {
        sapling_tick(vx, pos, rng);
    } else if is_leaf_block(id) {
        leaf_tick(vx, pos, rng);
    } else if crate::world::crop::is_immature_crop(id) {
        crop_tick(vx, pos, rng);
    }
}

/// W294: `CropBlock.randomTick`, 1:1 growth shape - one age per
/// roll of `nextInt(25 / growthSpeed + 1) == 0`, where the speed
/// counts the farmland under the 3x3 around the soil block
/// (`getGrowthSpeed`; our farmland has no moisture states, so every
/// farmland counts as MOIST - village farms sit next to water) plus
/// the same-crop row penalty. Beetroots roll the extra
/// `nextInt(3) != 0 -> skip` gate first (`BeetrootBlock`). The
/// `light >= 9` gate waits on a random-tick light accessor (the
/// grass stand-in shares that gap).
fn crop_tick(vx: &mut Voxel, pos: [i32; 3], rng: u32) {
    let id = vx.world.get(pos).0;
    let Some(next) = crate::world::crop::next_stage(id) else {
        return;
    };
    if crate::block::crop_stages::is_beetroot_stage(id) && !rng.is_multiple_of(3) {
        return;
    }
    let below_pos = [pos[0], pos[1] - 1, pos[2]];
    let below = vx.world.get(below_pos).0;
    let farmland = crate::block::crop_stages::FARMLAND_ID;
    // Legacy crops planted on dirt/grass before W294 still grow (at
    // base speed); fresh plants require farmland at place time.
    if below != farmland && below != DIRT_ID && below != GRASS_ID {
        return;
    }
    let mut speed = 1.0f32;
    if below == farmland {
        speed += 3.0;
        for dx in -1..=1i32 {
            for dz in -1..=1i32 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let p = [below_pos[0] + dx, below_pos[1], below_pos[2] + dz];
                if vx.world.get(p).0 == farmland {
                    speed += 0.75;
                }
            }
        }
    }
    // Row penalty: same crop on both axes (or diagonally) halves the
    // speed - vanilla's anti-solid-field rule.
    let same = |dx: i32, dz: i32| {
        crate::block::crop_stages::same_family(
            vx.world.get([pos[0] + dx, pos[1], pos[2] + dz]).0,
            id,
        )
    };
    let row = same(-1, 0) || same(1, 0);
    let col = same(0, -1) || same(0, 1);
    let diag = same(-1, -1) || same(1, -1) || same(-1, 1) || same(1, 1);
    if (row && col) || diag {
        speed /= 2.0;
    }
    let denom = (25.0 / speed) as u32 + 1;
    // Second hash so the beetroot pre-gate above doesn't correlate.
    let mut r = rng ^ 0x9E37_79B9;
    r ^= r << 13;
    r ^= r >> 17;
    r ^= r << 5;
    if !r.is_multiple_of(denom) {
        return;
    }
    crate::world::edit::emit(vx, pos, BlockId(next));
}

/// True for the standard 7 `_sapling` blocks (oak / birch / spruce /
/// jungle / acacia / dark_oak / cherry). Mangrove propagule + azalea
/// grow differently and are intentionally excluded.
fn is_sapling(id: u16) -> bool {
    crate::block::block_type(id)
        .map(|b| b.name.ends_with("_sapling"))
        .unwrap_or(false)
}

/// Probabilistic sapling growth. On a fraction of the random ticks a
/// sapling is sampled, if it stands on soil with trunk headroom it
/// becomes a small oak. The chance gate (plus the sparse sampling)
/// spreads growth over a handful of seconds rather than firing the
/// instant a sapling is placed.
fn sapling_tick(vx: &mut Voxel, pos: [i32; 3], rng: u32) {
    if !rng.is_multiple_of(SAPLING_GROW_DENOM) {
        return;
    }
    // Must stand on sustaining soil (dirt / grass).
    let below = vx.world.get([pos[0], pos[1] - 1, pos[2]]).0;
    if below != DIRT_ID && below != GRASS_ID {
        return;
    }
    let Some(edits) = crate::world::tree::grow_oak_edits(&vx.world, pos) else {
        return;
    };
    for (p, b) in edits {
        crate::world::edit::emit(vx, p, b);
    }
}

/// W303: any leaf species (the dispatch used to sample OAK only, so
/// birch/spruce/... canopies never decayed).
fn is_leaf_block(id: u16) -> bool {
    id == crate::block::LEAVES_ID
        || crate::block::block_type(id)
            .is_some_and(|b| b.name.ends_with("_leaves"))
}

/// W303: any log/trunk block sustains leaves (MC's `LOGS` tag - the
/// old check matched the OAK log id alone, letting other species'
/// canopies rot on living trees).
fn is_log_block(id: u16) -> bool {
    id == crate::block::WOOD_ID
        || crate::block::block_type(id)
            .is_some_and(|b| b.name.ends_with("_log") || b.name.ends_with("_wood"))
}

/// Leaf random tick, MC `LeavesBlock` distance rule: a leaf survives
/// while a log is reachable within distance 7 stepping THROUGH
/// leaves (6-connected; MC recomputes the `distance` state on
/// neighbour updates - the tick derives it directly with a
/// depth-capped BFS, replacing the W275 Chebyshev box stand-in that
/// kept leaves alive across air gaps). Player-placed leaves
/// (`persistent_leaves`) are exempt like MC's `persistent` state.
/// Decay drops the species sapling at 1-in-20 plus the oak/dark-oak
/// apple at 1-in-200.
fn leaf_tick(vx: &mut Voxel, pos: [i32; 3], rng: u32) {
    if vx.persistent_leaves.contains(&pos) {
        return;
    }
    if log_supported(&vx.world, pos) {
        return;
    }
    let id = vx.world.get(pos).0;
    crate::world::edit::emit(vx, pos, BlockId::AIR);
    if rng.is_multiple_of(LEAF_SAPLING_DROP_DENOM) {
        vx.spawn_drop(BlockId(sapling_for_leaf(id)), pos);
    }
    // Oak + dark oak canopies drop the 0.5% apple (loot tables).
    let name = crate::block::block_type(id).map(|b| b.name).unwrap_or("");
    if (id == crate::block::LEAVES_ID || name == "dark_oak_leaves")
        && (rng >> 8).is_multiple_of(200)
    {
        const APPLE_ITEM: u16 = 162;
        vx.spawn_drop(BlockId(APPLE_ITEM), pos);
    }
}

/// The `distance <= 6` reachability test: BFS from the leaf through
/// leaf blocks only (6-connected), depth-capped so a log adjacent to
/// any visited node within 5 hops keeps the MC max surviving
/// distance of 6. Worst case (orphaned dense canopy) visits a few
/// hundred cells once before the leaf decays.
fn log_supported(world: &infengine_voxel::World, start: [i32; 3]) -> bool {
    const NEIGHBOURS: [[i32; 3]; 6] = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ];
    let mut seen: std::collections::HashSet<[i32; 3]> =
        std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<([i32; 3], u8)> =
        std::collections::VecDeque::new();
    seen.insert(start);
    queue.push_back((start, 0));
    while let Some((p, depth)) = queue.pop_front() {
        for d in NEIGHBOURS {
            let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
            let id = world.get(n).0;
            if is_log_block(id) {
                return true;
            }
            if depth < 5 && is_leaf_block(id) && seen.insert(n) {
                queue.push_back((n, depth + 1));
            }
        }
    }
    false
}

/// Species sapling for a decaying leaf (`<species>_leaves` ->
/// `<species>_sapling` when the registry has it; oak otherwise).
fn sapling_for_leaf(leaf_id: u16) -> u16 {
    const OAK_SAPLING: u16 = 468;
    let Some(b) = crate::block::block_type(leaf_id) else {
        return OAK_SAPLING;
    };
    let Some(species) = b.name.strip_suffix("_leaves") else {
        return OAK_SAPLING;
    };
    let sapling = format!("{species}_sapling");
    crate::block::BLOCK_TYPES
        .iter()
        .find(|t| t.name == sapling)
        .map(|t| t.id)
        .unwrap_or(OAK_SAPLING)
}

/// True when the block directly blocks light from above: a non-air,
/// non-transparent block. Air and cutout decorations (saplings,
/// flowers, glass) count as open.
fn covered(world: &infengine_voxel::World, pos: [i32; 3]) -> bool {
    let above = world.get([pos[0], pos[1] + 1, pos[2]]);
    !above.is_air() && block_is_opaque(above)
}

/// Pure grass-block random-tick decision. Returns the single edit to
/// apply, or `None`.
///
/// - Death: an opaque block directly above smothers the grass; the
///   cell reverts to dirt (MC `GrassBlock` light-starve rule, using
///   "opaque above" as a stand-in for the light check).
/// - Spread: otherwise pick a random neighbour within +/-1 on each
///   axis; if it's dirt with an open (non-opaque) block above, it
///   becomes grass.
pub(crate) fn grass_tick(
    world: &infengine_voxel::World,
    pos: [i32; 3],
    rng: u32,
) -> Option<([i32; 3], BlockId)> {
    if world.get(pos).0 != GRASS_ID {
        return None;
    }
    if covered(world, pos) {
        return Some((pos, BlockId(DIRT_ID)));
    }
    // Random neighbour offset in the 3x3x3 cube around the cell.
    let dx = (rng % 3) as i32 - 1;
    let dy = ((rng / 3) % 3) as i32 - 1;
    let dz = ((rng / 9) % 3) as i32 - 1;
    if dx == 0 && dy == 0 && dz == 0 {
        return None;
    }
    let target = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
    if world.get(target).0 != DIRT_ID || covered(world, target) {
        return None;
    }
    Some((target, BlockId(GRASS_ID)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{Chunk, World};

    fn test_world() -> World {
        let mut w = World::new();
        for cx in 0..=1 {
            for cy in 0..=1 {
                for cz in 0..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
        w
    }

    /// Find an `rng` word whose decoded offset equals `(dx,dy,dz)`.
    fn rng_for_offset(dx: i32, dy: i32, dz: i32) -> u32 {
        for r in 0u32..1000 {
            let ox = (r % 3) as i32 - 1;
            let oy = ((r / 3) % 3) as i32 - 1;
            let oz = ((r / 9) % 3) as i32 - 1;
            if ox == dx && oy == dy && oz == dz {
                return r;
            }
        }
        panic!("no rng for offset");
    }

    #[test]
    fn grass_under_opaque_block_dies_to_dirt() {
        let mut w = test_world();
        w.set([5, 10, 5], BlockId(GRASS_ID));
        w.set([5, 11, 5], BlockId(1)); // stone smothers it
        assert_eq!(grass_tick(&w, [5, 10, 5], 0), Some(([5, 10, 5], BlockId(DIRT_ID))));
    }

    #[test]
    fn grass_spreads_to_adjacent_open_dirt() {
        let mut w = test_world();
        w.set([5, 10, 5], BlockId(GRASS_ID));
        w.set([6, 10, 5], BlockId(DIRT_ID)); // open dirt to the +x
        let rng = rng_for_offset(1, 0, 0);
        assert_eq!(grass_tick(&w, [5, 10, 5], rng), Some(([6, 10, 5], BlockId(GRASS_ID))));
    }

    #[test]
    fn grass_does_not_spread_to_covered_dirt() {
        let mut w = test_world();
        w.set([5, 10, 5], BlockId(GRASS_ID));
        w.set([6, 10, 5], BlockId(DIRT_ID));
        w.set([6, 11, 5], BlockId(1)); // dirt is roofed by stone
        let rng = rng_for_offset(1, 0, 0);
        assert_eq!(grass_tick(&w, [5, 10, 5], rng), None);
    }

    #[test]
    fn grass_tick_on_non_grass_is_none() {
        let mut w = test_world();
        w.set([5, 10, 5], BlockId(1)); // stone, not grass
        assert_eq!(grass_tick(&w, [5, 10, 5], 0), None);
    }

    #[test]
    fn grass_self_offset_is_noop() {
        let mut w = test_world();
        w.set([5, 10, 5], BlockId(GRASS_ID));
        // Offset (0,0,0) targets the grass itself - no spread.
        let rng = rng_for_offset(0, 0, 0);
        assert_eq!(grass_tick(&w, [5, 10, 5], rng), None);
    }

    /// W303: the MC distance rule - a log ADJACENT to the leaf (or
    /// reachable through a leaf bridge) sustains it; a log across an
    /// air gap does not (the old Chebyshev box said it did).
    #[test]
    fn leaf_bridge_to_log_sustains() {
        let mut w = test_world();
        w.set([10, 10, 10], BlockId(crate::block::WOOD_ID));
        // Leaf chain 11..=16 on x: [16] sits at distance 6 (the
        // leaf next to the log is distance 1) - still alive.
        for x in 11..=16 {
            w.set([x, 10, 10], BlockId(crate::block::LEAVES_ID));
        }
        assert!(log_supported(&w, [16, 10, 10]));
        // One more hop = distance 7 - past the MC cap, decays.
        w.set([17, 10, 10], BlockId(crate::block::LEAVES_ID));
        assert!(!log_supported(&w, [17, 10, 10]));
    }

    #[test]
    fn air_gap_breaks_support() {
        let mut w = test_world();
        // Log 2 away but with AIR between - the old box scan kept
        // this leaf alive; the flood-fill lets it decay.
        w.set([12, 10, 10], BlockId(crate::block::WOOD_ID));
        assert!(!log_supported(&w, [10, 10, 10]));
    }

    #[test]
    fn no_log_lets_leaf_decay() {
        let w = test_world(); // empty - no logs anywhere
        assert!(!log_supported(&w, [10, 10, 10]));
    }

    #[test]
    fn species_map_to_their_saplings() {
        // Birch leaves reseed birch, not oak.
        assert_ne!(
            sapling_for_leaf(crate::block::BIRCH_LEAVES_ID),
            sapling_for_leaf(crate::block::LEAVES_ID),
        );
        // Oak fallback for non-leaf ids.
        assert_eq!(sapling_for_leaf(1), 468);
    }
}
