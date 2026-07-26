//! W410 - smithing-table transform recipes.
//!
//! MC `SmithingMenu` + the `smithing_transform` recipe type: three
//! inputs (template / base / addition) deterministically map to one
//! output. 26.1.2 ships the netherite upgrades (diamond gear +
//! netherite_upgrade_smithing_template + netherite ingot); the
//! armor rows join when armor items land, and `smithing_trim`
//! stays out until armor rendering exists.
//!
//! Pure data + lookup so it unit-tests without UI / world state.

/// One transform: `template` + `base` + `addition` -> `output`.
pub(crate) struct SmithingRecipe {
    pub(crate) template: u16,
    pub(crate) base: u16,
    pub(crate) addition: u16,
    pub(crate) output: u16,
    pub(crate) name: &'static str,
}

/// Item ids from `item::ITEM_TYPES` (933 = netherite upgrade
/// template, 279 = netherite ingot; 155/157/159/161/637 = the
/// diamond tool row, 274..278 = the netherite row).
pub(crate) const SMITHING_RECIPES: &[SmithingRecipe] = &[
    SmithingRecipe { template: 933, base: 155, addition: 279, output: 274, name: "Netherite Pickaxe" },
    SmithingRecipe { template: 933, base: 157, addition: 279, output: 275, name: "Netherite Sword" },
    SmithingRecipe { template: 933, base: 159, addition: 279, output: 276, name: "Netherite Shovel" },
    SmithingRecipe { template: 933, base: 161, addition: 279, output: 277, name: "Netherite Axe" },
    SmithingRecipe { template: 933, base: 637, addition: 279, output: 278, name: "Netherite Hoe" },
];

/// Deterministic result for the current slot contents, or `None`
/// when the triple matches no recipe.
pub(crate) fn smith_result(
    template: Option<u16>,
    base: Option<u16>,
    addition: Option<u16>,
) -> Option<&'static SmithingRecipe> {
    let (t, b, a) = (template?, base?, addition?);
    SMITHING_RECIPES
        .iter()
        .find(|r| r.template == t && r.base == b && r.addition == a)
}

/// Is this item a smithing template (routes to the template slot)?
pub(crate) fn is_smithing_template(id: u16) -> bool {
    crate::item::item_type(id)
        .is_some_and(|it| it.name.ends_with("_smithing_template"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diamond_pickaxe_upgrades() {
        let r = smith_result(Some(933), Some(155), Some(279)).unwrap();
        assert_eq!(r.output, 274);
    }

    #[test]
    fn missing_template_no_result() {
        assert!(smith_result(None, Some(155), Some(279)).is_none());
        // A trim template is a valid TEMPLATE item but matches no
        // transform recipe (trims need armor rendering first).
        assert!(smith_result(Some(934), Some(155), Some(279)).is_none());
    }

    #[test]
    fn wrong_addition_no_result() {
        assert!(smith_result(Some(933), Some(155), Some(280)).is_none());
    }

    #[test]
    fn template_predicate_matches_all_template_items() {
        assert!(is_smithing_template(933));
        assert!(is_smithing_template(934));
        assert!(!is_smithing_template(279));
        assert!(!is_smithing_template(155));
    }
}
