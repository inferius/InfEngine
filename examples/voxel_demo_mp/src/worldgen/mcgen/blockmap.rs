//! W245.3 - blockstate -> engine block-id mapping for structure
//! templates.
//!
//! Resolution order:
//! 1. the exact registry name (the registry deliberately reuses MC
//!    names, so ~120 of the 164 village states hit directly),
//! 2. property-aware special cases (two-cell plants, door halves),
//! 3. the rename/substitute table (engine legacy names like
//!    `cobble`/`wood`/`planks`, plus nearest-material stand-ins for
//!    blocks the roster lacks - each substitution keeps the family),
//! 4. otherwise `Keep`: the cell is NOT written (the world keeps its
//!    terrain there) and the name is once-logged as a worklist item.
//!
//! Known engine-wide gaps, deliberately NOT worked around here:
//! stairs/doors/trapdoors/gates carry no orientation state in the
//! engine (stairs render east-facing), so their facing properties are
//! dropped; `wall_torch` maps to the floor torch because worldgen has
//! no path into the torch-orient map (same note as the mineshaft's
//! skipped supports torches).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use super::template::PaletteState;

/// The outcome of mapping one palette state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mapped {
    /// Write this engine block id.
    Id(u16),
    /// Leave the world cell untouched (unmapped block, door upper
    /// half, the jigsaw block itself).
    Keep,
}

pub(crate) fn registry_id(name: &str) -> Option<u16> {
    static IDX: OnceLock<HashMap<&'static str, u16>> = OnceLock::new();
    IDX.get_or_init(|| {
        crate::block::BLOCK_TYPES
            .iter()
            .map(|bt| (bt.name, bt.id))
            .collect()
    })
    .get(name)
    .copied()
}

/// MC name -> engine registry name. Substitutions (right side is a
/// different block) stay within the material family.
const RENAMES: &[(&str, &str)] = &[
    // Engine legacy names.
    ("grass_block", "grass"),
    ("cobblestone", "cobble"),
    ("oak_log", "wood"),
    ("oak_planks", "planks"),
    ("oak_leaves", "leaves"),
    ("white_wool", "wool"),
    // Wall torch -> floor torch (orient map unreachable from
    // worldgen).
    ("wall_torch", "torch"),
    // Missing smooth/chiseled sandstone variants -> plain family.
    ("smooth_sandstone_stairs", "sandstone_stairs"),
    ("smooth_sandstone_slab", "sandstone_slab"),
    ("chiseled_sandstone", "cut_sandstone"),
    // Missing diorite shapes -> the grey stone family.
    ("diorite_stairs", "stone_stairs"),
    ("diorite_slab", "smooth_stone_slab"),
    // Stripped + bark variants -> the plain logs.
    ("stripped_oak_log", "wood"),
    ("stripped_oak_wood", "wood"),
    ("stripped_spruce_log", "spruce_log"),
    ("stripped_spruce_wood", "spruce_log"),
    ("stripped_acacia_log", "acacia_log"),
    ("spruce_wood", "spruce_log"),
    ("acacia_wood", "acacia_log"),
    // Missing wood buttons/plates -> the existing neighbours.
    ("jungle_button", "stone_button"),
    ("spruce_pressure_plate", "oak_pressure_plate"),
    ("acacia_pressure_plate", "oak_pressure_plate"),
    // Farmland is not in the roster; dirt carries the crops.
    // W248 igloo basement: silverfish blocks wait for their mob -
    // the plain variants stand in.
    ("infested_stone_bricks", "stone_bricks"),
    ("infested_mossy_stone_bricks", "mossy_stone_bricks"),
    ("infested_chiseled_stone_bricks", "chiseled_stone_bricks"),
    // W251 outpost: no mossy cobblestone shapes yet - the plain
    // cobblestone shapes keep the family (full mossy_cobblestone
    // blocks hit the registry directly).
    ("mossy_cobblestone_stairs", "cobblestone_stairs"),
    ("mossy_cobblestone_slab", "cobblestone_slab"),
    ("mossy_cobblestone_wall", "cobblestone_wall"),
    // W259 mansion: the engine wire block carries the legacy name;
    // silverfish cobble waits for the mob; trapped chests act as
    // plain chests until the trap mechanic exists.
    ("redstone_wire", "redstone_dust"),
    ("infested_cobblestone", "cobble"),
    ("trapped_chest", "chest"),
    // W263 trial chambers: waxed copper states collapse onto the
    // canonical rows (identical textures; no weathering tick), and
    // the odd substitutes keep their material family.
    ("waxed_copper_block", "copper_block"),
    ("waxed_cut_copper", "cut_copper"),
    ("waxed_oxidized_copper", "oxidized_copper"),
    ("waxed_oxidized_cut_copper", "oxidized_cut_copper"),
    ("waxed_chiseled_copper", "chiseled_copper"),
    ("waxed_oxidized_chiseled_copper", "oxidized_chiseled_copper"),
    ("waxed_copper_grate", "copper_grate"),
    ("waxed_oxidized_copper_grate", "oxidized_copper_grate"),
    ("waxed_copper_bulb", "copper_bulb"),
    ("waxed_oxidized_copper_bulb", "copper_bulb"),
    ("waxed_cut_copper_stairs", "cut_copper_stairs"),
    ("waxed_oxidized_cut_copper_stairs", "oxidized_cut_copper_stairs"),
    ("waxed_cut_copper_slab", "cut_copper_slab"),
    ("waxed_oxidized_cut_copper_slab", "oxidized_cut_copper_slab"),
    ("waxed_copper_door", "copper_door"),
    ("waxed_oxidized_copper_door", "oxidized_copper_door"),
    ("waxed_copper_trapdoor", "copper_trapdoor"),
    ("waxed_oxidized_copper_trapdoor", "oxidized_copper_trapdoor"),
    ("powder_snow", "snow_block"),
    ("mangrove_wood", "mangrove_log"),
];

