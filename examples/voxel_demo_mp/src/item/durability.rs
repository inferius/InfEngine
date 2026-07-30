//! W471 - durability core: id-keyed max-damage lookup + the central
//! wear helper (`hurt_and_break`), ported from the decompiled MC
//! 26.1.2 sources.
//!
//! Ported semantics:
//! - `ItemStack.hurtAndBreak` / `processDurabilityChange` /
//!   `applyDamage` / `isBroken` (`net/minecraft/world/item/
//!   ItemStack.java`): wear counts UP from 0; `setDamageValue`
//!   clamps into `[0, max_damage]`; the stack is broken at
//!   `damage >= max_damage` and then shrinks by one. Creative
//!   (`hasInfiniteMaterials`, our `instabuild`) never wears.
//! - Per-action wear values from the item data components:
//!   `ToolMaterial.applyToolProperties` gives digger tools
//!   `Tool(damage_per_block = 1)` + `Weapon(item_damage_per_attack
//!   = 2)`; `applySwordProperties` gives swords `Tool(2)` +
//!   `Weapon(1)`; `ShearsItem.createToolProperties` gives shears
//!   `Tool(1)` and no Weapon component (attack wear 0).
//! - `LivingEntity.doHurtEquipment` (via `Player.hurtArmor`): each
//!   worn damageable piece takes `(int)max(1.0, damage / 4.0)`.
//!
//! The name-keyed max-damage rows live in the GENERATED
//! `durability_table.rs`; this module resolves engine item ids to
//! MC names (the registry mostly uses MC names already; the early
//! `<kind>_<tier>` legacy rows get swapped here) and precomputes an
//! id-indexed LUT.

use std::sync::OnceLock;

use super::durability_table::MAX_DAMAGE_TABLE;
use super::stack::ItemStack;
use super::{ITEM_TYPES, item_type};

/// Wear-relevant item class, mirroring which data components the MC
/// item carries (`DataComponents.TOOL` / `DataComponents.WEAPON`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WearClass {
    /// `applySwordProperties`: Tool(damage_per_block = 2) +
    /// Weapon(item_damage_per_attack = 1).
    Sword,
    /// `applyToolProperties` (pickaxe / shovel / axe / hoe):
    /// Tool(1) + Weapon(2).
    Digger,
    /// `ShearsItem`: Tool(1), no Weapon component.
    Shears,
    /// Damageable without Tool/Weapon (flint+steel, bow, armor,
    /// ...) or not damageable at all: no per-block / per-hit wear.
    None,
}

/// Per-id durability entry, precomputed once over `ITEM_TYPES`.
struct DurEntry {
    max_damage: Option<u16>,
    class: WearClass,
}

/// Map a legacy engine item name (`<kind>_<tier>`, e.g.
/// `pickaxe_wood`, plus the lone `gold_<kind>` shape) to the MC
/// snake_case name (`wooden_pickaxe`). Names already in MC form
/// pass through `None` here and hit the table directly.
fn legacy_mc_name(engine: &str) -> Option<String> {
    if let Some((kind, tier)) = engine.rsplit_once('_')
        && matches!(kind, "pickaxe" | "sword" | "shovel" | "axe" | "hoe")
    {
        let tier_adj = match tier {
            "wood" => "wooden",
            "gold" => "golden",
            "stone" | "iron" | "diamond" | "netherite" => tier,
            _ => return None,
        };
        return Some(format!("{tier_adj}_{kind}"));
    }
    // `gold_hoe`-style rows (tier prefix without the -en suffix).
    engine
        .strip_prefix("gold_")
        .map(|rest| format!("golden_{rest}"))
}

/// Wear class from the RESOLVED MC name (suffix shapes are exact in
/// the MC namespace).
fn class_of_mc_name(mc: &str) -> WearClass {
    if mc.ends_with("_sword") {
        WearClass::Sword
    } else if mc.ends_with("_pickaxe")
        || mc.ends_with("_shovel")
        || mc.ends_with("_axe")
        || mc.ends_with("_hoe")
    {
        WearClass::Digger
    } else if mc == "shears" {
        WearClass::Shears
    } else {
        WearClass::None
    }
}

fn table_lookup(name: &str) -> Option<u16> {
    MAX_DAMAGE_TABLE
        .binary_search_by(|(n, _)| n.cmp(&name))
        .ok()
        .map(|i| MAX_DAMAGE_TABLE[i].1)
}

