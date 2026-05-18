//! Tests for the generic `Container` API.
//!
//! Co-located with the source per the file-size policy. Mounted from
//! `container/mod.rs` under `#[cfg(test)]`. The widget code in
//! `container/ui.rs` is NOT tested here - that would need a render
//! context. The integration story lands in C.3 when the chest UI
//! is wired up.

use super::Container;

// ---------------------------------------------------------------
// Construction.
// ---------------------------------------------------------------

#[test]
fn with_size_initialises_empty() {
    let c = Container::with_size(27);
    assert_eq!(c.len(), 27);
    assert!(c.is_empty());
    // Every slot reads back as None.
    for i in 0..c.len() {
        assert_eq!(c.slot(i), None);
    }
}

// ---------------------------------------------------------------
// insert - multi-slot smart-stack.
// ---------------------------------------------------------------

#[test]
fn insert_into_empty_uses_first_slot() {
    let mut c = Container::with_size(3);
    let leftover = c.insert((3, 10));
    assert_eq!(leftover, 0);
    assert_eq!(c.slot(0), Some((3, 10)));
    assert_eq!(c.slot(1), None);
    assert_eq!(c.slot(2), None);
}

#[test]
fn insert_fills_partial_stacks_first() {
    // Two partial same-item slots and an empty slot. Incoming
    // (3, 10) should top up slot 0 before touching slot 2.
    let mut c = Container::with_size(3);
    c.set_slot(0, Some((3, 60))); // room 4
    c.set_slot(2, Some((3, 50))); // room 14
    let leftover = c.insert((3, 10));
    assert_eq!(leftover, 0);
    // 4 absorbed into slot 0, 6 into slot 2.
    assert_eq!(c.slot(0), Some((3, 64)));
    assert_eq!(c.slot(1), None);
    assert_eq!(c.slot(2), Some((3, 56)));
}

#[test]
fn insert_overflows_into_empty() {
    // Existing partial that doesn't fit the full incoming count.
    // The remainder lands in the first empty slot.
    let mut c = Container::with_size(3);
    c.set_slot(0, Some((3, 62))); // room 2
    let leftover = c.insert((3, 10));
    assert_eq!(leftover, 0);
    assert_eq!(c.slot(0), Some((3, 64)));
    // 8 left over -> first empty slot.
    assert_eq!(c.slot(1), Some((3, 8)));
    assert_eq!(c.slot(2), None);
}

#[test]
fn insert_returns_leftover_when_full() {
    // Two slots, both full of different items: there's no room for
    // sand. Full count comes back as leftover.
    let mut c = Container::with_size(2);
    c.set_slot(0, Some((1, 64)));
    c.set_slot(1, Some((2, 64)));
    let leftover = c.insert((4, 5));
    assert_eq!(leftover, 5);
    assert_eq!(c.slot(0), Some((1, 64)));
    assert_eq!(c.slot(1), Some((2, 64)));
}

// ---------------------------------------------------------------
// insert_at - per-slot accept filter.
// ---------------------------------------------------------------

#[test]
fn insert_at_respects_accept_filter() {
    let mut c = Container::with_size(2);
    // Slot 0 accepts only stone (id 1).
    fn only_stone(id: u16) -> bool {
        id == 1
    }
    c.set_accept(0, only_stone);
    // Reject dirt into slot 0 - full count is leftover.
    let leftover = c.insert_at(0, (2, 5));
    assert_eq!(leftover, 5);
    assert_eq!(c.slot(0), None);
    // Stone is accepted into slot 0.
    let leftover = c.insert_at(0, (1, 5));
    assert_eq!(leftover, 0);
    assert_eq!(c.slot(0), Some((1, 5)));
    // Slot 1 has no filter - dirt is fine.
    let leftover = c.insert_at(1, (2, 5));
    assert_eq!(leftover, 0);
    assert_eq!(c.slot(1), Some((2, 5)));
}

// ---------------------------------------------------------------
// swap.
// ---------------------------------------------------------------

#[test]
fn swap_swaps_both_directions() {
    let mut c = Container::with_size(3);
    c.set_slot(0, Some((1, 5)));
    c.set_slot(1, Some((2, 7)));
    assert!(c.swap(0, 1));
    assert_eq!(c.slot(0), Some((2, 7)));
    assert_eq!(c.slot(1), Some((1, 5)));
    // Swap with an empty slot also works.
    assert!(c.swap(1, 2));
    assert_eq!(c.slot(1), None);
    assert_eq!(c.slot(2), Some((1, 5)));
    // Self-swap is a no-op success.
    assert!(c.swap(0, 0));
    assert_eq!(c.slot(0), Some((2, 7)));
}

