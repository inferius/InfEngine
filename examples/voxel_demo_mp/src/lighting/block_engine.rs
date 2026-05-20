//! Block-light BFS propagation engine.
//!
//! Phase L.2. Mirrors `BlockLightEngine.java` in
//! `net/minecraft/world/level/lighting/`. The engine consumes a
//! flat block grid (`infengine_voxel::World`) and writes nibble-
//! packed block-light values into the per-chunk `LightGrid`.
//!
//! Algorithm (simplified one-phase variant):
//! 1. For every cell inside the affected chunk set, zero the block
//!    layer.
//! 2. Walk the chunks again and seed the BFS queue with every
//!    light-emitting voxel. Emitters write their own cell's value
//!    to their emission level (torch=14, etc).
//! 3. BFS: pop the highest-light cell, for each of the 6 axis
//!    neighbours, compute `new = current - decay(neighbour)` where
//!    decay = 1 for transparent (air, water, leaves) and 15 for
//!    opaque blocks. Opaque cells effectively stop propagation
//!    (a single step subtracts 15, leaving at most 0).
//! 4. Repeat until the queue empties.
//!
//! The conservative "full re-flood every dirty chunk + its 8
//! neighbours" strategy means the engine doesn't need the proper
//! two-phase unlight tracking from MC - we just blow the affected
//! region away and re-derive it from scratch. Cheap at 16^3 chunks.

use std::collections::VecDeque;

use infengine_voxel::{BlockId, CHUNK_SIZE, ChunkCoord};

use super::{LightingState, LightingWorldAccess, block_emission};
use crate::block::block_is_opaque;

/// One step decay through any transparent block (air, water, leaves,
/// glass, ...). Solid opaque blocks use [`OPAQUE_DECAY`] so a single
/// step kills the light.
pub(crate) const TRANSPARENT_DECAY: u8 = 1;
/// Decay through an opaque block. Subtracting this from any
/// `0..=15` light value leaves at most 0, so opaque cells terminate
/// the BFS branch at the face crossing.
pub(crate) const OPAQUE_DECAY: u8 = 15;

/// 6 axis-aligned propagation directions.
const NEIGHBOURS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// World-space `(chunk_coord, local)` from a world voxel coord.
#[inline]
fn split(pos: [i32; 3]) -> (ChunkCoord, [u32; 3]) {
    let cs = CHUNK_SIZE as i32;
    let cc = [
        pos[0].div_euclid(cs),
        pos[1].div_euclid(cs),
        pos[2].div_euclid(cs),
    ];
    let lc = [
        pos[0].rem_euclid(cs) as u32,
        pos[1].rem_euclid(cs) as u32,
        pos[2].rem_euclid(cs) as u32,
    ];
    (cc, lc)
}

/// Per-step decay for `block` (the cell light is PROPAGATING INTO).
/// Air + transparent voxels decay 1 per step; opaque voxels decay
/// 15 (= effectively block the BFS).
///
/// Note: `block::block_is_opaque` treats unknown ids as opaque
/// (it's a chunk-mesher helper where AIR is never queried). For
/// the lighting engines we explicitly fall back to transparent
/// when the block is air so the BFS walks through empty cells.
pub(crate) fn step_decay(block: BlockId) -> u8 {
    if block.is_air() {
        return TRANSPARENT_DECAY;
    }
    if block_is_opaque(block) {
        OPAQUE_DECAY
    } else {
        TRANSPARENT_DECAY
    }
}

/// Zero the block-light layer for every chunk in `affected`. The
/// engine will refill them from emitters during the BFS pass.
fn clear_block_layer(state: &mut LightingState, affected: &[ChunkCoord]) {
    for cc in affected {
        let g = state.ensure(*cc);
        for z in 0..CHUNK_SIZE as u32 {
            for y in 0..CHUNK_SIZE as u32 {
                for x in 0..CHUNK_SIZE as u32 {
                    g.set_block([x, y, z], 0);
                }
            }
        }
    }
}

