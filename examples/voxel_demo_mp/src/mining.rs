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
//! The driving tick loop still lives in `Voxel::update_play`
//! (the SFX + particle side effects need a deep `&mut self`
//! borrow). Pure progress arithmetic is split out as
//! `step_mining_session` (W47.13b) so the inline block stays
//! at "consume outcome" rather than "mutate clock + decide
//! flags inline".

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

/// Outcome of one frame of progress on an active mining session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MiningStep {
    /// True when the periodic dig SFX + crack-particle burst is
    /// due this frame (either the cooldown expired or the session
    /// just completed). Caller fires the side effects.
    pub fire_fx: bool,
    /// True when total progress reached the mining target; caller
    /// records the break and clears the session.
    pub done: bool,
}

/// W47.13b: advance a `MiningState` by `dt` and report whether the
/// dig FX should fire this frame + whether the session completed.
/// Pure arithmetic on the session struct; all side effects (SFX,
/// particles, world edit) stay in the caller.
pub(crate) fn step_mining_session(state: &mut MiningState, dt: f32) -> MiningStep {
    state.progress += dt;
    state.fx_cooldown -= dt;
    let done = state.progress >= state.total;
    let fire_fx = state.fx_cooldown <= 0.0 || done;
    if fire_fx {
        state.fx_cooldown = 0.22;
    }
    MiningStep { fire_fx, done }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh(total: f32) -> MiningState {
        MiningState {
            voxel: [0, 0, 0],
            progress: 0.0,
            total,
            block: BlockId(1),
            fx_cooldown: 0.0,
        }
    }

    #[test]
    fn first_step_fires_fx_and_resets_cooldown() {
        let mut s = fresh(1.0);
        let step = step_mining_session(&mut s, 0.05);
        assert!(step.fire_fx);
        assert!(!step.done);
        assert!((s.fx_cooldown - 0.22).abs() < 1e-5);
    }

    #[test]
    fn step_completes_when_progress_meets_total() {
        let mut s = fresh(0.10);
        let step = step_mining_session(&mut s, 0.20);
        assert!(step.done);
        assert!(step.fire_fx);
    }

    #[test]
    fn mid_session_no_fx_when_cooldown_positive() {
        let mut s = fresh(1.0);
        s.fx_cooldown = 0.18;
        let step = step_mining_session(&mut s, 0.05);
        assert!(!step.fire_fx);
        assert!(!step.done);
        assert!((s.fx_cooldown - 0.13).abs() < 1e-5);
    }

    #[test]
    fn cooldown_resets_on_completion_even_with_positive_cooldown() {
        let mut s = fresh(0.10);
        s.fx_cooldown = 0.50;
        let step = step_mining_session(&mut s, 0.20);
        assert!(step.done);
        assert!(step.fire_fx);
        assert!((s.fx_cooldown - 0.22).abs() < 1e-5);
    }
}
