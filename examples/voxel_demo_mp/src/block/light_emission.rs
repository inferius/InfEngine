//! MC `Blocks.java` `.lightLevel(...)` table (1:1 transcribe).
//!
//! Source: `net/minecraft/world/level/block/Blocks.java` (MC 26.1.2
//! decompile at `J:/Temp/template-mod-template-26.1.2/sources/
//! minecraft-merged-52430b475d-26.1.2-sources/`). Every block whose
//! `BlockBehaviour.Properties` carries a `.lightLevel(...)` modifier
//! is mirrored here as an `MC_EMITTERS` row keyed by the MC block
//! name. ZERO interpretation: if MC ships `TORCH ... .lightLevel(
//! statex -> 14)` we ship `("torch", 14)`.
//!
//! For state-dependent emitters MC uses lambdas:
//!   - `litBlockEmission(N)` returns N when the block's `lit`
//!     property is true, 0 otherwise. We store the "lit" value (N)
//!     under the canonical block name; the unlit form has no entry
//!     (= returns 0 implicitly).
//!   - State-derived emitters (sea pickle / cave vines / respawn
//!     anchor / trial spawner / vault / glow lichen) are listed
//!     with their MAX possible lightLevel - the engine doesn't have
//!     state for them yet, so the MAX is the safe "ready when state
//!     lands" value.
//!
//! Storage strategy mirrors `block/flammability.rs`:
//! 1. `MC_EMITTERS` is the authoritative table.
//! 2. `light_emission(id)` walks engine block id -> MC name
//!    (`engine_id_to_mc_name`) and returns the matching entry's
//!    value. Blocks not mapped return 0.

use crate::block::{FIRE_ID, GLOWSTONE_ID, LAVA_ID, TORCH_ID};

