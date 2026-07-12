//! W242: MC chest loot tables.
//!
//! Each table mirrors one JSON from the decompiled data pack
//! (`F:\Data\mc_assets\mc_src\26.1.2\data\minecraft\loot_table\...`)
//! as a const: pools with a uniform roll count and weighted item
//! entries carrying an optional `set_count` range. The roller maps
//! MC item names onto our `ITEM_TYPES` rows; names our registry
//! doesn't carry yet are skipped with a once-per-name log so a
//! future item wave picks them up by deleting a line from nowhere.
//!
//! First user: the monster-room dungeon chest (simple_dungeon).
//! Mineshaft / village chests reuse the same interpreter later.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::worldgen::mcgen::noise::SplitMix64;

/// One weighted entry of a loot pool: MC item name + weight +
/// inclusive stack-count range (MC `set_count` uniform; entries
/// without the function drop exactly 1).
pub(crate) struct LootEntry {
    pub(crate) name: &'static str,
    pub(crate) weight: u32,
    pub(crate) count_min: u8,
    pub(crate) count_max: u8,
}

const fn one(name: &'static str, weight: u32) -> LootEntry {
    LootEntry { name, weight, count_min: 1, count_max: 1 }
}

const fn counted(
    name: &'static str,
    weight: u32,
    count_min: u8,
    count_max: u8,
) -> LootEntry {
    LootEntry { name, weight, count_min, count_max }
}

/// One pool: roll `rolls_min..=rolls_max` times; each roll picks one
/// entry weighted by `weight`.
pub(crate) struct LootPool {
    pub(crate) rolls_min: u8,
    pub(crate) rolls_max: u8,
    pub(crate) entries: &'static [LootEntry],
}

pub(crate) struct LootTable {
    pub(crate) pools: &'static [LootPool],
}

/// `data/minecraft/loot_table/chests/simple_dungeon.json` (26.1.2),
/// entry-for-entry. Parity gaps, all skipped via the name mapper
/// until the items exist: enchanted_golden_apple, the three music
/// discs, name_tag, the four horse-armor tiers. `book` maps but
/// drops UNenchanted (no enchant_randomly support yet).
pub(crate) const SIMPLE_DUNGEON: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[
                counted("leather", 20, 1, 5),
                one("golden_apple", 15),
                one("enchanted_golden_apple", 2),
                one("music_disc_otherside", 2),
                one("music_disc_13", 15),
                one("music_disc_cat", 15),
                one("name_tag", 20),
                one("golden_horse_armor", 10),
                one("copper_horse_armor", 15),
                one("iron_horse_armor", 15),
                one("diamond_horse_armor", 5),
                one("book", 10),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 4,
            entries: &[
                counted("iron_ingot", 10, 1, 4),
                counted("gold_ingot", 5, 1, 4),
                one("bread", 20),
                counted("wheat", 20, 1, 4),
                one("bucket", 10),
                counted("redstone", 15, 1, 4),
                counted("coal", 15, 1, 4),
                counted("melon_seeds", 10, 2, 4),
                counted("pumpkin_seeds", 10, 2, 4),
                counted("beetroot_seeds", 10, 2, 4),
            ],
        },
        LootPool {
            rolls_min: 3,
            rolls_max: 3,
            entries: &[
                counted("bone", 10, 1, 8),
                counted("gunpowder", 10, 1, 8),
                counted("rotten_flesh", 10, 1, 8),
                counted("string", 10, 1, 8),
            ],
        },
    ],
};