/// Names we knowingly leave unplaced until their blocks exist. The
/// blockmap test pins the full-village unmapped set to this list so
/// a template update can't silently grow the gap.
pub(crate) const KNOWN_SKIPS: &[&str] = &[
    "brown_wall_banner",
    "campfire",
    // W251 outpost decoration.
    // W259 mansion decoration.
    // W261 ancient city decoration (the redstone lab's piston head,
    // the skull piles).
    // W263 trial chambers decoration (archaeology pots, tripwire
    // traps, dripstone, hanging chains).
    "tnt",
];

fn log_unmapped(name: &str) {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut set) = seen.lock()
        && set.insert(name.to_string())
    {
        tracing::info!(block = name, "W245: template blockstate not mapped, kept");
    }
}

/// Map one template palette state to an engine write.
pub(crate) fn map_state(state: &PaletteState) -> Mapped {
    let name = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
    match name {
        "air" | "cave_air" => return Mapped::Id(0),
        // The solver replaces jigsaw cells with their final_state
        // before placement; a raw jigsaw never logs.
        "jigsaw" | "structure_void" => return Mapped::Keep,
        // Data-marker structure blocks: MC's BlockIgnoreProcessor
        // drops them and the marker handlers clear the cell - air
        // matches the placed result (the igloo chest marker).
        "structure_block" => return Mapped::Id(0),
        // W331: the tripwire string is a near-invisible trap line -
        // air beats a magenta placeholder until a real string model
        // lands (the hooks themselves DO map).
        "tripwire" => return Mapped::Id(0),
        // Two-cell plants: the engine splits them into two ids.
        "tall_grass" => {
            let ours = match state.prop("half") {
                Some("upper") => "tall_grass_top",
                _ => "tall_grass_bottom",
            };
            return registry_id(ours).map(Mapped::Id).unwrap_or(Mapped::Keep);
        }
        // W294: crop states carry their growth age - route to the
        // matching stage id so village farms carve half-grown rows
        // like vanilla instead of everything fully mature.
        "wheat" | "carrots" | "potatoes" | "beetroots" => {
            let age: usize = state
                .prop("age")
                .and_then(|a| a.parse().ok())
                .unwrap_or(0);
            if let Some(id) =
                crate::block::crop_stages::stage_for_age(name, age)
            {
                return Mapped::Id(id);
            }
        }
        // No large fern blocks: the single fern stands in below, the
        // upper half opens to air.
        "large_fern" => {
            return match state.prop("half") {
                Some("upper") => Mapped::Id(0),
                _ => registry_id("fern").map(Mapped::Id).unwrap_or(Mapped::Keep),
            };
        }
        _ => {}
    }
    // Door upper halves: W311 - write the SAME door id as the lower
    // cell (the door.rs design note: upper halves share the id). The
    // old Mapped::Keep left the upper cell as terrain AIR, so every
    // template door was a half-door with a hole on top (the acacia
    // village-door screenshot). The mesher renders the upper cell as
    // the top panel by inheriting the lower cell's facing.
    if name.ends_with("_door") && state.prop("half") == Some("upper") {
        if let Some(id) = registry_id(name) {
            return Mapped::Id(id);
        }
        return Mapped::Keep;
    }
    if let Some(id) = registry_id(name) {
        return Mapped::Id(id);
    }
    // W331: filled cauldron states collapse onto the empty cauldron
    // (the water fill overlay needs the level-aware model).
    if name == "water_cauldron" || name == "lava_cauldron" {
        return registry_id("cauldron").map(Mapped::Id).unwrap_or(Mapped::Keep);
    }
    if let Some((_, ours)) = RENAMES.iter().find(|(mc, _)| *mc == name)
        && let Some(id) = registry_id(ours)
    {
        return Mapped::Id(id);
    }
    log_unmapped(name);
    Mapped::Keep
}

