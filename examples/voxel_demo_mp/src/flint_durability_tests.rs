//! W5.2b/W466 regression suite: flint+steel durability bookkeeping.
//!
//! W466 folded the old parallel `HashMap<usize, u16>` durability
//! map into the inventory slots themselves: each `ItemStack`
//! carries `components.damage`, counting UP from 0 until
//! `FLINT_AND_STEEL_MAX_USES`. Tests below drive the slot
//! mutations directly so we don't depend on the full RMB pipeline
//! (which requires a renderer + a world).
//!
//! W471 extends the suite to the generalized wear rules: the
//! `wear_selected_stack` funnel (MC `hurtAndBreak`), armor wear on
//! player damage (`doHurtEquipment` + the bypasses-armor kind
//! set), and the server-side `wear_client_selected` applier that
//! keeps a remote player's authoritative inventory wearing exactly
//! once per validated break / hit.

use infengine_voxel::BlockId;

use crate::Voxel;
use crate::block::{FLINT_AND_STEEL_ID, FLINT_AND_STEEL_MAX_USES};
use crate::inventory::{SlotKind, SplitMode};
use crate::item::{ItemComponents, ItemStack};
use crate::ui::InvAction;

/// Build a flint+steel stack with `damage` wear already on it.
fn worn_flint(damage: u16) -> ItemStack {
    ItemStack {
        id: FLINT_AND_STEEL_ID,
        count: 1,
        components: ItemComponents {
            damage,
            ..Default::default()
        },
    }
}

/// 64 RMB ignitions must burn a fresh flint+steel to empty.
#[test]
fn sixty_four_uses_empties_a_fresh_flint() {
    let mut v = Voxel::new();
    // Drop a fresh flint+steel into an empty slot (slot 10 is well
    // outside the curated starter hotbar).
    let slot = 10usize;
    v.inventory[slot] = Some(ItemStack::new(FLINT_AND_STEEL_ID, 1));
    v.selected_slot = slot;
    // Drive 63 uses; the slot must still hold the tool.
    for _ in 0..(FLINT_AND_STEEL_MAX_USES - 1) {
        v.consume_flint_and_steel_use();
    }
    assert!(v.inventory[slot].is_some(), "should still hold the tool");
    assert_eq!(
        v.inventory[slot].as_ref().unwrap().components.damage,
        FLINT_AND_STEEL_MAX_USES - 1,
        "63 uses = damage 63 (1 use left)"
    );
    // 64th use empties the slot.
    v.consume_flint_and_steel_use();
    assert!(
        v.inventory[slot].is_none(),
        "64th use should empty the slot"
    );
}

/// `pickup(FLINT_AND_STEEL_ID)` lands a fresh stack with empty
/// components - damage 0 IS the old MAX_USES durability mint.
#[test]
fn pickup_mints_fresh_components_for_flint() {
    let mut v = Voxel::new();
    // Wipe inventory so the pickup lands in slot 0.
    v.inventory = std::array::from_fn(|_| None);
    v.pickup(BlockId(FLINT_AND_STEEL_ID));
    assert_eq!(
        v.inventory[0],
        Some(ItemStack::new(FLINT_AND_STEEL_ID, 1))
    );
    assert!(
        v.inventory[0].as_ref().unwrap().components.is_empty(),
        "fresh pickup starts at damage 0 = full durability"
    );
}

/// A fresh pickup must NOT merge into a worn flint+steel stack -
/// that would silently inherit the wear.
#[test]
fn pickup_does_not_merge_into_worn_stack() {
    let mut v = Voxel::new();
    v.inventory = std::array::from_fn(|_| None);
    v.inventory[0] = Some(worn_flint(32));
    v.pickup(BlockId(FLINT_AND_STEEL_ID));
    // Worn stack untouched; fresh one claimed the next slot.
    assert_eq!(v.inventory[0], Some(worn_flint(32)));
    assert_eq!(
        v.inventory[1],
        Some(ItemStack::new(FLINT_AND_STEEL_ID, 1))
    );
}

