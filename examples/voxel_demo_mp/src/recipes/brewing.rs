//! W50.2 - brewing stand recipe table.
//!
//! MC brewing stand: 1 ingredient slot + 1 fuel slot
//! (blaze_powder) + 3 base bottle slots. The ingredient applies
//! its transform to each of the 3 bottles in parallel. Brew
//! takes 400 ticks (20 s) at the canonical TPS.

#![allow(dead_code)]

/// One brewing transform. The engine consumes one `ingredient`
/// item and converts each of the up-to-3 base bottles of type
/// `base` into `output`.
pub(crate) struct BrewingRecipe {
    pub(crate) ingredient: u16,
    pub(crate) base: u16,
    pub(crate) output: u16,
    pub(crate) name: &'static str,
    /// Ticks of brewing required (MC default 400).
    pub(crate) time_ticks: u32,
}

/// Demo brewing table. Item IDs are demo-specific; the relevant
/// constants live in `block::ids` once the brewing items get
/// real entries. For now we reference numeric placeholders so the
/// matcher / tick can be wired without blocking on item registry
/// expansion.
pub(crate) const BREWING_RECIPES: &[BrewingRecipe] = &[
    // Water bottle + nether wart -> awkward potion.
    BrewingRecipe {
        ingredient: 700, // nether_wart_item placeholder
        base: 701, // water_bottle placeholder
        output: 702, // awkward_potion placeholder
        name: "Awkward potion",
        time_ticks: 400,
    },
    // Awkward potion + glistering melon -> healing potion.
    BrewingRecipe {
        ingredient: 703,
        base: 702,
        output: 704,
        name: "Healing potion",
        time_ticks: 400,
    },
    // Awkward potion + sugar -> swiftness potion.
    BrewingRecipe {
        ingredient: 705,
        base: 702,
        output: 706,
        name: "Swiftness potion",
        time_ticks: 400,
    },
    // Awkward potion + spider eye -> poison potion.
    BrewingRecipe {
        ingredient: 707,
        base: 702,
        output: 708,
        name: "Poison potion",
        time_ticks: 400,
    },
    // Awkward potion + ghast tear -> regeneration potion.
    BrewingRecipe {
        ingredient: 709,
        base: 702,
        output: 710,
        name: "Regeneration potion",
        time_ticks: 400,
    },
    // Awkward potion + blaze powder -> strength potion.
    BrewingRecipe {
        ingredient: 711,
        base: 702,
        output: 712,
        name: "Strength potion",
        time_ticks: 400,
    },
    // Healing -> harming via fermented spider eye (corruption).
    BrewingRecipe {
        ingredient: 713,
        base: 704,
        output: 714,
        name: "Harming potion",
        time_ticks: 400,
    },
    // Swiftness -> slowness via fermented spider eye.
    BrewingRecipe {
        ingredient: 713,
        base: 706,
        output: 715,
        name: "Slowness potion",
        time_ticks: 400,
    },
];

/// Look up a brewing transform for an (ingredient, base) pair.
pub(crate) fn brewing_for(ingredient: u16, base: u16) -> Option<&'static BrewingRecipe> {
    BREWING_RECIPES
        .iter()
        .find(|r| r.ingredient == ingredient && r.base == base)
}
