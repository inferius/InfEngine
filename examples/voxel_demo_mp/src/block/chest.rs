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

/// Recover a cursor-held stack into the chest when the UI closes.
/// Returns the per-item leftover Vec the caller should spawn as
/// floor drops at the chest position - any item the chest couldn't
/// absorb (e.g. full chest, missing chest) lands here.
///
/// Pulled out as a free fn so the close path stays trivially
/// testable; the runtime caller wraps it with the chest lookup +
/// the drop spawn loop.
pub(crate) fn recover_held_on_close(
    chest: Option<&mut Container>,
    held: Option<(u16, u8)>,
) -> Vec<u16> {
    let Some((id, count)) = held else {
        return Vec::new();
    };
    let leftover = match chest {
        Some(c) => c.insert((id, count)),
        None => count,
    };
    (0..leftover).map(|_| id).collect()
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
#[path = "chest_tests.rs"]
mod chest_tests;
