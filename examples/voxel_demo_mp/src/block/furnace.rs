//! Furnace block behaviour: per-position 3-slot Container (input /
//! fuel / output) + the live cook step + the place / break / open
//! glue that ties it into the world-interact chain.
//!
//! C.4 phase breakdown:
//! - C.4a: this stub + the smelting tables + the block / item
//!   registry rows. Just enough to compile.
//! - C.4b: the real `place_furnace` / `break_furnace` helpers + the
//!   `Voxel::furnaces` + `FurnaceRuntime` map.
//! - C.4c: the live `furnace_tick` cook step.
//! - C.4d/e: save schema + offline catch-up wiring.
//! - C.4f: the RMB-open furnace UI.
//!
//! The 3 slots are kept in this fixed order:
//! - slot 0: input. Accepts only items where
//!   `smelting::smelt_recipe(id).is_some()` (see `accept_smeltable`).
//! - slot 1: fuel. Accepts only items where
//!   `smelting::fuel_burn_ticks(id) > 0` (see `accept_fuel`).
//! - slot 2: output. Accepts nothing from player insertion (see
//!   `accept_nothing`); only the cook step pushes into it.

#![allow(dead_code)]

use crate::container::Container;
use crate::smelting::{fuel_burn_ticks, smelt_recipe};

/// Number of slots in a single furnace. Locked at 3 (input / fuel
/// / output) by MC convention. The slot count is recorded per-
/// container so a future "blast furnace" variant could ship with a
/// different layout without touching the save schema.
pub(crate) const FURNACE_SLOTS: usize = 3;

/// Slot index of the input (smeltable item) slot. Lives at index 0
/// so the live + catch-up code can refer to "slot 0" without
/// magic numbers.
pub(crate) const FURNACE_INPUT_SLOT: usize = 0;
/// Slot index of the fuel slot.
pub(crate) const FURNACE_FUEL_SLOT: usize = 1;
/// Slot index of the output slot.
pub(crate) const FURNACE_OUTPUT_SLOT: usize = 2;

/// Build a fresh empty furnace container with the per-slot accept
/// filters wired up. C.4b uses this on the place path; C.4d uses
/// it on the save-load rehydrate path so on-disk slot contents
/// land in a container that already rejects player misuse of the
/// output slot.
///
/// Accept filters are `fn(u16) -> bool` (the `AcceptFn` alias). MC
/// keeps the output slot read-only from the player's side, so the
/// `accept_nothing` predicate rejects every id.
pub(crate) fn new_furnace() -> Container {
    let mut c = Container::with_size(FURNACE_SLOTS);
    c.set_accept(FURNACE_INPUT_SLOT, accept_smeltable);
    c.set_accept(FURNACE_FUEL_SLOT, accept_fuel);
    c.set_accept(FURNACE_OUTPUT_SLOT, accept_nothing);
    c
}

/// Accept-filter for the furnace input slot: only items with a
/// registered smelt recipe land here. Free fn (not a closure) so
/// the `AcceptFn = fn(u16) -> bool` alias accepts it.
pub(crate) fn accept_smeltable(id: u16) -> bool {
    smelt_recipe(id).is_some()
}

/// Accept-filter for the furnace fuel slot: only items with a
/// non-zero burn duration land here.
pub(crate) fn accept_fuel(id: u16) -> bool {
    fuel_burn_ticks(id) > 0
}

/// Accept-filter for the furnace output slot: rejects every id.
/// The cook step bypasses the filter via `Container::set_slot`,
/// which is the documented "raw setter" intended for save-load +
/// admin paths.
pub(crate) fn accept_nothing(_id: u16) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_furnace_has_three_slots() {
        let c = new_furnace();
        assert_eq!(c.len(), FURNACE_SLOTS);
        assert!(c.is_empty());
    }

    #[test]
    fn input_slot_accepts_smeltables_only() {
        let c = new_furnace();
        // Cobble is smeltable -> accepted at input slot.
        assert!(c.accepts(FURNACE_INPUT_SLOT, 8));
        // Stone has no recipe -> rejected.
        assert!(!c.accepts(FURNACE_INPUT_SLOT, 1));
        // Iron ore is smeltable.
        assert!(c.accepts(FURNACE_INPUT_SLOT, 14));
    }

    #[test]
    fn fuel_slot_accepts_fuels_only() {
        let c = new_furnace();
        // Coal is a fuel.
        assert!(c.accepts(FURNACE_FUEL_SLOT, 13));
        // Planks are a fuel.
        assert!(c.accepts(FURNACE_FUEL_SLOT, 7));
        // Stone is not a fuel.
        assert!(!c.accepts(FURNACE_FUEL_SLOT, 1));
        // Iron ore is not a fuel.
        assert!(!c.accepts(FURNACE_FUEL_SLOT, 14));
    }

    #[test]
    fn output_slot_rejects_every_id() {
        let c = new_furnace();
        for id in 0u16..=64 {
            assert!(
                !c.accepts(FURNACE_OUTPUT_SLOT, id),
                "output slot must reject id {id}"
            );
        }
    }

    #[test]
    fn insert_into_output_via_player_path_fails() {
        // insert_at honours the accept filter -> a player-driven
        // place into the output slot returns the full count as
        // leftover and leaves the slot untouched.
        let mut c = new_furnace();
        let leftover = c.insert_at(FURNACE_OUTPUT_SLOT, (1, 4));
        assert_eq!(leftover, 4);
        assert_eq!(c.slot(FURNACE_OUTPUT_SLOT), None);
    }

    #[test]
    fn set_slot_bypasses_filter_for_cook_path() {
        // The cook step writes the output via `set_slot` (which the
        // C.2 docs call the "raw setter intended for save-load +
        // admin paths"). It must succeed regardless of the accept
        // filter.
        let mut c = new_furnace();
        c.set_slot(FURNACE_OUTPUT_SLOT, Some((43, 1)));
        assert_eq!(c.slot(FURNACE_OUTPUT_SLOT), Some((43, 1)));
    }
}
