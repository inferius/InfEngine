//! W242/W477: MC loot-table interpreter + the generated chest tables.
//!
//! Each table mirrors one JSON from the decompiled data pack
//! (`F:\Data\mc_assets\mc_src\26.1.2\data\minecraft\loot_table\...`)
//! as a const: pools with a uniform roll count and weighted item
//! entries carrying an optional `set_count` range. The roller maps
//! MC item names onto our `ITEM_TYPES` rows; names our registry
//! doesn't carry yet are skipped with a once-per-name log so a
//! future item wave picks them up by deleting a line from nowhere.
//!
//! W477: the 28 hand-transcribed W242 chest tables are replaced by
//! GENERATED consts covering the full `chests/**` directory (56
//! tables incl. the 16 village + 13 trial-chamber files) - see
//! `tables_gen*`. Entity loot (`mob/loot_table.rs`, W474) reuses the
//! same interpreter. Which worldgen chest rolls which table lives in
//! `routing`.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::worldgen::mcgen::noise::SplitMix64;

pub(crate) mod routing;
pub(crate) mod tables_gen;
pub(crate) mod tables_gen_trial;
pub(crate) mod tables_gen_village;

#[cfg(test)]
mod tests_hand_equiv;

/// One weighted entry of a loot pool: MC item name + weight +
/// inclusive stack-count range (MC `set_count` uniform; entries
/// without the function drop exactly 1). W477: `subtable` models
/// `minecraft:loot_table` ref entries (the trial-chamber reward
/// chain) - a picked ref rolls the referenced table inline.
#[derive(Debug, PartialEq)]
pub(crate) struct LootEntry {
    pub(crate) name: &'static str,
    pub(crate) weight: u32,
    pub(crate) count_min: u8,
    pub(crate) count_max: u8,
    pub(crate) subtable: Option<&'static LootTable>,
}

pub(crate) const fn one(name: &'static str, weight: u32) -> LootEntry {
    LootEntry { name, weight, count_min: 1, count_max: 1, subtable: None }
}

pub(crate) const fn counted(
    name: &'static str,
    weight: u32,
    count_min: u8,
    count_max: u8,
) -> LootEntry {
    LootEntry { name, weight, count_min, count_max, subtable: None }
}

/// W477: a `minecraft:loot_table` ref entry.
pub(crate) const fn reft(table: &'static LootTable, weight: u32) -> LootEntry {
    LootEntry {
        name: "",
        weight,
        count_min: 1,
        count_max: 1,
        subtable: Some(table),
    }
}

/// One pool: roll `rolls_min..=rolls_max` times; each roll picks one
/// entry weighted by `weight`.
#[derive(Debug, PartialEq)]
pub(crate) struct LootPool {
    pub(crate) rolls_min: u8,
    pub(crate) rolls_max: u8,
    pub(crate) entries: &'static [LootEntry],
}

#[derive(Debug, PartialEq)]
pub(crate) struct LootTable {
    pub(crate) pools: &'static [LootPool],
}

/// MC item name -> our `ITEM_TYPES` name where they differ. Names
/// not listed resolve verbatim; names that resolve to nothing are
/// skipped (once-logged).
const LOOT_NAME_MAP: &[(&str, &str)] = &[
    ("redstone", "redstone_dust"),
    // W474: the entity tables drop the MC raw-meat names; our item
    // rows carry the raw_ prefix.
    ("beef", "raw_beef"),
    ("porkchop", "raw_porkchop"),
    ("chicken", "raw_chicken"),
    ("mutton", "raw_mutton"),
    ("cod", "raw_cod"),
    ("salmon", "raw_salmon"),
    // W477: chest-table names whose items exist under engine-legacy
    // rows (same convention as the redstone row above). Names with
    // NO equivalent item (snowball, cake, trident, bundle, the trim
    // templates...) stay unmapped and ride the once-per-name skip
    // log until their items land.
    ("oak_log", "wood"),
    ("oak_planks", "planks"),
    ("white_wool", "wool"),
    ("tall_grass", "tall_grass_bottom"),
    ("iron_shovel", "shovel_iron"),
    ("wooden_pickaxe", "pickaxe_wood"),
];

fn item_id_for_loot_name(name: &str) -> Option<u16> {
    let mapped = LOOT_NAME_MAP
        .iter()
        .find(|(mc, _)| *mc == name)
        .map(|(_, ours)| *ours)
        .unwrap_or(name);
    crate::item::ITEM_TYPES
        .iter()
        .find(|it| it.name == mapped)
        .map(|it| it.id)
}