/// Drag a partially-used flint+steel into an empty slot. The
/// damage travels inside the stack to the new slot index.
#[test]
fn drag_moves_damage_with_stack() {
    let mut v = Voxel::new();
    v.inventory = std::array::from_fn(|_| None);
    // Slot 5 holds a half-burned flint+steel (damage 32).
    let from = 5usize;
    let to = 20usize;
    v.inventory[from] = Some(worn_flint(32));
    v.apply_inv_action(InvAction::Drag {
        from: SlotKind::Inv(from),
        to: SlotKind::Inv(to),
        split: SplitMode::All,
    });
    assert!(v.inventory[from].is_none(), "from slot should be empty");
    assert_eq!(
        v.inventory[to],
        Some(worn_flint(32)),
        "damage must travel with the stack"
    );
}

/// Swap a flint+steel with an unrelated stack. The flint+steel
/// moves to the destination slot with its damage; the unrelated
/// stack lands in the source slot (plain components).
#[test]
fn drag_swap_with_unrelated_preserves_flint_damage() {
    let mut v = Voxel::new();
    v.inventory = std::array::from_fn(|_| None);
    let a = 5usize;
    let b = 20usize;
    v.inventory[a] = Some(worn_flint(10));
    v.inventory[b] = Some(ItemStack::new(1, 16)); // stone stack
    v.apply_inv_action(InvAction::Drag {
        from: SlotKind::Inv(a),
        to: SlotKind::Inv(b),
        split: SplitMode::All,
    });
    // Stone landed at slot a, flint+steel landed at slot b.
    assert_eq!(v.inventory[a], Some(ItemStack::new(1, 16)));
    assert_eq!(v.inventory[b], Some(worn_flint(10)));
}

/// Two flint+steels with DIFFERENT damage never merge on drag -
/// they swap instead (`same_stackable` requires equal components).
#[test]
fn drag_does_not_merge_differently_worn_flints() {
    let mut v = Voxel::new();
    v.inventory = std::array::from_fn(|_| None);
    let a = 5usize;
    let b = 20usize;
    v.inventory[a] = Some(worn_flint(10));
    v.inventory[b] = Some(worn_flint(40));
    v.apply_inv_action(InvAction::Drag {
        from: SlotKind::Inv(a),
        to: SlotKind::Inv(b),
        split: SplitMode::All,
    });
    // Swapped, both wear values intact.
    assert_eq!(v.inventory[a], Some(worn_flint(40)));
    assert_eq!(v.inventory[b], Some(worn_flint(10)));
}

/// `consume_flint_and_steel_use` on a fresh stack registers the
/// first use as damage 1 - the old map-less fallback ("no entry =
/// full durability") is now the default-components case.
#[test]
fn first_use_on_fresh_stack_sets_damage_one() {
    let mut v = Voxel::new();
    v.inventory = std::array::from_fn(|_| None);
    let slot = 3usize;
    v.inventory[slot] = Some(ItemStack::new(FLINT_AND_STEEL_ID, 1));
    v.selected_slot = slot;
    v.consume_flint_and_steel_use();
    // First use brings the counter from 0 -> 1 (MAX-1 uses left).
    assert_eq!(
        v.inventory[slot].as_ref().unwrap().components.damage,
        1,
    );
}

// ------------------------------------------------------------------
// W471: generalized wear (tools / weapons / armor / MP applier).
// ------------------------------------------------------------------

const IRON_PICKAXE: u16 = 18; // pickaxe_iron, max_damage 250
const IRON_SWORD: u16 = 25; // sword_iron, max_damage 250
const IRON_HELMET: u16 = 887; // max_damage 165
const IRON_BOOTS: u16 = 890; // max_damage 195

