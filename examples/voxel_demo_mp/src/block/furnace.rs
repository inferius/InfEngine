//! Furnace block behaviour: per-position 3-slot Container (input /
//! fuel / output) + live runtime state (`FurnaceRuntime`) + cook
//! step + the place / break / open glue that ties it into the
//! world-interact chain.
//!
//! C.4 phase breakdown:
//! - C.4a: stub + smelting tables + the block / item registry rows.
//! - C.4b: this file's `place_furnace` / `break_furnace` helpers +
//!   the `Voxel::furnaces` + `FurnaceRuntime` map.
//! - C.4c: live `furnace_tick` cook step.
//! - C.4d/e: save schema + offline catch-up wiring.
//! - C.4f: RMB-open furnace UI.
//!
//! The 3 slots are kept in this fixed order:
//! - slot 0: input. Accepts only items where
//!   `smelting::smelt_recipe(id).is_some()` (see `accept_smeltable`).
//! - slot 1: fuel. Accepts only items where
//!   `smelting::fuel_burn_ticks(id) > 0` (see `accept_fuel`).
//! - slot 2: output. Accepts nothing from player insertion (see
//!   `accept_nothing`); only the cook step pushes into it via
//!   `Container::set_slot` (the raw setter that bypasses filters).
//!
//! ## Unit conventions
//!
//! `FurnaceRuntime` tracks fuel + cook progress in **milliseconds**
//! so the live tick can step at sub-tick smoothness. The offline
//! catch-up in `world_save::timers::catch_up_furnace` operates in
//! **game ticks** (50 ms each). The save bridge converts ms <->
//! ticks at the edge.

#![allow(dead_code)]

use std::collections::HashMap;

use crate::container::Container;
use crate::smelting::{fuel_burn_ticks, smelt_recipe};

/// Number of slots in a single furnace. Locked at 3 (input / fuel /
/// output) by MC convention. The slot count is recorded per-
/// container so a future "blast furnace" variant could ship with a
/// different layout without touching the save schema.
pub(crate) const FURNACE_SLOTS: usize = 3;

/// Slot index of the input (smeltable item) slot.
pub(crate) const FURNACE_INPUT_SLOT: usize = 0;
/// Slot index of the fuel slot.
pub(crate) const FURNACE_FUEL_SLOT: usize = 1;
/// Slot index of the output slot.
pub(crate) const FURNACE_OUTPUT_SLOT: usize = 2;

/// Game-tick length in ms. Mirrors `world_save::timers::TICK_MS`
/// (kept locally so this module doesn't pull in the save layer for
/// such a small constant).
pub(crate) const TICK_MS: u32 = 50;

/// Cook ticks per output. Matches MC's 200-tick (10 s) smelt time.
/// Mirrors `world_save::timers::FURNACE_COOK_TICKS_PER_OUTPUT`.
pub(crate) const FURNACE_COOK_TICKS_PER_OUTPUT: u32 = 200;

/// Cook duration per output in milliseconds. The live tick path
/// compares `FurnaceRuntime::cook_progress_ms` against this; the
/// catch-up dispatcher works in ticks. 200 * 50 = 10000 ms.
pub(crate) const FURNACE_COOK_MS_PER_OUTPUT: u32 =
    FURNACE_COOK_TICKS_PER_OUTPUT * TICK_MS;

/// Per-furnace live runtime state.
///
/// Lives alongside the per-furnace `Container` so the cook tick can
/// advance progress without bloating the saved slot vector. The
/// save schema persists this struct verbatim - the field set is
/// frozen for the on-disk shape, additions land in a v2->v3
/// migration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FurnaceRuntime {
    /// Remaining burn time of the current fuel item, in
    /// milliseconds. When this hits zero and the input slot still
    /// has a smeltable, the tick consumes another fuel item and
    /// resets this to `fuel_full_ms`.
    pub fuel_left_ms: u32,
    /// Total burn time of the current fuel item, in milliseconds.
    /// Used to render the UI fuel bar as `fuel_left_ms /
    /// fuel_full_ms`. Cleared back to zero when the furnace runs
    /// idle so the bar reads empty.
    pub fuel_full_ms: u32,
    /// Accumulated cook progress for the in-flight output, in
    /// milliseconds. Reaches `FURNACE_COOK_MS_PER_OUTPUT` to emit
    /// one output. The overflow is preserved (see
    /// `furnace_tick`) so a long single-frame step doesn't lose
    /// fractional progress.
    pub cook_progress_ms: u32,
}

/// Build a fresh empty furnace container with the per-slot accept
/// filters wired up. Used by `place_furnace` on the place path and
/// by the save-load rehydrate path so on-disk slot contents land in
/// a container that already rejects player misuse of the output
/// slot.
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
/// The cook step bypasses the filter via `Container::set_slot`
/// (or by merging into a same-id stack on the runtime side).
pub(crate) fn accept_nothing(_id: u16) -> bool {
    false
}

/// Insert a fresh empty furnace into `furnaces` at `pos` (no-op if
/// one already exists). The runtime map is initialised lazily via
/// `entry(pos).or_insert_with(...)`. Mirrors `chest::new_chest`'s
/// place-path shape.
pub(crate) fn place_furnace(
    furnaces: &mut HashMap<[i32; 3], Container>,
    runtimes: &mut HashMap<[i32; 3], FurnaceRuntime>,
    pos: [i32; 3],
) {
    furnaces.entry(pos).or_insert_with(new_furnace);
    runtimes.entry(pos).or_default();
}

/// Drain the furnace at `pos` for break-drops: removes it from
/// both `furnaces` and `runtimes`, returns the per-item drop list
/// for the 3 slots.
///
/// Mirrors `chest::drain_drops` but operates against the runtime
/// map directly - chest break is currently a `.remove()` followed
/// by a separate `drain_drops` call; furnace_break collapses the
/// two so the tick path doesn't accidentally leave a runtime
/// entry behind.
///
/// Returns an empty vec when the furnace doesn't exist at `pos`.
pub(crate) fn break_furnace(
    furnaces: &mut HashMap<[i32; 3], Container>,
    runtimes: &mut HashMap<[i32; 3], FurnaceRuntime>,
    pos: [i32; 3],
) -> Vec<u16> {
    runtimes.remove(&pos);
    let Some(furnace) = furnaces.remove(&pos) else {
        return Vec::new();
    };
    drain_drops(&furnace)
}

/// Flatten a furnace's slot contents into a per-item drop list.
/// Each slot of `(id, count)` expands to `count` copies of `id`.
/// Output slot included - the player gets back any already-cooked
/// output the furnace held at break time.
pub(crate) fn drain_drops(furnace: &Container) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for (id, count) in furnace.slots().iter().flatten() {
        for _ in 0..*count {
            out.push(*id);
        }
    }
    out
}

/// Recover a cursor-held stack into the furnace when the UI
/// closes. Returns the per-item leftover Vec the caller spawns as
/// floor drops at the furnace position. Mirrors
/// `chest::recover_held_on_close` but operates against the furnace
/// container - same accept filters apply, so a held stack of the
/// wrong type for the input slot may end up leftover even when
/// other slots are technically empty.
pub(crate) fn recover_held_on_close(
    furnace: Option<&mut Container>,
    held: Option<(u16, u8)>,
) -> Vec<u16> {
    let Some((id, count)) = held else {
        return Vec::new();
    };
    let leftover = match furnace {
        Some(c) => c.insert((id, count)),
        None => count,
    };
    (0..leftover).map(|_| id).collect()
}

#[cfg(test)]
#[path = "furnace_tests.rs"]
mod furnace_tests;
