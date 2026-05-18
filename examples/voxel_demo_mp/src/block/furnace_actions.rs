//! GridAction <-> furnace mutation glue for the UI panel.
//!
//! Pulled out of `block::furnace` so the parent module stays under
//! the 500-LOC soft cap. Mirrors the chest module's split: one
//! file holds the place / break / cook step; this file holds the
//! per-click apply helpers.
//!
//! Both `apply_furnace_action` (chest-side LMB / RMB / shift) and
//! `apply_player_action` (player-side) route through `Container::
//! insert_at` so the accept filters on the furnace stay honoured:
//! a non-fuel stack dropped onto the fuel slot bounces back to
//! the cursor; the output slot rejects every player-driven place.

#![allow(dead_code)]

use crate::container::Container;
use crate::container::ui::GridAction;
use crate::item::stack::{drop_into_first_free, merge_into, take_n};

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
    partner: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    action: GridAction,
) {
    match action {
        GridAction::Take(idx) => {
            if held.is_none()
                && let Some((id, n)) = furnace.take(idx)
            {
                *held = Some((id, n));
            }
        }
        GridAction::TakeHalf(idx) => {
            if held.is_some() {
                return;
            }
            let Some((_id, count)) = furnace.slot(idx) else {
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
            // the player's inventory.
            let Some((id, count)) = furnace.slot(idx) else {
                return;
            };
            let leftover = drop_into_first_free(partner, (id, count));
            if leftover == 0 {
                furnace.set_slot(idx, None);
            } else {
                furnace.set_slot(idx, Some((id, leftover)));
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
    inv: &mut [Option<(u16, u8)>],
    furnace: &mut Container,
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
            let Some(slot) = inv.get_mut(idx) else { return };
            let Some((id, count)) = *slot else { return };
            let leftover = furnace.insert((id, count));
            if leftover == 0 {
                *slot = None;
            } else {
                *slot = Some((id, leftover));
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
    held: &mut Option<(u16, u8)>,
    idx: usize,
    single: bool,
) {
    let Some((id, count)) = *held else { return };
    let to_place = if single { 1 } else { count };
    let leftover_in_cursor = count - to_place;
    let leftover_after_insert = furnace.insert_at(idx, (id, to_place));
    let new_cursor_count = leftover_in_cursor + leftover_after_insert;
    *held = if new_cursor_count == 0 {
        None
    } else {
        Some((id, new_cursor_count))
    };
}

/// Place-into-slot variant for the player inventory side. Honours
/// the per-item `max_stack` from the item registry via the shared
/// stack helpers.
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
    let pre = *slot;
    let leftover_after = merge_into(slot, (id, to_place));
    let new_cursor_count = leftover_in_cursor + leftover_after;
    *held = if new_cursor_count == 0 {
        None
    } else {
        Some((id, new_cursor_count))
    };
    // Swap on full-stack reject when the slot held a different id.
    if leftover_after == to_place && !single && pre.is_some_and(|(eid, _)| eid != id) {
        let existing = slot.take();
        *slot = Some((id, count));
        *held = existing;
    }
}
