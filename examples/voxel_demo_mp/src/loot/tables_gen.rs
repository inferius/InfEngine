//! W477 - chest loot tables GENERATED from
//! `data/minecraft/loot_table/chests/*.json` (MC 26.1.2). Do not
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


/// `chests/abandoned_mineshaft.json` (26.1.2). Dropped, items emit
/// plain (queued): enchant_randomly on book.
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

/// `chests/ancient_city.json` (26.1.2). Dropped, items emit plain
/// (queued): set_damage on diamond_hoe; enchant_with_levels on
/// diamond_hoe, diamond_leggings, iron_leggings; enchant_randomly on
/// book; set_potion on potion.
pub(crate) const ANCIENT_CITY: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 5,
            rolls_max: 10,
            entries: &[
                counted("enchanted_golden_apple", 1, 1, 2),
                one("music_disc_otherside", 1),
                one("compass", 2),
                counted("sculk_catalyst", 2, 1, 2),
                one("diamond_hoe", 2),
                one("lead", 2),
                one("diamond_horse_armor", 2),
                counted("leather", 2, 1, 5),
                one("music_disc_13", 2),
                one("music_disc_cat", 2),
                one("diamond_leggings", 2),
                one("book", 3),
                counted("sculk", 3, 4, 10),
                counted("sculk_sensor", 3, 1, 3),
                counted("candle", 3, 1, 4),
                counted("amethyst_shard", 3, 1, 15),
                counted("experience_bottle", 3, 1, 3),
                counted("glow_berries", 3, 1, 15),
                one("iron_leggings", 3),
                counted("echo_shard", 4, 1, 3),
                counted("disc_fragment_5", 4, 1, 3),
                counted("potion", 5, 1, 3),
                one("book", 5),
                counted("book", 5, 3, 10),
                counted("bone", 5, 1, 15),
                counted("soul_torch", 5, 1, 15),
                counted("coal", 7, 6, 15),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 75),
                one("ward_armor_trim_smithing_template", 4),
                one("silence_armor_trim_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/ancient_city_ice_box.json` (26.1.2). Dropped, items emit
/// plain (queued): set_stew_effect on suspicious_stew.
pub(crate) const ANCIENT_CITY_ICE_BOX: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 4,
            rolls_max: 10,
            entries: &[
                counted("suspicious_stew", 1, 2, 6),
                counted("golden_carrot", 1, 1, 10),
                counted("baked_potato", 1, 1, 10),
                counted("packed_ice", 2, 2, 6),
                counted("snowball", 4, 2, 6),
            ],
        },
    ],
};