/// `data/minecraft/loot_table/chests/abandoned_mineshaft.json`
/// (26.1.2), entry-for-entry. Known mapper skips (once-logged until
/// the items exist): enchanted_golden_apple, name_tag; `book` drops
/// UNenchanted (no enchant_randomly). The `empty` weight-5 entry of
/// pool 1 is a real MC no-drop slot.
pub(crate) const ABANDONED_MINESHAFT: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("golden_apple", 20),
                one("enchanted_golden_apple", 1),
                one("name_tag", 30),
                one("book", 10),
                one("iron_pickaxe", 5),
                // minecraft:empty weight 5 - a rolled blank.
                one("", 5),
            ],
        },
        LootPool {
            rolls_min: 2,
            rolls_max: 4,
            entries: &[
                counted("iron_ingot", 10, 1, 5),
                counted("gold_ingot", 5, 1, 3),
                counted("redstone", 5, 4, 9),
                counted("lapis_lazuli", 5, 4, 9),
                counted("diamond", 3, 1, 2),
                counted("coal", 10, 3, 8),
                counted("bread", 15, 1, 3),
                counted("glow_berries", 15, 3, 6),
                counted("melon_seeds", 10, 2, 4),
                counted("pumpkin_seeds", 10, 2, 4),
                counted("beetroot_seeds", 10, 2, 4),
            ],
        },
        LootPool {
            rolls_min: 3,
            rolls_max: 3,
            entries: &[
                counted("rail", 20, 4, 8),
                counted("powered_rail", 5, 1, 4),
                counted("detector_rail", 5, 1, 4),
                counted("activator_rail", 5, 1, 4),
                counted("torch", 15, 1, 16),
            ],
        },
    ],
};

/// `data/minecraft/loot_table/chests/desert_pyramid.json` (26.1.2),
/// entry-for-entry. Mapper skips until the items exist: horse
/// armors, enchanted_golden_apple, the dune armor-trim template;
/// `book` drops UNenchanted. The weight-15 / weight-6 `empty`
/// entries are real MC no-drop slots.
pub(crate) const DESERT_PYRAMID: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 4,
            entries: &[
                counted("diamond", 5, 1, 3),
                counted("iron_ingot", 15, 1, 5),
                counted("gold_ingot", 15, 2, 7),
                counted("emerald", 15, 1, 3),
                counted("bone", 25, 4, 6),
                counted("spider_eye", 25, 1, 3),
                counted("rotten_flesh", 25, 3, 7),
                counted("leather", 20, 1, 5),
                one("copper_horse_armor", 15),
                one("iron_horse_armor", 15),
                one("golden_horse_armor", 10),
                one("diamond_horse_armor", 5),
                one("book", 20),
                one("golden_apple", 20),
                one("enchanted_golden_apple", 2),
                one("", 15),
            ],
        },
        LootPool {
            rolls_min: 4,
            rolls_max: 4,
            entries: &[
                counted("bone", 10, 1, 8),
                counted("gunpowder", 10, 1, 8),
                counted("rotten_flesh", 10, 1, 8),
                counted("string", 10, 1, 8),
                counted("sand", 10, 1, 8),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 6),
                counted("dune_armor_trim_smithing_template", 1, 2, 2),
            ],
        },
    ],
};

/// `data/minecraft/loot_table/chests/igloo_chest.json` (26.1.2).
pub(crate) const IGLOO_CHEST: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 8,
            entries: &[
                counted("apple", 15, 1, 3),
                counted("coal", 15, 1, 4),
                counted("gold_nugget", 10, 1, 3),
                one("stone_axe", 2),
                one("rotten_flesh", 10),
                one("emerald", 1),
                counted("wheat", 10, 2, 3),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[one("golden_apple", 1)],
        },
    ],
};

