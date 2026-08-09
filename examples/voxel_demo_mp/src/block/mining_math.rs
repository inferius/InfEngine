//! W473 - MC mining + melee math, ported 1:1 from the decompiled
//! 26.1.2 sources.
//!
//! Ported semantics:
//! - `BlockBehaviour.getDestroyProgress` (~355): per-tick progress =
//!   `player.getDestroySpeed(state) / destroyTime /
//!   (hasCorrectToolForDrops ? 30 : 100)`; the block breaks when the
//!   cumulative progress reaches 1. `destroySpeed == -1` (bedrock
//!   family) returns 0 progress - the block never breaks in
//!   survival. `destroyTime == 0` (instabreak) divides to infinity -
//!   the block breaks on the first tick.
//! - `Player.getDestroySpeed` (~586): the held `Tool` component's
//!   mining speed (material speed when the block sits in the tool's
//!   mineable tag, else 1.0), then `/5` when the player's eyes are
//!   in water (`SUBMERGED_MINING_SPEED` attribute default 0.2) and
//!   `/5` again when airborne (`!onGround`). The Efficiency
//!   (`MINING_EFFICIENCY`), Haste / Mining Fatigue effect and Aqua
//!   Affinity (submerged attribute back to 1.0) hooks belong to the
//!   future enchant / effects waves - seams marked inline.
//! - `Player.hasCorrectToolForDrops` (~618): a block that
//!   `requiresCorrectToolForDrops` only drops when the held tool's
//!   `Tool.Rule` chain says so. Per `ToolMaterial`, digger tools
//!   deny drops through the `incorrect_for_<material>_tool` tag and
//!   grant them through their `mineable/<class>` tag; the deny tags
//!   are unions of `needs_{stone,iron,diamond}_tool`, which the
//!   generated table stores as `min_tier` (wood+gold = 1,
//!   stone+copper = 2, iron = 3, diamond+netherite = 4).
//! - `ToolMaterial` speeds: wood 2, stone 4, copper 5, iron 6,
//!   diamond 8, gold 12, netherite 9.
//! - `ShearsItem.createToolProperties` + `applySwordProperties`
//!   (`ToolMaterial.java`): shears mine-and-drop cobweb at 15 and
//!   override leaves 15 / wool 5 / vine + glow_lichen 2; swords
//!   mine-and-drop cobweb at 15. The remaining sword rules
//!   (`sword_efficient` 1.5x, `sword_instantly_mines`) are a
//!   deliberate gap - they only shave fractions of a second off
//!   plant breaks that are near-instant anyway.
//! - `Items.java` + `Attributes.ATTACK_DAMAGE` (player base 1.0):
//!   melee damage = 1 + attackDamageBaseline + material
//!   attackDamageBonus. MC's attack cooldown / sweep mechanic is
//!   absent in the engine - damage values are ported, the cooldown
//!   deliberately is not (partial ports of it would misrepresent
//!   both systems). Spears carry MC's `KineticWeapon` component
//!   (velocity-scaled damage), also an absent mechanic - they fall
//!   back to hand damage here.
//!
//! Block rows live in the GENERATED `hardness_table.rs`; this module
//! resolves engine block ids -> MC names and precomputes id-indexed
//! LUTs for both blocks and held items.

use std::sync::OnceLock;

use super::hardness_table::HARDNESS_TABLE;
use super::{BLOCK_TYPES, COBWEB_ID};

/// Generated-table row: `(mc_name, destroy_time,
/// requires_correct_tool, mineable_class_mask, min_tier)`.
pub(crate) type HardnessRow = (&'static str, f32, bool, u8, u8);

/// `mineable/pickaxe` membership bit.
pub(crate) const CLASS_PICKAXE: u8 = 1;
/// `mineable/axe` membership bit.
pub(crate) const CLASS_AXE: u8 = 2;
/// `mineable/shovel` membership bit.
pub(crate) const CLASS_SHOVEL: u8 = 4;
/// `mineable/hoe` membership bit.
pub(crate) const CLASS_HOE: u8 = 8;

/// Per-block dig profile resolved from the generated table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BlockDig {
    /// MC `destroyTime`: -1 = unbreakable, 0 = instabreak.
    pub(crate) destroy_time: f32,
    /// MC `requiresCorrectToolForDrops`.
    pub(crate) requires_correct_tool: bool,
    /// Which `mineable/*` tags contain the block (`CLASS_*` bits).
    pub(crate) class_mask: u8,
    /// Smallest tool tier whose drops are not denied (0 = any).
    pub(crate) min_tier: u8,
    /// `ShearsItem` override speed for this block (1.0 = none).
    pub(crate) shears_speed: f32,
}

