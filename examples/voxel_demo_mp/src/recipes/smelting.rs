//! W50.2 - smelting (furnace) recipe table.
//!
//! Furnaces in MC consume fuel + an input item over `time_ticks`
//! and produce `output`. Standard MC tick budget is 200 ticks
//! per item (10 seconds at 20 TPS). The engine ticks at the same
//! rate, so we keep that number.

#![allow(dead_code)]

/// One smelting recipe.
pub(crate) struct SmeltingRecipe {
    pub(crate) input: u16,
    pub(crate) output: (u16, u8),
    /// Player-facing label for the recipe book / tooltip.
    pub(crate) name: &'static str,
    /// Ticks of furnace burn time required to finish the smelt.
    pub(crate) time_ticks: u32,
    /// XP awarded when output is removed from the furnace; the
    /// demo doesn't track XP yet, but we keep the field so the
    /// table stays MC-shape from day one.
    pub(crate) xp: f32,
}

/// XP per smelt operation roughly matches MC. Demo uses 200 tick
/// default (10 s).
const DEFAULT_TIME: u32 = 200;

pub(crate) const SMELTING_RECIPES: &[SmeltingRecipe] = &[
    // ---- Ores -> ingots (canonical MC results). ----
    SmeltingRecipe {
        input: 287, // raw_iron
        output: (14, 1), // iron ingot
        name: "Iron ingot",
        time_ticks: DEFAULT_TIME,
        xp: 0.7,
    },
    SmeltingRecipe {
        input: 289, // raw_gold
        output: (318, 1), // gold ingot
        name: "Gold ingot",
        time_ticks: DEFAULT_TIME,
        xp: 1.0,
    },
    SmeltingRecipe {
        input: 288, // raw_copper
        output: (319, 1), // copper ingot (id from block::ids)
        name: "Copper ingot",
        time_ticks: DEFAULT_TIME,
        xp: 0.7,
    },
    // ---- Raw blocks (cobble -> stone, sand -> glass). ----
    SmeltingRecipe {
        input: 8, // cobble
        output: (1, 1), // smooth stone
        name: "Stone",
        time_ticks: DEFAULT_TIME,
        xp: 0.1,
    },
    SmeltingRecipe {
        input: 4, // sand
        output: (9, 1), // glass
        name: "Glass",
        time_ticks: DEFAULT_TIME,
        xp: 0.1,
    },
    SmeltingRecipe {
        input: 2, // dirt-ish placeholder for clay
        output: (86, 1), // bricks
        name: "Brick",
        time_ticks: DEFAULT_TIME,
        xp: 0.3,
    },
    // ---- Food. ----
    SmeltingRecipe {
        input: 493, // wheat (stand-in for raw beef/pork in demo)
        output: (164, 1), // cooked_beef (id 164 in block::ids)
        name: "Cooked beef",
        time_ticks: DEFAULT_TIME,
        xp: 0.35,
    },
    // ---- Wood -> charcoal. ----
    SmeltingRecipe {
        input: 5, // oak log
        output: (13, 1), // charcoal/coal
        name: "Charcoal",
        time_ticks: DEFAULT_TIME,
        xp: 0.15,
    },
    SmeltingRecipe {
        input: 88, // birch log
        output: (13, 1),
        name: "Charcoal (birch)",
        time_ticks: DEFAULT_TIME,
        xp: 0.15,
    },
    SmeltingRecipe {
        input: 127, // acacia log
        output: (13, 1),
        name: "Charcoal (acacia)",
        time_ticks: DEFAULT_TIME,
        xp: 0.15,
    },
];

/// Fuel value (in ticks of burn time) for an item used in a
/// furnace fuel slot. Returns 0 for non-fuel items.
pub(crate) fn fuel_burn_ticks(item: u16) -> u32 {
    match item {
        13 => 1600, // coal / charcoal
        5 | 88 | 127 | 130 | 133 | 136 | 233 | 237 => 300, // logs
        7 | 89 | 128 | 131 | 134 | 137 | 234 | 238 => 300, // planks
        302 => 100, // stick (single)
        595 => 300, // ladder
        // Lava bucket would be huge in MC (20000); skip since the
        // demo's bucket item is BUCKET_EMPTY_ID + filled variants.
        _ => 0,
    }
}

/// Look up a smelting recipe by input item id.
pub(crate) fn smelting_for(input: u16) -> Option<&'static SmeltingRecipe> {
    SMELTING_RECIPES.iter().find(|r| r.input == input)
}
