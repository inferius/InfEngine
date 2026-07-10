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
const RADIUS_CHUNKS: i32 = 4;

/// Cells sampled per eligible chunk per frame, before the global cap.
const CELLS_PER_CHUNK: usize = 1;

/// Hard per-frame ceiling on random-tick cell visits, independent of
/// how many chunks are loaded. Most samples land on air / stone and
/// no-op, so this is generous.
const MAX_CELLS_PER_TICK: usize = 256;

/// Inverse chance a sampled sapling advances toward a tree on a given
/// random tick (1 in N). Higher = slower growth.
const SAPLING_GROW_DENOM: u32 = 8;

/// Chebyshev radius searched for a sustaining log around a leaf. A
/// leaf with no `WOOD_ID` log inside this box decays. (MC uses a
/// distance-7 flood-fill through leaves; the box scan is the cheap
/// stateless stand-in.)
const LEAF_DECAY_RADIUS: i32 = 4;

/// Inverse chance a decaying leaf drops an oak sapling (1 in N).
const LEAF_SAPLING_DROP_DENOM: u32 = 20;

/// Inverse chance a sampled immature crop advances to mature on a
/// given random tick (1 in N). Higher = slower growth.
const CROP_GROW_DENOM: u32 = 6;

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
    let (cy_min, cy_max) = Voxel::vertical_chunk_range(vx.player_pos.y);
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
    } else if id == crate::block::LEAVES_ID {
        leaf_tick(vx, pos, rng);
    } else if crate::world::crop::is_immature_crop(id) {
        crop_tick(vx, pos, rng);
    }
}

/// Immature-crop random tick: on a fraction of ticks, if the crop
/// stands on soil (dirt / grass), advance it to the mature stage.
/// Two-stage growth; MC's finer stage chain + farmland hydration are
/// a follow-up.
fn crop_tick(vx: &mut Voxel, pos: [i32; 3], rng: u32) {
    if !rng.is_multiple_of(CROP_GROW_DENOM) {
        return;
    }
    let below = vx.world.get([pos[0], pos[1] - 1, pos[2]]).0;
    if below != DIRT_ID && below != GRASS_ID {
        return;
    }
    let id = vx.world.get(pos).0;
    if let Some(mature) = crate::world::crop::mature_for(id) {
        crate::world::edit::emit(vx, pos, BlockId(mature));
    }
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

/// Leaf random tick: a tree-generated leaf with no sustaining log in
/// range decays to air. Player-placed leaves (tracked in
/// `persistent_leaves`) are exempt. On decay there's a small chance
/// to drop an oak sapling, so chopping a tree can reseed.
fn leaf_tick(vx: &mut Voxel, pos: [i32; 3], rng: u32) {
    if vx.persistent_leaves.contains(&pos) {
        return;
    }
    if has_log_within(&vx.world, pos, LEAF_DECAY_RADIUS) {
        return;
    }
    crate::world::edit::emit(vx, pos, BlockId::AIR);
    if rng.is_multiple_of(LEAF_SAPLING_DROP_DENOM) {
        // Oak sapling (id 468) - replantable drop.
        vx.spawn_drop(BlockId(468), pos);
    }
}

/// True when a `WOOD_ID` log sits within `r` (Chebyshev) of `pos`.
/// Early-exits on the first hit so leaves near a trunk are cheap;
/// only orphaned leaves pay the full box scan before decaying.
fn has_log_within(world: &infengine_voxel::World, pos: [i32; 3], r: i32) -> bool {
    for dx in -r..=r {
        for dy in -r..=r {
            for dz in -r..=r {
                if world.get([pos[0] + dx, pos[1] + dy, pos[2] + dz]).0
                    == crate::block::WOOD_ID
                {
                    return true;
                }
            }
        }
    }
    false
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

    #[test]
    fn log_within_radius_sustains_leaf() {
        let mut w = test_world();
        w.set([10, 10, 10], BlockId(crate::block::WOOD_ID));
        // Leaf 3 cells away on x - inside the radius-4 box.
        assert!(has_log_within(&w, [13, 10, 10], LEAF_DECAY_RADIUS));
    }

    #[test]
    fn no_log_in_range_lets_leaf_decay() {
        let w = test_world(); // empty - no logs anywhere
        assert!(!has_log_within(&w, [10, 10, 10], LEAF_DECAY_RADIUS));
    }

    #[test]
    fn log_just_outside_radius_does_not_sustain() {
        let mut w = test_world();
        // Log 5 cells away on x, radius is 4 -> out of range.
        w.set([15, 10, 10], BlockId(crate::block::WOOD_ID));
        assert!(!has_log_within(&w, [10, 10, 10], LEAF_DECAY_RADIUS));
    }
}