#[test]
fn swap_refuses_when_accept_filter_blocks() {
    let mut c = Container::with_size(2);
    fn only_stone(id: u16) -> bool {
        id == 1
    }
    c.set_accept(0, only_stone);
    c.set_slot(0, Some((1, 5))); // stone in stone-only slot
    c.set_slot(1, Some((2, 7))); // dirt in unrestricted slot
    // Swap would put dirt in the stone-only slot -> refused.
    assert!(!c.swap(0, 1));
    // Both slots untouched.
    assert_eq!(c.slot(0), Some((1, 5)));
    assert_eq!(c.slot(1), Some((2, 7)));
}

// ---------------------------------------------------------------
// transfer_from.
// ---------------------------------------------------------------

#[test]
fn transfer_from_moves_full_stack() {
    let mut src = Container::with_size(2);
    let mut dst = Container::with_size(2);
    src.set_slot(0, Some((3, 10)));
    let moved = src.transfer_from(0, &mut dst);
    assert_eq!(moved, 10);
    assert_eq!(src.slot(0), None);
    assert_eq!(dst.slot(0), Some((3, 10)));
}

#[test]
fn transfer_from_partial_when_target_full() {
    // Destination has just enough room for some of the stack; the
    // rest stays in the source slot.
    let mut src = Container::with_size(1);
    let mut dst = Container::with_size(2);
    src.set_slot(0, Some((3, 30)));
    dst.set_slot(0, Some((3, 60))); // room 4
    dst.set_slot(1, Some((1, 64))); // full, wrong item
    let moved = src.transfer_from(0, &mut dst);
    assert_eq!(moved, 4);
    // 26 remain in the source.
    assert_eq!(src.slot(0), Some((3, 26)));
    assert_eq!(dst.slot(0), Some((3, 64)));
    assert_eq!(dst.slot(1), Some((1, 64)));
}

#[test]
fn transfer_from_empty_slot_is_noop() {
    let mut src = Container::with_size(1);
    let mut dst = Container::with_size(1);
    let moved = src.transfer_from(0, &mut dst);
    assert_eq!(moved, 0);
    assert_eq!(src.slot(0), None);
    assert_eq!(dst.slot(0), None);
}

// ---------------------------------------------------------------
// take / take_n.
// ---------------------------------------------------------------

#[test]
fn take_clears_slot() {
    let mut c = Container::with_size(1);
    c.set_slot(0, Some((3, 7)));
    let taken = c.take(0);
    assert_eq!(taken, Some((3, 7)));
    assert_eq!(c.slot(0), None);
    // Taking from an already-empty slot returns None.
    assert_eq!(c.take(0), None);
}

#[test]
fn take_n_partial_leaves_remainder() {
    let mut c = Container::with_size(1);
    c.set_slot(0, Some((3, 10)));
    let taken = c.take_n(0, 4);
    assert_eq!(taken, Some((3, 4)));
    assert_eq!(c.slot(0), Some((3, 6)));
}

#[test]
fn take_n_exact_clears_slot() {
    let mut c = Container::with_size(1);
    c.set_slot(0, Some((3, 5)));
    let taken = c.take_n(0, 5);
    assert_eq!(taken, Some((3, 5)));
    assert_eq!(c.slot(0), None);
}

#[test]
fn take_n_too_many_returns_none() {
    // Asking for more than the slot has must leave the slot
    // untouched - matches the `item::stack::take_n` contract.
    let mut c = Container::with_size(1);
    c.set_slot(0, Some((3, 5)));
    let taken = c.take_n(0, 10);
    assert!(taken.is_none());
    assert_eq!(c.slot(0), Some((3, 5)));
}

// ---------------------------------------------------------------
// count_of / space_for.
// ---------------------------------------------------------------

#[test]
fn count_of_sums_across_slots() {
    let mut c = Container::with_size(4);
    c.set_slot(0, Some((3, 10)));
    c.set_slot(1, Some((1, 64))); // different item
    c.set_slot(2, Some((3, 25)));
    c.set_slot(3, None);
    assert_eq!(c.count_of(3), 35);
    assert_eq!(c.count_of(1), 64);
    // Unknown item - zero.
    assert_eq!(c.count_of(9999), 0);
}

#[test]
fn space_for_subtracts_existing() {
    // 3 slots: one partial same-item, one full same-item, one empty.
    // Space for item 3 = (room in partial) + (full cap of empty).
    let mut c = Container::with_size(3);
    c.set_slot(0, Some((3, 40))); // room 24
    c.set_slot(1, Some((3, 64))); // room 0
    c.set_slot(2, None);          // room 64
    // 24 + 0 + 64 = 88.
    assert_eq!(c.space_for(3), 88);
    // For a different item only the empty slot contributes.
    assert_eq!(c.space_for(1), 64);
}

#[test]
fn space_for_respects_accept_filter() {
    // Slot 0 only accepts stone; an empty filtered slot contributes
    // zero space for dirt.
    let mut c = Container::with_size(2);
    fn only_stone(id: u16) -> bool {
        id == 1
    }
    c.set_accept(0, only_stone);
    // Slot 0 + slot 1 both empty.
    assert_eq!(c.space_for(2), 64); // only slot 1 counts
    assert_eq!(c.space_for(1), 128); // both slots count
}