/// `chests/bastion_bridge.json` (26.1.2). Dropped, items emit plain
/// (queued): set_damage on crossbow; enchant_randomly on crossbow,
/// golden_axe, golden_boots, golden_chestplate, golden_helmet,
/// golden_leggings.
pub(crate) const BASTION_BRIDGE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("lodestone", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 2,
            entries: &[
                one("crossbow", 1),
                counted("spectral_arrow", 1, 10, 28),
                counted("gilded_blackstone", 1, 8, 12),
                counted("crying_obsidian", 1, 3, 8),
                one("gold_block", 1),
                counted("gold_ingot", 1, 4, 9),
                counted("iron_ingot", 1, 4, 9),
                one("golden_sword", 1),
                one("golden_chestplate", 1),
                one("golden_helmet", 1),
                one("golden_leggings", 1),
                one("golden_boots", 1),
                one("golden_axe", 1),
            ],
        },
        LootPool {
            rolls_min: 2,
            rolls_max: 4,
            entries: &[
                counted("string", 1, 1, 6),
                counted("leather", 1, 1, 3),
                counted("arrow", 1, 5, 17),
                counted("iron_nugget", 1, 2, 6),
                counted("gold_nugget", 1, 2, 6),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 11),
                one("snout_armor_trim_smithing_template", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 9),
                one("netherite_upgrade_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/bastion_hoglin_stable.json` (26.1.2). Dropped, items emit
/// plain (queued): set_damage on diamond_pickaxe, diamond_shovel;
/// enchant_randomly on diamond_pickaxe, diamond_shovel, golden_axe.
pub(crate) const BASTION_HOGLIN_STABLE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("diamond_shovel", 15),
                one("diamond_pickaxe", 12),
                one("netherite_scrap", 8),
                one("ancient_debris", 12),
                counted("ancient_debris", 5, 2, 2),
                one("saddle", 12),
                counted("gold_block", 16, 2, 4),
                counted("golden_carrot", 10, 8, 17),
                one("golden_apple", 10),
            ],
        },
        LootPool {
            rolls_min: 3,
            rolls_max: 4,
            entries: &[
                one("golden_axe", 1),
                counted("crying_obsidian", 1, 1, 5),
                counted("glowstone", 1, 3, 6),
                counted("gilded_blackstone", 1, 2, 5),
                counted("soul_sand", 1, 2, 7),
                counted("crimson_nylium", 1, 2, 7),
                counted("gold_nugget", 1, 2, 8),
                counted("leather", 1, 1, 3),
                counted("arrow", 1, 5, 17),
                counted("string", 1, 3, 8),
                counted("porkchop", 1, 2, 5),
                counted("cooked_porkchop", 1, 2, 5),
                counted("crimson_fungus", 1, 2, 7),
                counted("crimson_roots", 1, 2, 7),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 11),
                one("snout_armor_trim_smithing_template", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 9),
                one("netherite_upgrade_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/bastion_other.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_randomly on book, crossbow, diamond_pickaxe,
/// golden_axe, golden_boots, iron_sword; set_damage on crossbow,
/// iron_sword.
pub(crate) const BASTION_OTHER: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("diamond_pickaxe", 6),
                one("diamond_shovel", 6),
                one("crossbow", 6),
                one("ancient_debris", 12),
                one("netherite_scrap", 4),
                counted("spectral_arrow", 10, 10, 22),
                one("piglin_banner_pattern", 9),
                one("music_disc_pigstep", 5),
                counted("golden_carrot", 12, 6, 17),
                one("golden_apple", 9),
                one("book", 10),
            ],
        },
        LootPool {
            rolls_min: 2,
            rolls_max: 2,
            entries: &[
                one("iron_sword", 2),
                one("iron_block", 2),
                one("golden_boots", 1),
                one("golden_axe", 1),
                one("gold_block", 2),
                one("crossbow", 1),
                counted("gold_ingot", 2, 1, 6),
                counted("iron_ingot", 2, 1, 6),
                one("golden_sword", 1),
                one("golden_chestplate", 1),
                one("golden_helmet", 1),
                one("golden_leggings", 1),
                one("golden_boots", 1),
                counted("crying_obsidian", 2, 1, 5),
            ],
        },
        LootPool {
            rolls_min: 3,
            rolls_max: 4,
            entries: &[
                counted("gilded_blackstone", 2, 1, 5),
                counted("iron_chain", 1, 2, 10),
                counted("magma_cream", 2, 2, 6),
                counted("bone_block", 1, 3, 6),
                counted("iron_nugget", 1, 2, 8),
                counted("obsidian", 1, 4, 6),
                counted("gold_nugget", 1, 2, 8),
                counted("string", 1, 4, 6),
                counted("arrow", 2, 5, 17),
                one("cooked_porkchop", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 11),
                one("snout_armor_trim_smithing_template", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 9),
                one("netherite_upgrade_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/bastion_treasure.json` (26.1.2). Dropped, items emit plain
/// (queued): set_damage on diamond_boots, diamond_chestplate,
/// diamond_helmet, diamond_leggings, diamond_spear, diamond_sword;
/// enchant_randomly on diamond_boots, diamond_chestplate,
/// diamond_helmet, diamond_leggings, diamond_spear, diamond_sword.
pub(crate) const BASTION_TREASURE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 3,
            rolls_max: 3,
            entries: &[
                one("netherite_ingot", 15),
                one("ancient_debris", 10),
                one("netherite_scrap", 8),
                counted("ancient_debris", 4, 2, 2),
                one("diamond_sword", 6),
                one("diamond_spear", 6),
                one("diamond_chestplate", 6),
                one("diamond_helmet", 6),
                one("diamond_leggings", 6),
                one("diamond_boots", 6),
                one("diamond_sword", 6),
                one("diamond_spear", 6),
                one("diamond_chestplate", 5),
                one("diamond_helmet", 5),
                one("diamond_boots", 5),
                one("diamond_leggings", 5),
                counted("diamond", 5, 2, 6),
                one("enchanted_golden_apple", 2),
            ],
        },
        LootPool {
            rolls_min: 3,
            rolls_max: 4,
            entries: &[
                counted("spectral_arrow", 1, 12, 25),
                counted("gold_block", 1, 2, 5),
                counted("iron_block", 1, 2, 5),
                counted("gold_ingot", 1, 3, 9),
                counted("iron_ingot", 1, 3, 9),
                counted("crying_obsidian", 1, 3, 5),
                counted("quartz", 1, 8, 23),
                counted("gilded_blackstone", 1, 5, 15),
                counted("magma_cream", 1, 3, 8),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 11),
                one("snout_armor_trim_smithing_template", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("netherite_upgrade_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/buried_treasure.json` (26.1.2). Dropped, items emit plain
/// (queued): set_potion (pool).
pub(crate) const BURIED_TREASURE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("heart_of_the_sea", 1),
            ],
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
            entries: &[
                one("potion", 1),
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

/// `chests/desert_pyramid.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_randomly on book.
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

/// `chests/end_city_treasure.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_with_levels on diamond_boots, diamond_chestplate,
/// diamond_helmet, diamond_leggings, diamond_pickaxe, diamond_shovel,
/// diamond_spear, diamond_sword, iron_boots, iron_chestplate,
/// iron_helmet, iron_leggings, iron_pickaxe, iron_shovel, iron_sword.
pub(crate) const END_CITY_TREASURE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 6,
            entries: &[
                counted("diamond", 5, 2, 7),
                counted("iron_ingot", 10, 4, 8),
                counted("gold_ingot", 15, 2, 7),
                counted("emerald", 2, 2, 6),
                counted("beetroot_seeds", 5, 1, 10),
                one("saddle", 3),
                one("copper_horse_armor", 1),
                one("iron_horse_armor", 1),
                one("golden_horse_armor", 1),
                one("diamond_horse_armor", 1),
                one("diamond_sword", 3),
                one("diamond_spear", 3),
                one("diamond_boots", 3),
                one("diamond_chestplate", 3),
                one("diamond_leggings", 3),
                one("diamond_helmet", 3),
                one("diamond_pickaxe", 3),
                one("diamond_shovel", 3),
                one("iron_sword", 3),
                one("iron_boots", 3),
                one("iron_chestplate", 3),
                one("iron_leggings", 3),
                one("iron_helmet", 3),
                one("iron_pickaxe", 3),
                one("iron_shovel", 3),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 14),
                one("spire_armor_trim_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/igloo_chest.json` (26.1.2).
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
            entries: &[
                one("golden_apple", 1),
            ],
        },
    ],
};

/// `chests/jungle_temple.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_with_levels on book.
pub(crate) const JUNGLE_TEMPLE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 6,
            entries: &[
                counted("diamond", 3, 1, 3),
                counted("iron_ingot", 10, 1, 5),
                counted("gold_ingot", 15, 2, 7),
                counted("bamboo", 15, 1, 3),
                counted("emerald", 2, 1, 3),
                counted("bone", 20, 4, 6),
                counted("rotten_flesh", 16, 3, 7),
                counted("leather", 3, 1, 5),
                one("copper_horse_armor", 1),
                one("iron_horse_armor", 1),
                one("golden_horse_armor", 1),
                one("diamond_horse_armor", 1),
                one("book", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 2),
                counted("wild_armor_trim_smithing_template", 1, 2, 2),
            ],
        },
    ],
};

