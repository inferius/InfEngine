//! W477 - chest loot tables GENERATED from
//! `data/minecraft/loot_table/chests/village/*.json` (MC 26.1.2). Do not
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

use super::{LootPool, LootTable, counted, one};


/// `chests/village/village_armorer.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_ARMORER: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 5,
            entries: &[
                counted("iron_ingot", 2, 1, 3),
                counted("bread", 4, 1, 4),
                one("iron_helmet", 1),
                one("emerald", 1),
            ],
        },
    ],
};

/// `chests/village/village_butcher.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_BUTCHER: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 5,
            entries: &[
                one("emerald", 1),
                counted("porkchop", 6, 1, 3),
                counted("wheat", 6, 1, 3),
                counted("beef", 6, 1, 3),
                counted("mutton", 6, 1, 3),
                counted("coal", 3, 1, 3),
            ],
        },
    ],
};

/// `chests/village/village_cartographer.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_CARTOGRAPHER: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 5,
            entries: &[
                counted("map", 10, 1, 3),
                counted("paper", 15, 1, 5),
                one("compass", 5),
                counted("bread", 15, 1, 4),
                counted("stick", 5, 1, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("bundle", 1),
                one("", 2),
            ],
        },
    ],
};

/// `chests/village/village_desert_house.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_DESERT_HOUSE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 8,
            entries: &[
                one("clay_ball", 1),
                one("green_dye", 1),
                counted("cactus", 10, 1, 4),
                counted("wheat", 10, 1, 7),
                counted("bread", 10, 1, 4),
                one("book", 1),
                counted("dead_bush", 2, 1, 3),
                counted("emerald", 1, 1, 3),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("bundle", 1),
                one("", 2),
            ],
        },
    ],
};

/// `chests/village/village_fisher.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_FISHER: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 5,
            entries: &[
                one("emerald", 1),
                counted("cod", 2, 1, 3),
                counted("salmon", 1, 1, 3),
                counted("water_bucket", 1, 1, 3),
                counted("barrel", 1, 1, 3),
                counted("wheat_seeds", 3, 1, 3),
                counted("coal", 2, 1, 3),
            ],
        },
    ],
};

/// `chests/village/village_fletcher.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_FLETCHER: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 5,
            entries: &[
                one("emerald", 1),
                counted("arrow", 2, 1, 3),
                counted("feather", 6, 1, 3),
                counted("egg", 2, 1, 3),
                counted("flint", 6, 1, 3),
                counted("stick", 6, 1, 3),
            ],
        },
    ],
};

/// `chests/village/village_mason.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_MASON: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 5,
            entries: &[
                counted("clay_ball", 1, 1, 3),
                one("flower_pot", 1),
                one("stone", 2),
                one("stone_bricks", 2),
                counted("bread", 4, 1, 4),
                one("yellow_dye", 1),
                one("smooth_stone", 1),
                one("emerald", 1),
            ],
        },
    ],
};

/// `chests/village/village_plains_house.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_PLAINS_HOUSE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 8,
            entries: &[
                counted("gold_nugget", 1, 1, 3),
                one("dandelion", 2),
                one("poppy", 1),
                counted("potato", 10, 1, 7),
                counted("bread", 10, 1, 4),
                counted("apple", 10, 1, 5),
                one("book", 1),
                one("feather", 1),
                counted("emerald", 2, 1, 4),
                counted("oak_sapling", 5, 1, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("bundle", 1),
                one("", 2),
            ],
        },
    ],
};

/// `chests/village/village_savanna_house.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_SAVANNA_HOUSE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 8,
            entries: &[
                counted("gold_nugget", 1, 1, 3),
                one("short_grass", 5),
                one("tall_grass", 5),
                counted("bread", 10, 1, 4),
                counted("wheat_seeds", 10, 1, 5),
                counted("emerald", 2, 1, 4),
                counted("acacia_sapling", 10, 1, 2),
                one("saddle", 1),
                counted("torch", 1, 1, 2),
                one("bucket", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("bundle", 1),
                one("", 2),
            ],
        },
    ],
};

