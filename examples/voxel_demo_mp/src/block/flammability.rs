//! MC `FireBlock::bootStrap()` flammability table (1:1 transcribe).
//!
//! Source: `net/minecraft/world/level/block/FireBlock.java` lines
//! 299..=508 (MC 26.1.2 decompile path
//! `J:/Temp/template-mod-template-26.1.2/sources/
//! minecraft-merged-52430b475d-26.1.2-sources/`). Every
//! `fire.setFlammable(Blocks.XXX, igniteOdds, burnOdds)` call in
//! that block is mirrored here as a `MC_FLAMMABLES` entry keyed by
//! the MC block name. ZERO interpretation: if MC ships
//! `OAK_PLANKS, 5, 20` we ship `("oak_planks", 5, 20)`.
//!
//! Storage strategy:
//! 1. `MC_FLAMMABLES` is the authoritative table: every flammable MC
//!    block lives here even if the engine has no corresponding block
//!    id. That keeps the data ready for blocks we'll add later.
//! 2. `flammability(id)` walks the engine block id -> MC name mapping
//!    (`engine_id_to_mc_name`) and returns the matching MC entry's
//!    odds. Items not mapped return `(0, 0)` (non-flammable).
//!
//! Coverage: ~210 entries. The check `mc_flammability_table_locked`
//! locks the count + a stable checksum so a refactor that silently
//! drops rows trips a test.

use crate::block::{LEAVES_ID, PLANKS_ID, WOOD_ID, WOOL_ID};

