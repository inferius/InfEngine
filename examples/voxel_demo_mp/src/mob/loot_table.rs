//! W474 - per-kind entity loot, GENERATED from
//! `data/minecraft/loot_table/entities/<kind>.json` (MC 26.1.2).
//! Do not hand-edit table rows; regenerate instead.
//!
//! Reuses the chest-loot interpreter (`crate::loot`): each kind gets
//! a BASE table (unconditional pools) plus an optional PLAYER table
//! (pools gated on the MC `killed_by_player` condition - rolled only
//! when the kill credits a player). Sheep wool and the magma-cream
//! pool carry runtime gates (`sheared` / slime size) and live in
//! dedicated consts the roller applies by hand.
//!
//! Unported table features (queued, listed per kind in the W474
//! report): `looting` count bonuses, on-fire `furnace_smelt`,
//! `random_chance_with_enchanted_bonus` rare-drop pools (zombie /
//! husk / drowned / zombie_villager / zombified_piglin iron+carrot+
//! potato, rabbit_foot, wither skull, guardian/elder fishing-table
//! ref, ghast music_disc_tears, creeper discs), `set_potion` on the
//! stray's tipped arrow. Item names our registry lacks skip with the
//! once-per-name log in `crate::loot` (rabbit, rabbit_hide,
//! phantom_membrane, glass_bottle, totem_of_undying,
//! tide_armor_trim_smithing_template).

use crate::loot::{LootEntry, LootPool, LootTable};

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

const EMPTY: LootTable = LootTable { pools: &[] };

/// `entities/sheep.json` pool 1: 1-2 mutton, always.
pub(crate) const SHEEP_BASE: LootTable = LootTable {
    pools: &[LootPool {
        rolls_min: 1,
        rolls_max: 1,
        entries: &[counted("mutton", 1, 1, 2)],
    }],
};

/// `entities/sheep.json` pool 2: an alternatives chain over the 16
/// `entities/sheep/<color>` refs, each 1 wool of the mob's color,
/// gated `sheared: false`. The engine has one wool id, so every
/// color ref collapses to it (wool-color parity queued); the sheared
/// gate is applied by `mob_loot`, not the interpreter.
pub(crate) const SHEEP_WOOL: LootTable = LootTable {
    pools: &[LootPool {
        rolls_min: 1,
        rolls_max: 1,
        entries: &[one("wool", 1)],
    }],
};

/// `loot_table/shearing/sheep/<color>.json`: uniform 1-3 rolls of one
/// wool. Used by the shears interaction, not the death drop.
pub(crate) const SHEEP_SHEARING: LootTable = LootTable {
    pools: &[LootPool {
        rolls_min: 1,
        rolls_max: 3,
        entries: &[one("wool", 1)],
    }],
};

/// `entities/magma_cube.json`: magma_cream uniform(-2..1) (clamped:
/// P(1 cream) = 1/4, folded into a weighted blank), gated NOT size 1
/// so `mob_loot` rolls this only for size > 1 cubes. The froglight
/// entries (killed-by-frog) are queued with the other unsupported
/// conditions.
pub(crate) const MAGMA_CUBE_SIZED: LootTable = LootTable {
    pools: &[LootPool {
        rolls_min: 1,
        rolls_max: 1,
        entries: &[one("magma_cream", 1), one("", 3)],
    }],
};

/// `entities/zombie.json` - unconditional pools.
pub(crate) const ZOMBIE: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("rotten_flesh", 1, 0, 2)] },
    ],
};

/// `entities/chicken.json` - unconditional pools.
pub(crate) const CHICKEN: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("feather", 1, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("chicken", 1)] },
    ],
};

/// `entities/pig.json` - unconditional pools.
pub(crate) const PIG: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("porkchop", 1, 1, 3)] },
    ],
};

/// `entities/cow.json` - unconditional pools.
pub(crate) const COW: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("leather", 1, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("beef", 1, 1, 3)] },
    ],
};

/// `entities/skeleton.json` - unconditional pools.
pub(crate) const SKELETON: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("arrow", 1, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("bone", 1, 0, 2)] },
    ],
};

/// `entities/spider.json` - unconditional pools.
pub(crate) const SPIDER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("string", 1, 0, 2)] },
    ],
};

/// `entities/spider.json` - killed_by_player pools.
pub(crate) const SPIDER_PLAYER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("spider_eye", 1), one("", 2)] },
    ],
};

/// `entities/polar_bear.json` - unconditional pools.
pub(crate) const POLAR_BEAR: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("cod", 3, 0, 2), counted("salmon", 1, 0, 2)] },
    ],
};

/// `entities/rabbit.json` - unconditional pools.
pub(crate) const RABBIT: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("rabbit_hide", 1, 0, 1)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("rabbit", 1)] },
    ],
};