fn lut() -> &'static [DurEntry] {
    static LUT: OnceLock<Vec<DurEntry>> = OnceLock::new();
    LUT.get_or_init(|| {
        let max_id =
            ITEM_TYPES.iter().map(|it| it.id).max().unwrap_or(0) as usize;
        let mut v: Vec<DurEntry> = Vec::with_capacity(max_id + 1);
        for id in 0..=max_id as u16 {
            let entry = item_type(id)
                .map(|it| {
                    let (max, name) = match table_lookup(it.name) {
                        Some(m) => (Some(m), it.name.to_string()),
                        None => match legacy_mc_name(it.name) {
                            Some(mc) => (table_lookup(&mc), mc),
                            None => (None, it.name.to_string()),
                        },
                    };
                    DurEntry {
                        max_damage: max,
                        // Wear values only apply to damageable
                        // items (an undamageable id never reaches
                        // `hurt_and_break` anyway, but keep the
                        // class honest).
                        class: if max.is_some() {
                            class_of_mc_name(&name)
                        } else {
                            WearClass::None
                        },
                    }
                })
                .unwrap_or(DurEntry {
                    max_damage: None,
                    class: WearClass::None,
                });
            v.push(entry);
        }
        v
    })
}

/// Max damage (durability budget) for an item id, or `None` for
/// items that never wear out. The MC `minecraft:max_damage`
/// component value, resolved through the generated table.
pub(crate) fn max_damage_for(item_id: u16) -> Option<u16> {
    lut().get(item_id as usize).and_then(|e| e.max_damage)
}

/// Wear applied to the held item per block broken - the MC
/// `Tool.damage_per_block` component default (`Item.mineBlock`):
/// digger tools + shears 1, swords 2, everything else 0. The
/// caller owns the block-side gate (`destroySpeed != 0`).
pub(crate) fn mine_wear_amount(item_id: u16) -> u16 {
    match lut().get(item_id as usize).map(|e| e.class) {
        Some(WearClass::Sword) => 2,
        Some(WearClass::Digger) | Some(WearClass::Shears) => 1,
        _ => 0,
    }
}

/// Wear applied to the held item per melee hit - the MC
/// `Weapon.item_damage_per_attack` component default
/// (`ItemStack.postHurtEnemy`): swords 1, digger tools 2, shears
/// (no Weapon component) and everything else 0.
pub(crate) fn attack_wear_amount(item_id: u16) -> u16 {
    match lut().get(item_id as usize).map(|e| e.class) {
        Some(WearClass::Sword) => 1,
        Some(WearClass::Digger) => 2,
        _ => 0,
    }
}

/// Armor wear per damaging hit - `LivingEntity.doHurtEquipment`:
/// `(int)Math.max(1.0F, damage / 4.0F)` applied to EACH worn
/// damageable piece; zero/negative damage wears nothing.
pub(crate) fn armor_wear_amount(damage: f32) -> u16 {
    if damage <= 0.0 {
        return 0;
    }
    (damage / 4.0).max(1.0) as u16
}

/// W472 seam - Unbreaking: map INTENDED wear to APPLIED wear.
///
/// MC routes positive durability changes through
/// `EnchantmentHelper.processDurabilityChange`, where the Unbreaking
/// enchantment independently ignores each intended point with a
/// level-dependent probability. The stack's enchant list is
/// consulted HERE; until the enchanting wave lands no stack carries
/// an Unbreaking id, so this is the identity.
fn unbreaking_reduced_wear(stack: &ItemStack, intended: u16) -> u16 {
    // W472: define the Unbreaking enchant id, look its level up in
    // `stack.components.enchants`, and roll the per-point skip odds
    // from the source formula here.
    let _ = &stack.components.enchants;
    intended
}

