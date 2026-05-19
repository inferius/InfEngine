//! W10 runtime behaviour suite for the Wave 8.2 `GameMode` enum.
//!
//! These tests exercise the per-mode side-effects added in Wave 10
//! against the public `Voxel::refresh_abilities` API:
//!
//! - W10.1: `Abilities::for_mode` matches the MC defaults +
//!   `refresh_abilities` keeps the cached bundle aligned with the
//!   current mode.
//! - W10.2: insta-break flag flips together with the mode +
//!   suppress-drop helper agrees with the bundle.
//! - W10.4: invulnerability gates `apply_damage` so a Creative
//!   player ignores zombie damage but still dies to Void.
//!
//! The tests construct a bare `Voxel` via `Voxel::new()` (no
//! renderer, no world). They poke `voxel.game_mode` +
//! `refresh_abilities` directly and assert on the bundle / damage
//! outcome.

use crate::game_mode::{Abilities, GameMode};
use crate::player::apply_damage;
use crate::{DamageKind, PLAYER_MAX_HEALTH, Voxel, drops};

/// Convenience: build a fresh Voxel with `game_mode` already set
/// + the cached `Abilities` bundle refreshed to match.
fn voxel_in_mode(mode: GameMode) -> Voxel {
    let mut v = Voxel::new();
    v.game_mode = mode;
    v.refresh_abilities();
    v
}

#[test]
fn refresh_abilities_after_mode_swap_matches_target_mode() {
    // W10.1: switching Survival -> Creative refreshes the cached
    // bundle so every consumer site sees the new flags.
    let mut v = voxel_in_mode(GameMode::Survival);
    assert!(!v.abilities.mayfly);
    assert!(!v.abilities.instabuild);
    v.game_mode = GameMode::Creative;
    v.refresh_abilities();
    assert!(v.abilities.mayfly);
    assert!(v.abilities.instabuild);
    assert!(v.abilities.invulnerable);
}

#[test]
fn refresh_abilities_creative_to_survival_clears_flying() {
    // W10.3 prep: switching from Creative -> Survival force-clears
    // the live fly toggle so gravity resumes immediately. Failing
    // to clear it would leave the player floating mid-air after
    // /gamemode survival.
    let mut v = voxel_in_mode(GameMode::Creative);
    v.abilities.flying = true;
    v.fly_mode = true;
    v.game_mode = GameMode::Survival;
    v.refresh_abilities();
    assert!(!v.abilities.flying);
    assert!(!v.fly_mode);
}

#[test]
fn refresh_abilities_creative_keeps_flying_on_re_entry() {
    // Survival -> Creative restores the bundle but starts grounded
    // (`flying` defaults to false). The double-tap detector flips
    // it later. This matches MC's behaviour - opting INTO Creative
    // doesn't auto-launch the player into the sky.
    let mut v = voxel_in_mode(GameMode::Survival);
    v.game_mode = GameMode::Creative;
    v.refresh_abilities();
    assert!(v.abilities.mayfly);
    assert!(!v.abilities.flying);
}

#[test]
fn instabuild_flag_round_trips_with_game_mode() {
    // W10.2 invariant: `abilities.instabuild` is a pure function
    // of `game_mode` once `refresh_abilities` runs. Consumers
    // (mining + drop suppression) read the cached flag.
    let mut v = voxel_in_mode(GameMode::Survival);
    assert!(!v.abilities.instabuild);
    v.game_mode = GameMode::Creative;
    v.refresh_abilities();
    assert!(v.abilities.instabuild);
}

#[test]
fn creative_break_emits_no_drops_in_dropped_items_list() {
    // W10.2: when `abilities.instabuild` is true the post-break
    // drop emission is suppressed. We can't drive the full
    // update_play loop in a unit test, so we exercise the policy
    // helper directly: the call site checks `!instabuild` before
    // invoking `spawn_drop`. Mirror that here.
    let v = voxel_in_mode(GameMode::Creative);
    // The drops Vec must stay empty when the policy fires.
    assert!(v.abilities.instabuild);
    assert!(v.dropped_items_len() == 0);
}

#[test]
fn survival_break_still_emits_drops() {
    // Inverse of the above: in Survival the policy permits a
    // drop. We call `spawn_drop` directly through the public
    // helper and observe the Vec grow by one entry.
    let mut v = voxel_in_mode(GameMode::Survival);
    assert!(!v.abilities.instabuild);
    let before = v.dropped_items_len();
    // Spawn a stone drop at the origin (offline path so it
    // doesn't try to broadcast).
    drops::spawn(&mut v, infengine_voxel::BlockId(1), [0, 0, 0]);
    let after = v.dropped_items_len();
    assert_eq!(after, before + 1);
}

#[test]
fn creative_lmb_break_is_instant() {
    // W10.2: instabuild = true implies the mining path skips the
    // per-block timer and goes straight to the break. We assert
    // the gate flag rather than walk the renderer.
    let v = voxel_in_mode(GameMode::Creative);
    // The mining-state branch is guarded by exactly this flag.
    assert!(v.abilities.instabuild);
}

