//! Generic container - one or more slots of `Option<ItemStack>`
//! plus the ops needed to move stacks between slots. Chests,
//! furnaces, brewing stands, and the player inventory all use this
//! shape; what differs is the slot count + the per-slot validation
//! predicate.
//!
//! W467: slots hold full `ItemStack`s (id + count + components), so
//! a damaged tool parked in a chest keeps its damage and two
//! differently-worn tools never count-merge (`same_stackable` is
//! the merge gate everywhere). The arithmetic is delegated to
//! `item::stack::*` so the per-item `max_stack` from the item
//! registry stays the single source of truth. This module's
//! contribution is the multi-slot orchestration: smart-stack
//! insertion across N slots, swap with accept-filter validation,
//! and cross-container transfer.

pub(crate) mod ui;

#[cfg(test)]
mod container_tests;

use crate::item::ItemStack;
use crate::item::stack::{merge_into_stacks, take_n_stacks};

/// One slot's payload. `None` = empty.
pub(crate) type Slot = Option<ItemStack>;

/// Per-slot accept filter. Returns `true` when the predicate
/// allows `item_id` to live in the slot. Filters stay id-keyed -
/// what fits in a furnace fuel slot is a property of the item
/// kind, not of per-stack components.
pub(crate) type AcceptFn = fn(u16) -> bool;

/// Generic N-slot container. Backs chests (27 slots, no accept
/// filter), furnaces (3 slots: input / fuel / output, each with a
/// different accept predicate), and brewing stands (5 slots).
///
/// Stack arithmetic respects each item's `max_stack` from the item
/// registry; this struct just glues the per-slot ops together.
#[derive(Debug, Clone, Default)]
pub(crate) struct Container {
    slots: Vec<Slot>,
    /// Optional per-slot accept filter. `None` means "anything fits
    /// in any slot" (chests). Furnace input slot accepts only
    /// smeltable items; output slot accepts only the smelt result.
    /// Indices outside the Vec default to "accept any".
    accept: Vec<Option<AcceptFn>>,
}

impl Container {
    /// Build an empty container with `n` slots and no accept
    /// filters. Use `set_accept` to lock slots to a predicate.
    pub(crate) fn with_size(n: usize) -> Self {
        Self {
            slots: vec![None; n],
            accept: Vec::new(),
        }
    }

    /// Read-only view of every slot. Index order is meaningful -
    /// callers can rely on it (e.g. furnace.slot(0) = input).
    pub(crate) fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// Read one slot as an owned clone. Returns `None` for both
    /// "empty slot" and "index out of range"; callers that care
    /// about the distinction should consult `len()` first.
    /// Containers are small, so the clone-returning accessor keeps
    /// call sites simple; use `slot_ref` when a borrow suffices.
    pub(crate) fn slot(&self, i: usize) -> Slot {
        self.slots.get(i).cloned().unwrap_or(None)
    }

    /// Borrowing variant of `slot` - the render / gate paths that
    /// only inspect id + count use this to skip the clone.
    pub(crate) fn slot_ref(&self, i: usize) -> Option<&ItemStack> {
        self.slots.get(i).and_then(|s| s.as_ref())
    }

    /// `(id, count)` projection of one slot - for id-keyed gates
    /// (fuel checks, timers) and count-math assertions that don't
    /// care about components.
    pub(crate) fn slot_tuple(&self, i: usize) -> Option<(u16, u8)> {
        self.slot_ref(i).map(|st| st.as_tuple())
    }

    /// Overwrite one slot. No accept-filter check - this is the raw
    /// setter intended for save-load + admin paths. Use `insert_at`
    /// for user-driven inserts.
    pub(crate) fn set_slot(&mut self, i: usize, slot: Slot) {
        if let Some(s) = self.slots.get_mut(i) {
            *s = slot;
        }
    }

