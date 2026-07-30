//! W470 - MC container-click core: a semantics-faithful port of
//! `AbstractContainerMenu.doClick` (MC 26.1.2 decompile) over this
//! engine's stack slots.
//!
//! One implementation for every grid: the chest / furnace / brewing
//! panels, the 3x3 crafting table, the inventory screen and the MP
//! wire all route their slot interactions through the free fns in
//! this module, so the rules (rounding, ordering, caps) can never
//! drift between screens.
//!
//! The abstraction is a flat list of `SectionMut`s - each section
//! is one storage's slot slice plus its per-slot accept table (the
//! engine's `Slot.mayPlace`). A "global index" (gi) addresses the
//! concatenation, mirroring MC's single `slots` list per menu.
//!
//! Ported ops (MC `ContainerInput` names):
//! - PICKUP -> `pickup_click`, plus `throw_from_cursor` for the
//!   slotId -999 click-outside branch.
//! - QUICK_MOVE -> `quick_move` over an explicit fill order
//!   (adapters express each menu's `quickMoveStack` routing;
//!   `move_stack_to_order` is the `moveItemStackTo` two-phase port).
//! - SWAP -> `swap_click` (number keys; no offhand / button-40 in
//!   this engine).
//! - CLONE -> `clone_click` (creative middle-click).
//! - THROW -> `throw_from_slot` (Q / Ctrl+Q on a hovered slot).
//! - QUICK_CRAFT -> `quick_craft_apply` (drag-paint, applied on
//!   release; `can_add_drag_slot` is the add-slot eligibility the
//!   UI trackers mirror).
//! - PICKUP_ALL -> `pickup_all` (double-click collect).
//!
//! Deliberate deviations from the source (documented per fn):
//! - Per-slot max-stack caps don't exist here: `Slot.
//!   getMaxStackSize(stack)` is always the item registry's
//!   `item_max_stack(id)`, so the SWAP / PICKUP overflow-split
//!   branches (`source.getCount() > maxStackSize`) are dead and not
//!   ported.
//! - Result slots (craft output) are VIRTUAL in this engine (they
//!   are computed, not stored), so the THROW / QUICK_MOVE repeat
//!   loops that re-poll a refilling result slot collapse to a
//!   single application, and `canTakeItemForPickAll` needs no
//!   override.
//! - The QUICK_CRAFT three-phase packet protocol (header/type masks)
//!   is replaced by client-side visit tracking + one apply call; the
//!   eligibility checks and the distribution math are ported 1:1.

use crate::item::ItemStack;
use crate::item::item_max_stack;
use crate::item::stack::take_n_stacks;

/// Per-slot accept predicate - same shape as `Container`'s
/// `AcceptFn` (id-keyed: what fits in a furnace fuel slot is a
/// property of the item kind, not per-stack components).
pub(crate) type SlotAccept = fn(u16) -> bool;

/// One section of an open screen: a contiguous run of slots
/// belonging to one storage (container rows, player inventory,
/// craft grid). `accept` runs parallel to `slots`; missing entries
/// (shorter slice, `None`) accept everything - mirroring
/// `Container::accepts`.
pub(crate) struct SectionMut<'a> {
    pub(crate) slots: &'a mut [Option<ItemStack>],
    pub(crate) accept: &'a [Option<SlotAccept>],
}

impl<'a> SectionMut<'a> {
    /// Accept-free section (player inventory, craft grids).
    pub(crate) fn open(slots: &'a mut [Option<ItemStack>]) -> Self {
        Self { slots, accept: &[] }
    }
}

/// Total slot count across all sections.
pub(crate) fn total_slots(sections: &[SectionMut<'_>]) -> usize {
    sections.iter().map(|s| s.slots.len()).sum()
}

/// Resolve a global index to `(section, local)` coordinates.
fn locate(sections: &[SectionMut<'_>], gi: usize) -> Option<(usize, usize)> {
    let mut base = 0usize;
    for (si, s) in sections.iter().enumerate() {
        if gi < base + s.slots.len() {
            return Some((si, gi - base));
        }
        base += s.slots.len();
    }
    None
}

/// Mutable slot lookup by global index. Out-of-range -> `None`
/// (every op treats that as a no-op, like MC's bounds guards).
pub(crate) fn slot_mut<'a, 'b>(
    sections: &'b mut [SectionMut<'a>],
    gi: usize,
) -> Option<&'b mut Option<ItemStack>> {
    let (si, li) = locate(sections, gi)?;
    sections[si].slots.get_mut(li)
}