/// `data/minecraft/loot_table/chests/pillager_outpost.json` (26.1.2).
/// Parity gaps skipped via the name mapper until the items exist:
/// crossbow, experience_bottle, tripwire_hook, goat_horn (+ its
/// set_instrument function), sentry_armor_trim_smithing_template;
/// `book` drops UNenchanted (no enchant_randomly).
pub(crate) const PILLAGER_OUTPOST: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 0,
            rolls_max: 1,
            entries: &[one("crossbow", 1)],
        },
        LootPool {
            rolls_min: 2,
            rolls_max: 3,
            entries: &[
                counted("wheat", 7, 3, 5),
                counted("potato", 5, 2, 5),
                counted("carrot", 5, 3, 5),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[counted("dark_oak_log", 1, 2, 3)],
        },
        LootPool {
            rolls_min: 2,
            rolls_max: 3,
            entries: &[
                one("experience_bottle", 7),
                counted("string", 4, 1, 6),
                counted("arrow", 4, 2, 7),
                counted("tripwire_hook", 3, 1, 3),
                counted("iron_ingot", 3, 1, 3),
                one("book", 1),
            ],
        },
        LootPool {
            rolls_min: 0,
            rolls_max: 1,
            entries: &[one("goat_horn", 1)],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 3),
                counted("sentry_armor_trim_smithing_template", 1, 2, 2),
            ],
        },
    ],
};

/// `data/minecraft/loot_table/chests/shipwreck_map.json` (26.1.2).
/// The map itself + navigation gear. Gaps via the name mapper: map,
/// compass, clock, the coast trim + nautilus armors.
pub(crate) const SHIPWRECK_MAP: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("map", 1)] },
        LootPool {
            rolls_min: 3,
            rolls_max: 3,
            entries: &[
                one("compass", 1),
                one("map", 1),
                one("clock", 1),
                counted("paper", 20, 1, 10),
                counted("feather", 10, 1, 5),
                counted("book", 5, 1, 5),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 5),
                counted("coast_armor_trim_smithing_template", 1, 2, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 148),
                one("copper_nautilus_armor", 20),
                one("iron_nautilus_armor", 10),
                one("golden_nautilus_armor", 5),
                one("diamond_nautilus_armor", 2),
            ],
        },
    ],
};

/// `data/minecraft/loot_table/chests/shipwreck_supply.json` (26.1.2).
pub(crate) const SHIPWRECK_SUPPLY: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 10,
            entries: &[
                counted("paper", 8, 1, 12),
                counted("potato", 7, 2, 6),
                counted("moss_block", 7, 1, 4),
                counted("poisonous_potato", 7, 2, 6),
                counted("carrot", 7, 4, 8),
                counted("wheat", 7, 8, 21),
                one("suspicious_stew", 10),
                counted("coal", 6, 2, 8),
                counted("rotten_flesh", 5, 5, 24),
                counted("pumpkin", 2, 1, 3),
                counted("bamboo", 2, 1, 3),
                counted("gunpowder", 3, 1, 5),
                counted("tnt", 1, 1, 2),
                one("leather_helmet", 3),
                one("leather_chestplate", 3),
                one("leather_leggings", 3),
                one("leather_boots", 3),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 5),
                counted("coast_armor_trim_smithing_template", 1, 2, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 148),
                one("copper_nautilus_armor", 20),
                one("iron_nautilus_armor", 10),
                one("golden_nautilus_armor", 5),
                one("diamond_nautilus_armor", 2),
            ],
        },
    ],
};

/// `data/minecraft/loot_table/chests/shipwreck_treasure.json`
/// (26.1.2).
pub(crate) const SHIPWRECK_TREASURE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 6,
            entries: &[
                counted("iron_ingot", 90, 1, 5),
                counted("gold_ingot", 10, 1, 5),
                counted("emerald", 40, 1, 5),
                one("diamond", 5),
                one("experience_bottle", 5),
            ],
        },
        LootPool {
            rolls_min: 2,
            rolls_max: 5,
            entries: &[
                counted("iron_nugget", 50, 1, 10),
                counted("gold_nugget", 10, 1, 10),
                counted("lapis_lazuli", 20, 1, 10),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 5),
                counted("coast_armor_trim_smithing_template", 1, 2, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 148),
                one("copper_nautilus_armor", 20),
                one("iron_nautilus_armor", 10),
                one("golden_nautilus_armor", 5),
                one("diamond_nautilus_armor", 2),
            ],
        },
    ],
};