    /// Total slot count.
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// True when every slot is empty (NOT the same as `len()==0`).
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }

    /// Lock slot `i` to a predicate. Items that don't satisfy the
    /// predicate are rejected by `insert_at` / `swap`. Calling this
    /// with an `i` larger than `len()` grows the `accept` vec on
    /// demand - useful when filters are set up before the slot
    /// count is finalised.
    pub(crate) fn set_accept(&mut self, i: usize, pred: AcceptFn) {
        if self.accept.len() <= i {
            self.accept.resize(i + 1, None);
        }
        self.accept[i] = Some(pred);
    }

    /// Does slot `i` accept `item_id`? Indices without a filter
    /// (the common chest case) always accept.
    pub(crate) fn accepts(&self, i: usize, item_id: u16) -> bool {
        match self.accept.get(i).copied().flatten() {
            Some(pred) => pred(item_id),
            None => true,
        }
    }

    /// Insert as much of `incoming` as possible across all slots.
    /// Fills existing `same_stackable` partial stacks first (equal
    /// id AND equal components - a worn tool never tops up a fresh
    /// stack), then empty slots, each new slot cloning the incoming
    /// components. Returns leftover count (0 = full merge); the
    /// leftover conceptually keeps the incoming components. Slots
    /// blocked by an accept filter are skipped.
    ///
    /// This is the "pickup an item from the world" entry point:
    /// the floor drop walks every slot to find a home.
    pub(crate) fn insert(&mut self, incoming: ItemStack) -> u8 {
        if incoming.count == 0 {
            return 0;
        }
        let cap = crate::item::item_max_stack(incoming.id);
        let mut remaining = incoming.count;
        // Pass 1: top up matching partial stacks ONLY. Skip empties
        // here so we keep slot count low - matches Minecraft's
        // pickup behaviour.
        for i in 0..self.slots.len() {
            if remaining == 0 {
                return 0;
            }
            if !self.accepts(i, incoming.id) {
                continue;
            }
            if let Some(st) = self.slots[i].as_mut()
                && st.same_stackable(&incoming)
                && st.count < cap
            {
                let take = remaining.min(cap - st.count);
                st.count += take;
                remaining -= take;
            }
        }
        // Pass 2: claim empty slots, filter-aware, splitting across
        // multiple slots when `count > max_stack`. Each fresh slot
        // clones the incoming components.
        for i in 0..self.slots.len() {
            if remaining == 0 {
                return 0;
            }
            if !self.accepts(i, incoming.id) {
                continue;
            }
            if self.slots[i].is_some() {
                continue;
            }
            let take = remaining.min(cap);
            self.slots[i] = Some(incoming.with_count(take));
            remaining -= take;
        }
        remaining
    }

    /// Insert into a specific slot. Returns leftover. Honours the
    /// per-slot accept filter: if `incoming.id` is rejected, the
    /// full count comes back as leftover and the slot is untouched.
    /// Merging requires `same_stackable` (see `merge_into_stacks`).
    pub(crate) fn insert_at(&mut self, i: usize, incoming: ItemStack) -> u8 {
        if incoming.count == 0 {
            return 0;
        }
        if !self.accepts(i, incoming.id) {
            return incoming.count;
        }
        let Some(slot) = self.slots.get_mut(i) else {
            return incoming.count;
        };
        merge_into_stacks(slot, incoming)
    }

    /// Take everything in slot `i`. Returns the contents (or `None`
    /// when the slot was already empty or `i` is out of range).
    pub(crate) fn take(&mut self, i: usize) -> Slot {
        match self.slots.get_mut(i) {
            Some(s) => s.take(),
            None => None,
        }
    }

    /// Take exactly `n` from slot `i`. Returns `None` (and leaves
    /// the slot untouched) when the slot has fewer than `n` or is
    /// empty / out of range. Both the taken part and the remainder
    /// keep the stack's components (cloned).
    pub(crate) fn take_n(&mut self, i: usize, n: u8) -> Slot {
        let slot = self.slots.get_mut(i)?;
        take_n_stacks(slot, n)
    }

    /// Swap two slots' contents. Returns `false` when the swap
    /// would violate either side's accept filter (and leaves both
    /// slots untouched). Same-index calls succeed as a no-op.
    /// Components travel with their stacks.
    pub(crate) fn swap(&mut self, a: usize, b: usize) -> bool {
        if a == b {
            return true;
        }
        if a >= self.slots.len() || b >= self.slots.len() {
            return false;
        }
        // Validate both directions before mutating.
        if let Some(st) = self.slots[a].as_ref()
            && !self.accepts(b, st.id)
        {
            return false;
        }
        if let Some(st) = self.slots[b].as_ref()
            && !self.accepts(a, st.id)
        {
            return false;
        }
        self.slots.swap(a, b);
        true
    }

    /// Move the full contents of slot `from` of `self` into
    /// `other` (smart-stack: existing stacks first, then empties).
    /// Returns the count actually moved. Stops short when `other`
    /// has no room - the remainder stays in the source slot with
    /// its components intact.
    ///
    /// This is the shift-click / "quick-move" operation:
    /// `player.transfer_from(slot, &mut chest)` for player -> chest,
    /// `chest.transfer_from(slot, &mut player)` for the reverse.
    pub(crate) fn transfer_from(
        &mut self,
        from: usize,
        other: &mut Container,
    ) -> u8 {
        let Some(source_slot) = self.slots.get_mut(from) else {
            return 0;
        };
        let Some(stack) = source_slot.clone() else {
            return 0;
        };
        // Try to deposit the whole stack into `other`.
        let leftover = other.insert(stack.clone());
        let moved = stack.count - leftover;
        if leftover == 0 {
            *source_slot = None;
        } else {
            *source_slot = Some(stack.with_count(leftover));
        }
        moved
    }

    /// Total count of `item_id` across all slots, components
    /// ignored (id-keyed - used for fuel checks etc.). Returns
    /// `u32` so containers with many full stacks of the same item
    /// don't overflow a `u8`.
    pub(crate) fn count_of(&self, item_id: u16) -> u32 {
        let mut total: u32 = 0;
        for st in self.slots.iter().flatten() {
            if st.id == item_id {
                total += st.count as u32;
            }
        }
        total
    }

    /// Free space (in items) for a PLAIN stack of `item_id` across
    /// all slots: top-up room only in matching stacks whose
    /// components are empty (a plain incoming item cannot merge
    /// into a worn stack), plus full-cap room in empty slots. Skips
    /// slots blocked by an accept filter.
    pub(crate) fn space_for(&self, item_id: u16) -> u32 {
        let cap = crate::item::item_max_stack(item_id) as u32;
        let mut total: u32 = 0;
        for (i, slot) in self.slots.iter().enumerate() {
            if !self.accepts(i, item_id) {
                continue;
            }
            match slot {
                None => total += cap,
                Some(st) if st.id == item_id && st.components.is_empty() => {
                    total += cap - (st.count as u32).min(cap);
                }
                Some(_) => {}
            }
        }
        total
    }
}
