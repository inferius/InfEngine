//! Time-warp catch-up dispatch for scheduled timers.
//!
//! Pulled out of `timers::mod` to keep both files under the 500-LOC
//! soft cap. The on-disk types + envelope I/O stay in `mod.rs`; this
//! file owns the pure-function simulation that replays elapsed wall
//! time against every due timer.
//!
//! See `docs/design/save_architecture_final.md` ("Time-warp catch-up")
//! for the contract. Algorithm summary:
//!
//! 1. `elapsed_ms = (now_wall_ms - last_saved_wall_ms).min(MAX_OFFLINE_MS)`.
//! 2. `cutoff_ms = last_saved_wall_ms + elapsed_ms`.
//! 3. Partition `timers.items` on `due_at_wall_ms <= cutoff_ms`.
//! 4. Each due timer dispatches to a per-kind catch-up that returns
//!    `CatchUpResult { keep, events }`.
//! 5. Aggregate events. Replace `items` with the retained timers. Bump
//!    `last_saved_wall_ms` to `cutoff_ms`.
//! 6. Return the event list. Caller applies it to live world state.

use super::{
    FURNACE_COOK_TICKS_PER_OUTPUT, GROWTH_PERIOD_MS, MAX_OFFLINE_MS, ScheduledTimers,
    TICK_MS, Timer, TimerKind,
};

/// Result of replaying a single timer against the elapsed wall time.
///
/// `keep` is either the timer to retain (possibly with mutated state)
/// or `None` if the timer has been fully consumed; `events` is the
/// list of side effects the caller needs to apply to live world
/// state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CatchUpResult {
    /// Updated timer to keep, or `None` to drop the timer (event
    /// fired or stage finished).
    pub keep: Option<Timer>,
    /// Events emitted by the catch-up - blocks placed in inventories,
    /// crops harvested, scheduled liquid events. The caller applies
    /// these to live world state.
    pub events: Vec<CatchUpEvent>,
}

/// One side effect emitted by `catch_up` that the caller must apply
/// to live world state. The variants are deliberately small + flat
/// (no nested timers, no recursion) so wiring B.7 into a chest
/// inventory + a re-scheduled `LiquidSchedule` stays a straight
/// match on the kind.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CatchUpEvent {
    /// Furnace finished cooking one item. Caller pushes `count`
    /// units of `block` into the furnace's output slot.
    FurnaceOutput {
        pos: [i32; 3],
        dim: String,
        block: u16,
        count: u8,
    },
    /// Crop reached `max_stage`. Caller swaps the live block for its
    /// harvested form + adds `count` units of `block` to the player's
    /// drop list (or the chunk's loot table).
    CropHarvested {
        pos: [i32; 3],
        dim: String,
        block: u16,
        count: u8,
    },
    /// One scheduled liquid-spread tick. Caller re-hydrates the live
    /// `LiquidSchedule` with the timer's `due_at_wall_ms` so the
    /// spread resumes mid-flight rather than re-rolling from scratch.
    LiquidScheduled {
        pos: [i32; 3],
        dim: String,
        liquid_id: u16,
        due_at_wall_ms: u64,
    },
}