/// Write `light` into the cell at world coord `pos`, but only when
/// `pos`'s chunk is in `affected`. Returns the new value if the
/// write happened, `None` otherwise.
fn try_set_block_light(
    state: &mut LightingState,
    affected: &[ChunkCoord],
    pos: [i32; 3],
    light: u8,
) -> Option<u8> {
    let (cc, lc) = split(pos);
    if !affected.contains(&cc) {
        return None;
    }
    let g = state.ensure(cc);
    let existing = g.get_block(lc);
    if light > existing {
        g.set_block(lc, light);
        Some(light)
    } else {
        None
    }
}

/// Read block-light at `pos`. Returns 0 for chunks outside `affected`
/// (the BFS treats them as void / unlit).
fn get_block_light(state: &LightingState, affected: &[ChunkCoord], pos: [i32; 3]) -> u8 {
    let (cc, lc) = split(pos);
    if !affected.contains(&cc) {
        return 0;
    }
    state.get(cc).map(|g| g.get_block(lc)).unwrap_or(0)
}

/// Seed the BFS with every emitter inside `affected`. Returns the
/// initial queue of `(pos, light)` tuples sorted as the BFS will
/// see them. Each emitter cell also has its own grid entry written
/// so the BFS-propagate step sees the source level.
fn seed_emitters(
    world: &impl LightingWorldAccess,
    state: &mut LightingState,
    affected: &[ChunkCoord],
) -> VecDeque<([i32; 3], u8)> {
    let mut queue: VecDeque<([i32; 3], u8)> = VecDeque::new();
    for cc in affected {
        let cs = CHUNK_SIZE as i32;
        let base = [cc[0] * cs, cc[1] * cs, cc[2] * cs];
        for z in 0..CHUNK_SIZE as i32 {
            for y in 0..CHUNK_SIZE as i32 {
                for x in 0..CHUNK_SIZE as i32 {
                    let p = [base[0] + x, base[1] + y, base[2] + z];
                    let id = world.get_block(p).0;
                    let emit = block_emission(id);
                    if emit > 0
                        && let Some(v) = try_set_block_light(state, affected, p, emit)
                    {
                        queue.push_back((p, v));
                    }
                }
            }
        }
    }
    queue
}

/// BFS-propagate the seeded queue across the affected region.
fn propagate(world: &impl LightingWorldAccess, state: &mut LightingState, affected: &[ChunkCoord], mut queue: VecDeque<([i32; 3], u8)>) {
    while let Some((pos, level)) = queue.pop_front() {
        if level <= 1 {
            // A 1-light cell can only push 0 -> nothing to add.
            continue;
        }
        // Re-check the stored level in case a later seed wrote a
        // higher value at the same cell - we want to use the
        // freshest level for propagation.
        let stored = get_block_light(state, affected, pos);
        if stored > level {
            // A later push superseded this entry; the higher
            // value's own queue entry will handle propagation.
            continue;
        }
        for n in NEIGHBOURS {
            let np = [pos[0] + n[0], pos[1] + n[1], pos[2] + n[2]];
            let nb = world.get_block(np);
            let decay = step_decay(nb);
            let new_level = level.saturating_sub(decay);
            if new_level == 0 {
                continue;
            }
            if let Some(v) = try_set_block_light(state, affected, np, new_level) {
                queue.push_back((np, v));
            }
        }
    }
}

/// Full re-flood of `affected`'s block-light layer from emitters.
///
/// 1. Zero every cell in the affected chunks.
/// 2. Seed the BFS from every emitter inside the affected region.
/// 3. Walk neighbours one step at a time, decaying by `step_decay`
///    until no cell has a higher value to push.
///
/// Cells outside `affected` are treated as if their block-light
/// is 0; nothing is written to them. This means a torch right next
/// to the boundary of the affected set won't bleed light into
/// chunks we haven't been asked to relight (call sites compensate
/// by including the 8 neighbour chunks of any edited chunk).
pub(crate) fn propagate_block_light(
    world: &impl LightingWorldAccess,
    state: &mut LightingState,
    affected: &[ChunkCoord],
) {
    clear_block_layer(state, affected);
    let queue = seed_emitters(world, state, affected);
    propagate(world, state, affected, queue);
}
