//! Integration tests for the scheduled-timers persistence + catch-up
//! dispatch (`world_save::timers`).
//!
//! Pulled out of `tests.rs` to keep that file under the 500-LOC soft
//! cap; mirrors the `chunks_tests.rs` + `players_tests.rs` +
//! `entities_tests.rs` split. Each test spins up a fresh
//! `tempfile::TempDir` and drives the `_in_dir` variants so OS
//! data-dir state never bleeds between runs.
//!
//! Coverage:
//! - Round trips: empty + 3-kind populated bundle.
//! - Missing file -> Ok(None).
//! - Refusal: unknown envelope version.
//! - Catch-up:
//!   - Furnace emits one output per cook period; drops on input
//!     exhaustion.
//!   - Crop advances stages over elapsed periods; harvest event at
//!     max stage.
//!   - LiquidScheduled emits + drops the timer.
//!   - Offline cap clamps 30-day saves to 7-day equivalent.
//!   - Future timers (due after the cutoff) pass through untouched.
//!   - `last_saved_wall_ms` bumps to the cutoff on every call.

use tempfile::TempDir;

use super::timers::{
    CatchUpEvent, FURNACE_COOK_TICKS_PER_OUTPUT, GROWTH_PERIOD_MS, MAX_OFFLINE_MS,
    ScheduledTimers, TIMERS_FORMAT_VERSION, TICK_MS, Timer, TimerKind, catch_up,
    read_timers_in_dir, timers_path_in_dir, write_timers_in_dir,
};

const SLUG: &str = "slot";
const DIM: &str = "overworld";

fn furnace_timer(due: u64, fuel: u32, cook: u32, input: u16, output: u16) -> Timer {
    Timer {
        pos: [1, 2, 3],
        dim: DIM.to_string(),
        due_at_wall_ms: due,
        kind: TimerKind::Furnace {
            fuel_left_ticks: fuel,
            cook_progress_ticks: cook,
            input_block: input,
            output_block: output,
        },
    }
}

fn crop_timer(due: u64, stage: u8, max_stage: u8) -> Timer {
    Timer {
        pos: [4, 5, 6],
        dim: DIM.to_string(),
        due_at_wall_ms: due,
        kind: TimerKind::Crop { stage, max_stage },
    }
}

fn liquid_timer(due: u64, liquid_id: u16) -> Timer {
    Timer {
        pos: [7, 8, 9],
        dim: DIM.to_string(),
        due_at_wall_ms: due,
        kind: TimerKind::LiquidScheduled { liquid_id },
    }
}

#[test]
fn timers_roundtrip_empty() {
    let tmp = TempDir::new().unwrap();
    let original = ScheduledTimers::empty(42_000);
    write_timers_in_dir(tmp.path(), SLUG, &original).unwrap();

    let loaded = read_timers_in_dir(tmp.path(), SLUG)
        .unwrap()
        .expect("timers file must exist after write");
    assert_eq!(loaded, original);
    assert_eq!(loaded.format_version, TIMERS_FORMAT_VERSION);
    assert_eq!(loaded.last_saved_wall_ms, 42_000);
}

#[test]
fn timers_roundtrip_three_kinds() {
    // One Furnace + one Crop + one LiquidScheduled. Each variant's
    // payload has to round-trip cleanly so the dispatcher gets the
    // exact state the runtime wrote.
    let tmp = TempDir::new().unwrap();
    let original = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![
            furnace_timer(1_000_000, 400, 50, 17, 4),
            crop_timer(2_000_000, 2, 7),
            liquid_timer(3_000_000, 1),
        ],
        last_saved_wall_ms: 500_000,
    };
    write_timers_in_dir(tmp.path(), SLUG, &original).unwrap();

    let loaded = read_timers_in_dir(tmp.path(), SLUG).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.items.len(), 3);
    matches!(loaded.items[0].kind, TimerKind::Furnace { .. });
    matches!(loaded.items[1].kind, TimerKind::Crop { .. });
    matches!(loaded.items[2].kind, TimerKind::LiquidScheduled { .. });
}

#[test]
fn timers_missing_returns_none() {
    let tmp = TempDir::new().unwrap();
    let result = read_timers_in_dir(tmp.path(), SLUG).unwrap();
    assert!(result.is_none());
}