/// `entities/zombified_piglin.json` - unconditional pools.
pub(crate) const ZOMBIFIED_PIGLIN: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("rotten_flesh", 1, 0, 1)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("gold_nugget", 1, 0, 1)] },
    ],
};

/// `entities/cat.json` - unconditional pools.
pub(crate) const CAT: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("string", 1, 0, 2)] },
    ],
};

/// `entities/iron_golem.json` - unconditional pools.
pub(crate) const IRON_GOLEM: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("poppy", 1, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("iron_ingot", 1, 3, 5)] },
    ],
};

/// `entities/horse.json` - unconditional pools.
pub(crate) const HORSE: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("leather", 1, 0, 2)] },
    ],
};

/// `entities/ghast.json` - unconditional pools.
pub(crate) const GHAST: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("ghast_tear", 1, 0, 1)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("gunpowder", 1, 0, 2)] },
    ],
};

/// `entities/zombie_villager.json` - unconditional pools.
pub(crate) const ZOMBIE_VILLAGER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("rotten_flesh", 1, 0, 2)] },
    ],
};

/// `entities/creeper.json` - unconditional pools.
pub(crate) const CREEPER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("gunpowder", 1, 0, 2)] },
    ],
};

/// `entities/donkey.json` - unconditional pools.
pub(crate) const DONKEY: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("leather", 1, 0, 2)] },
    ],
};

/// `entities/witch.json` - unconditional pools.
pub(crate) const WITCH: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 3, entries: &[counted("glowstone_dust", 1, 0, 2), counted("sugar", 1, 0, 2), counted("spider_eye", 1, 0, 2), counted("glass_bottle", 1, 0, 2), counted("gunpowder", 1, 0, 2), counted("stick", 2, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("redstone", 1, 4, 8)] },
    ],
};

/// `entities/drowned.json` - unconditional pools.
pub(crate) const DROWNED: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("rotten_flesh", 1, 0, 2)] },
    ],
};

/// `entities/husk.json` - unconditional pools.
pub(crate) const HUSK: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("rotten_flesh", 1, 0, 2)] },
    ],
};

/// `entities/stray.json` - unconditional pools.
pub(crate) const STRAY: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("arrow", 1, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("bone", 1, 0, 2)] },
    ],
};

/// `entities/stray.json` - killed_by_player pools.
pub(crate) const STRAY_PLAYER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("tipped_arrow", 1, 0, 1)] },
    ],
};

/// `entities/vindicator.json` - killed_by_player pools.
pub(crate) const VINDICATOR_PLAYER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("emerald", 1, 0, 1)] },
    ],
};

/// `entities/evoker.json` - unconditional pools.
pub(crate) const EVOKER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("totem_of_undying", 1)] },
    ],
};

/// `entities/evoker.json` - killed_by_player pools.
pub(crate) const EVOKER_PLAYER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("emerald", 1, 0, 1)] },
    ],
};

/// `entities/blaze.json` - killed_by_player pools.
pub(crate) const BLAZE_PLAYER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("blaze_rod", 1, 0, 1)] },
    ],
};

/// `entities/wither_skeleton.json` - unconditional pools.
pub(crate) const WITHER_SKELETON: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("coal", 1), one("", 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("bone", 1, 0, 2)] },
    ],
};

/// `entities/guardian.json` - unconditional pools.
pub(crate) const GUARDIAN: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("prismarine_shard", 1, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("cod", 2), one("prismarine_crystals", 2), one("", 1)] },
    ],
};

/// `entities/elder_guardian.json` - unconditional pools.
pub(crate) const ELDER_GUARDIAN: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("prismarine_shard", 1, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("cod", 3), one("prismarine_crystals", 2), one("", 1)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("", 4), one("tide_armor_trim_smithing_template", 1)] },
    ],
};

/// `entities/elder_guardian.json` - killed_by_player pools.
pub(crate) const ELDER_GUARDIAN_PLAYER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("wet_sponge", 1)] },
    ],
};

/// `entities/enderman.json` - unconditional pools.
pub(crate) const ENDERMAN: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("ender_pearl", 1, 0, 1)] },
    ],
};

/// `entities/cave_spider.json` - unconditional pools.
pub(crate) const CAVE_SPIDER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("string", 1, 0, 2)] },
    ],
};

/// `entities/cave_spider.json` - killed_by_player pools.
pub(crate) const CAVE_SPIDER_PLAYER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("spider_eye", 1), one("", 2)] },
    ],
};

/// `entities/phantom.json` - killed_by_player pools.
pub(crate) const PHANTOM_PLAYER: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("phantom_membrane", 1, 0, 1)] },
    ],
};

/// `entities/mooshroom.json` - unconditional pools.
pub(crate) const MOOSHROOM: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("leather", 1, 0, 2)] },
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("beef", 1, 1, 3)] },
    ],
};

/// `entities/squid.json` - unconditional pools.
pub(crate) const SQUID: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("ink_sac", 1, 1, 3)] },
    ],
};