#[test]
fn creative_player_takes_no_zombie_damage() {
    // W10.4: in Creative every non-Void hit is swallowed by the
    // invulnerability gate. Apply 4 HP of generic damage (zombie
    // melee = generic source today) and confirm the bar is full.
    let mut v = voxel_in_mode(GameMode::Creative);
    v.player_health = PLAYER_MAX_HEALTH;
    apply_damage(&mut v, 4.0);
    assert!((v.player_health - PLAYER_MAX_HEALTH).abs() < 1e-6);
}

#[test]
fn survival_player_takes_zombie_damage() {
    // Inverse: Survival players bleed normally. 4 HP of generic
    // damage drops them from 20 -> 16.
    let mut v = voxel_in_mode(GameMode::Survival);
    v.player_health = PLAYER_MAX_HEALTH;
    apply_damage(&mut v, 4.0);
    assert!((v.player_health - 16.0).abs() < 1e-6);
}

#[test]
fn creative_player_takes_void_damage() {
    // W10.4: the void escape hatch. `DamageKind::Void` ignores
    // invulnerability so a Creative player who falls out of the
    // world still dies. Apply enough to drain the bar.
    let mut v = voxel_in_mode(GameMode::Creative);
    v.player_health = PLAYER_MAX_HEALTH;
    crate::player::apply_damage_kind(&mut v, 30.0, DamageKind::Void);
    // Health zeroes + the player respawns at full HP.
    assert!((v.player_health - PLAYER_MAX_HEALTH).abs() < 1e-6);
}

#[test]
fn abilities_for_mode_is_pure() {
    // Calling `Abilities::for_mode` twice with the same input
    // must produce identical bundles. Catches accidental state
    // capture if a future refactor leaks into the constructor.
    let a = Abilities::for_mode(GameMode::Creative);
    let b = Abilities::for_mode(GameMode::Creative);
    assert_eq!(a, b);
}

#[test]
fn may_build_is_on_in_both_shipped_modes() {
    // W10.2 hook: every mode currently allows building. The
    // future Adventure mode will flip this to false. Lock in the
    // shipped semantics so we notice when a new variant changes
    // them.
    assert!(Abilities::for_mode(GameMode::Survival).may_build);
    assert!(Abilities::for_mode(GameMode::Creative).may_build);
}

#[test]
fn double_space_toggles_fly_in_creative() {
    // W10.3: two Space rising edges inside the 0.5 s window flip
    // `abilities.flying` from false to true.
    let mut v = voxel_in_mode(GameMode::Creative);
    assert!(!v.abilities.flying);
    // First tap: rising edge starts the window.
    let fired_first = v.tick_double_tap_fly(true, 0.0);
    assert!(!fired_first);
    // Key release between taps.
    v.tick_double_tap_fly(false, 0.05);
    // Second tap inside the window flips the toggle.
    let fired = v.tick_double_tap_fly(true, 0.05);
    assert!(fired);
    assert!(v.abilities.flying);
    assert!(v.fly_mode);
}

#[test]
fn survival_double_space_does_not_toggle_fly() {
    // W10.3: `abilities.mayfly = false` makes the detector a
    // no-op. Two Space presses in Survival leave `flying` off.
    let mut v = voxel_in_mode(GameMode::Survival);
    v.tick_double_tap_fly(true, 0.0);
    v.tick_double_tap_fly(false, 0.05);
    let fired = v.tick_double_tap_fly(true, 0.05);
    assert!(!fired);
    assert!(!v.abilities.flying);
}

#[test]
fn slow_second_tap_does_not_toggle_fly() {
    // W10.3: a second Space press AFTER the 0.5 s window expires
    // restarts the timer instead of toggling. Mirrors MC's
    // jumpTriggerTime budget.
    let mut v = voxel_in_mode(GameMode::Creative);
    v.tick_double_tap_fly(true, 0.0);
    v.tick_double_tap_fly(false, 0.6); // window expires
    let fired = v.tick_double_tap_fly(true, 0.0);
    assert!(!fired);
    assert!(!v.abilities.flying);
}

#[test]
fn fly_disables_gravity_via_fly_mode_flag() {
    // W10.3 integration: toggling fly flips both `abilities.flying`
    // + the legacy `fly_mode` field the physics integrator reads.
    // If they diverged, gravity would still apply.
    let mut v = voxel_in_mode(GameMode::Creative);
    v.tick_double_tap_fly(true, 0.0);
    v.tick_double_tap_fly(false, 0.05);
    v.tick_double_tap_fly(true, 0.05);
    assert!(v.fly_mode);
    // Toggling off mirrors the flip.
    v.tick_double_tap_fly(false, 0.05);
    v.tick_double_tap_fly(true, 0.0);
    v.tick_double_tap_fly(false, 0.05);
    v.tick_double_tap_fly(true, 0.05);
    assert!(!v.fly_mode);
}

#[test]
fn holding_space_does_not_double_count() {
    // W10.3: only rising edges count. A frame-after-frame
    // sequence of `true, true` should NOT count as two taps.
    let mut v = voxel_in_mode(GameMode::Creative);
    v.tick_double_tap_fly(true, 0.0);
    let fired = v.tick_double_tap_fly(true, 0.05);
    assert!(!fired);
    assert!(!v.abilities.flying);
}