/// Central wear application - the port of `ItemStack.hurtAndBreak`.
///
/// Raises the stack's damage by `amount` (after the Unbreaking
/// seam), clamped into `[0, max_damage]` like `setDamageValue`.
/// Returns `true` when the item BROKE this call (`isBroken`:
/// damage reached max) - the stack shrinks by one, which for the
/// stack-of-1 damageable items clears the slot. The caller plays
/// the break FX on `true`.
///
/// No-ops (`false`): creative (`hasInfiniteMaterials` gate in
/// `processDurabilityChange`), zero amount, empty slot, and items
/// without a max_damage row.
pub(crate) fn hurt_and_break(
    slot: &mut Option<ItemStack>,
    amount: u16,
    creative: bool,
) -> bool {
    if creative || amount == 0 {
        return false;
    }
    let Some(st) = slot.as_mut() else {
        return false;
    };
    let Some(max) = max_damage_for(st.id) else {
        return false;
    };
    let applied = unbreaking_reduced_wear(st, amount);
    if applied == 0 {
        return false;
    }
    let damage = st.components.damage.saturating_add(applied).min(max);
    st.components.damage = damage;
    if damage >= max {
        // `applyDamage`: shrink(1) once broken. Damageable items
        // stack to 1, so this clears the slot; mirror the shrink
        // arithmetic anyway for robustness.
        if st.count <= 1 {
            *slot = None;
        } else {
            st.count -= 1;
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The generated table must stay sorted - `table_lookup` binary
    /// searches it.
    #[test]
    fn table_is_sorted_by_name() {
        for w in MAX_DAMAGE_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} !< {}", w[0].0, w[1].0);
        }
    }

    /// Source-pinned spot checks (ToolMaterial.java values) through
    /// the id-keyed resolver, covering both the legacy
    /// `<kind>_<tier>` names and the MC-shaped rows.
    #[test]
    fn tool_tier_max_damage_matches_source() {
        assert_eq!(max_damage_for(16), Some(59)); // pickaxe_wood
        assert_eq!(max_damage_for(17), Some(131)); // pickaxe_stone
        assert_eq!(max_damage_for(18), Some(250)); // pickaxe_iron
        assert_eq!(max_damage_for(154), Some(32)); // pickaxe_gold
        assert_eq!(max_damage_for(155), Some(1561)); // pickaxe_diamond
        assert_eq!(max_damage_for(274), Some(2031)); // pickaxe_netherite
        assert_eq!(max_damage_for(23), Some(59)); // sword_wood
        assert_eq!(max_damage_for(874), Some(250)); // iron_pickaxe (MC name)
        assert_eq!(max_damage_for(946), Some(59)); // wooden_axe (MC name)
        assert_eq!(max_damage_for(335), Some(32)); // gold_hoe (prefix swap)
        assert_eq!(max_damage_for(636), Some(32)); // hoe_gold (suffix swap)
    }

    /// Direct `.durability(N)` items from Items.java.
    #[test]
    fn direct_durability_matches_source() {
        assert_eq!(max_damage_for(48), Some(64)); // flint_and_steel
        assert_eq!(
            max_damage_for(48),
            Some(crate::block::FLINT_AND_STEEL_MAX_USES),
            "table row must agree with the legacy const"
        );
        assert_eq!(max_damage_for(638), Some(384)); // bow
        assert_eq!(max_damage_for(639), Some(465)); // crossbow
        assert_eq!(max_damage_for(640), Some(238)); // shears
        assert_eq!(max_damage_for(641), Some(64)); // fishing_rod
        assert_eq!(max_damage_for(642), Some(336)); // shield
    }

    /// Armor durability = ArmorType unit x material multiplier
    /// (ArmorType.getDurability).
    #[test]
    fn armor_max_damage_matches_source() {
        assert_eq!(max_damage_for(882), Some(55)); // leather_helmet 11*5
        assert_eq!(max_damage_for(885), Some(65)); // leather_boots 13*5
        assert_eq!(max_damage_for(886), Some(240)); // chainmail_chestplate 16*15
        assert_eq!(max_damage_for(887), Some(165)); // iron_helmet 11*15
        assert_eq!(max_damage_for(889), Some(225)); // iron_leggings 15*15
        assert_eq!(max_damage_for(892), Some(112)); // golden_chestplate 16*7
        assert_eq!(max_damage_for(898), Some(429)); // diamond_boots 13*33
        assert_eq!(max_damage_for(989), Some(407)); // netherite_helmet 11*37
        assert_eq!(max_damage_for(990), Some(592)); // netherite_chestplate 16*37
    }

    /// Non-damageable ids resolve to None (blocks, materials).
    #[test]
    fn non_damageable_is_none() {
        assert_eq!(max_damage_for(1), None); // stone
        assert_eq!(max_damage_for(7), None); // planks
        assert_eq!(max_damage_for(320), None); // blaze_powder
        assert_eq!(max_damage_for(u16::MAX), None); // out of range
    }

    /// Every registry row whose name shape marks it damageable in
    /// MC must resolve to a table row - catches future registry
    /// additions that miss the name mapping.
    #[test]
    fn registry_damageable_names_all_resolve() {
        let suffixes = [
            "_sword",
            "_pickaxe",
            "_shovel",
            "_hoe",
            "_helmet",
            "_chestplate",
            "_leggings",
            "_boots",
        ];
        let exact = ["shears", "bow", "crossbow", "fishing_rod", "shield"];
        let mut missing: Vec<&str> = Vec::new();
        for it in ITEM_TYPES.iter() {
            let n = it.name;
            let damage_shaped = suffixes.iter().any(|s| n.ends_with(s))
                || exact.contains(&n)
                || n == "flint_and_steel"
                // legacy `<kind>_<tier>` prefixes
                || legacy_mc_name(n).is_some_and(|mc| {
                    suffixes.iter().any(|s| mc.ends_with(s))
                });
            if damage_shaped && max_damage_for(it.id).is_none() {
                missing.push(n);
            }
        }
        assert!(missing.is_empty(), "unresolved damageable: {missing:?}");
    }

    /// Per-action wear values (Tool.damage_per_block /
    /// Weapon.item_damage_per_attack component defaults).
    #[test]
    fn wear_amounts_match_components() {
        // sword_iron: Tool(2) + Weapon(1).
        assert_eq!(mine_wear_amount(25), 2);
        assert_eq!(attack_wear_amount(25), 1);
        // pickaxe_iron / iron_axe / hoe: Tool(1) + Weapon(2).
        assert_eq!(mine_wear_amount(18), 1);
        assert_eq!(attack_wear_amount(18), 2);
        assert_eq!(mine_wear_amount(873), 1); // iron_axe
        assert_eq!(attack_wear_amount(873), 2);
        assert_eq!(mine_wear_amount(334), 1); // iron_hoe
        // shears: Tool(1), no Weapon.
        assert_eq!(mine_wear_amount(640), 1);
        assert_eq!(attack_wear_amount(640), 0);
        // flint+steel: damageable but neither Tool nor Weapon.
        assert_eq!(mine_wear_amount(48), 0);
        assert_eq!(attack_wear_amount(48), 0);
        // armor / plain blocks: nothing.
        assert_eq!(mine_wear_amount(887), 0);
        assert_eq!(attack_wear_amount(887), 0);
        assert_eq!(mine_wear_amount(1), 0);
        assert_eq!(attack_wear_amount(1), 0);
    }

    /// doHurtEquipment: (int)max(1.0, damage / 4.0).
    #[test]
    fn armor_wear_amount_matches_do_hurt_equipment() {
        assert_eq!(armor_wear_amount(0.0), 0);
        assert_eq!(armor_wear_amount(-3.0), 0);
        assert_eq!(armor_wear_amount(0.5), 1);
        assert_eq!(armor_wear_amount(3.9), 1);
        assert_eq!(armor_wear_amount(4.0), 1);
        assert_eq!(armor_wear_amount(7.9), 1); // trunc(1.975) = 1
        assert_eq!(armor_wear_amount(8.0), 2);
        assert_eq!(armor_wear_amount(10.0), 2); // trunc(2.5) = 2
        assert_eq!(armor_wear_amount(20.0), 5);
    }

    #[test]
    fn hurt_and_break_accumulates_and_breaks_at_threshold() {
        // flint_and_steel: max 64.
        let mut slot = Some(ItemStack::new(48, 1));
        for i in 1..64u16 {
            assert!(!hurt_and_break(&mut slot, 1, false));
            assert_eq!(slot.as_ref().unwrap().components.damage, i);
        }
        // 64th point: damage reaches max -> broke, slot clears.
        assert!(hurt_and_break(&mut slot, 1, false));
        assert!(slot.is_none());
    }

    #[test]
    fn hurt_and_break_clamps_overshoot_to_max() {
        // gold pickaxe (max 32): one big hit past the cap still
        // breaks exactly once, damage clamped like setDamageValue.
        let mut slot = Some(ItemStack::new(154, 1));
        assert!(hurt_and_break(&mut slot, 1000, false));
        assert!(slot.is_none());
    }

    #[test]
    fn hurt_and_break_creative_is_noop() {
        let mut slot = Some(ItemStack::new(48, 1));
        assert!(!hurt_and_break(&mut slot, 5, true));
        assert_eq!(slot.as_ref().unwrap().components.damage, 0);
    }

    #[test]
    fn hurt_and_break_non_damageable_is_noop() {
        let mut slot = Some(ItemStack::new(1, 16)); // stone
        assert!(!hurt_and_break(&mut slot, 5, false));
        assert_eq!(slot.as_ref().unwrap().components.damage, 0);
        // Empty slot + zero amount are no-ops too.
        let mut empty: Option<ItemStack> = None;
        assert!(!hurt_and_break(&mut empty, 5, false));
        let mut tool = Some(ItemStack::new(18, 1));
        assert!(!hurt_and_break(&mut tool, 0, false));
        assert_eq!(tool.as_ref().unwrap().components.damage, 0);
    }
}
