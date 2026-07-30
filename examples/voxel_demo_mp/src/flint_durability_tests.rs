//! W5.2b/W466 regression suite: flint+steel durability bookkeeping.
//!
//! W466 folded the old parallel `HashMap<usize, u16>` durability
//! map into the inventory slots themselves: each `ItemStack`
//! carries `components.damage`, counting UP from 0 until
//! `FLINT_AND_STEEL_MAX_USES`. Tests below drive the slot
//! mutations directly so we don't depend on the full RMB pipeline
//! (which requires a renderer + a world).

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
