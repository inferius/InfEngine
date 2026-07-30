//! W470: GridAction adapters for the furnace panel - thin shims
//! over the SHARED click core (`container::screen` ->
//! `container::click`). The old per-panel take/place/transfer
//! copies are gone; the only furnace-specific knowledge left is
//! the `FURNACE_CFG` quick-move routing (smeltables -> input slot,
//! fuels -> fuel slot, hotbar<->main fallback, result slot empties
//! hotbar-first) which mirrors `AbstractFurnaceMenu.quickMoveStack`.
//! Accept filters ride inside the `Container` itself (input /
//! fuel / output predicates from `block::furnace`).
//!
//! W472: `creative` threads through (the CLONE arm rides the wire
//! now, gated on the sender's announced game mode).

#![allow(dead_code)]

use crate::container::Container;
use crate::container::ui::GridAction;
use crate::item::ItemStack;

/// Apply one `GridAction` against the furnace-side section of the
/// [furnace, player inventory] screen. Returns thrown stacks
/// (Q / Ctrl+Q) for the caller to spawn as drops.
pub(crate) fn apply_furnace_action(
    furnace: &mut Container,
    partner: &mut [Option<ItemStack>],
    held: &mut Option<ItemStack>,
    action: GridAction,
    creative: bool,
) -> Vec<ItemStack> {
    crate::container::screen::apply_grid_action(
        furnace,
        partner,
        held,
        false,
        action,
        &crate::container::screen::FURNACE_CFG,
        creative,
    )
}

/// Player-inventory-side counterpart of `apply_furnace_action`.
pub(crate) fn apply_player_action(
    inv: &mut [Option<ItemStack>],
    furnace: &mut Container,
    held: &mut Option<ItemStack>,
    action: GridAction,
    creative: bool,
) -> Vec<ItemStack> {
    crate::container::screen::apply_grid_action(
        furnace,
        inv,
        held,
        true,
        action,
        &crate::container::screen::FURNACE_CFG,
        creative,
    )
}