/// Documented fallback for engine rows without an MC block
/// counterpart: MC stone-like 1.5 s destroy time, pickaxe-class,
/// but `requires_correct_tool = false` so nothing silently stops
/// dropping. Reached only by item-shaped registry rows (tools,
/// foods, materials) that can never occupy a world voxel.
const FALLBACK_DIG: BlockDig = BlockDig {
    destroy_time: 1.5,
    requires_correct_tool: false,
    class_mask: CLASS_PICKAXE,
    min_tier: 0,
    shears_speed: 1.0,
};

/// Engine registry name -> MC block name, for the rows whose names
/// predate the MC-faithful naming convention. Mirrored in the
/// generator script's `RENAMES`.
const ENGINE_RENAMES: &[(&str, &str)] = &[
    ("bed", "white_bed"),
    // W477: the ("coal", "coal_ore") row left with the registry
    // rename - block 13 IS "coal_ore" now.
    ("cobble", "cobblestone"),
    ("flower", "poppy"),
    ("flower_blue", "blue_orchid"),
    ("flower_yellow", "dandelion"),
    ("grass", "grass_block"),
    ("iron", "iron_ore"),
    ("leaves", "oak_leaves"),
    ("light_block", "light"),
    ("obsidian_pillar", "obsidian"),
    ("planks", "oak_planks"),
    ("portal", "nether_portal"),
    ("wood", "oak_log"),
    ("wool", "white_wool"),
];

fn table_row(name: &str) -> Option<&'static HardnessRow> {
    HARDNESS_TABLE
        .binary_search_by(|(n, ..)| n.cmp(&name))
        .ok()
        .map(|i| &HARDNESS_TABLE[i])
}

/// Resolve an engine registry name to its generated-table row.
/// Mirrors the generator's `resolve_engine` funnel: exact name,
/// rename table, then the engine's state-variant shapes (crop
/// `_age<N>` stages, `_immature` crops, double-block `_top` /
/// `_bottom` / `_upper` / `_lower` halves, `lit_` furnace / lamp /
/// ore variants).
fn resolve_engine_name(name: &str) -> Option<&'static HardnessRow> {
    if let Some(row) = table_row(name) {
        return Some(row);
    }
    if let Ok(i) =
        ENGINE_RENAMES.binary_search_by(|(engine, _)| (*engine).cmp(name))
    {
        return table_row(ENGINE_RENAMES[i].1);
    }
    if let Some(base) = name.rsplit_once("_age").and_then(|(base, rest)| {
        (!rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
            .then_some(base)
    }) && let Some(row) = table_row(base)
    {
        return Some(row);
    }
    for suffix in ["_immature", "_top", "_bottom", "_upper", "_lower"] {
        if let Some(base) = name.strip_suffix(suffix)
            && let Some(row) = table_row(base)
        {
            return Some(row);
        }
    }
    if let Some(base) = name.strip_prefix("lit_")
        && let Some(row) = table_row(base)
    {
        return Some(row);
    }
    None
}

/// `ShearsItem.createToolProperties` override speeds, keyed by the
/// RESOLVED MC name: cobweb 15 (mines-and-drops), `#leaves` 15,
/// `#wool` 5, vine + glow_lichen 2. Both tags are name-shaped in
/// 26.1.2 (`*_leaves` / `*_wool` cover every member).
fn shears_speed_for(mc_name: &str) -> f32 {
    if mc_name == "cobweb" || mc_name.ends_with("_leaves") {
        15.0
    } else if mc_name.ends_with("_wool") {
        5.0
    } else if mc_name == "vine" || mc_name == "glow_lichen" {
        2.0
    } else {
        1.0
    }
}

struct BlockLut {
    digs: Vec<BlockDig>,
    unresolved: u32,
}

fn block_lut() -> &'static BlockLut {
    static LUT: OnceLock<BlockLut> = OnceLock::new();
    LUT.get_or_init(|| {
        let max_id =
            BLOCK_TYPES.iter().map(|b| b.id).max().unwrap_or(0) as usize;
        let mut digs = vec![FALLBACK_DIG; max_id + 1];
        let mut unresolved = 0u32;
        // Air has no registry row (ids start at 1); seed slot 0
        // from the table directly (MC air is instabreak).
        if let Some(&(_, dt, req, mask, tier)) = table_row("air") {
            digs[0] = BlockDig {
                destroy_time: dt,
                requires_correct_tool: req,
                class_mask: mask,
                min_tier: tier,
                shears_speed: 1.0,
            };
        }
        for b in BLOCK_TYPES.iter() {
            match resolve_engine_name(b.name) {
                Some(&(mc_name, dt, req, mask, tier)) => {
                    digs[b.id as usize] = BlockDig {
                        destroy_time: dt,
                        requires_correct_tool: req,
                        class_mask: mask,
                        min_tier: tier,
                        shears_speed: shears_speed_for(mc_name),
                    };
                }
                None => unresolved += 1,
            }
        }
        BlockLut { digs, unresolved }
    })
}

