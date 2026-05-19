//! Pure-data tests for the mob subsystem.
//!
//! Behaviour tests that drive a live `Voxel` live in the various
//! integration test modules (`dim_switch_tests.rs` etc.). This file
//! hosts the small data-shape regressions that don't need a full
//! voxel runtime: balance-constant invariants, the `MobKind`
//! round-trip and the spawn-rng determinism.

use super::kinds::{
    KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_SHEEP, KIND_SKELETON, KIND_SPIDER, KIND_ZOMBIE,
    MobKind,
};
use super::spawn::SpawnRng;
use super::state::{
    MOB_ATTACK_REACH, MOB_CAP_DEFAULT, MOB_DEATH_FADE, MOB_DESPAWN_DIST_SQ,
    MOB_HIT_DAMAGE, MOB_LUNGE_TIME, MOB_MAX_HEALTH, MOB_SPEED,
    ZOMBIE_ATTACK_COOLDOWN, ZOMBIE_ATTACK_RANGE, ZOMBIE_MAX_HEALTH,
    ZOMBIE_SIGHT_RANGE, ZOMBIE_SPEED, ZOMBIE_TOUCH_DAMAGE,
};

// Re-export check: the same `MobKind` discriminants must come out
// of both the `kinds` module and the top-level `mob` re-export. We
// don't want a parallel-naming regression where chat commands
// import a different `KIND_*` than the spawn loop.

#[test]
fn balance_constants_are_positive_finite() {
    // Catch a stray sign-flip or `f32::NAN` regression at compile-
    // independent runtime - cheap insurance. Read each constant
    // through a `black_box`-style binding so clippy treats the
    // check as a runtime assert, not a compile-time tautology.
    let knobs: [f32; 13] = [
        MOB_DEATH_FADE,
        MOB_SPEED,
        MOB_MAX_HEALTH,
        MOB_HIT_DAMAGE,
        MOB_DESPAWN_DIST_SQ,
        MOB_LUNGE_TIME,
        MOB_ATTACK_REACH,
        ZOMBIE_SPEED,
        ZOMBIE_MAX_HEALTH,
        ZOMBIE_TOUCH_DAMAGE,
        ZOMBIE_ATTACK_COOLDOWN,
        ZOMBIE_ATTACK_RANGE,
        ZOMBIE_SIGHT_RANGE,
    ];
    for v in knobs {
        assert!(v.is_finite() && v > 0.0, "{}", v);
    }
    let cap = MOB_CAP_DEFAULT;
    assert!(cap > 0);
}

#[test]
fn zombie_outclasses_baseline_passive() {
    // Sanity guard: a zombie has to walk faster + bite harder
    // than a sheep, otherwise the player has nothing to fear.
    // Bind through locals so clippy doesn't see the comparison
    // as a const tautology.
    let zs = ZOMBIE_SPEED;
    let ms = MOB_SPEED;
    let zh = ZOMBIE_MAX_HEALTH;
    let mh = MOB_MAX_HEALTH;
    let zd = ZOMBIE_TOUCH_DAMAGE;
    let zsr = ZOMBIE_SIGHT_RANGE;
    let zar = ZOMBIE_ATTACK_RANGE;
    assert!(zs > ms);
    assert!(zh > mh);
    assert!(zd > 0.0);
    assert!(zsr > zar);
}

#[test]
fn legacy_kind_constants_match_enum_discriminants() {
    // The crate root + chat commands rely on the legacy `KIND_*`
    // names. Renumbering would silently break wire + save compat;
    // pin every one to the enum form.
    assert_eq!(KIND_SHEEP, MobKind::Sheep as u8);
    assert_eq!(KIND_ZOMBIE, MobKind::Zombie as u8);
    assert_eq!(KIND_CHICKEN, MobKind::Chicken as u8);
    assert_eq!(KIND_PIG, MobKind::Pig as u8);
    assert_eq!(KIND_COW, MobKind::Cow as u8);
    assert_eq!(KIND_SKELETON, MobKind::Skeleton as u8);
    assert_eq!(KIND_SPIDER, MobKind::Spider as u8);
}

#[test]
fn spawn_rng_is_deterministic_for_seeded_state() {
    // The wander + spawn paths share the same counter-based PRNG.
    // Same seed -> identical stream. Used by replay + by the
    // weighted-pick distribution tests in H.3.
    let mut s1 = 0xC0FFEEu32;
    let mut s2 = 0xC0FFEEu32;
    let mut r1 = SpawnRng::new(&mut s1);
    let mut r2 = SpawnRng::new(&mut s2);
    for _ in 0..32 {
        assert_eq!(r1.next_u32(), r2.next_u32());
    }
}

#[test]
fn spawn_rng_next_f32_in_unit_interval() {
    // The wander picks `next_f32() * TAU` to choose an angle.
    // Anything outside [0, 1] would skew the ring distribution.
    let mut s = 1u32;
    let mut r = SpawnRng::new(&mut s);
    for _ in 0..512 {
        let v = r.next_f32();
        assert!((0.0..=1.0).contains(&v), "{}", v);
    }
}

#[test]
fn spawn_rng_diverges_from_distinct_seeds() {
    // Same RNG family but different starting state must produce
    // a different short prefix - otherwise the determinism test
    // is masking a stuck PRNG.
    let mut s1 = 1u32;
    let mut s2 = 2u32;
    let mut r1 = SpawnRng::new(&mut s1);
    let mut r2 = SpawnRng::new(&mut s2);
    let a: Vec<u32> = (0..8).map(|_| r1.next_u32()).collect();
    let b: Vec<u32> = (0..8).map(|_| r2.next_u32()).collect();
    assert_ne!(a, b);
}
