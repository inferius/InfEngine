//! Save / apply for the scheduled-timers bundle (liquid spread +
//! furnace progress).

use tracing::debug;

use crate::Voxel;
use crate::liquid::liquid_for;

// helpers is `world_save::runtime::helpers`; its parent is
// `world_save::runtime` (which exposes OVERWORLD pub(super)) and
// grandparent `world_save` (where current_wall_ms lives).
use super::super::OVERWORLD;
use crate::world_save::current_wall_ms;
use crate::world_save::timers::{
    ScheduledTimers, TIMERS_FORMAT_VERSION, Timer, TimerKind,
    catch_up::CatchUpEvent,
};

/// Build a `ScheduledTimers` bundle from the live `LiquidSchedule`
/// plus every active furnace. Crops don't exist yet; their builders
/// land later in Phase C.
///
/// Furnace timers: one per furnace that has both an input AND a
/// fuel source loaded (live OR fuel_left_ms > 0). The timer's
/// `fuel_left_ticks` reflects the current burn divided by TICK_MS;
/// `cook_progress_ticks` does the same for cook progress; the
/// `input_block` carries slot-0's id and the `output_block` is
/// the recipe's product. Offline catch-up replays at most the
/// `MAX_OFFLINE_MS` cap.
pub(in crate::world_save::runtime) fn build_timers(voxel: &Voxel) -> ScheduledTimers {
    let now = current_wall_ms();
    let mut items: Vec<Timer> = Vec::new();
    for pos in voxel.liquid_schedule.iter() {
        // The schedule stores `due_at_tick` privately; for the save
        // we approximate with "fire on the next boot's catch-up
        // cycle". That keeps the spread paused while the world is
        // offline (no time-warp acceleration) but resumes it the
        // moment the player logs back in. Phase B follow-up surfaces
        // the actual due-tick if cross-boot accuracy is needed.
        let liquid_id = voxel.world.get(pos).0;
        items.push(Timer {
            pos,
            dim: OVERWORLD.to_string(),
            due_at_wall_ms: now,
            kind: TimerKind::LiquidScheduled { liquid_id },
        });
    }
    // Furnace timers. Only mint a timer for furnaces with a
    // smeltable input (otherwise the catch-up has nothing to do).
    // Each emits zero or more FurnaceOutput events on load.
    for (pos, furnace) in voxel.furnaces.iter() {
        let Some((input_id, _)) = furnace.slot(0) else {
            continue;
        };
        let Some((output_id, _)) = crate::smelting::smelt_recipe(input_id) else {
            continue;
        };
        let runtime = voxel
            .furnace_runtimes
            .get(pos)
            .copied()
            .unwrap_or_default();
        // Convert ms -> ticks at the boundary. Round down (saturating
        // div) so the catch-up doesn't accidentally claim an extra
        // tick of fuel that wasn't there.
        let fuel_left_ticks = runtime.fuel_left_ms
            / crate::block::furnace::TICK_MS;
        let cook_progress_ticks = runtime.cook_progress_ms
            / crate::block::furnace::TICK_MS;
        // Skip timers that have no fuel left AND no progress AND
        // no spare fuel in the slot - they're inert.
        let has_fuel_in_slot = matches!(furnace.slot(1), Some((id, n)) if n > 0
            && crate::smelting::fuel_burn_ticks(id) > 0);
        if fuel_left_ticks == 0 && cook_progress_ticks == 0 && !has_fuel_in_slot {
            continue;
        }
        items.push(Timer {
            pos: *pos,
            dim: OVERWORLD.to_string(),
            due_at_wall_ms: now,
            kind: TimerKind::Furnace {
                fuel_left_ticks,
                cook_progress_ticks,
                input_block: input_id,
                output_block: output_id,
            },
        });
    }
    ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items,
        last_saved_wall_ms: now,
    }
}

/// Apply the events emitted by `timers::catch_up`.
///
/// Wired variants:
/// - `FurnaceOutput`: push `count` of `block` into the matching
///   furnace's output slot (slot 2). Uses `Container::set_slot`
///   so the accept_nothing filter doesn't reject the cook's own
///   product. Furnaces no longer in `voxel.furnaces` (player
///   broke them mid-offline) drop the event silently.
/// - `CropHarvested`: deferred. Crops land in a later Phase C
///   commit.
/// - `LiquidScheduled`: consumed by `hydrate_liquid_schedule` so
///   this path skips them.
pub(in crate::world_save::runtime) fn apply_catch_up_events(
    voxel: &mut Voxel,
    events: &[CatchUpEvent],
) {
    for evt in events {
        match evt {
            CatchUpEvent::FurnaceOutput { pos, block, count, .. } => {
                let landed = crate::block::furnace::apply_catch_up_output(
                    &mut voxel.furnaces,
                    *pos,
                    *block,
                    *count,
                );
                if !landed {
                    debug!(?pos, ?block, ?count, "world_save: furnace catch-up output dropped");
                }
            }
            CatchUpEvent::CropHarvested { pos, .. } => {
                debug!(?pos, "world_save: crop catch-up event (Phase C TODO)");
            }
            CatchUpEvent::LiquidScheduled { .. } => {
                // Handled by hydrate_liquid_schedule.
            }
        }
    }
}

/// Re-arm the live `LiquidSchedule` from `LiquidScheduled` events.
/// Each event becomes a fresh schedule entry at `liquid_clock +
/// tick_period` so the spread picks up where it left off.
pub(in crate::world_save::runtime) fn hydrate_liquid_schedule(
    voxel: &mut Voxel,
    events: &[CatchUpEvent],
) {
    for evt in events {
        if let CatchUpEvent::LiquidScheduled { pos, .. } = evt {
            let b = voxel.world.get(*pos);
            if let Some(lq) = liquid_for(b) {
                let when = voxel.liquid_clock + (lq.tick_period.max(1)) as u64;
                voxel.liquid_schedule.schedule(*pos, when);
                voxel.liquid_positions.insert(*pos);
            }
        }
    }
}
