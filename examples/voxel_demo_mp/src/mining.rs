//! Active mining session state.
//!
//! `MiningState` is the small struct on `Voxel` that tracks a
//! single hold-to-mine session: which voxel the player is
//! breaking, how much cumulative destroy progress they have made,
//! and the SFX/particle cooldown that paces the crack burst.
//!
//! W473: the per-block hardness lookup moved to the generated
//! `block::hardness_table` + `block::mining_math` (a 1:1 port of
//! MC `getDestroyProgress`). Progress now accumulates in MC
//! progress units - the session completes at `progress >= total`
//! with `total = 1.0`, and the per-second rate comes from
//! `mining_math::destroy_progress_per_second` (re-evaluated every
//! frame, so tool swaps / water / airborne modifiers apply
//! mid-session like MC's per-tick check).
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
    /// Cumulative MC destroy progress (0..=total).
    pub(crate) progress: f32,
    /// Completion threshold - 1.0 in MC progress units. The crack
    /// overlay + HUD percentage derive from `progress / total`.
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

/// W47.13b/W473: advance a `MiningState` by `dt` at the given
/// per-second destroy-progress rate and report whether the dig FX
/// should fire this frame + whether the session completed.
///
/// `rate_per_sec` comes from
/// `block::mining_math::destroy_progress_per_second`: 0 for
/// unbreakable blocks (the session never completes), infinity for
/// instabreak blocks (completes immediately - handled explicitly
/// so a zero `dt` frame cannot produce a NaN).
///
/// Pure arithmetic on the session struct; all side effects (SFX,
/// particles, world edit) stay in the caller.
pub(crate) fn step_mining_session(
    state: &mut MiningState,
    dt: f32,
    rate_per_sec: f32,
) -> MiningStep {
    if rate_per_sec.is_infinite() {
        state.progress = state.total;
    } else {
        state.progress += rate_per_sec * dt;
    }
    state.fx_cooldown -= dt;
    let done = state.progress >= state.total;
    let fire_fx = state.fx_cooldown <= 0.0 || done;
    if fire_fx {
        state.fx_cooldown = 0.22;
    }
    MiningStep { fire_fx, done }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> MiningState {
        MiningState {
            voxel: [0, 0, 0],
            progress: 0.0,
            total: 1.0,
            block: BlockId(1),
            fx_cooldown: 0.0,
        }
    }

    #[test]
    fn first_step_fires_fx_and_resets_cooldown() {
        let mut s = fresh();
        let step = step_mining_session(&mut s, 0.05, 1.0);
        assert!(step.fire_fx);
        assert!(!step.done);
        assert!((s.fx_cooldown - 0.22).abs() < 1e-5);
    }

    #[test]
    fn step_completes_when_progress_meets_total() {
        let mut s = fresh();
        // 0.2 s at 10 progress/s covers the 1.0 total.
        let step = step_mining_session(&mut s, 0.20, 10.0);
        assert!(step.done);
        assert!(step.fire_fx);
    }

    #[test]
    fn mid_session_no_fx_when_cooldown_positive() {
        let mut s = fresh();
        s.fx_cooldown = 0.18;
        let step = step_mining_session(&mut s, 0.05, 1.0);
        assert!(!step.fire_fx);
        assert!(!step.done);
        assert!((s.fx_cooldown - 0.13).abs() < 1e-5);
    }

    #[test]
    fn cooldown_resets_on_completion_even_with_positive_cooldown() {
        let mut s = fresh();
        s.fx_cooldown = 0.50;
        let step = step_mining_session(&mut s, 0.20, 10.0);
        assert!(step.done);
        assert!(step.fire_fx);
        assert!((s.fx_cooldown - 0.22).abs() < 1e-5);
    }

    /// W473: an unbreakable block (rate 0 - bedrock) never
    /// completes no matter how long the button is held.
    #[test]
    fn zero_rate_never_completes() {
        let mut s = fresh();
        for _ in 0..10_000 {
            let step = step_mining_session(&mut s, 0.1, 0.0);
            assert!(!step.done);
        }
        assert_eq!(s.progress, 0.0);
    }

    /// W473: an instabreak block (infinite rate - torch) completes
    /// on the first frame, even a zero-dt one.
    #[test]
    fn infinite_rate_completes_immediately() {
        let mut s = fresh();
        let step = step_mining_session(&mut s, 0.0, f32::INFINITY);
        assert!(step.done);
        assert!(!s.progress.is_nan());
    }
}