/// One survival block break = `Tool.damage_per_block` wear on the
/// SELECTED stack only (digger tools 1, swords 2).
#[test]
fn wear_selected_applies_mine_amounts_to_selected_only() {
    let mut v = Voxel::new();
    v.inventory = std::array::from_fn(|_| None);
    v.inventory[2] = Some(ItemStack::new(IRON_PICKAXE, 1));
    v.inventory[3] = Some(ItemStack::new(IRON_SWORD, 1));
    v.selected_slot = 2;
    let wear = crate::item::durability::mine_wear_amount(IRON_PICKAXE);
    assert_eq!(wear, 1);
    v.wear_selected_stack(wear);
    assert_eq!(v.inventory[2].as_ref().unwrap().components.damage, 1);
    // Neighbour slot untouched.
    assert_eq!(v.inventory[3].as_ref().unwrap().components.damage, 0);
    // Sword mines at 2 per block.
    v.selected_slot = 3;
    let wear = crate::item::durability::mine_wear_amount(IRON_SWORD);
    assert_eq!(wear, 2);
    v.wear_selected_stack(wear);
    assert_eq!(v.inventory[3].as_ref().unwrap().components.damage, 2);
}

/// Melee wear values (`Weapon.item_damage_per_attack`): sword 1
/// per hit, digger tool 2 per hit.
#[test]
fn attack_wear_sword_one_digger_two() {
    assert_eq!(crate::item::durability::attack_wear_amount(IRON_SWORD), 1);
    assert_eq!(
        crate::item::durability::attack_wear_amount(IRON_PICKAXE),
        2
    );
    // Bare block: nothing.
    assert_eq!(crate::item::durability::attack_wear_amount(1), 0);
}

/// The selected tool breaks exactly at max damage and the slot
/// clears (MC `isBroken` >= threshold + `shrink(1)`).
#[test]
fn wear_selected_breaks_tool_at_threshold() {
    let mut v = Voxel::new();
    v.inventory = std::array::from_fn(|_| None);
    v.inventory[0] = Some(ItemStack::new(IRON_PICKAXE, 1));
    v.selected_slot = 0;
    if let Some(st) = v.inventory[0].as_mut() {
        st.components.damage = 249; // one point left of 250
    }
    v.wear_selected_stack(1);
    assert!(v.inventory[0].is_none(), "tool must break at max damage");
}

/// Creative (`instabuild`) never wears - the MC
/// infinite-materials gate in `processDurabilityChange`.
#[test]
fn wear_selected_creative_is_noop() {
    let mut v = Voxel::new();
    v.inventory = std::array::from_fn(|_| None);
    v.inventory[0] = Some(ItemStack::new(IRON_PICKAXE, 1));
    v.selected_slot = 0;
    v.abilities.instabuild = true;
    v.wear_selected_stack(1);
    assert_eq!(v.inventory[0].as_ref().unwrap().components.damage, 0);
    // Flint route shares the gate.
    v.inventory[0] = Some(ItemStack::new(FLINT_AND_STEEL_ID, 1));
    v.consume_flint_and_steel_use();
    assert_eq!(v.inventory[0].as_ref().unwrap().components.damage, 0);
}

/// Armor wear on a mitigated hit: every worn damageable piece
/// takes `max(1, floor(damage / 4))` of the PRE-mitigation
/// amount (`Player.hurtArmor` -> `doHurtEquipment`).
#[test]
fn armor_wears_on_generic_damage() {
    let mut v = Voxel::new();
    v.armor_slots[0] = Some(ItemStack::new(IRON_HELMET, 1));
    v.armor_slots[3] = Some(ItemStack::new(IRON_BOOTS, 1));
    crate::player::apply_damage_kind(&mut v, 8.0, crate::DamageKind::Generic);
    assert_eq!(v.armor_slots[0].as_ref().unwrap().components.damage, 2);
    assert_eq!(v.armor_slots[3].as_ref().unwrap().components.damage, 2);
    // Small hit still costs the 1-point minimum.
    crate::player::apply_damage_kind(&mut v, 1.0, crate::DamageKind::Generic);
    assert_eq!(v.armor_slots[0].as_ref().unwrap().components.damage, 3);
}