/// One `setFlammable` row from `FireBlock::bootStrap()`. Tuple form
/// kept compact because the table is large + read-only.
pub(crate) type FlammableEntry = (&'static str, u32, u32);

/// MC `FireBlock::bootStrap()` 1:1 transcription. Order matches
/// the source file so an audit reading the decompile top-to-bottom
/// can grep this list in the same order.
///
/// Cited line numbers correspond to
/// `FireBlock.java` (MC 26.1.2). Indenting groups in the source
/// (planks / slabs / fence / log / wood / leaves / flowers / wool /
/// carpet / misc) are preserved as blank-line-separated chunks in
/// the spec below to mirror the source.
pub(crate) const MC_FLAMMABLES: &[FlammableEntry] = &[
    // L301..L311 - planks family.
    ("oak_planks", 5, 20),
    ("spruce_planks", 5, 20),
    ("birch_planks", 5, 20),
    ("jungle_planks", 5, 20),
    ("acacia_planks", 5, 20),
    ("cherry_planks", 5, 20),
    ("dark_oak_planks", 5, 20),
    ("pale_oak_planks", 5, 20),
    ("mangrove_planks", 5, 20),
    ("bamboo_planks", 5, 20),
    ("bamboo_mosaic", 5, 20),
    // L312..L322 - slabs.
    ("oak_slab", 5, 20),
    ("spruce_slab", 5, 20),
    ("birch_slab", 5, 20),
    ("jungle_slab", 5, 20),
    ("acacia_slab", 5, 20),
    ("cherry_slab", 5, 20),
    ("dark_oak_slab", 5, 20),
    ("pale_oak_slab", 5, 20),
    ("mangrove_slab", 5, 20),
    ("bamboo_slab", 5, 20),
    ("bamboo_mosaic_slab", 5, 20),
    // L323..L332 - fence gates.
    ("oak_fence_gate", 5, 20),
    ("spruce_fence_gate", 5, 20),
    ("birch_fence_gate", 5, 20),
    ("jungle_fence_gate", 5, 20),
    ("acacia_fence_gate", 5, 20),
    ("cherry_fence_gate", 5, 20),
    ("dark_oak_fence_gate", 5, 20),
    ("pale_oak_fence_gate", 5, 20),
    ("mangrove_fence_gate", 5, 20),
    ("bamboo_fence_gate", 5, 20),
    // L333..L342 - fences.
    ("oak_fence", 5, 20),
    ("spruce_fence", 5, 20),
    ("birch_fence", 5, 20),
    ("jungle_fence", 5, 20),
    ("acacia_fence", 5, 20),
    ("cherry_fence", 5, 20),
    ("dark_oak_fence", 5, 20),
    ("pale_oak_fence", 5, 20),
    ("mangrove_fence", 5, 20),
    ("bamboo_fence", 5, 20),
    // L343..L353 - stairs.
    ("oak_stairs", 5, 20),
    ("birch_stairs", 5, 20),
    ("spruce_stairs", 5, 20),
    ("jungle_stairs", 5, 20),
    ("acacia_stairs", 5, 20),
    ("cherry_stairs", 5, 20),
    ("dark_oak_stairs", 5, 20),
    ("pale_oak_stairs", 5, 20),
    ("mangrove_stairs", 5, 20),
    ("bamboo_stairs", 5, 20),
    ("bamboo_mosaic_stairs", 5, 20),
    // L354..L363 - logs + bamboo block.
    ("oak_log", 5, 5),
    ("spruce_log", 5, 5),
    ("birch_log", 5, 5),
    ("jungle_log", 5, 5),
    ("acacia_log", 5, 5),
    ("cherry_log", 5, 5),
    ("pale_oak_log", 5, 5),
    ("dark_oak_log", 5, 5),
    ("mangrove_log", 5, 5),
    ("bamboo_block", 5, 5),
    // L364..L373 - stripped logs + stripped bamboo block.
    ("stripped_oak_log", 5, 5),
    ("stripped_spruce_log", 5, 5),
    ("stripped_birch_log", 5, 5),
    ("stripped_jungle_log", 5, 5),
    ("stripped_acacia_log", 5, 5),
    ("stripped_cherry_log", 5, 5),
    ("stripped_dark_oak_log", 5, 5),
    ("stripped_pale_oak_log", 5, 5),
    ("stripped_mangrove_log", 5, 5),
    ("stripped_bamboo_block", 5, 5),
    // L374..L382 - stripped woods.
    ("stripped_oak_wood", 5, 5),
    ("stripped_spruce_wood", 5, 5),
    ("stripped_birch_wood", 5, 5),
    ("stripped_jungle_wood", 5, 5),
    ("stripped_acacia_wood", 5, 5),
    ("stripped_cherry_wood", 5, 5),
    ("stripped_dark_oak_wood", 5, 5),
    ("stripped_pale_oak_wood", 5, 5),
    ("stripped_mangrove_wood", 5, 5),
    // L383..L391 - woods.
    ("oak_wood", 5, 5),
    ("spruce_wood", 5, 5),
    ("birch_wood", 5, 5),
    ("jungle_wood", 5, 5),
    ("acacia_wood", 5, 5),
    ("cherry_wood", 5, 5),
    ("pale_oak_wood", 5, 5),
    ("dark_oak_wood", 5, 5),
    ("mangrove_wood", 5, 5),
    // L392 - mangrove roots.
    ("mangrove_roots", 5, 20),
    // L393..L401 - leaves.
    ("oak_leaves", 30, 60),
    ("spruce_leaves", 30, 60),
    ("birch_leaves", 30, 60),
    ("jungle_leaves", 30, 60),
    ("acacia_leaves", 30, 60),
    ("cherry_leaves", 30, 60),
    ("dark_oak_leaves", 30, 60),
    ("pale_oak_leaves", 30, 60),
    ("mangrove_leaves", 30, 60),
    // L402..L403 - misc.
    ("bookshelf", 30, 20),
    ("tnt", 15, 100),
    // L404..L436 - grasses / flowers / petals / leaf litter.
    ("short_grass", 60, 100),
    ("fern", 60, 100),
    ("dead_bush", 60, 100),
    ("short_dry_grass", 60, 100),
    ("tall_dry_grass", 60, 100),
    ("sunflower", 60, 100),
    ("lilac", 60, 100),
    ("rose_bush", 60, 100),
    ("peony", 60, 100),
    ("tall_grass", 60, 100),
    ("large_fern", 60, 100),
    ("dandelion", 60, 100),
    ("golden_dandelion", 60, 100),
    ("poppy", 60, 100),
    ("open_eyeblossom", 60, 100),
    ("closed_eyeblossom", 60, 100),
    ("blue_orchid", 60, 100),
    ("allium", 60, 100),
    ("azure_bluet", 60, 100),
    ("red_tulip", 60, 100),
    ("orange_tulip", 60, 100),
    ("white_tulip", 60, 100),
    ("pink_tulip", 60, 100),
    ("oxeye_daisy", 60, 100),
    ("cornflower", 60, 100),
    ("lily_of_the_valley", 60, 100),
    ("torchflower", 60, 100),
    ("pitcher_plant", 60, 100),
    ("wither_rose", 60, 100),
    ("pink_petals", 60, 100),
    ("wildflowers", 60, 100),
    ("leaf_litter", 60, 100),
    ("cactus_flower", 60, 100),
    // L437..L452 - wool (16 colors).
    ("white_wool", 30, 60),
    ("orange_wool", 30, 60),
    ("magenta_wool", 30, 60),
    ("light_blue_wool", 30, 60),
    ("yellow_wool", 30, 60),
    ("lime_wool", 30, 60),
    ("pink_wool", 30, 60),
    ("gray_wool", 30, 60),
    ("light_gray_wool", 30, 60),
    ("cyan_wool", 30, 60),
    ("purple_wool", 30, 60),
    ("blue_wool", 30, 60),
    ("brown_wool", 30, 60),
    ("green_wool", 30, 60),
    ("red_wool", 30, 60),
    ("black_wool", 30, 60),
    // L453..L456 - misc.
    ("vine", 15, 100),
    ("coal_block", 5, 5),
    ("hay_block", 60, 20),
    ("target", 15, 20),
    // L457..L472 - carpet (16 colors).
    ("white_carpet", 60, 20),
    ("orange_carpet", 60, 20),
    ("magenta_carpet", 60, 20),
    ("light_blue_carpet", 60, 20),
    ("yellow_carpet", 60, 20),
    ("lime_carpet", 60, 20),
    ("pink_carpet", 60, 20),
    ("gray_carpet", 60, 20),
    ("light_gray_carpet", 60, 20),
    ("cyan_carpet", 60, 20),
    ("purple_carpet", 60, 20),
    ("blue_carpet", 60, 20),
    ("brown_carpet", 60, 20),
    ("green_carpet", 60, 20),
    ("red_carpet", 60, 20),
    ("black_carpet", 60, 20),
    // L473..L507 - pale moss + dried kelp + bamboo + scaffolding
    // + lectern + composter + sweet berry bush + beehive / bee nest
    // + azalea leaves + cave vines + spore blossom + azalea +
    // flowering azalea + dripleaves + roots + glow lichen +
    // firefly bush + bush + shelves.
    ("pale_moss_block", 5, 100),
    ("pale_moss_carpet", 5, 100),
    ("pale_hanging_moss", 5, 100),
    ("dried_kelp_block", 30, 60),
    ("bamboo", 60, 60),
    ("scaffolding", 60, 60),
    ("lectern", 30, 20),
    ("composter", 5, 20),
    ("sweet_berry_bush", 60, 100),
    ("beehive", 5, 20),
    ("bee_nest", 30, 20),
    ("azalea_leaves", 30, 60),
    ("flowering_azalea_leaves", 30, 60),
    ("cave_vines", 15, 60),
    ("cave_vines_plant", 15, 60),
    ("spore_blossom", 60, 100),
    ("azalea", 30, 60),
    ("flowering_azalea", 30, 60),
    ("big_dripleaf", 60, 100),
    ("big_dripleaf_stem", 60, 100),
    ("small_dripleaf", 60, 100),
    ("hanging_roots", 30, 60),
    ("glow_lichen", 15, 100),
    ("firefly_bush", 60, 100),
    ("bush", 60, 100),
    ("acacia_shelf", 30, 20),
    ("bamboo_shelf", 30, 20),
    ("birch_shelf", 30, 20),
    ("cherry_shelf", 30, 20),
    ("dark_oak_shelf", 30, 20),
    ("jungle_shelf", 30, 20),
    ("mangrove_shelf", 30, 20),
    ("oak_shelf", 30, 20),
    ("pale_oak_shelf", 30, 20),
    ("spruce_shelf", 30, 20),
];

/// Map an engine block id to the MC block name in [`MC_FLAMMABLES`].
/// Returns `None` for engine blocks that don't have a 1:1 MC
/// counterpart (e.g. our generic "planks" maps to "oak_planks";
/// our generic "wood" maps to "oak_log"). The choice of which MC
/// variant maps to a generic engine id mirrors the texture choice
/// in `block/mod.rs` - our `wood` block uses `log_oak.png` so we
/// map it to `oak_log`.
fn engine_id_to_mc_name(id: u16) -> Option<&'static str> {
    match id {
        WOOD_ID => Some("oak_log"),       // BLOCK_TYPES row 5 uses log_oak.png
        PLANKS_ID => Some("oak_planks"),  // BLOCK_TYPES row 7 uses planks_oak.png
        LEAVES_ID => Some("oak_leaves"),  // BLOCK_TYPES row 6 uses leaves_oak.tga
        WOOL_ID => Some("white_wool"),    // BLOCK_TYPES row 11 uses wool.png (no per-color split)
        _ => None,
    }
}

