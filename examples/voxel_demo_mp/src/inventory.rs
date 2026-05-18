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

/// Hotbar slots: index 0 is the "break" slot (empty hand), the rest
/// hold the block ids in `Self::HOTBAR_BLOCKS`.
/// Hotbar occupies the first row of the inventory grid; the
/// remaining `INVENTORY_ROWS - 1` rows are the open-inventory
/// extension Minecraft players expect.
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
    for slot in inv.iter_mut() {
        if let Some((b, count)) = slot.as_mut()
            && *b == block
            && *count < STACK_MAX
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
    // Protect the break-tool slot.
    if matches!(to, SlotKind::Inv(0)) {
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
                    && (tc as u16 + from_stack.1 as u16) <= STACK_MAX as u16 =>
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
                let merge = (STACK_MAX - tc).min(take);
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
            let take = (STACK_MAX - tc).min(from_stack.1);
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
