//! Chest block behaviour: per-position container state + the small
//! glue that ties placement / break / RMB-open into the existing
//! world interact chain.
//!
//! The 27-slot container itself lives in `Voxel::chests`, keyed by
//! absolute world voxel coords. This module owns the helpers that
//! mint a fresh container on place, drop the contents as item
//! entities on break, and translate a frame of `GridAction`s into
//! per-slot mutations during the open-UI loop.
//!
//! Phase C.3 keeps the player inventory in its current
//! `Vec<Option<(BlockId, u8)>>` shape (no Container migration), so
//! the helpers below operate on `Slot = Option<(u16, u8)>` directly:
//! the same shape both the existing inventory + the new chest
//! container use at the wire level. The thin player-inventory
//! adapter at the bottom of this file lets the UI loop reuse the
//! generic `Container` ops against the host's hotbar + storage
//! rows without rewriting the player inventory storage.

#![allow(dead_code)]

use crate::container::Container;
use crate::container::ui::GridAction;
use crate::item::stack::{drop_into_first_free, merge_into, take_n};

/// Number of slots in a single chest. Matches MC's small chest
/// (3 rows of 9). Larger / double-chests can change this per-chest
/// later without touching the save schema (slot count is recorded
/// per-instance).
pub(crate) const CHEST_SLOTS: usize = 27;

/// Build a fresh empty chest container. Used by the world-interact
/// place path when the player drops a chest block at a voxel that
/// doesn't yet have an entry in `Voxel::chests`.
pub(crate) fn new_chest() -> Container {
    Container::with_size(CHEST_SLOTS)
}

/// Flatten a chest's contents into the per-item drop list emitted
/// on break. Each slot of `(id, count)` expands to `count` copies
/// of `id` so the caller can spawn one drop entity per item -
/// matching the rest of the game's "one drop per item" convention.
/// Returns an empty vec for an empty chest.
pub(crate) fn drain_drops(chest: &Container) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for (id, count) in chest.slots().iter().flatten() {
        for _ in 0..*count {
            out.push(*id);
        }
    }
    out
}

/// Apply one `GridAction` against the chest container. Returns the
/// next held-stack value (mirroring how `inv_apply_drag` returns
/// the next cursor state).
///
/// `partner` is the player's inventory slots, used for shift-click
/// transfers. We hand it in as `&mut [Slot]` (the existing
/// inventory shape) instead of a `Container` so the chest UI works
/// against the unchanged inventory storage.
pub(crate) fn apply_chest_action(
    chest: &mut Container,
    partner: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    action: GridAction,
) {
    match action {
        GridAction::Take(idx) => {
            // Pick up the whole slot into the cursor.
            if held.is_none()
                && let Some((id, n)) = chest.take(idx)
            {
                *held = Some((id, n));
            }
        }
        GridAction::TakeHalf(idx) => {
            // RMB on a non-empty slot with no held stack: pull
            // half (rounded up) onto the cursor. Matches MC's
            // RMB-take semantics.
            if held.is_some() {
                return;
            }
            let Some((_id, count)) = chest.slot(idx) else {
                return;
            };
            let half = count.div_ceil(2);
            if let Some(taken) = chest.take_n(idx, half) {
                *held = Some(taken);
            }
        }
        GridAction::Place(idx) => {
            // Drop the held stack into the slot - merge / swap.
            place_held(chest, held, idx, false);
        }
        GridAction::PlaceOne(idx) => {
            // Place exactly one item from the held stack. RMB
            // with a held stack.
            place_held(chest, held, idx, true);
        }
        GridAction::Transfer(idx) => {
            // Shift-click: move the chest slot's contents into
            // the player's inventory.
            let Some((id, count)) = chest.slot(idx) else {
                return;
            };
            let leftover = drop_into_first_free(partner, (id, count));
            if leftover == 0 {
                chest.set_slot(idx, None);
            } else {
                chest.set_slot(idx, Some((id, leftover)));
            }
        }
    }
}

/// Apply one `GridAction` against the player's inventory side of
/// the chest UI. The partner direction (shift-click target) is the
/// chest. Mirrors `apply_chest_action` but with the slots <->
/// container roles reversed.
pub(crate) fn apply_player_action(
    inv: &mut [Option<(u16, u8)>],
    chest: &mut Container,
    held: &mut Option<(u16, u8)>,
    action: GridAction,
) {
    match action {
        GridAction::Take(idx) => {
            if held.is_none()
                && let Some(slot) = inv.get_mut(idx)
                && let Some((id, n)) = slot.take()
            {
                *held = Some((id, n));
            }
        }
        GridAction::TakeHalf(idx) => {
            if held.is_some() {
                return;
            }
            let Some(slot) = inv.get_mut(idx) else { return };
            let Some((_id, count)) = *slot else { return };
            let half = count.div_ceil(2);
            if let Some(taken) = take_n(slot, half) {
                *held = Some(taken);
            }
        }
        GridAction::Place(idx) => {
            place_into_slot(inv, held, idx, false);
        }
        GridAction::PlaceOne(idx) => {
            place_into_slot(inv, held, idx, true);
        }
        GridAction::Transfer(idx) => {
            // Shift-click: move the player-inv slot's contents
            // into the chest.
            let Some(slot) = inv.get_mut(idx) else { return };
            let Some((id, count)) = *slot else { return };
            let leftover = chest.insert((id, count));
            if leftover == 0 {
                *slot = None;
            } else {
                *slot = Some((id, leftover));
            }
        }
    }
}

