//! W348 - horse/donkey riding + taming (`AbstractHorse` port).
//!
//! Attributes follow the vanilla generators, derived DETERMINISTICALLY
//! from the mob id so they survive save/reload without persisting:
//! speed = (0.45 + 3 rolls * 0.3) * 0.25 (donkey fixed 0.175), jump
//! strength = 0.4 + 3 rolls * 0.2 (donkey fixed 0.5). The engine
//! ground factor ~43 maps the MC movement-speed attribute to blocks/s
//! (a 0.1-attr player walks ~4.3 b/s), jump strength maps b/t -> b/s
//! at x20.
//!
//! Taming is `RunAroundLikeCrazyGoal` verbatim: an untamed mount
//! sprints to random nearby points at 1.2x; each tick has a 1/50
//! chance to roll `rand(100) < temper` -> tamed, else temper += 5 and
//! the rider is thrown (angry sound).

use super::kinds::{KIND_DONKEY, KIND_HORSE};
use crate::worldgen::mcgen::noise::SplitMix64;

/// MC movement-speed attribute -> engine blocks/s.
const SPEED_TO_BPS: f32 = 43.0;

pub(crate) fn is_rideable(kind: u8) -> bool {
    matches!(kind, KIND_HORSE | KIND_DONKEY)
}

/// Per-horse gallop speed in blocks/s (MC `generateSpeed`, donkey
/// fixed 0.175 per `createBaseChestedHorseAttributes`).
pub(crate) fn riding_speed(kind: u8, id: u32) -> f32 {
    if kind == KIND_DONKEY {
        return 0.175 * SPEED_TO_BPS;
    }
    let mut r = SplitMix64::new(id as u64 ^ 0x4852_5350_4544);
    let mut roll = || r.next_f64() as f32;
    (0.45 + roll() * 0.3 + roll() * 0.3 + roll() * 0.3) * 0.25 * SPEED_TO_BPS
}

/// Per-horse jump launch in blocks/s (MC `generateJumpStrength`
/// b/t x20; donkey fixed 0.5).
pub(crate) fn jump_power(kind: u8, id: u32) -> f32 {
    if kind == KIND_DONKEY {
        return 0.5 * 20.0;
    }
    let mut r = SplitMix64::new(id as u64 ^ 0x4852_4A50);
    let mut roll = || r.next_f64() as f32;
    (0.4 + roll() * 0.2 + roll() * 0.2 + roll() * 0.2) * 20.0
}

/// Rider EYE height above the mount's feet (horse stands 1.6, the
/// saddle sits ~1.4, a seated player's eye adds ~1.2).
pub(crate) fn rider_eye(kind: u8) -> f32 {
    let _ = kind;
    2.6
}

/// `RunAroundLikeCrazyGoal`: `random.nextInt(maxTemper=100) < temper`.
pub(crate) fn tame_roll(temper: u8, roll: u8) -> bool {
    roll < temper
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W348: attribute ranges pin the vanilla generators - horse
    /// speed 4.84..14.51 b/s, jump 8..20 b/s, donkey fixed.
    #[test]
    fn w348_attribute_ranges_match_mc() {
        for id in 0..200u32 {
            let s = riding_speed(KIND_HORSE, id);
            assert!((4.83..=14.52).contains(&s), "speed {s}");
            let j = jump_power(KIND_HORSE, id);
            assert!((7.99..=20.01).contains(&j), "jump {j}");
        }
        assert!((riding_speed(KIND_DONKEY, 7) - 7.525).abs() < 1e-3);
        assert!((jump_power(KIND_DONKEY, 7) - 10.0).abs() < 1e-4);
        // Deterministic per id (survives reload).
        assert_eq!(
            riding_speed(KIND_HORSE, 42).to_bits(),
            riding_speed(KIND_HORSE, 42).to_bits(),
        );
    }

    /// W348: the taming ladder - temper 0 never tames, each buck
    /// (+5) widens the roll window.
    #[test]
    fn w348_tame_roll_ladder() {
        assert!(!tame_roll(0, 0));
        assert!(!tame_roll(0, 99));
        assert!(tame_roll(5, 4));
        assert!(!tame_roll(5, 5));
        assert!(tame_roll(100, 99));
    }
}