/// `entities/glow_squid.json` - unconditional pools.
pub(crate) const GLOW_SQUID: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("glow_ink_sac", 1, 1, 3)] },
    ],
};

/// `entities/skeleton_horse.json` - unconditional pools.
pub(crate) const SKELETON_HORSE: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("bone", 1, 0, 2)] },
    ],
};

/// `entities/mule.json` - unconditional pools.
pub(crate) const MULE: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("leather", 1, 0, 2)] },
    ],
};

/// `entities/parrot.json` - unconditional pools.
pub(crate) const PARROT: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("feather", 1, 1, 2)] },
    ],
};

/// `entities/turtle.json` - unconditional pools.
pub(crate) const TURTLE: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[counted("seagrass", 3, 0, 2)] },
    ],
};

/// `entities/panda.json` - unconditional pools.
pub(crate) const PANDA: LootTable = LootTable {
    pools: &[
        LootPool { rolls_min: 1, rolls_max: 1, entries: &[one("bamboo", 1)] },
    ],
};

/// Base + player-gated tables for a kind byte. Every kind resolves;
/// kinds whose MC table is empty (wolf!) or missing get `EMPTY` and
/// drop nothing. Unknown bytes collapse to Sheep handling upstream
/// (`mob_loot` special-cases sheep before calling this).
pub(crate) fn entity_tables(
    kind: u8,
) -> (&'static LootTable, &'static LootTable) {
    use crate::mob::kinds::*;
    match kind {
        k if k == KIND_ZOMBIE => (&ZOMBIE, &EMPTY),
        k if k == KIND_CHICKEN => (&CHICKEN, &EMPTY),
        k if k == KIND_PIG => (&PIG, &EMPTY),
        k if k == KIND_COW => (&COW, &EMPTY),
        k if k == KIND_SKELETON => (&SKELETON, &EMPTY),
        k if k == KIND_SPIDER => (&SPIDER, &SPIDER_PLAYER),
        k if k == KIND_POLAR_BEAR => (&POLAR_BEAR, &EMPTY),
        k if k == KIND_RABBIT => (&RABBIT, &EMPTY),
        k if k == KIND_ZOMBIFIED_PIGLIN => (&ZOMBIFIED_PIGLIN, &EMPTY),
        k if k == KIND_CAT => (&CAT, &EMPTY),
        k if k == KIND_IRON_GOLEM => (&IRON_GOLEM, &EMPTY),
        k if k == KIND_HORSE => (&HORSE, &EMPTY),
        k if k == KIND_GHAST => (&GHAST, &EMPTY),
        k if k == KIND_ZOMBIE_VILLAGER => (&ZOMBIE_VILLAGER, &EMPTY),
        k if k == KIND_CREEPER => (&CREEPER, &EMPTY),
        k if k == KIND_DONKEY => (&DONKEY, &EMPTY),
        k if k == KIND_WITCH => (&WITCH, &EMPTY),
        k if k == KIND_DROWNED => (&DROWNED, &EMPTY),
        k if k == KIND_HUSK => (&HUSK, &EMPTY),
        k if k == KIND_STRAY => (&STRAY, &STRAY_PLAYER),
        k if k == KIND_VINDICATOR => (&EMPTY, &VINDICATOR_PLAYER),
        k if k == KIND_EVOKER => (&EVOKER, &EVOKER_PLAYER),
        k if k == KIND_BLAZE => (&EMPTY, &BLAZE_PLAYER),
        k if k == KIND_WITHER_SKELETON => (&WITHER_SKELETON, &EMPTY),
        k if k == KIND_GUARDIAN => (&GUARDIAN, &EMPTY),
        k if k == KIND_ELDER_GUARDIAN => (&ELDER_GUARDIAN, &ELDER_GUARDIAN_PLAYER),
        k if k == KIND_ENDERMAN => (&ENDERMAN, &EMPTY),
        k if k == KIND_CAVE_SPIDER => (&CAVE_SPIDER, &CAVE_SPIDER_PLAYER),
        k if k == KIND_PHANTOM => (&EMPTY, &PHANTOM_PLAYER),
        k if k == KIND_MOOSHROOM => (&MOOSHROOM, &EMPTY),
        k if k == KIND_SQUID => (&SQUID, &EMPTY),
        k if k == KIND_GLOW_SQUID => (&GLOW_SQUID, &EMPTY),
        k if k == KIND_SKELETON_HORSE => (&SKELETON_HORSE, &EMPTY),
        k if k == KIND_MULE => (&MULE, &EMPTY),
        k if k == KIND_PARROT => (&PARROT, &EMPTY),
        k if k == KIND_TURTLE => (&TURTLE, &EMPTY),
        k if k == KIND_PANDA => (&PANDA, &EMPTY),
        _ => (&EMPTY, &EMPTY),
    }
}
