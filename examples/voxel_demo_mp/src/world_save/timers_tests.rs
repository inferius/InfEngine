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

/// Diagnostic dump (not a regression test - #[ignore]d). Reads a LIVE
/// save's scheduled.bin + the chunks around each liquid timer and
/// prints the neighbourhood, so a "N liquid timers never settle"
/// report can be root-caused offline. Run with:
///   cargo test -p voxel_demo_mp --release --lib \
///     dump_live_save_liquid_timers -- --ignored --nocapture
/// Slug via DUMP_SLUG env (default test-mc-15).
#[test]
#[ignore]
#[allow(clippy::print_stdout)]
fn dump_live_save_liquid_timers() {
    let slug = std::env::var("DUMP_SLUG").unwrap_or_else(|_| "test-mc-15".into());
    let Ok(Some(bundle)) = crate::world_save::timers::read_timers(&slug) else {
        println!("no timers bundle for slug {slug}");
        return;
    };
    println!("timers: {} items", bundle.items.len());
    let cs = infengine_voxel::CHUNK_SIZE as i32;
    let name_of = |id: u16| -> String {
        if id == 0 {
            "air".into()
        } else {
            crate::block::block_type(id)
                .map(|bt| bt.name.to_string())
                .unwrap_or_else(|| format!("id{id}"))
        }
    };
    for t in &bundle.items {
        let TimerKind::LiquidScheduled { liquid_id } = &t.kind else {
            println!("- {:?} dim={} kind={:?}", t.pos, t.dim, t.kind);
            continue;
        };
        println!(
            "- liquid timer at {:?} dim={} liquid={}",
            t.pos,
            t.dim,
            name_of(*liquid_id)
        );
        // Print a 3x3x3 neighbourhood: block name + saved liquid level.
        for dy in (-1..=1).rev() {
            for dz in -1..=1 {
                let mut row = String::new();
                for dx in -1..=1 {
                    let p = [t.pos[0] + dx, t.pos[1] + dy, t.pos[2] + dz];
                    let cc = [
                        p[0].div_euclid(cs),
                        p[1].div_euclid(cs),
                        p[2].div_euclid(cs),
                    ];
                    let cell = crate::world_save::chunks::read_chunk(&slug, &t.dim, cc)
                        .ok()
                        .flatten()
                        .map(|payload| {
                            let local = [
                                p[0].rem_euclid(cs) as u8,
                                p[1].rem_euclid(cs) as u8,
                                p[2].rem_euclid(cs) as u8,
                            ];
                            let lvl = payload
                                .liquid_levels
                                .iter()
                                .find(|(l, _)| *l == local)
                                .map(|(_, v)| *v);
                            let chunk = payload.voxels.into_chunk().ok();
                            let id = chunk
                                .map(|c| {
                                    c.get(
                                        local[0] as usize,
                                        local[1] as usize,
                                        local[2] as usize,
                                    )
                                    .0
                                })
                                .unwrap_or(u16::MAX);
                            match lvl {
                                Some(l) => format!("{}:{}", name_of(id), l),
                                None => name_of(id),
                            }
                        })
                        .unwrap_or_else(|| "?".into());
                    row.push_str(&format!("{cell:<22}"));
                }
                println!("  y{dy:+} z{dz:+}: {row}");
            }
        }
    }
}

