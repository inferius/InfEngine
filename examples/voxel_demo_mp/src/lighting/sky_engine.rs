//! Sky-light propagation engine.
//!
//! Phase L.3. Mirrors `SkyLightEngine.java` in
//! `net/minecraft/world/level/lighting/`. Two phases:
//!
//! 1. **Column walk**: for every `(x, z)` column inside the affected
//!    region, walk from the top of the region downward. Above the
//!    region the sky is 15. While the cell is transparent the sky
//!    light passes through unchanged (decay = 0 vertically straight
//!    down, per MC). The first opaque cell blocks the column; the
//!    rest of the column writes 0.
//! 2. **Horizontal flood**: BFS from every lit cell into its 6
//!    neighbours with the standard 1-per-step decay (15 through
//!    opaque) - the same algorithm as the block-light engine. This
//!    lights up overhangs + cave entries from open columns.
//!
//! The conservative full re-flood strategy (matches the block
//! engine) keeps the implementation small; MC's incremental
//! `setSkyLight*` paths are a follow-up.

use std::collections::VecDeque;

use infengine_voxel::{CHUNK_SIZE, ChunkCoord, World};

use super::LightingState;
use super::block_engine::step_decay;
use crate::block::block_is_opaque;

const SKY_FULL: u8 = 15;

const NEIGHBOURS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

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

/// Zero the sky-light layer in every affected chunk.
fn clear_sky_layer(state: &mut LightingState, affected: &[ChunkCoord]) {
    for cc in affected {
        let g = state.ensure(*cc);
        for z in 0..CHUNK_SIZE as u32 {
            for y in 0..CHUNK_SIZE as u32 {
                for x in 0..CHUNK_SIZE as u32 {
                    g.set_sky([x, y, z], 0);
                }
            }
        }
    }
}

fn try_set_sky_light(
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
    let existing = g.get_sky(lc);
    if light > existing {
        g.set_sky(lc, light);
        Some(light)
    } else {
        None
    }
}

fn get_sky_light(state: &LightingState, affected: &[ChunkCoord], pos: [i32; 3]) -> u8 {
    let (cc, lc) = split(pos);
    if !affected.contains(&cc) {
        return 0;
    }
    state.get(cc).map(|g| g.get_sky(lc)).unwrap_or(0)
}

/// Y-range spanned by `affected`. Returns `(y_min_inclusive,
/// y_max_inclusive)` as world voxel coords.
fn y_range(affected: &[ChunkCoord]) -> Option<(i32, i32)> {
    if affected.is_empty() {
        return None;
    }
    let cs = CHUNK_SIZE as i32;
    let mut y_min = i32::MAX;
    let mut y_max = i32::MIN;
    for cc in affected {
        let lo = cc[1] * cs;
        let hi = lo + cs - 1;
        if lo < y_min {
            y_min = lo;
        }
        if hi > y_max {
            y_max = hi;
        }
    }
    Some((y_min, y_max))
}

/// Set of unique `(x, z)` columns in the affected region.
fn affected_columns(affected: &[ChunkCoord]) -> Vec<(i32, i32)> {
    let cs = CHUNK_SIZE as i32;
    let mut out: Vec<(i32, i32)> = Vec::new();
    for cc in affected {
        let bx = cc[0] * cs;
        let bz = cc[2] * cs;
        for dz in 0..cs {
            for dx in 0..cs {
                let p = (bx + dx, bz + dz);
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Column-walk pass: for each `(x, z)` column inside the affected
/// region, walk from `y_max` downward. Sky starts at 15 above the
/// world ceiling; each opaque cell zeros the rest of the column.
///
/// Returns the queue of cells seeded with sky=15 (the open top of
/// each affected column) for the horizontal BFS to flood out from.
/// MC's `SkyLightEngine.checkNode` does the same: opens the column
/// then re-floods adjacent cells.
fn column_walk(
    world: &World,
    state: &mut LightingState,
    affected: &[ChunkCoord],
    y_min: i32,
    y_max: i32,
) -> VecDeque<([i32; 3], u8)> {
    let mut queue: VecDeque<([i32; 3], u8)> = VecDeque::new();
    for (x, z) in affected_columns(affected) {
        let mut light = SKY_FULL;
        for y in (y_min..=y_max).rev() {
            let p = [x, y, z];
            let b = world.get(p);
            // Air is never opaque; `block_is_opaque` treats
            // unknown ids as opaque so we have to short-circuit
            // air ourselves (matches block_engine::step_decay).
            let opaque = !b.is_air() && block_is_opaque(b);
            if opaque {
                // Opaque blocks the column. Everything below
                // starts at 0 + only gets re-lit by the
                // horizontal BFS.
                light = 0;
                // Still write zero so the layer is consistent.
                let _ = try_set_sky_light(state, affected, p, 0);
                continue;
            }
            // Transparent cell - keep current column light.
            if let Some(v) = try_set_sky_light(state, affected, p, light) {
                queue.push_back((p, v));
            }
        }
    }
    queue
}

fn propagate(
    world: &World,
    state: &mut LightingState,
    affected: &[ChunkCoord],
    mut queue: VecDeque<([i32; 3], u8)>,
) {
    while let Some((pos, level)) = queue.pop_front() {
        if level <= 1 {
            continue;
        }
        let stored = get_sky_light(state, affected, pos);
        if stored > level {
            continue;
        }
        for n in NEIGHBOURS {
            let np = [pos[0] + n[0], pos[1] + n[1], pos[2] + n[2]];
            let nb = world.get(np);
            let decay = step_decay(nb);
            let new_level = level.saturating_sub(decay);
            if new_level == 0 {
                continue;
            }
            if let Some(v) = try_set_sky_light(state, affected, np, new_level) {
                queue.push_back((np, v));
            }
        }
    }
}

/// Full re-flood of the sky layer for the affected chunks.
///
/// 1. Zero every cell's sky value in `affected`.
/// 2. Column-walk from `y_max` to `y_min` inside each affected
///    column. Transparent cells inherit 15 from above; opaque
///    cells block the column.
/// 3. BFS-flood from the lit cells into their 6 neighbours with
///    the standard 1-per-step decay through transparent voxels
///    (15 through opaque, killing the branch).
pub(crate) fn propagate_sky_light(
    world: &World,
    state: &mut LightingState,
    affected: &[ChunkCoord],
) {
    clear_sky_layer(state, affected);
    let Some((y_min, y_max)) = y_range(affected) else {
        return;
    };
    let queue = column_walk(world, state, affected, y_min, y_max);
    propagate(world, state, affected, queue);
}
