//! Per-biome spawn pools: weighted (MobKind, weight) tables sorted
//! by time-of-day.
//!
//! Adapts MC's `MobSpawnSettings::SpawnerData` pattern - each biome
//! carries a per-`MobCategory` list of weighted spawn candidates,
//! and the spawner picks one via a weighted draw over the full
//! list. We collapse `MobCategory` into a day / night split (the
//! light gate already discriminates between hostile + passive in
//! `light.rs`) and ship one static table per biome.
//!
//! Cited MC source files:
//! - `net/minecraft/world/level/biome/MobSpawnSettings.java` -
//!   per-biome spawner data: spawners + spawn-cost map.
//! - `net/minecraft/world/level/biome/MobSpawnSettings$SpawnerData`
//!   - the `(EntityType, weight, min, max)` 4-tuple - we ship only
//!     `(MobKind, weight)`.
//! - `net/minecraft/world/level/biome/Biomes.java` - per-biome
//!   spawn pool literals (Plains: 8x chicken / 4x cow / etc.).

use crate::worldgen::Biome;

use super::kinds::{
    KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_POLAR_BEAR, KIND_RABBIT, KIND_SHEEP,
    KIND_CREEPER, KIND_DONKEY, KIND_SKELETON, KIND_SPIDER, KIND_WOLF,
    KIND_ZOMBIE, KIND_SLIME, KIND_DROWNED, KIND_HUSK, KIND_STRAY, KIND_ENDERMAN, KIND_MOOSHROOM, KIND_PHANTOM,
};

/// Per-biome day / night spawn lists. Each entry is `(kind, weight)`
/// where higher weight = more likely. An empty list (e.g. `Beach`'s
/// pools) means no mobs spawn for that time-of-day in that biome.
///
/// Weights follow MC's `Biomes.java` shape: passive mobs land in
/// the 1..=8 range, hostile in 30..=80. Skeletons get a slightly
/// higher weight in deserts (MC: stray + skeleton variant cluster
/// in cold + dry biomes).
pub(crate) struct BiomeSpawnPool {
    pub(crate) day: &'static [(u8, u32)],
    pub(crate) night: &'static [(u8, u32)],
}

const EMPTY: &[(u8, u32)] = &[];

// W162: rabbits roam plains, deserts and snowy/taiga biomes in MC.
// W354: vanilla common-farm weights (sheep 12 / pig 10 /
// chicken 10 / cow 8); the rabbit stays for the meadow biomes this
// bucket also covers.
const PLAINS_DAY: &[(u8, u32)] = &[
    (KIND_SHEEP, 12),
    (KIND_PIG, 10),
    (KIND_CHICKEN, 10),
    (KIND_COW, 8),
    (KIND_RABBIT, 2),
    (KIND_DONKEY, 1),
];
// W382: a light phantom presence stands in for MC insomnia
// spawning (no sleep-deprivation tracking yet - follow-up).
const PLAINS_NIGHT: &[(u8, u32)] =
    &[(KIND_ZOMBIE, 70), (KIND_SKELETON, 50), (KIND_SPIDER, 30), (KIND_CREEPER, 50), (KIND_ENDERMAN, 10), (KIND_PHANTOM, 5)];

// W162: the desert is a classic rabbit biome (gold/desert variant).
// W354: vanilla deserts spawn NO farm animals - rabbit only (the
// R17 "cows and chickens in the desert" report).
const DESERT_DAY: &[(u8, u32)] = &[(KIND_RABBIT, 4)];
// W377: the desert swaps most zombies for husks, the tundra its
// skeletons for strays, and the swamp nights raise drowned near
// water (MC per-biome monster overrides).
const DESERT_NIGHT: &[(u8, u32)] =
    &[(KIND_ZOMBIE, 10), (KIND_HUSK, 70), (KIND_SKELETON, 80), (KIND_SPIDER, 20), (KIND_CREEPER, 50), (KIND_ENDERMAN, 10)];

// W158: MC forest pools carry wolves (weight 5).
const FOREST_DAY: &[(u8, u32)] = &[
    (KIND_SHEEP, 12),
    (KIND_PIG, 10),
    (KIND_CHICKEN, 10),
    (KIND_COW, 8),
    (KIND_WOLF, 5),
];
const FOREST_NIGHT: &[(u8, u32)] =
    &[(KIND_ZOMBIE, 80), (KIND_SKELETON, 40), (KIND_SPIDER, 30), (KIND_CREEPER, 50), (KIND_ENDERMAN, 10)];

