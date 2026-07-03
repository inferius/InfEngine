//! Liquid simulation: data, planning, applying, and rendering helpers.
//!
//! Sub-modules:
//! - `plan` - pure MC-faithful planner. Pulls each non-source cell
//!   through `getNewLiquid`, then runs `spread` to push downward
//!   and into ranked side targets.
//! - `apply` - mutates world / level / active / positions from a
//!   `LiquidPlanStep` and emits a `LiquidTape` describing changes.
//! - `merge` - source-promotion pass (the infinite-water trick).
//! - `mesh` - corner-height + flow-vector + drop-distance helpers
//!   used by the renderer.
//! - `tests` - shared `LiquidSim` driver + the full behaviour suite.
//!
//! The split was forced by the file-size policy
//! (`docs/design/file_size_policy.md`): the unified `liquid.rs` had
//! crossed the 1000-LOC hard cap.

pub(crate) mod apply;
pub(crate) mod boundary;
pub(crate) mod merge;
pub(crate) mod mesh;
pub(crate) mod plan;
pub(crate) mod schedule;

#[cfg(test)]
mod tests;

use crate::block::{LAVA_ID, WATER_ID};
use infengine_voxel::BlockId;

// Public surface used by `main.rs` and other modules.
pub(crate) use apply::liquid_apply_plan;
pub(crate) use merge::liquid_merge_step;
pub(crate) use mesh::{
    bottom_open, counts_as_liquid, liquid_corner_height_at, push_quad_flow,
    side_open,
};
// `distance_to_drop_at` and `liquid_plan_step` are used by the
// test sim only; re-exporting them under `cfg(test)` keeps the
// public surface clean while still giving `tests.rs` direct
// access through `super::*`.
#[cfg(test)]
pub(crate) use mesh::distance_to_drop_at;
pub(crate) use plan::{LiquidPlanStep, liquid_plan_cell};
#[cfg(test)]
pub(crate) use plan::liquid_plan_step;

// ---------------------------------------------------------------
// Liquid registry.
// ---------------------------------------------------------------

/// Per-liquid physics parameters. The generic `tick_liquid_*`
/// passes read these so adding a new liquid kind = one
/// `Liquid` row + one `BlockType` entry with the matching
/// `F_SWIM | F_DEDICATED_RENDER | F_TRANSPARENT_RENDER` flags.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Liquid {
    /// `BlockId.0` for this liquid's voxel state. Source +
    /// flowing share the same id; the `liquid_level` map
    /// distinguishes them.
    pub(crate) block_id: u16,
    /// Horizontal spread distance cap from any source. MC water
    /// = 7, MC overworld lava = 3.
    pub(crate) max_range: u8,
    /// Per-step level penalty applied each cell when spreading
    /// horizontally. MC's `FlowingFluid.getDropOff` returns 1
    /// for water and 2 for overworld lava - the higher drop_off
    /// makes lava thin out faster (visible as a steeper
    /// own_height ramp). Source level (0) -> first ring level
    /// = drop_off; second ring = 2 * drop_off; etc.
    pub(crate) drop_off: u8,
    /// Net ticks between spread + merge + decay updates. MC
    /// ticks are 50 ms; a net tick is ~16 ms (60 Hz), so water
    /// (every 5 game ticks = 250 ms) maps to ~15 net ticks
    /// here. Lava overworld (every 30 game ticks = 1500 ms)
    /// maps to ~90.
    pub(crate) tick_period: u32,
    /// Legacy field from the split fall+spread scheduler. The
    /// unified `tick_liquid_step` runs both in the same pass,
    /// so this is unused today. Kept for future per-fluid
    /// tuning (e.g. lava that streams down slower than water).
    #[allow(dead_code)]
    pub(crate) fall_period: u32,
    /// Whether two adjacent sources promote a flow voxel between
    /// them to a source. MC: water = true, lava = false.
    pub(crate) infinite_source: bool,
    /// Horizontal velocity multiplier while a player is
    /// submerged in this liquid. Lower = thicker drag. Water
    /// = 0.45, lava = 0.20.
    pub(crate) swim_horiz_drag: f32,
    /// Y-velocity floor while submerged (negative number = how
    /// fast the player can sink). Water -1.8, lava -0.5.
    pub(crate) swim_fall_cap: f32,
    /// Y-velocity set when the player holds the jump key while
    /// submerged (active swim-up). Water 2.2, lava 1.0.
    pub(crate) swim_jump_vel: f32,
    /// Passive upward acceleration applied every frame (units:
    /// m/s^2) when the player is submerged without holding
    /// jump. Models buoyancy: water > gravity = drift up;
    /// lava < gravity = slow sink. Capped at +1.0 m/s.
    pub(crate) swim_buoyancy: f32,
    /// Mining-time multiplier applied to every block the
    /// player breaks while submerged. Water 5.0, lava 5.0 -
    /// MC slows underwater mining drastically.
    pub(crate) submerged_mining_mul: f32,
    /// HP damage per second the player takes while submerged.
    /// Water 0.0, lava 4.0.
    pub(crate) submerged_damage_per_sec: f32,
    /// Whether being submerged in this liquid drains the
    /// player's air meter (and triggers drowning damage when
    /// the meter hits zero). Water = true, lava = false (lava
    /// burns directly via `submerged_damage_per_sec`).
    pub(crate) drains_air: bool,
    /// RGBA tint composited as a full-screen overlay while the
    /// player is submerged. Water reads as a cool blue haze,
    /// lava as a bright orange wash. Alpha controls intensity.
    pub(crate) submerged_tint: [f32; 4],
    /// Horizontal velocity (m/s) the current pushes the player
    /// in the flow direction while standing in a flow voxel.
    /// Source cells have no flow so no push. Water 4.0,
    /// lava 1.5 (sticky).
    pub(crate) current_strength: f32,
}