/// Replay all due timers against `now_wall_ms`, capped at
/// `MAX_OFFLINE_MS` of elapsed time so a year-old save doesn't burn
/// minutes simulating on load.
///
/// Steps:
/// 1. `elapsed_ms = (now_wall_ms - timers.last_saved_wall_ms).min(MAX_OFFLINE_MS)`.
/// 2. `cutoff = last_saved_wall_ms + elapsed_ms`.
/// 3. For each timer with `due_at_wall_ms <= cutoff`, dispatch to a
///    per-kind catch-up fn. The fn returns `CatchUpResult { keep,
///    events }`; events go into the aggregated list, `keep` is
///    reinserted into `items` (or dropped).
/// 4. Future timers (due after the cutoff) pass through untouched.
/// 5. `last_saved_wall_ms` is bumped to `cutoff` so the next call
///    measures elapsed from there - even on the no-due-timers case
///    this prevents drift from accumulating across restarts.
///
/// Returns the aggregated event list. The caller applies events to
/// live world state (push into furnace output, swap crop block,
/// re-schedule liquid ticks).
pub(crate) fn catch_up(
    timers: &mut ScheduledTimers,
    now_wall_ms: u64,
) -> Vec<CatchUpEvent> {
    let raw_elapsed = now_wall_ms.saturating_sub(timers.last_saved_wall_ms);
    let elapsed_ms = raw_elapsed.min(MAX_OFFLINE_MS);
    let cutoff_ms = timers.last_saved_wall_ms.saturating_add(elapsed_ms);

    let mut events: Vec<CatchUpEvent> = Vec::new();
    let old_items = std::mem::take(&mut timers.items);
    let mut new_items: Vec<Timer> = Vec::with_capacity(old_items.len());

    for timer in old_items {
        if timer.due_at_wall_ms > cutoff_ms {
            // Future timer - pass through untouched.
            new_items.push(timer);
            continue;
        }
        // Per-timer catch-up uses the GLOBAL elapsed window (capped),
        // not a per-due slice: the design doc models "the entire
        // offline window has elapsed for every due timer" so the
        // catch-up dispatchers don't need to reason about each
        // timer's individual schedule.
        let result = dispatch_catch_up(timer, elapsed_ms, cutoff_ms);
        events.extend(result.events);
        if let Some(keep) = result.keep {
            new_items.push(keep);
        }
    }

    timers.items = new_items;
    timers.last_saved_wall_ms = cutoff_ms;
    events
}

/// Route a single due timer to its per-kind catch-up fn. Keeps
/// `catch_up` itself short + lets the dispatch grow with new variants
/// without ballooning the outer loop.
fn dispatch_catch_up(timer: Timer, elapsed_ms: u64, now_wall_ms: u64) -> CatchUpResult {
    match &timer.kind {
        TimerKind::Furnace { .. } => catch_up_furnace(timer, elapsed_ms),
        TimerKind::Crop { .. } => catch_up_crop(timer, elapsed_ms),
        TimerKind::LiquidScheduled { .. } => {
            catch_up_liquid_scheduled(timer, now_wall_ms)
        }
    }
}

/// Furnace catch-up. While `fuel > 0 && input > 0`, each game tick
/// drains one fuel + advances cook progress by 1. At cook progress
/// at or above `FURNACE_COOK_TICKS_PER_OUTPUT`, emit one
/// `FurnaceOutput`, reset progress, consume one input. The runtime
/// models input slots as a single block + an implicit count (count
/// tracking is Phase C); here we keep the timer alive after each cook
/// unless fuel hit zero since the live slot may have many more inputs
/// queued.
///
/// Once fuel runs out, drop the timer - the runtime will mint a fresh
/// one when the player tosses in more fuel.
fn catch_up_furnace(mut timer: Timer, elapsed_ms: u64) -> CatchUpResult {
    let mut events: Vec<CatchUpEvent> = Vec::new();

    let (mut fuel, mut cook, input, output) = match timer.kind {
        TimerKind::Furnace {
            fuel_left_ticks,
            cook_progress_ticks,
            input_block,
            output_block,
        } => (fuel_left_ticks, cook_progress_ticks, input_block, output_block),
        // dispatch_catch_up only calls this on the Furnace variant.
        _ => unreachable!("catch_up_furnace called on non-furnace timer"),
    };

    if input == 0 {
        // No input loaded -> timer is moot, drop it. The runtime
        // mints a fresh timer on next item insert.
        return CatchUpResult {
            keep: None,
            events,
        };
    }

    // Number of game ticks the elapsed window can spend.
    let mut ticks_left: u64 = elapsed_ms / TICK_MS;
    while ticks_left > 0 && fuel > 0 {
        // How many ticks until either the cook finishes or fuel runs
        // out? Take the minimum so we advance in one batch.
        let cook_remaining = FURNACE_COOK_TICKS_PER_OUTPUT - cook;
        let step = ticks_left
            .min(cook_remaining as u64)
            .min(fuel as u64);
        if step == 0 {
            break;
        }
        cook += step as u32;
        fuel -= step as u32;
        ticks_left -= step;
        if cook >= FURNACE_COOK_TICKS_PER_OUTPUT {
            events.push(CatchUpEvent::FurnaceOutput {
                pos: timer.pos,
                dim: timer.dim.clone(),
                block: output,
                count: 1,
            });
            cook = 0;
            // Phase C will track an input-stack count; until then a
            // single completed cook is taken to mean the slot still
            // has more items queued. The runtime is free to clear
            // the timer (input_block = 0) when its physical slot
            // empties.
        }
    }

    if fuel == 0 {
        // Out of fuel - the live runtime will re-mint a timer when
        // the player tops the furnace up.
        return CatchUpResult {
            keep: None,
            events,
        };
    }

    timer.kind = TimerKind::Furnace {
        fuel_left_ticks: fuel,
        cook_progress_ticks: cook,
        input_block: input,
        output_block: output,
    };
    CatchUpResult {
        keep: Some(timer),
        events,
    }
}