/// `chests/jungle_temple_dispenser.json` (26.1.2).
pub(crate) const JUNGLE_TEMPLE_DISPENSER: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 2,
            entries: &[
                counted("arrow", 30, 2, 7),
            ],
        },
    ],
};

/// `chests/nether_bridge.json` (26.1.2).
pub(crate) const NETHER_BRIDGE: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 4,
            entries: &[
                counted("diamond", 5, 1, 3),
                counted("iron_ingot", 5, 1, 5),
                counted("gold_ingot", 15, 1, 3),
                one("golden_sword", 5),
                one("golden_chestplate", 5),
                one("flint_and_steel", 5),
                counted("nether_wart", 5, 3, 7),
                one("saddle", 10),
                one("golden_horse_armor", 8),
                one("copper_horse_armor", 5),
                one("iron_horse_armor", 5),
                one("diamond_horse_armor", 3),
                counted("obsidian", 2, 2, 4),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 14),
                one("rib_armor_trim_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/pillager_outpost.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_randomly on book; set_instrument (pool).
pub(crate) const PILLAGER_OUTPOST: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 0,
            rolls_max: 1,
            entries: &[
                one("crossbow", 1),
            ],
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
            entries: &[
                counted("dark_oak_log", 1, 2, 3),
            ],
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
            entries: &[
                one("goat_horn", 1),
            ],
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

/// `chests/ruined_portal.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_randomly on golden_axe, golden_boots,
/// golden_chestplate, golden_helmet, golden_hoe, golden_leggings,
/// golden_pickaxe, golden_shovel, golden_sword.
pub(crate) const RUINED_PORTAL: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 4,
            rolls_max: 8,
            entries: &[
                counted("obsidian", 40, 1, 2),
                counted("flint", 40, 1, 4),
                counted("iron_nugget", 40, 9, 18),
                one("flint_and_steel", 40),
                one("fire_charge", 40),
                one("golden_apple", 15),
                counted("gold_nugget", 15, 4, 24),
                one("golden_sword", 15),
                one("golden_axe", 15),
                one("golden_hoe", 15),
                one("golden_shovel", 15),
                one("golden_pickaxe", 15),
                one("golden_boots", 15),
                one("golden_chestplate", 15),
                one("golden_helmet", 15),
                one("golden_leggings", 15),
                counted("glistering_melon_slice", 5, 4, 12),
                one("golden_horse_armor", 5),
                one("light_weighted_pressure_plate", 5),
                counted("golden_carrot", 5, 4, 12),
                one("clock", 5),
                counted("gold_ingot", 5, 2, 8),
                one("bell", 1),
                one("enchanted_golden_apple", 1),
                counted("gold_block", 1, 1, 2),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 1),
                counted("lodestone", 2, 1, 2),
            ],
        },
    ],
};