/// `chests/village/village_shepherd.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_SHEPHERD: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 5,
            entries: &[
                counted("white_wool", 6, 1, 8),
                counted("black_wool", 3, 1, 3),
                counted("gray_wool", 2, 1, 3),
                counted("brown_wool", 2, 1, 3),
                counted("light_gray_wool", 2, 1, 3),
                one("emerald", 1),
                one("shears", 1),
                counted("wheat", 6, 1, 6),
            ],
        },
    ],
};

/// `chests/village/village_snowy_house.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_SNOWY_HOUSE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 8,
            entries: &[
                one("blue_ice", 1),
                one("snow_block", 4),
                counted("potato", 10, 1, 7),
                counted("bread", 10, 1, 4),
                counted("beetroot_seeds", 10, 1, 5),
                one("beetroot_soup", 1),
                one("furnace", 1),
                counted("emerald", 1, 1, 4),
                counted("snowball", 10, 1, 7),
                counted("coal", 5, 1, 4),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("bundle", 1),
                one("", 2),
            ],
        },
    ],
};

/// `chests/village/village_taiga_house.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_TAIGA_HOUSE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 8,
            entries: &[
                counted("iron_nugget", 1, 1, 5),
                one("fern", 2),
                one("large_fern", 2),
                counted("potato", 10, 1, 7),
                counted("sweet_berries", 5, 1, 7),
                counted("bread", 10, 1, 4),
                counted("pumpkin_seeds", 5, 1, 5),
                one("pumpkin_pie", 1),
                counted("emerald", 2, 1, 4),
                counted("spruce_sapling", 5, 1, 5),
                one("spruce_sign", 1),
                counted("spruce_log", 10, 1, 5),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("bundle", 1),
                one("", 2),
            ],
        },
    ],
};

/// `chests/village/village_tannery.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_TANNERY: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 5,
            entries: &[
                counted("leather", 1, 1, 3),
                one("leather_chestplate", 2),
                one("leather_boots", 2),
                one("leather_helmet", 2),
                counted("bread", 5, 1, 4),
                one("leather_leggings", 2),
                one("saddle", 1),
                counted("emerald", 1, 1, 4),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("bundle", 1),
                one("", 2),
            ],
        },
    ],
};

/// `chests/village/village_temple.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_TEMPLE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 8,
            entries: &[
                counted("redstone", 2, 1, 4),
                counted("bread", 7, 1, 4),
                counted("rotten_flesh", 7, 1, 4),
                counted("lapis_lazuli", 1, 1, 4),
                counted("gold_ingot", 1, 1, 4),
                counted("emerald", 1, 1, 4),
            ],
        },
    ],
};

/// `chests/village/village_toolsmith.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_TOOLSMITH: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 8,
            entries: &[
                counted("diamond", 1, 1, 3),
                counted("iron_ingot", 5, 1, 5),
                counted("gold_ingot", 1, 1, 3),
                counted("bread", 15, 1, 3),
                one("iron_pickaxe", 5),
                counted("coal", 1, 1, 3),
                counted("stick", 20, 1, 3),
                one("iron_shovel", 5),
            ],
        },
    ],
};

/// `chests/village/village_weaponsmith.json` (26.1.2).
pub(crate) const VILLAGE_VILLAGE_WEAPONSMITH: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 8,
            entries: &[
                counted("diamond", 3, 1, 3),
                counted("iron_ingot", 10, 1, 5),
                counted("gold_ingot", 5, 1, 3),
                counted("bread", 15, 1, 3),
                counted("apple", 15, 1, 3),
                one("iron_pickaxe", 5),
                one("iron_sword", 5),
                one("iron_spear", 5),
                one("copper_spear", 7),
                one("iron_chestplate", 5),
                one("iron_helmet", 5),
                one("iron_leggings", 5),
                one("iron_boots", 5),
                counted("obsidian", 5, 3, 7),
                counted("oak_sapling", 5, 3, 7),
                one("saddle", 3),
                one("copper_horse_armor", 1),
                one("iron_horse_armor", 1),
                one("golden_horse_armor", 1),
                one("diamond_horse_armor", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("bundle", 1),
                one("", 2),
            ],
        },
    ],
};
