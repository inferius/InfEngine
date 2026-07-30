//! Stack types + stack-arithmetic helpers.
//!
//! W466 adds the MC-style `ItemStack` model: `id + count +
//! components` where `components` carries per-stack state (damage,
//! future enchants / custom names). The player inventory holds
//! `Option<ItemStack>` slots; W467 pushed the stack type into the
//! container family (chests / furnaces / brewing stands); W468
//! carried it through drops, the entities save, and the net wire
//! (inventory snapshots, container state, per-viewer cursors), so
//! the arithmetic helpers below are stack-typed end to end. What
//! still moves plain `(u16, u8)` tuples - the craft grids +
//! `crafting_table_held`, stonecutter / smithing inputs,
//! `armor_slots`, trades - converts at the boundary via
//! `from_tuple` / `as_tuple` until their own waves.

use serde::{Deserialize, Serialize};

use super::item_max_stack;

/// Per-stack component data - the MC `DataComponents` subset this
/// engine tracks. A default (all-empty) value means "plain stack";
/// stacks merge only when their components match exactly.
///
/// Serialized inside `ItemStack` in the player save (bincode,
/// positional - new fields need a format bump + migration arrow).
/// The `#[serde(default)]` attributes only help JSON-shaped
/// consumers; they do NOT make bincode forward-compatible.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct ItemComponents {
    /// Wear counter, counting UP from 0 (MC `damage` component).
    /// The item breaks when damage reaches the item's max uses
    /// (`FLINT_AND_STEEL_MAX_USES` / `tool_max_durability`).
    #[serde(default)]
    pub damage: u16,
    /// `(enchant_id, level)` pairs (MC `enchantments`). Unused
    /// until the enchanting wave lands; part of the schema now so
    /// the player-save format doesn't need another bump for it.
    #[serde(default)]
    pub enchants: Vec<(u8, u8)>,
    /// Custom display name (MC `custom_name`). Unused until an
    /// anvil-style rename lands.
    #[serde(default)]
    pub name: Option<String>,
}

impl ItemComponents {
    /// True when every component is at its default - the stack
    /// behaves like a plain `(id, count)` tuple.
    pub(crate) fn is_empty(&self) -> bool {
        self.damage == 0 && self.enchants.is_empty() && self.name.is_none()
    }
}

/// One inventory stack: item id + count + per-stack components.
/// The W466 replacement for the `(BlockId, u8)` slot tuple.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct ItemStack {
    /// Item id (the shared block/item id space; `BlockId.0`).
    pub id: u16,
    /// Stack count. Saturates at the item's max stack.
    pub count: u8,
    /// Per-stack component data (damage, enchants, name).
    pub components: ItemComponents,
}

impl ItemStack {
    /// Fresh stack with default (empty) components.
    pub(crate) fn new(id: u16, count: u8) -> Self {
        Self {
            id,
            count,
            components: ItemComponents::default(),
        }
    }

    /// Lift a plain `(id, count)` tuple into a stack with empty
    /// components. The boundary conversion for the consumers that
    /// still move tuples (craft grids, stonecutter / smithing,
    /// trades, legacy save shapes).
    pub(crate) fn from_tuple((id, count): (u16, u8)) -> Self {
        Self::new(id, count)
    }

    /// Project the stack down to a plain `(id, count)` tuple -
    /// drops the components. The other half of the boundary.
    pub(crate) fn as_tuple(&self) -> (u16, u8) {
        (self.id, self.count)
    }

    /// Clone of this stack with a different count. The standard
    /// way to split / decrement without touching components.
    pub(crate) fn with_count(&self, count: u8) -> Self {
        Self {
            id: self.id,
            count,
            components: self.components.clone(),
        }
    }

    /// True when `other` can merge into this stack: equal id AND
    /// equal components. MC stacks only combine when their
    /// component data matches exactly - a worn tool never merges
    /// into a fresh one.
    pub(crate) fn same_stackable(&self, other: &ItemStack) -> bool {
        self.id == other.id && self.components == other.components
    }
}

