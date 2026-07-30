//! W471 - max-damage (durability) table, generated 1:1 from the
//! decompiled MC 26.1.2 sources. Regenerate with the generator
//! script rather than editing rows by hand.
//!
//! Inputs:
//! - `net/minecraft/world/item/Items.java` - `Item.Properties`
//!   registrations: direct `.durability(N)`, tool helpers
//!   (`.sword/.pickaxe/.spear` + `ShovelItem`/`AxeItem`/`HoeItem`
//!   constructors) and `.humanoidArmor`/`.wolfArmor`.
//! - `net/minecraft/world/item/ToolMaterial.java` - per-tier tool
//!   durability (WOOD 59, STONE 131, COPPER 190, IRON 250, GOLD 32,
//!   DIAMOND 1561, NETHERITE 2031).
//! - `net/minecraft/world/item/equipment/ArmorMaterials.java` -
//!   armor multipliers (LEATHER 5, COPPER 11, CHAINMAIL 15, IRON 15,
//!   GOLD 7, DIAMOND 33, TURTLE_SCUTE 25, NETHERITE 37,
//!   ARMADILLO_SCUTE 4).
//! - `net/minecraft/world/item/equipment/ArmorType.java` - per-slot
//!   units (`ArmorType.getDurability`: HELMET 11, CHESTPLATE 16,
//!   LEGGINGS 15, BOOTS 13, BODY 16).
//!
//! `horseArmor` / `nautilusArmor` items carry no durability in the
//! source (`setDamageOnHurt(false)`, no `.durability` call) and are
//! deliberately absent. Items the engine registry lacks are
//! harmless extra rows - the id-keyed lookup never reaches them.

/// `MC item name -> max_damage` (the `minecraft:max_damage`
/// component value; wear counts UP from 0 and the item breaks at
/// this threshold). Sorted by name.
pub(crate) const MAX_DAMAGE_TABLE: &[(&str, u16)] = &[
    ("bow", 384),
    ("brush", 64),
    ("carrot_on_a_stick", 25),
    ("chainmail_boots", 195),
    ("chainmail_chestplate", 240),
    ("chainmail_helmet", 165),
    ("chainmail_leggings", 225),
    ("copper_axe", 190),
    ("copper_boots", 143),
    ("copper_chestplate", 176),
    ("copper_helmet", 121),
    ("copper_hoe", 190),
    ("copper_leggings", 165),
    ("copper_pickaxe", 190),
    ("copper_shovel", 190),
    ("copper_spear", 190),
    ("copper_sword", 190),
    ("crossbow", 465),
    ("diamond_axe", 1561),
    ("diamond_boots", 429),
    ("diamond_chestplate", 528),
    ("diamond_helmet", 363),
    ("diamond_hoe", 1561),
    ("diamond_leggings", 495),
    ("diamond_pickaxe", 1561),
    ("diamond_shovel", 1561),
    ("diamond_spear", 1561),
    ("diamond_sword", 1561),
    ("elytra", 432),
    ("fishing_rod", 64),
    ("flint_and_steel", 64),
    ("golden_axe", 32),
    ("golden_boots", 91),
    ("golden_chestplate", 112),
    ("golden_helmet", 77),
    ("golden_hoe", 32),
    ("golden_leggings", 105),
    ("golden_pickaxe", 32),
    ("golden_shovel", 32),
    ("golden_spear", 32),
    ("golden_sword", 32),
    ("iron_axe", 250),
    ("iron_boots", 195),
    ("iron_chestplate", 240),
    ("iron_helmet", 165),
    ("iron_hoe", 250),
    ("iron_leggings", 225),
    ("iron_pickaxe", 250),
    ("iron_shovel", 250),
    ("iron_spear", 250),
    ("iron_sword", 250),
    ("leather_boots", 65),
    ("leather_chestplate", 80),
    ("leather_helmet", 55),
    ("leather_leggings", 75),
    ("mace", 500),
    ("netherite_axe", 2031),
    ("netherite_boots", 481),
    ("netherite_chestplate", 592),
    ("netherite_helmet", 407),
    ("netherite_hoe", 2031),
    ("netherite_leggings", 555),
    ("netherite_pickaxe", 2031),
    ("netherite_shovel", 2031),
    ("netherite_spear", 2031),
    ("netherite_sword", 2031),
    ("shears", 238),
    ("shield", 336),
    ("stone_axe", 131),
    ("stone_hoe", 131),
    ("stone_pickaxe", 131),
    ("stone_shovel", 131),
    ("stone_spear", 131),
    ("stone_sword", 131),
    ("trident", 250),
    ("turtle_helmet", 275),
    ("warped_fungus_on_a_stick", 100),
    ("wolf_armor", 64),
    ("wooden_axe", 59),
    ("wooden_hoe", 59),
    ("wooden_pickaxe", 59),
    ("wooden_shovel", 59),
    ("wooden_spear", 59),
    ("wooden_sword", 59),
];