// W158: the taiga is the wolf's home biome in MC (weight 8).
const TAIGA_DAY: &[(u8, u32)] = &[
    (KIND_SHEEP, 12),
    (KIND_PIG, 10),
    (KIND_CHICKEN, 10),
    (KIND_COW, 8),
    (KIND_WOLF, 8),
    (KIND_RABBIT, 4),
];
const TAIGA_NIGHT: &[(u8, u32)] = &[(KIND_ZOMBIE, 60), (KIND_SKELETON, 40), (KIND_CREEPER, 40), (KIND_ENDERMAN, 10)];

// W161: snowy tundra is the polar bear's home (MC: frozen biomes).
// W162: snowy rabbits (white variant) roam the same tundra.
// W354: vanilla snowy plains carry NO sheep - rabbit 10 + polar
// bear 1.
const TUNDRA_DAY: &[(u8, u32)] =
    &[(KIND_RABBIT, 10), (KIND_POLAR_BEAR, 1)];
const TUNDRA_NIGHT: &[(u8, u32)] = &[(KIND_ZOMBIE, 80), (KIND_SKELETON, 8), (KIND_STRAY, 64), (KIND_CREEPER, 40), (KIND_ENDERMAN, 10)];

// W354: vanilla jungles run the common farm list PLUS an extra
// chicken entry (10 + 10).
const JUNGLE_DAY: &[(u8, u32)] = &[
    (KIND_SHEEP, 12),
    (KIND_PIG, 10),
    (KIND_CHICKEN, 20),
    (KIND_COW, 8),
];
const JUNGLE_NIGHT: &[(u8, u32)] =
    &[(KIND_ZOMBIE, 70), (KIND_SKELETON, 40), (KIND_SPIDER, 30), (KIND_CREEPER, 50), (KIND_ENDERMAN, 10)];

// W354: vanilla swamps spawn no farm animals (slime / frog only,
// neither ported yet).
// W382: mushroom fields grow mooshroom herds (the only
// creature in the MC pool) and stay hostile-free at night.
pub(crate) const MUSHROOM_DAY: &[(u8, u32)] = &[(KIND_MOOSHROOM, 8)];
pub(crate) const MUSHROOM_NIGHT: &[(u8, u32)] = &[];

const SWAMP_DAY: &[(u8, u32)] = EMPTY;
// W376: swamp nights grow slimes (surface slime spawning is a
// swamp signature; the witch stays hut-resident, not pooled).
const SWAMP_NIGHT: &[(u8, u32)] = &[(KIND_ZOMBIE, 60), (KIND_DROWNED, 30), (KIND_SKELETON, 20), (KIND_CREEPER, 40), (KIND_SLIME, 30), (KIND_ENDERMAN, 10)];

const SAVANNA_DAY: &[(u8, u32)] = &[
    (KIND_SHEEP, 12),
    (KIND_PIG, 10),
    (KIND_CHICKEN, 10),
    (KIND_COW, 8),
    (KIND_DONKEY, 1),
];
const SAVANNA_NIGHT: &[(u8, u32)] =
    &[(KIND_ZOMBIE, 60), (KIND_SKELETON, 50), (KIND_SPIDER, 30), (KIND_CREEPER, 50), (KIND_ENDERMAN, 10)];

// W354: windswept hills run the common farm list + llama (not
// ported).
const MOUNTAIN_DAY: &[(u8, u32)] = &[
    (KIND_SHEEP, 12),
    (KIND_PIG, 10),
    (KIND_CHICKEN, 10),
    (KIND_COW, 8),
];
const MOUNTAIN_NIGHT: &[(u8, u32)] = &[(KIND_ZOMBIE, 80), (KIND_SKELETON, 40), (KIND_CREEPER, 40), (KIND_ENDERMAN, 10)];