/// Crop catch-up. `stages_to_advance = elapsed_ms / GROWTH_PERIOD_MS`
/// (integer div). At `max_stage` emit a single `CropHarvested` event
/// and drop the timer; below `max_stage` keep the timer with the
/// bumped stage and a new `due_at_wall_ms` shifted forward by the
/// number of full periods we just consumed.
///
/// Note: at least one full growth period must elapse for a stage to
/// advance. A timer that fired exactly at `due_at_wall_ms` with no
/// extra elapsed time stays at the same stage + the same due (the
/// dispatcher will re-fire it on the next boot). This matches the
/// design-doc spec "stages_to_advance = elapsed / GROWTH_PERIOD_MS".
fn catch_up_crop(mut timer: Timer, elapsed_ms: u64) -> CatchUpResult {
    let (stage, max_stage) = match timer.kind {
        TimerKind::Crop { stage, max_stage } => (stage, max_stage),
        _ => unreachable!("catch_up_crop called on non-crop timer"),
    };

    if max_stage == 0 {
        // Degenerate config - nothing to do. Drop the timer so we
        // don't keep dispatching it on every boot.
        return CatchUpResult {
            keep: None,
            events: Vec::new(),
        };
    }

    let elapsed_periods = elapsed_ms / GROWTH_PERIOD_MS;
    if elapsed_periods == 0 {
        // Less than one growth period elapsed - no stage advance, no
        // reschedule. Keep the timer for the next boot.
        return CatchUpResult {
            keep: Some(timer),
            events: Vec::new(),
        };
    }
    let new_stage = (stage as u64 + elapsed_periods).min(max_stage as u64) as u8;

    if new_stage >= max_stage {
        // Crop is ripe - emit harvest + drop the timer. The runtime
        // applies the live block swap.
        return CatchUpResult {
            keep: None,
            events: vec![CatchUpEvent::CropHarvested {
                pos: timer.pos,
                dim: timer.dim.clone(),
                // Block id = max_stage for the moment; Phase C / E
                // will refine this with per-crop output tables. The
                // catch-up surfaces what stage finished so the caller
                // can look up the right drop.
                block: max_stage as u16,
                count: 1,
            }],
        };
    }

    // Below ripe - bump stage + reschedule. Re-anchor `due_at_wall_ms`
    // forward by `elapsed_periods * GROWTH_PERIOD_MS` from the old
    // due, so the next catch-up sees the right offset even if the
    // server stays running past `now`.
    timer.kind = TimerKind::Crop {
        stage: new_stage,
        max_stage,
    };
    timer.due_at_wall_ms = timer
        .due_at_wall_ms
        .saturating_add(elapsed_periods * GROWTH_PERIOD_MS);
    CatchUpResult {
        keep: Some(timer),
        events: Vec::new(),
    }
}

/// Liquid scheduled-tick catch-up. The simplest of the three: emit
/// the event so the caller can re-hydrate its in-memory
/// `LiquidSchedule`, then drop the timer (the live schedule is now
/// the source of truth for follow-up spreads).
fn catch_up_liquid_scheduled(timer: Timer, now_wall_ms: u64) -> CatchUpResult {
    let liquid_id = match timer.kind {
        TimerKind::LiquidScheduled { liquid_id } => liquid_id,
        _ => unreachable!("catch_up_liquid_scheduled called on non-liquid timer"),
    };
    let event = CatchUpEvent::LiquidScheduled {
        pos: timer.pos,
        dim: timer.dim,
        liquid_id,
        due_at_wall_ms: now_wall_ms,
    };
    CatchUpResult {
        keep: None,
        events: vec![event],
    }
}