#[test]
fn timers_unknown_version_refuses_read() {
    let tmp = TempDir::new().unwrap();
    write_timers_in_dir(tmp.path(), SLUG, &ScheduledTimers::empty(0)).unwrap();

    let path = timers_path_in_dir(tmp.path(), SLUG);
    let mut bytes = std::fs::read(&path).unwrap();
    let bad = 777u32.to_le_bytes();
    bytes[4..8].copy_from_slice(&bad);
    std::fs::write(&path, &bytes).unwrap();
    let bak = path.with_file_name(format!(
        "{}.bak",
        path.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&bak);

    let err = read_timers_in_dir(tmp.path(), SLUG)
        .expect_err("expected envelope version mismatch");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("777"),
        "error must mention the unsupported version, got: {err}"
    );
}

#[test]
fn catch_up_furnace_emits_one_output_per_10s() {
    // 25 s elapsed against a furnace with plenty of fuel + an active
    // input. The dispatcher must emit two outputs (10 s + 20 s
    // boundaries) and keep the timer alive with the residual 5 s of
    // cook progress (5 s = 100 ticks) on the third item.
    let last_save = 1_000_000;
    let now = last_save + 25_000;
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![furnace_timer(last_save, /*fuel*/ 1_000, /*cook*/ 0, 17, 4)],
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);

    let outputs: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, CatchUpEvent::FurnaceOutput { .. }))
        .collect();
    assert_eq!(outputs.len(), 2, "25s should produce two 10s cooks");
    if let CatchUpEvent::FurnaceOutput { block, count, .. } = outputs[0] {
        assert_eq!(*block, 4);
        assert_eq!(*count, 1);
    }
    // Timer kept with the leftover cook progress + reduced fuel.
    assert_eq!(timers.items.len(), 1);
    if let TimerKind::Furnace {
        cook_progress_ticks,
        fuel_left_ticks,
        ..
    } = timers.items[0].kind
    {
        // 25 s = 500 ticks. Two full cooks consumed 400 ticks; 100
        // remain on the in-progress third item.
        assert_eq!(cook_progress_ticks, 100);
        // 500 ticks of fuel consumed; 500 remain of the 1000-tick
        // burner.
        assert_eq!(fuel_left_ticks, 500);
    } else {
        panic!("expected furnace timer after catch-up");
    }
}

#[test]
fn catch_up_furnace_drops_timer_when_input_exhausted() {
    // Furnace with input_block = 0 means "no input loaded". The
    // dispatcher must drop the timer so the runtime can mint a fresh
    // one when the player tosses items in.
    let last_save = 0;
    let now = last_save + 100_000;
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![furnace_timer(0, 1_000, 0, /*input=*/ 0, 4)],
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);
    assert!(events.is_empty(), "no input -> no outputs");
    assert!(
        timers.items.is_empty(),
        "no-input furnace timer must drop on catch-up"
    );
}

#[test]
fn catch_up_furnace_drops_timer_when_fuel_runs_out() {
    // Furnace with just enough fuel for one cook + an active input.
    // Should emit one output, then drop because fuel hit zero.
    let last_save = 0;
    let now = last_save + 30_000; // 30 s = 600 ticks elapsed
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![furnace_timer(
            0,
            FURNACE_COOK_TICKS_PER_OUTPUT,
            0,
            17,
            4,
        )],
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);
    let outputs: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, CatchUpEvent::FurnaceOutput { .. }))
        .collect();
    assert_eq!(outputs.len(), 1, "one cook before fuel runs out");
    assert!(
        timers.items.is_empty(),
        "out-of-fuel timer must drop on catch-up"
    );
}

#[test]
fn catch_up_crop_advances_stages() {
    // 15 min elapsed against a stage-0 / max-stage-7 crop. Each 5 min
    // = one stage, so the new stage = 3 (3 full periods consumed).
    let last_save = 1_000_000;
    let now = last_save + 15 * 60 * 1000;
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![crop_timer(last_save, 0, 7)],
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);
    assert!(events.is_empty(), "below max stage emits no harvest");
    assert_eq!(timers.items.len(), 1);
    if let TimerKind::Crop { stage, max_stage } = timers.items[0].kind {
        assert_eq!(stage, 3, "15 min / 5 min per stage = 3 stages");
        assert_eq!(max_stage, 7);
    } else {
        panic!("expected crop timer after catch-up");
    }
}

