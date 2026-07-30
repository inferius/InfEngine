//! Inventory + hotbar + crafting-grid slot logic.
//!
//! Owns the small but tricky pile of types + helpers that move
//! `ItemStack`s around the client's 36-slot inventory grid + the
//! 2x2 craft grid (stack-typed since W469):
//!
//! - `SlotKind` is the drag&drop payload + wire address - one of
//!   36 inventory cells, one of 4 craft cells, or the single
//!   craft-output cell. Bridges to `net::NetSlot` via the two
//!   `From` impls so client intents serialise cleanly.
//! - `SplitMode` controls how much of a source stack moves on
//!   one drag completion (whole stack, half, single).
//! - `inv_pickup` / `inv_consume` are the small "+1 / -1"
//!   helpers used by mining + crafting outcomes.
//! - `inv_apply_drag` is the server-side mirror of the local UI
//!   drag&drop apply path; both route through `drag_stacks`
//!   (W468) so the client's local prediction and the
//!   authoritative snapshot use one rule set.
//!
//! State lives on `Voxel`: the host's own `inventory`,
//! `selected_slot`, `craft_grid`, plus `saved_inventories` for
//! disconnected players. The host's per-client view is the
//! `ClientInv` struct in `net.rs`.
//!
//! The 2x2 recipe lookup (`match_recipe`) still lives in
//! `main.rs` until the future crafting module carves it out -
//! this module just calls into it through `crate::match_recipe`.

use infengine_voxel::BlockId;
use serde::{Deserialize, Serialize};

use crate::match_recipe;
use crate::net::{ClientInv, NetSlot};

/// Hotbar occupies the first row of the inventory grid; the
/// remaining `INVENTORY_ROWS - 1` rows are the open-inventory
/// extension Minecraft players expect. Every slot is a normal
/// stack slot - mining is driven by LMB regardless of what's
/// held (the held tool only affects mining speed + drops).
pub(crate) const HOTBAR_SLOTS: usize = 9;
pub(crate) const INVENTORY_COLS: usize = 9;
pub(crate) const INVENTORY_ROWS: usize = 4;
pub(crate) const INVENTORY_SLOTS: usize = INVENTORY_COLS * INVENTORY_ROWS;
pub(crate) const STACK_MAX: u8 = 64;

// W470: the click-core screen adapter duplicates the two geometry
// consts (the headless lib mounts `container` but not this
// module). Keep them in lockstep.
const _: () = assert!(
    crate::container::screen::HOTBAR_SLOTS == HOTBAR_SLOTS
        && crate::container::screen::INVENTORY_SLOTS == INVENTORY_SLOTS
);

/// Identifier for an inventory cell. Used as the drag&drop payload
/// so we know both ends of a drag operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SlotKind {
    /// One of the 36 main inventory slots (0..HOTBAR_SLOTS = hotbar,
    /// HOTBAR_SLOTS..INVENTORY_SLOTS = storage rows).
    Inv(usize),
    /// One of the 4 cells in the 2x2 crafting grid.
    Craft(usize),
    /// The single crafting-output cell. Drag-only (you can't drop
    /// stacks onto it).
    CraftOut,
}

/// Quantity of items to move on a single drag completion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SplitMode {
    /// Move every item in the source stack.
    #[default]
    All,
    /// Move ceil(count/2) - lets the player halve down to 1
    /// through repeated shift-drags.
    Half,
    /// Move exactly one item - useful for filling crafting slots
    /// without dumping the rest of the stack.
    One,
}

impl From<SlotKind> for NetSlot {
    fn from(k: SlotKind) -> Self {
        match k {
            SlotKind::Inv(i) => NetSlot::Inv(i as u32),
            SlotKind::Craft(i) => NetSlot::Craft(i as u32),
            SlotKind::CraftOut => NetSlot::CraftOut,
        }
    }
}

impl From<NetSlot> for SlotKind {
    fn from(k: NetSlot) -> Self {
        match k {
            NetSlot::Inv(i) => SlotKind::Inv(i as usize),
            NetSlot::Craft(i) => SlotKind::Craft(i as usize),
            NetSlot::CraftOut => SlotKind::CraftOut,
        }
    }
}

/// W470: shift-click routing for the standalone inventory screen,
/// as global indices over the [inventory 36, craft grid 4] screen
/// (`InventoryMenu.quickMoveStack` conventions adapted to this
/// engine's layout - no armor grid / offhand yet):
/// - craft cell -> player inv, main rows first then hotbar
///   (`moveItemStackTo(9, 45, false)`),
/// - main slot -> hotbar (forward),
/// - hotbar slot -> main rows (forward).
///
/// The craft OUTPUT slot is virtual and returns an empty order.
pub(crate) fn inv_screen_quick_move_order(
    slot: SlotKind,
    inv_len: usize,
) -> Vec<usize> {
    match slot {
        SlotKind::Craft(_) => (HOTBAR_SLOTS..inv_len)
            .chain(0..HOTBAR_SLOTS)
            .collect(),
        SlotKind::Inv(i) if i < HOTBAR_SLOTS => {
            (HOTBAR_SLOTS..inv_len).collect()
        }
        SlotKind::Inv(_) => (0..HOTBAR_SLOTS).collect(),
        SlotKind::CraftOut => Vec::new(),
    }
}