/// Look up the active spawn pool for `biome`. Returns the static
/// pool struct; the spawner pulls the right list (day / night) at
/// the call site based on `compute_day_factor`.
pub(crate) fn pool_for(biome: Biome) -> BiomeSpawnPool {
    match biome {
        Biome::Plains => BiomeSpawnPool { day: PLAINS_DAY, night: PLAINS_NIGHT },
        Biome::Desert => BiomeSpawnPool { day: DESERT_DAY, night: DESERT_NIGHT },
        Biome::Mountain => {
            BiomeSpawnPool { day: MOUNTAIN_DAY, night: MOUNTAIN_NIGHT }
        }
        Biome::Tundra => {
            BiomeSpawnPool { day: TUNDRA_DAY, night: TUNDRA_NIGHT }
        }
        Biome::Taiga => BiomeSpawnPool { day: TAIGA_DAY, night: TAIGA_NIGHT },
        Biome::Forest => {
            BiomeSpawnPool { day: FOREST_DAY, night: FOREST_NIGHT }
        }
        Biome::Jungle => {
            BiomeSpawnPool { day: JUNGLE_DAY, night: JUNGLE_NIGHT }
        }
        Biome::Swamp => BiomeSpawnPool { day: SWAMP_DAY, night: SWAMP_NIGHT },
        Biome::Savanna => {
            BiomeSpawnPool { day: SAVANNA_DAY, night: SAVANNA_NIGHT }
        }
        Biome::SnowyMountain => {
            BiomeSpawnPool { day: TUNDRA_DAY, night: TUNDRA_NIGHT }
        }
        Biome::Beach => BiomeSpawnPool { day: EMPTY, night: EMPTY },
    }
}

