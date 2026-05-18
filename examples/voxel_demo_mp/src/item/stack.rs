//! Stack-arithmetic helpers typed against `(item_id, count)`.
//!
//! These are the per-item-aware equivalents of the existing
//! `inventory::inv_pickup` / `inv_consume` helpers, which assume a
//! flat `STACK_MAX = 64` and ignore item-specific limits. The new
//! module respects each item's `max_stack` so tools (max_stack=1)
//! can't accidentally merge into a 64-deep pile.
//!
//! C.2 / C.3 will migrate `inventory.rs` + the chest UI onto these.
//! For now they live alongside the legacy helpers without replacing
//! them: nothing in the existing code path calls in here yet.

use super::item_max_stack;

/// Merge `incoming` into `slot` up to the destination item's
/// `max_stack`. Returns the leftover count that couldn't fit (0 when
/// the merge was complete).
///
/// Rules:
/// - Empty slot accepts up to `max_stack`; the rest is leftover.
/// - Same-item slot tops up to `max_stack`; the rest is leftover.
/// - Different-item slot rejects the whole stack; full count is
///   leftover and the slot is untouched.
pub(crate) fn merge_into(
    slot: &mut Option<(u16, u8)>,
    incoming: (u16, u8),
) -> u8 {
    let (id, count) = incoming;
    if count == 0 {
        return 0;
    }
    let cap = item_max_stack(id);
    match slot {
        None => {
            let take = count.min(cap);
            *slot = Some((id, take));
            count - take
        }
        Some((sid, scount)) if *sid == id => {
            let room = cap.saturating_sub(*scount);
            let take = count.min(room);
            *scount += take;
            count - take
        }
        Some(_) => count,
    }
}

/// Split a slot's stack in half. Returns the moved-out half (the
/// smaller half on odd counts - leaves the larger half in the
/// source slot so a misclick doesn't punish the player). Leaves
/// the slot with the remainder, or `None` if the original count
/// was 1 (in which case the lone item is moved and the slot
/// clears).
pub(crate) fn split_half(
    slot: &mut Option<(u16, u8)>,
) -> Option<(u16, u8)> {
    let (id, count) = (*slot)?;
    if count == 0 {
        *slot = None;
        return None;
    }
    // floor(count / 2) moved out; ceil stays. count==1 -> moved=0
    // is a degenerate "no split possible" - treat as moving the
    // lone item so the slot clears (matches the "singleton clears"
    // test contract).
    let moved = if count == 1 { 1 } else { count / 2 };
    let remaining = count - moved;
    if remaining == 0 {
        *slot = None;
    } else {
        *slot = Some((id, remaining));
    }
    Some((id, moved))
}

/// Take exactly `n` items from a slot. Returns `None` (and leaves
/// the slot untouched) if the slot has fewer than `n`, or if it's
/// empty. The slot is cleared to `None` when the take exhausts it.
pub(crate) fn take_n(
    slot: &mut Option<(u16, u8)>,
    n: u8,
) -> Option<(u16, u8)> {
    if n == 0 {
        return None;
    }
    let (id, count) = (*slot)?;
    if count < n {
        return None;
    }
    let remaining = count - n;
    if remaining == 0 {
        *slot = None;
    } else {
        *slot = Some((id, remaining));
    }
    Some((id, n))
}

/// Drop a stack into the inventory: top up matching non-full slots
/// first (cheapest move + keeps slot count low), then fall back to
/// the first empty slot. Returns the leftover count - 0 when the
/// whole stack landed, otherwise the surplus the caller has to spill
/// (drop on the floor, refuse the action, ...).
///
/// Mirrors the two-pass behaviour of `inventory::inv_pickup` but is
/// typed for `(u16, u8)` and honours the item's per-id `max_stack`.
pub(crate) fn drop_into_first_free(
    inv: &mut [Option<(u16, u8)>],
    item: (u16, u8),
) -> u8 {
    let (id, mut remaining) = item;
    if remaining == 0 {
        return 0;
    }
    let cap = item_max_stack(id);
    // Pass 1: top up existing same-item stacks.
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return 0;
        }
        if let Some((sid, scount)) = slot.as_mut()
            && *sid == id
            && *scount < cap
        {
            let room = cap - *scount;
            let take = remaining.min(room);
            *scount += take;
            remaining -= take;
        }
    }
    // Pass 2: claim empty slots, splitting across multiple if the
    // remainder still exceeds `max_stack` (only relevant when
    // someone passes a > max_stack incoming count).
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return 0;
        }
        if slot.is_none() {
            let take = remaining.min(cap);
            *slot = Some((id, take));
            remaining -= take;
        }
    }
    remaining
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_into_zero_incoming_is_noop() {
        let mut slot = None;
        assert_eq!(merge_into(&mut slot, (3, 0)), 0);
        assert!(slot.is_none());
    }

    #[test]
    fn split_half_singleton_clears_slot() {
        let mut slot = Some((3, 1));
        assert_eq!(split_half(&mut slot), Some((3, 1)));
        assert!(slot.is_none());
    }

    #[test]
    fn take_n_zero_returns_none() {
        let mut slot = Some((3, 5));
        assert!(take_n(&mut slot, 0).is_none());
        assert_eq!(slot, Some((3, 5)));
    }

    #[test]
    fn take_n_exact_clears_slot() {
        let mut slot = Some((3, 5));
        assert_eq!(take_n(&mut slot, 5), Some((3, 5)));
        assert!(slot.is_none());
    }
}
