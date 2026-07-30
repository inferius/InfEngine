//! Inventory + hotbar + crafting-grid slot logic.
//!
//! Owns the small but tricky pile of types + helpers that move
//! `(BlockId, count)` stacks around the client's 36-slot
//! inventory grid + the 2x2 craft grid:
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
//!   drag&drop apply path; the client predicts locally and the
//!   server runs the same logic to produce the authoritative
//!   snapshot.
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

/// Pop one of `block` into the inventory. First tries to top up an
/// existing non-full stack, then claims the first empty slot.
/// Returns `true` when the block was successfully stored (false
/// means the inventory was completely full + the drop was lost).
pub(crate) fn inv_pickup(
    inv: &mut [Option<(BlockId, u8)>; INVENTORY_SLOTS],
    block: BlockId,
) -> bool {
    if block == BlockId::AIR {
        return false;
    }
    // W53.3: per-item max stack honours tools/buckets/etc.
    let cap = crate::item::item_max_stack(block.0);
    for slot in inv.iter_mut() {
        if let Some((b, count)) = slot.as_mut()
            && *b == block
            && *count < cap
        {
            *count += 1;
            return true;
        }
    }
    for slot in inv.iter_mut() {
        if slot.is_none() {
            *slot = Some((block, 1));
            return true;
        }
    }
    false
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

/// Decrement one item of `block` somewhere in the inventory.
/// Returns `true` if a stack matched + was decremented (slot is
/// cleared when the count hits zero); `false` means the inventory
/// didn't have the block.
pub(crate) fn inv_consume(
    inv: &mut [Option<(BlockId, u8)>; INVENTORY_SLOTS],
    block: BlockId,
) -> bool {
    for slot in inv.iter_mut() {
        if let Some((b, count)) = slot.as_mut()
            && *b == block
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                *slot = None;
            }
            return true;
        }
    }
    false
}