/// Dig profile for a block id. Ids outside the registry (air) and
/// the item-shaped pseudo rows resolve to `FALLBACK_DIG`; the count
/// of fallback rows is logged once so a registry addition that
/// misses the name funnel is visible.
pub(crate) fn hardness_for(block_id: u16) -> BlockDig {
    let lut = block_lut();
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        tracing::info!(
            "W473: hardness table resolved {} of {} registry rows \
             ({} item-shaped rows use the stone-like fallback)",
            BLOCK_TYPES.len() as u32 - lut.unresolved,
            BLOCK_TYPES.len(),
            lut.unresolved,
        );
    });
    lut.digs
        .get(block_id as usize)
        .copied()
        .unwrap_or(FALLBACK_DIG)
}

/// Count of registry rows that hit the fallback (test hook).
#[cfg(test)]
pub(crate) fn unresolved_registry_rows() -> u32 {
    block_lut().unresolved
}

// ---------------------------------------------------------------
// Held-item side: tool material / class / melee damage.
// ---------------------------------------------------------------

/// Per-item combat + dig profile (`ToolMaterial` x `Items.java`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ItemCombat {
    /// `CLASS_*` bit of the digger class, 0 for non-digger items.
    pub(crate) dig_class: u8,
    /// Drop tier: wood+gold 1, stone+copper 2, iron 3,
    /// diamond+netherite 4. 0 for non-digger items.
    pub(crate) tier: u8,
    /// `ToolMaterial.speed` applied inside the mineable tag.
    pub(crate) dig_speed: f32,
    /// Total melee damage: player base 1.0 + item attribute.
    pub(crate) attack_damage: f32,
    pub(crate) is_sword: bool,
    pub(crate) is_shears: bool,
}

const HAND: ItemCombat = ItemCombat {
    dig_class: 0,
    tier: 0,
    dig_speed: 1.0,
    attack_damage: 1.0,
    is_sword: false,
    is_shears: false,
};

/// `ToolMaterial.java` rows: (tier, speed, attackDamageBonus).
fn material_stats(material: &str) -> Option<(u8, f32, f32)> {
    Some(match material {
        "wooden" => (1, 2.0, 0.0),
        "golden" => (1, 12.0, 0.0),
        "stone" => (2, 4.0, 1.0),
        "copper" => (2, 5.0, 1.0),
        "iron" => (3, 6.0, 2.0),
        "diamond" => (4, 8.0, 3.0),
        "netherite" => (4, 9.0, 4.0),
        _ => return None,
    })
}

/// `Items.java` per-registration `attackDamageBaseline` (the value
/// passed next to the `ToolMaterial`): swords 3, pickaxes 1,
/// shovels 1.5 for every material; axes and hoes vary per material.
fn attack_baseline(kind: &str, material: &str) -> f32 {
    match kind {
        "sword" => 3.0,
        "pickaxe" => 1.0,
        "shovel" => 1.5,
        "axe" => match material {
            "stone" | "copper" => 7.0,
            "diamond" | "netherite" => 5.0,
            // wooden 6, golden 6, iron 6.
            _ => 6.0,
        },
        "hoe" => match material {
            "stone" | "copper" => -1.0,
            "iron" => -2.0,
            "diamond" => -3.0,
            "netherite" => -4.0,
            // wooden 0, golden 0.
            _ => 0.0,
        },
        _ => 0.0,
    }
}

fn class_bit(kind: &str) -> u8 {
    match kind {
        "pickaxe" => CLASS_PICKAXE,
        "axe" => CLASS_AXE,
        "shovel" => CLASS_SHOVEL,
        "hoe" => CLASS_HOE,
        _ => 0,
    }
}