/// Map a blockstate STRING (`minecraft:cobblestone[facing=north]`) -
/// the shape jigsaw `final_state` uses.
pub(crate) fn map_state_string(s: &str) -> Mapped {
    let (name, props_str) = match s.split_once('[') {
        Some((n, rest)) => (n, rest.strip_suffix(']').unwrap_or(rest)),
        None => (s, ""),
    };
    let props: Vec<(String, String)> = props_str
        .split(',')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    map_state(&PaletteState { name: name.trim().to_string(), props })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(name: &str, props: &[(&str, &str)]) -> PaletteState {
        PaletteState {
            name: name.to_string(),
            props: props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn direct_renamed_and_air_states_map() {
        assert_eq!(map_state(&st("minecraft:air", &[])), Mapped::Id(0));
        assert_eq!(map_state(&st("minecraft:cave_air", &[])), Mapped::Id(0));
        assert_eq!(
            map_state(&st("minecraft:cobblestone", &[])),
            Mapped::Id(crate::block::COBBLE_ID)
        );
        assert_eq!(
            map_state(&st("minecraft:oak_planks", &[])),
            Mapped::Id(crate::block::PLANKS_ID)
        );
        // Direct registry hit with properties dropped.
        let stairs = map_state(&st("minecraft:oak_stairs", &[("facing", "north")]));
        assert!(matches!(stairs, Mapped::Id(_)));
    }

    #[test]
    fn split_plants_and_door_halves() {
        assert_eq!(
            map_state(&st("minecraft:tall_grass", &[("half", "lower")])),
            Mapped::Id(758),
        );
        assert_eq!(
            map_state(&st("minecraft:tall_grass", &[("half", "upper")])),
            Mapped::Id(759),
        );
        // W311: the upper half writes the SAME door id as the lower
        // (the old Keep left a hole above every template door).
        assert_eq!(
            map_state(&st("minecraft:oak_door", &[("half", "upper")])),
            map_state(&st("minecraft:oak_door", &[("half", "lower")])),
        );
        assert!(matches!(
            map_state(&st("minecraft:oak_door", &[("half", "lower")])),
            Mapped::Id(_)
        ));
        assert_eq!(map_state(&st("minecraft:jigsaw", &[])), Mapped::Keep);
    }

    #[test]
    fn state_strings_parse() {
        assert_eq!(
            map_state_string("minecraft:cobblestone"),
            Mapped::Id(crate::block::COBBLE_ID)
        );
        assert!(matches!(
            map_state_string("minecraft:oak_stairs[facing=west,half=bottom]"),
            Mapped::Id(_)
        ));
    }

    /// The full-village coverage pin: mapping every palette state of
    /// every packaged template leaves EXACTLY the known-skip set
    /// unmapped. A template-data update that introduces new unmapped
    /// blocks fails here instead of silently thinning villages.
    #[test]
    fn village_palettes_map_or_are_known_skips() {
        let mut unmapped: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut mapped_states = 0usize;
        for id in super::super::village_data::all_template_ids() {
            let t = super::super::village_data::template(&id).expect("template loads");
            for s in &t.palette {
                let name =
                    s.name.strip_prefix("minecraft:").unwrap_or(&s.name).to_string();
                match map_state(s) {
                    Mapped::Id(_) => mapped_states += 1,
                    Mapped::Keep => {
                        // Structural keeps (jigsaw, door uppers) are
                        // expected; count real gaps only.
                        if name != "jigsaw"
                            && name != "structure_void"
                            && !(name.ends_with("_door")
                                && s.prop("half") == Some("upper"))
                        {
                            unmapped.insert(name);
                        }
                    }
                }
            }
        }
        assert!(mapped_states > 1000, "mapped palette states: {mapped_states}");
        let expected: std::collections::BTreeSet<String> =
            KNOWN_SKIPS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            unmapped, expected,
            "unmapped set drifted from KNOWN_SKIPS - new blocks to map?"
        );
    }
}