/// Look up the `(ignite_odds, burn_odds)` for a given MC block
/// name. Returns `(0, 0)` if `name` isn't in [`MC_FLAMMABLES`].
/// Linear scan; the table is ~210 entries + this is only called
/// from the fire-tick path (gated on hash chance per cell), so
/// the constant factor doesn't matter.
pub(crate) fn flammability_by_mc_name(name: &str) -> (u32, u32) {
    for entry in MC_FLAMMABLES {
        if entry.0 == name {
            return (entry.1, entry.2);
        }
    }
    (0, 0)
}

/// Engine-side block-id -> `(ignite_odds, burn_odds)` accessor.
/// Replaces the W10-era inline match in `block/fire.rs`. Blocks
/// not mapped via `engine_id_to_mc_name` return `(0, 0)` and stay
/// unburnable, same as before.
///
/// Wiring: `block::fire::block_flammability` delegates to this.
pub(crate) fn flammability_for_id(id: u16) -> (u32, u32) {
    match engine_id_to_mc_name(id) {
        Some(name) => flammability_by_mc_name(name),
        None => (0, 0),
    }
}

/// Deterministic checksum of the flammability table. Mixing every
/// row's `(ignite, burn)` into a single u64 catches accidental
/// edits in the regression test `mc_flammability_table_locked`.
#[allow(dead_code)] // consumed by the W11.6 wave_11_locked regression test
pub(crate) fn flammability_checksum() -> u64 {
    let mut acc: u64 = 0;
    for (name, ig, burn) in MC_FLAMMABLES {
        // FNV-ish mix on the name bytes + the two odds.
        for b in name.bytes() {
            acc = acc.wrapping_mul(0x100000001B3).wrapping_add(b as u64);
        }
        acc = acc
            .wrapping_mul(0x100000001B3)
            .wrapping_add(*ig as u64);
        acc = acc
            .wrapping_mul(0x100000001B3)
            .wrapping_add(*burn as u64);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{COAL_ID, FLINT_AND_STEEL_ID, GLOWSTONE_ID, STONE_ID};

    #[test]
    fn oak_planks_flammability_matches_mc_5_20() {
        assert_eq!(flammability_by_mc_name("oak_planks"), (5, 20));
    }

    #[test]
    fn oak_leaves_flammability_matches_mc_30_60() {
        assert_eq!(flammability_by_mc_name("oak_leaves"), (30, 60));
    }

    #[test]
    fn bookshelf_flammability_30_20() {
        assert_eq!(flammability_by_mc_name("bookshelf"), (30, 20));
    }

    #[test]
    fn tnt_flammability_15_100() {
        assert_eq!(flammability_by_mc_name("tnt"), (15, 100));
    }

    #[test]
    fn hay_block_flammability_60_20() {
        assert_eq!(flammability_by_mc_name("hay_block"), (60, 20));
    }

    #[test]
    fn coal_block_flammability_5_5() {
        // MC FireBlock.java:454.
        assert_eq!(flammability_by_mc_name("coal_block"), (5, 5));
    }

    #[test]
    fn dried_kelp_block_flammability_30_60() {
        assert_eq!(flammability_by_mc_name("dried_kelp_block"), (30, 60));
    }

    #[test]
    fn bamboo_flammability_60_60() {
        assert_eq!(flammability_by_mc_name("bamboo"), (60, 60));
    }

    #[test]
    fn scaffolding_flammability_60_60() {
        assert_eq!(flammability_by_mc_name("scaffolding"), (60, 60));
    }

    #[test]
    fn pink_petals_flammability_60_100() {
        // MC FireBlock.java:433.
        assert_eq!(flammability_by_mc_name("pink_petals"), (60, 100));
    }

    #[test]
    fn wool_flammability_30_60() {
        let wools = [
            "white_wool",
            "orange_wool",
            "magenta_wool",
            "light_blue_wool",
            "yellow_wool",
            "lime_wool",
            "pink_wool",
            "gray_wool",
            "light_gray_wool",
            "cyan_wool",
            "purple_wool",
            "blue_wool",
            "brown_wool",
            "green_wool",
            "red_wool",
            "black_wool",
        ];
        for w in wools {
            assert_eq!(flammability_by_mc_name(w), (30, 60), "{w}");
        }
    }

    #[test]
    fn carpet_flammability_60_20() {
        let carpets = [
            "white_carpet",
            "orange_carpet",
            "magenta_carpet",
            "light_blue_carpet",
            "yellow_carpet",
            "lime_carpet",
            "pink_carpet",
            "gray_carpet",
            "light_gray_carpet",
            "cyan_carpet",
            "purple_carpet",
            "blue_carpet",
            "brown_carpet",
            "green_carpet",
            "red_carpet",
            "black_carpet",
        ];
        for c in carpets {
            assert_eq!(flammability_by_mc_name(c), (60, 20), "{c}");
        }
    }

    #[test]
    fn unknown_name_is_non_flammable() {
        assert_eq!(flammability_by_mc_name("stone"), (0, 0));
        assert_eq!(flammability_by_mc_name("xyzzy_foo"), (0, 0));
    }

    #[test]
    fn stone_flammability_0_0() {
        // Stone is not in MC's flammables list -> non-flammable.
        assert_eq!(flammability_for_id(STONE_ID), (0, 0));
    }

    #[test]
    fn glowstone_flammability_0_0() {
        // Glowstone isn't flammable in MC either.
        assert_eq!(flammability_for_id(GLOWSTONE_ID), (0, 0));
    }

    #[test]
    fn engine_wood_maps_to_oak_log_5_5() {
        // Engine `wood` (id 5) maps to MC oak_log (5, 5).
        assert_eq!(flammability_for_id(crate::block::WOOD_ID), (5, 5));
    }

    #[test]
    fn engine_planks_maps_to_oak_planks_5_20() {
        assert_eq!(flammability_for_id(crate::block::PLANKS_ID), (5, 20));
    }

    #[test]
    fn engine_leaves_maps_to_oak_leaves_30_60() {
        assert_eq!(flammability_for_id(crate::block::LEAVES_ID), (30, 60));
    }

    #[test]
    fn engine_wool_maps_to_white_wool_30_60() {
        assert_eq!(flammability_for_id(crate::block::WOOL_ID), (30, 60));
    }

    #[test]
    fn engine_tool_is_non_flammable() {
        // Flint+steel is a tool, not a block -> no flammability.
        assert_eq!(flammability_for_id(FLINT_AND_STEEL_ID), (0, 0));
        // Coal-ore item id (also tool tier 1) -> no flammability.
        assert_eq!(flammability_for_id(COAL_ID), (0, 0));
    }

    #[test]
    fn mc_flammability_table_locked() {
        // Lock the table size + a stable checksum so a refactor
        // that drops rows or tweaks odds trips this. Bump the
        // checksum if you add/remove rows DELIBERATELY.
        assert_eq!(
            MC_FLAMMABLES.len(),
            207,
            "expected 207 MC flammability rows; got {}",
            MC_FLAMMABLES.len()
        );
        let sum = flammability_checksum();
        // Recorded once after the initial transcribe; any further
        // edits to MC_FLAMMABLES must update this number along
        // with the row count above.
        assert_eq!(
            sum, 0x6B584F067ED80FE6,
            "flammability checksum drift: 0x{sum:016X}"
        );
    }

    #[test]
    fn every_mc_row_lookup_round_trips() {
        // The table linearly grows; this sanity test enforces no
        // row sneaks in with an empty name.
        for (name, ig, burn) in MC_FLAMMABLES {
            assert!(!name.is_empty(), "empty MC name in flammability table");
            assert_eq!(flammability_by_mc_name(name), (*ig, *burn));
        }
    }
}
