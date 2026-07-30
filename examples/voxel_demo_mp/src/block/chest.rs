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
//! W467: the container family and the player inventory are both
//! `Option<ItemStack>`-slotted, so the apply helpers below move
//! full stacks - components (damage / enchants / name) travel with
//! every take / place / transfer, and merging anywhere requires
//! `same_stackable` (a worn tool never count-merges into a fresh
//! one). Only the break/spill edge still flattens to bare item ids
//! because the drop-entity path is tuple-shaped until W468.

#![allow(dead_code)]

use crate::container::Container;
use crate::container::ui::GridAction;
use crate::item::ItemStack;
use crate::item::stack::{drop_into_first_free_stacks, merge_into_stacks, take_n_stacks};

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

/// W128: world direction the EXT_POS double-chest half extends
/// toward, per facing (the axis PERPENDICULAR to the latch face).
/// Shared by the renderer and the pairing rule so the visual merge
/// and the 54-slot UI always agree.
pub(crate) fn ext_pos_dir(facing: u8) -> [i32; 3] {
    match facing % 4 {
        0 => [0, 0, 1],  // facing east -> +Z
        1 => [-1, 0, 0], // facing south -> -X
        2 => [0, 0, -1], // facing west -> -Z
        _ => [1, 0, 0],  // facing north -> +X
    }
}

/// W55.2b / W128 - find the partner chest for a double-chest pair.
/// MC rule: the adjacent chest along the axis PERPENDICULAR to the
/// facing, with the SAME facing, both having live containers. (The
/// old rule paired ANY adjacent chest, so the UI and the renderer
/// could disagree about what is a double chest.)
pub(crate) fn partner_for(
    world: &infengine_voxel::World,
    chests: &std::collections::HashMap<[i32; 3], Container>,
    facing: &std::collections::HashMap<[i32; 3], u8>,
    pos: [i32; 3],
) -> Option<[i32; 3]> {
    let f = facing.get(&pos).copied().unwrap_or(3);
    let ext = ext_pos_dir(f);
    for dir in [ext, [-ext[0], 0, -ext[2]]] {
        let p = [pos[0] + dir[0], pos[1], pos[2] + dir[2]];
        if world.get(p).0 != crate::block::CHEST_ID {
            continue;
        }
        if !chests.contains_key(&p) {
            continue;
        }
        if facing.get(&p).copied().unwrap_or(3) != f {
            continue;
        }
        return Some(p);
    }
    None
}

/// Flatten a chest's contents into the per-item drop list emitted
/// on break. Each slot expands to `count` copies of its id so the
/// caller can spawn one drop entity per item - matching the rest
/// of the game's "one drop per item" convention. Returns an empty
/// vec for an empty chest.
///
/// W468 carries components through drops; until then this edge
/// flattens to bare ids exactly like before W467 (a spilled worn
/// tool respawns fresh - no regression, same loss as today).
pub(crate) fn drain_drops(chest: &Container) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for st in chest.slots().iter().flatten() {
        for _ in 0..st.count {
            out.push(st.id);
        }
    }
    out
}

/// Recover a cursor-held stack into the chest when the UI closes.
/// Returns the per-item leftover Vec the caller should spawn as
/// floor drops at the chest position - any item the chest couldn't
/// absorb (e.g. full chest, missing chest) lands here. The part
/// that DOES land keeps the held stack's components; the spilled
/// leftover flattens to ids (drop path is tuple-shaped until W468).
///
/// Pulled out as a free fn so the close path stays trivially
/// testable; the runtime caller wraps it with the chest lookup +
/// the drop spawn loop.
pub(crate) fn recover_held_on_close(
    chest: Option<&mut Container>,
    held: Option<ItemStack>,
) -> Vec<u16> {
    let Some(stack) = held else {
        return Vec::new();
    };
    let id = stack.id;
    let count = stack.count;
    let leftover = match chest {
        Some(c) => c.insert(stack),
        None => count,
    };
    (0..leftover).map(|_| id).collect()
}