/// Write a plain `(id, count)` tuple back into a stack slot,
/// PRESERVING the slot's existing components when the id is
/// unchanged. The W466 snapshot write-back rule: UI panels + net
/// snapshots move tuples, so a count-only change (take 3 cobble)
/// must not strip the damage off an untouched tool sitting in the
/// same slot. When the id changed or the slot emptied, the tuple
/// wins wholesale (fresh components).
pub(crate) fn write_tuple_preserving(
    slot: &mut Option<ItemStack>,
    tuple: Option<(u16, u8)>,
) {
    match (slot.as_mut(), tuple) {
        (Some(st), Some((id, count))) if st.id == id => {
            st.count = count;
        }
        (_, Some((id, count))) => {
            *slot = Some(ItemStack::new(id, count));
        }
        (_, None) => {
            *slot = None;
        }
    }
}

/// Merge `incoming` into `slot` up to the destination item's
/// `max_stack`. Returns the leftover count that couldn't fit (0 when
/// the merge was complete). The leftover conceptually keeps
/// `incoming`'s components - only the count comes back, the caller
/// still holds the incoming stack.
///
/// Rules (W467 components-aware lift of the old tuple `merge_into`):
/// - Empty slot adopts the incoming stack - components included -
///   up to `max_stack`; the rest is leftover.
/// - A `same_stackable` slot (equal id AND equal components) tops
///   up to `max_stack`; the rest is leftover.
/// - Any other slot (different id OR different components - a worn
///   tool never merges into a fresh one) rejects the whole stack;
///   full count is leftover and the slot is untouched.
pub(crate) fn merge_into_stacks(
    slot: &mut Option<ItemStack>,
    incoming: ItemStack,
) -> u8 {
    if incoming.count == 0 {
        return 0;
    }
    let cap = item_max_stack(incoming.id);
    match slot {
        None => {
            let take = incoming.count.min(cap);
            let leftover = incoming.count - take;
            *slot = Some(incoming.with_count(take));
            leftover
        }
        Some(st) if st.same_stackable(&incoming) => {
            let room = cap.saturating_sub(st.count);
            let take = incoming.count.min(room);
            st.count += take;
            incoming.count - take
        }
        Some(_) => incoming.count,
    }
}

/// Split a slot's stack in half. Returns the moved-out half (the
/// smaller half on odd counts - leaves the larger half in the
/// source slot so a misclick doesn't punish the player). Both
/// halves keep the stack's components (cloned). Leaves the slot
/// with the remainder, or `None` if the original count was 1 (in
/// which case the lone item is moved and the slot clears).
pub(crate) fn split_half_stacks(
    slot: &mut Option<ItemStack>,
) -> Option<ItemStack> {
    let st = slot.as_ref()?.clone();
    if st.count == 0 {
        *slot = None;
        return None;
    }
    // floor(count / 2) moved out; ceil stays. count==1 -> moved=0
    // is a degenerate "no split possible" - treat as moving the
    // lone item so the slot clears (matches the "singleton clears"
    // test contract).
    let moved = if st.count == 1 { 1 } else { st.count / 2 };
    let remaining = st.count - moved;
    if remaining == 0 {
        *slot = None;
    } else {
        *slot = Some(st.with_count(remaining));
    }
    Some(st.with_count(moved))
}

/// Take exactly `n` items from a slot. Returns `None` (and leaves
/// the slot untouched) if the slot has fewer than `n`, or if it's
/// empty. The slot is cleared to `None` when the take exhausts it.
/// The taken part clones the slot's components (a half-taken worn
/// stack is worn on both sides).
pub(crate) fn take_n_stacks(
    slot: &mut Option<ItemStack>,
    n: u8,
) -> Option<ItemStack> {
    if n == 0 {
        return None;
    }
    let st = slot.as_ref()?.clone();
    if st.count < n {
        return None;
    }
    let remaining = st.count - n;
    if remaining == 0 {
        *slot = None;
    } else {
        *slot = Some(st.with_count(remaining));
    }
    Some(st.with_count(n))
}

