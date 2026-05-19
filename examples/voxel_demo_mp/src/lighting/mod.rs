//! Per-voxel lighting subsystem.
//!
//! Phase L (Wave 3): block + sky light propagation with a day/night
//! shader multiplier. Lights are stored per-chunk as nibble-packed
//! `LightGrid`s (4 bits per cell per layer) and refreshed by two
//! BFS engines:
//!
//! - `block_engine.rs` floods block-emitted light (torch=14,
//!   glowstone/lava/fire=15) outward through transparent voxels.
//! - `sky_engine.rs` runs a top-down column walk for daylight then
//!   a horizontal flood for caves / overhangs.
//!
//! Lighting is NEVER persisted on disk (matches MC) - the runtime
//! re-derives both layers from the block grid on world load + on
//! dim switch.
//!
//! Module layout mirrors MC's `net/minecraft/world/level/lighting`
//! package at a coarse level. Each engine is the "incremental"
//! variant simplified to a conservative full re-flood of the
//! touched chunk + its 8 neighbours per block edit.

use std::collections::{HashMap, HashSet};

use infengine_voxel::ChunkCoord;

pub(crate) mod grid;
pub(crate) mod block_engine;
pub(crate) mod sky_engine;

#[cfg(test)]
mod grid_tests;
#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod day_factor_tests;
#[cfg(test)]
mod mesh_integration_tests;

pub(crate) use grid::LightGrid;

/// Per-`Voxel` lighting state: one `LightGrid` per loaded chunk
/// plus a dirty-set of world voxel positions that need re-lighting
/// on the next lighting tick.
///
/// Lives parallel to `World.chunks`. A chunk's grid is created
/// lazily on the first `get_or_init`; entries get cleared when the
/// chunk unloads (dim swap, save unload). The `dirty` set is drained
/// each lighting tick and translates voxel coords into the set of
/// chunks that need a full re-flood pass.
#[derive(Debug, Default)]
pub(crate) struct LightingState {
    /// Loaded chunks -> their nibble-packed light grids.
    pub(crate) grids: HashMap<ChunkCoord, LightGrid>,
    /// World voxel positions touched since the last lighting tick.
    /// Drained by `take_dirty` + fed back as chunk coords to the
    /// engines.
    pub(crate) dirty: HashSet<[i32; 3]>,
}

impl LightingState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Drop every stored grid + flush the dirty set. Used on
    /// dim switch (the active dim's chunks completely change) so
    /// the next access re-derives lighting from scratch.
    pub(crate) fn clear(&mut self) {
        self.grids.clear();
        self.dirty.clear();
    }

    /// Mark `voxel` as needing a re-light on the next lighting
    /// tick. The exact recompute strategy lives in the engine
    /// modules; this just records the request.
    pub(crate) fn mark_dirty(&mut self, voxel: [i32; 3]) {
        self.dirty.insert(voxel);
    }

    /// Get-or-create the `LightGrid` for `chunk`. Brand-new entries
    /// start fully zeroed; the engines populate them.
    pub(crate) fn ensure(&mut self, chunk: ChunkCoord) -> &mut LightGrid {
        self.grids.entry(chunk).or_insert_with(LightGrid::empty)
    }

    /// Borrow the grid for `chunk` without creating one. Returns
    /// `None` for chunks the engines have never visited (treated
    /// by callers as "all zero" / "treat as full sky 15" depending
    /// on context).
    pub(crate) fn get(&self, chunk: ChunkCoord) -> Option<&LightGrid> {
        self.grids.get(&chunk)
    }

    /// Drain the dirty set, returning all queued voxel positions.
    /// Caller is expected to feed them into the engines + clear
    /// downstream state.
    pub(crate) fn take_dirty(&mut self) -> HashSet<[i32; 3]> {
        std::mem::take(&mut self.dirty)
    }

    /// Drop the grid for `chunk` if present. Used on chunk unload.
    #[allow(dead_code)] // wired up in a follow-up commit that streams chunks out.
    pub(crate) fn forget(&mut self, chunk: ChunkCoord) {
        self.grids.remove(&chunk);
    }
}

/// Block-emission lookup. Mirrors MC's `BlockBehaviour.lightEmission`
/// constants in `net/minecraft/world/level/block/Blocks.java`:
/// torch=14, glowstone=15, lava=15, fire=15. Every other block id
/// (including air) returns 0. Block ids resolve via the constants
/// declared in `crate::block`.
///
/// Kept here (not in `block/mod.rs`) so the lighting module owns
/// its own emitter table - the block table is the source of truth
/// for the IDs themselves, lighting just consults them.
pub(crate) fn block_emission(id: u16) -> u8 {
    use crate::block::{FIRE_ID, GLOWSTONE_ID, LAVA_ID, TORCH_ID};
    match id {
        TORCH_ID => 14,
        GLOWSTONE_ID | LAVA_ID | FIRE_ID => 15,
        _ => 0,
    }
}

// ---------------------------------------------------------------
// Day / night ramp.
// ---------------------------------------------------------------

/// Map a `(day_time, day_length_s)` pair to the shader's sky-light
/// multiplier.
///
/// Returns a value in `[0.04, 1.0]`:
/// - Day (phase ~0.0..0.5): 1.0 (full daylight).
/// - Night (phase ~0.55..0.95): 0.04 (faint moonlight floor).
/// - Dawn (0.5..0.55) + Dusk (0.95..1.0): smoothstep ramp.
///
/// Phase is computed from `day_time / day_length_s` and wrapped via
/// `rem_euclid(1.0)` so the function is total over the real line.
/// The shader reads this as `vec2<f32>(block_light_norm,
/// sky_light_norm * day_factor)` then takes a max for the effective
/// light value.
pub(crate) fn compute_day_factor(day_time: f32, day_length_s: f32) -> f32 {
    let phase = (day_time / day_length_s.max(1.0)).rem_euclid(1.0);
    // Cubic smoothstep: 3t^2 - 2t^3.
    fn smoothstep(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
    const NIGHT_FLOOR: f32 = 0.04;
    const DAY_PEAK: f32 = 1.0;
    if phase < 0.5 {
        // Full day.
        DAY_PEAK
    } else if phase < 0.55 {
        // Dusk: lerp from day -> night via smoothstep.
        let t = (phase - 0.5) / 0.05;
        let s = smoothstep(t);
        DAY_PEAK + (NIGHT_FLOOR - DAY_PEAK) * s
    } else if phase < 0.95 {
        NIGHT_FLOOR
    } else if phase < 1.0 {
        // Dawn: lerp from night -> day.
        let t = (phase - 0.95) / 0.05;
        let s = smoothstep(t);
        NIGHT_FLOOR + (DAY_PEAK - NIGHT_FLOOR) * s
    } else {
        DAY_PEAK
    }
}
