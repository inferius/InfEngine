//! W477 - chest loot tables GENERATED from
//! `data/minecraft/loot_table/chests/trial_chambers/*.json` (MC 26.1.2). Do not
//! hand-edit rows; regenerate instead (temp-dir python, see the W477
//! commit).
//!
//! Feature coverage: pool rolls (constant / uniform), weighted item
//! entries, `set_count` (constant / uniform), `minecraft:empty`
//! no-drop slots, `minecraft:loot_table` refs (`reft`, the
//! trial-chamber reward chain), and pool-level `random_chance`
//! encoded as an exact weighted empty slot on the constant-1-roll
//! pools MC uses it on. Every other function (enchant_randomly /
//! enchant_with_levels / set_damage / set_potion / set_stew_effect /
//! exploration_map / set_name / set_ominous_bottle_amplifier /
//! set_enchantments) drops with a per-table doc note - the entry
//! emits the PLAIN item, exactly like the audited W242 hand tables
//! did. Names the item registry lacks skip via the roller's
//! once-per-name log.

use super::{LootPool, LootTable, counted, one, reft};


/// `chests/trial_chambers/corridor.json` (26.1.2). Dropped, items emit
/// plain (queued): set_damage on iron_axe, stone_axe, stone_pickaxe;
/// enchant_randomly on iron_axe.
pub(crate) const TRIAL_CHAMBERS_CORRIDOR: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[
                one("iron_axe", 1),
                counted("honeycomb", 1, 2, 8),
                one("stone_axe", 2),
                one("stone_pickaxe", 2),
                counted("ender_pearl", 2, 1, 2),
                counted("bamboo_hanging_sign", 2, 1, 4),
                counted("bamboo_planks", 2, 3, 6),
                counted("scaffolding", 2, 2, 10),
                counted("torch", 2, 3, 6),
                counted("tuff", 3, 8, 20),
            ],
        },
    ],
};

/// `chests/trial_chambers/entrance.json` (26.1.2).
pub(crate) const TRIAL_CHAMBERS_ENTRANCE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 3,
            entries: &[
                one("trial_key", 1),
                counted("stick", 5, 2, 5),
                one("wooden_axe", 10),
                counted("honeycomb", 10, 2, 8),
                counted("arrow", 10, 5, 10),
            ],
        },
    ],
};

/// `chests/trial_chambers/intersection.json` (26.1.2). Dropped, items
/// emit plain (queued): set_damage on diamond_axe, diamond_pickaxe.
pub(crate) const TRIAL_CHAMBERS_INTERSECTION: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[
                one("diamond_block", 1),
                counted("emerald_block", 5, 1, 3),
                one("diamond_axe", 5),
                one("diamond_pickaxe", 5),
                counted("diamond", 10, 1, 2),
                counted("cake", 20, 1, 4),
                counted("amethyst_shard", 20, 8, 20),
                counted("iron_block", 20, 1, 2),
            ],
        },
    ],
};

/// `chests/trial_chambers/intersection_barrel.json` (26.1.2). Dropped,
/// items emit plain (queued): set_damage on compass, diamond_axe,
/// diamond_pickaxe, golden_axe, golden_pickaxe; enchant_randomly on
/// diamond_axe.
pub(crate) const TRIAL_CHAMBERS_INTERSECTION_BARREL: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[
                one("diamond_axe", 1),
                one("diamond_pickaxe", 1),
                counted("diamond", 1, 1, 3),
                one("compass", 1),
                counted("bucket", 1, 1, 2),
                one("golden_axe", 4),
                one("golden_pickaxe", 4),
                counted("bamboo_planks", 10, 5, 15),
                counted("baked_potato", 10, 6, 10),
            ],
        },
    ],
};

/// `chests/trial_chambers/reward.json` (26.1.2). Dropped, items emit
/// plain (queued): pool random_chance 0.25 -> weighted empty slot
/// (exact).
pub(crate) const TRIAL_CHAMBERS_REWARD: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                reft(&TRIAL_CHAMBERS_REWARD_RARE, 8),
                reft(&TRIAL_CHAMBERS_REWARD_COMMON, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[
                reft(&TRIAL_CHAMBERS_REWARD_COMMON, 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                reft(&TRIAL_CHAMBERS_REWARD_UNIQUE, 1),
                one("", 3),
            ],
        },
    ],
};

/// `chests/trial_chambers/reward_common.json` (26.1.2). Dropped, items
/// emit plain (queued): set_potion on tipped_arrow;
/// set_ominous_bottle_amplifier on ominous_bottle.
pub(crate) const TRIAL_CHAMBERS_REWARD_COMMON: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                counted("arrow", 4, 2, 8),
                counted("tipped_arrow", 4, 2, 8),
                counted("emerald", 4, 2, 4),
                counted("wind_charge", 3, 1, 3),
                counted("iron_ingot", 3, 1, 4),
                counted("honey_bottle", 3, 1, 2),
                one("ominous_bottle", 2),
                counted("wind_charge", 1, 4, 12),
                counted("diamond", 1, 1, 2),
            ],
        },
    ],
};