/// Apply a drag&drop on a `ClientInv`. Mirrors the local
/// `apply_inv_action` logic so the server's authoritative view
/// stays in sync with what the client UI predicts.
pub(crate) fn inv_apply_drag(
    inv: &mut ClientInv,
    from: SlotKind,
    to: SlotKind,
    split: SplitMode,
) {
    if from == to {
        return;
    }
    // Can't drop INTO the crafting output.
    if matches!(to, SlotKind::CraftOut) {
        return;
    }
    let from_stack = match from {
        SlotKind::Inv(i) => inv.inventory.get(i).copied().flatten(),
        SlotKind::Craft(i) => inv.craft_grid.get(i).copied().flatten(),
        SlotKind::CraftOut => match_recipe(&inv.craft_grid),
    };
    let Some(from_stack) = from_stack else {
        return;
    };
    let to_stack: Option<(BlockId, u8)> = match to {
        SlotKind::Inv(i) => inv.inventory.get(i).copied().flatten(),
        SlotKind::Craft(i) => inv.craft_grid.get(i).copied().flatten(),
        SlotKind::CraftOut => unreachable!(),
    };
    let write_to = |inv: &mut ClientInv, slot: SlotKind, val: Option<(BlockId, u8)>| match slot {
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
    if matches!(from, SlotKind::CraftOut) {
        match to_stack {
            None => {
                for cell in inv.craft_grid.iter_mut() {
                    if let Some((_, c)) = cell.as_mut() {
                        *c = c.saturating_sub(1);
                        if *c == 0 {
                            *cell = None;
                        }
                    }
                }
                write_to(inv, to, Some(from_stack));
            }
            Some((tb, tc))
                if tb == from_stack.0
                    && (tc as u16 + from_stack.1 as u16)
                        <= crate::item::item_max_stack(tb.0) as u16 =>
            {
                for cell in inv.craft_grid.iter_mut() {
                    if let Some((_, c)) = cell.as_mut() {
                        *c = c.saturating_sub(1);
                        if *c == 0 {
                            *cell = None;
                        }
                    }
                }
                write_to(inv, to, Some((tb, tc + from_stack.1)));
            }
            _ => {}
        }
        return;
    }
    let take = match split {
        SplitMode::All => 0u8,
        SplitMode::Half => (from_stack.1 as u16).div_ceil(2) as u8,
        SplitMode::One => 1u8,
    };
    if take > 0 {
        let take = take.max(1).min(from_stack.1);
        let rem = from_stack.1 - take;
        match to_stack {
            Some((tb, tc)) if tb == from_stack.0 => {
                let cap = crate::item::item_max_stack(tb.0);
                let merge = cap.saturating_sub(tc).min(take);
                let new_to = tc + merge;
                let leftover = take - merge;
                write_to(inv, to, Some((tb, new_to)));
                let back = rem + leftover;
                write_to(
                    inv,
                    from,
                    if back == 0 {
                        None
                    } else {
                        Some((from_stack.0, back))
                    },
                );
            }
            None => {
                write_to(inv, to, Some((from_stack.0, take)));
                write_to(
                    inv,
                    from,
                    if rem == 0 {
                        None
                    } else {
                        Some((from_stack.0, rem))
                    },
                );
            }
            Some(_) => {}
        }
        return;
    }
    match to_stack {
        Some((tb, tc)) if tb == from_stack.0 => {
            let cap = crate::item::item_max_stack(tb.0);
            let take = cap.saturating_sub(tc).min(from_stack.1);
            let new_to = tc + take;
            let rem = from_stack.1 - take;
            write_to(inv, to, Some((tb, new_to)));
            write_to(
                inv,
                from,
                if rem == 0 {
                    None
                } else {
                    Some((from_stack.0, rem))
                },
            );
        }
        _ => {
            write_to(inv, to, Some(from_stack));
            write_to(inv, from, to_stack);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_client_inv() -> ClientInv {
        ClientInv {
            inventory: [None; INVENTORY_SLOTS],
            craft_grid: [None; 4],
        }
    }

    #[test]
    fn slot_0_accepts_drop_after_break_slot_removal() {
        // Pre Q.1, dropping INTO Inv(0) was a no-op. After Q.1
        // slot 0 is a normal stack slot - dragging a stone stack
        // out of slot 5 into slot 0 has to actually move it.
        let mut inv = empty_client_inv();
        inv.inventory[5] = Some((BlockId(1), 8));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(5),
            SlotKind::Inv(0),
            SplitMode::All,
        );
        assert_eq!(inv.inventory[0], Some((BlockId(1), 8)));
        assert_eq!(inv.inventory[5], None);
    }

    #[test]
    fn slot_0_pickup_lands_normally() {
        // inv_pickup walks slot 0 like any other - an empty
        // inventory should now land the first picked item in
        // slot 0.
        let mut inv: [Option<(BlockId, u8)>; INVENTORY_SLOTS] =
            [None; INVENTORY_SLOTS];
        assert!(inv_pickup(&mut inv, BlockId(2)));
        assert_eq!(inv[0], Some((BlockId(2), 1)));
    }

    #[test]
    fn slot_0_half_split_moves_into_empty_target() {
        // Confirm SplitMode::Half also lands in slot 0 (not just
        // SplitMode::All) - belts-and-braces against any future
        // regression that would re-introduce a slot-0 guard for
        // a single split mode.
        let mut inv = empty_client_inv();
        inv.inventory[3] = Some((BlockId(7), 10));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(3),
            SlotKind::Inv(0),
            SplitMode::Half,
        );
        // ceil(10/2) = 5 moves; 5 stays behind.
        assert_eq!(inv.inventory[0], Some((BlockId(7), 5)));
        assert_eq!(inv.inventory[3], Some((BlockId(7), 5)));
    }

    #[test]
    fn slot_0_merges_existing_stack() {
        // Dragging a stack of the same id onto a slot-0 stack
        // merges them (subject to STACK_MAX). Mirrors the
        // behaviour of any other slot.
        let mut inv = empty_client_inv();
        inv.inventory[0] = Some((BlockId(1), 30));
        inv.inventory[5] = Some((BlockId(1), 20));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(5),
            SlotKind::Inv(0),
            SplitMode::All,
        );
        assert_eq!(inv.inventory[0], Some((BlockId(1), 50)));
        assert_eq!(inv.inventory[5], None);
    }

    #[test]
    fn slot_0_swap_with_occupied_target() {
        // Different block ids: drag should swap, not protect
        // slot 0.
        let mut inv = empty_client_inv();
        inv.inventory[0] = Some((BlockId(2), 4));
        inv.inventory[1] = Some((BlockId(7), 9));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(1),
            SlotKind::Inv(0),
            SplitMode::All,
        );
        // Swap: 7's stack lands in slot 0; 2's stack lands in slot 1.
        assert_eq!(inv.inventory[0], Some((BlockId(7), 9)));
        assert_eq!(inv.inventory[1], Some((BlockId(2), 4)));
    }

    #[test]
    fn slot_0_one_split_decrements_source() {
        // SplitMode::One peels exactly 1 off the source stack
        // into the destination, even when the destination is
        // slot 0.
        let mut inv = empty_client_inv();
        inv.inventory[4] = Some((BlockId(8), 3));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(4),
            SlotKind::Inv(0),
            SplitMode::One,
        );
        assert_eq!(inv.inventory[0], Some((BlockId(8), 1)));
        assert_eq!(inv.inventory[4], Some((BlockId(8), 2)));
    }

    #[test]
    fn craft_output_still_rejected_as_drop_target() {
        // CraftOut remains write-only; confirm the existing
        // protection wasn't accidentally tied to the slot-0
        // guard removal.
        let mut inv = empty_client_inv();
        inv.inventory[2] = Some((BlockId(3), 6));
        inv_apply_drag(
            &mut inv,
            SlotKind::Inv(2),
            SlotKind::CraftOut,
            SplitMode::All,
        );
        // Source untouched, no drop happened.
        assert_eq!(inv.inventory[2], Some((BlockId(3), 6)));
    }
}