/// Pick one entry from `pool` weighted by `weight`. Returns `None`
/// when the pool is empty. `roll01` is a uniform sample in `[0, 1]`.
pub(crate) fn weighted_pick(pool: &[(u8, u32)], roll01: f32) -> Option<u8> {
    if pool.is_empty() {
        return None;
    }
    let total: u32 = pool.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        return None;
    }
    // Compute target as a u32 in `[0, total)`, then walk the
    // cumulative bins until the running sum exceeds it.
    let r = (roll01.clamp(0.0, 0.9999) * total as f32) as u32;
    let mut acc = 0u32;
    for (kind, w) in pool {
        acc += *w;
        if r < acc {
            return Some(*kind);
        }
    }
    // Floating-point edge: clamp to last entry.
    pool.last().map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::Biome;

    #[test]
    fn beach_pools_are_empty_day_and_night() {
        let p = pool_for(Biome::Beach);
        assert!(p.day.is_empty());
        assert!(p.night.is_empty());
    }

    #[test]
    fn plains_pool_has_distinct_day_and_night() {
        let p = pool_for(Biome::Plains);
        assert!(!p.day.is_empty());
        assert!(!p.night.is_empty());
        // Day should be predominantly passive, night predominantly
        // hostile.
        let day_passive_w: u32 = p
            .day
            .iter()
            .filter(|(k, _)| {
                *k == KIND_CHICKEN
                    || *k == KIND_COW
                    || *k == KIND_PIG
                    || *k == KIND_SHEEP
                    // W162: rabbit is a passive day spawner too.
                    || *k == KIND_RABBIT
                    // W344: donkey too.
                    || *k == KIND_DONKEY
            })
            .map(|(_, w)| *w)
            .sum();
        let night_hostile_w: u32 = p
            .night
            .iter()
            .filter(|(k, _)| {
                *k == KIND_ZOMBIE
                    || *k == KIND_SKELETON
                    || *k == KIND_SPIDER
                    || *k == KIND_CREEPER
                    // W381: the enderman haunts every overworld
                    // night pool at MC weight 10.
                    || *k == KIND_ENDERMAN
                    // W382: the phantom insomnia stand-in.
                    || *k == KIND_PHANTOM
            })
            .map(|(_, w)| *w)
            .sum();
        let day_total: u32 = p.day.iter().map(|(_, w)| *w).sum();
        let night_total: u32 = p.night.iter().map(|(_, w)| *w).sum();
        assert_eq!(day_passive_w, day_total);
        assert_eq!(night_hostile_w, night_total);
    }

    #[test]
    fn weighted_pick_on_empty_returns_none() {
        assert_eq!(weighted_pick(&[], 0.5), None);
    }

    #[test]
    fn weighted_pick_with_single_entry_always_returns_it() {
        assert_eq!(weighted_pick(&[(KIND_SHEEP, 1)], 0.0), Some(KIND_SHEEP));
        assert_eq!(weighted_pick(&[(KIND_SHEEP, 1)], 0.5), Some(KIND_SHEEP));
        assert_eq!(weighted_pick(&[(KIND_SHEEP, 1)], 0.99), Some(KIND_SHEEP));
    }

    #[test]
    fn weighted_pick_zero_total_returns_none() {
        // All-zero weights are degenerate; the picker must not
        // divide by zero.
        let degenerate: &[(u8, u32)] = &[(KIND_SHEEP, 0), (KIND_ZOMBIE, 0)];
        assert_eq!(weighted_pick(degenerate, 0.5), None);
    }

    #[test]
    fn weighted_pick_distribution_over_many_samples() {
        // 10_000 deterministic draws over `[(sheep, 1), (zombie, 3)]`
        // should land ~25% sheep + ~75% zombie. Use the same xorshift
        // family the live spawner runs so we're testing the actual
        // picking path.
        use super::super::spawn::SpawnRng;
        let pool: &[(u8, u32)] = &[(KIND_SHEEP, 1), (KIND_ZOMBIE, 3)];
        let mut seed = 0xC0FFEEu32;
        let mut rng = SpawnRng::new(&mut seed);
        let mut sheep = 0u32;
        let mut zombie = 0u32;
        let n = 10_000u32;
        for _ in 0..n {
            match weighted_pick(pool, rng.next_f32()) {
                Some(KIND_SHEEP) => sheep += 1,
                Some(KIND_ZOMBIE) => zombie += 1,
                _ => panic!("unexpected"),
            }
        }
        let ratio = sheep as f32 / n as f32;
        // Expect ~0.25 +/- 0.03 (3% slack over 10k draws is generous).
        assert!(
            (0.22..=0.28).contains(&ratio),
            "sheep ratio out of band: {} ({} sheep, {} zombie)",
            ratio,
            sheep,
            zombie
        );
    }

    #[test]
    fn every_biome_has_a_pool() {
        // Pool lookup must be total over the Biome enum. Adding a
        // new variant requires editing `pool_for`; this test makes
        // the failure loud.
        for b in [
            Biome::Plains,
            Biome::Desert,
            Biome::Mountain,
            Biome::Tundra,
            Biome::Taiga,
            Biome::Forest,
            Biome::Jungle,
            Biome::Swamp,
            Biome::Savanna,
            Biome::SnowyMountain,
            Biome::Beach,
        ] {
            let _ = pool_for(b);
        }
    }

    #[test]
    fn desert_skeletons_outweigh_other_hostiles() {
        // Desert at night biases toward skeletons (MC convention).
        // Specific check: skeleton weight > zombie weight in the
        // desert night pool.
        let p = pool_for(Biome::Desert);
        let zombie_w: u32 = p
            .night
            .iter()
            .filter(|(k, _)| *k == KIND_ZOMBIE)
            .map(|(_, w)| *w)
            .sum();
        let skeleton_w: u32 = p
            .night
            .iter()
            .filter(|(k, _)| *k == KIND_SKELETON)
            .map(|(_, w)| *w)
            .sum();
        assert!(skeleton_w > zombie_w, "{} > {}", skeleton_w, zombie_w);
    }

    #[test]
    fn forest_carries_wolves_plains_does_not() {
        // W354: the vanilla common-farm weights are IDENTICAL across
        // forest and plains (sheep 12 / pig 10 / chicken 10 / cow 8),
        // so the old more-sheep-in-forest pin no longer holds. The
        // biome flavour that DOES distinguish them in MC is the wolf.
        let f = pool_for(Biome::Forest);
        let p = pool_for(Biome::Plains);
        assert!(f.day.iter().any(|(k, _)| *k == KIND_WOLF));
        assert!(!p.day.iter().any(|(k, _)| *k == KIND_WOLF));
    }

    #[test]
    fn snowy_mountain_inherits_tundra_pool() {
        // Both cold biomes share a sparse passive list + a
        // hostile-only night list (matches MC: snowy taiga + snowy
        // plains share most spawners).
        let sm = pool_for(Biome::SnowyMountain);
        let t = pool_for(Biome::Tundra);
        assert_eq!(sm.day.len(), t.day.len());
        assert_eq!(sm.night.len(), t.night.len());
    }
}
