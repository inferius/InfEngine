//! Per-biome spawn pools: MC `MobCategory` lists with weighted
//! `(kind, weight, minCount, maxCount)` rows, generated from the
//! 26.1.2 biome jsons into `pool_table.rs` (W475).
//!
//! Pre-W475 the pools were hand-written day / night `(kind, weight)`
//! pairs; the categories + pack counts now come straight from the
//! data files. W479: the day / night MONSTER split moved from the
//! category clock gate onto the MC light rule (`light::
//! is_dark_enough_for_hostile` sampled at the anchor + per member),
//! so dark caves spawn monsters at noon exactly like MC.
//!
//! Cited MC source files:
//! - `net/minecraft/world/level/biome/MobSpawnSettings.java` -
//!   per-biome spawner data: spawners + spawn-cost map.
//! - `net/minecraft/world/entity/MobCategory.java` - the per-category
//!   caps (monster 70 / creature 10 / ambient 15 / water_creature 5 /
//!   water_ambient 20 / underground_water_creature 5, each per 17x17
//!   spawnable chunks).
//! - `net/minecraft/world/entity/EntityType.java` - the category each
//!   entity type COUNTS against (`type.getCategory()` in
//!   `NaturalSpawner.isValidSpawnPostitionForType`).

use crate::worldgen::Biome;

pub(crate) use super::pool_table::{CategoryPools, SpawnRow};
use super::pool_table;

/// MC `MobCategory` port - the spawn cycles + caps discriminator.
/// `MISC` is intentionally absent: MISC kinds (villager, iron golem)
/// never naturally spawn and never count toward a cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpawnCategory {
    Monster,
    Creature,
    Ambient,
    WaterCreature,
    WaterAmbient,
    UndergroundWaterCreature,
}

/// Iteration order mirrors `MobCategory.values()` (minus the
/// unported `axolotls` + the never-spawning `misc`).
pub(crate) const ALL_CATEGORIES: [SpawnCategory; 6] = [
    SpawnCategory::Monster,
    SpawnCategory::Creature,
    SpawnCategory::Ambient,
    SpawnCategory::WaterCreature,
    SpawnCategory::WaterAmbient,
    SpawnCategory::UndergroundWaterCreature,
];

impl SpawnCategory {
    /// W475: per-category live-mob caps, replacing the old single
    /// all-kinds cap of 25.
    ///
    /// Scaling choice: MC's `MobCategory` caps (monster 70 /
    /// creature 10 / ambient 15 / water_creature 5 / water_ambient
    /// 20 / underground_water_creature 5) are normalised per 17x17
    /// spawnable chunks - exactly one player's spawn region, so for
    /// the engine's single-host world the plain values apply. The
    /// engine however despawns mobs beyond an 80-block radius vs
    /// MC's 128-block despawn sphere, so the population the caps
    /// govern lives in a (80/128)^2 ~= 0.39 smaller area. We scale
    /// every cap by 2/5 (all six land on integers): monster 28 /
    /// creature 4 / ambient 6 / water_creature 2 / water_ambient 8 /
    /// underground_water_creature 2. The MC RATIOS are exact; the
    /// sum (50) is about the old day+night average around the single
    /// cap 25 - monsters only fill at night, creatures by day, so
    /// the typical live count stays in the former band.
    pub(crate) fn cap(self) -> usize {
        match self {
            SpawnCategory::Monster => 28,
            SpawnCategory::Creature => 4,
            SpawnCategory::Ambient => 6,
            SpawnCategory::WaterCreature => 2,
            SpawnCategory::WaterAmbient => 8,
            SpawnCategory::UndergroundWaterCreature => 2,
        }
    }
}

/// The spawner rows of `pools` for one category.
pub(crate) fn rows_of(
    pools: &'static CategoryPools,
    cat: SpawnCategory,
) -> &'static [SpawnRow] {
    match cat {
        SpawnCategory::Monster => pools.monster,
        SpawnCategory::Creature => pools.creature,
        SpawnCategory::Ambient => pools.ambient,
        SpawnCategory::WaterCreature => pools.water_creature,
        SpawnCategory::WaterAmbient => pools.water_ambient,
        SpawnCategory::UndergroundWaterCreature => {
            pools.underground_water_creature
        }
    }
}