/// Build the profile from a RESOLVED MC item name
/// (`<material>_<kind>` for the tool families).
fn combat_of_mc_name(mc: &str) -> ItemCombat {
    if mc == "shears" {
        // `ShearsItem`: Tool component with the per-block override
        // rules (applied via `BlockDig.shears_speed`), no attribute
        // modifiers -> hand damage.
        return ItemCombat { is_shears: true, ..HAND };
    }
    if mc == "trident" {
        // `TridentItem.createAttributes`: +8 attack damage. No
        // digger class (empty Tool rules).
        return ItemCombat { attack_damage: 9.0, ..HAND };
    }
    if mc == "mace" {
        // `MaceItem.createAttributes`: +5 attack damage (the smash
        // fall-distance bonus is part of the absent cooldown-family
        // mechanics).
        return ItemCombat { attack_damage: 6.0, ..HAND };
    }
    let Some((material, kind)) = mc.rsplit_once('_') else {
        return HAND;
    };
    let Some((tier, speed, bonus)) = material_stats(material) else {
        return HAND;
    };
    let damage = 1.0 + attack_baseline(kind, material) + bonus;
    match kind {
        "sword" => ItemCombat {
            attack_damage: damage,
            is_sword: true,
            ..HAND
        },
        "pickaxe" | "axe" | "shovel" | "hoe" => ItemCombat {
            dig_class: class_bit(kind),
            tier,
            dig_speed: speed,
            attack_damage: damage,
            ..HAND
        },
        _ => HAND,
    }
}

fn item_lut() -> &'static [ItemCombat] {
    static LUT: OnceLock<Vec<ItemCombat>> = OnceLock::new();
    LUT.get_or_init(|| {
        let max_id = crate::item::ITEM_TYPES
            .iter()
            .map(|it| it.id)
            .max()
            .unwrap_or(0) as usize;
        let mut v = vec![HAND; max_id + 1];
        for it in crate::item::ITEM_TYPES.iter() {
            let combat = match crate::item::durability::legacy_mc_name(it.name)
            {
                Some(mc) => combat_of_mc_name(&mc),
                None => combat_of_mc_name(it.name),
            };
            v[it.id as usize] = combat;
        }
        v
    })
}

/// Combat/dig profile for a held item id (empty hand for unknown
/// ids and non-tool items).
pub(crate) fn item_combat(item_id: u16) -> ItemCombat {
    item_lut().get(item_id as usize).copied().unwrap_or(HAND)
}

/// Total melee attack damage for a held item id -
/// `Attributes.ATTACK_DAMAGE` (player base 1.0) plus the item's
/// mainhand modifier. Wood sword 4 ... netherite sword 8, axes 7-10
/// per source; bare hand and non-weapons 1.0.
pub(crate) fn attack_damage(item_id: u16) -> f32 {
    item_combat(item_id).attack_damage
}

/// True when the held item is a digger tool of the given
/// `CLASS_*` bit (shovel-RMB path, hoe-RMB till, HUD labels).
pub(crate) fn is_dig_class(item_id: u16, class: u8) -> bool {
    item_combat(item_id).dig_class & class != 0
}

/// `Player.hasCorrectToolForDrops`: either the block does not
/// require a correct tool, or the held tool's rules grant drops -
/// digger class matching the block's mineable tag at a sufficient
/// tier, or the sword / shears cobweb `minesAndDrops` rule.
pub(crate) fn has_correct_tool_for_drops(held_id: u16, block_id: u16) -> bool {
    let dig = hardness_for(block_id);
    if !dig.requires_correct_tool {
        return true;
    }
    let held = item_combat(held_id);
    if block_id == COBWEB_ID && (held.is_sword || held.is_shears) {
        return true;
    }
    held.dig_class & dig.class_mask != 0 && held.tier >= dig.min_tier
}

/// `Player.getDestroySpeed`: the held tool's mining speed against
/// this block, then the water-eye and airborne penalties.
pub(crate) fn dig_speed(
    held_id: u16,
    block_id: u16,
    eye_in_water: bool,
    on_ground: bool,
) -> f32 {
    let dig = hardness_for(block_id);
    let held = item_combat(held_id);
    let mut speed = if held.is_shears && dig.shears_speed > 1.0 {
        dig.shears_speed
    } else if held.is_sword && block_id == COBWEB_ID {
        // `applySwordProperties`: minesAndDrops(COBWEB, 15).
        15.0
    } else if held.dig_class & dig.class_mask != 0 {
        held.dig_speed
    } else {
        1.0
    };
    // Enchant seam - Efficiency (`MINING_EFFICIENCY` attribute)
    // adds to `speed` here when `speed > 1`; Haste / Mining
    // Fatigue multiply after it. No stack carries them yet.
    if eye_in_water {
        // `SUBMERGED_MINING_SPEED` attribute default 0.2. Enchant
        // seam - Aqua Affinity restores this to 1.0.
        speed /= 5.0;
    }
    if !on_ground {
        speed /= 5.0;
    }
    speed
}

