//! Unit tests for `block::chest`: chest container construction,
//! per-action mutations against both grids, drain-on-break, and
//! recover-on-close behaviour. Pulled out of `chest.rs` so the
//! module body stays under the 500-LOC soft cap.

use super::{
    CHEST_SLOTS, apply_chest_action, apply_player_action, drain_drops, new_chest,
    recover_held_on_close,
};
use crate::container::Container;
use crate::container::ui::GridAction;
use crate::item::ItemStack;

fn assert_slot(c: &Container, idx: usize, expected: Option<ItemStack>) {
    assert_eq!(c.slot(idx), expected);
}

#[test]
fn new_chest_is_27_empty_slots() {
    let c = new_chest();
    assert_eq!(c.len(), CHEST_SLOTS);
    assert!(c.is_empty());
}

#[test]
fn take_pulls_full_stack_onto_cursor() {
    let mut chest = new_chest();
    chest.set_slot(3, Some(ItemStack::new(1, 12)));
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Take(3));
    assert_eq!(held, Some(ItemStack::new(1, 12)));
    assert_slot(&chest, 3, None);
}

#[test]
fn take_half_rounds_up() {
    let mut chest = new_chest();
    chest.set_slot(0, Some(ItemStack::new(1, 7)));
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::TakeHalf(0));
    // 7 / 2 = 4 (rounded up); slot left with 3.
    assert_eq!(held, Some(ItemStack::new(1, 4)));
    assert_slot(&chest, 0, Some(ItemStack::new(1, 3)));
}

#[test]
fn place_full_stack_into_empty_slot() {
    let mut chest = new_chest();
    let mut held: Option<ItemStack> = Some(ItemStack::new(1, 9));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(5));
    assert_eq!(held, None);
    assert_slot(&chest, 5, Some(ItemStack::new(1, 9)));
}

#[test]
fn place_one_lifts_a_single_item() {
    let mut chest = new_chest();
    let mut held: Option<ItemStack> = Some(ItemStack::new(1, 5));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::PlaceOne(2));
    // 1 placed, cursor still has 4.
    assert_eq!(held, Some(ItemStack::new(1, 4)));
    assert_slot(&chest, 2, Some(ItemStack::new(1, 1)));
}

#[test]
fn place_full_stack_swaps_with_different_item() {
    let mut chest = new_chest();
    chest.set_slot(0, Some(ItemStack::new(2, 4)));
    let mut held: Option<ItemStack> = Some(ItemStack::new(1, 9));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(0));
    // Items 1 & 2 swap.
    assert_eq!(held, Some(ItemStack::new(2, 4)));
    assert_slot(&chest, 0, Some(ItemStack::new(1, 9)));
}

#[test]
fn place_same_id_tops_up_partial() {
    let mut chest = new_chest();
    chest.set_slot(1, Some(ItemStack::new(1, 60)));
    // Stone caps at 64, so a 5-stack incoming tops up to 64
    // and leaves 1 on the cursor.
    let mut held: Option<ItemStack> = Some(ItemStack::new(1, 5));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(1));
    assert_eq!(held, Some(ItemStack::new(1, 1)));
    assert_slot(&chest, 1, Some(ItemStack::new(1, 64)));
}

#[test]
fn transfer_from_chest_moves_to_player_inv() {
    let mut chest = new_chest();
    chest.set_slot(2, Some(ItemStack::new(1, 10)));
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Transfer(2));
    assert_eq!(held, None);
    assert_slot(&chest, 2, None);
    assert_eq!(inv[0], Some(ItemStack::new(1, 10)));
}

#[test]
fn transfer_from_player_moves_to_chest() {
    let mut chest = new_chest();
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    inv[4] = Some(ItemStack::new(1, 8));
    apply_player_action(&mut inv, &mut chest, &mut held, GridAction::Transfer(4));
    assert_eq!(inv[4], None);
    // First free chest slot gets the stack.
    assert_slot(&chest, 0, Some(ItemStack::new(1, 8)));
}

#[test]
fn player_take_pulls_from_inv() {
    let mut chest = new_chest();
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    inv[7] = Some(ItemStack::new(1, 20));
    apply_player_action(&mut inv, &mut chest, &mut held, GridAction::Take(7));
    assert_eq!(held, Some(ItemStack::new(1, 20)));
    assert_eq!(inv[7], None);
}