/// `data/minecraft/loot_table/chests/buried_treasure.json` (26.1.2).
/// Gaps via the name mapper: heart_of_the_sea, prismarine_crystals,
/// iron_spear, the water-breathing potion (set_potion), nautilus
/// armors.
pub(crate) const BURIED_TREASURE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[one("heart_of_the_sea", 1)],
        },
        LootPool {
            rolls_min: 5,
            rolls_max: 8,
            entries: &[
                counted("iron_ingot", 20, 1, 4),
                counted("gold_ingot", 10, 1, 4),
                counted("tnt", 5, 1, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[
                counted("emerald", 5, 4, 8),
                counted("diamond", 5, 1, 2),
                counted("prismarine_crystals", 5, 1, 5),
            ],
        },
        LootPool {
            rolls_min: 0,
            rolls_max: 1,
            entries: &[
                one("leather_chestplate", 1),
                one("iron_sword", 1),
                one("iron_spear", 1),
            ],
        },
        LootPool {
            rolls_min: 2,
            rolls_max: 2,
            entries: &[
                counted("cooked_cod", 1, 2, 4),
                counted("cooked_salmon", 1, 2, 4),
            ],
        },
        LootPool {
            rolls_min: 0,
            rolls_max: 2,
            entries: &[one("potion", 1)],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 148),
                one("copper_nautilus_armor", 20),
                one("iron_nautilus_armor", 10),
                one("golden_nautilus_armor", 5),
                one("diamond_nautilus_armor", 2),
            ],
        },
    ],
};

/// MC item name -> our `ITEM_TYPES` name where they differ. Names
/// not listed resolve verbatim; names that resolve to nothing are
/// skipped (once-logged).
const LOOT_NAME_MAP: &[(&str, &str)] = &[("redstone", "redstone_dust")];

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
            let count = roll_range(rng, entry.count_min, entry.count_max);
            // W243: `minecraft:empty` entries (a real no-drop slot in
            // e.g. abandoned_mineshaft pool 1) roll silently.
            if entry.name.is_empty() {
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
        .filter(|&i| container.slot(i).is_none())
        .collect();
    for &(id, count) in stacks {
        if free.is_empty() {
            break;
        }
        let pick = (rng.next_u64() % free.len() as u64) as usize;
        let slot = free.swap_remove(pick);
        container.set_slot(slot, Some((id, count)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every simple_dungeon entry either resolves to a real item id
    /// or is one of the KNOWN parity gaps - a new unmapped name is a
    /// regression (typo or a renamed item row).
    #[test]
    fn simple_dungeon_names_resolve_or_are_known_gaps() {
        const KNOWN_GAPS: &[&str] = &[
            "enchanted_golden_apple",
            "music_disc_otherside",
            "music_disc_13",
            "music_disc_cat",
            "name_tag",
            "golden_horse_armor",
            "copper_horse_armor",
            "iron_horse_armor",
            "diamond_horse_armor",
        ];
        for pool in SIMPLE_DUNGEON.pools {
            for e in pool.entries {
                if item_id_for_loot_name(e.name).is_none() {
                    assert!(
                        KNOWN_GAPS.contains(&e.name),
                        "loot name {} resolves to nothing and is not a \
                         known gap",
                        e.name
                    );
                }
            }
        }
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
        let mut c = crate::container::Container::with_size(27);
        c.set_slot(3, Some((1, 1)));
        let stacks = vec![(10u16, 2u8), (11, 3), (12, 4)];
        fill_container(&mut c, &stacks, &mut SplitMix64::new(7));
        assert_eq!(c.slot(3), Some((1, 1)), "existing slot untouched");
        let placed: Vec<_> =
            (0..27).filter_map(|i| c.slot(i)).filter(|s| s.0 >= 10).collect();
        assert_eq!(placed.len(), 3);
    }
}
