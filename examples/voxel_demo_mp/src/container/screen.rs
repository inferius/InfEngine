//! W470 - screen adapter: turns the panels' queued `GridAction`s
//! (and the drag-paint visit lists) into calls on the shared click
//! core (`container::click`), against a two-section screen of
//! [container slots, player inventory].
//!
//! This is the ONE place the container-family panels (chest /
//! furnace / brewing, plus their MP wire appliers) mutate slots.
//! The per-menu differences - MC's `quickMoveStack` routing - are
//! data (`ScreenCfg`), not code: chests shift-click into every
//! container slot and land container->player moves hotbar-first;
//! furnaces route smeltables to the input slot, fuels to the fuel
//! slot, fall back to a hotbar<->main move, and only the RESULT
//! slot empties hotbar-first (AbstractFurnaceMenu lines: result
//! `moveItemStackTo(3, 39, true)`, input/fuel `(3, 39, false)`).

use super::Container;
use super::click::{self, SectionMut};
use super::ui::GridAction;
use crate::item::ItemStack;

/// Player-inventory geometry. Mirrors `inventory::HOTBAR_SLOTS` /
/// `INVENTORY_SLOTS` from the bin crate - duplicated as consts here
/// because the `inventory` module is excluded from the headless lib
/// target while `container` is included (a compile-time assert in
/// `ui/render.rs` keeps the two pairs in sync).
pub(crate) const HOTBAR_SLOTS: usize = 9;
pub(crate) const INVENTORY_SLOTS: usize = 36;

/// Player -> container shift-click routing (MC `quickMoveStack`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum QuickRoute {
    /// Chest-like: the stack targets every container slot in index
    /// order; a full container refuses with no side effects
    /// (ChestMenu returns EMPTY - no hotbar/main fallback).
    AllContainerSlots,
    /// Furnace / brewing-like: the FIRST listed slot whose accept
    /// predicate passes the item id is the ONLY target; when none
    /// accepts (or the move fails), fall back to a hotbar<->main
    /// move inside the player inventory (the source's trailing
    /// `else if` arms).
    FirstAccepting(&'static [usize]),
}

/// Which container slots empty toward the player HOTBAR-FIRST on
/// shift-click (`moveItemStackTo(.., .., true)` in the source).
#[derive(Debug, Clone, Copy)]
pub(crate) enum BackwardsMode {
    /// Every container slot (ChestMenu, BrewingStandMenu).
    All,
    /// Only the listed slots (AbstractFurnaceMenu: the result
    /// slot); the rest go main-first.
    Only(&'static [usize]),
}

/// Per-menu quick-move configuration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScreenCfg {
    pub(crate) route: QuickRoute,
    pub(crate) backwards: BackwardsMode,
}

/// ChestMenu routing.
pub(crate) const CHEST_CFG: ScreenCfg = ScreenCfg {
    route: QuickRoute::AllContainerSlots,
    backwards: BackwardsMode::All,
};

/// AbstractFurnaceMenu routing: input slot 0, fuel slot 1; result
/// slot 2 is the only hotbar-first source.
pub(crate) const FURNACE_CFG: ScreenCfg = ScreenCfg {
    route: QuickRoute::FirstAccepting(&[0, 1]),
    backwards: BackwardsMode::Only(&[2]),
};

/// BrewingStandMenu routing: fuel slot first (the source checks
/// `FuelSlot.mayPlaceItem` before the ingredient), then the
/// ingredient, then the three bottle slots.
pub(crate) const BREWING_CFG: ScreenCfg = ScreenCfg {
    route: QuickRoute::FirstAccepting(&[1, 0, 2, 3, 4]),
    backwards: BackwardsMode::All,
};

/// Player-inventory fill order, hotbar-first backwards:
/// hotbar 8..0 then main rows bottom-up (`moveItemStackTo(.., ..,
/// true)` over the standard player section).
fn inv_order_backwards(base: usize) -> Vec<usize> {
    (0..HOTBAR_SLOTS)
        .rev()
        .chain((HOTBAR_SLOTS..INVENTORY_SLOTS).rev())
        .map(|i| base + i)
        .collect()
}

/// Player-inventory fill order, main-first forward: main rows then
/// hotbar 0..8 (`moveItemStackTo(.., .., false)`).
fn inv_order_forward(base: usize) -> Vec<usize> {
    (HOTBAR_SLOTS..INVENTORY_SLOTS)
        .chain(0..HOTBAR_SLOTS)
        .map(|i| base + i)
        .collect()
}