/// Pop one of `block` into a stack-typed inventory. First tries to
/// top up an existing non-full PLAIN stack, then claims the first
/// empty slot. Returns `true` when the block was successfully
/// stored (false means the inventory was completely full + the
/// drop was lost). W468: stack-typed - the tuple inventory shape
/// is gone (`ClientInv` holds `Option<ItemStack>` slots now).
pub(crate) fn inv_pickup(
    inv: &mut [Option<crate::item::ItemStack>; INVENTORY_SLOTS],
    block: BlockId,
) -> bool {
    inv_pickup_n_stacks(inv, block, 1)
}

/// Pop `count` of `block` into the player's `ItemStack`-typed
/// inventory. Tops up partial stacks first, then claims empty
/// slots. Returns `true` if the full count fit; `false` (and some
/// items already merged) when the inventory ran out of room before
/// `count` was exhausted.
///
/// W50.3: used by the crafting-table close handler to dump the
/// 3x3 grid contents back into the player inv when the panel is
/// closed via the X button.
///
/// W466: replaces the tuple-typed `inv_pickup_n` (the player
/// inventory holds stacks now). One components rule: merges only
/// land in PLAIN (default-component) stacks so a returned
/// craft-grid stack never piles onto a worn tool. Freshly claimed
/// slots mint stacks with empty components.
pub(crate) fn inv_pickup_n_stacks(
    inv: &mut [Option<crate::item::ItemStack>; INVENTORY_SLOTS],
    block: BlockId,
    count: u8,
) -> bool {
    if block == BlockId::AIR || count == 0 {
        return true;
    }
    let cap = crate::item::item_max_stack(block.0);
    let mut remaining = count;
    // Pass 1: top up matching plain partial stacks.
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return true;
        }
        if let Some(st) = slot.as_mut()
            && st.id == block.0
            && st.components.is_empty()
            && st.count < cap
        {
            let room = cap - st.count;
            let take = room.min(remaining);
            st.count += take;
            remaining -= take;
        }
    }
    // Pass 2: claim empty slots.
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return true;
        }
        if slot.is_none() {
            let take = cap.min(remaining);
            *slot = Some(crate::item::ItemStack::new(block.0, take));
            remaining -= take;
        }
    }
    remaining == 0
}

/// Drag a stack between two real slots - the ONE implementation of
/// the inventory drag&drop rules, shared by the local UI apply
/// (`Voxel::apply_inv_action`) and the server-side authoritative
/// mirror (`inv_apply_drag`) so the two can never drift. Lives here
/// (not in `item::stack`) because `SplitMode` is bin-only - the
/// headless lib re-mounts `item/` without this module.
///
/// Returns the new `(from, to)` slot values, or `None` when the
/// drag is refused (both slots stay untouched). Rules (W468):
/// - Partial move (`Half` / `One`): merges into a `same_stackable`
///   target up to the item's `max_stack` (surplus returns to the
///   source), moves into an empty target, REFUSES a mismatched
///   target (a split can't swap).
/// - Whole move (`All`): merges into a `same_stackable` target up
///   to `max_stack` (remainder stays in the source); anything else
///   swaps the two slots wholesale. Components travel with their
///   stacks; splits clone them to both halves.
pub(crate) fn drag_stacks(
    from_stack: crate::item::ItemStack,
    to_stack: Option<crate::item::ItemStack>,
    split: SplitMode,
) -> Option<(Option<crate::item::ItemStack>, Option<crate::item::ItemStack>)> {
    let cap = crate::item::item_max_stack(from_stack.id);
    let take = match split {
        SplitMode::All => 0u8,
        SplitMode::Half => (from_stack.count as u16).div_ceil(2) as u8,
        SplitMode::One => 1u8,
    };
    if take > 0 {
        let take = take.max(1).min(from_stack.count);
        let rem = from_stack.count - take;
        return match to_stack {
            Some(ts) if ts.same_stackable(&from_stack) => {
                let merge = cap.saturating_sub(ts.count).min(take);
                let leftover = take - merge;
                let back = rem + leftover;
                let new_to = ts.with_count(ts.count + merge);
                let new_from = (back > 0).then(|| from_stack.with_count(back));
                Some((new_from, Some(new_to)))
            }
            None => {
                let new_from = (rem > 0).then(|| from_stack.with_count(rem));
                Some((new_from, Some(from_stack.with_count(take))))
            }
            // Target occupied by a different item (or same id with
            // different components): a split into an unrelated slot
            // would either swap (loses semantics) or refuse. Refuse.
            Some(_) => None,
        };
    }
    // Whole-stack drag.
    match to_stack {
        Some(ts) if ts.same_stackable(&from_stack) => {
            let merge = cap.saturating_sub(ts.count).min(from_stack.count);
            let rem = from_stack.count - merge;
            let new_to = ts.with_count(ts.count + merge);
            let new_from = (rem > 0).then(|| from_stack.with_count(rem));
            Some((new_from, Some(new_to)))
        }
        other => Some((other, Some(from_stack))),
    }
}

