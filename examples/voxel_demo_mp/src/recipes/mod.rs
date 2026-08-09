//! Crafting / smelting / brewing recipe tables.
//!
//! - `crafting_table` (W478, GENERATED): every MC 26.1.2
//!   `crafting_shaped` + `crafting_shapeless` recipe that resolves in
//!   the engine item registry. Shaped cells and shapeless ingredients
//!   are per-slot acceptable-id SETS (`Ingredient`) so a `#planks`
//!   tag slot accepts any plank member without exploding into
//!   per-member recipes.
//! - `matcher` (W478): the ported MC matching semantics -
//!   `ShapedRecipePattern.matches` (bbox trim + dims gate + mirror +
//!   empty-cell strictness) and `ShapelessRecipe.matches` (exact
//!   multiset via bipartite assignment).
//! - `book` (W478): unified recipe list + the 2x2-capable subset the
//!   recipe book / `vx.recipe` auto-fill index into, plus the shared
//!   craft-grid fill helper.
//! - `stonecutter` + `stonecutting_table` (W478, GENERATED): MC
//!   stonecutting cuts.
//! - `smelting` + `smelting_table` (W476, GENERATED): furnace rows.
//! - `brewing`: brewing stand ingredient + base -> potion (W50.2).
//! - `smithing`: W410 hand table (regen out of scope for W478).
//!
//! The W47.27 hand-rolled shapeless table (`RECIPES`) and the W50.2
//! demo shaped table (`shaped.rs`) are GONE - the generated set
//! supersedes them, deliberately correcting the old stand-ins
//! (chest = 8-plank ring, hay bale = 9 wheat, compass = 4 iron +
//! redstone, bucket consumes iron ingots not ore, ...).

pub(crate) mod book;
pub(crate) mod brewing;
pub(crate) mod crafting_table;
pub(crate) mod matcher;
pub(crate) mod smelting;
// W476: GENERATED smelting + fuel rows (MC 26.1.2 recipe jsons +
// FuelValues.java). `smelting` re-exports the tables.
pub(crate) mod smelting_table;
pub(crate) mod smithing;
pub(crate) mod stonecutter;
pub(crate) mod stonecutting_table;

/// One ingredient slot: the set of item ids the slot accepts (a tag
/// or id-list resolved against the engine registry). In a shaped
/// pattern cell the EMPTY slice means the cell must stay empty.
pub(crate) type Ingredient = &'static [u16];

/// MC `crafting_shaped` recipe: a `w x h` pattern of ingredient
/// cells (row-major, pre-shrunk like `ShapedRecipePattern.shrink`).
/// The pattern fits anywhere in the grid (the matcher trims the
/// grid to its bounding box) and may be horizontally mirrored;
/// every grid cell outside the pattern must be empty.
pub(crate) struct ShapedRecipe {
    pub(crate) w: u8,
    pub(crate) h: u8,
    /// `w * h` cells; the empty slice = the cell must be empty.
    pub(crate) cells: &'static [Ingredient],
    pub(crate) output: (u16, u8),
    pub(crate) name: &'static str,
}

impl ShapedRecipe {
    /// Number of non-empty pattern cells - the
    /// `ShapedRecipePattern.ingredientCount` early-out.
    pub(crate) fn ingredient_count(&self) -> usize {
        self.cells.iter().filter(|c| !c.is_empty()).count()
    }

    /// MC `canCraftInDimensions`: does the pattern fit a
    /// `gw x gh` grid?
    pub(crate) fn fits(&self, gw: u8, gh: u8) -> bool {
        self.w <= gw && self.h <= gh
    }
}

/// MC `crafting_shapeless` recipe: an exact multiset of ingredient
/// slots - every non-empty grid cell must pay one distinct slot,
/// extra items reject the match.
pub(crate) struct ShapelessRecipe {
    pub(crate) ingredients: &'static [Ingredient],
    pub(crate) output: (u16, u8),
    pub(crate) name: &'static str,
}
