//! W5.2b regression suite: flint+steel durability bookkeeping.
//!
//! `Voxel::item_durability` is a parallel `HashMap<usize, u16>`
//! keyed by inventory slot index. Tests below drive the slot
//! mutations directly so we don't depend on the full RMB pipeline
//! (which requires a renderer + a world).

use infengine_voxel::BlockId;

use crate::block::{FLINT_AND_STEEL_ID, FLINT_AND_STEEL_MAX_USES};
use crate::inventory::{SlotKind, SplitMode};
use crate::ui::InvAction;
use crate::Voxel;

/// 64 RMB ignitions must burn a fresh flint+steel to empty.
#[test]
fn sixty_four_uses_empties_a_fresh_flint() {
    let mut v = Voxel::new();
    // Drop a fresh flint+steel into an empty slot (slot 10 is well
    // outside the curated starter hotbar).
    let slot = 10usize;
    v.inventory[slot] = Some((BlockId(FLINT_AND_STEEL_ID), 1));
    v.item_durability
        .insert(slot, FLINT_AND_STEEL_MAX_USES);
    v.selected_slot = slot;
    // Drive 63 uses; the slot must still hold the tool.
    for _ in 0..(FLINT_AND_STEEL_MAX_USES - 1) {
        v.consume_flint_and_steel_use();
    }
    assert!(v.inventory[slot].is_some(), "should still hold the tool");
    assert_eq!(v.item_durability.get(&slot).copied(), Some(1));
    // 64th use empties the slot + clears the map entry.
    v.consume_flint_and_steel_use();
    assert!(
        v.inventory[slot].is_none(),
        "64th use should empty the slot"
    );
    assert!(
        v.item_durability.get(&slot).is_none(),
        "64th use should remove the durability tracker"
    );
}

/// `pickup(FLINT_AND_STEEL_ID)` lands a fresh stack AND mints a
/// MAX_USES durability entry for the new slot.
#[test]
fn pickup_mints_max_durability_for_flint() {
    let mut v = Voxel::new();
    // Wipe inventory so the pickup lands in slot 0.
    v.inventory = [None; crate::inventory::INVENTORY_SLOTS];
    v.item_durability.clear();
    v.pickup(BlockId(FLINT_AND_STEEL_ID));
    assert_eq!(v.inventory[0], Some((BlockId(FLINT_AND_STEEL_ID), 1)));
    assert_eq!(
        v.item_durability.get(&0).copied(),
        Some(FLINT_AND_STEEL_MAX_USES),
    );
}

/// `pickup` of a non-tool block does NOT touch the durability map.
#[test]
fn pickup_does_not_mint_durability_for_other_blocks() {
    let mut v = Voxel::new();
    v.inventory = [None; crate::inventory::INVENTORY_SLOTS];
    v.item_durability.clear();
    v.pickup(BlockId(1)); // stone
    assert_eq!(v.inventory[0], Some((BlockId(1), 1)));
    assert!(v.item_durability.is_empty());
}

/// Drag a partially-used flint+steel into an empty slot. The
/// durability follows the stack to the new slot index.
#[test]
fn drag_moves_durability_with_stack() {
    let mut v = Voxel::new();
    v.inventory = [None; crate::inventory::INVENTORY_SLOTS];
    v.item_durability.clear();
    // Slot 5 holds a half-burned flint+steel (32 uses left).
    let from = 5usize;
    let to = 20usize;
    v.inventory[from] = Some((BlockId(FLINT_AND_STEEL_ID), 1));
    v.item_durability.insert(from, 32);
    v.apply_inv_action(InvAction::Drag {
        from: SlotKind::Inv(from),
        to: SlotKind::Inv(to),
        split: SplitMode::All,
    });
    assert!(v.inventory[from].is_none(), "from slot should be empty");
    assert_eq!(v.inventory[to], Some((BlockId(FLINT_AND_STEEL_ID), 1)));
    assert!(
        v.item_durability.get(&from).is_none(),
        "old slot's durability entry should be cleared"
    );
    assert_eq!(
        v.item_durability.get(&to).copied(),
        Some(32),
        "durability must follow the stack"
    );
}

/// Swap a flint+steel with an unrelated stack. The flint+steel
/// moves to the destination slot and its durability follows;
/// the unrelated stack lands in the source slot (no durability
/// for it).
#[test]
fn drag_swap_with_unrelated_preserves_flint_durability() {
    let mut v = Voxel::new();
    v.inventory = [None; crate::inventory::INVENTORY_SLOTS];
    v.item_durability.clear();
    let a = 5usize;
    let b = 20usize;
    v.inventory[a] = Some((BlockId(FLINT_AND_STEEL_ID), 1));
    v.inventory[b] = Some((BlockId(1), 16)); // stone stack
    v.item_durability.insert(a, 10);
    v.apply_inv_action(InvAction::Drag {
        from: SlotKind::Inv(a),
        to: SlotKind::Inv(b),
        split: SplitMode::All,
    });
    // Stone landed at slot a, flint+steel landed at slot b.
    assert_eq!(v.inventory[a], Some((BlockId(1), 16)));
    assert_eq!(v.inventory[b], Some((BlockId(FLINT_AND_STEEL_ID), 1)));
    // Durability follows the flint+steel.
    assert!(v.item_durability.get(&a).is_none());
    assert_eq!(v.item_durability.get(&b).copied(), Some(10));
}

/// `consume_flint_and_steel_use` falls back to MAX_USES when the
/// durability map has no entry for the selected slot. This
/// matches a freshly-pickup'd tool that pre-dates W5.2b on an
/// older save - the first use treats it as a brand-new stack.
#[test]
fn consume_use_without_map_entry_falls_back_to_max() {
    let mut v = Voxel::new();
    v.inventory = [None; crate::inventory::INVENTORY_SLOTS];
    v.item_durability.clear();
    let slot = 3usize;
    v.inventory[slot] = Some((BlockId(FLINT_AND_STEEL_ID), 1));
    v.selected_slot = slot;
    v.consume_flint_and_steel_use();
    // First use brings the counter from MAX -> MAX-1.
    assert_eq!(
        v.item_durability.get(&slot).copied(),
        Some(FLINT_AND_STEEL_MAX_USES - 1),
    );
}