/// Log a skipped loot name once per process, not once per chest.
fn log_skipped(name: &'static str) {
    static SEEN: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut set) = seen.lock()
        && set.insert(name)
    {
        tracing::info!(item = name, "W242: loot item not in registry, skipped");
    }
}

/// Position-mixed loot seed: same world seed + chest position = same
/// loot, forever - chest contents survive chunk regeneration and are
/// identical in every 3x3 feature replay.
pub(crate) fn chest_pos_seed(seed: u32, pos: [i32; 3]) -> u64 {
    (seed as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((pos[0] as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
        .wrapping_add((pos[1] as u64).wrapping_mul(0x94D0_49BB_1331_11EB))
        .wrapping_add((pos[2] as u64).wrapping_mul(0x1656_67B1_9E37_79F9))
        ^ 0x1007_C4E5_7000_0000
}

#[inline]
fn roll_range(rng: &mut SplitMix64, min: u8, max: u8) -> u8 {
    if max <= min {
        return min;
    }
    min + (rng.next_u64() % (max - min + 1) as u64) as u8
}

/// Roll a loot table into `(item_id, count)` stacks. Deterministic
/// for a given rng state, so a position-seeded rng makes chest loot
/// replay-stable across regeneration.
pub(crate) fn roll(table: &LootTable, rng: &mut SplitMix64) -> Vec<(u16, u8)> {
    let mut out = Vec::new();
    for pool in table.pools {
        let rolls = roll_range(rng, pool.rolls_min, pool.rolls_max);
        let total: u32 = pool.entries.iter().map(|e| e.weight).sum();
        if total == 0 {
            continue;
        }
        for _ in 0..rolls {
            let mut pick = (rng.next_u64() % total as u64) as u32;
            let entry = pool
                .entries
                .iter()
                .find(|e| {
                    if pick < e.weight {
                        true
                    } else {
                        pick -= e.weight;
                        false
                    }
                })
                .expect("weighted pick is within total by construction");
            // W477: `minecraft:loot_table` ref entries (the
            // trial-chamber reward chain) roll the referenced table
            // inline. The MC data is acyclic, so the recursion is
            // bounded (reward -> reward_common is depth 1).
            if let Some(sub) = entry.subtable {
                let nested = roll(sub, rng);
                out.extend(nested);
                continue;
            }
            let count = roll_range(rng, entry.count_min, entry.count_max);
            // W243: `minecraft:empty` entries (a real no-drop slot in
            // e.g. abandoned_mineshaft pool 1) roll silently.
            if entry.name.is_empty() {
                continue;
            }
            // W474: entity tables carry `set_count` uniforms with a
            // 0 floor (skeleton arrows 0-2, ...) - a rolled 0 is a
            // no-drop, never a zero-count stack.
            if count == 0 {
                continue;
            }
            match item_id_for_loot_name(entry.name) {
                Some(id) => out.push((id, count)),
                None => log_skipped(entry.name),
            }
        }
    }
    out
}

/// Scatter rolled stacks into random empty slots of a container,
/// like MC spreads chest loot instead of packing it front-to-back.
/// Stacks beyond the free-slot count are dropped (a 27-slot chest
/// never overflows with the shipped tables).
pub(crate) fn fill_container(
    container: &mut crate::container::Container,
    stacks: &[(u16, u8)],
    rng: &mut SplitMix64,
) {
    let mut free: Vec<usize> = (0..container.len())
        .filter(|&i| container.slot_ref(i).is_none())
        .collect();
    for &(id, count) in stacks {
        if free.is_empty() {
            break;
        }
        let pick = (rng.next_u64() % free.len() as u64) as usize;
        let slot = free.swap_remove(pick);
        // Loot is always freshly minted - empty components.
        container.set_slot(slot, Some(crate::item::ItemStack::new(id, count)));
    }
}

#[cfg(test)]
mod tests {
    use super::tables_gen::{ALL_TABLES, SIMPLE_DUNGEON};
    use super::*;

    /// W477: every entry of every GENERATED chest table either
    /// resolves to a real item id or is one of the KNOWN parity
    /// gaps - a new unmapped name is a regression (typo, a renamed
    /// item row, or an MC data bump growing the frontier).
    #[test]
    fn all_table_names_resolve_or_are_known_gaps() {
        const KNOWN_GAPS: &[&str] = &[
            "bolt_armor_trim_smithing_template",
            "bundle",
            "cake",
            "copper_spear",
            "egg",
            "flow_armor_trim_smithing_template",
            "flow_banner_pattern",
            "guster_banner_pattern",
            "heavy_core",
            "honey_bottle",
            "large_fern",
            "lead",
            "music_disc_creator",
            "music_disc_precipice",
            "ominous_bottle",
            "snowball",
            "spire_armor_trim_smithing_template",
            "spruce_sign",
            "sweet_berries",
            "trident",
            "wind_charge",
        ];
        for (table_name, table) in ALL_TABLES {
            for pool in table.pools {
                for e in pool.entries {
                    if e.name.is_empty() {
                        continue; // empty slots + subtable refs
                    }
                    if item_id_for_loot_name(e.name).is_none() {
                        assert!(
                            KNOWN_GAPS.contains(&e.name),
                            "{table_name}: loot name {} resolves to \
                             nothing and is not a known gap",
                            e.name
                        );
                    }
                }
            }
        }
    }

    /// W477: chest coal is the real coal ITEM (325), not the id-13
    /// ore-block hybrid the pre-wave item table shadowed it with.
    #[test]
    fn loot_coal_resolves_to_the_coal_item() {
        assert_eq!(item_id_for_loot_name("coal"), Some(325));
    }

    /// Same seed = same loot; different seeds diverge. (The dungeon
    /// chest seeds the rng from world seed + chest position, so this
    /// is what makes loot replay-stable.)
    #[test]
    fn rolls_are_deterministic_per_seed() {
        let a = roll(&SIMPLE_DUNGEON, &mut SplitMix64::new(1234));
        let b = roll(&SIMPLE_DUNGEON, &mut SplitMix64::new(1234));
        assert_eq!(a, b);
        assert!(!a.is_empty(), "3 constant rolls of pool 3 guarantee loot");
    }

    /// W477: `reft` entries roll the referenced table inline - the
    /// trial-chamber reward chain produces real stacks through two
    /// levels of refs (reward -> reward_common/-rare -> items).
    #[test]
    fn reward_chain_rolls_through_subtable_refs() {
        use super::tables_gen_trial::TRIAL_CHAMBERS_REWARD;
        let mut hits = 0;
        for seed in 0..64u64 {
            let stacks =
                roll(&TRIAL_CHAMBERS_REWARD, &mut SplitMix64::new(seed));
            hits += usize::from(!stacks.is_empty());
            let again =
                roll(&TRIAL_CHAMBERS_REWARD, &mut SplitMix64::new(seed));
            assert_eq!(stacks, again, "seed {seed}: ref rolls deterministic");
        }
        // Pool 1 (1 guaranteed roll into rare/common) + pool 2 make
        // an all-empty streak over 64 seeds effectively impossible -
        // only fully-skipped picks (unmapped names) drop stacks.
        assert!(hits > 32, "reward chain rolled {hits}/64 non-empty");
    }

    /// Roll counts stay inside the per-pool bounds: pool1 1-3 +
    /// pool2 1-4 + pool3 exactly 3 = 5..=10 stacks before skips.
    /// With skips (4 gap names in pool 1) the floor is pool2 min 1 +
    /// pool3 3 = 4.
    #[test]
    fn roll_counts_respect_pool_bounds() {
        for seed in 0..200u64 {
            let stacks = roll(&SIMPLE_DUNGEON, &mut SplitMix64::new(seed));
            assert!(
                (4..=10).contains(&stacks.len()),
                "seed {seed}: {} stacks",
                stacks.len()
            );
            for (id, count) in &stacks {
                assert!(*id != 0 && *count >= 1 && *count <= 8);
            }
        }
    }

    /// fill_container scatters into empty slots without overwriting.
    #[test]
    fn fill_scatters_into_empty_slots() {
        use crate::item::ItemStack;
        let mut c = crate::container::Container::with_size(27);
        c.set_slot(3, Some(ItemStack::new(1, 1)));
        let stacks = vec![(10u16, 2u8), (11, 3), (12, 4)];
        fill_container(&mut c, &stacks, &mut SplitMix64::new(7));
        assert_eq!(
            c.slot(3),
            Some(ItemStack::new(1, 1)),
            "existing slot untouched"
        );
        let placed: Vec<_> = (0..27)
            .filter_map(|i| c.slot(i))
            .filter(|s| s.id >= 10)
            .collect();
        assert_eq!(placed.len(), 3);
    }
}