/// Helper: place all-or-one of the held stack into a chest slot.
/// Returns nothing; updates `held` + the slot in place.
fn place_held(
    chest: &mut Container,
    held: &mut Option<(u16, u8)>,
    idx: usize,
    single: bool,
) {
    let Some((id, count)) = *held else { return };
    // Single-item place lifts one from the cursor, then merges
    // it into the slot.
    let to_place = if single { 1 } else { count };
    let leftover_in_cursor = count - to_place;
    let leftover_after_insert = chest.insert_at(idx, (id, to_place));
    // What didn't fit in the slot comes back to the cursor;
    // combine with anything the single-mode lift left behind.
    let new_cursor_count = leftover_in_cursor + leftover_after_insert;
    *held = if new_cursor_count == 0 {
        None
    } else {
        Some((id, new_cursor_count))
    };
    // Swap path: if the slot already held a DIFFERENT item AND
    // the place was a "full stack onto a foreign slot", MC swaps
    // the two. We only swap when the cursor is now empty (i.e.
    // every item we held landed somewhere) - otherwise the
    // partial merge stays partial. Detect "nothing landed" by
    // comparing pre-insert leftover to to_place.
    if leftover_after_insert == to_place && !single {
        // The slot rejected our items because they didn't match.
        // Swap: take the slot's existing contents into the cursor,
        // place our items into the now-empty slot.
        let existing = chest.take(idx);
        let placed = chest.insert_at(idx, (id, count));
        // If our items now fit, the swap succeeded - drop the
        // cursor onto the slot we just cleared.
        if placed == 0 {
            *held = existing;
        } else {
            // Restore the original cursor; the swap failed (slot
            // accept filter rejected). Drop existing back.
            if let Some(prev) = existing {
                let _ = chest.insert_at(idx, prev);
            }
            *held = Some((id, count));
        }
    }
}