/// One `.lightLevel(...)` row from `Blocks.java`. Tuple form
/// kept compact because the table is read-only.
pub(crate) type EmitterEntry = (&'static str, u8);

/// MC `Blocks.java` 1:1 transcription of every `.lightLevel(...)`
/// invocation. Order roughly matches the source line order so an
/// audit reading the decompile top-to-bottom can grep this list
/// in the same order.
///
/// Comments cite the MC source line in `Blocks.java`.
pub(crate) const MC_EMITTERS: &[EmitterEntry] = &[
    // L255 - LAVA: statex -> 15.
    ("lava", 15),
    // L1007 - BROWN_MUSHROOM: statex -> 1.
    ("brown_mushroom", 1),
    // L1194 - TORCH: statex -> 14.
    ("torch", 14),
    // L1199 - WALL_TORCH: statex -> 14.
    ("wall_torch", 14),
    // L1209 - FIRE: statex -> 15.
    ("fire", 15),
    // L1221 - SOUL_FIRE: statex -> 10.
    ("soul_fire", 10),
    // L1298 - FURNACE (lit): litBlockEmission(13). Stored as the
    // lit value; unlit -> implicit 0.
    ("furnace", 13),
    // L1895 - REDSTONE_ORE (lit): litBlockEmission(9).
    ("redstone_ore", 9),
    // L1906 - REDSTONE_TORCH (lit): litBlockEmission(7).
    ("redstone_torch", 7),
    // L1911 - REDSTONE_WALL_TORCH (lit): litBlockEmission(7).
    ("redstone_wall_torch", 7),
    // L2035 - SOUL_TORCH: statex -> 10.
    ("soul_torch", 10),
    // L2040 - SOUL_WALL_TORCH: statex -> 10.
    ("soul_wall_torch", 10),
    // L2045 - COPPER_TORCH: statex -> 14.
    ("copper_torch", 14),
    // L2050 - COPPER_WALL_TORCH: statex -> 14.
    ("copper_wall_torch", 14),
    // L2059 - GLOWSTONE: statex -> 15.
    ("glowstone", 15),
    // L2065 - NETHER_PORTAL: statex -> 11.
    ("nether_portal", 11),
    // L2084 - JACK_O_LANTERN: statex -> 15.
    ("jack_o_lantern", 15),
    // L2371 - GLOW_LICHEN: GlowLichenBlock.emission(7); max = 7.
    ("glow_lichen", 7),
    // L2481 - ENCHANTING_TABLE: statex -> 7.
    ("enchanting_table", 7),
    // L2485 - BREWING_STAND: statex -> 1.
    ("brewing_stand", 1),
    // L2494 - LAVA_CAULDRON: statex -> 15.
    ("lava_cauldron", 15),
    // L2507 - END_PORTAL: statex -> 15.
    ("end_portal", 15),
    // L2519 - END_PORTAL_FRAME: statex -> 1.
    ("end_portal_frame", 1),
    // L2530 - DRAGON_EGG: statex -> 1.
    ("dragon_egg", 1),
    // L2537 - REDSTONE_LAMP (lit): litBlockEmission(15).
    ("redstone_lamp", 15),
    // L2567 - ENDER_CHEST: statex -> 7.
    ("ender_chest", 7),
    // L2599 - BEACON: statex -> 15.
    ("beacon", 15),
    // L3054 - LIGHT: LightBlock.LIGHT_EMISSION; max = 15.
    ("light", 15),
    // L3098 - SEA_LANTERN: statex -> 15.
    ("sea_lantern", 15),
    // L4125 - END_ROD: statex -> 14.
    ("end_rod", 14),
    // L4204 - END_GATEWAY: statex -> 15.
    ("end_gateway", 15),
    // L4238 - MAGMA_BLOCK: statex -> 3.
    ("magma_block", 3),
    // L5003 - SEA_PICKLE: 0 / 3 / 6 / 9 / 12; max = 12 when 4 alive.
    // We store 12 as the "ready when state lands" max.
    ("sea_pickle", 12),
    // L5019 - CONDUIT: statex -> 15.
    ("conduit", 15),
    // L5157 - SMOKER (lit): litBlockEmission(13).
    ("smoker", 13),
    // L5167 - BLAST_FURNACE (lit): litBlockEmission(13).
    ("blast_furnace", 13),
    // L5216 - LANTERN: statex -> 15.
    ("lantern", 15),
    // L5228 - SOUL_LANTERN: statex -> 10.
    ("soul_lantern", 10),
    // L5242 - COPPER_LANTERN (variant create call): statex -> 15.
    ("copper_lantern", 15),
    // L5254 - CAMPFIRE (lit): litBlockEmission(15).
    ("campfire", 15),
    // L5266 - SOUL_CAMPFIRE (lit): litBlockEmission(10).
    ("soul_campfire", 10),
    // L5359 - SHROOMLIGHT: statex -> 15.
    ("shroomlight", 15),
    // L5605 - CRYING_OBSIDIAN: statex -> 10.
    ("crying_obsidian", 10),
    // L5615 - RESPAWN_ANCHOR: 0/3/7/11/15 by charge; max = 15.
    ("respawn_anchor", 15),
    // L5717 - CANDLE_CAKE: litBlockEmission(3).
    ("candle_cake", 3),
    // L5792 - AMETHYST_CLUSTER: statex -> 5.
    ("amethyst_cluster", 5),
    // L5798 - LARGE_AMETHYST_BUD: statex -> 4.
    ("large_amethyst_bud", 4),
    // L5803 - MEDIUM_AMETHYST_BUD: statex -> 2.
    ("medium_amethyst_bud", 2),
    // L5808 - SMALL_AMETHYST_BUD: statex -> 1.
    ("small_amethyst_bud", 1),
    // L5876 - SCULK_SENSOR: statex -> 1.
    ("sculk_sensor", 1),
    // L5876 - CALIBRATED_SCULK_SENSOR (ofLegacyCopy(SCULK_SENSOR)).
    ("calibrated_sculk_sensor", 1),
    // L5899 - SCULK_CATALYST: statex -> 6.
    ("sculk_catalyst", 6),
    // L6158 - COPPER_BULB (lit): litBlockEmission(15).
    ("copper_bulb", 15),
    // L6163 - EXPOSED_COPPER_BULB (lit): litBlockEmission(12).
    ("exposed_copper_bulb", 12),
    // L6168 - WEATHERED_COPPER_BULB (lit): litBlockEmission(8).
    ("weathered_copper_bulb", 8),
    // L6173 - OXIDIZED_COPPER_BULB (lit): litBlockEmission(4).
    ("oxidized_copper_bulb", 4),
    // L6343 - CAVE_VINES: CaveVines.emission(14); max = 14.
    ("cave_vines", 14),
    // L6354 - CAVE_VINES_PLANT: CaveVines.emission(14); max = 14.
    ("cave_vines_plant", 14),
    // L6527 - OCHRE_FROGLIGHT: statex -> 15.
    ("ochre_froglight", 15),
    // L6532 - VERDANT_FROGLIGHT: statex -> 15.
    ("verdant_froglight", 15),
    // L6537 - PEARLESCENT_FROGLIGHT: statex -> 15.
    ("pearlescent_froglight", 15),
    // L6566 - TRIAL_SPAWNER: per-state; max = 14 in ACTIVE state.
    ("trial_spawner", 14),
    // L6580 - VAULT: per-state; max = 12 in active states.
    ("vault", 12),
    // L6657 - FIREFLY_BUSH: statex -> 2.
    ("firefly_bush", 2),
    // L6828 - CANDLE: CandleBlock.LIGHT_EMISSION; max = 12 (3 per
    // candle x 4). Stored as MAX so the table is "ready when state
    // lands".
    ("candle", 12),
];

/// Map an engine block id to its MC block name in [`MC_EMITTERS`].
/// Returns `None` for engine blocks that have no MC emitter
/// counterpart. New engine blocks added in W11.3 should extend
/// this mapping so their lighting picks up the MC value
/// automatically.
fn engine_id_to_mc_name(id: u16) -> Option<&'static str> {
    use crate::block::{
        COPPER_TORCH_ID, COPPER_WALL_TORCH_ID, CRYING_OBSIDIAN_ID, END_ROD_ID,
        ENDER_CHEST_ID, JACK_O_LANTERN_ID, LANTERN_ID, LIGHT_BLOCK_ID,
        LIT_REDSTONE_LAMP_ID, LIT_REDSTONE_ORE_ID, MAGMA_BLOCK_ID,
        REDSTONE_TORCH_ID, REDSTONE_WALL_TORCH_ID, RESPAWN_ANCHOR_ID,
        SEA_LANTERN_ID, SHROOMLIGHT_ID, SOUL_FIRE_ID, SOUL_LANTERN_ID,
        SOUL_TORCH_ID, SOUL_WALL_TORCH_ID,
    };
    match id {
        TORCH_ID => Some("torch"),
        // W312: the portal curtain glows light 11 like MC - the id
        // was missing from this map, so lit portals baked NO light
        // (the user's 'portal should glow' report).
        crate::block::PORTAL_ID => Some("nether_portal"),
        GLOWSTONE_ID => Some("glowstone"),
        LAVA_ID => Some("lava"),
        FIRE_ID => Some("fire"),
        REDSTONE_TORCH_ID => Some("redstone_torch"),
        REDSTONE_WALL_TORCH_ID => Some("redstone_wall_torch"),
        SOUL_TORCH_ID => Some("soul_torch"),
        SOUL_WALL_TORCH_ID => Some("soul_wall_torch"),
        COPPER_TORCH_ID => Some("copper_torch"),
        COPPER_WALL_TORCH_ID => Some("copper_wall_torch"),
        SEA_LANTERN_ID => Some("sea_lantern"),
        JACK_O_LANTERN_ID => Some("jack_o_lantern"),
        SOUL_FIRE_ID => Some("soul_fire"),
        END_ROD_ID => Some("end_rod"),
        MAGMA_BLOCK_ID => Some("magma_block"),
        SHROOMLIGHT_ID => Some("shroomlight"),
        LANTERN_ID => Some("lantern"),
        SOUL_LANTERN_ID => Some("soul_lantern"),
        CRYING_OBSIDIAN_ID => Some("crying_obsidian"),
        RESPAWN_ANCHOR_ID => Some("respawn_anchor"),
        ENDER_CHEST_ID => Some("ender_chest"),
        LIT_REDSTONE_LAMP_ID => Some("redstone_lamp"),
        LIT_REDSTONE_ORE_ID => Some("redstone_ore"),
        LIGHT_BLOCK_ID => Some("light"),
        _ => None,
    }
}

