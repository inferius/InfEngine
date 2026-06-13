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

/// W163: the furnace-family blocks that share the smelting container,
/// tick, place/break glue and RMB-open UI - plain furnace, smoker and
/// blast furnace. They were dead blocks (placeable, no interaction)
/// before this; routing them through the proven furnace path makes
/// them smelt. MC's 2x speed + recipe partition (smoker = food,
/// blast = ores/metal) is a documented follow-up - for now all three
/// run the standard furnace recipe set at furnace speed.
pub(crate) fn is_furnace_like(id: u16) -> bool {
    use crate::block::{BLAST_FURNACE_ID, FURNACE_ID, SMOKER_ID};
    id == FURNACE_ID || id == SMOKER_ID || id == BLAST_FURNACE_ID
}

// GridAction <-> furnace mutation helpers live in the sibling
// `furnace_actions` module so this file stays under the 500-LOC
// soft cap. Re-export the public surface.
#[path = "furnace_actions.rs"]
mod actions;
pub(crate) use actions::{apply_furnace_action, apply_player_action};

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

/// Advance every active furnace by `dt_ms` of wall-clock time.
///
/// Per-furnace algorithm (mirrors MC's behaviour, simplified):
/// 1. If input is empty OR has no recipe OR output can't accept
///    the recipe result -> idle. Clear `fuel_full_ms` once the
///    current burn finishes so the UI bar shows empty.
/// 2. If `fuel_left_ms == 0` and fuel slot has fuel -> consume
///    one fuel item, set `fuel_left_ms = fuel_full_ms =
///    burn_ticks * TICK_MS`.
/// 3. Burn `dt_ms.min(fuel_left_ms)` from the fuel + same amount
///    into `cook_progress_ms`.
/// 4. When `cook_progress_ms >= FURNACE_COOK_MS_PER_OUTPUT`:
///    decrement input by 1, push the output into slot 2 (using
///    the same-id merge path - the runtime bypasses the accept
///    filter via direct slot writes when the slot is empty).
///    Preserve the overflow on `cook_progress_ms`.
///
/// Decisions per the C.4 brief:
/// - Fuel does NOT decay when idle (no input or output full).
/// - Cook progress does NOT decay when fuel runs out mid-cook
///   (vanilla MC slowly drains it; we defer that nicety).
///
/// `dt_ms` is the per-frame wall-clock delta in ms. The caller
/// supplies the same delta to every furnace.
pub(crate) fn furnace_tick(
    furnaces: &mut HashMap<[i32; 3], Container>,
    runtimes: &mut HashMap<[i32; 3], FurnaceRuntime>,
    dt_ms: u32,
) {
    if dt_ms == 0 {
        return;
    }
    // Walk every furnace by position; the runtime entry for the
    // same key may be missing (the live runtime is lazy) so we
    // mint a zero entry on first contact.
    let positions: Vec<[i32; 3]> = furnaces.keys().copied().collect();
    for pos in positions {
        let rt_entry = runtimes.entry(pos).or_default();
        let Some(furnace) = furnaces.get_mut(&pos) else {
            continue;
        };
        step_one_furnace(furnace, rt_entry, dt_ms);
    }
}

