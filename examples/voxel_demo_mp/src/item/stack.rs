//! Stack types + stack-arithmetic helpers.
//!
//! W466 adds the MC-style `ItemStack` model: `id + count +
//! components` where `components` carries per-stack state (damage,
//! future enchants / custom names). The player inventory holds
//! `Option<ItemStack>` slots; everything outside the inventory core
//! (containers, drops, net messages, trades) still moves plain
//! `(u16, u8)` tuples in W466 and converts at the boundary via
//! `from_tuple` / `as_tuple` - the W467/W468 waves push the stack
//! type outward.
//!
//! The tuple-typed arithmetic helpers below (`merge_into`,
//! `split_half`, `take_n`, `drop_into_first_free`) predate the
//! stack type and still serve the container/chest code, which is
//! tuple-shaped until W467.

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
    /// components. The W466 boundary conversion for containers /
    /// drops / net messages that still move tuples.
    pub(crate) fn from_tuple((id, count): (u16, u8)) -> Self {
        Self::new(id, count)
    }

    /// Project the stack down to a plain `(id, count)` tuple -
    /// drops the components. The other half of the W466 boundary.
    pub(crate) fn as_tuple(&self) -> (u16, u8) {
        (self.id, self.count)
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
        let mut worn = ItemStack::new(7, 1);
        worn.components.damage = 5;
        assert!(!plain_a.same_stackable(&worn));
        // Two stacks worn by the SAME amount -> mergeable.
        let mut worn_b = ItemStack::new(7, 1);
        worn_b.components.damage = 5;
        assert!(worn.same_stackable(&worn_b));
    }

    #[test]
    fn write_tuple_preserving_keeps_components_on_same_id() {
        let mut slot = Some(ItemStack {
            id: 48,
            count: 1,
            components: ItemComponents {
                damage: 20,
                ..Default::default()
            },
        });
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