/// Drop a stack into the inventory: top up `same_stackable` non-full
/// slots first (cheapest move + keeps slot count low), then fall
/// back to empty slots, each fresh slot cloning the incoming
/// components. Returns the leftover count - 0 when the whole stack
/// landed, otherwise the surplus the caller has to spill (drop on
/// the floor, refuse the action, ...).
///
/// Mirrors the two-pass behaviour of `inventory::inv_pickup` but is
/// stack-typed and honours the item's per-id `max_stack`. The
/// component rule means a worn tool passing through here never
/// count-merges into a fresh one - it claims its own slot.
pub(crate) fn drop_into_first_free_stacks(
    inv: &mut [Option<ItemStack>],
    item: ItemStack,
) -> u8 {
    let mut remaining = item.count;
    if remaining == 0 {
        return 0;
    }
    let cap = item_max_stack(item.id);
    // Pass 1: top up existing same-stackable stacks.
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return 0;
        }
        if let Some(st) = slot.as_mut()
            && st.same_stackable(&item)
            && st.count < cap
        {
            let room = cap - st.count;
            let take = remaining.min(room);
            st.count += take;
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
            *slot = Some(item.with_count(take));
            remaining -= take;
        }
    }
    remaining
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shorthand: a worn variant of `ItemStack::new`.
    fn worn(id: u16, count: u8, damage: u16) -> ItemStack {
        let mut st = ItemStack::new(id, count);
        st.components.damage = damage;
        st
    }

    #[test]
    fn components_default_is_empty() {
        let c = ItemComponents::default();
        assert!(c.is_empty());
        let worn = ItemComponents {
            damage: 3,
            ..Default::default()
        };
        assert!(!worn.is_empty());
        let named = ItemComponents {
            name: Some("Excalibur".to_string()),
            ..Default::default()
        };
        assert!(!named.is_empty());
    }

    #[test]
    fn from_tuple_as_tuple_round_trip() {
        let st = ItemStack::from_tuple((48, 3));
        assert_eq!(st.id, 48);
        assert_eq!(st.count, 3);
        assert!(st.components.is_empty());
        assert_eq!(st.as_tuple(), (48, 3));
        // `new` and `from_tuple` agree.
        assert_eq!(st, ItemStack::new(48, 3));
    }

    #[test]
    fn same_stackable_requires_equal_components() {
        let plain_a = ItemStack::new(7, 10);
        let plain_b = ItemStack::new(7, 32);
        // Same id + both default components -> mergeable
        // regardless of count.
        assert!(plain_a.same_stackable(&plain_b));
        // Different id -> never.
        assert!(!plain_a.same_stackable(&ItemStack::new(8, 10)));
        // Same id, one worn -> not mergeable.
        assert!(!plain_a.same_stackable(&worn(7, 1, 5)));
        // Two stacks worn by the SAME amount -> mergeable.
        assert!(worn(7, 1, 5).same_stackable(&worn(7, 3, 5)));
    }

    #[test]
    fn write_tuple_preserving_keeps_components_on_same_id() {
        let mut slot = Some(worn(48, 1, 20));
        // Count-only change keeps the damage.
        write_tuple_preserving(&mut slot, Some((48, 1)));
        assert_eq!(slot.as_ref().unwrap().components.damage, 20);
        // Id change replaces wholesale (fresh components).
        write_tuple_preserving(&mut slot, Some((7, 12)));
        assert_eq!(slot, Some(ItemStack::new(7, 12)));
        // None clears.
        write_tuple_preserving(&mut slot, None);
        assert!(slot.is_none());
        // Writing into an empty slot mints a fresh stack.
        write_tuple_preserving(&mut slot, Some((3, 4)));
        assert_eq!(slot, Some(ItemStack::new(3, 4)));
    }

    #[test]
    fn merge_into_stacks_zero_incoming_is_noop() {
        let mut slot = None;
        assert_eq!(merge_into_stacks(&mut slot, ItemStack::new(3, 0)), 0);
        assert!(slot.is_none());
    }

    #[test]
    fn merge_into_stacks_empty_slot_adopts_components() {
        let mut slot: Option<ItemStack> = None;
        let leftover = merge_into_stacks(&mut slot, worn(48, 1, 9));
        assert_eq!(leftover, 0);
        assert_eq!(slot, Some(worn(48, 1, 9)));
    }

    #[test]
    fn merge_into_stacks_requires_same_stackable() {
        // Fresh incoming onto a worn slot: full reject, slot
        // untouched (a worn tool never merges into a fresh one).
        let mut slot = Some(worn(7, 4, 5));
        let leftover = merge_into_stacks(&mut slot, ItemStack::new(7, 3));
        assert_eq!(leftover, 3);
        assert_eq!(slot, Some(worn(7, 4, 5)));
        // Equal-components incoming tops up.
        let leftover = merge_into_stacks(&mut slot, worn(7, 3, 5));
        assert_eq!(leftover, 0);
        assert_eq!(slot, Some(worn(7, 7, 5)));
    }

    #[test]
    fn merge_into_stacks_tops_up_to_cap() {
        let mut slot = Some(ItemStack::new(3, 60));
        let leftover = merge_into_stacks(&mut slot, ItemStack::new(3, 10));
        assert_eq!(leftover, 6);
        assert_eq!(slot, Some(ItemStack::new(3, 64)));
        // Different item stays fully rejected.
        let leftover = merge_into_stacks(&mut slot, ItemStack::new(2, 20));
        assert_eq!(leftover, 20);
    }

    #[test]
    fn split_half_stacks_singleton_clears_slot() {
        let mut slot = Some(ItemStack::new(3, 1));
        assert_eq!(split_half_stacks(&mut slot), Some(ItemStack::new(3, 1)));
        assert!(slot.is_none());
    }

    #[test]
    fn split_half_stacks_clones_components_to_both_halves() {
        let mut slot = Some(worn(9, 7, 12));
        let moved = split_half_stacks(&mut slot);
        // floor(7/2) = 3 moved; 4 stays. Both worn by 12.
        assert_eq!(moved, Some(worn(9, 3, 12)));
        assert_eq!(slot, Some(worn(9, 4, 12)));
        // Empty slot -> None.
        let mut empty: Option<ItemStack> = None;
        assert!(split_half_stacks(&mut empty).is_none());
    }

    #[test]
    fn take_n_stacks_zero_returns_none() {
        let mut slot = Some(ItemStack::new(3, 5));
        assert!(take_n_stacks(&mut slot, 0).is_none());
        assert_eq!(slot, Some(ItemStack::new(3, 5)));
    }

    #[test]
    fn take_n_stacks_exact_clears_slot() {
        let mut slot = Some(ItemStack::new(3, 5));
        assert_eq!(take_n_stacks(&mut slot, 5), Some(ItemStack::new(3, 5)));
        assert!(slot.is_none());
    }

    #[test]
    fn take_n_stacks_clones_components_to_both_parts() {
        let mut slot = Some(worn(9, 6, 3));
        let taken = take_n_stacks(&mut slot, 2);
        assert_eq!(taken, Some(worn(9, 2, 3)));
        assert_eq!(slot, Some(worn(9, 4, 3)));
        // Too many -> None, slot untouched.
        assert!(take_n_stacks(&mut slot, 10).is_none());
        assert_eq!(slot, Some(worn(9, 4, 3)));
    }

    #[test]
    fn drop_into_first_free_stacks_tops_up_then_claims_empty() {
        let mut inv: Vec<Option<ItemStack>> =
            vec![Some(ItemStack::new(3, 62)), None, None];
        let leftover =
            drop_into_first_free_stacks(&mut inv, ItemStack::new(3, 10));
        assert_eq!(leftover, 0);
        assert_eq!(inv[0], Some(ItemStack::new(3, 64)));
        assert_eq!(inv[1], Some(ItemStack::new(3, 8)));
        assert_eq!(inv[2], None);
    }

    #[test]
    fn drop_into_first_free_stacks_worn_never_merges_into_fresh() {
        let mut inv: Vec<Option<ItemStack>> =
            vec![Some(ItemStack::new(48, 1)), None];
        let leftover = drop_into_first_free_stacks(&mut inv, worn(48, 1, 7));
        assert_eq!(leftover, 0);
        // Fresh stack untouched; worn stack claimed the empty slot
        // with its components intact.
        assert_eq!(inv[0], Some(ItemStack::new(48, 1)));
        assert_eq!(inv[1], Some(worn(48, 1, 7)));
    }

    #[test]
    fn drop_into_first_free_stacks_leftover_when_full() {
        let mut inv: Vec<Option<ItemStack>> = vec![Some(ItemStack::new(2, 64))];
        let leftover =
            drop_into_first_free_stacks(&mut inv, ItemStack::new(4, 5));
        assert_eq!(leftover, 5);
        assert_eq!(inv[0], Some(ItemStack::new(2, 64)));
    }
}