/// Look up an MC emitter's `lightLevel` by MC block name. Returns
/// `0` if `name` isn't in [`MC_EMITTERS`].
pub(crate) fn emission_by_mc_name(name: &str) -> u8 {
    for entry in MC_EMITTERS {
        if entry.0 == name {
            return entry.1;
        }
    }
    0
}

/// Engine-side block-id -> `lightLevel` accessor. Blocks not in
/// the engine-id mapping return 0 (non-emitter). Replaces the
/// hand-written match in `lighting::block_emission`.
pub(crate) fn light_emission_for_id(id: u16) -> u8 {
    // W168: a (lit) candle emits light 3 in MC; checked by name so all
    // 17 colours share it without 17 table rows (keeps the emission
    // checksum stable). 1-4 count scaling (3/6/9/12) is a follow-up.
    if crate::block::block_type(id)
        .is_some_and(|b| b.name == "candle" || b.name.ends_with("_candle"))
    {
        return 3;
    }
    match engine_id_to_mc_name(id) {
        Some(name) => emission_by_mc_name(name),
        None => 0,
    }
}

/// Deterministic checksum of the emission table. Catches accidental
/// row drops / value drifts in the regression test
/// `mc_emission_table_locked`.
#[allow(dead_code)] // consumed by the W11.6 wave_11_locked regression test
pub(crate) fn emission_checksum() -> u64 {
    let mut acc: u64 = 0;
    for (name, level) in MC_EMITTERS {
        for b in name.bytes() {
            acc = acc.wrapping_mul(0x100000001B3).wrapping_add(b as u64);
        }
        acc = acc
            .wrapping_mul(0x100000001B3)
            .wrapping_add(*level as u64);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torch_emits_14() {
        assert_eq!(emission_by_mc_name("torch"), 14);
    }

    #[test]
    fn glowstone_emits_15() {
        assert_eq!(emission_by_mc_name("glowstone"), 15);
    }

    #[test]
    fn lava_emits_15() {
        assert_eq!(emission_by_mc_name("lava"), 15);
    }

    #[test]
    fn fire_emits_15() {
        assert_eq!(emission_by_mc_name("fire"), 15);
    }

    #[test]
    fn soul_fire_emits_10() {
        assert_eq!(emission_by_mc_name("soul_fire"), 10);
    }

    #[test]
    fn redstone_torch_emits_7() {
        assert_eq!(emission_by_mc_name("redstone_torch"), 7);
    }

    #[test]
    fn soul_torch_emits_10() {
        assert_eq!(emission_by_mc_name("soul_torch"), 10);
    }

    #[test]
    fn jack_o_lantern_emits_15() {
        assert_eq!(emission_by_mc_name("jack_o_lantern"), 15);
    }

    #[test]
    fn beacon_emits_15() {
        assert_eq!(emission_by_mc_name("beacon"), 15);
    }

    #[test]
    fn end_rod_emits_14() {
        assert_eq!(emission_by_mc_name("end_rod"), 14);
    }

    #[test]
    fn magma_block_emits_3() {
        assert_eq!(emission_by_mc_name("magma_block"), 3);
    }

    #[test]
    fn shroomlight_emits_15() {
        assert_eq!(emission_by_mc_name("shroomlight"), 15);
    }

    #[test]
    fn lantern_emits_15() {
        assert_eq!(emission_by_mc_name("lantern"), 15);
    }

    #[test]
    fn soul_lantern_emits_10() {
        assert_eq!(emission_by_mc_name("soul_lantern"), 10);
    }

    #[test]
    fn crying_obsidian_emits_10() {
        assert_eq!(emission_by_mc_name("crying_obsidian"), 10);
    }

    #[test]
    fn ender_chest_emits_7() {
        assert_eq!(emission_by_mc_name("ender_chest"), 7);
    }

    #[test]
    fn brewing_stand_emits_1() {
        assert_eq!(emission_by_mc_name("brewing_stand"), 1);
    }

    #[test]
    fn redstone_lamp_lit_emits_15() {
        assert_eq!(emission_by_mc_name("redstone_lamp"), 15);
    }

    #[test]
    fn furnace_lit_emits_13() {
        assert_eq!(emission_by_mc_name("furnace"), 13);
    }

    #[test]
    fn nether_portal_emits_11() {
        assert_eq!(emission_by_mc_name("nether_portal"), 11);
        // W312: the ENGINE id resolves too (the map row was missing).
        assert_eq!(light_emission_for_id(crate::block::PORTAL_ID), 11);
    }

    #[test]
    fn froglight_variants_emit_15() {
        for n in ["ochre_froglight", "verdant_froglight", "pearlescent_froglight"] {
            assert_eq!(emission_by_mc_name(n), 15, "{n}");
        }
    }

    #[test]
    fn amethyst_buds_emit_descending() {
        // Cluster=5, large=4, medium=2, small=1.
        assert_eq!(emission_by_mc_name("amethyst_cluster"), 5);
        assert_eq!(emission_by_mc_name("large_amethyst_bud"), 4);
        assert_eq!(emission_by_mc_name("medium_amethyst_bud"), 2);
        assert_eq!(emission_by_mc_name("small_amethyst_bud"), 1);
    }

    #[test]
    fn unknown_name_emits_0() {
        assert_eq!(emission_by_mc_name("stone"), 0);
        assert_eq!(emission_by_mc_name("xyzzy_foo"), 0);
    }

    #[test]
    fn engine_torch_id_maps_to_torch() {
        use crate::block::TORCH_ID;
        assert_eq!(light_emission_for_id(TORCH_ID), 14);
    }

    #[test]
    fn engine_glowstone_id_maps_to_glowstone() {
        use crate::block::GLOWSTONE_ID;
        assert_eq!(light_emission_for_id(GLOWSTONE_ID), 15);
    }

    #[test]
    fn engine_lava_id_maps_to_lava() {
        use crate::block::LAVA_ID;
        assert_eq!(light_emission_for_id(LAVA_ID), 15);
    }

    #[test]
    fn engine_fire_id_maps_to_fire() {
        use crate::block::FIRE_ID;
        assert_eq!(light_emission_for_id(FIRE_ID), 15);
    }

    #[test]
    fn engine_air_id_emits_0() {
        assert_eq!(light_emission_for_id(0), 0);
    }

    #[test]
    fn engine_stone_id_emits_0() {
        use crate::block::STONE_ID;
        assert_eq!(light_emission_for_id(STONE_ID), 0);
    }

    #[test]
    fn mc_emission_table_locked() {
        // Lock the table size + a stable checksum so a refactor
        // that drops rows or tweaks values trips this.
        assert_eq!(
            MC_EMITTERS.len(),
            65,
            "expected 65 MC emitter rows; got {}",
            MC_EMITTERS.len()
        );
        let sum = emission_checksum();
        // Recorded once after the initial transcribe; any deliberate
        // edits to MC_EMITTERS must update this number along with the
        // row count above.
        assert_eq!(
            sum, 0xEF3D3956269C5D2A,
            "emission checksum drift: 0x{sum:016X}"
        );
    }

    #[test]
    fn every_emitter_row_lookup_round_trips() {
        for (name, level) in MC_EMITTERS {
            assert!(!name.is_empty(), "empty MC name in emitter table");
            assert_eq!(emission_by_mc_name(name), *level);
        }
    }
}