/// The MC `EntityType` category a kind COUNTS against (the cap
/// bookkeeping side; which pool LIST a kind is drawn from is the
/// biome json's business - e.g. the jungle ocelot sits in the
/// `monster` spawner list yet counts as a CREATURE, exactly like
/// MC). `None` = `MobCategory.MISC` (villager, iron golem):
/// never counted, never drawn.
pub(crate) fn category_of_kind(kind: u8) -> Option<SpawnCategory> {
    use super::kinds::MobKind;
    Some(match MobKind::from_u8(kind) {
        MobKind::Zombie
        | MobKind::Skeleton
        | MobKind::Spider
        | MobKind::ZombifiedPiglin
        | MobKind::Ghast
        | MobKind::MagmaCube
        | MobKind::ZombieVillager
        | MobKind::Creeper
        | MobKind::Witch
        | MobKind::Slime
        | MobKind::Drowned
        | MobKind::Husk
        | MobKind::Stray
        | MobKind::Pillager
        | MobKind::Vindicator
        | MobKind::Evoker
        | MobKind::Blaze
        | MobKind::WitherSkeleton
        | MobKind::Guardian
        | MobKind::ElderGuardian
        | MobKind::Enderman
        | MobKind::CaveSpider
        | MobKind::Silverfish
        | MobKind::Phantom => SpawnCategory::Monster,
        MobKind::Bat => SpawnCategory::Ambient,
        MobKind::Squid => SpawnCategory::WaterCreature,
        MobKind::GlowSquid => SpawnCategory::UndergroundWaterCreature,
        MobKind::Villager | MobKind::IronGolem => return None,
        // Every remaining kind is EntityType category CREATURE
        // (sheep..camel incl. cat, wandering trader, horse family).
        _ => SpawnCategory::Creature,
    })
}

/// Look up the generated category pools for `biome`. The
/// off-`Biome`-enum buckets (`MUSHROOM`, `OCEAN`, `NETHER`) are
/// selected directly by the spawner before this mapper runs.
pub(crate) fn pool_for(biome: Biome) -> &'static CategoryPools {
    match biome {
        Biome::Plains => &pool_table::PLAINS,
        Biome::Desert => &pool_table::DESERT,
        Biome::Mountain => &pool_table::MOUNTAIN,
        Biome::Tundra | Biome::SnowyMountain => &pool_table::TUNDRA,
        Biome::Taiga => &pool_table::TAIGA,
        Biome::Forest => &pool_table::FOREST,
        Biome::Jungle => &pool_table::JUNGLE,
        Biome::Swamp => &pool_table::SWAMP,
        Biome::Savanna => &pool_table::SAVANNA,
        Biome::Beach => &pool_table::BEACH,
    }
}

/// Pick one row from `rows` weighted by `weight`. Returns `None`
/// when the list is empty or all-zero. `roll01` is a uniform sample
/// in `[0, 1]`.
pub(crate) fn weighted_pick(
    rows: &'static [SpawnRow],
    roll01: f32,
) -> Option<&'static SpawnRow> {
    if rows.is_empty() {
        return None;
    }
    let total: u32 = rows.iter().map(|r| r.weight).sum();
    if total == 0 {
        return None;
    }
    // Compute target as a u32 in `[0, total)`, then walk the
    // cumulative bins until the running sum exceeds it.
    let r = (roll01.clamp(0.0, 0.9999) * total as f32) as u32;
    let mut acc = 0u32;
    for row in rows {
        acc += row.weight;
        if r < acc {
            return Some(row);
        }
    }
    // Floating-point edge: clamp to last entry.
    rows.last()
}

/// Log the MC species the generated table had to skip (no engine
/// `MobKind` yet) - once per process, one line for all names.
pub(crate) fn log_skipped_species_once() {
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        tracing::info!(
            "W475: spawn pools skip {} MC species (no engine kind \
             yet): {}",
            pool_table::SKIPPED_SPECIES.len(),
            pool_table::SKIPPED_SPECIES.join(", ")
        );
    });
}

#[cfg(test)]
mod tests {
    use super::super::kinds::{
        KIND_BAT, KIND_CAMEL, KIND_CHICKEN, KIND_COW, KIND_DONKEY,
        KIND_DROWNED, KIND_ENDERMAN, KIND_GLOW_SQUID, KIND_HORSE,
        KIND_HUSK, KIND_IRON_GOLEM, KIND_MOOSHROOM, KIND_PHANTOM,
        KIND_PIG, KIND_RABBIT, KIND_SHEEP, KIND_SKELETON, KIND_SLIME,
        KIND_SQUID, KIND_STRAY, KIND_TURTLE, KIND_VILLAGER,
        KIND_WANDERING_TRADER, KIND_WITCH, KIND_WOLF, KIND_ZOMBIE,
        KIND_ZOMBIE_VILLAGER,
    };
    use super::*;
    use crate::worldgen::Biome;

