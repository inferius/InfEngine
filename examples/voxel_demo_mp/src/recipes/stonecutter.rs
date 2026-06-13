//! W171 - stonecutter cut recipes.
//!
//! The stonecutter is a 1-input -> pick-1-of-N-outputs station (MC
//! `StonecutterMenu`). Unlike the 3x3 crafting grid it's a flat list:
//! one input block yields several "cut" variants, and the player
//! clicks the one they want. Most cuts are 1:1; slabs yield 2 (MC
//! parity - the stonecutter is more material-efficient than the grid).
//!
//! Pure data + lookup so it unit-tests without any UI / world state.

/// One stonecutter cut: `count` of `output` from a single `input`.
pub(crate) struct CutRecipe {
    pub(crate) input: u16,
    pub(crate) output: u16,
    pub(crate) count: u8,
    pub(crate) name: &'static str,
}

/// The cut table. Ids are the engine block ids (see `block::ids`).
/// New cuts = one row; the panel reads them through `cuts_for`.
pub(crate) const STONECUTTER_RECIPES: &[CutRecipe] = &[
    // stone (1) ->
    CutRecipe { input: 1, output: 73, count: 1, name: "Stone Stairs" },
    CutRecipe { input: 1, output: 69, count: 2, name: "Stone Slab" },
    CutRecipe { input: 1, output: 110, count: 1, name: "Stone Bricks" },
    CutRecipe { input: 1, output: 113, count: 1, name: "Chiseled Stone Bricks" },
    // cobblestone (8) ->
    CutRecipe { input: 8, output: 74, count: 1, name: "Cobblestone Stairs" },
    CutRecipe { input: 8, output: 70, count: 2, name: "Cobblestone Slab" },
    CutRecipe { input: 8, output: 82, count: 1, name: "Cobblestone Wall" },
    // stone_bricks (110) ->
    CutRecipe { input: 110, output: 528, count: 1, name: "Stone Brick Stairs" },
    CutRecipe { input: 110, output: 514, count: 2, name: "Stone Brick Slab" },
    CutRecipe { input: 110, output: 539, count: 1, name: "Stone Brick Wall" },
    CutRecipe { input: 110, output: 113, count: 1, name: "Chiseled Stone Bricks" },
    // sandstone (26) ->
    CutRecipe { input: 26, output: 525, count: 1, name: "Sandstone Stairs" },
    CutRecipe { input: 26, output: 511, count: 2, name: "Sandstone Slab" },
];

/// Every cut available for `input`, in table order. Empty when the
/// input isn't a stonecutter material.
pub(crate) fn cuts_for(input: u16) -> Vec<&'static CutRecipe> {
    STONECUTTER_RECIPES.iter().filter(|r| r.input == input).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stone_yields_its_four_cuts() {
        let cuts = cuts_for(1);
        assert_eq!(cuts.len(), 4, "stone -> stairs/slab/bricks/chiseled");
        // The slab cut yields 2 (MC efficiency); the rest yield 1.
        let slab = cuts.iter().find(|r| r.output == 69).expect("slab cut");
        assert_eq!(slab.count, 2);
        assert!(cuts.iter().all(|r| r.input == 1));
    }

    #[test]
    fn non_material_has_no_cuts() {
        assert!(cuts_for(7).is_empty()); // oak planks aren't cuttable
        assert!(cuts_for(0).is_empty()); // air
    }

    #[test]
    fn every_cut_output_differs_from_input() {
        for r in STONECUTTER_RECIPES {
            assert_ne!(r.input, r.output, "{} cuts to itself", r.name);
            assert!(r.count >= 1);
        }
    }
}
