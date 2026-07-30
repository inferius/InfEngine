//! GridAction <-> furnace mutation glue for the UI panel.
//!
//! Pulled out of `block::furnace` so the parent module stays under
//! the 500-LOC soft cap. Mirrors the chest module's split: one
//! file holds the place / break / cook step; this file holds the
//! per-click apply helpers.
//!
//! Both `apply_furnace_action` (furnace-side LMB / RMB / shift) and
//! `apply_player_action` (player-side) route through `Container::
//! insert_at` so the accept filters on the furnace stay honoured:
//! a non-fuel stack dropped onto the fuel slot bounces back to
//! the cursor; the output slot rejects every player-driven place.
//!
//! W467: stack-native - components ride through every take / place
//! / transfer, and merges require `same_stackable`.

#![allow(dead_code)]

use crate::container::Container;
use crate::container::ui::GridAction;
use crate::item::ItemStack;
use crate::item::stack::{drop_into_first_free_stacks, merge_into_stacks, take_n_stacks};

/// Apply one `GridAction` against the furnace container. Returns
/// the next held-stack value indirectly through `held`. Mirrors
/// `chest::apply_chest_action` but goes through the accept-filter-
/// aware `Container::insert_at` path so player-driven placements
/// into the output slot bounce.
///
/// `partner` is the player's inventory slots, used for shift-click
/// transfers (Transfer action).
pub(crate) fn apply_furnace_action(
    furnace: &mut Container,
    partner: &mut [Option<ItemStack>],
    held: &mut Option<ItemStack>,
    action: GridAction,
) {
    match action {
        GridAction::Take(idx) => {
            if held.is_none()
                && let Some(st) = furnace.take(idx)
            {
                *held = Some(st);
            }
        }
        GridAction::TakeHalf(idx) => {
            if held.is_some() {
                return;
            }
            let Some(count) = furnace.slot_ref(idx).map(|st| st.count) else {
                return;
            };
            let half = count.div_ceil(2);
            if let Some(taken) = furnace.take_n(idx, half) {
                *held = Some(taken);
            }
        }
        GridAction::Place(idx) => {
            place_held(furnace, held, idx, false);
        }
        GridAction::PlaceOne(idx) => {
            place_held(furnace, held, idx, true);
        }
        GridAction::Transfer(idx) => {
            // Shift-click pulls the furnace slot's contents into
            // the player's inventory (components ride along).
            let Some(stack) = furnace.slot(idx) else {
                return;
            };
            let leftover = drop_into_first_free_stacks(partner, stack.clone());
            if leftover == 0 {
                furnace.set_slot(idx, None);
            } else {
                furnace.set_slot(idx, Some(stack.with_count(leftover)));
            }
        }
    }
}

/// Apply one `GridAction` against the player's inventory side of
/// the furnace UI. Mirrors `chest::apply_player_action` but the
/// shift-click target is the furnace - which honours its accept
/// filters, so a non-fuel item shift-clicked into a furnace with
/// only the fuel slot empty falls back to the input slot (or
/// stays put if none of the slots accept it).
pub(crate) fn apply_player_action(
    inv: &mut [Option<ItemStack>],
    furnace: &mut Container,
    held: &mut Option<ItemStack>,
    action: GridAction,
) {
    match action {
        GridAction::Take(idx) => {
            if held.is_none()
                && let Some(slot) = inv.get_mut(idx)
                && let Some(st) = slot.take()
            {
                *held = Some(st);
            }
        }
        GridAction::TakeHalf(idx) => {
            if held.is_some() {
                return;
            }
            let Some(slot) = inv.get_mut(idx) else { return };
            let Some(count) = slot.as_ref().map(|st| st.count) else {
                return;
            };
            let half = count.div_ceil(2);
            if let Some(taken) = take_n_stacks(slot, half) {
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
            let Some(slot) = inv.get_mut(idx) else { return };
            let Some(stack) = slot.clone() else { return };
            let leftover = furnace.insert(stack.clone());
            if leftover == 0 {
                *slot = None;
            } else {
                *slot = Some(stack.with_count(leftover));
            }
        }
    }
}

/// Place all-or-one of the held stack into a furnace slot,
/// honouring the accept filter via `Container::insert_at`. Items
/// rejected by the slot's predicate (e.g. iron ore into the fuel
/// slot, or anything into the output slot) come back to the
/// cursor.
fn place_held(
    furnace: &mut Container,
    held: &mut Option<ItemStack>,
    idx: usize,
    single: bool,
) {
    let Some(cur) = held.clone() else { return };
    let to_place = if single { 1 } else { cur.count };
    let leftover_in_cursor = cur.count - to_place;
    let leftover_after_insert =
        furnace.insert_at(idx, cur.with_count(to_place));
    let new_cursor_count = leftover_in_cursor + leftover_after_insert;
    *held = if new_cursor_count == 0 {
        None
    } else {
        Some(cur.with_count(new_cursor_count))
    };
}

/// Place-into-slot variant for the player inventory side. Honours
/// the per-item `max_stack` from the item registry via the shared
/// stack helpers.
fn place_into_slot(
    inv: &mut [Option<ItemStack>],
    held: &mut Option<ItemStack>,
    idx: usize,
    single: bool,
) {
    let Some(cur) = held.clone() else { return };
    let Some(slot) = inv.get_mut(idx) else { return };
    let to_place = if single { 1 } else { cur.count };
    let leftover_in_cursor = cur.count - to_place;
    let pre_mismatch = slot
        .as_ref()
        .is_some_and(|st| !st.same_stackable(&cur));
    let leftover_after = merge_into_stacks(slot, cur.with_count(to_place));
    let new_cursor_count = leftover_in_cursor + leftover_after;
    *held = if new_cursor_count == 0 {
        None
    } else {
        Some(cur.with_count(new_cursor_count))
    };
    // Swap on full-stack reject when the slot held non-stackable
    // content (different id, or same id with different components).
    if leftover_after == to_place && !single && pre_mismatch {
        let existing = slot.take();
        *slot = Some(cur);
        *held = existing;
    }
}