/// Single-furnace cook step. Pulled out so the per-frame `for`
/// loop above stays a short orchestrator + the actual logic is
/// unit-testable in isolation.
///
/// Returns nothing; mutates `furnace` slots and `runtime` in place.
pub(crate) fn step_one_furnace(
    furnace: &mut Container,
    runtime: &mut FurnaceRuntime,
    dt_ms: u32,
) {
    // Gate: idle if input is empty OR has no recipe.
    let Some((input_id, _input_count)) = furnace.slot(FURNACE_INPUT_SLOT) else {
        // No input. Let the current fuel finish (so the UI burn
        // bar drains visibly) but don't accumulate cook progress.
        drain_fuel_idle(runtime, dt_ms);
        return;
    };
    let Some((output_id, output_count_per)) = smelt_recipe(input_id) else {
        drain_fuel_idle(runtime, dt_ms);
        return;
    };
    // Output gate: must be empty OR same-id-with-room.
    if !output_slot_can_accept(furnace, output_id, output_count_per) {
        drain_fuel_idle(runtime, dt_ms);
        return;
    }

    // Light a fresh fuel item if needed.
    if runtime.fuel_left_ms == 0 {
        let Some((fuel_id, fuel_count)) = furnace.slot(FURNACE_FUEL_SLOT) else {
            // No fuel to light. Cook progress stays put (no
            // decay) but fuel bar stays at zero.
            runtime.fuel_full_ms = 0;
            return;
        };
        let burn_ticks = fuel_burn_ticks(fuel_id);
        if burn_ticks == 0 {
            // Fuel slot somehow contains a non-fuel - bail.
            runtime.fuel_full_ms = 0;
            return;
        }
        // Consume one fuel item. Same-id stack decrement, or
        // clear the slot when count drops to 0.
        let new_count = fuel_count.saturating_sub(1);
        if new_count == 0 {
            furnace.set_slot(FURNACE_FUEL_SLOT, None);
        } else {
            furnace.set_slot(FURNACE_FUEL_SLOT, Some((fuel_id, new_count)));
        }
        runtime.fuel_full_ms = burn_ticks * TICK_MS;
        runtime.fuel_left_ms = runtime.fuel_full_ms;
    }

    // Burn fuel + accumulate cook progress in lockstep.
    let burn_step = dt_ms.min(runtime.fuel_left_ms);
    runtime.fuel_left_ms -= burn_step;
    runtime.cook_progress_ms = runtime
        .cook_progress_ms
        .saturating_add(burn_step);
    if runtime.fuel_left_ms == 0 {
        // Fuel exhausted this tick. Drop the bar's max-burn
        // reference so the UI reads "no fuel"; cook progress
        // stays put (we don't decay mid-cook). A fresh fuel item
        // next tick will set both `fuel_left_ms` and
        // `fuel_full_ms` from the new burn duration.
        runtime.fuel_full_ms = 0;
    }

    // Check for cook completion. A long single-frame delta could
    // span multiple outputs in theory; in practice dt_ms < 1000
    // and one output costs 10 000 ms, so the inner check fires at
    // most once. We still loop defensively so the math holds for
    // synthetic test deltas.
    while runtime.cook_progress_ms >= FURNACE_COOK_MS_PER_OUTPUT {
        // Re-fetch the input slot fresh - the previous iteration
        // may have emptied it.
        let Some((cur_in_id, cur_in_count)) = furnace.slot(FURNACE_INPUT_SLOT) else {
            break;
        };
        // Recipe may differ from the original `output_id` if the
        // player swapped the input mid-cook. Bail in that case;
        // the next tick re-evaluates.
        let Some((cur_out_id, cur_out_count)) = smelt_recipe(cur_in_id) else {
            break;
        };
        if !output_slot_can_accept(furnace, cur_out_id, cur_out_count) {
            break;
        }
        runtime.cook_progress_ms -= FURNACE_COOK_MS_PER_OUTPUT;
        // Decrement input by 1; clear when it hits 0.
        let new_in = cur_in_count.saturating_sub(1);
        if new_in == 0 {
            furnace.set_slot(FURNACE_INPUT_SLOT, None);
        } else {
            furnace.set_slot(FURNACE_INPUT_SLOT, Some((cur_in_id, new_in)));
        }
        // Push the output. The output slot's accept_nothing
        // filter blocks `insert_at`, so we update via set_slot:
        // either start a new stack or grow the existing one.
        let new_output = match furnace.slot(FURNACE_OUTPUT_SLOT) {
            None => Some((cur_out_id, cur_out_count)),
            Some((sid, scount)) if sid == cur_out_id => {
                Some((cur_out_id, scount.saturating_add(cur_out_count)))
            }
            // Mismatched output id - bail out of the cook loop.
            Some(_) => {
                // Put progress back so we don't lose it next tick.
                runtime.cook_progress_ms += FURNACE_COOK_MS_PER_OUTPUT;
                break;
            }
        };
        furnace.set_slot(FURNACE_OUTPUT_SLOT, new_output);
    }
}

/// Drain `fuel_left_ms` by `dt_ms` while the furnace is idle (no
/// valid input + output combo). The cook progress does NOT advance.
/// Clears `fuel_full_ms` once the current burn finishes so the UI
/// fuel bar reads empty.
fn drain_fuel_idle(runtime: &mut FurnaceRuntime, dt_ms: u32) {
    if runtime.fuel_left_ms == 0 {
        runtime.fuel_full_ms = 0;
        return;
    }
    let step = dt_ms.min(runtime.fuel_left_ms);
    runtime.fuel_left_ms -= step;
    if runtime.fuel_left_ms == 0 {
        runtime.fuel_full_ms = 0;
    }
}

/// True when the output slot can accept `count` more of `output_id`.
/// Empty slot accepts; same-id stack with room accepts; different-id
/// or full same-id slot rejects.
fn output_slot_can_accept(
    furnace: &Container,
    output_id: u16,
    count: u8,
) -> bool {
    match furnace.slot(FURNACE_OUTPUT_SLOT) {
        None => true,
        Some((sid, scount)) if sid == output_id => {
            let cap = crate::item::item_max_stack(output_id);
            scount.saturating_add(count) <= cap
        }
        Some(_) => false,
    }
}

/// Apply a `FurnaceOutput` catch-up event to a furnace map. Used
/// by the save layer's `apply_catch_up_events` to land the offline
/// cook results back on the live world.
///
/// Wire steps:
/// 1. If `furnaces` has no entry at `pos`, the event is dropped
///    silently (the player broke the furnace mid-offline).
/// 2. Otherwise merge `count` of `block` into the output slot
///    (slot 2). Uses `Container::set_slot` so the accept_nothing
///    filter never rejects the cook's own product.
/// 3. If the output slot already holds a different item, the
///    event is dropped (shouldn't happen in practice - the
///    catch-up dispatcher checks the same gate - but we guard
///    defensively).
///
/// Returns `true` when the event landed, `false` otherwise.
pub(crate) fn apply_catch_up_output(
    furnaces: &mut HashMap<[i32; 3], Container>,
    pos: [i32; 3],
    block: u16,
    count: u8,
) -> bool {
    let Some(furnace) = furnaces.get_mut(&pos) else {
        return false;
    };
    let new_output = match furnace.slot(FURNACE_OUTPUT_SLOT) {
        None => Some((block, count)),
        Some((sid, scount)) if sid == block => {
            Some((block, scount.saturating_add(count)))
        }
        Some(_) => return false,
    };
    furnace.set_slot(FURNACE_OUTPUT_SLOT, new_output);
    true
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

#[cfg(test)]
#[path = "furnace_tick_tests.rs"]
mod furnace_tick_tests;