/// `chests/shipwreck_map.json` (26.1.2). Dropped, items emit plain
/// (queued): exploration_map on map; set_name on map.
pub(crate) const SHIPWRECK_MAP: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("map", 1),
            ],
        },
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

/// `chests/shipwreck_supply.json` (26.1.2). Dropped, items emit plain
/// (queued): set_stew_effect on suspicious_stew; enchant_randomly on
/// leather_boots, leather_chestplate, leather_helmet, leather_leggings.
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

/// `chests/shipwreck_treasure.json` (26.1.2).
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

/// `chests/simple_dungeon.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_randomly on book.
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

/// `chests/spawn_bonus_chest.json` (26.1.2).
pub(crate) const SPAWN_BONUS_CHEST: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("stone_axe", 1),
                one("wooden_axe", 3),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("stone_pickaxe", 1),
                one("wooden_pickaxe", 3),
            ],
        },
        LootPool {
            rolls_min: 3,
            rolls_max: 3,
            entries: &[
                counted("apple", 5, 1, 2),
                counted("bread", 3, 1, 2),
                counted("salmon", 3, 1, 2),
            ],
        },
        LootPool {
            rolls_min: 4,
            rolls_max: 4,
            entries: &[
                counted("stick", 10, 1, 12),
                counted("oak_planks", 10, 1, 12),
                counted("oak_log", 3, 1, 3),
                counted("spruce_log", 3, 1, 3),
                counted("birch_log", 3, 1, 3),
                counted("jungle_log", 3, 1, 3),
                counted("acacia_log", 3, 1, 3),
                counted("dark_oak_log", 3, 1, 3),
                counted("mangrove_log", 3, 1, 3),
            ],
        },
    ],
};

/// `chests/stronghold_corridor.json` (26.1.2). Dropped, items emit
/// plain (queued): enchant_with_levels on book.
pub(crate) const STRONGHOLD_CORRIDOR: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 3,
            entries: &[
                one("ender_pearl", 10),
                counted("diamond", 3, 1, 3),
                counted("iron_ingot", 10, 1, 5),
                counted("gold_ingot", 5, 1, 3),
                counted("redstone", 5, 4, 9),
                counted("bread", 15, 1, 3),
                counted("apple", 15, 1, 3),
                one("iron_pickaxe", 5),
                one("iron_sword", 5),
                one("iron_chestplate", 5),
                one("iron_helmet", 5),
                one("iron_leggings", 5),
                one("iron_boots", 5),
                one("golden_apple", 1),
                counted("leather", 1, 1, 5),
                one("copper_horse_armor", 1),
                one("iron_horse_armor", 1),
                one("golden_horse_armor", 1),
                one("diamond_horse_armor", 1),
                one("music_disc_otherside", 1),
                one("book", 1),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 9),
                one("eye_armor_trim_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/stronghold_crossing.json` (26.1.2). Dropped, items emit
/// plain (queued): enchant_with_levels on book.
pub(crate) const STRONGHOLD_CROSSING: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 4,
            entries: &[
                counted("iron_ingot", 10, 1, 5),
                counted("gold_ingot", 5, 1, 3),
                counted("redstone", 5, 4, 9),
                counted("coal", 10, 3, 8),
                counted("bread", 15, 1, 3),
                counted("apple", 15, 1, 3),
                one("iron_pickaxe", 1),
                one("book", 1),
            ],
        },
    ],
};