/// Place-into-slot variant for the player inventory side. Uses
/// `merge_into` / `take_n` on a raw slot ref so it mirrors how the
/// existing inventory module mutates its `Vec<Option<(u16, u8)>>`.
fn place_into_slot(
    inv: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    idx: usize,
    single: bool,
) {
    let Some((id, count)) = *held else { return };
    let Some(slot) = inv.get_mut(idx) else { return };
    let to_place = if single { 1 } else { count };
    let leftover_in_cursor = count - to_place;
    // Try a merge first.
    let pre = *slot;
    let leftover_after = merge_into(slot, (id, to_place));
    let new_cursor_count = leftover_in_cursor + leftover_after;
    *held = if new_cursor_count == 0 {
        None
    } else {
        Some((id, new_cursor_count))
    };
    // Swap on full-stack reject (same-id merge with no room is
    // also "no swap" because there's no foreign content).
    if leftover_after == to_place && !single && pre.is_some_and(|(eid, _)| eid != id) {
        let existing = slot.take();
        *slot = Some((id, count));
        *held = existing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_slot(c: &Container, idx: usize, expected: Option<(u16, u8)>) {
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
        chest.set_slot(3, Some((1, 12)));
        let mut held: Option<(u16, u8)> = None;
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Take(3));
        assert_eq!(held, Some((1, 12)));
        assert_slot(&chest, 3, None);
    }

    #[test]
    fn take_half_rounds_up() {
        let mut chest = new_chest();
        chest.set_slot(0, Some((1, 7)));
        let mut held: Option<(u16, u8)> = None;
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::TakeHalf(0));
        // 7 / 2 = 4 (rounded up); slot left with 3.
        assert_eq!(held, Some((1, 4)));
        assert_slot(&chest, 0, Some((1, 3)));
    }

    #[test]
    fn place_full_stack_into_empty_slot() {
        let mut chest = new_chest();
        let mut held: Option<(u16, u8)> = Some((1, 9));
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(5));
        assert_eq!(held, None);
        assert_slot(&chest, 5, Some((1, 9)));
    }

    #[test]
    fn place_one_lifts_a_single_item() {
        let mut chest = new_chest();
        let mut held: Option<(u16, u8)> = Some((1, 5));
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::PlaceOne(2));
        // 1 placed, cursor still has 4.
        assert_eq!(held, Some((1, 4)));
        assert_slot(&chest, 2, Some((1, 1)));
    }

    #[test]
    fn place_full_stack_swaps_with_different_item() {
        let mut chest = new_chest();
        chest.set_slot(0, Some((2, 4)));
        let mut held: Option<(u16, u8)> = Some((1, 9));
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(0));
        // Items 1 & 2 swap.
        assert_eq!(held, Some((2, 4)));
        assert_slot(&chest, 0, Some((1, 9)));
    }

    #[test]
    fn place_same_id_tops_up_partial() {
        let mut chest = new_chest();
        chest.set_slot(1, Some((1, 60)));
        // Stone caps at 64, so a 5-stack incoming tops up to 64
        // and leaves 1 on the cursor.
        let mut held: Option<(u16, u8)> = Some((1, 5));
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(1));
        assert_eq!(held, Some((1, 1)));
        assert_slot(&chest, 1, Some((1, 64)));
    }

    #[test]
    fn transfer_from_chest_moves_to_player_inv() {
        let mut chest = new_chest();
        chest.set_slot(2, Some((1, 10)));
        let mut held: Option<(u16, u8)> = None;
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Transfer(2));
        assert_eq!(held, None);
        assert_slot(&chest, 2, None);
        assert_eq!(inv[0], Some((1, 10)));
    }

    #[test]
    fn transfer_from_player_moves_to_chest() {
        let mut chest = new_chest();
        let mut held: Option<(u16, u8)> = None;
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        inv[4] = Some((1, 8));
        apply_player_action(&mut inv, &mut chest, &mut held, GridAction::Transfer(4));
        assert_eq!(inv[4], None);
        // First free chest slot gets the stack.
        assert_slot(&chest, 0, Some((1, 8)));
    }

    #[test]
    fn player_take_pulls_from_inv() {
        let mut chest = new_chest();
        let mut held: Option<(u16, u8)> = None;
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        inv[7] = Some((1, 20));
        apply_player_action(&mut inv, &mut chest, &mut held, GridAction::Take(7));
        assert_eq!(held, Some((1, 20)));
        assert_eq!(inv[7], None);
    }

    #[test]
    fn player_place_one_drops_a_single_item() {
        let mut chest = new_chest();
        let mut held: Option<(u16, u8)> = Some((1, 3));
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_player_action(&mut inv, &mut chest, &mut held, GridAction::PlaceOne(0));
        assert_eq!(held, Some((1, 2)));
        assert_eq!(inv[0], Some((1, 1)));
    }

    #[test]
    fn transfer_full_chest_leaves_overflow_in_source() {
        // Player inv full of stone; transfer 30 stone from chest
        // back into player - chest stack should partial-deplete
        // since topping up 9 partial 64-stacks may not absorb all.
        let mut chest = new_chest();
        chest.set_slot(0, Some((1, 30)));
        let mut held: Option<(u16, u8)> = None;
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        // Pre-fill every player slot with a different id so the
        // transfer fully fails.
        for slot in inv.iter_mut() {
            *slot = Some((2, 64));
        }
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Transfer(0));
        // Nothing moved; chest still has 30 stone.
        assert_slot(&chest, 0, Some((1, 30)));
    }

    #[test]
    fn place_one_starts_a_new_stack_in_empty_slot() {
        let mut chest = new_chest();
        let mut held: Option<(u16, u8)> = Some((5, 1));
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::PlaceOne(10));
        assert_eq!(held, None);
        assert_slot(&chest, 10, Some((5, 1)));
    }

    #[test]
    fn take_on_empty_slot_is_noop() {
        let mut chest = new_chest();
        let mut held: Option<(u16, u8)> = None;
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Take(0));
        assert_eq!(held, None);
    }

    #[test]
    fn place_with_no_held_stack_is_noop() {
        let mut chest = new_chest();
        chest.set_slot(0, Some((1, 4)));
        let mut held: Option<(u16, u8)> = None;
        let mut inv: Vec<Option<(u16, u8)>> = vec![None; 36];
        apply_chest_action(&mut chest, &mut inv, &mut held, GridAction::Place(0));
        assert_eq!(held, None);
        assert_slot(&chest, 0, Some((1, 4)));
    }

    #[test]
    fn drain_drops_empty_chest_yields_no_drops() {
        let chest = new_chest();
        assert!(drain_drops(&chest).is_empty());
    }

    #[test]
    fn drain_drops_expands_each_stack_to_per_item_drops() {
        let mut chest = new_chest();
        chest.set_slot(0, Some((1, 3)));
        chest.set_slot(5, Some((2, 1)));
        chest.set_slot(26, Some((1, 2)));
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
    fn place_then_break_drops_contents() {
        // Mimic the world place->fill->break flow: mint a chest,
        // fill some slots, drain - the resulting drop list should
        // match the filled contents (per-item).
        let mut chest = new_chest();
        // Three slots filled: 4 stone, 1 sword, 2 stone.
        chest.set_slot(2, Some((1, 4)));
        chest.set_slot(7, Some((23, 1)));
        chest.set_slot(20, Some((1, 2)));
        let drops = drain_drops(&chest);
        assert_eq!(drops.len(), 7);
        // One sword in the result.
        assert!(drops.contains(&23));
        // 6 stone copies.
        assert_eq!(drops.iter().filter(|d| **d == 1).count(), 6);
    }
}