/// `chests/trial_chambers/reward_ominous.json` (26.1.2). Dropped, items
/// emit plain (queued): pool random_chance 0.75 -> weighted empty slot
/// (exact).
pub(crate) const TRIAL_CHAMBERS_REWARD_OMINOUS: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                reft(&TRIAL_CHAMBERS_REWARD_OMINOUS_RARE, 8),
                reft(&TRIAL_CHAMBERS_REWARD_OMINOUS_COMMON, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[
                reft(&TRIAL_CHAMBERS_REWARD_OMINOUS_COMMON, 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                reft(&TRIAL_CHAMBERS_REWARD_OMINOUS_UNIQUE, 3),
                one("", 1),
            ],
        },
    ],
};

/// `chests/trial_chambers/reward_ominous_common.json` (26.1.2).
/// Dropped, items emit plain (queued): set_potion on tipped_arrow;
/// set_ominous_bottle_amplifier on ominous_bottle.
pub(crate) const TRIAL_CHAMBERS_REWARD_OMINOUS_COMMON: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                counted("emerald", 5, 4, 10),
                counted("wind_charge", 4, 8, 12),
                counted("tipped_arrow", 3, 4, 12),
                counted("diamond", 2, 2, 3),
                one("ominous_bottle", 1),
            ],
        },
    ],
};

/// `chests/trial_chambers/reward_ominous_rare.json` (26.1.2). Dropped,
/// items emit plain (queued): enchant_with_levels on crossbow,
/// diamond_axe, diamond_chestplate; enchant_randomly on book;
/// set_enchantments on book.
pub(crate) const TRIAL_CHAMBERS_REWARD_OMINOUS_RARE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("emerald_block", 5),
                one("iron_block", 4),
                one("crossbow", 4),
                one("golden_apple", 3),
                one("diamond_axe", 3),
                one("diamond_chestplate", 3),
                one("book", 2),
                one("book", 2),
                one("book", 2),
                one("diamond_block", 1),
            ],
        },
    ],
};

/// `chests/trial_chambers/reward_ominous_unique.json` (26.1.2).
pub(crate) const TRIAL_CHAMBERS_REWARD_OMINOUS_UNIQUE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("enchanted_golden_apple", 3),
                one("flow_armor_trim_smithing_template", 3),
                one("flow_banner_pattern", 2),
                one("music_disc_creator", 1),
                one("heavy_core", 1),
            ],
        },
    ],
};

/// `chests/trial_chambers/reward_rare.json` (26.1.2). Dropped, items
/// emit plain (queued): set_damage on shield; enchant_with_levels on
/// bow, crossbow, diamond_axe, diamond_chestplate, iron_axe,
/// iron_chestplate; enchant_randomly on book.
pub(crate) const TRIAL_CHAMBERS_REWARD_RARE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                counted("emerald", 3, 2, 4),
                one("shield", 3),
                one("bow", 3),
                one("crossbow", 2),
                one("iron_axe", 2),
                one("iron_chestplate", 2),
                counted("golden_carrot", 2, 1, 2),
                one("book", 2),
                one("book", 2),
                one("diamond_chestplate", 1),
                one("diamond_axe", 1),
            ],
        },
    ],
};

/// `chests/trial_chambers/reward_unique.json` (26.1.2).
pub(crate) const TRIAL_CHAMBERS_REWARD_UNIQUE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("golden_apple", 4),
                one("bolt_armor_trim_smithing_template", 3),
                one("guster_banner_pattern", 2),
                one("music_disc_precipice", 2),
                one("trident", 1),
            ],
        },
    ],
};

/// `chests/trial_chambers/supply.json` (26.1.2). Dropped, items emit
/// plain (queued): set_potion on potion, tipped_arrow; set_damage on
/// stone_pickaxe.
pub(crate) const TRIAL_CHAMBERS_SUPPLY: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 5,
            entries: &[
                counted("arrow", 2, 4, 14),
                counted("tipped_arrow", 1, 4, 8),
                counted("tipped_arrow", 1, 4, 8),
                counted("baked_potato", 2, 2, 4),
                counted("glow_berries", 2, 2, 10),
                counted("acacia_planks", 1, 3, 6),
                counted("moss_block", 1, 2, 5),
                counted("bone_meal", 1, 2, 5),
                counted("tuff", 1, 5, 10),
                counted("torch", 1, 3, 6),
                counted("potion", 1, 2, 2),
                counted("potion", 1, 2, 2),
                one("stone_pickaxe", 2),
                one("milk_bucket", 1),
            ],
        },
    ],
};