/// `chests/stronghold_library.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_with_levels on book.
pub(crate) const STRONGHOLD_LIBRARY: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 10,
            entries: &[
                counted("book", 20, 1, 3),
                counted("paper", 20, 2, 7),
                one("map", 1),
                one("compass", 1),
                one("book", 10),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("eye_armor_trim_smithing_template", 1),
            ],
        },
    ],
};

/// `chests/underwater_ruin_big.json` (26.1.2). Dropped, items emit
/// plain (queued): enchant_randomly on book, fishing_rod;
/// exploration_map on map; set_name on map.
pub(crate) const UNDERWATER_RUIN_BIG: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 8,
            entries: &[
                counted("coal", 10, 1, 4),
                counted("gold_nugget", 10, 1, 3),
                one("emerald", 1),
                one("stone_spear", 2),
                counted("wheat", 10, 2, 3),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("golden_apple", 1),
                one("book", 5),
                one("leather_chestplate", 1),
                one("golden_helmet", 1),
                one("fishing_rod", 5),
                one("map", 10),
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

/// `chests/underwater_ruin_small.json` (26.1.2). Dropped, items emit
/// plain (queued): enchant_randomly on fishing_rod; exploration_map on
/// map; set_name on map.
pub(crate) const UNDERWATER_RUIN_SMALL: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 2,
            rolls_max: 8,
            entries: &[
                counted("coal", 10, 1, 4),
                one("stone_axe", 2),
                one("stone_spear", 2),
                one("rotten_flesh", 5),
                one("emerald", 1),
                counted("wheat", 10, 2, 3),
            ],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("leather_chestplate", 1),
                one("golden_helmet", 1),
                one("fishing_rod", 5),
                one("map", 5),
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

/// `chests/woodland_mansion.json` (26.1.2). Dropped, items emit plain
/// (queued): enchant_randomly on book.
pub(crate) const WOODLAND_MANSION: LootTable = LootTable {
    pools: &[
        LootPool {
            rolls_min: 1,
            rolls_max: 3,
            entries: &[
                one("lead", 20),
                one("golden_apple", 15),
                one("enchanted_golden_apple", 2),
                one("music_disc_13", 15),
                one("music_disc_cat", 15),
                one("chainmail_chestplate", 10),
                one("diamond_hoe", 15),
                one("diamond_chestplate", 5),
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
                counted("resin_clump", 50, 2, 4),
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
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[
                one("", 1),
                one("vex_armor_trim_smithing_template", 1),
            ],
        },
    ],
};

/// Every generated table with its MC data id - the audit/test
/// sweep surface. Referenced by tests; the allow keeps the
/// not-yet-routed tables (end city / bonus chest / dispenser /
/// ominous rewards) compiling warning-free until their
/// structures land.
#[allow(dead_code)]
pub(crate) const ALL_TABLES: &[(&str, &LootTable)] = &[
    ("abandoned_mineshaft", &ABANDONED_MINESHAFT),
    ("ancient_city", &ANCIENT_CITY),
    ("ancient_city_ice_box", &ANCIENT_CITY_ICE_BOX),
    ("bastion_bridge", &BASTION_BRIDGE),
    ("bastion_hoglin_stable", &BASTION_HOGLIN_STABLE),
    ("bastion_other", &BASTION_OTHER),
    ("bastion_treasure", &BASTION_TREASURE),
    ("buried_treasure", &BURIED_TREASURE),
    ("desert_pyramid", &DESERT_PYRAMID),
    ("end_city_treasure", &END_CITY_TREASURE),
    ("igloo_chest", &IGLOO_CHEST),
    ("jungle_temple", &JUNGLE_TEMPLE),
    ("jungle_temple_dispenser", &JUNGLE_TEMPLE_DISPENSER),
    ("nether_bridge", &NETHER_BRIDGE),
    ("pillager_outpost", &PILLAGER_OUTPOST),
    ("ruined_portal", &RUINED_PORTAL),
    ("shipwreck_map", &SHIPWRECK_MAP),
    ("shipwreck_supply", &SHIPWRECK_SUPPLY),
    ("shipwreck_treasure", &SHIPWRECK_TREASURE),
    ("simple_dungeon", &SIMPLE_DUNGEON),
    ("spawn_bonus_chest", &SPAWN_BONUS_CHEST),
    ("stronghold_corridor", &STRONGHOLD_CORRIDOR),
    ("stronghold_crossing", &STRONGHOLD_CROSSING),
    ("stronghold_library", &STRONGHOLD_LIBRARY),
    ("trial_chambers/corridor", &super::tables_gen_trial::TRIAL_CHAMBERS_CORRIDOR),
    ("trial_chambers/entrance", &super::tables_gen_trial::TRIAL_CHAMBERS_ENTRANCE),
    ("trial_chambers/intersection", &super::tables_gen_trial::TRIAL_CHAMBERS_INTERSECTION),
    ("trial_chambers/intersection_barrel", &super::tables_gen_trial::TRIAL_CHAMBERS_INTERSECTION_BARREL),
    ("trial_chambers/reward", &super::tables_gen_trial::TRIAL_CHAMBERS_REWARD),
    ("trial_chambers/reward_common", &super::tables_gen_trial::TRIAL_CHAMBERS_REWARD_COMMON),
    ("trial_chambers/reward_ominous", &super::tables_gen_trial::TRIAL_CHAMBERS_REWARD_OMINOUS),
    ("trial_chambers/reward_ominous_common", &super::tables_gen_trial::TRIAL_CHAMBERS_REWARD_OMINOUS_COMMON),
    ("trial_chambers/reward_ominous_rare", &super::tables_gen_trial::TRIAL_CHAMBERS_REWARD_OMINOUS_RARE),
    ("trial_chambers/reward_ominous_unique", &super::tables_gen_trial::TRIAL_CHAMBERS_REWARD_OMINOUS_UNIQUE),
    ("trial_chambers/reward_rare", &super::tables_gen_trial::TRIAL_CHAMBERS_REWARD_RARE),
    ("trial_chambers/reward_unique", &super::tables_gen_trial::TRIAL_CHAMBERS_REWARD_UNIQUE),
    ("trial_chambers/supply", &super::tables_gen_trial::TRIAL_CHAMBERS_SUPPLY),
    ("underwater_ruin_big", &UNDERWATER_RUIN_BIG),
    ("underwater_ruin_small", &UNDERWATER_RUIN_SMALL),
    ("village/village_armorer", &super::tables_gen_village::VILLAGE_VILLAGE_ARMORER),
    ("village/village_butcher", &super::tables_gen_village::VILLAGE_VILLAGE_BUTCHER),
    ("village/village_cartographer", &super::tables_gen_village::VILLAGE_VILLAGE_CARTOGRAPHER),
    ("village/village_desert_house", &super::tables_gen_village::VILLAGE_VILLAGE_DESERT_HOUSE),
    ("village/village_fisher", &super::tables_gen_village::VILLAGE_VILLAGE_FISHER),
    ("village/village_fletcher", &super::tables_gen_village::VILLAGE_VILLAGE_FLETCHER),
    ("village/village_mason", &super::tables_gen_village::VILLAGE_VILLAGE_MASON),
    ("village/village_plains_house", &super::tables_gen_village::VILLAGE_VILLAGE_PLAINS_HOUSE),
    ("village/village_savanna_house", &super::tables_gen_village::VILLAGE_VILLAGE_SAVANNA_HOUSE),
    ("village/village_shepherd", &super::tables_gen_village::VILLAGE_VILLAGE_SHEPHERD),
    ("village/village_snowy_house", &super::tables_gen_village::VILLAGE_VILLAGE_SNOWY_HOUSE),
    ("village/village_taiga_house", &super::tables_gen_village::VILLAGE_VILLAGE_TAIGA_HOUSE),
    ("village/village_tannery", &super::tables_gen_village::VILLAGE_VILLAGE_TANNERY),
    ("village/village_temple", &super::tables_gen_village::VILLAGE_VILLAGE_TEMPLE),
    ("village/village_toolsmith", &super::tables_gen_village::VILLAGE_VILLAGE_TOOLSMITH),
    ("village/village_weaponsmith", &super::tables_gen_village::VILLAGE_VILLAGE_WEAPONSMITH),
    ("woodland_mansion", &WOODLAND_MANSION),
];