/// Decrement one item of `block` somewhere in the inventory.
/// Returns `true` if a stack matched + was decremented (slot is
/// cleared when the count hits zero); `false` means the inventory
/// didn't have the block. Components are irrelevant for a place
/// consume - the first id match pays, like the tuple-era helper.
pub(crate) fn inv_consume(
    inv: &mut [Option<crate::item::ItemStack>; INVENTORY_SLOTS],
    block: BlockId,
) -> bool {
    for slot in inv.iter_mut() {
        if let Some(st) = slot.as_mut()
            && st.id == block.0
        {
            st.count = st.count.saturating_sub(1);
            if st.count == 0 {
                *slot = None;
            }
            return true;
        }
    }
    false
}

/// Apply a drag&drop on a `ClientInv`. W468: the split/merge/swap
/// rules come from the SHARED `item::stack::drag_stacks` helper -
/// the same function the local `Voxel::apply_inv_action` uses - so
/// the server's authoritative view can never drift from what the
/// client UI predicts. W469: craft cells are stack-typed too, so
/// components ride through the server's craft grid unchanged.
pub(crate) fn inv_apply_drag(
    inv: &mut ClientInv,
    from: SlotKind,
    to: SlotKind,
    split: SplitMode,
) {
    use crate::item::ItemStack;
    if from == to {
        return;
    }
    // Can't drop INTO the crafting output.
    if matches!(to, SlotKind::CraftOut) {
        return;
    }
    let read = |inv: &ClientInv, slot: SlotKind| -> Option<ItemStack> {
        match slot {
            SlotKind::Inv(i) => inv.inventory.get(i).cloned().flatten(),
            SlotKind::Craft(i) => inv.craft_grid.get(i).cloned().flatten(),
            SlotKind::CraftOut => match_recipe(&inv.craft_grid)
                .map(|(b, c)| ItemStack::new(b.0, c)),
        }
    };
    let write =
        |inv: &mut ClientInv, slot: SlotKind, val: Option<ItemStack>| match slot {
            SlotKind::Inv(i) => {
                if i < inv.inventory.len() {
                    inv.inventory[i] = val;
                }
            }
            SlotKind::Craft(i) => {
                if i < inv.craft_grid.len() {
                    inv.craft_grid[i] = val;
                }
            }
            SlotKind::CraftOut => {}
        };
    let Some(from_stack) = read(inv, from) else {
        return;
    };
    let to_stack = read(inv, to);
    if matches!(from, SlotKind::CraftOut) {
        // CraftOut drag: must land in an empty slot or merge into a
        // `same_stackable` stack with room for the full packet.
        let fits_merge = to_stack.as_ref().is_some_and(|ts| {
            ts.same_stackable(&from_stack)
                && (ts.count as u16 + from_stack.count as u16)
                    <= crate::item::item_max_stack(ts.id) as u16
        });
        if to_stack.is_none() || fits_merge {
            for cell in inv.craft_grid.iter_mut() {
                if let Some(st) = cell.as_mut() {
                    st.count = st.count.saturating_sub(1);
                    if st.count == 0 {
                        *cell = None;
                    }
                }
            }
            let new_to = match to_stack {
                Some(ts) => ts.with_count(ts.count + from_stack.count),
                None => from_stack,
            };
            write(inv, to, Some(new_to));
        }
        return;
    }
    if let Some((new_from, new_to)) = drag_stacks(from_stack, to_stack, split) {
        write(inv, to, new_to);
        write(inv, from, new_from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemStack;

    fn empty_client_inv() -> ClientInv {
        ClientInv {
            inventory: std::array::from_fn(|_| None),
            craft_grid: std::array::from_fn(|_| None),
        }
    }

    #[test]
    fn slot_0_accepts_drop_after_break_slot_removal() {
        // Pre Q.1, dropping INTO Inv(0) was a no-op. After Q.1
        // slot 0 is a normal stack slot - dragging a stone stack
        // out of slot 5 into slot 0 has to actually move it.
        let mut inv = empty_client_inv();
        inv.inventory[5] = Some(ItemStack::new(1, 8));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(5),
            SlotKind::Inv(0),
            SplitMode::All,
        );
        assert_eq!(inv.inventory[0], Some(ItemStack::new(1, 8)));
        assert_eq!(inv.inventory[5], None);
    }

    #[test]
    fn slot_0_pickup_lands_normally() {
        // inv_pickup walks slot 0 like any other - an empty
        // inventory should now land the first picked item in
        // slot 0.
        let mut inv: [Option<ItemStack>; INVENTORY_SLOTS] =
            std::array::from_fn(|_| None);
        assert!(inv_pickup(&mut inv, BlockId(2)));
        assert_eq!(inv[0], Some(ItemStack::new(2, 1)));
    }

    #[test]
    fn slot_0_half_split_moves_into_empty_target() {
        // Confirm SplitMode::Half also lands in slot 0 (not just
        // SplitMode::All) - belts-and-braces against any future
        // regression that would re-introduce a slot-0 guard for
        // a single split mode.
        let mut inv = empty_client_inv();
        inv.inventory[3] = Some(ItemStack::new(7, 10));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(3),
            SlotKind::Inv(0),
            SplitMode::Half,
        );
        // ceil(10/2) = 5 moves; 5 stays behind.
        assert_eq!(inv.inventory[0], Some(ItemStack::new(7, 5)));
        assert_eq!(inv.inventory[3], Some(ItemStack::new(7, 5)));
    }

    #[test]
    fn slot_0_merges_existing_stack() {
        // Dragging a stack of the same id onto a slot-0 stack
        // merges them (subject to the item's max stack). Mirrors
        // the behaviour of any other slot.
        let mut inv = empty_client_inv();
        inv.inventory[0] = Some(ItemStack::new(1, 30));
        inv.inventory[5] = Some(ItemStack::new(1, 20));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(5),
            SlotKind::Inv(0),
            SplitMode::All,
        );
        assert_eq!(inv.inventory[0], Some(ItemStack::new(1, 50)));
        assert_eq!(inv.inventory[5], None);
    }

    #[test]
    fn slot_0_swap_with_occupied_target() {
        // Different block ids: drag should swap, not protect
        // slot 0.
        let mut inv = empty_client_inv();
        inv.inventory[0] = Some(ItemStack::new(2, 4));
        inv.inventory[1] = Some(ItemStack::new(7, 9));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(1),
            SlotKind::Inv(0),
            SplitMode::All,
        );
        // Swap: 7's stack lands in slot 0; 2's stack lands in slot 1.
        assert_eq!(inv.inventory[0], Some(ItemStack::new(7, 9)));
        assert_eq!(inv.inventory[1], Some(ItemStack::new(2, 4)));
    }

    #[test]
    fn slot_0_one_split_decrements_source() {
        // SplitMode::One peels exactly 1 off the source stack
        // into the destination, even when the destination is
        // slot 0.
        let mut inv = empty_client_inv();
        inv.inventory[4] = Some(ItemStack::new(8, 3));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(4),
            SlotKind::Inv(0),
            SplitMode::One,
        );
        assert_eq!(inv.inventory[0], Some(ItemStack::new(8, 1)));
        assert_eq!(inv.inventory[4], Some(ItemStack::new(8, 2)));
    }

    #[test]
    fn craft_output_still_rejected_as_drop_target() {
        // CraftOut remains write-only; confirm the existing
        // protection wasn't accidentally tied to the slot-0
        // guard removal.
        let mut inv = empty_client_inv();
        inv.inventory[2] = Some(ItemStack::new(3, 6));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(2),
            SlotKind::CraftOut,
            SplitMode::All,
        );
        // Source untouched, no drop happened.
        assert_eq!(inv.inventory[2], Some(ItemStack::new(3, 6)));
    }

    /// W468: server drags obey the components rules the local UI
    /// uses - a worn stack full-placed onto a fresh same-id stack
    /// SWAPS (never merges), and a split onto it refuses.
    #[test]
    fn drag_worn_onto_fresh_swaps_not_merges() {
        let mut worn = ItemStack::new(7, 4);
        worn.components.damage = 9;
        let mut inv = empty_client_inv();
        inv.inventory[1] = Some(worn.clone());
        inv.inventory[2] = Some(ItemStack::new(7, 5));
        // Split refuses.
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(1),
            SlotKind::Inv(2),
            SplitMode::Half,
        );
        assert_eq!(inv.inventory[1], Some(worn.clone()));
        assert_eq!(inv.inventory[2], Some(ItemStack::new(7, 5)));
        // Full place swaps; both keep their components.
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(1),
            SlotKind::Inv(2),
            SplitMode::All,
        );
        assert_eq!(inv.inventory[1], Some(ItemStack::new(7, 5)));
        assert_eq!(inv.inventory[2], Some(worn));
    }
}