/// Damage kinds in the MC `bypasses_armor` tag (fall, drown,
/// magic, void) wear NO armor - same kind set that skips the
/// mitigation math.
#[test]
fn armor_does_not_wear_on_bypassing_kinds() {
    let mut v = Voxel::new();
    v.armor_slots[0] = Some(ItemStack::new(IRON_HELMET, 1));
    // Small per-kind amounts so the sum stays clear of the death
    // path (a fresh Voxel has no world for a respawn).
    crate::player::apply_damage_kind(&mut v, 4.0, crate::DamageKind::Fall);
    crate::player::apply_damage_kind(&mut v, 4.0, crate::DamageKind::Drown);
    crate::player::apply_damage_kind(&mut v, 3.0, crate::DamageKind::Magic);
    assert_eq!(
        v.armor_slots[0].as_ref().unwrap().components.damage,
        0,
        "fall / drown / magic damage must not wear armor"
    );
}

/// A worn-out armor piece disappears from its slot when it breaks.
#[test]
fn armor_piece_breaks_off_the_body() {
    let mut v = Voxel::new();
    v.armor_slots[0] = Some(ItemStack::new(IRON_HELMET, 1));
    if let Some(st) = v.armor_slots[0].as_mut() {
        st.components.damage = 164; // 165 max - one hit left
    }
    crate::player::apply_damage_kind(&mut v, 2.0, crate::DamageKind::Generic);
    assert!(v.armor_slots[0].is_none(), "helmet must break off");
}

/// W471 MP: the server-side applier wears the authoritative
/// `ClientInv` slot named by the sender's pose exactly once per
/// validated action, and queues the confirming snapshot.
#[test]
fn wear_client_selected_wears_authoritative_slot_once() {
    use std::collections::{HashMap, HashSet};
    let mut inventories: HashMap<u32, crate::net::ClientInv> = HashMap::new();
    let mut pending: HashSet<u32> = HashSet::new();
    let cid = 7u32;
    let mut inv = crate::net::ClientInv::starter();
    inv.inventory[4] = Some(ItemStack::new(IRON_PICKAXE, 1));
    inventories.insert(cid, inv);
    crate::net::wear_client_selected(
        &mut inventories,
        &mut pending,
        cid,
        4,
        crate::item::durability::mine_wear_amount,
    );
    let inv = inventories.get(&cid).unwrap();
    assert_eq!(
        inv.inventory[4].as_ref().unwrap().components.damage,
        1,
        "one validated break = exactly one wear point"
    );
    assert!(pending.contains(&cid), "wear must queue a snapshot");
}

/// The `SEL_NONE` pose sentinel (creative sender / unknown pose)
/// skips authoritative wear; so does a non-tool in the slot.
#[test]
fn wear_client_selected_respects_sentinel_and_non_tools() {
    use std::collections::{HashMap, HashSet};
    let mut inventories: HashMap<u32, crate::net::ClientInv> = HashMap::new();
    let mut pending: HashSet<u32> = HashSet::new();
    let cid = 7u32;
    let mut inv = crate::net::ClientInv::starter();
    inv.inventory[4] = Some(ItemStack::new(IRON_PICKAXE, 1));
    inv.inventory[5] = Some(ItemStack::new(1, 16)); // stone stack
    inventories.insert(cid, inv);
    // Creative sentinel: untouched.
    crate::net::wear_client_selected(
        &mut inventories,
        &mut pending,
        cid,
        crate::net::SEL_NONE,
        crate::item::durability::mine_wear_amount,
    );
    // Non-tool slot: untouched.
    crate::net::wear_client_selected(
        &mut inventories,
        &mut pending,
        cid,
        5,
        crate::item::durability::mine_wear_amount,
    );
    // Unknown client id: no panic, no effect.
    crate::net::wear_client_selected(
        &mut inventories,
        &mut pending,
        99,
        4,
        crate::item::durability::mine_wear_amount,
    );
    let inv = inventories.get(&cid).unwrap();
    assert_eq!(inv.inventory[4].as_ref().unwrap().components.damage, 0);
    assert_eq!(inv.inventory[5], Some(ItemStack::new(1, 16)));
    assert!(pending.is_empty(), "no wear -> no snapshot traffic");
}