/// Read-only slot lookup by global index.
pub(crate) fn slot_ref<'a, 'b>(
    sections: &'b [SectionMut<'a>],
    gi: usize,
) -> Option<&'b ItemStack> {
    let (si, li) = locate(sections, gi)?;
    sections[si].slots.get(li)?.as_ref()
}

/// The engine's `Slot.mayPlace`: does the slot at `gi` accept
/// `item_id`? Sections without a predicate accept everything;
/// out-of-range indices reject.
pub(crate) fn accepts(sections: &[SectionMut<'_>], gi: usize, item_id: u16) -> bool {
    match locate(sections, gi) {
        Some((si, li)) => match sections[si].accept.get(li).copied().flatten() {
            Some(pred) => pred(item_id),
            None => true,
        },
        None => false,
    }
}

/// `Slot.safeInsert(stack, amount)` port: move up to `amount` items
/// from `incoming` into `slot`, bounded by the item's max stack and
/// the slot's current fill. The caller has already checked the
/// accept filter (mirroring how `safeInsert` is reached behind
/// `mayPlace` in the source). `incoming.count` is reduced in place;
/// a drained incoming is the caller's cue to clear its cursor.
fn safe_insert(slot: &mut Option<ItemStack>, incoming: &mut ItemStack, amount: u8) {
    if incoming.count == 0 {
        return;
    }
    let cap = item_max_stack(incoming.id);
    let existing = slot.as_ref().map(|st| st.count).unwrap_or(0);
    let transferable = amount
        .min(incoming.count)
        .min(cap.saturating_sub(existing));
    if transferable == 0 {
        return;
    }
    match slot {
        None => {
            *slot = Some(incoming.with_count(transferable));
            incoming.count -= transferable;
        }
        Some(st) if st.same_stackable(incoming) => {
            st.count += transferable;
            incoming.count -= transferable;
        }
        // Mismatched occupied slot: safeInsert is a no-op (the
        // PICKUP swap branch handles that case before calling us).
        Some(_) => {}
    }
}

/// PICKUP arm of `doClick` for a real slot: LMB (`secondary =
/// false`) / RMB (`secondary = true`) with or without a cursor
/// stack. Source lines 428-470:
/// - empty slot + cursor: place all (LMB) / one (RMB) via
///   `safeInsert`, gated on `mayPlace`.
/// - filled slot + empty cursor: take all (LMB) / the LARGER half -
///   `(count + 1) / 2`, i.e. cursor gets ceil (RMB).
/// - filled slot + cursor, slot accepts the cursor item:
///   `same_stackable` merges all/one via `safeInsert`; a foreign
///   stack SWAPS wholesale (components travel both ways).
/// - filled slot + cursor, slot REJECTS the cursor item (furnace
///   output): a `same_stackable` click pulls the slot's contents
///   INTO the cursor up to its max stack (source line 459-464).
pub(crate) fn pickup_click(
    sections: &mut [SectionMut<'_>],
    cursor: &mut Option<ItemStack>,
    gi: usize,
    secondary: bool,
) {
    let accept_ok = cursor
        .as_ref()
        .map(|cur| accepts(sections, gi, cur.id))
        .unwrap_or(false);
    let Some(slot) = slot_mut(sections, gi) else {
        return;
    };
    match (slot.is_some(), cursor.as_mut()) {
        (false, Some(cur)) => {
            if accept_ok {
                let amount = if secondary { 1 } else { cur.count };
                safe_insert(slot, cur, amount);
                if cur.count == 0 {
                    *cursor = None;
                }
            }
        }
        (true, None) => {
            let count = slot.as_ref().map(|st| st.count).unwrap_or(0);
            // MC: (count + 1) / 2 - the cursor takes the CEIL half.
            let amount = if secondary { count.div_ceil(2) } else { count };
            *cursor = take_n_stacks(slot, amount);
        }
        (true, Some(cur)) => {
            let same = slot
                .as_ref()
                .is_some_and(|st| st.same_stackable(cur));
            if accept_ok {
                if same {
                    let amount = if secondary { 1 } else { cur.count };
                    safe_insert(slot, cur, amount);
                    if cur.count == 0 {
                        *cursor = None;
                    }
                } else if cur.count <= item_max_stack(cur.id) {
                    // Foreign stack: swap slot <-> cursor. The count
                    // guard is the source's `carried.getCount() <=
                    // slot.getMaxStackSize(carried)`; with per-item
                    // caps it always holds, kept for fidelity.
                    let existing = slot.take();
                    *slot = Some(cursor.take().expect("cursor checked Some"));
                    *cursor = existing;
                }
            } else if same {
                // Place-restricted slot (mayPlace false) but the
                // contents match the cursor: pull into the cursor,
                // bounded by the cursor's remaining room.
                let room = item_max_stack(cur.id).saturating_sub(cur.count);
                let have = slot.as_ref().map(|st| st.count).unwrap_or(0);
                let take = have.min(room);
                if let Some(taken) = take_n_stacks(slot, take) {
                    cur.count += taken.count;
                }
            }
        }
        (false, None) => {}
    }
}

/// PICKUP arm, slotId == -999 (click outside the window): LMB
/// throws the whole cursor stack, RMB splits one off. Returns the
/// stack to spawn as a drop (the caller owns velocities / net).
pub(crate) fn throw_from_cursor(
    cursor: &mut Option<ItemStack>,
    whole_stack: bool,
) -> Option<ItemStack> {
    let cur = cursor.as_mut()?;
    if whole_stack {
        return cursor.take();
    }
    let one = cur.with_count(1);
    cur.count -= 1;
    if cur.count == 0 {
        *cursor = None;
    }
    Some(one)
}

/// THROW arm: Q (`whole_stack = false` -> one item) / Ctrl+Q
/// (`true` -> the whole stack) on the hovered slot. Source gate:
/// only fires with an EMPTY cursor. Returns the stack to spawn.
/// The source's Ctrl+Q re-poll loop (for refilling result slots)
/// is not needed - result slots are virtual here.
pub(crate) fn throw_from_slot(
    sections: &mut [SectionMut<'_>],
    cursor: &Option<ItemStack>,
    gi: usize,
    whole_stack: bool,
) -> Option<ItemStack> {
    if cursor.is_some() {
        return None;
    }
    let slot = slot_mut(sections, gi)?;
    let count = slot.as_ref()?.count;
    let amount = if whole_stack { count } else { 1 };
    take_n_stacks(slot, amount)
}

/// SWAP arm: exchange the hovered slot `gi` with the hotbar slot
/// `hotbar_gi` (the number-key press). Accept filters are honoured
/// in BOTH directions - an armor-ish filtered slot refuses a
/// non-matching hotbar stack and vice versa. The source's
/// overflow-split branches (`source.getCount() > maxStackSize`)
/// are dead with per-item caps and not ported.
pub(crate) fn swap_click(
    sections: &mut [SectionMut<'_>],
    gi: usize,
    hotbar_gi: usize,
) {
    if gi == hotbar_gi {
        return;
    }
    let source = slot_ref(sections, gi).cloned();
    let hot = slot_ref(sections, hotbar_gi).cloned();
    match (source, hot) {
        (None, None) => {}
        (Some(s), None) => {
            // Hovered -> hotbar (source empty branch mirrored: MC
            // indexes hotbar as the `buttonNum` side).
            if accepts(sections, hotbar_gi, s.id) {
                if let Some(slot) = slot_mut(sections, gi) {
                    *slot = None;
                }
                if let Some(slot) = slot_mut(sections, hotbar_gi) {
                    *slot = Some(s);
                }
            }
        }
        (None, Some(h)) => {
            // Hotbar -> hovered; the hovered slot must accept.
            if accepts(sections, gi, h.id) {
                if let Some(slot) = slot_mut(sections, hotbar_gi) {
                    *slot = None;
                }
                if let Some(slot) = slot_mut(sections, gi) {
                    *slot = Some(h);
                }
            }
        }
        (Some(s), Some(h)) => {
            if accepts(sections, gi, h.id) && accepts(sections, hotbar_gi, s.id) {
                if let Some(slot) = slot_mut(sections, gi) {
                    *slot = Some(h);
                }
                if let Some(slot) = slot_mut(sections, hotbar_gi) {
                    *slot = Some(s);
                }
            }
        }
    }
}

/// CLONE arm: creative middle-click. Cursor must be empty; it
/// receives a full max-stack copy of the hovered stack (components
/// cloned). `creative` is the caller-supplied
/// `player.hasInfiniteMaterials()`.
pub(crate) fn clone_click(
    sections: &[SectionMut<'_>],
    cursor: &mut Option<ItemStack>,
    gi: usize,
    creative: bool,
) {
    if !creative || cursor.is_some() {
        return;
    }
    if let Some(st) = slot_ref(sections, gi) {
        *cursor = Some(st.with_count(item_max_stack(st.id)));
    }
}

/// `canItemQuickReplace(slot, stack, ignoreSize = true)` port: an
/// empty slot always qualifies; an occupied one must be
/// `same_stackable` (the size clause is trivially true with
/// per-item caps: `slot.count <= stack max`).
pub(crate) fn can_quick_replace(
    slot: Option<&ItemStack>,
    stack: &ItemStack,
) -> bool {
    match slot {
        None => true,
        Some(st) => st.same_stackable(stack),
    }
}

/// PICKUP_ALL arm: double-click with a non-empty cursor collects
/// every `same_stackable` stack across the WHOLE screen into the
/// cursor, up to its max stack. Source ordering: two passes over
/// the flat slot list, pass 0 skipping already-FULL stacks (partial
/// stacks are hoovered first), pass 1 taking from full ones. The
/// clicked slot must not hold a pickable item - after the first
/// click of the double-click picked the stack up, it is empty, so
/// the guard degenerates to "clicked slot is empty".
/// (The source's reverse-order `buttonNum != 0` variant has no
/// input binding here and is not ported.)
pub(crate) fn pickup_all(
    sections: &mut [SectionMut<'_>],
    cursor: &mut Option<ItemStack>,
    clicked_gi: usize,
) {
    let Some(cur) = cursor.as_mut() else {
        return;
    };
    if slot_ref(sections, clicked_gi).is_some() {
        return;
    }
    let cap = item_max_stack(cur.id);
    let total = total_slots(sections);
    for pass in 0..2 {
        for gi in 0..total {
            if cur.count >= cap {
                return;
            }
            let Some(st) = slot_ref(sections, gi) else {
                continue;
            };
            if !st.same_stackable(cur) {
                continue;
            }
            // Pass 0 skips full stacks (source line 549).
            if pass == 0 && st.count == item_max_stack(st.id) {
                continue;
            }
            let take = st.count.min(cap - cur.count);
            if take == 0 {
                continue;
            }
            if let Some(slot) = slot_mut(sections, gi)
                && take_n_stacks(slot, take).is_some()
            {
                cur.count += take;
            }
        }
    }
}

/// `getQuickCraftPlaceCount` port for the two mouse buttons: LMB
/// drag splits `floor(count / slots)` per slot, RMB places one.
/// (Type 2 - creative middle-drag full stacks - is not wired to
/// any input and not ported.)
pub(crate) fn quick_craft_place_count(
    n_slots: usize,
    one_per_slot: bool,
    stack: &ItemStack,
) -> u8 {
    if one_per_slot {
        return 1;
    }
    match (stack.count as usize).checked_div(n_slots) {
        Some(per) => per as u8,
        // Degenerate zero-slot call: whole count (MC default arm).
        None => stack.count,
    }
}

/// Add-slot eligibility for the drag tracker (source lines
/// 354-357): the slot must be quick-replaceable (empty or
/// `same_stackable`), must accept the item, and the cursor must
/// hold MORE items than slots already visited.
pub(crate) fn can_add_drag_slot(
    slot: Option<&ItemStack>,
    accept_ok: bool,
    cursor: &ItemStack,
    visited: usize,
) -> bool {
    can_quick_replace(slot, cursor) && accept_ok && (cursor.count as usize) > visited
}

/// QUICK_CRAFT end-of-drag apply (source `quickcraftStatus == 2`
/// arm): distribute the cursor stack over the visited slots.
/// - A single visited slot degrades to a plain PICKUP click with
///   the drag's button (source line 362-366).
/// - Multi-slot: each eligible slot (re-checked with the `>=`
///   apply-time variant) receives `place_count + carry`, capped at
///   the item's max stack; the remainder returns to the cursor.
///   Slots are OVERWRITTEN with a copy of the source stack at the
///   new count (`source.copyWithCount(newCount)`), so fresh slots
///   clone the cursor's components.
pub(crate) fn quick_craft_apply(
    sections: &mut [SectionMut<'_>],
    cursor: &mut Option<ItemStack>,
    visited: &[usize],
    one_per_slot: bool,
) {
    if visited.is_empty() {
        return;
    }
    if visited.len() == 1 {
        pickup_click(sections, cursor, visited[0], one_per_slot);
        return;
    }
    let Some(source) = cursor.clone() else {
        return;
    };
    if source.count == 0 {
        *cursor = None;
        return;
    }
    let n = visited.len();
    let mut remaining = source.count as i32;
    for &gi in visited {
        let slot_now = slot_ref(sections, gi);
        // Apply-time eligibility: `>=` here vs `>` at add time
        // (source line 382 vs 356).
        let ok = can_quick_replace(slot_now, &source)
            && accepts(sections, gi, source.id)
            && (source.count as usize) >= n;
        if !ok {
            continue;
        }
        let carry = slot_now.map(|st| st.count).unwrap_or(0);
        let max_size = item_max_stack(source.id);
        let new_count = quick_craft_place_count(n, one_per_slot, &source)
            .saturating_add(carry)
            .min(max_size);
        remaining -= (new_count - carry) as i32;
        if let Some(slot) = slot_mut(sections, gi) {
            *slot = Some(source.with_count(new_count));
        }
    }
    *cursor = (remaining > 0).then(|| source.with_count(remaining.min(255) as u8));
}

/// `moveItemStackTo` port over an EXPLICIT fill order (each MC menu
/// hard-codes its ranges + direction in `quickMoveStack`; adapters
/// express the same routing as an index list). Two phases, exactly
/// like the source:
/// 1. Merge into occupied `same_stackable` slots along the order
///    (no accept check - matching contents imply prior acceptance).
///    Skipped entirely for max-stack-1 items (`isStackable`).
/// 2. Place the remainder into the FIRST empty slot that accepts
///    the item (the source `break`s after one empty slot; with
///    per-item caps the whole remainder always fits it).
///
/// Returns true when anything moved. `stack.count` is reduced in
/// place; the caller re-slots any leftover.
pub(crate) fn move_stack_to_order(
    sections: &mut [SectionMut<'_>],
    stack: &mut ItemStack,
    order: &[usize],
) -> bool {
    let mut changed = false;
    let cap = item_max_stack(stack.id);
    if cap > 1 {
        for &gi in order {
            if stack.count == 0 {
                break;
            }
            let Some(slot) = slot_mut(sections, gi) else {
                continue;
            };
            let Some(st) = slot.as_mut() else {
                continue;
            };
            if !st.same_stackable(stack) {
                continue;
            }
            let total = st.count as u16 + stack.count as u16;
            if total <= cap as u16 {
                st.count = total as u8;
                stack.count = 0;
                changed = true;
            } else if st.count < cap {
                stack.count -= cap - st.count;
                st.count = cap;
                changed = true;
            }
        }
    }
    if stack.count > 0 {
        for &gi in order {
            if slot_ref(sections, gi).is_some() || !accepts(sections, gi, stack.id) {
                continue;
            }
            let put = stack.count.min(cap);
            if let Some(slot) = slot_mut(sections, gi) {
                *slot = Some(stack.with_count(put));
                stack.count -= put;
                changed = true;
            }
            break;
        }
    }
    changed
}

/// QUICK_MOVE arm: shift-click the slot at `source_gi`, moving its
/// stack along `order` (the menu-specific destination routing).
/// The remainder stays in the source slot with its components. The
/// source's repeat-while-same-item loop collapses to one
/// application (no auto-refilling slots in the section model).
/// Returns true when anything moved.
pub(crate) fn quick_move(
    sections: &mut [SectionMut<'_>],
    source_gi: usize,
    order: &[usize],
) -> bool {
    let Some(slot) = slot_mut(sections, source_gi) else {
        return false;
    };
    let Some(mut stack) = slot.take() else {
        return false;
    };
    let changed = move_stack_to_order(sections, &mut stack, order);
    if stack.count > 0
        && let Some(slot) = slot_mut(sections, source_gi)
    {
        *slot = Some(stack);
    }
    changed
}

#[cfg(test)]
#[path = "click_tests.rs"]
mod click_tests;