#[test]
fn player_place_one_drops_a_single_item() {
    let mut chest = new_chest();
    let mut held: Option<ItemStack> = Some(ItemStack::new(1, 3));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_player_action(&mut inv, &mut chest, &mut held, GridAction::PlaceOne(0));
    assert_eq!(held, Some(ItemStack::new(1, 2)));
    assert_eq!(inv[0], Some(ItemStack::new(1, 1)));
}

#[test]
fn transfer_full_chest_leaves_overflow_in_source() {
    // Player inv full of stone; transfer 30 stone from chest
    // back into player - chest stack should partial-deplete
    // since topping up 9 partial 64-stacks may not absorb all.
    let mut chest = new_chest();
    chest.set_slot(0, Some(ItemStack::new(1, 30)));
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    // Pre-fill every player slot with a different id so the
    // transfer fully fails.
    for slot in inv.iter_mut() {
        *slot = Some(ItemStack::new(2, 64));
    }
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Transfer(0));
    // Nothing moved; chest still has 30 stone.
    assert_slot(&chest, 0, Some(ItemStack::new(1, 30)));
}

#[test]
fn place_one_starts_a_new_stack_in_empty_slot() {
    let mut chest = new_chest();
    let mut held: Option<ItemStack> = Some(ItemStack::new(5, 1));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::PlaceOne(10));
    assert_eq!(held, None);
    assert_slot(&chest, 10, Some(ItemStack::new(5, 1)));
}

#[test]
fn take_on_empty_slot_is_noop() {
    let mut chest = new_chest();
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Take(0));
    assert_eq!(held, None);
}

#[test]
fn place_with_no_held_stack_is_noop() {
    let mut chest = new_chest();
    chest.set_slot(0, Some(ItemStack::new(1, 4)));
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(0));
    assert_eq!(held, None);
    assert_slot(&chest, 0, Some(ItemStack::new(1, 4)));
}

#[test]
fn drain_drops_empty_chest_yields_no_drops() {
    let chest = new_chest();
    assert!(drain_drops(&chest).is_empty());
}

#[test]
fn drain_drops_expands_each_stack_to_per_item_drops() {
    let mut chest = new_chest();
    chest.set_slot(0, Some(ItemStack::new(1, 3)));
    chest.set_slot(5, Some(ItemStack::new(2, 1)));
    chest.set_slot(26, Some(ItemStack::new(1, 2)));
    let drops = drain_drops(&chest);
    // 3 + 1 + 2 = 6 drops total.
    assert_eq!(drops.len(), 6);
    // 5 of id 1 + 1 of id 2.
    let ones = drops.iter().filter(|d| **d == 1).count();
    let twos = drops.iter().filter(|d| **d == 2).count();
    assert_eq!(ones, 5);
    assert_eq!(twos, 1);
}

#[test]
fn recover_held_no_stack_no_drops() {
    let mut chest = new_chest();
    let drops = recover_held_on_close(Some(&mut chest), None);
    assert!(drops.is_empty());
}

#[test]
fn recover_held_lands_in_empty_chest() {
    let mut chest = new_chest();
    let drops = recover_held_on_close(Some(&mut chest), Some(ItemStack::new(1, 9)));
    assert!(drops.is_empty(), "empty chest should absorb the stack");
    assert_eq!(chest.slot(0), Some(ItemStack::new(1, 9)));
}

#[test]
fn recover_held_overflow_falls_as_drops() {
    // 27 slots * 64 stack of stone = 1728 capacity. Pre-fill
    // every slot so nothing fits, then drop a held stack of
    // 3 -> the entire held stack returns as floor drops.
    let mut chest = new_chest();
    for i in 0..CHEST_SLOTS {
        chest.set_slot(i, Some(ItemStack::new(1, 64)));
    }
    let drops = recover_held_on_close(Some(&mut chest), Some(ItemStack::new(1, 3)));
    assert_eq!(drops, vec![1, 1, 1]);
}

#[test]
fn recover_held_no_chest_dumps_everything_as_drops() {
    // Chest was already broken / gone (None) - the entire
    // held stack lands on the floor.
    let drops = recover_held_on_close(None, Some(ItemStack::new(5, 4)));
    assert_eq!(drops, vec![5, 5, 5, 5]);
}

// ---------------------------------------------------------------
// W467: components ride through the chest UI ops.
// ---------------------------------------------------------------

/// Shorthand: a worn single-item stack.
fn worn(id: u16, count: u8, damage: u16) -> ItemStack {
    let mut st = ItemStack::new(id, count);
    st.components.damage = damage;
    st
}

