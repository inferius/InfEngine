//! W171 - stonecutter cut recipes (W478: GENERATED from MC data).
//!
//! The stonecutter is a 1-input -> pick-1-of-N-outputs station (MC
//! `StonecutterMenu`). Unlike the 3x3 crafting grid it's a flat list:
//! one input block yields several "cut" variants, and the player
//! clicks the one they want. Counts come straight from the MC
//! `minecraft:stonecutting` jsons (slabs yield 2, everything else
//! mostly 1 - the stonecutter is more material-efficient than the
//! grid).
//!
//! W478: the hand table is gone - `stonecutting_table.rs` carries
//! every resolvable MC 26.1.2 stonecutting recipe; this module keeps
//! the row type + the `cuts_for` lookup the panel reads.

/// One stonecutter cut: `count` of `output` from a single `input`.
pub(crate) struct CutRecipe {
    pub(crate) input: u16,
    pub(crate) output: u16,
    pub(crate) count: u8,
    pub(crate) name: &'static str,
}

pub(crate) use super::stonecutting_table::STONECUTTER_RECIPES;

/// Every cut available for `input`, in table order. Empty when the
/// input isn't a stonecutter material.
pub(crate) fn cuts_for(input: u16) -> Vec<&'static CutRecipe> {
    STONECUTTER_RECIPES.iter().filter(|r| r.input == input).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stone_yields_the_mc_cut_family() {
        // MC 26.1.2 stone stonecutting: cobblestone + slab/stairs/
        // wall families + the stone-brick line (11 cuts).
        let cuts = cuts_for(1);
        assert_eq!(cuts.len(), 11, "stone cut count");
        let outputs: Vec<u16> = cuts.iter().map(|r| r.output).collect();
        assert!(outputs.contains(&110), "stone -> stone_bricks");
        assert!(outputs.contains(&113), "stone -> chiseled_stone_bricks");
        assert!(outputs.contains(&8), "stone -> cobblestone (26.1.2 cut)");
        // Slab cuts yield 2 per input (MC efficiency).
        let slab = cuts.iter().find(|r| r.output == 69).expect("stone slab");
        assert_eq!(slab.count, 2);
        assert!(cuts.iter().all(|r| r.input == 1));
    }

    #[test]
    fn cobblestone_family_comes_from_mc_data() {
        let cuts = cuts_for(8);
        let outputs: Vec<u16> = cuts.iter().map(|r| r.output).collect();
        assert!(outputs.contains(&74), "cobblestone stairs");
        assert!(outputs.contains(&82), "cobblestone wall");
        let slab = cuts.iter().find(|r| r.output == 70).expect("cobble slab");
        assert_eq!(slab.count, 2);
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

    #[test]
    fn table_is_sorted_by_input_id() {
        for w in STONECUTTER_RECIPES.windows(2) {
            assert!(w[0].input <= w[1].input, "{} out of order", w[1].name);
        }
    }
}
