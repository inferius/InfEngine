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
use super::attributes::mob_attributes;
use super::spawn::{SpawnRng, initial_health_for};
use super::state::{
    MOB_ATTACK_REACH, MOB_CAP_DEFAULT, MOB_DEATH_FADE, MOB_DESPAWN_DIST_SQ,
    MOB_HIT_DAMAGE, MOB_LUNGE_TIME, MobState, ZOMBIE_ATTACK_COOLDOWN,
    ZOMBIE_ATTACK_RANGE, ZOMBIE_SIGHT_RANGE, contact_damage_of,
    kind_panic_mult, kind_walk_speed,
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
    let knobs: [f32; 8] = [
        MOB_DEATH_FADE,
        MOB_HIT_DAMAGE,
        MOB_DESPAWN_DIST_SQ,
        MOB_LUNGE_TIME,
        MOB_ATTACK_REACH,
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
    // than a cow, otherwise the player has nothing to fear.
    // W474: health + damage read the attribute table now.
    let zs = kind_walk_speed(KIND_ZOMBIE);
    let ms = kind_walk_speed(KIND_COW);
    let zh = initial_health_for(KIND_ZOMBIE);
    let mh = initial_health_for(KIND_SHEEP);
    let zombie = MobState::new_natural(1, KIND_ZOMBIE, [0.0; 3], [0.0; 3], 0.0, 0.0, zh);
    let zd = contact_damage_of(&zombie);
    let zsr = ZOMBIE_SIGHT_RANGE;
    let zar = ZOMBIE_ATTACK_RANGE;
    assert!(zs > ms);
    assert!(zh > mh);
    assert!(zd > 0.0);
    assert!(zsr > zar);
}

/// W474: pin the generated attribute rows against the decompiled
/// `createAttributes()` values named in the wave spec. A regenerate
/// that drifts (or a hand edit) fails loudly here.
#[test]
fn w474_attribute_table_pins_match_source() {
    use super::kinds::*;
    // Zombie.createAttributes: 20 HP / 0.23 speed / 3 attack / 2 armor.
    let z = mob_attributes(KIND_ZOMBIE);
    assert_eq!(
        (z.max_health, z.movement_speed, z.attack_damage, z.armor),
        (20.0, 0.23, 3.0, 2.0)
    );
    // Spider.createAttributes: 16 HP / 0.3 speed.
    let s = mob_attributes(KIND_SPIDER);
    assert_eq!((s.max_health, s.movement_speed), (16.0, 0.3));
    // AbstractCow.createAttributes: 10 HP / 0.2 speed.
    let c = mob_attributes(KIND_COW);
    assert_eq!((c.max_health, c.movement_speed), (10.0, 0.2));
    // EnderMan.createAttributes: 40 HP / 7 attack.
    let e = mob_attributes(KIND_ENDERMAN);
    assert_eq!((e.max_health, e.attack_damage), (40.0, 7.0));
    // Wolf.createAttributes: 8 HP / 4 attack.
    let w = mob_attributes(KIND_WOLF);
    assert_eq!((w.max_health, w.attack_damage), (8.0, 4.0));
    // PolarBear.createAttributes: 30 HP / 6 attack.
    let p = mob_attributes(KIND_POLAR_BEAR);
    assert_eq!((p.max_health, p.attack_damage), (30.0, 6.0));
    // AbstractSkeleton.createAttributes: 20 HP / 0.25 speed.
    let sk = mob_attributes(KIND_SKELETON);
    assert_eq!((sk.max_health, sk.movement_speed), (20.0, 0.25));
    // The spawn HP wiring: the W473-flagged wrong arms are fixed.
    assert_eq!(initial_health_for(KIND_ZOMBIE), 20.0);
    assert_eq!(initial_health_for(KIND_SKELETON), 20.0);
    assert_eq!(initial_health_for(KIND_SPIDER), 16.0);
    assert_eq!(initial_health_for(KIND_COW), 10.0);
    // Source-faithful special cases survive the table wiring.
    assert_eq!(initial_health_for(KIND_HORSE), 22.0, "generateMaxHealth midpoint");
    assert_eq!(initial_health_for(KIND_MAGMA_CUBE), 4.0, "size-2 identity");
    // Nonzero armor is exactly the zombie family.
    for k in 0..=MOB_KIND_MAX {
        let a = mob_attributes(k).armor;
        let zombie_family = matches!(
            k,
            x if x == KIND_ZOMBIE
                || x == KIND_ZOMBIE_VILLAGER
                || x == KIND_HUSK
                || x == KIND_DROWNED
                || x == KIND_ZOMBIFIED_PIGLIN
        );
        if zombie_family {
            assert_eq!(a, 2.0, "kind {k} armor");
        } else {
            assert_eq!(a, 0.0, "kind {k} armor");
        }
    }
}

/// W474: per-kind contact damage reads the table; the slime family
/// stays size-based per `Slime.setSize` / `MagmaCube.getAttackDamage`.
#[test]
fn w474_contact_damage_reads_attribute_table() {
    use super::kinds::*;
    let mk = |kind: u8, size: u8| {
        let mut m = MobState::new_natural(1, kind, [0.0; 3], [0.0; 3], 0.0, 0.0, 8.0);
        m.size = size;
        m
    };
    assert_eq!(contact_damage_of(&mk(KIND_ZOMBIE, 1)), 3.0);
    assert_eq!(contact_damage_of(&mk(KIND_ZOMBIFIED_PIGLIN, 1)), 5.0);
    assert_eq!(contact_damage_of(&mk(KIND_ENDERMAN, 1)), 7.0);
    assert_eq!(contact_damage_of(&mk(KIND_BLAZE, 1)), 6.0);
    assert_eq!(contact_damage_of(&mk(KIND_WOLF, 1)), 4.0);
    assert_eq!(contact_damage_of(&mk(KIND_POLAR_BEAR, 1)), 6.0);
    // Slime: attack = size, tiny deals nothing (isDealsDamage).
    assert_eq!(contact_damage_of(&mk(KIND_SLIME, 4)), 4.0);
    assert_eq!(contact_damage_of(&mk(KIND_SLIME, 1)), 0.0);
    // Magma cube: super.getAttackDamage() + 2, bites even tiny.
    assert_eq!(contact_damage_of(&mk(KIND_MAGMA_CUBE, 2)), 4.0);
    assert_eq!(contact_damage_of(&mk(KIND_MAGMA_CUBE, 1)), 3.0);
}

/// W474: mob ARMOR mitigates damage dealt TO mobs through the same
/// CombatRules curve the player path uses - a 4.0 swing on armor-2
/// lands 3.936; armor-0 kinds take it unmitigated.
#[test]
fn w474_mob_armor_absorbs_damage() {
    use crate::player::damage_after_armor_absorb;
    let armored = mob_attributes(KIND_ZOMBIE).armor;
    let bare = mob_attributes(KIND_SPIDER).armor;
    let hit = 4.0;
    let dealt = damage_after_armor_absorb(hit, armored, 0.0);
    assert!((dealt - 3.936).abs() < 1e-4, "zombie armor 2: {dealt}");
    assert_eq!(damage_after_armor_absorb(hit, bare, 0.0), hit);
}

/// W474: `kind_walk_speed` rows reconcile against the generated
/// attribute table x10 for every WALKING kind. Exemptions (each
/// commented in state.rs): goal-speedModifier amblers (villager,
/// wandering trader), slime-family size speeds, the horse-family
/// spawn averages that already equal their base rows, and the
/// non-walkers whose MOVEMENT_SPEED attribute is an unused registry
/// default (ghast, bee, phantom, bat, squid, glow squid).
#[test]
fn walk_speed_matches_attribute_table() {
    use super::kinds::*;
    let exempt = [
        KIND_VILLAGER,
        KIND_WANDERING_TRADER,
        KIND_SLIME,
        KIND_MAGMA_CUBE,
        KIND_GHAST,
        KIND_BEE,
        KIND_PHANTOM,
        KIND_BAT,
        KIND_SQUID,
        KIND_GLOW_SQUID,
    ];
    for k in 0..=MOB_KIND_MAX {
        if exempt.contains(&k) {
            continue;
        }
        let table = mob_attributes(k).movement_speed * 10.0;
        let row = kind_walk_speed(k);
        assert!(
            (table - row).abs() < 1e-3,
            "kind {k} ({}): table x10 {table} vs walk row {row}",
            MobKind::from_u8(k).name()
        );
    }
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

/// W289: pin the MC per-kind movement tables (attribute x10 walk
/// speeds + PanicGoal speed modifiers) so a stray edit can't silently
/// deviate from the vanilla values.
#[test]
fn mc_speed_tables_pin() {
    assert_eq!(kind_walk_speed(KIND_COW), 2.0);
    assert_eq!(kind_walk_speed(KIND_SHEEP), 2.3);
    assert_eq!(kind_walk_speed(KIND_PIG), 2.5);
    assert_eq!(kind_walk_speed(KIND_CHICKEN), 2.5);
    // W300: zombified piglin walks at the zombie attribute (0.23).
    assert_eq!(
        kind_walk_speed(crate::mob::kinds::KIND_ZOMBIFIED_PIGLIN),
        2.3
    );
    // W302: villager ambles at attr 0.5 x stroll modifier 0.4.
    assert_eq!(kind_walk_speed(crate::mob::kinds::KIND_VILLAGER), 2.0);
    // W315: cat attr 0.3.
    assert_eq!(kind_walk_speed(crate::mob::kinds::KIND_CAT), 3.0);
    // W322: golem attr 0.25; horse averaged attr ~0.225.
    assert_eq!(kind_walk_speed(crate::mob::kinds::KIND_IRON_GOLEM), 2.5);
    assert_eq!(kind_walk_speed(crate::mob::kinds::KIND_HORSE), 2.25);
    // W327: ghast drifts; magma cube slime attr 0.3.
    assert_eq!(kind_walk_speed(crate::mob::kinds::KIND_GHAST), 1.2);
    assert_eq!(kind_walk_speed(crate::mob::kinds::KIND_MAGMA_CUBE), 3.0);
    // W330: zombie family attr 0.23.
    assert_eq!(
        kind_walk_speed(crate::mob::kinds::KIND_ZOMBIE_VILLAGER),
        2.3
    );
    assert_eq!(kind_panic_mult(KIND_COW), 2.0);
    assert_eq!(kind_panic_mult(KIND_CHICKEN), 1.4);
    assert_eq!(kind_panic_mult(KIND_SHEEP), 1.25);
}