/// `BlockBehaviour.getDestroyProgress`, rescaled from per-tick to
/// per-second (MC runs the check on 20 Hz ticks; the engine's
/// mining session integrates frame dt). Returns 0 for unbreakable
/// blocks (the session never completes) and `f32::INFINITY` for
/// instabreak blocks (the session completes on the first frame).
pub(crate) fn destroy_progress_per_second(
    held_id: u16,
    block_id: u16,
    eye_in_water: bool,
    on_ground: bool,
) -> f32 {
    let dig = hardness_for(block_id);
    if dig.destroy_time < 0.0 {
        return 0.0;
    }
    if dig.destroy_time == 0.0 {
        return f32::INFINITY;
    }
    let modifier = if has_correct_tool_for_drops(held_id, block_id) {
        30.0
    } else {
        100.0
    };
    dig_speed(held_id, block_id, eye_in_water, on_ground)
        / dig.destroy_time
        / modifier
        * 20.0
}

/// Block-side gate for per-block tool wear (`Item.mineBlock`: no
/// wear when the block's destroy speed is 0). Liquids keep the
/// legacy scoop exemption - they are not block mines.
pub(crate) fn mine_wear_applies(block_id: u16) -> bool {
    !super::block_is_liquid(infengine_voxel::BlockId(block_id))
        && hardness_for(block_id).destroy_time != 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seconds a full mining session takes at this rate.
    fn secs(rate: f32) -> f32 {
        1.0 / rate
    }

    /// The generated table must stay sorted - lookups binary
    /// search it. Same for the rename table.
    #[test]
    fn tables_are_sorted_by_name() {
        for w in HARDNESS_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} !< {}", w[0].0, w[1].0);
        }
        for w in ENGINE_RENAMES.windows(2) {
            assert!(w[0].0 < w[1].0, "{} !< {}", w[0].0, w[1].0);
        }
    }

    /// Blocks.java spot pins through the id-keyed resolver,
    /// covering the rename + state-variant funnels.
    #[test]
    fn hardness_rows_match_source() {
        // stone: strength(1.5, 6.0) + requiresCorrectToolForDrops.
        let stone = hardness_for(1);
        assert_eq!(stone.destroy_time, 1.5);
        assert!(stone.requires_correct_tool);
        assert_eq!(stone.class_mask, CLASS_PICKAXE);
        assert_eq!(stone.min_tier, 0);
        // cobble (rename): strength(2.0, 6.0).
        assert_eq!(hardness_for(8).destroy_time, 2.0);
        // wood -> oak_log: strength(2.0), axe class.
        let log = hardness_for(5);
        assert_eq!(log.destroy_time, 2.0);
        assert_eq!(log.class_mask, CLASS_AXE);
        // leaves -> oak_leaves: leavesProperties strength 0.2, hoe.
        let leaves = hardness_for(6);
        assert_eq!(leaves.destroy_time, 0.2);
        assert_eq!(leaves.class_mask, CLASS_HOE);
        assert_eq!(leaves.shears_speed, 15.0);
        // dirt: 0.5, shovel, no correct-tool requirement.
        let dirt = hardness_for(2);
        assert_eq!(dirt.destroy_time, 0.5);
        assert!(!dirt.requires_correct_tool);
        assert_eq!(dirt.class_mask, CLASS_SHOVEL);
        // torch: instabreak.
        assert_eq!(hardness_for(10).destroy_time, 0.0);
        // wool -> white_wool: 0.8, shears override 5.
        let wool = hardness_for(11);
        assert_eq!(wool.destroy_time, 0.8);
        assert_eq!(wool.shears_speed, 5.0);
        // bed -> white_bed: registerBed strength 0.2.
        assert_eq!(hardness_for(35).destroy_time, 0.2);
        // bedrock: strength(-1) = unbreakable.
        assert_eq!(
            hardness_for(crate::block::BEDROCK_ID).destroy_time,
            -1.0
        );
        // obsidian: 50, needs diamond (tier 4).
        let obsidian = hardness_for(crate::block::OBSIDIAN_ID);
        assert_eq!(obsidian.destroy_time, 50.0);
        assert_eq!(obsidian.min_tier, 4);
        // iron_ore: 3.0, needs stone (tier 2).
        let iron_ore = hardness_for(14);
        assert_eq!(iron_ore.destroy_time, 3.0);
        assert!(iron_ore.requires_correct_tool);
        assert_eq!(iron_ore.min_tier, 2);
        // snow layer: 0.1, requires a shovel for drops.
        let snow = hardness_for(15);
        assert_eq!(snow.destroy_time, 0.1);
        assert!(snow.requires_correct_tool);
        assert_eq!(snow.class_mask, CLASS_SHOVEL);
        // cobweb: 4.0, requires correct tool, in no mineable tag.
        let web = hardness_for(COBWEB_ID);
        assert_eq!(web.destroy_time, 4.0);
        assert!(web.requires_correct_tool);
        assert_eq!(web.class_mask, 0);
    }

    /// Engine state-variant names resolve through the funnel.
    #[test]
    fn state_variant_names_resolve() {
        // lit_redstone_ore (66) -> redstone_ore: 3.0, needs iron.
        let lit_ore = hardness_for(66);
        assert_eq!(lit_ore.destroy_time, 3.0);
        assert_eq!(lit_ore.min_tier, 3);
        // oak_door_upper (78) -> oak_door 3.0 axe.
        assert_eq!(hardness_for(78).destroy_time, 3.0);
        // obsidian_pillar (450) -> obsidian.
        assert_eq!(hardness_for(450).destroy_time, 50.0);
        // wheat_age3 (846) -> wheat instabreak.
        assert_eq!(hardness_for(846).destroy_time, 0.0);
        // tall_grass_top (759) -> tall_grass instabreak.
        assert_eq!(hardness_for(759).destroy_time, 0.0);
    }

    /// Every engine registry row either resolves to a real MC row
    /// or is one of the item-shaped pseudo rows (tools, foods,
    /// materials) documented on `FALLBACK_DIG`. The count is
    /// pinned so a future registry addition that misses the name
    /// funnel fails loudly here.
    #[test]
    fn registry_coverage_is_pinned() {
        assert_eq!(unresolved_registry_rows(), 83);
        // And none of the unresolved rows is block-shaped: every
        // row that PLACES a block must resolve. The lone exemption
        // is the W1-era `meat` block - an engine-only legacy block
        // with no MC counterpart; it deliberately rides the
        // documented stone-like fallback.
        let engine_only = ["meat"];
        for it in crate::item::ITEM_TYPES.iter() {
            if let Some(block_id) = it.places {
                let placeable_row =
                    crate::block::block_type(block_id).map(|b| b.name);
                if let Some(name) = placeable_row
                    && !engine_only.contains(&name)
                {
                    assert!(
                        resolve_engine_name(name).is_some(),
                        "placeable block {name} (id {block_id}) \
                         did not resolve"
                    );
                }
            }
        }
    }

    /// ToolMaterial rows through the name funnel: legacy
    /// `<kind>_<tier>` ids and MC-named ids.
    #[test]
    fn tool_material_rows_match_source() {
        // pickaxe_wood (16): tier 1, speed 2.
        let wood_pick = item_combat(16);
        assert_eq!(wood_pick.dig_class, CLASS_PICKAXE);
        assert_eq!(wood_pick.tier, 1);
        assert_eq!(wood_pick.dig_speed, 2.0);
        // pickaxe_stone (17): tier 2, speed 4.
        assert_eq!(item_combat(17).tier, 2);
        assert_eq!(item_combat(17).dig_speed, 4.0);
        // pickaxe_iron (18): tier 3, speed 6.
        assert_eq!(item_combat(18).tier, 3);
        assert_eq!(item_combat(18).dig_speed, 6.0);
        // pickaxe_gold (154): tier 1 (wood-level drops), speed 12.
        assert_eq!(item_combat(154).tier, 1);
        assert_eq!(item_combat(154).dig_speed, 12.0);
        // pickaxe_diamond (155): tier 4, speed 8.
        assert_eq!(item_combat(155).tier, 4);
        assert_eq!(item_combat(155).dig_speed, 8.0);
        // pickaxe_netherite (274): tier 4, speed 9.
        assert_eq!(item_combat(274).tier, 4);
        assert_eq!(item_combat(274).dig_speed, 9.0);
        // shovels + axes + hoes pick up their class.
        assert_eq!(item_combat(29).dig_class, CLASS_SHOVEL); // wood
        assert_eq!(item_combat(159).dig_class, CLASS_SHOVEL); // diamond
        assert_eq!(item_combat(276).tier, 4); // shovel_netherite
        assert_eq!(item_combat(32).dig_class, CLASS_AXE);
        assert_eq!(item_combat(161).dig_speed, 8.0); // axe_diamond
        assert_eq!(item_combat(334).dig_class, CLASS_HOE); // iron_hoe
        assert_eq!(item_combat(335).tier, 1); // gold_hoe -> golden
        // MC-named rows resolve identically.
        assert_eq!(item_combat(874), item_combat(18)); // iron_pickaxe
        assert_eq!(item_combat(946), item_combat(32)); // wooden_axe
        // shears flag.
        assert!(item_combat(640).is_shears);
        // a plain block in hand = bare hand.
        assert_eq!(item_combat(1), HAND);
        assert_eq!(item_combat(u16::MAX), HAND);
    }

    /// Melee damage rows: total = base 1.0 + baseline + bonus.
    #[test]
    fn attack_damage_matches_source() {
        // Swords: baseline 3.0 + bonus.
        assert_eq!(attack_damage(23), 4.0); // wood
        assert_eq!(attack_damage(24), 5.0); // stone
        assert_eq!(attack_damage(25), 6.0); // iron
        assert_eq!(attack_damage(156), 4.0); // gold
        assert_eq!(attack_damage(157), 7.0); // diamond
        assert_eq!(attack_damage(275), 8.0); // netherite
        // Axes: per-material baselines 6/7/6/6/5/5.
        assert_eq!(attack_damage(32), 7.0); // wood 1+6+0
        assert_eq!(attack_damage(33), 9.0); // stone 1+7+1
        assert_eq!(attack_damage(160), 7.0); // gold 1+6+0
        assert_eq!(attack_damage(34), 9.0); // iron 1+6+2
        assert_eq!(attack_damage(161), 9.0); // diamond 1+5+3
        assert_eq!(attack_damage(277), 10.0); // netherite 1+5+4
        // Pickaxes 1+1+bonus, shovels 1+1.5+bonus.
        assert_eq!(attack_damage(16), 2.0);
        assert_eq!(attack_damage(18), 4.0);
        assert_eq!(attack_damage(155), 5.0);
        assert_eq!(attack_damage(29), 2.5);
        assert_eq!(attack_damage(31), 4.5);
        assert_eq!(attack_damage(276), 6.5);
        // Hoes all land on 1.0 (negative baselines cancel).
        assert_eq!(attack_damage(332), 1.0);
        assert_eq!(attack_damage(334), 1.0);
        assert_eq!(attack_damage(278), 1.0);
        // Hand, blocks, foods: base 1.0.
        assert_eq!(attack_damage(0), 1.0);
        assert_eq!(attack_damage(1), 1.0);
        assert_eq!(attack_damage(640), 1.0); // shears
    }

    /// getDestroyProgress pins, in seconds.
    #[test]
    fn destroy_progress_matches_source_formula() {
        // Stone bare hand: 1 / 1.5 / 100 per tick -> 7.5 s, and
        // the drop is denied (wrong tool).
        let rate = destroy_progress_per_second(0, 1, false, true);
        assert!((secs(rate) - 7.5).abs() < 1e-3, "{}", secs(rate));
        assert!(!has_correct_tool_for_drops(0, 1));
        // Stone + wood pickaxe: 2 / 1.5 / 30 -> 1.125 s, drops.
        let rate = destroy_progress_per_second(16, 1, false, true);
        assert!((secs(rate) - 1.125).abs() < 1e-3);
        assert!(has_correct_tool_for_drops(16, 1));
        // Stone + iron pickaxe: 6 / 1.5 / 30 -> 0.375 s.
        let rate = destroy_progress_per_second(18, 1, false, true);
        assert!((secs(rate) - 0.375).abs() < 1e-3);
        // Obsidian + diamond pickaxe: 8 / 50 / 30 -> 9.375 s.
        let rate = destroy_progress_per_second(
            155,
            crate::block::OBSIDIAN_ID,
            false,
            true,
        );
        assert!((secs(rate) - 9.375).abs() < 1e-3);
        // Obsidian + iron pickaxe: wrong tier -> /100 path
        // (6 / 50 / 100 -> 41.667 s) AND no drop.
        let rate = destroy_progress_per_second(
            18,
            crate::block::OBSIDIAN_ID,
            false,
            true,
        );
        assert!((secs(rate) - 41.6667).abs() < 1e-2);
        assert!(!has_correct_tool_for_drops(18, crate::block::OBSIDIAN_ID));
        // Bedrock: progress rate 0 - the session never completes.
        assert_eq!(
            destroy_progress_per_second(
                155,
                crate::block::BEDROCK_ID,
                false,
                true
            ),
            0.0
        );
        // Torch: instabreak.
        assert!(
            destroy_progress_per_second(0, 10, false, true).is_infinite()
        );
        // Dirt: hand 0.75 s vs wood shovel 0.375 s vs iron 0.125 s.
        let hand = destroy_progress_per_second(0, 2, false, true);
        assert!((secs(hand) - 0.75).abs() < 1e-3);
        let wood = destroy_progress_per_second(29, 2, false, true);
        assert!((secs(wood) - 0.375).abs() < 1e-3);
        let iron = destroy_progress_per_second(31, 2, false, true);
        assert!((secs(iron) - 0.125).abs() < 1e-3);
    }

    /// needs_* tier gates on drops.
    #[test]
    fn drop_tier_gates_match_tags() {
        const DIAMOND_ORE: u16 = 118;
        let diamond_ore_row = crate::block::block_type(DIAMOND_ORE)
            .map(|b| b.name)
            .unwrap_or("");
        assert_eq!(diamond_ore_row, "diamond_ore", "id drifted");
        // diamond ore (needs_iron): iron pick yes, stone pick no.
        assert!(has_correct_tool_for_drops(18, DIAMOND_ORE));
        assert!(!has_correct_tool_for_drops(17, DIAMOND_ORE));
        // iron ore (needs_stone): stone pick yes, wood pick no,
        // gold pick no (gold = wood-level drops per the
        // incorrect_for_gold_tool tag).
        assert!(has_correct_tool_for_drops(17, 14));
        assert!(!has_correct_tool_for_drops(16, 14));
        assert!(!has_correct_tool_for_drops(154, 14));
        // Gold pick still drops plain stone (min_tier 0).
        assert!(has_correct_tool_for_drops(154, 1));
        // Wrong class never drops: shovel on stone.
        assert!(!has_correct_tool_for_drops(31, 1));
        // Cobweb: sword + shears drop it, pickaxe does not.
        assert!(has_correct_tool_for_drops(25, COBWEB_ID));
        assert!(has_correct_tool_for_drops(640, COBWEB_ID));
        assert!(!has_correct_tool_for_drops(18, COBWEB_ID));
    }

    /// Water-eye and airborne dig penalties (/5 each, stacking).
    #[test]
    fn water_and_airborne_penalties() {
        let base = destroy_progress_per_second(18, 1, false, true);
        let wet = destroy_progress_per_second(18, 1, true, true);
        let air = destroy_progress_per_second(18, 1, false, false);
        let both = destroy_progress_per_second(18, 1, true, false);
        assert!((wet - base / 5.0).abs() < 1e-6);
        assert!((air - base / 5.0).abs() < 1e-6);
        assert!((both - base / 25.0).abs() < 1e-6);
    }

    /// Shears override speeds (ShearsItem.createToolProperties).
    #[test]
    fn shears_speeds_match_source() {
        // wool 5: 5 / 0.8 / 30 -> 0.24 s.
        let rate = destroy_progress_per_second(640, 11, false, true);
        assert!((secs(rate) - 0.24).abs() < 1e-3);
        // leaves 15: 15 / 0.2 / 30 = 2.5 progress per tick - MC
        // breaks it in a single tick; the per-second rate is 50.
        let rate = destroy_progress_per_second(640, 6, false, true);
        assert!((secs(rate) - 0.02).abs() < 1e-3);
        // cobweb 15 via shears AND sword: 15 / 4 / 30 -> 0.4 s.
        let rate = destroy_progress_per_second(640, COBWEB_ID, false, true);
        assert!((secs(rate) - 0.4).abs() < 1e-3);
        let rate = destroy_progress_per_second(25, COBWEB_ID, false, true);
        assert!((secs(rate) - 0.4).abs() < 1e-3);
        // hand cobweb: 1 / 4 / 100 -> 20 s (correct-tool denied).
        let rate = destroy_progress_per_second(0, COBWEB_ID, false, true);
        assert!((secs(rate) - 20.0).abs() < 1e-2);
    }

    /// Wear gate: instabreak blocks cost 0 durability, liquids
    /// keep the scoop exemption, regular blocks wear.
    #[test]
    fn mine_wear_gate() {
        assert!(!mine_wear_applies(10)); // torch instabreak
        assert!(!mine_wear_applies(846)); // wheat_age3 instabreak
        assert!(!mine_wear_applies(crate::block::WATER_ID));
        assert!(!mine_wear_applies(crate::block::LAVA_ID));
        assert!(mine_wear_applies(1)); // stone
        assert!(mine_wear_applies(2)); // dirt
    }
}
