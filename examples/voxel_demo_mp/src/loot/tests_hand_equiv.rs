//! W477: hand-vs-generated snapshot equivalence.
//!
//! The full 56-table `tables_gen*` set is GENERATED from the MC
//! 26.1.2 chest jsons. During the wave every one of the 28 audited
//! W242 hand transcriptions was diffed against its generated
//! counterpart via `PartialEq` - all 28 matched, then the hand
//! tables were deleted. These five stay behind as literal snapshots
//! (covering uniform + constant rolls, `set_count` ranges, the
//! `minecraft:empty` no-drop slots and multi-pool layouts) so a
//! generator regression can never silently rewrite the semantics.

use super::{LootPool, LootTable, counted, one};
use super::{tables_gen as g};

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
            entries: &[one("", 11), one("snout_armor_trim_smithing_template", 1)],
        },
        LootPool {
            rolls_min: 1,
            rolls_max: 1,
            entries: &[one("", 9), one("netherite_upgrade_smithing_template", 1)],
        },
    ],
};

#[test]
fn generated_tables_match_the_hand_snapshots() {
    let pairs: &[(&str, &LootTable, &LootTable)] = &[
        ("SIMPLE_DUNGEON", &SIMPLE_DUNGEON, &g::SIMPLE_DUNGEON),
        ("ABANDONED_MINESHAFT", &ABANDONED_MINESHAFT, &g::ABANDONED_MINESHAFT),
        ("DESERT_PYRAMID", &DESERT_PYRAMID, &g::DESERT_PYRAMID),
        ("SHIPWRECK_TREASURE", &SHIPWRECK_TREASURE, &g::SHIPWRECK_TREASURE),
        ("BASTION_HOGLIN_STABLE", &BASTION_HOGLIN_STABLE, &g::BASTION_HOGLIN_STABLE),
    ];
    for (name, hand, generated) in pairs {
        assert_eq!(hand, generated, "table {name} diverged");
    }
}