#[test]
fn take_and_place_round_trip_keeps_damage() {
    // Put a worn tool into a chest via the cursor, take it back:
    // the damage must survive both hops.
    let mut chest = new_chest();
    let mut held: Option<ItemStack> = Some(worn(48, 1, 20));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(4));
    assert_eq!(held, None);
    assert_eq!(chest.slot(4), Some(worn(48, 1, 20)));
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Take(4));
    assert_eq!(held, Some(worn(48, 1, 20)));
    assert_slot(&chest, 4, None);
}

#[test]
fn place_fresh_onto_worn_swaps_instead_of_merging() {
    // MC rule: a fresh stack full-placed onto a same-id worn slot
    // swaps the two (they are not stackable together).
    let mut chest = new_chest();
    chest.set_slot(0, Some(worn(48, 1, 7)));
    let mut held: Option<ItemStack> = Some(ItemStack::new(48, 1));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(0));
    assert_eq!(held, Some(worn(48, 1, 7)));
    assert_eq!(chest.slot(0), Some(ItemStack::new(48, 1)));
}

#[test]
fn transfer_to_player_inv_keeps_damage() {
    // Shift-click a worn tool out of the chest: it must not
    // count-merge into the fresh same-id stack in the inventory.
    let mut chest = new_chest();
    chest.set_slot(2, Some(worn(48, 1, 33)));
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    inv[0] = Some(ItemStack::new(48, 1));
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Transfer(2));
    assert_slot(&chest, 2, None);
    assert_eq!(inv[0], Some(ItemStack::new(48, 1)));
    assert_eq!(inv[1], Some(worn(48, 1, 33)));
}

#[test]
fn transfer_from_player_keeps_damage() {
    // Shift-click a worn tool into the chest: same rule, chest
    // side.
    let mut chest = new_chest();
    chest.set_slot(0, Some(ItemStack::new(48, 1)));
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    inv[3] = Some(worn(48, 1, 12));
    apply_player_action(&mut inv, &mut chest, &mut held, GridAction::Transfer(3));
    assert_eq!(inv[3], None);
    assert_slot(&chest, 0, Some(ItemStack::new(48, 1)));
    assert_slot(&chest, 1, Some(worn(48, 1, 12)));
}

#[test]
fn take_half_of_worn_stack_wears_both_halves() {
    let mut chest = new_chest();
    chest.set_slot(0, Some(worn(9, 7, 5)));
    let mut held: Option<ItemStack> = None;
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::TakeHalf(0));
    // 4 taken (rounded up), 3 remain - both sides worn by 5.
    assert_eq!(held, Some(worn(9, 4, 5)));
    assert_slot(&chest, 0, Some(worn(9, 3, 5)));
}

#[test]
fn recover_held_keeps_components_in_chest() {
    // Closing the UI with a worn tool on the cursor drops it back
    // into the chest with its damage intact.
    let mut chest = new_chest();
    let drops = recover_held_on_close(Some(&mut chest), Some(worn(48, 1, 9)));
    assert!(drops.is_empty());
    assert_eq!(chest.slot(0), Some(worn(48, 1, 9)));
}

#[test]
fn recover_held_partial_overflow_splits() {
    // Chest has room for 4 more stone (slot 0 has 60 of 64,
    // rest are full of a different id). Held 10 stone ->
    // 4 absorbed, 6 returned as drops.
    let mut chest = new_chest();
    chest.set_slot(0, Some(ItemStack::new(1, 60)));
    for i in 1..CHEST_SLOTS {
        chest.set_slot(i, Some(ItemStack::new(2, 64)));
    }
    let drops = recover_held_on_close(Some(&mut chest), Some(ItemStack::new(1, 10)));
    assert_eq!(drops.len(), 6);
    assert!(drops.iter().all(|d| *d == 1));
    assert_eq!(chest.slot(0), Some(ItemStack::new(1, 64)));
}

#[test]
fn place_then_break_drops_contents() {
    // Mimic the world place->fill->break flow: mint a chest,
    // fill some slots, drain - the resulting drop list should
    // match the filled contents (per-item).
    let mut chest = new_chest();
    // Three slots filled: 4 stone, 1 sword, 2 stone.
    chest.set_slot(2, Some(ItemStack::new(1, 4)));
    chest.set_slot(7, Some(ItemStack::new(23, 1)));
    chest.set_slot(20, Some(ItemStack::new(1, 2)));
    let drops = drain_drops(&chest);
    assert_eq!(drops.len(), 7);
    // One sword in the result.
    assert!(drops.contains(&23));
    // 6 stone copies.
    assert_eq!(drops.iter().filter(|d| **d == 1).count(), 6);
}