/// Apply one `GridAction` against the [container, player inventory]
/// screen. `player_side` says which section the action's index
/// addresses. Returns any thrown stacks (Q / Ctrl+Q) for the
/// caller to spawn as drops - empty for the non-throw ops.
///
/// `creative` gates the CLONE arm (`player.hasInfiniteMaterials()`).
pub(crate) fn apply_grid_action(
    container: &mut Container,
    inv: &mut [Option<ItemStack>],
    cursor: &mut Option<ItemStack>,
    player_side: bool,
    action: GridAction,
    cfg: &ScreenCfg,
    creative: bool,
) -> Vec<ItemStack> {
    let clen = container.len();
    let inv_len = inv.len();
    let (cslots, caccept) = container.slots_and_accept_mut();
    let mut sections = [
        SectionMut { slots: cslots, accept: caccept },
        SectionMut::open(inv),
    ];
    let gi = |idx: usize| if player_side { clen + idx } else { idx };
    let mut thrown: Vec<ItemStack> = Vec::new();
    match action {
        // The four PICKUP shapes collapse onto one ported op: the
        // UI emits Take vs Place based on the cursor state, but the
        // core's PICKUP arm handles both ends itself.
        GridAction::Take(idx) | GridAction::Place(idx) => {
            click::pickup_click(&mut sections, cursor, gi(idx), false);
        }
        GridAction::TakeHalf(idx) | GridAction::PlaceOne(idx) => {
            click::pickup_click(&mut sections, cursor, gi(idx), true);
        }
        GridAction::Transfer(idx) => {
            let source_gi = gi(idx);
            if player_side {
                match cfg.route {
                    QuickRoute::AllContainerSlots => {
                        let order: Vec<usize> = (0..clen).collect();
                        click::quick_move(&mut sections, source_gi, &order);
                    }
                    QuickRoute::FirstAccepting(priority) => {
                        let id = click::slot_ref(&sections, source_gi)
                            .map(|st| st.id);
                        let target = id.and_then(|id| {
                            priority.iter().copied().find(|&s| {
                                s < clen && click::accepts(&sections, s, id)
                            })
                        });
                        let moved = match target {
                            Some(t) => click::quick_move(
                                &mut sections,
                                source_gi,
                                &[t],
                            ),
                            None => false,
                        };
                        if !moved {
                            // Hotbar<->main fallback (the source's
                            // trailing arms).
                            let order: Vec<usize> = if idx < HOTBAR_SLOTS {
                                (HOTBAR_SLOTS..inv_len.min(INVENTORY_SLOTS))
                                    .map(|i| clen + i)
                                    .collect()
                            } else {
                                (0..HOTBAR_SLOTS.min(inv_len))
                                    .map(|i| clen + i)
                                    .collect()
                            };
                            click::quick_move(&mut sections, source_gi, &order);
                        }
                    }
                }
            } else {
                let backwards = match cfg.backwards {
                    BackwardsMode::All => true,
                    BackwardsMode::Only(list) => list.contains(&idx),
                };
                let order = if backwards {
                    inv_order_backwards(clen)
                } else {
                    inv_order_forward(clen)
                };
                click::quick_move(&mut sections, source_gi, &order);
            }
        }
        GridAction::Collect(idx) => {
            click::pickup_all(&mut sections, cursor, gi(idx));
        }
        GridAction::SwapHotbar(idx, h) => {
            if (h as usize) < HOTBAR_SLOTS {
                click::swap_click(&mut sections, gi(idx), clen + h as usize);
            }
        }
        GridAction::ThrowOne(idx) => {
            if let Some(st) =
                click::throw_from_slot(&mut sections, cursor, gi(idx), false)
            {
                thrown.push(st);
            }
        }
        GridAction::ThrowAll(idx) => {
            if let Some(st) =
                click::throw_from_slot(&mut sections, cursor, gi(idx), true)
            {
                thrown.push(st);
            }
        }
        GridAction::CloneSlot(idx) => {
            click::clone_click(&sections, cursor, gi(idx), creative);
        }
    }
    thrown
}

/// Apply a completed drag-paint (QUICK_CRAFT release) against the
/// [container, player inventory] screen. `visited` is the ordered
/// (player_side, idx) visit list from the UI tracker; `one_per_slot`
/// = RMB drag.
pub(crate) fn apply_grid_drag(
    container: &mut Container,
    inv: &mut [Option<ItemStack>],
    cursor: &mut Option<ItemStack>,
    visited: &[(bool, usize)],
    one_per_slot: bool,
) {
    let clen = container.len();
    let (cslots, caccept) = container.slots_and_accept_mut();
    let mut sections = [
        SectionMut { slots: cslots, accept: caccept },
        SectionMut::open(inv),
    ];
    // Dedup while preserving visit order (the wire may replay).
    let mut gis: Vec<usize> = Vec::with_capacity(visited.len());
    for &(side, idx) in visited {
        let gi = if side { clen + idx } else { idx };
        if !gis.contains(&gi) {
            gis.push(gi);
        }
    }
    click::quick_craft_apply(&mut sections, cursor, &gis, one_per_slot);
}