/// Apply one `GridAction` against the chest container. Returns the
/// next held-stack value (mirroring how `inv_apply_drag` returns
/// the next cursor state).
///
/// `partner` is the player's inventory slots, used for shift-click
/// transfers. We hand it in as `&mut [Option<ItemStack>]` (the
/// inventory's slot shape) so the chest UI works against the
/// player-inventory storage directly.
pub(crate) fn apply_chest_action(
    chest: &mut Container,
    partner: &mut [Option<ItemStack>],
    held: &mut Option<ItemStack>,
    action: GridAction,
) {
    match action {
        GridAction::Take(idx) => {
            // Pick up the whole slot into the cursor.
            if held.is_none()
                && let Some(st) = chest.take(idx)
            {
                *held = Some(st);
            }
        }
        GridAction::TakeHalf(idx) => {
            // RMB on a non-empty slot with no held stack: pull
            // half (rounded up) onto the cursor. Matches MC's
            // RMB-take semantics. Components clone to both halves.
            if held.is_some() {
                return;
            }
            let Some(count) = chest.slot_ref(idx).map(|st| st.count) else {
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
            // the player's inventory (components ride along).
            let Some(stack) = chest.slot(idx) else {
                return;
            };
            let leftover = drop_into_first_free_stacks(partner, stack.clone());
            if leftover == 0 {
                chest.set_slot(idx, None);
            } else {
                chest.set_slot(idx, Some(stack.with_count(leftover)));
            }
        }
    }
}

/// Apply one `GridAction` against the player's inventory side of
/// the chest UI. The partner direction (shift-click target) is the
/// chest. Mirrors `apply_chest_action` but with the slots <->
/// container roles reversed.
pub(crate) fn apply_player_action(
    inv: &mut [Option<ItemStack>],
    chest: &mut Container,
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
            // Shift-click: move the player-inv slot's contents
            // into the chest.
            let Some(slot) = inv.get_mut(idx) else { return };
            let Some(stack) = slot.clone() else { return };
            let leftover = chest.insert(stack.clone());
            if leftover == 0 {
                *slot = None;
            } else {
                *slot = Some(stack.with_count(leftover));
            }
        }
    }
}

/// Helper: place all-or-one of the held stack into a chest slot.
/// Returns nothing; updates `held` + the slot in place.
fn place_held(
    chest: &mut Container,
    held: &mut Option<ItemStack>,
    idx: usize,
    single: bool,
) {
    let Some(cur) = held.clone() else { return };
    // Single-item place lifts one from the cursor, then merges
    // it into the slot.
    let to_place = if single { 1 } else { cur.count };
    let leftover_in_cursor = cur.count - to_place;
    let leftover_after_insert = chest.insert_at(idx, cur.with_count(to_place));
    // What didn't fit in the slot comes back to the cursor;
    // combine with anything the single-mode lift left behind.
    let new_cursor_count = leftover_in_cursor + leftover_after_insert;
    *held = if new_cursor_count == 0 {
        None
    } else {
        Some(cur.with_count(new_cursor_count))
    };
    // Swap path: if the slot held NON-STACKABLE content (different
    // id, or same id with different components - MC swaps a fresh
    // tool onto a worn one) AND the place was a "full stack onto a
    // foreign slot", swap the two. Detect "nothing landed" by
    // comparing pre-insert leftover to to_place.
    if leftover_after_insert == to_place && !single {
        // The slot rejected our items because they didn't match.
        // Swap: take the slot's existing contents into the cursor,
        // place our items into the now-empty slot.
        let existing = chest.take(idx);
        let placed = chest.insert_at(idx, cur.clone());
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
            *held = Some(cur);
        }
    }
}

/// Place-into-slot variant for the player inventory side. Uses
/// `merge_into_stacks` / `take_n_stacks` on a raw slot ref so it
/// mirrors how the inventory module mutates its stack slots.
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
    // Try a merge first. `merge_into_stacks` rejects wholesale
    // when the slot isn't `same_stackable` with the cursor.
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
    // content (a same-stackable merge with no room is "no swap"
    // because there's no foreign content).
    if leftover_after == to_place && !single && pre_mismatch {
        let existing = slot.take();
        *slot = Some(cur);
        *held = existing;
    }
}

#[cfg(test)]
#[path = "chest_tests.rs"]
mod chest_tests;