#[test]
fn catch_up_crop_emits_harvest_at_max_stage() {
    // Crop one stage off ripe + just enough elapsed for one more
    // advance. Must emit `CropHarvested` and drop the timer.
    let last_save = 1_000_000;
    let now = last_save + GROWTH_PERIOD_MS;
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![crop_timer(last_save, /*stage*/ 6, /*max*/ 7)],
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);
    let harvests: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, CatchUpEvent::CropHarvested { .. }))
        .collect();
    assert_eq!(harvests.len(), 1, "ripe crop must emit one harvest");
    if let CatchUpEvent::CropHarvested { block, count, .. } = harvests[0] {
        assert_eq!(*count, 1);
        // catch_up_crop reports the max stage as the output block id
        // (Phase C will refine this); pinning here makes the field
        // round-trip explicit so a future renumber breaks the test.
        assert_eq!(*block, 7);
    }
    assert!(
        timers.items.is_empty(),
        "harvested crop timer must drop after catch-up"
    );
}

#[test]
fn catch_up_liquid_scheduled_emits_event_drops_timer() {
    // A scheduled liquid tick whose due moment has passed. Must emit
    // a `LiquidScheduled` event so the runtime can re-hydrate its
    // live `LiquidSchedule`, then drop the timer.
    let last_save = 0;
    let now = last_save + 10_000;
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![liquid_timer(5_000, /*liquid_id*/ 1)],
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);
    assert_eq!(events.len(), 1);
    match &events[0] {
        CatchUpEvent::LiquidScheduled {
            liquid_id,
            due_at_wall_ms,
            ..
        } => {
            assert_eq!(*liquid_id, 1);
            assert_eq!(
                *due_at_wall_ms, now,
                "liquid catch-up reports the cutoff as the new due"
            );
        }
        other => panic!("expected LiquidScheduled, got {other:?}"),
    }
    assert!(timers.items.is_empty(), "liquid timer must drop on emit");
}

#[test]
fn catch_up_caps_offline_at_seven_days() {
    // 30 days elapsed must be clamped to MAX_OFFLINE_MS = 7 days. The
    // furnace gets 7 days worth of cook ticks, not 30. With 1000
    // ticks of fuel that's well under the 7-day cap, so the timer
    // drops on fuel exhaustion after emitting 5 outputs (1000 / 200).
    let last_save = 0;
    let thirty_days_ms = 30 * 24 * 60 * 60 * 1000;
    let now = last_save + thirty_days_ms;
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![furnace_timer(last_save, 1_000, 0, 17, 4)],
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);
    let outputs = events
        .iter()
        .filter(|e| matches!(e, CatchUpEvent::FurnaceOutput { .. }))
        .count();
    // 1000 ticks of fuel / 200 ticks per cook = 5 outputs max.
    assert_eq!(outputs, 5, "fuel-limited furnace caps at 5 outputs");
    // last_saved_wall_ms bumps by exactly MAX_OFFLINE_MS, not the raw
    // 30-day delta - the cap is what protects boot time.
    assert_eq!(timers.last_saved_wall_ms, last_save + MAX_OFFLINE_MS);
}

#[test]
fn catch_up_keeps_future_timers_untouched() {
    // A timer due tomorrow must pass through unchanged on a boot
    // that elapses just a few seconds.
    let last_save = 1_000_000;
    let now = last_save + 30_000;
    let future_due = last_save + 24 * 60 * 60 * 1000;
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: vec![crop_timer(future_due, 0, 7)],
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);
    assert!(events.is_empty());
    assert_eq!(timers.items.len(), 1);
    assert_eq!(timers.items[0].due_at_wall_ms, future_due);
    if let TimerKind::Crop { stage, max_stage } = timers.items[0].kind {
        assert_eq!(stage, 0, "future-due crop must not advance stage");
        assert_eq!(max_stage, 7);
    }
}

#[test]
fn catch_up_bumps_last_saved_wall_ms() {
    // Even on a no-due-timers call `last_saved_wall_ms` advances to
    // `now` so drift can't accumulate across restarts.
    let last_save = 1_000_000;
    let now = last_save + 60_000;
    let mut timers = ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items: Vec::new(),
        last_saved_wall_ms: last_save,
    };
    let events = catch_up(&mut timers, now);
    assert!(events.is_empty());
    assert_eq!(timers.last_saved_wall_ms, now);
}

#[test]
fn catch_up_tick_constant_matches_50ms() {
    // Sanity guard: pin the runtime tick rate so the furnace catch-up
    // math doesn't silently drift when someone tweaks the constant.
    assert_eq!(TICK_MS, 50);
}