/// Companion diagnostic to `dump_live_save_liquid_timers`: rebuild the
/// saved neighbourhood in a headless Voxel (disk chunks + real worldgen
/// for the gaps), seed the schedule with the persisted timers, then run
/// the REAL per-cell tick machinery and watch whether the schedule
/// drains. Reproduces "N liquid timers tick forever" reports offline.
/// Run with:
///   cargo test -p voxel_demo_mp --release --bin voxel_demo_mp \
///     simulate_live_save_liquid_settle -- --ignored --nocapture
#[test]
#[ignore]
#[allow(clippy::print_stdout)]
fn simulate_live_save_liquid_settle() {
    let slug = std::env::var("DUMP_SLUG").unwrap_or_else(|_| "test-mc-15".into());
    let seed: u32 = std::env::var("DUMP_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(857148473);
    let Ok(Some(bundle)) = crate::world_save::timers::read_timers(&slug) else {
        println!("no timers bundle for slug {slug}");
        return;
    };
    let dim = "overworld".to_string();
    let timer_cells: Vec<[i32; 3]> = bundle
        .items
        .iter()
        .filter(|t| {
            t.dim == dim && matches!(t.kind, TimerKind::LiquidScheduled { .. })
        })
        .map(|t| t.pos)
        .collect();
    println!("seeding {} liquid timers", timer_cells.len());

    let cs = infengine_voxel::CHUNK_SIZE as i32;
    let ctx = crate::worldgen::WorldgenCtx {
        base_seed: seed,
        dim: dim.clone(),
        profile: crate::worldgen::WorldGen::Mc,
    };
    let mut v = crate::Voxel::new();
    // Pull a chunk box around every timer: disk first (real edited
    // state incl. liquid levels), worldgen for never-saved gaps.
    let mut wanted: std::collections::BTreeSet<[i32; 3]> = Default::default();
    for p in &timer_cells {
        let cc = [
            p[0].div_euclid(cs),
            p[1].div_euclid(cs),
            p[2].div_euclid(cs),
        ];
        for dx in -2..=2 {
            for dy in -2..=2 {
                for dz in -2..=2 {
                    wanted.insert([cc[0] + dx, cc[1] + dy, cc[2] + dz]);
                }
            }
        }
    }
    let mut from_disk = 0usize;
    let mut from_gen = 0usize;
    for cc in wanted {
        match crate::world_save::chunks::read_chunk(&slug, &dim, cc) {
            Ok(Some(payload)) => {
                let coord = payload.coord;
                let liquid_levels = payload.liquid_levels.clone();
                let chunk = payload.voxels.into_chunk().expect("chunk decode");
                let origin = [coord[0] * cs, coord[1] * cs, coord[2] * cs];
                // Index liquid cells like apply_chunk_payload does.
                for ly in 0..cs {
                    for lz in 0..cs {
                        for lx in 0..cs {
                            let b = chunk.get(lx as usize, ly as usize, lz as usize);
                            if crate::liquid::liquid_for(b).is_some() {
                                v.liquid_positions.insert([
                                    origin[0] + lx,
                                    origin[1] + ly,
                                    origin[2] + lz,
                                ]);
                            }
                        }
                    }
                }
                v.world.insert_chunk(coord, chunk);
                for (local, level) in liquid_levels {
                    let wp = [
                        origin[0] + local[0] as i32,
                        origin[1] + local[1] as i32,
                        origin[2] + local[2] as i32,
                    ];
                    v.liquid_level.insert(wp, level.min(8));
                }
                from_disk += 1;
            }
            _ => {
                let chunk = crate::worldgen::generate_chunk_at_ctx(cc, &ctx);
                let origin = [cc[0] * cs, cc[1] * cs, cc[2] * cs];
                for ly in 0..cs {
                    for lz in 0..cs {
                        for lx in 0..cs {
                            let b = chunk.get(lx as usize, ly as usize, lz as usize);
                            if crate::liquid::liquid_for(b).is_some() {
                                v.liquid_positions.insert([
                                    origin[0] + lx,
                                    origin[1] + ly,
                                    origin[2] + lz,
                                ]);
                            }
                        }
                    }
                }
                v.world.insert_chunk(cc, chunk);
                from_gen += 1;
            }
        }
    }
    println!("chunks: {from_disk} disk + {from_gen} gen");

    for p in &timer_cells {
        v.liquid_schedule.schedule(*p, 1);
    }
    v.liquid_clock = 0;
    let mut last_fired: Vec<[i32; 3]> = Vec::new();
    for tick in 1..=400u64 {
        v.liquid_clock = tick;
        let mut dirty: std::collections::HashSet<infengine_voxel::ChunkCoord> =
            Default::default();
        let mut fired: Vec<[i32; 3]> = Vec::new();
        while let Some(pos) = v.liquid_schedule.pop_due(tick) {
            let b = v.world.get(pos);
            let Some(lq) = crate::liquid::liquid_for(b) else {
                continue;
            };
            let lq = *lq;
            v.tick_liquid_cell(pos, &lq, tick, &mut dirty);
            fired.push(pos);
        }
        for lq in crate::liquid::LIQUIDS {
            if lq.infinite_source {
                v.tick_liquid_merge(lq);
            }
        }
        if tick <= 10 || tick % 40 == 0 || v.liquid_schedule.len() == 0 {
            println!(
                "tick {tick}: fired={} sched={} dirty_chunks={}",
                fired.len(),
                v.liquid_schedule.len(),
                dirty.len()
            );
        }
        last_fired = fired;
        if v.liquid_schedule.len() == 0 {
            println!("SETTLED at tick {tick}");
            return;
        }
    }
    println!("NOT SETTLED after 400 ticks; survivors:");
    for pos in v.liquid_schedule.iter().take(40) {
        let b = v.world.get(pos);
        let lvl = v.liquid_level.get(&pos).copied();
        println!("  {:?} block={} level={:?}", pos, b.0, lvl);
    }
    println!("last tick fired {} cells: {:?}", last_fired.len(), &last_fired[..last_fired.len().min(40)]);
}

/// W324 diag: print the terrain column under the saved player pose -
/// disk chunk if present, live worldgen otherwise. Confirms whether
/// the ground the player should stand on actually exists in the
/// current build (fall-through-world reports).
///
///     DUMP_SLUG=test-mc-21 cargo test -p voxel_demo_mp --release \
///         --bin voxel_demo_mp dump_ground_under_saved_player -- --ignored --nocapture
#[test]
#[ignore]
#[allow(clippy::print_stdout)]
fn dump_ground_under_saved_player() {
    let slug = std::env::var("DUMP_SLUG").unwrap_or_else(|_| "test-mc-21".into());
    let Ok(Some(player)) =
        crate::world_save::players::read_player(&slug, "host")
    else {
        println!("no host player save for slug {slug}");
        return;
    };
    let world_meta = match crate::world_save::world_json::read_world(&slug) {
        Ok(m) => m,
        Err(e) => {
            println!("no world.json: {e}");
            return;
        }
    };
    let seed = world_meta.seed;
    println!(
        "player pos {:?} yaw {:.2} dim {:?} seed {seed}",
        player.pos, player.yaw, player.last_dimension,
    );
    let dim = player.last_dimension.clone();
    let ctx = crate::worldgen::WorldgenCtx {
        base_seed: seed,
        dim: dim.clone(),
        profile: crate::worldgen::WorldGen::Mc,
    };
    let cs = infengine_voxel::CHUNK_SIZE as i32;
    // DUMP_POS=x,y,z overrides the probe point (defaults to the
    // saved player pose).
    let over: Option<[f32; 3]> = std::env::var("DUMP_POS").ok().and_then(|v| {
        let p: Vec<f32> =
            v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        (p.len() == 3).then(|| [p[0], p[1], p[2]])
    });
    let probe = over.unwrap_or(player.pos);
    let wx = probe[0].floor() as i32;
    let wz = probe[2].floor() as i32;
    let py = probe[1];
    println!(
        "mc_floor_at({wx},{wz}) = {}",
        crate::worldgen::mcgen::terrain::mc_floor_at(&ctx, wx, wz)
    );
    // Walk the vertical column +-24 blocks around the player.
    let y_lo = (py as i32 - 24).max(-64);
    let y_hi = (py as i32 + 8).min(319);
    let mut cache: std::collections::HashMap<[i32; 3], infengine_voxel::Chunk> =
        Default::default();
    for y in (y_lo..=y_hi).rev() {
        let cc = [wx.div_euclid(cs), y.div_euclid(cs), wz.div_euclid(cs)];
        let chunk = cache.entry(cc).or_insert_with(|| {
            match crate::world_save::chunks::read_chunk(&slug, &dim, cc) {
                Ok(Some(payload)) => {
                    println!("  chunk {cc:?}: FROM DISK");
                    payload.voxels.into_chunk().expect("chunk decode")
                }
                _ => {
                    println!("  chunk {cc:?}: generated");
                    crate::worldgen::generate_chunk_at_ctx(cc, &ctx)
                }
            }
        });
        let b = chunk
            .get(
                wx.rem_euclid(cs) as usize,
                y.rem_euclid(cs) as usize,
                wz.rem_euclid(cs) as usize,
            );
        let name = crate::block::block_type(b.0)
            .map(|t| t.name)
            .unwrap_or("?");
        let marker = if (y as f32) <= py && py < y as f32 + 1.0 {
            " <-- player"
        } else {
            ""
        };
        println!("y {y:4}: id {:4} {name}{marker}", b.0);
    }
}

/// W324 diag: run the real load_world on a slug headless and report
/// the outcome + where the player lands. Repro for the
/// fell-through-world-after-load report.
///
///     DUMP_SLUG=test-mc-21 cargo test -p voxel_demo_mp --release \
///         --bin voxel_demo_mp load_world_headless_repro -- --ignored --nocapture
#[test]
#[ignore]
#[allow(clippy::print_stdout)]
fn load_world_headless_repro() {
    let slug = std::env::var("DUMP_SLUG").unwrap_or_else(|_| "test-mc-21".into());
    let mut v = crate::Voxel::new();
    match crate::world_save::load_world(&mut v, &slug) {
        Ok(_loaded) => {
            println!(
                "load_world OK: live_chunks={} player_pos={:?} dim={} embedded={}",
                v.world.iter_chunks().count(),
                v.player_pos,
                v.current_dim,
                v.capsule_embedded(v.player_pos),
            );
        }
        Err(e) => {
            println!("load_world FAILED: kind={:?} err={e}", e.kind());
        }
    }
}