/// Registry of every flowing liquid. The tick loop iterates this
/// table and runs the shared physics with each liquid's params,
/// so the code path is "this is a liquid" rather than
/// "this is water".
pub(crate) const LIQUIDS: &[Liquid] = &[
    Liquid {
        block_id: WATER_ID,
        max_range: 7,
        drop_off: 1,
        // W104: units are now fixed 20 Hz game ticks (see tick_liquids),
        // so these match MC directly: water spreads every 5 ticks
        // (250 ms) and a falling column advances every 2 ticks (100 ms).
        tick_period: 5,
        fall_period: 2,
        infinite_source: true,
        swim_horiz_drag: 0.45,
        swim_fall_cap: -1.8,
        swim_jump_vel: 2.2,
        swim_buoyancy: 12.0,
        submerged_mining_mul: 5.0,
        submerged_damage_per_sec: 0.0,
        drains_air: true,
        submerged_tint: [0.10, 0.30, 0.60, 0.40],
        current_strength: 4.0,
    },
    Liquid {
        block_id: LAVA_ID,
        // MC overworld lava: drop_off = 2, max amount = 7
        // -> 3 cells of flow before evaporation. In our level
        // convention (0 = source, level += drop_off per ring),
        // that's levels 2, 4, 6 -> 3 rings, so max_range = 7
        // and drop_off = 2 lines up with MC.
        max_range: 7,
        drop_off: 2,
        // W104: fixed 20 Hz game ticks. MC overworld lava spreads every
        // 30 ticks (1.5 s); a falling lava column advances every 10
        // ticks (0.5 s).
        tick_period: 30,
        fall_period: 10,
        infinite_source: false,
        swim_horiz_drag: 0.20,
        swim_fall_cap: -0.5,
        swim_jump_vel: 1.0,
        swim_buoyancy: 4.0,
        submerged_mining_mul: 5.0,
        submerged_damage_per_sec: 4.0,
        drains_air: false,
        submerged_tint: [1.00, 0.35, 0.05, 0.55],
        current_strength: 1.5,
    },
];

/// Look up a `Liquid` definition for a placed block. Returns
/// `None` for non-liquid voxels (every regular solid block).
pub(crate) fn liquid_for(b: BlockId) -> Option<&'static Liquid> {
    LIQUIDS.iter().find(|l| l.block_id == b.0)
}