    fn weight_of(rows: &[SpawnRow], kind: u8) -> u32 {
        rows.iter().filter(|r| r.kind == kind).map(|r| r.weight).sum()
    }

    fn row_of(rows: &'static [SpawnRow], kind: u8) -> &'static SpawnRow {
        rows.iter()
            .find(|r| r.kind == kind)
            .unwrap_or_else(|| panic!("kind {kind} missing"))
    }

    #[test]
    fn plains_creature_rows_match_plains_json() {
        // plains.json creature list: sheep 12 / pig 10 / chicken 10 /
        // cow 8 (all 4-4 packs) + horse 5 (2-6) + donkey 1 (1-3).
        // W479: the R24 wandering-trader stand-in row is GONE - the
        // real WanderingTraderSpawner port owns him now.
        let c = pool_for(Biome::Plains).creature;
        for (kind, w, mn, mx) in [
            (KIND_SHEEP, 12, 4, 4),
            (KIND_PIG, 10, 4, 4),
            (KIND_CHICKEN, 10, 4, 4),
            (KIND_COW, 8, 4, 4),
            (KIND_HORSE, 5, 2, 6),
            (KIND_DONKEY, 1, 1, 3),
        ] {
            let r = row_of(c, kind);
            assert_eq!((r.weight, r.min_count, r.max_count), (w, mn, mx));
        }
        assert_eq!(c.len(), 6);
        assert_eq!(weight_of(c, KIND_WANDERING_TRADER), 0, "R24 row gone");
    }

    #[test]
    fn plains_monster_rows_match_plains_json() {
        // The 100-weight quartet + zombie 90 (zombie_horse holds the
        // other 5 in MC - skip-logged) + zombie_villager 5 + witch 5
        // + enderman 10 (1-4). W479: the R24 phantom stand-in row is
        // GONE - the real PhantomSpawner port owns it now.
        let m = pool_for(Biome::Plains).monster;
        for (kind, w) in [
            (super::super::kinds::KIND_SPIDER, 100),
            (KIND_SKELETON, 100),
            (super::super::kinds::KIND_CREEPER, 100),
            (KIND_SLIME, 100),
            (KIND_ZOMBIE, 90),
            (KIND_ZOMBIE_VILLAGER, 5),
            (KIND_WITCH, 5),
            (KIND_ENDERMAN, 10),
        ] {
            assert_eq!(weight_of(m, kind), w, "kind {kind}");
        }
        assert_eq!(weight_of(m, KIND_PHANTOM), 0, "R24 row gone");
        let e = row_of(m, KIND_ENDERMAN);
        assert_eq!((e.min_count, e.max_count), (1, 4));
    }

    #[test]
    fn river_rows_match_river_json() {
        // W479: river.json - the full 8-entry overworld monster set
        // PLUS drowned 100 (1-1, the river night threat), squid 2
        // (1-4) in water_creature, no creatures, salmon skip-logged.
        let p = &pool_table::RIVER;
        assert_eq!(weight_of(p.monster, KIND_DROWNED), 100);
        let d = row_of(p.monster, KIND_DROWNED);
        assert_eq!((d.min_count, d.max_count), (1, 1));
        assert_eq!(p.monster.len(), 9);
        assert!(p.creature.is_empty());
        let s = row_of(p.water_creature, KIND_SQUID);
        assert_eq!((s.weight, s.min_count, s.max_count), (2, 1, 4));
        assert!(p.water_ambient.is_empty(), "salmon has no kind yet");
        let g = row_of(p.underground_water_creature, KIND_GLOW_SQUID);
        assert_eq!((g.weight, g.min_count, g.max_count), (10, 4, 6));
    }

    #[test]
    fn desert_night_biases_husk_over_skeleton_over_zombie() {
        // desert.json: husk 80 > skeleton 50 > zombie 19 (the old
        // hand pool had skeleton 80 > husk 70 - inverted vs MC).
        let m = pool_for(Biome::Desert).monster;
        assert_eq!(weight_of(m, KIND_HUSK), 80);
        assert_eq!(weight_of(m, KIND_SKELETON), 50);
        assert_eq!(weight_of(m, KIND_ZOMBIE), 19);
        assert_eq!(weight_of(m, KIND_ZOMBIE_VILLAGER), 1);
    }

    #[test]
    fn desert_creatures_are_rabbit_and_camel() {
        let c = pool_for(Biome::Desert).creature;
        let r = row_of(c, KIND_RABBIT);
        assert_eq!((r.weight, r.min_count, r.max_count), (12, 2, 3));
        let cm = row_of(c, KIND_CAMEL);
        assert_eq!((cm.weight, cm.min_count, cm.max_count), (1, 1, 1));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn tundra_swaps_skeletons_for_strays() {
        // snowy_plains.json: stray 80 vs skeleton 20.
        let m = pool_for(Biome::Tundra).monster;
        assert_eq!(weight_of(m, KIND_STRAY), 80);
        assert_eq!(weight_of(m, KIND_SKELETON), 20);
    }

    #[test]
    fn beach_turtles_pack_2_to_5_and_nights_are_hostile() {
        // beach.json: turtle 5 (2-5) is the one creature; the
        // monster list is the full 8-entry overworld set (the old
        // hand pool shipped an EMPTY beach night).
        let b = pool_for(Biome::Beach);
        let t = row_of(b.creature, KIND_TURTLE);
        assert_eq!((t.weight, t.min_count, t.max_count), (5, 2, 5));
        assert_eq!(b.creature.len(), 1);
        assert_eq!(b.monster.len(), 8);
    }

    #[test]
    fn nether_monster_rows_include_enderman() {
        // nether_wastes.json: zombified_piglin 100 / ghast 50 /
        // magma_cube 2 / enderman 1 (strider + piglin skip-logged).
        let m = pool_table::NETHER.monster;
        assert_eq!(
            weight_of(m, super::super::kinds::KIND_ZOMBIFIED_PIGLIN),
            100
        );
        assert_eq!(weight_of(m, super::super::kinds::KIND_GHAST), 50);
        assert_eq!(weight_of(m, super::super::kinds::KIND_MAGMA_CUBE), 2);
        assert_eq!(weight_of(m, KIND_ENDERMAN), 1);
        assert_eq!(m.len(), 4);
        // All nether packs are 4-4 per the json.
        assert!(m.iter().all(|r| (r.min_count, r.max_count) == (4, 4)));
    }

    #[test]
    fn mushroom_fields_spawn_mooshrooms_and_no_monsters() {
        let p = &pool_table::MUSHROOM;
        assert!(p.monster.is_empty());
        let m = row_of(p.creature, KIND_MOOSHROOM);
        assert_eq!((m.weight, m.min_count, m.max_count), (8, 4, 8));
    }

    #[test]
    fn ocean_water_rows_match_ocean_json() {
        // ocean.json: squid 1 (1-4) in water_creature; drowned 5 in
        // monster; glow_squid 10 (4-6) underground_water_creature.
        let p = &pool_table::OCEAN;
        let s = row_of(p.water_creature, KIND_SQUID);
        assert_eq!((s.weight, s.min_count, s.max_count), (1, 1, 4));
        assert_eq!(weight_of(p.monster, super::super::kinds::KIND_DROWNED), 5);
        let g = row_of(p.underground_water_creature, KIND_GLOW_SQUID);
        assert_eq!((g.weight, g.min_count, g.max_count), (10, 4, 6));
        // Cod / dolphin / nautilus have no engine kind yet.
        assert!(p.water_ambient.is_empty());
    }

    #[test]
    fn every_overworld_bucket_has_bat_and_glow_squid_rows() {
        // Every mapped overworld biome json carries bat 10 (8-8)
        // ambient + glow_squid 10 (4-6) underground water rows.
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
            Biome::Beach,
        ] {
            let p = pool_for(b);
            let bat = row_of(p.ambient, KIND_BAT);
            assert_eq!((bat.weight, bat.min_count, bat.max_count), (10, 8, 8));
            let g = row_of(p.underground_water_creature, KIND_GLOW_SQUID);
            assert_eq!((g.weight, g.min_count, g.max_count), (10, 4, 6));
        }
    }

    #[test]
    fn forest_carries_wolves_plains_does_not() {
        let f = pool_for(Biome::Forest).creature;
        let p = pool_for(Biome::Plains).creature;
        assert_eq!(weight_of(f, KIND_WOLF), 5);
        assert_eq!(weight_of(p, KIND_WOLF), 0);
    }

    #[test]
    fn snowy_mountain_shares_the_tundra_pool() {
        let sm = pool_for(Biome::SnowyMountain);
        let t = pool_for(Biome::Tundra);
        assert!(std::ptr::eq(sm, t));
    }

    #[test]
    fn category_caps_keep_mc_ratios() {
        // 2/5 of MC's 70/10/15/5/20/5 (see cap() doc).
        assert_eq!(SpawnCategory::Monster.cap(), 28);
        assert_eq!(SpawnCategory::Creature.cap(), 4);
        assert_eq!(SpawnCategory::Ambient.cap(), 6);
        assert_eq!(SpawnCategory::WaterCreature.cap(), 2);
        assert_eq!(SpawnCategory::WaterAmbient.cap(), 8);
        assert_eq!(SpawnCategory::UndergroundWaterCreature.cap(), 2);
        let sum: usize = ALL_CATEGORIES.iter().map(|c| c.cap()).sum();
        assert_eq!(sum, crate::mob::state::MOB_CAP_DEFAULT);
    }

    #[test]
    fn category_of_kind_maps_the_entity_type_registry() {
        assert_eq!(category_of_kind(KIND_ZOMBIE), Some(SpawnCategory::Monster));
        assert_eq!(category_of_kind(KIND_PHANTOM), Some(SpawnCategory::Monster));
        assert_eq!(category_of_kind(KIND_SHEEP), Some(SpawnCategory::Creature));
        // The jungle ocelot: drawn from the monster LIST but counts
        // as a CREATURE (EntityType.OCELOT category).
        assert_eq!(
            category_of_kind(super::super::kinds::KIND_OCELOT),
            Some(SpawnCategory::Creature)
        );
        assert_eq!(category_of_kind(KIND_BAT), Some(SpawnCategory::Ambient));
        assert_eq!(
            category_of_kind(KIND_SQUID),
            Some(SpawnCategory::WaterCreature)
        );
        assert_eq!(
            category_of_kind(KIND_GLOW_SQUID),
            Some(SpawnCategory::UndergroundWaterCreature)
        );
        // MISC kinds are never counted / drawn.
        assert_eq!(category_of_kind(KIND_VILLAGER), None);
        assert_eq!(category_of_kind(KIND_IRON_GOLEM), None);
    }

    #[test]
    fn weighted_pick_on_empty_returns_none() {
        assert_eq!(weighted_pick(&[], 0.5).map(|r| r.kind), None);
    }

    #[test]
    fn weighted_pick_single_entry_always_returns_it() {
        static ONE: &[SpawnRow] = &[SpawnRow {
            kind: KIND_SHEEP,
            weight: 1,
            min_count: 1,
            max_count: 1,
        }];
        for roll in [0.0, 0.5, 0.99] {
            assert_eq!(weighted_pick(ONE, roll).map(|r| r.kind), Some(KIND_SHEEP));
        }
    }

    #[test]
    fn weighted_pick_zero_total_returns_none() {
        static DEGENERATE: &[SpawnRow] = &[
            SpawnRow { kind: KIND_SHEEP, weight: 0, min_count: 1, max_count: 1 },
            SpawnRow { kind: KIND_ZOMBIE, weight: 0, min_count: 1, max_count: 1 },
        ];
        assert!(weighted_pick(DEGENERATE, 0.5).is_none());
    }

    #[test]
    fn weighted_pick_distribution_over_many_samples() {
        // 10_000 deterministic draws over `[(sheep, 1), (zombie, 3)]`
        // should land ~25% sheep + ~75% zombie, via the same
        // xorshift family the live spawner runs.
        use super::super::spawn::SpawnRng;
        static POOL: &[SpawnRow] = &[
            SpawnRow { kind: KIND_SHEEP, weight: 1, min_count: 1, max_count: 1 },
            SpawnRow { kind: KIND_ZOMBIE, weight: 3, min_count: 1, max_count: 1 },
        ];
        let mut seed = 0xC0FFEEu32;
        let mut rng = SpawnRng::new(&mut seed);
        let mut sheep = 0u32;
        let n = 10_000u32;
        for _ in 0..n {
            match weighted_pick(POOL, rng.next_f32()).map(|r| r.kind) {
                Some(k) if k == KIND_SHEEP => sheep += 1,
                Some(k) if k == KIND_ZOMBIE => {}
                _ => panic!("unexpected"),
            }
        }
        let ratio = sheep as f32 / n as f32;
        assert!(
            (0.22..=0.28).contains(&ratio),
            "sheep ratio out of band: {ratio}"
        );
    }

    #[test]
    fn every_biome_has_a_pool() {
        // Pool lookup must be total over the Biome enum.
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
}
