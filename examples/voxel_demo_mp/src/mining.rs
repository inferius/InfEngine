//! Active mining session state + per-block hardness lookup.
//!
//! `MiningState` is the small struct on `Voxel` that tracks a
//! single hold-to-mine session: which voxel the player is
//! breaking, how much progress they've made toward the total
//! mine time, and the SFX/particle cooldown that paces the
//! crack burst. The lookup table `block_hardness` is the
//! baseline mine time in seconds per block kind, before
//! tool-effectiveness multipliers (handled in `block::`) and
//! submerged slowdown (handled in `player::submerged_*`)
//! adjust it.
//!
//! The actual tick loop that advances a session toward
//! completion lives inline in `Voxel::update_play` - it
//! interleaves with mob/PvP attack arbitration and the
//! `mine_break` -> world edit broadcast path, so splitting it
//! out would obscure the play-update flow without a real
//! readability win. This module is just the data + the
//! per-block constant.

use infengine_voxel::BlockId;

/// In-progress mining session. Created when the player starts
/// holding LMB on a solid voxel, advanced each frame in
/// `update_play`, and torn down either when the break completes
/// (the voxel goes to air via the edit broadcast path) or when
/// LMB releases / the aim shifts / the target voxel disappears
/// because someone else broke it first.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MiningState {
    pub(crate) voxel: [i32; 3],
    pub(crate) progress: f32,
    pub(crate) total: f32,
    pub(crate) block: BlockId,
    /// Time (seconds) since the last dig sound + crack particle
    /// burst. Reset to 0 each time the periodic effect fires.
    pub(crate) fx_cooldown: f32,
}

/// Seconds required to fully mine each block kind. Higher =
/// tougher material. Air falls through to a tiny "miss" delay
/// so the swing still produces feedback. Multiplied by
/// `tool_effectiveness` (block module) and the submerged-eye
/// liquid slowdown (player module) before driving the session.
pub(crate) fn block_hardness(block: BlockId) -> f32 {
    match block.0 {
        0 => 0.05,        // air (no-op swing)
        1 | 8 => 1.6,     // stone, cobble
        2 | 3 => 0.55,    // dirt, grass
        4 => 0.5,         // sand
        5 => 1.0,         // wood log
        6 => 0.25,        // leaves
        7 => 0.9,         // planks
        9 => 0.45,        // glass
        10 => 0.1,        // torch
        11 => 0.6,        // wool
        12 => 0.4,        // meat
        13 => 2.4,        // coal ore
        14 => 3.2,        // iron ore
        15 => 0.4,        // snow
        _ => 0.8,
    }
}
