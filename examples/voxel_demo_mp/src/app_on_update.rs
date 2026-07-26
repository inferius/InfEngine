//! W47.22 - on_update body relocated from main.rs.
//!
//! Per-frame tick driver. Calls plugin on_tick, polls master queries,
//! handles Esc / pause input, computes input axes, drives mining +
//! place/break + bucket + combat helpers, runs tick_net + chunk
//! streaming, advances world simulation (mobs, liquids, drops,
//! furnaces, lava ignite, lighting, day clock), schedules ambient
//! particles, and orchestrates the world render pass.
//!
//! `self.` rewritten to `vx.`, `Self::` to `Voxel::`. Body otherwise
//! unchanged.

#![allow(clippy::too_many_lines)]

use crate::*;
use infengine_window::{EngineWindow, FrameContext};

pub(crate) fn on_update(vx: &mut Voxel, window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.clamp(1e-4, 0.1);
        // W103 perf-diag: wall-clock start of this frame's on_update so the
        // 2.5s diag can split total frame cost into sim phases vs render.
        let _frame_start = std::time::Instant::now();
        // W198.16: this frame's discrete phase costs, captured so the
        // low-FPS auto-log can print a hitch's breakdown into the log
        // (deduped to once per diag window) - the user can't reliably
        // screenshot the F3 peak line while the FPS is bouncing.
        let mut stream_us_frame: u64 = 0;
        let mut light_us_frame: u64 = 0;
        // Wave 7: fire on_tick for every loaded plugin BEFORE
        // anything else in the frame so plugins see a stable
        // world state (no edits applied mid-tick). day_time
        // mirrors the day/night cycle if active.
        let day_time = (vx.app_start.elapsed().as_secs_f32()) % 1200.0;
        plugin::events::dispatch_tick(vx, dt, day_time);
        // FPS counter: heavier new-frame weight so the reading
        // tracks reality under load. The old 0.9/0.1 EMA was so
        // sticky that a freshly-stalled game still showed the
        // previous steady-state value for seconds. Clamp the
        // displayed value to a sensible range too.
        let inst = (1.0 / dt.max(1e-3)).clamp(1.0, 999.0);
        vx.fps_ema = vx.fps_ema * 0.5 + inst * 0.5;

        // W75: lightweight per-frame perf diagnostic. Accumulates the
        // GPU render-pass cost and, every ~600 frames, logs the queue
        // depths + average render_ms so a frame-rate collapse can be
        // pinned to remesh / stream churn (remesh_q), lighting
        // (light_jobs), the falling queue, streaming (chunks), or the
        // render pass (render_ms). If all are low but fps is low the
        // cost is in the per-frame sim ticks. Cheap + logged sparsely.
        use std::sync::atomic::{AtomicU64, Ordering as DiagOrd};
        static DIAG_N: AtomicU64 = AtomicU64::new(0);
        static DIAG_REND_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_ACC_US: AtomicU64 = AtomicU64::new(0);
        // W103: per-phase sim cost accumulators (microseconds), drained in the
        // same 2.5s window as render_ms. SIM = everything in on_update before
        // the render() call; the named phases are the prime suspects.
        static DIAG_SIM_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_UPDATE_PLAY_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_STREAM_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_MOBS_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_LIQUIDS_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_DRAWQ_US: AtomicU64 = AtomicU64::new(0);
        // W198.2: liquid mesh rebuild, the untimed sim_ms gap suspect.
        static DIAG_LIQMESH_US: AtomicU64 = AtomicU64::new(0);
        // W198.14: lighting dispatch (`tick_lighting`) - clones chunks on
        // the main thread at 10 Hz; a periodic travel-stutter suspect that
        // was previously lumped into `sim`.
        static DIAG_LIGHT_US: AtomicU64 = AtomicU64::new(0);
        // W198.14: per-phase PEAK (worst single frame) via fetch_max. The
        // averages above dilute a periodic spike into nothing (a 25ms hitch
        // 10x/s reads as <0.5ms avg) - peaks are the only way to localize
        // the felt turning/travel stutter to a phase.
        static DIAG_SIM_PEAK_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_STREAM_PEAK_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_LIGHT_PEAK_US: AtomicU64 = AtomicU64::new(0);
        // W198.16: full phase breakdown of the WORST sim frame in the
        // window, captured when a new max is seen. At window flush, if that
        // worst frame crossed the hitch threshold, its breakdown is logged
        // once - a deduped (<=1 per 2.5s) low-FPS hitch record so the user
        // doesn't have to screenshot the bouncing F3 line. All in us.
        static DIAG_WSIM_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_WHEAD_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_WWORLD_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_WMID_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_WRPREP_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_WSTREAM_US: AtomicU64 = AtomicU64::new(0);
        static DIAG_WLIGHT_US: AtomicU64 = AtomicU64::new(0);
        // W95: fire on a ~2.5s WALL-CLOCK interval, not every 600 frames.
        // At 10 FPS a 600-frame gate only logged once per 60s, so a frame
        // collapse produced no diag before the user quit. Now a stall is
        // visible within ~2.5s. `py`/`vy` expose the player Y + vertical
        // velocity so a "falling forever" spawn (which churns chunk
        // streaming -> continuous lighting/remesh) is distinguishable from
        // a stable-but-slow frame or a one-off heavy initial load.
        let frames = DIAG_N.fetch_add(1, DiagOrd::Relaxed) + 1;
        let dt_us = (dt * 1_000_000.0) as u64;
        let acc_us = DIAG_ACC_US.fetch_add(dt_us, DiagOrd::Relaxed) + dt_us;
        if acc_us >= 2_500_000 {
            DIAG_N.store(0, DiagOrd::Relaxed);
            DIAG_ACC_US.store(0, DiagOrd::Relaxed);
            let rend_us = DIAG_REND_US.swap(0, DiagOrd::Relaxed);
            let sim_us = DIAG_SIM_US.swap(0, DiagOrd::Relaxed);
            let up_us = DIAG_UPDATE_PLAY_US.swap(0, DiagOrd::Relaxed);
            let str_us = DIAG_STREAM_US.swap(0, DiagOrd::Relaxed);
            let mob_us = DIAG_MOBS_US.swap(0, DiagOrd::Relaxed);
            let liq_us = DIAG_LIQUIDS_US.swap(0, DiagOrd::Relaxed);
            let dq_us = DIAG_DRAWQ_US.swap(0, DiagOrd::Relaxed);
            let liqm_us = DIAG_LIQMESH_US.swap(0, DiagOrd::Relaxed);
            let light_us = DIAG_LIGHT_US.swap(0, DiagOrd::Relaxed);
            let sim_pk = DIAG_SIM_PEAK_US.swap(0, DiagOrd::Relaxed);
            let str_pk = DIAG_STREAM_PEAK_US.swap(0, DiagOrd::Relaxed);
            let light_pk = DIAG_LIGHT_PEAK_US.swap(0, DiagOrd::Relaxed);
            let f = frames.max(1);
            // W198: also stash the per-phase ms so the F3 overlay can show
            // them (the user screenshots the HUD instead of the console).
            crate::perf::store(&crate::perf::RENDER_MS, (rend_us / f) as f32 / 1000.0);
            crate::perf::store(&crate::perf::SIM_MS, (sim_us / f) as f32 / 1000.0);
            crate::perf::store(&crate::perf::STREAM_MS, (str_us / f) as f32 / 1000.0);
            crate::perf::store(&crate::perf::LIQUIDS_MS, (liq_us / f) as f32 / 1000.0);
            crate::perf::store(&crate::perf::DRAWQ_MS, (dq_us / f) as f32 / 1000.0);
            crate::perf::store(&crate::perf::MOBS_MS, (mob_us / f) as f32 / 1000.0);
            crate::perf::store(&crate::perf::LIQMESH_MS, (liqm_us / f) as f32 / 1000.0);
            crate::perf::store(&crate::perf::LIGHT_MS, (light_us / f) as f32 / 1000.0);
            // W198.14: peaks stored raw (worst single frame), NOT averaged.
            crate::perf::store(&crate::perf::SIM_PEAK_MS, sim_pk as f32 / 1000.0);
            crate::perf::store(&crate::perf::STREAM_PEAK_MS, str_pk as f32 / 1000.0);
            crate::perf::store(&crate::perf::LIGHT_PEAK_MS, light_pk as f32 / 1000.0);
            // W198.16: deduped low-FPS hitch log (<=1 per 2.5s window). If the
            // worst frame this window crossed the hitch line, log its phase
            // breakdown so a stutter is diagnosable from the log alone - no
            // racing to screenshot the bouncing F3 line. world_tick = wworld-
            // whead, pre_render = wmid-wworld, render_prep = wrprep-wmid,
            // drawq_tail = wsim-wrprep; stream/light are discrete.
            const HITCH_SIM_US: u64 = 14_000; // ~below 70 FPS in sim alone
            let wsim = DIAG_WSIM_US.swap(0, DiagOrd::Relaxed);
            let whead = DIAG_WHEAD_US.swap(0, DiagOrd::Relaxed);
            let wworld = DIAG_WWORLD_US.swap(0, DiagOrd::Relaxed);
            let wmid = DIAG_WMID_US.swap(0, DiagOrd::Relaxed);
            let wrprep = DIAG_WRPREP_US.swap(0, DiagOrd::Relaxed);
            let wstream = DIAG_WSTREAM_US.swap(0, DiagOrd::Relaxed);
            let wlight = DIAG_WLIGHT_US.swap(0, DiagOrd::Relaxed);
            if wsim >= HITCH_SIM_US {
                let ms = |u: u64| u as f32 / 1000.0;
                tracing::warn!(
                    worst_sim_ms = ms(wsim),
                    head_ms = ms(whead),
                    world_tick_ms = ms(wworld.saturating_sub(whead)),
                    pre_render_ms = ms(wmid.saturating_sub(wworld)),
                    render_prep_ms = ms(wrprep.saturating_sub(wmid)),
                    drawq_tail_ms = ms(wsim.saturating_sub(wrprep)),
                    stream_ms = ms(wstream),
                    light_ms = ms(wlight),
                    chunks = vx.world.iter_chunks().count(),
                    remesh_q = vx.pending_remesh.len(),
                    light_jobs = vx.pending_lighting_jobs,
                    "W198.16: low-FPS hitch (worst frame this 2.5s window)"
                );
            }
            tracing::info!(
                fps = vx.fps_ema as i32,
                chunks = vx.world.iter_chunks().count(),
                remesh_q = vx.pending_remesh.len(),
                light_jobs = vx.pending_lighting_jobs,
                falling = vx.falling_pending.len(),
                drops = vx.dropped_items.len(),
                py = vx.player_pos.y,
                vy = vx.player_vel.y,
                render_ms = (rend_us / f) as f32 / 1000.0,
                sim_ms = (sim_us / f) as f32 / 1000.0,
                update_play_ms = (up_us / f) as f32 / 1000.0,
                stream_ms = (str_us / f) as f32 / 1000.0,
                mobs_ms = (mob_us / f) as f32 / 1000.0,
                liquids_ms = (liq_us / f) as f32 / 1000.0,
                drawq_ms = (dq_us / f) as f32 / 1000.0,
                liqmesh_ms = (liqm_us / f) as f32 / 1000.0,
                light_ms = (light_us / f) as f32 / 1000.0,
                sim_peak_ms = sim_pk as f32 / 1000.0,
                stream_peak_ms = str_pk as f32 / 1000.0,
                light_peak_ms = light_pk as f32 / 1000.0,
                liq_pos = vx.liquid_positions.len(),
                liq_sched = vx.liquid_schedule.len(),
                // W284: wedge + backlog gauges. `light_dirty` growing
                // while `light_jobs` sits at the in-flight cap = the
                // lighting dispatcher is wedged (leaked counter);
                // `liq_backlog` non-draining = liquid remesh churn.
                liq_backlog = vx.liquid_remesh_backlog.len(),
                light_dirty = vx.lighting.dirty.len(),
                light_segq = vx.pending_lighting_segments.len(),
                "voxel: perf diag"
            );
        }

        // Pull any pending master-query results back onto the main
        // thread (refresh + announce both run on background workers).
        vx.poll_master_queries();

        // Esc behaviour depends on net role:
        //  - Offline: classic Play <-> Paused toggle (whole world
        //    halts while the pause overlay is up).
        //  - Server / Client: world must keep running for the
        //    other peers, so Esc just toggles `mp_menu_open` which
        //    drives an overlay UI; physics / net tick continue.
        // Settings always backs out via Esc regardless of role.
        // W53.1: when egui owns keyboard focus (search box, chat
        // input, MP setup form, settings text fields) the in-world
        // hotkeys must NOT fire. Typing "E" in creative search
        // would otherwise close the panel; typing "T" while in a
        // text field would open chat on top. Single source of
        // truth so every hotkey gate below shares it.
        let egui_kb = vx.ui.as_ref().map(|u| u.wants_keyboard_input()).unwrap_or(false);

        let esc_now = ctx.input.is_pressed(Key::Escape);
        let esc_just = esc_now && !vx.esc_prev;
        vx.esc_prev = esc_now;
        if esc_just {
            let mp = !matches!(vx.net, NetRole::Offline);
            match vx.state {
                AppState::Playing => {
                    // Chat input takes priority - Esc just dismisses
                    // it without opening the MP menu underneath.
                    // The chest UI takes second priority - Esc
                    // closes it before falling through to the
                    // mp-menu / pause path.
                    if vx.chat_open {
                        vx.chat_open = false;
                        vx.chat_input.clear();
                        // W11.5: closing chat hides autocomplete.
                        vx.chat_completion = None;
                    } else if vx.chest_open.is_some() {
                        vx.close_chest_ui();
                    } else if vx.furnace_open.is_some() {
                        vx.close_furnace_ui();
                    } else if vx.crafting_table_open.is_some() {
                        vx.close_crafting_table_ui();
                    } else if vx.brewing_open.is_some() {
                        vx.close_brewing_ui();
                    } else if vx.stonecutter_open.is_some() {
                        vx.close_stonecutter_ui();
                    } else if vx.smithing_open.is_some() {
                        vx.close_smithing_ui();
                    } else if vx.trade_open.is_some() {
                        vx.trade_open = None;
                    } else if vx.creative_tab.open {
                        // W20.3: ESC closes the creative tab (same as E).
                        vx.creative_tab.close();
                    } else if mp {
                        vx.mp_menu_open = !vx.mp_menu_open;
                    } else {
                        vx.state = AppState::Paused;
                    }
                }
                AppState::Paused => vx.state = AppState::Playing,
                AppState::Settings { back_to } => {
                    vx.state = match back_to {
                        BackTo::MainMenu => AppState::MainMenu,
                        BackTo::Pause => AppState::Paused,
                    };
                }
                AppState::MainMenu
                | AppState::DirectConnect
                | AppState::ServerBrowser
                | AppState::HostSetup => {
                    // Wave 4 (Phase G.6): defer to the menu's own
                    // ESC handler. It returns true after popping a
                    // sub-screen back to Main; we don't do
                    // anything else in that case. False means the
                    // user is already on Main, which in the new MC
                    // hierarchy is a no-op (no quit-on-Esc).
                    let _ = vx.menu.on_escape();
                }
                // L.1: ESC during loading is intentionally a no-op
                // (cancel support is a deferred follow-up).
                AppState::Loading { .. } => {}
            }
        }

        // E toggles the inventory overlay while playing. When the
        // overlay is open we release the cursor + freeze player
        // input so the UI is interactable.
        // F3 toggles the debug panel. Mirrors Minecraft's debug
        // overlay: cheap to leave running, off by default.
        let f3_now = ctx.input.is_pressed(Key::F3);
        if f3_now
            && !vx.f3_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
        {
            vx.show_debug = !vx.show_debug;
        }
        vx.f3_prev = f3_now;
        // F1 hides / shows the entire HUD (hotbar, crosshair,
        // health, mob bars). Useful for screenshots + benchmarks.
        let f1_now = ctx.input.is_pressed(Key::F1);
        if f1_now
            && !vx.f1_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
        {
            vx.hud_visible = !vx.hud_visible;
        }
        vx.f1_prev = f1_now;
        // F5 cycles the camera through first-person and two
        // third-person poses (behind, then in front). Disabled
        // while chat is open so accidental F5 mid-typing doesn't
        // disorient the player.
        let f5_now = ctx.input.is_pressed(Key::F5);
        if f5_now
            && !vx.f5_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
        {
            vx.camera_mode = (vx.camera_mode + 1) % 3;
        }
        vx.f5_prev = f5_now;
        // F6 toggles the live debug panel.
        let f6_now = ctx.input.is_pressed(Key::F6);
        if f6_now
            && !vx.f6_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
        {
            vx.show_debug_panel = !vx.show_debug_panel;
        }
        vx.f6_prev = f6_now;
        // F4 cycles weather: clear -> rain -> snow -> clear.
        // Broadcasts when hosting so peers follow the change.
        let f4_now = ctx.input.is_pressed(Key::F4);
        if f4_now
            && !vx.f4_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
        {
            vx.weather = (vx.weather + 1) % 3;
            if let NetRole::Server { server, .. } = &mut vx.net
                && let Ok(buf) = bincode::serde::encode_to_vec(
                    vx.weather,
                    bincode::config::standard(),
                )
            {
                server.broadcast_bytes("vx.weather", buf);
            }
            let label = match vx.weather {
                1 => "Rain",
                2 => "Snow",
                _ => "Clear",
            };
            vx.chat_log.push((
                std::time::Instant::now(),
                format!("Weather: {label}"),
            ));
        }
        vx.f4_prev = f4_now;
        // Q drops one item from the selected hotbar slot. No
        // dropped-entity model yet so the item simply
        // disappears - useful for trimming overstocked stacks.
        let q_now = ctx.input.is_pressed(Key::Q);
        if q_now
            && !vx.q_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
            && !vx.inventory_open
            && !egui_kb
            && vx.selected_slot < HOTBAR_SLOTS
            && let Some((b, c)) = vx.inventory[vx.selected_slot]
        {
            // Ctrl held = drop the entire stack at once.
            let drop_all = ctx.input.is_pressed(Key::ControlLeft)
                || ctx.input.is_pressed(Key::ControlRight);
            let new_count = if drop_all { 0 } else { c.saturating_sub(1) };
            vx.inventory[vx.selected_slot] = if new_count == 0 {
                None
            } else {
                Some((b, new_count))
            };
            // Small puff at the player's feet so the drop is
            // visible; mirrors the break-particle texture so
            // identifying the block stays easy.
            let color = vx
                .registry
                .get(b)
                .map(|d| d.color)
                .unwrap_or([0.8, 0.8, 0.8]);
            let voxel = [
                vx.player_pos.x.floor() as i32,
                (vx.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                vx.player_pos.z.floor() as i32,
            ];
            vx.spawn_break_particles_tinted(voxel, color);
        }
        vx.q_prev = q_now;
        // Z undoes the last placement. Routes through the edit
        // pipeline so the air swap replicates + persists. Gated
        // on chat / inventory so accidental Z while typing
        // doesn't trash builds.
        let z_now = ctx.input.is_pressed(Key::Z);
        if z_now
            && !vx.z_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
            && !vx.inventory_open
            && !egui_kb
            && let Some(voxel) = vx.place_history.pop()
        {
            let current = vx.world.get(voxel);
            if current != BlockId::AIR {
                let edit = EditMsg {
                    x: voxel[0],
                    y: voxel[1],
                    z: voxel[2],
                    block: 0,
                };
                vx.world_set_tracked(voxel, BlockId::AIR);
                let coords = Voxel::affected_chunks(voxel);
                vx.rebuild_chunks(&coords);
                let cfg = bincode::config::standard();
                match &mut vx.net {
                    NetRole::Offline => {}
                    NetRole::Server { server, .. } => {
                        vx.edit_history.push(edit);
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(edit, cfg)
                        {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                    NetRole::Client { pending_edits, .. } => {
                        pending_edits.push(edit);
                    }
                }
                // Refund: pickup() handles stack merging so the
                // undone block returns to the inventory.
                vx.pickup(current);
            }
        }
        vx.z_prev = z_now;
        // G toggles the chunk-grid overlay. Cheap debug aid;
        // only fires when F3 debug is also showing to avoid
        // cluttering normal play.
        let g_now = ctx.input.is_pressed(Key::G);
        if g_now
            && !vx.g_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
            && !egui_kb
        {
            vx.show_chunk_grid = !vx.show_chunk_grid;
        }
        vx.g_prev = g_now;
        // H cycles the targeting wireframe between Auto, Build and
        // Break. Drives the per-frame highlight render decision and
        // is persisted per-player in PlayerSave so the choice
        // survives /save + relaunch.
        let h_now = ctx.input.is_pressed(Key::H);
        if h_now
            && !vx.h_prev
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
            && !egui_kb
        {
            vx.highlight_mode = vx.highlight_mode.cycle();
            let label = vx.highlight_mode.label();
            vx.chat_log.push((
                std::time::Instant::now(),
                format!("Highlight mode: {label}"),
            ));
        }
        vx.h_prev = h_now;
        // Tab held = playerlist overlay. Hidden in singleplayer
        // (the overlay drawer also guards on net role, but the
        // flag is gated here too so debug panels stay clean).
        vx.tab_held =
            matches!(vx.state, AppState::Playing) && ctx.input.is_pressed(Key::Tab);
        // 1..9 select the matching hotbar slot directly (Minecraft
        // convention). Digit 1 maps to slot 0 so the user-facing
        // numbering matches the visual order in the hotbar.
        if matches!(vx.state, AppState::Playing) && !vx.chat_open && !egui_kb {
            let digits = [
                Key::Digit1,
                Key::Digit2,
                Key::Digit3,
                Key::Digit4,
                Key::Digit5,
                Key::Digit6,
                Key::Digit7,
                Key::Digit8,
                Key::Digit9,
            ];
            for (i, key) in digits.iter().enumerate() {
                if ctx.input.is_pressed(*key) && i < HOTBAR_SLOTS {
                    vx.selected_slot = i;
                    break;
                }
            }
        }

        let e_now = ctx.input.is_pressed(Key::E);
        let e_just = e_now && !vx.e_prev;
        vx.e_prev = e_now;
        if e_just
            && matches!(vx.state, AppState::Playing)
            && !vx.chat_open
            && !egui_kb
        {
            // Pressing E while a chest / furnace UI is open closes
            // it first instead of stacking a second panel on top.
            if vx.chest_open.is_some() {
                vx.close_chest_ui();
            } else if vx.furnace_open.is_some() {
                vx.close_furnace_ui();
            } else if vx.creative_tab.open {
                // W10.6: E closes the creative tab when it's open.
                vx.creative_tab.close();
            } else if vx.abilities.mayfly {
                // W10.6: Creative-mode E opens the searchable
                // creative tab instead of the survival
                // inventory. `mayfly` is the canonical
                // "creative-ish abilities active" proxy (today
                // Creative is the only mode with mayfly=true).
                // Close the survival inventory if it happened
                // to be open from a mode switch.
                vx.inventory_open = false;
                vx.creative_tab.open_now();
            } else {
                vx.inventory_open = !vx.inventory_open;
            }
            // Drop any queued mouse-button edits the click that
            // closes the inventory would otherwise be replayed
            // against the world.
            vx.lmb_pending = false;
            vx.rmb_pending = false;
        }

        // T opens the chat input. Only fires in MP roles + while
        // no other overlay is up. Esc + Enter both close it.
        let t_now = ctx.input.is_pressed(Key::T);
        let t_just = t_now && !vx.t_prev;
        vx.t_prev = t_now;
        if t_just
            && matches!(vx.state, AppState::Playing)
            && !vx.inventory_open
            && !vx.mp_menu_open
            && !vx.chat_open
            && !egui_kb
        {
            // Chat opens in any net role (including Offline) so
            // local slash commands like /give work in
            // singleplayer too.
            vx.chat_open = true;
            vx.chat_input.clear();
        }

        // F = eat (HOLD, W370). MC ItemUseAnimation: food takes 1.6 s
        // of held use - chew sounds pace the hold, the item consumes
        // + heals ON COMPLETION, releasing F cancels with nothing
        // spent. (Direct HP heal stands in until the hunger system
        // lands - tracked in the parity matrix.)
        let f_now = ctx.input.is_pressed(Key::F);
        let f_just = f_now && !vx.f_prev;
        vx.f_prev = f_now;
        let eat_ctx_ok = matches!(vx.state, AppState::Playing)
            && !vx.inventory_open
            && !vx.mp_menu_open
            && !vx.chat_open
            && !egui_kb;
        if f_just
            && eat_ctx_ok
            && vx.eating.is_none()
            && vx.player_health < PLAYER_MAX_HEALTH
            && vx
                .inventory
                .iter()
                .any(|s| matches!(s, Some((b, _)) if *b == BlockId(12)))
        {
            vx.eating = Some((12, 0.0));
        }
        if let Some((food, t0)) = vx.eating {
            const EAT_DURATION: f32 = 1.6;
            const CHEW_PERIOD: f32 = 0.28;
            if !f_now || !eat_ctx_ok {
                vx.eating = None;
            } else {
                let t1 = t0 + dt;
                if (t1 / CHEW_PERIOD) as u32 != (t0 / CHEW_PERIOD) as u32 {
                    let pos = [vx.player_pos.x, vx.player_pos.y, vx.player_pos.z];
                    vx.sfx_at("eat", pos, |a, g| {
                        let _ = a.play_thud(280.0, 0.15 * g);
                    });
                }
                if t1 >= EAT_DURATION {
                    // Consume + heal; the slot may have vanished
                    // mid-chew (drop, MP take) - then nothing happens.
                    let mut ate = false;
                    for slot in vx.inventory.iter_mut() {
                        if let Some((b, c)) = slot.as_mut()
                            && *b == BlockId(food)
                        {
                            *c = c.saturating_sub(1);
                            if *c == 0 {
                                *slot = None;
                            }
                            ate = true;
                            break;
                        }
                    }
                    if ate {
                        vx.player_health =
                            (vx.player_health + 4.0).min(PLAYER_MAX_HEALTH);
                        let pos =
                            [vx.player_pos.x, vx.player_pos.y, vx.player_pos.z];
                        // MC finishes a meal with the burp.
                        vx.sfx_at("burp", pos, |a, g| {
                            let _ = a.play_thud(180.0, 0.2 * g);
                        });
                    }
                    vx.eating = None;
                } else {
                    vx.eating = Some((food, t1));
                }
            }
        }

        if matches!(vx.state, AppState::Playing) && vx.world_loaded {
            // update_play itself gates player input on
            // `vx.inventory_open` - we still need to tick it every
            // frame so gravity + collision + net pose broadcasts
            // keep running while the menu is up.
            //
            // W8.1: also gated on `world_loaded` so the simulation
            // path can't run while the user is on the main menu.
            // W198.9: cumulative time up to here = the frame head (plugin
            // tick, master-query drain, ESC/input). Splits the untimed gap.
            crate::perf::store(
                &crate::perf::CHK_HEAD_MS,
                _frame_start.elapsed().as_secs_f32() * 1000.0,
            );
            let _t = std::time::Instant::now();
            vx.update_play(ctx, dt);
            DIAG_UPDATE_PLAY_US.fetch_add(_t.elapsed().as_micros() as u64, DiagOrd::Relaxed);
            // W123: underwater / cave / music ambience timers.
            crate::audio_ambience::tick(vx, dt);
            let _t = std::time::Instant::now();
            vx.stream_chunks();
            let stream_us = _t.elapsed().as_micros() as u64;
            stream_us_frame = stream_us;
            DIAG_STREAM_US.fetch_add(stream_us, DiagOrd::Relaxed);
            DIAG_STREAM_PEAK_US.fetch_max(stream_us, DiagOrd::Relaxed);
            // W134: minecart simulation + rider pin + host snapshot
            // stream. Runs after update_play (which zeroed the walk
            // velocity while riding) and before the targeting ray
            // below so the camera follows the fresh cart pose.
            let _t = std::time::Instant::now();
            crate::minecart::tick_all(vx, dt);
            // W140: arrow ballistics + hits + host snapshot stream.
            crate::projectile::tick_arrows(vx, dt);
            // W337: ghast fireballs (impact = power-1 explosion).
            crate::projectile::tick_fireballs(vx, dt);
            // W338: primed TNT fuses.
            crate::tnt::tick(vx, dt);
            let _ms = _t.elapsed().as_secs_f32() * 1000.0;
            if _ms > 3.0 {
                tracing::warn!(ms = _ms, phase = "minecart+arrows", "W230: slow world-tick phase");
            }
        } else {
            // Paused / in a menu screen: drop captured mouse motion
            // so the camera doesn't lurch when play resumes.
            vx.mouse_dx = 0.0;
            vx.mouse_dy = 0.0;
            vx.wheel_accum = 0.0;
        }
        vx.apply_cursor_settings(window);

        // Build UI state first so the immediate-mode pass mutates vx
        // freely. The renderer is borrowed afterwards once UI state has
        // settled.
        let _ui_t = std::time::Instant::now();
        vx.render_ui(window);
        crate::perf::store(&crate::perf::UI_MS, _ui_t.elapsed().as_secs_f32() * 1000.0);
        {
            let _ms = _ui_t.elapsed().as_secs_f32() * 1000.0;
            if _ms > 3.0 {
                tracing::warn!(ms = _ms, phase = "render_ui", "W230: slow world-tick phase");
            }
        }
        // W230: everything from here to tick_net (hotkeys, targeting,
        // place/break input) shares one coarse probe.
        let _wt_input_tail_t = std::time::Instant::now();

        let forward = Vec3::new(
            vx.player_yaw.cos() * vx.player_pitch.cos(),
            vx.player_pitch.sin(),
            -vx.player_yaw.sin() * vx.player_pitch.cos(),
        );

        // Cast a ray from the eye along the look direction so the
        // crosshair / wireframe / edit code can target the voxel the
        // player is currently aiming at. Pass-through decorations
        // (torch, flowers) are skipped so the ray reaches whatever
        // solid block lives behind them - placing a block adjacent
        // to a torch no longer fails when the crosshair clips the
        // torch's full-voxel hitbox.
        vx.target = if matches!(vx.state, AppState::Playing) {
            // W132: thin-shape aware - a flat rail only blocks the
            // ray inside its real outline box, like MC.
            crate::world::targeting::raycast_target(
                &vx.world,
                vx.player_pos,
                forward,
                6.0,
                |b| !b.is_air() && !block_is_ray_passable(b),
                // W347: snow layers pick with their real height
                // (layers/8). W362: doors pick with their 3/16
                // panel - an OPEN door lets the ray through the
                // doorway to the block behind (clicking it no
                // longer toggles the door), while the panel slab
                // itself stays clickable to close it.
                |p, b| {
                    if b.0 == 15 {
                        let h = vx.snow_positions.get(&p).copied().unwrap_or(1)
                            as f32
                            / 8.0;
                        return Some(([0.0, 0.0, 0.0], [1.0, h, 1.0]));
                    }
                    if crate::block::door::is_door_block(b.0) {
                        let below = [p[0], p[1] - 1, p[2]];
                        let lower = if vx.door_facing.contains_key(&p)
                            || !vx.door_facing.contains_key(&below)
                        {
                            p
                        } else {
                            below
                        };
                        let facing =
                            vx.door_facing.get(&lower).copied().unwrap_or(0);
                        let edge = if vx.door_open.contains(&lower) {
                            crate::block::door::open_edge(facing)
                        } else {
                            crate::block::door::closed_edge(facing)
                        };
                        return Some(crate::block::door::panel_box(edge));
                    }
                    None
                },
            )
        } else {
            None
        };
        // Separate ray for mining: stops on the FIRST non-air,
        // non-liquid voxel - decorations included. The
        // placement-target ray tunnels through `F_RAY_PASS`
        // blocks so the player can drop blocks next to a torch
        // without selecting it; this one stops at torches so
        // they can be mined. Liquids stay invisible to both
        // rays - they're not directly breakable (the bucket
        // scoop handles water/lava source pickup), and
        // wireframe-highlighting a falling stream would just
        // confuse the player.
        vx.mine_target = if matches!(vx.state, AppState::Playing) {
            // W132: same thin-shape awareness for the mining ray.
            crate::world::targeting::raycast_target(
                &vx.world,
                vx.player_pos,
                forward,
                6.0,
                |b| !b.is_air() && !block_is_liquid(b),
                // W362: same snow + door panel boxes as the aim
                // ray so an open door is minable only through its
                // panel.
                |p, b| {
                    if b.0 == 15 {
                        let h = vx.snow_positions.get(&p).copied().unwrap_or(1)
                            as f32
                            / 8.0;
                        return Some(([0.0, 0.0, 0.0], [1.0, h, 1.0]));
                    }
                    if crate::block::door::is_door_block(b.0) {
                        let below = [p[0], p[1] - 1, p[2]];
                        let lower = if vx.door_facing.contains_key(&p)
                            || !vx.door_facing.contains_key(&below)
                        {
                            p
                        } else {
                            below
                        };
                        let facing =
                            vx.door_facing.get(&lower).copied().unwrap_or(0);
                        let edge = if vx.door_open.contains(&lower) {
                            crate::block::door::open_edge(facing)
                        } else {
                            crate::block::door::closed_edge(facing)
                        };
                        return Some(crate::block::door::panel_box(edge));
                    }
                    None
                },
            )
        } else {
            None
        };

        // Apply pending mouse-button edits before rendering. LMB
        // always breaks the targeted voxel. RMB places the selected
        // hotbar block at the adjacent empty cell, but is ignored
        // when the selected slot is empty or holds a tool/weapon.
        let lmb = vx.lmb_pending;
        let rmb = vx.rmb_pending;
        vx.lmb_pending = false;
        vx.rmb_pending = false;
        // W30 fix: lmb_just_pressed is consumed by the creative insta-
        // break path further down. Stale "true" with no target voxel
        // is harmless since the next Pressed event would set it to
        // true again (idempotent).
        // The currently-held block: contents of the selected hotbar
        // slot. `None` = empty hand (or 0-count remnant).
        let selected_block = vx.inventory[vx.selected_slot].map(|(b, _)| b);
        // Net mode flag: clients defer block edits to the server +
        // wait for the server echo before applying. Offline + server
        // apply locally as before.
        let is_client = matches!(vx.net, NetRole::Client { .. });
        // Mob attack interception: if LMB and a mob is closer than
        // the targeted voxel (or there's no voxel target), route
        // the click as a melee strike instead of a block break.
        let combat_gate = lmb && !vx.any_panel_open();
        let mob_target = if combat_gate {
            vx.raycast_mob(vx.player_pos, forward, MOB_ATTACK_REACH)
        } else {
            None
        };
        let pvp_target = if combat_gate {
            vx.raycast_remote_player(vx.player_pos, forward, PVP_REACH)
        } else {
            None
        };
        // W395: a SEPARATE mob pick for RMB. The trade + mount
        // blocks below read `attack_mob`, but the combat gate only
        // fills it on LMB - so right-click never saw the villager
        // or the horse (the R23 "can't click the villager" report).
        // apply_combat keeps consuming only `attack_mob`, so a
        // right click still never strikes.
        let interact_mob: Option<u32> = if rmb && !vx.any_panel_open() {
            vx.raycast_mob(vx.player_pos, forward, MOB_ATTACK_REACH)
                .map(|(id, _)| id)
        } else {
            None
        };
        // W100: compare the mob hit against the MINING ray (`mine_target`,
        // which stops at the first breakable block), not the placement ray
        // (`target`, which passes through decorations). Using the placement
        // ray let a block behind the mob look "closer", so an attack also
        // dug the terrain behind the mob. The mining ray is what would
        // actually break, so it's the correct distance to arbitrate against.
        let voxel_dist = vx.mine_target.map(|h| {
            let c = Vec3::new(
                h.voxel[0] as f32 + 0.5,
                h.voxel[1] as f32 + 0.5,
                h.voxel[2] as f32 + 0.5,
            );
            (c - vx.player_pos).length()
        });
        // W47.13a: arbitration moved to world::place_break::
        // resolve_attack_target. Pure-data helper picks the closest
        // of (pvp, mob, voxel) and returns at most one combat target.
        let crate::world::place_break::AttackTarget {
            player: attack_player,
            mob: attack_mob,
        } = crate::world::place_break::resolve_attack_target(
            pvp_target,
            mob_target,
            voxel_dist,
        );
        // W134: LMB on a minecart pops it back to the item (MC's
        // instant cart break) when no mob/player stands closer and
        // the cart is nearer than the targeted voxel.
        let mut cart_broken = false;
        if combat_gate
            && attack_mob.is_none()
            && attack_player.is_none()
            && let Some((cart_id, cart_d)) = crate::minecart::raycast_cart(
                &vx.carts,
                vx.player_pos,
                forward,
                4.0,
            )
            && voxel_dist.is_none_or(|vd| cart_d < vd)
        {
            crate::minecart::break_cart(vx, cart_id);
            cart_broken = true;
        }
        // W47.13k: stamina drain + PvP/mob attack dispatch
        // (host applies directly, client sends a hit request).
        crate::world::place_break::apply_combat(
            vx, attack_player, attack_mob, is_client,
        );
        // Mining: LMB held + a voxel in view, but no mob in front
        // (a melee swing takes priority). Tick the active session
        // toward `total`; on completion, set `mine_break` so the
        // block-edit path below executes the break exactly once.
        //
        // W10.2: `may_build = false` (future Adventure mode)
        // suppresses the mining path entirely. Today every mode
        // ships with `may_build = true`; this is a forward hook.
        let lmb_active = vx.lmb_held
            && !vx.any_panel_open()
            && !cart_broken
            && attack_mob.is_none()
            && attack_player.is_none()
            && vx.abilities.may_build;
        // W47.13j: mining damage tick (creative insta-break path +
        // session start/restart + step + crack SFX + dust puff +
        // completion -> mine_break). Lives in world::place_break.
        let mine_break = crate::world::place_break::tick_mining(
            vx, lmb_active, dt,
        );
        // Suppress the legacy single-click break: mining now drives
        // it via `mine_break`. Keep `lmb` consumed so a fresh
        // press doesn't bleed through somewhere else.
        let _ = lmb;
        // Block break: fires once when `mine_break` is set (i.e.
        // the hold-to-mine session completed). The path applies
        // the world edit, broadcasts, credits the pickup, and
        // plays the break SFX.
        if !vx.inventory_open
            && !vx.mp_menu_open
            && let Some(voxel) = mine_break
        {
            // W47.13d: full break pipeline (world set + chunk rebuild
            // + particles + edit broadcast + drops + container drain
            // + dislodge + SFX) lives in world::place_break.
            crate::world::place_break::apply_block_break(vx, voxel, is_client);
        }
        // Empty-bucket scoop: runs BEFORE the placement-target
        // chain because `vx.target` is the placement ray which
        // tunnels through `F_RAY_PASS` liquids - if the player
        // points an empty bucket at a water source with sky
        // behind it, `vx.target` is `None` and the bucket
        // branch inside the chain below would never fire. This
        // path runs its own purpose-built raycast that DOES
        // stop at liquid voxels.
        let bucket_scooped = if !vx.any_panel_open()
            && attack_mob.is_none()
            && rmb
            && let Some(held) = selected_block
            && held.0 == BUCKET_EMPTY_ID
        {
            vx.try_scoop_with_bucket(forward)
        } else {
            false
        };
        // W66.2: RMB on a breedable animal while holding breeding food
        // feeds it (sets love + consumes one food), taking priority
        // over the block-place cascade below. Host / offline only -
        // a pure client lets the server echo drive the love timer.
        let mut fed = false;
        if !is_client
            && rmb
            && !vx.any_panel_open()
            && selected_block
                .is_some_and(|b| crate::mob::breed::is_breeding_food(b.0))
            && let Some((mob_id, _)) =
                vx.raycast_mob(vx.player_pos, forward, MOB_ATTACK_REACH)
        {
            // W290: per-species food check happens inside try_feed.
            let held = selected_block.map(|b| b.0).unwrap_or(0);
            fed = crate::mob::breed::try_feed(vx, mob_id, held);
            if fed {
                if !vx.abilities.instabuild
                    && let Some((_, count)) =
                        vx.inventory[vx.selected_slot].as_mut()
                {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        vx.inventory[vx.selected_slot] = None;
                    }
                }
                vx.rmb_pending = false;
            }
        }
        // W134: RMB on a cart mounts it (priority over the block
        // cascade, mirroring the feed branch above). Shift hops
        // back off; host/offline only - client riding is part of
        // the MP cart follow-up.
        let mut cart_mounted = false;
        if rmb
            && !fed
            && !bucket_scooped
            && !vx.any_panel_open()
            && vx.riding_cart.is_none()
            && !matches!(vx.net, NetRole::Client { .. })
            && let Some((cart_id, _)) = crate::minecart::raycast_cart(
                &vx.carts,
                vx.player_pos,
                forward,
                3.5,
            )
        {
            crate::minecart::mount(vx, cart_id);
            cart_mounted = true;
        }
        // W348: RMB on an adult horse/donkey mounts it (MC
        // doPlayerRide - the rider's rotation snaps to the mount).
        // Host/offline only, like the cart; client riding is the
        // same MP follow-up.
        if rmb
            && !fed
            && !bucket_scooped
            && !cart_mounted
            && !vx.any_panel_open()
            && vx.riding_mob.is_none()
            && vx.riding_cart.is_none()
            && !matches!(vx.net, NetRole::Client { .. })
            && let Some(mid) = attack_mob.or(interact_mob)
            && let Some(h) = vx.mobs.iter().find(|m| {
                m.id == mid
                    && crate::mob::riding::is_rideable(m.kind)
                    && m.baby_left <= 0.0
                    && m.dying <= 0.0
            })
        {
            vx.player_yaw = h.yaw;
            vx.riding_mob = Some(mid);
            vx.riding_wish = [0.0, 0.0];
            vx.riding_jump = false;
            vx.rmb_pending = false;
        }
        // W319: RMB on a villager opens the trade panel (MC
        // interact). Runs before the block cascade so aiming at a
        // villager never places a block through them.
        let mut trade_opened = false;
        if rmb
            && !vx.any_panel_open()
            && let Some(mid) = attack_mob.or(interact_mob)
            && vx.mobs.iter().any(|m| {
                m.id == mid && m.kind == crate::mob::kinds::KIND_VILLAGER
            })
        {
            vx.trade_open = Some(mid);
            trade_opened = true;
            // W341: claim the profession from the nearest
            // workstation once, then keep it for the session.
            if !vx.mob_professions.contains_key(&mid)
                && let Some(mp) =
                    vx.mobs.iter().find(|m| m.id == mid).map(|m| m.pos)
            {
                let prof =
                    crate::trades::profession_near(&vx.world, mp, mid);
                vx.mob_professions.insert(mid, prof);
            }
        }
        // Block place: RMB single click stays instant; no mining.
        if !fed
            && !bucket_scooped
            && !cart_mounted
            && !trade_opened
            && !vx.any_panel_open()
            && attack_mob.is_none()
            && let Some(hit) = vx.target
        {
            // W47.13c: arbitration of the RMB cascade lives in
            // world::place_break::classify_rmb_action. Each branch
            // below pattern-matches on the predicted action instead
            // of re-deriving the (target id, held id, tier, tool,
            // may_build) conditions inline.
            let predicted = {
                let held = selected_block.unwrap_or(BlockId::AIR);
                crate::world::place_break::classify_rmb_action(
                    rmb,
                    vx.world.get(hit.voxel).0,
                    selected_block.map(|b| b.0),
                    shovel_tier(held),
                    is_tool_block(held),
                    vx.abilities.may_build,
                )
            };
            use crate::world::place_break::RmbAction;
            if false {
                // Sentinel arm so the existing `else if ...`
                // structure stays a single `if/else if` chain.
                unreachable!()
            } else if predicted == RmbAction::PathGrass {
                // W47.13f: shovel-RMB on grass -> dirt local edit +
                // host broadcast / client vx.path request.
                crate::world::place_break::apply_shovel_grass_path(
                    vx, hit.voxel,
                );
            } else if predicted == RmbAction::TillFarmland {
                // W294: hoe-RMB on dirt/grass -> farmland.
                crate::world::place_break::apply_hoe_till(vx, hit.voxel);
            } else if predicted == RmbAction::ApplyBonemeal {
                // W414: bone meal growth.
                crate::world::place_break::apply_bonemeal(vx, hit.voxel);
            } else if predicted == RmbAction::OpenFurnace {
                // W47.13i: furnace open + mint + client notify.
                crate::world::place_break::open_furnace(vx, hit.voxel);
            } else if predicted == RmbAction::OpenChest {
                // W47.13i: chest open + mint + client notify.
                crate::world::place_break::open_chest(vx, hit.voxel);
            } else if predicted == RmbAction::OpenCraftingTable {
                // W50.3: crafting table open (client-side 3x3 grid).
                crate::world::place_break::open_crafting_table(
                    vx, hit.voxel,
                );
            } else if predicted == RmbAction::OpenBrewingStand {
                // W50.5: brewing stand open.
                crate::world::place_break::open_brewing_stand(
                    vx, hit.voxel,
                );
            } else if predicted == RmbAction::PlayNoteBlock {
                // W164: note block - cycle pitch + play the note.
                crate::world::place_break::play_note_block(vx, hit.voxel);
            } else if predicted == RmbAction::OpenStonecutter {
                // W171: stonecutter - open the cut-select UI.
                crate::world::place_break::open_stonecutter(vx, hit.voxel);
            } else if predicted == RmbAction::OpenSmithingTable {
                // W410: smithing table - open the upgrade UI.
                crate::world::place_break::open_smithing_table(vx, hit.voxel);
            } else if predicted == RmbAction::ToggleDoor {
                // W61.2 / W61.3: toggle door + trapdoor open
                // state. The render path swaps the model on the
                // next frame; AABB / ray-pass flips so the
                // player walks through.
                let target_id = vx.world.get(hit.voxel).0;
                let world_ptr: *const infengine_voxel::World = &vx.world;
                // SAFETY: read-only borrow for the duration of
                // the toggle; vx.door_open is the only mutable
                // field touched.
                let now_open = unsafe {
                    crate::block::door::toggle_door(
                        &mut vx.door_open, &*world_ptr, hit.voxel,
                    )
                };
                vx.lmb_pending = false;
                vx.rmb_pending = false;
                // W125: real MC open/close sounds per material.
                let is_trap = crate::block::door::is_trapdoor_block(target_id);
                let is_iron = target_id == 79 || target_id == 549;
                let key = match (is_trap, is_iron, now_open) {
                    (false, false, true) => "door/wood/open",
                    (false, false, false) => "door/wood/close",
                    (false, true, true) => "door/iron/open",
                    (false, true, false) => "door/iron/close",
                    (true, false, true) => "trapdoor/wood/open",
                    (true, false, false) => "trapdoor/wood/close",
                    (true, true, true) => "trapdoor/iron/open",
                    (true, true, false) => "trapdoor/iron/close",
                };
                let spos = [
                    hit.voxel[0] as f32 + 0.5,
                    hit.voxel[1] as f32 + 0.5,
                    hit.voxel[2] as f32 + 0.5,
                ];
                vx.sfx_at(key, spos, |_, _| {});
                // W125: broadcast the toggle so every peer flips the
                // same door (state is keyed by the LOWER half).
                let lower =
                    crate::block::door::lower_pos(&vx.world, hit.voxel);
                let facing =
                    vx.door_facing.get(&lower).copied().unwrap_or(0);
                let payload: ([i32; 3], u8, bool) =
                    (lower, facing, now_open);
                match &mut vx.net {
                    NetRole::Offline => {}
                    NetRole::Server { server, .. } => {
                        let cfg = bincode::config::standard();
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(payload, cfg)
                        {
                            server.broadcast_bytes("vx.door_state", buf);
                        }
                    }
                    NetRole::Client { client, .. } => {
                        let cfg = bincode::config::standard();
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(payload, cfg)
                        {
                            let _ =
                                client.send_bytes("vx.door_state", buf);
                        }
                    }
                }
                // Force a remesh so the door model flips immediately.
                // Include the upper half's chunk (the door can span a
                // chunk boundary vertically).
                let cs = infengine_voxel::CHUNK_SIZE as i32;
                let upper = [lower[0], lower[1] + 1, lower[2]];
                let mut ccs = vec![[
                    lower[0].div_euclid(cs),
                    lower[1].div_euclid(cs),
                    lower[2].div_euclid(cs),
                ]];
                let ucc = [
                    upper[0].div_euclid(cs),
                    upper[1].div_euclid(cs),
                    upper[2].div_euclid(cs),
                ];
                if !ccs.contains(&ucc) {
                    ccs.push(ucc);
                }
                vx.rebuild_chunks(&ccs);
            } else if predicted == RmbAction::ToggleLever {
                // W67.1: flip the lever + re-solve its redstone net.
                crate::world::redstone::toggle_lever(vx, hit.voxel);
                vx.lmb_pending = false;
                vx.rmb_pending = false;
            } else if predicted == RmbAction::SleepBed {
                // W47.13i: bed sleep (day-phase gated; host bumps
                // clock, client asks host).
                crate::world::place_break::try_sleep_bed(vx);
            } else if predicted == RmbAction::IgniteTnt {
                // W338: prime the clicked TNT block.
                crate::tnt::ignite_block(vx, hit.voxel);
            } else if predicted == RmbAction::IgnitePortal {
                // W47.13g: flint+steel ignite (portal frame or fire
                // drop) + durability consume + pending flag clear.
                crate::world::place_break::apply_flint_and_steel_ignite(
                    vx,
                    hit.place_at,
                );
            } else if predicted == RmbAction::BucketUse
                && let Some(held) = selected_block
            {
                // W47.13h: bucket scoop / pour pipeline (scoop
                // raycast + source-only gate + creative-mode skips
                // + edit broadcast + liquid level reset + splash
                // SFX) lives in world::place_break::apply_bucket_use.
                crate::world::place_break::apply_bucket_use(
                    vx, &hit, held, forward,
                );
            } else if predicted == RmbAction::PlaceMinecart {
                // W134: drop a cart entity onto the targeted rail
                // (consumes one minecart item outside creative).
                crate::minecart::place_cart_from_click(vx, hit.voxel);
            } else if let RmbAction::SpawnEgg(kind) = predicted {
                // W145: spawn the egg's mob on the clicked surface
                // + consume one egg outside creative. Host/offline
                // only - the server owns mob state.
                if !matches!(vx.net, crate::net::NetRole::Client { .. }) {
                    let p = [
                        hit.place_at[0] as f32 + 0.5,
                        hit.place_at[1] as f32,
                        hit.place_at[2] as f32 + 0.5,
                    ];
                    crate::mob::spawn::spawn_from_egg(vx, kind, p);
                    if !vx.abilities.instabuild
                        && let Some((_, count)) =
                            vx.inventory[vx.selected_slot].as_mut()
                    {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            vx.inventory[vx.selected_slot] = None;
                        }
                    }
                }
            } else if predicted == RmbAction::PlaceBlock
                && let Some(block) = selected_block
            {
                // W47.13e: full place pipeline (target resolve +
                // overlap / support / occupied gates + torch wall
                // swap + edit broadcast + container mint + stack
                // decrement + SFX + particles) lives in
                // world::place_break::apply_block_place.
                crate::world::place_break::apply_block_place(
                    vx, &hit, block, is_client,
                );
            }
        }
        {
            let _ms = _wt_input_tail_t.elapsed().as_secs_f32() * 1000.0;
            if _ms > 3.0 {
                tracing::warn!(ms = _ms, phase = "input_tail", "W230: slow world-tick phase");
            }
        }
        // Net tick: poll + apply + broadcast / send. Always runs
        // so a CLI-started server can accept connects even before
        // a player picks a world; the heavier world simulation
        // gates on `world_loaded` below.
        {
            let _t = std::time::Instant::now();
            vx.tick_net();
            let _ms = _t.elapsed().as_secs_f32() * 1000.0;
            if _ms > 3.0 {
                tracing::warn!(ms = _ms, phase = "net", "W230: slow world-tick phase");
            }
        }
        // W8.1: every tick below this point touches the world -
        // chunks, mobs, liquids, lighting, day clock. Skip them
        // entirely while the menu is up so the bootstrap stage
        // doesn't burn CPU on phantom simulation. The renderer
        // still needs a clear colour set each frame so the egui
        // pass draws over a stable backdrop. W9 will replace this
        // with a MC-style panorama; for now we pin a neutral dark
        // navy so the menu reads cleanly.
        if !vx.world_loaded {
            // L.4: if we are in the Loading state, drive the async
            // world bootstrap each frame: tick the worker pool via
            // stream_chunks, update the loaded_chunks counter, and
            // transition to Playing once all chunks are in.
            if matches!(vx.state, AppState::Loading { .. }) {
                // stream_chunks dispatches pending coords to workers
                // and drains finished results into vx.world each
                // frame - this is the same path used during normal
                // gameplay streaming.
                let _t = std::time::Instant::now();
                vx.stream_chunks();
                DIAG_STREAM_US.fetch_add(_t.elapsed().as_micros() as u64, DiagOrd::Relaxed);
                // Count how many chunks the world holds now.
                let live = vx.world.iter_chunks().count() as u32;
                // Update the counter. Extract target + phase as Copy
                // values so we can call &mut vx methods below
                // without holding a borrow into vx.state.
                let (target, phase) =
                    if let AppState::Loading {
                        target_chunks,
                        ref mut loaded_chunks,
                        phase,
                        ..
                    } = vx.state
                    {
                        *loaded_chunks = live;
                        (target_chunks, phase)
                    } else {
                        // matches!() guard above means this is
                        // unreachable, but the else branch
                        // satisfies the compiler.
                        unreachable!()
                    };
                // Once all chunks are in, dispatch lighting + start meshes.
                if live >= target && phase == LoadingPhase::GeneratingChunks {
                    // Phase: BuildingLighting.
                    if let AppState::Loading { ref mut phase, .. } = vx.state {
                        *phase = LoadingPhase::BuildingLighting;
                    }
                    // W27.4: dispatch lighting BFS to worker pool so
                    // the 2-minute main-thread freeze is replaced by a
                    // sub-second worker BFS. Without workers (headless
                    // tests) this runs synchronously as before.
                    vx.rebuild_all_lighting();
                    // W27.4: if workers are running, stay in
                    // BuildingLighting until WorkerResult::Lit arrives
                    // (pending_lighting_jobs > 0). The per-frame tick
                    // below handles the transition once the job lands.
                    // If no workers (synchronous path), lighting is
                    // already done -> fall through to BuildingMeshes now.
                    if vx.pending_lighting_jobs == 0 {
                        // Synchronous path completed: go straight to
                        // BuildingMeshes (same behaviour as pre-W27).
                        if let AppState::Loading { ref mut phase, .. } = vx.state {
                            *phase = LoadingPhase::BuildingMeshes;
                        }
                        let all_coords: Vec<ChunkCoord> =
                            vx.world.iter_chunks().map(|(c, _)| *c).collect();
                        for coord in all_coords {
                            if vx.pending_remesh_set.insert(coord) {
                                vx.pending_remesh.push_back(coord);
                            }
                        }
                    }
                    // Finalisation happens in the BuildingMeshes ->
                    // Finalising gate below once pending_remesh drains.
                }
                // W27.4: once the lighting worker job returns, advance
                // from BuildingLighting to BuildingMeshes.
                if phase == LoadingPhase::BuildingLighting
                    && vx.pending_lighting_jobs == 0
                    // Guard: workers may not have been used at all
                    // (e.g. if we somehow end up here without workers).
                    && vx.workers.is_some()
                {
                    if let AppState::Loading { ref mut phase, .. } = vx.state {
                        *phase = LoadingPhase::BuildingMeshes;
                    }
                    // W23.2: enqueue all chunks for deferred GPU upload.
                    // WorkerResult::Lit already added affected chunks to
                    // pending_remesh, but we add ALL coords here to
                    // guarantee nothing is skipped (Lit may not have
                    // arrived for every chunk if some had no grids yet).
                    let all_coords: Vec<ChunkCoord> =
                        vx.world.iter_chunks().map(|(c, _)| *c).collect();
                    for coord in all_coords {
                        if vx.pending_remesh_set.insert(coord) {
                            vx.pending_remesh.push_back(coord);
                        }
                    }
                }
                // W23.2: once all deferred meshes are built, finalise.
                if phase == LoadingPhase::BuildingMeshes
                    && vx.pending_remesh.is_empty()
                {
                    if !vx.load_state_applied {
                        vx.load_state_applied = true;
                        // Phase: Finalising - post-load bookkeeping.
                        if let AppState::Loading { ref mut phase, .. } = vx.state {
                            *phase = LoadingPhase::Finalising;
                        }
                        // W87: resolve the surface spawn BEFORE loading
                        // persisted state. load_world transiently clears +
                        // re-streams the procedural chunks, so safe_spawn has
                        // to read the populated world here - running it after
                        // load saw an empty world and returned its y=90 sky
                        // fallback (player spawned in the air and fell). The
                        // chunks exist now (post-mesh), so this finds real
                        // ground. load_world's fresh-player branch keeps this
                        // resolved position; a saved player overrides it with
                        // their logged-off spot.
                        let spawn = vx.safe_spawn();
                        vx.player_pos = spawn;
                        vx.player_vel = Vec3::ZERO;
                        vx.fall_start_y = None;
                        // Load persisted server state (edits, timers,
                        // containers, player). Clients skip - they sync via
                        // the catch-up broadcast on first heartbeat.
                        if matches!(vx.net, NetRole::Server { .. } | NetRole::Offline) {
                            vx.load_server_state();
                        }
                        // W112: load_server_state may restore a player position
                        // saved before the world's vertical scale changed (sea
                        // level 12 -> 63 lifted the terrain), or any spot since
                        // buried by an edit. If the loaded capsule is embedded in
                        // solid ground, fall back to the surface spawn resolved
                        // above instead of leaving the player stuck in the terrain.
                        if vx.capsule_embedded(vx.player_pos) {
                            // W324: loud breadcrumb - this fallback can
                            // land OUTSIDE the freshly generated ring
                            // (the spawn column), which used to mean a
                            // silent fall through ungenerated terrain.
                            // The update_play unloaded-chunk physics
                            // freeze now holds the player until the
                            // column streams in, but the relocation
                            // itself should never pass unnoticed.
                            tracing::warn!(
                                saved_pos = ?vx.player_pos,
                                fallback = ?spawn,
                                "W324: saved player pose is buried - relocating to spawn",
                            );
                            vx.player_pos = spawn;
                            vx.player_vel = Vec3::ZERO;
                            vx.fall_start_y = None;
                        }
                        // W236: load_world enqueues every loaded chunk for
                        // the WORKER remesh pipeline instead of meshing
                        // them synchronously (the 30-50 s "loader stuck"
                        // stall). If it queued work, drop back to
                        // BuildingMeshes so the progress screen stays up
                        // while the pool drains it; this branch re-fires
                        // with load_state_applied == true and finishes.
                        if !vx.pending_remesh.is_empty() {
                            if let AppState::Loading { ref mut phase, .. } = vx.state {
                                *phase = LoadingPhase::BuildingMeshes;
                            }
                        } else {
                            vx.world_loaded = true;
                            vx.state = AppState::Playing;
                            info!(
                                slug = ?vx.current_world_slug,
                                net = %vx.net.label(),
                                chunks = live,
                                "voxel: world loaded - entering Playing"
                            );
                        }
                    } else {
                        vx.world_loaded = true;
                        vx.state = AppState::Playing;
                        info!(
                            slug = ?vx.current_world_slug,
                            net = %vx.net.label(),
                            chunks = live,
                            "voxel: world loaded - entering Playing"
                        );
                    }
                }
            }
            // W8.1 + bila-screen fix: no world simulation, but the
            // renderer + egui pass below MUST still run so the main
            // menu / loading screen actually paints. The legacy
            // `return` here skipped the entire on_update tail
            // (including renderer.render + the egui overlay callback)
            // - that's what produced the all-white screen.
            vx.particles.tick(dt);
            if let Some(r) = vx.renderer.as_mut() {
                r.set_clear_color(0.04, 0.05, 0.09, 1.0);
            }
            // Run the renderer directly here and short-circuit the
            // world-side work below; renderer.render() still gets the
            // egui callback so menu / loading UI paints.
            let ui = vx.ui.as_mut();
            if let Some(renderer) = vx.renderer.as_mut() {
                let _ = if let Some(ui) = ui {
                    renderer.render(|d, q, e, v, s| ui.render(d, q, e, v, s))
                } else {
                    renderer.render(|_, _, _, _, _| {})
                };
            }
            if vx.requested_quit {
                std::process::exit(0);
            }
            return;
        }
        // W230: name the slow sub-phase of the world-tick block. The
        // W198.16 hitch line blamed "world_tick 30ms" without saying
        // WHICH tick - any phase over the threshold logs itself.
        macro_rules! wt_slow {
            ($label:literal, $body:expr) => {{
                let _t = std::time::Instant::now();
                let r = $body;
                let _ms = _t.elapsed().as_secs_f32() * 1000.0;
                if _ms > 3.0 {
                    tracing::warn!(ms = _ms, phase = $label, "W230: slow world-tick phase");
                }
                r
            }};
        }
        // Authoritative side simulates wandering mobs. The render
        // path reads `vx.remote_mobs` which gets populated by
        // both `tick_mobs` (server) and `vx.mob` (client).
        let _t = std::time::Instant::now();
        wt_slow!("mobs", vx.tick_mobs(dt));
        DIAG_MOBS_US.fetch_add(_t.elapsed().as_micros() as u64, DiagOrd::Relaxed);
        // Generic liquid physics: drive each registered liquid
        // (water, lava, ...) at its own MC-style cadence. Per
        // liquid the host runs fall -> spread -> optional source
        // merge -> decay. Clients skip and rely on `vx.edit`.
        let _t = std::time::Instant::now();
        wt_slow!("liquids", vx.tick_liquids(dt));
        DIAG_LIQUIDS_US.fetch_add(_t.elapsed().as_micros() as u64, DiagOrd::Relaxed);
        // W64: gravity for sand / concrete_powder / gravel. Host
        // drops each queued unsupported falling cell one voxel per
        // tick; clients apply the resulting `vx.edit` traffic.
        wt_slow!("falling", crate::world::falling::tick(vx));
        // W65: MC-style random block ticks (grass spread; later
        // sapling growth, leaf decay, crops). Host-only sample over
        // chunks near the player; clients get the `vx.edit` stream.
        wt_slow!("random_tick", crate::world::random_tick::tick(vx, dt));
        // Step every floating drop entity: gravity, bob,
        // pickup-on-touch, despawn after lifetime. Host runs
        // the full sim; clients also tick locally for smooth
        // motion between server snapshots.
        wt_slow!("drops", vx.tick_drops(dt));
        // Live furnace cook step. Walks every furnace + advances
        // fuel + cook progress by `dt_ms` (the frame delta in
        // milliseconds). The offline catch-up path in
        // `world_save::timers::catch_up_furnace` handles the
        // window between save and reload.
        let dt_ms = (dt * 1000.0).round().max(0.0) as u32;
        let _wt_furnace_t = std::time::Instant::now();
        block::furnace::furnace_tick(
            &mut vx.furnaces,
            &mut vx.furnace_runtimes,
            dt_ms,
        );
        // W50.5: brewing stand tick (5-slot container + 400-tick
        // brew cycle that transforms matching bottle slots).
        block::brewing::brewing_tick(
            &mut vx.brewing_stands,
            &mut vx.brewing_runtimes,
            dt_ms,
        );
        // W172: hopper transport. On a fixed cadence, each hopper
        // pulls one item from the container above + pushes one to the
        // container below (all stored in vx.chests). Wall-clock gated
        // so the rate is frame-rate independent.
        vx.hopper_clock_ms = vx.hopper_clock_ms.saturating_add(dt_ms);
        if vx.hopper_clock_ms >= block::hopper::HOPPER_INTERVAL_MS {
            vx.hopper_clock_ms = 0;
            let hoppers: Vec<[i32; 3]> = vx
                .chests
                .keys()
                .copied()
                .filter(|p| vx.world.get(*p).0 == crate::block::HOPPER_ID)
                .collect();
            if !hoppers.is_empty() {
                block::hopper::transport_round(&mut vx.chests, &hoppers);
            }
        }
        // W5.3b: broadcast furnace state snapshots to registered
        // viewers at ~5 Hz so a remote client sees the cook
        // progress + fuel bar move smoothly. The throttle gate
        // avoids spamming a snapshot every frame.
        if matches!(vx.net, NetRole::Server { .. }) {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let viewer_positions: Vec<[i32; 3]> =
                vx.furnace_viewers.keys().copied().collect();
            for pos in viewer_positions {
                if vx.should_send_furnace_state(pos, now_ms) {
                    vx.broadcast_furnace_state(pos);
                }
            }
        }
        {
            let _ms = _wt_furnace_t.elapsed().as_secs_f32() * 1000.0;
            if _ms > 3.0 {
                tracing::warn!(ms = _ms, phase = "furnace+hopper", "W230: slow world-tick phase");
            }
        }
        // Q.4c: fire random-tick. Runs server-side; clients pick
        // up the resulting `vx.edit` traffic. Cheap when
        // `vx.fires` is empty (early return).
        wt_slow!("fire", vx.fire_tick());
        // Q.4d: lava-source-block fire emission (MC's
        // LavaFluid.randomTick). Walks every lava source cell
        // once per net tick; each gates 1/LAVA_IGNITE_RATE_DENOM
        // and rolls the two-branch ignite algorithm.
        wt_slow!("lava_emit", vx.tick_lava_fire_emit());
        // Phase F.6: portal-overlap counter + cross-dim teleport.
        wt_slow!("portal", vx.tick_player_portal(dt));
        // Auto-close the chest UI when the player walks more than
        // ~5 blocks from the open chest. Matches MC's "stay close
        // to the container" rule. Done here (per-frame) so the
        // close fires even while no input lands.
        if let Some(pos) = vx.chest_open {
            let dx = vx.player_pos.x - (pos[0] as f32 + 0.5);
            let dy = vx.player_pos.y - (pos[1] as f32 + 0.5);
            let dz = vx.player_pos.z - (pos[2] as f32 + 0.5);
            let dist_sq = dx * dx + dy * dy + dz * dz;
            const CHEST_MAX_DIST: f32 = 5.0;
            if dist_sq > CHEST_MAX_DIST * CHEST_MAX_DIST {
                vx.close_chest_ui();
            }
        }
        // Same 5-block leash for the furnace UI. Mirrors the
        // chest-close check above so walking away closes the panel
        // and routes the held stack back to the furnace (or to
        // floor drops if nothing fits there).
        if let Some(pos) = vx.furnace_open {
            let dx = vx.player_pos.x - (pos[0] as f32 + 0.5);
            let dy = vx.player_pos.y - (pos[1] as f32 + 0.5);
            let dz = vx.player_pos.z - (pos[2] as f32 + 0.5);
            let dist_sq = dx * dx + dy * dy + dz * dz;
            const FURNACE_MAX_DIST: f32 = 5.0;
            if dist_sq > FURNACE_MAX_DIST * FURNACE_MAX_DIST {
                vx.close_furnace_ui();
            }
        }
        // Mob audio runs on every peer (host + each client): step
        // sting per moving sheep, occasional baa, both
        // distance-attenuated through `sfx_at`.
        wt_slow!("mob_audio", vx.tick_mob_audio(dt));
        // W9.5: emit ambient flame + smoke particles from every
        // torch / fire / lava source within visible range of the
        // player. Capped at AMBIENT_PARTICLE_CAP_PER_TICK so a
        // forest of torches doesn't blow the global particle
        // budget. Runs every frame; the per-source accumulator
        // takes care of the 5 Hz target rate.
        wt_slow!("ambient_particles", vx.tick_ambient_particles(dt));
        // Tick particles regardless of net role.
        wt_slow!("particles", vx.particles.tick(dt));
        // W198.9: cumulative after the per-frame world-tick block (mobs/
        // liquids/drops/fire/ambient/particles). CHK_WORLD - CHK_HEAD is the
        // tick cost; CHK_MID - CHK_WORLD is the camera/sky/lighting-tick
        // pre-render cost. Localizes the untimed ~20ms gap.
        crate::perf::store(
            &crate::perf::CHK_WORLD_MS,
            _frame_start.elapsed().as_secs_f32() * 1000.0,
        );
        // W293/W295: the user's hitches (89-297 ms) live in this
        // CHK_WORLD..CHK_MID span and the first probe started too
        // late (after the lighting tick + liquid remesh) - cover the
        // WHOLE span with named marks now.
        let _pr_t = std::time::Instant::now();
        // Tick the day clock unconditionally - even paused / in
        // menus the world keeps turning so the host's lighting is
        // never out of sync with what a client sees. /freezetime
        // pauses the advance.
        if !vx.freeze_time {
            vx.day_time = (vx.day_time + dt).rem_euclid(DAY_LENGTH_S);
        }
        // Phase L.6: lighting tick at ~10 Hz. Drains the dirty
        // set + re-floods every affected chunk + 26-neighbour
        // halo, then asks the renderer to rebuild those meshes
        // so the new per-vertex light reaches the GPU.
        vx.last_lighting_tick_s += dt;
        if vx.last_lighting_tick_s >= LIGHTING_TICK_PERIOD_S {
            vx.last_lighting_tick_s = 0.0;
            // W198.14: time the lighting dispatch separately - it clones the
            // affected chunks + halo on the main thread and was invisible,
            // folded into `sim`. fetch_max captures the worst 10 Hz spike.
            let light_t0 = std::time::Instant::now();
            let affected = vx.tick_lighting();
            if !affected.is_empty() && vx.renderer.is_some() {
                vx.rebuild_chunks(&affected);
            }
            let light_us = light_t0.elapsed().as_micros() as u64;
            light_us_frame = light_us;
            DIAG_LIGHT_US.fetch_add(light_us, DiagOrd::Relaxed);
            DIAG_LIGHT_PEAK_US.fetch_max(light_us, DiagOrd::Relaxed);
        }
        let _ms_light_tick = _pr_t.elapsed().as_secs_f32() * 1000.0;
        // Day <-> night transition announcer. Fires once per
        // crossing using the sun elevation so the chat doesn't
        // spam every frame.
        let phase_t = (vx.day_time / DAY_LENGTH_S).rem_euclid(1.0);
        let elev_t = (phase_t * std::f32::consts::TAU).sin();
        let is_night_now = elev_t < -0.05;
        if is_night_now != vx.was_night {
            let line = if is_night_now {
                "* night falls"
            } else {
                "* dawn breaks"
            };
            vx.chat_log.push((
                std::time::Instant::now(),
                line.to_string(),
            ));
            vx.was_night = is_night_now;
        }

        // Rebuild the combined water mesh once the dirty flag is
        // set. Done before the renderer borrow below so the
        // create_mesh / remove_mesh churn doesn't fight with the
        // per-frame draw calls.
        let liqmesh_t0 = std::time::Instant::now();
        vx.maybe_rebuild_liquid_mesh();
        DIAG_LIQMESH_US.fetch_add(liqmesh_t0.elapsed().as_micros() as u64, DiagOrd::Relaxed);
        let _ms_liqmesh = _pr_t.elapsed().as_secs_f32() * 1000.0;
        let player_pos = vx.player_pos;
        // W77: borrow the mesh table instead of cloning the whole
        // HashMap<ChunkCoord, Vec<(BlockId, MeshHandle)>> every frame.
        // The clone allocated one Vec per loaded chunk per frame, a cost
        // that scaled with view distance (hundreds at vd=4, thousands at
        // vd=12) for no benefit - the render pass only reads it. The
        // mutations that follow touch disjoint Voxel fields, so the
        // shared borrow is fine.
        let chunk_meshes = &vx.chunk_meshes;
        let registry = &vx.registry;
        // Wireframe outline follows the mining ray so the player
        // sees the highlight on a decoration (torch / flower)
        // that they can break, not on the solid block hidden
        // behind it. Falls back to the placement target when no
        // mineable cell is in view (shouldn't happen in
        // practice since `mine_target` matches every non-air
        // voxel).
        let target = vx.mine_target.or(vx.target);
        // W176: snapshot panel-open BEFORE the renderer borrow (the
        // held-item swing-gate needs it, and `any_panel_open()` borrows
        // all of `vx` so it can't run while `renderer` is held).
        let any_panel_open = vx.any_panel_open();
        // W212: snapshot the worldgen ctx too (`current_worldgen_ctx`
        // borrows all of `vx`) - the underwater fog biome sample below
        // runs while `ui` + `renderer` are held.
        let worldgen_ctx = vx.current_worldgen_ctx();
        let ui = vx.ui.as_mut();
        let Some(renderer) = vx.renderer.as_mut() else {
            return;
        };
        {
            let cam = renderer.camera_3d_mut();
            // Refresh global shader time. Drives the animated
            // water surface (caustic noise scrolls along the
            // baked flow direction).
            cam.time_seconds = vx.app_start.elapsed().as_secs_f32();
            // Phase L.5: sky-light multiplier for the day/night
            // cycle. Read by the mesh3d shader as `camera.time.y`
            // and folded into the per-vertex `light.y` (sky)
            // value via `max(block, sky * day_factor)`.
            cam.day_factor = lighting::compute_day_factor(
                vx.day_time,
                DAY_LENGTH_S,
            );
            // Head-bob offset: a small vertical bob + lateral
            // sway scaled by current horizontal speed so a still
            // player has a rock-steady camera. Skip while in fly
            // mode - hovering shouldn't shake the view.
            let h_speed = (vx.player_vel.x * vx.player_vel.x
                + vx.player_vel.z * vx.player_vel.z)
                .sqrt();
            let bob_amount =
                if !vx.settings.head_bob || vx.fly_mode || !vx.grounded {
                    0.0
                } else {
                    (h_speed / PLAYER_SPEED).clamp(0.0, 1.5)
                };
            // Sprint widens the bob a little (still subtle) so
            // running feels heavier than walking.
            let sprint_boost = if vx.sprinting { 1.6 } else { 1.0 };
            let bob_y =
                (vx.head_bob_phase * 2.0).sin() * 0.05 * bob_amount * sprint_boost;
            let bob_x =
                vx.head_bob_phase.sin() * 0.03 * bob_amount * sprint_boost;
            let right = Vec3::new(
                -(-vx.player_yaw.sin()),
                0.0,
                -vx.player_yaw.cos(),
            )
            .normalize_or_zero();
            let bob = Vec3::new(right.x * bob_x, bob_y, right.z * bob_x);
            // Third-person modes pull the camera back along the
            // view ray (mode 1) or push it in front of the player
            // looking backward (mode 2). The offset is short
            // enough that it usually stays inside a 1-chunk view
            // bubble even on tight walls.
            const TP_DIST: f32 = 4.5;
            match vx.camera_mode {
                0 => {
                    cam.position = player_pos + bob;
                    cam.target = player_pos + bob + forward;
                }
                1 => {
                    cam.position = player_pos + bob - forward * TP_DIST;
                    cam.target = player_pos + bob;
                }
                _ => {
                    cam.position = player_pos + bob + forward * TP_DIST;
                    cam.target = player_pos + bob;
                }
            }
            cam.fov_y_rad = vx.fov_y_rad;
        }
        // W47.15: sky / weather / fog tint moved into render::sky.
        // W306: `fixed_time` dims (nether 18000, End 6000) pin the
        // celestial clock - the nether visibly cycled day/night
        // shadows because the shared overworld day_time kept feeding
        // the sun direction. MC freezes it per dimension_type.
        let eff_day_time = {
            let d = crate::worldgen::dim_registry::dim_def_or_overworld(
                &vx.current_dim,
            );
            match d.fixed_time {
                Some(t) => t as f32 / 24_000.0 * crate::DAY_LENGTH_S,
                None => vx.day_time,
            }
        };
        let (sun_dir, base_sky) = day_night(eff_day_time);
        // W113: underwater post-process - true when the camera (eye) voxel is
        // water, so the sky/fog setup can wash the view blue + pull the fog in
        // tight (MC's submerged look). player_pos.y is the eye height.
        let eye = vx.player_pos;
        // W203: waterlogged plant cells count as water - the murk must
        // not flicker off while swimming through a kelp forest.
        let underwater = crate::block::block_is_watery(
            vx.world
                .get([eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32])
                .0,
        );
        // W207: MC waterVision ramp - the eye adjusts to the water the
        // longer it stays submerged (+20 tick-units/s under, -200/s
        // out, clamp 0..600, exactly LocalPlayer.waterVisionTime).
        if underwater {
            vx.water_vision_time = (vx.water_vision_time + dt * 20.0).min(600.0);
        } else {
            vx.water_vision_time = (vx.water_vision_time - dt * 200.0).max(0.0);
        }
        // W221: sky exposure at the camera - underground the distance
        // fog + clear colour fade toward darkness instead of glowing
        // sky-blue (the user's "light shining from the fog in caves").
        // Eased so cave entrances don't pop.
        {
            let p = [eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32];
            let cs = infengine_voxel::CHUNK_SIZE as i32;
            let cc = [p[0].div_euclid(cs), p[1].div_euclid(cs), p[2].div_euclid(cs)];
            let sky = vx
                .lighting
                .grids
                .get(&cc)
                .map(|g| {
                    g.get_sky([
                        p[0].rem_euclid(cs) as u32,
                        p[1].rem_euclid(cs) as u32,
                        p[2].rem_euclid(cs) as u32,
                    ])
                })
                .unwrap_or(15);
            // Floor 0.04: pitch-black fog reads as a render hole; MC's
            // cave horizon keeps a whisper of tint.
            let target = (sky as f32 / 15.0).max(0.04);
            let ease = (dt * 3.0).min(1.0);
            vx.cave_dark += (target - vx.cave_dark) * ease;
        }
        let water_vision =
            crate::render::sky::water_vision_mult(vx.water_vision_time);
        // W212: the biome's water_fog_color at the camera - only worth
        // the climate sample when actually submerged in an Mc world.
        let water_fog = if underwater {
            matches!(worldgen_ctx.profile, crate::worldgen::WorldGen::Mc).then(|| {
                let mc = crate::worldgen::mcgen::terrain::mc_full_biome_at(
                    &worldgen_ctx,
                    eye.x.floor() as i32,
                    eye.z.floor() as i32,
                );
                crate::worldgen::mcgen::colors::water_fog_color(mc)
            })
        } else {
            None
        };
        // W155/W264: the dimension registry decides the sky - roofed
        // dims render the nether haze, skylight-less open dims (the
        // End) the void; everything else runs the day cycle.
        let dim_def =
            crate::worldgen::dim_registry::dim_def_or_overworld(&vx.current_dim);
        let dim_sky = if dim_def.has_ceiling {
            crate::render::sky::DimSky::NetherHaze
        } else if !dim_def.has_skylight {
            crate::render::sky::DimSky::EndVoid
        } else {
            crate::render::sky::DimSky::Day
        };
        // W321: with an active LOD ring the pop-in fog curtain moves
        // out to the LOD edge - the shells ARE the far scenery.
        let mut fog_distance_chunks = if vx.settings.lod_distance
            > vx.settings.view_distance
            && !vx.lod_meshes.is_empty()
        {
            vx.settings.lod_distance
        } else {
            vx.settings.view_distance
        };
        // W333: the far ring pushes the curtain out again.
        if vx.settings.lod2_distance > fog_distance_chunks
            && !vx.lod2_meshes.is_empty()
        {
            fog_distance_chunks = vx.settings.lod2_distance;
        }
        let _sky = crate::render::sky::apply_tint_and_fog(
            renderer,
            sun_dir,
            base_sky,
            vx.weather,
            vx.lightning_flash,
            fog_distance_chunks,
            underwater,
            water_vision,
            water_fog,
            vx.cave_dark,
            dim_sky,
        );
        let _ms_sky = _pr_t.elapsed().as_secs_f32() * 1000.0;
        let player_cube_handle = *vx.player_cube.get_or_insert_with(|| renderer.cube_handle());
        let session_secs = vx.session_started.map(|s| s.elapsed().as_secs_f32()).unwrap_or(0.0);
        if dim_sky == crate::render::sky::DimSky::Day {
            crate::render::celestial::draw(
                renderer,
                player_cube_handle,
                player_pos,
                sun_dir,
                vx.day_time,
                session_secs,
            );
        }
        let _ms_celestial = _pr_t.elapsed().as_secs_f32() * 1000.0;
        // W299: dynamic emitter point lights REMOVED - MC has no
        // dynamic lights. Block light is the baked BFS (torch 14,
        // glowstone 15...), identical at any viewer distance. The
        // nearest-32 pass double-lit surfaces near the player and
        // popped as emitters crossed the 40-block cull ring (the
        // "light gets stronger as I approach" nether report).
        let _ms_plights = _pr_t.elapsed().as_secs_f32() * 1000.0;
        // W46.13: legacy pre-MC torch render path REMOVED.
        //
        // Up to W45 this block drew every entry in `vx.torch_positions`
        // as a hand-built stick cube + a flicker-animated emissive tip
        // cube. Since W39+ the chunk mesher emits the proper MC
        // template_torch.json geometry (verified in W46.11 log:
        // "torch-family resolved to Single(JSON)") with the right texture
        // UVs, so this custom render path was producing a SECOND, ugly,
        // untextured torch on top of the textured one - the "pulsing
        // top" the user reported in W46 debugging.
        //
        // `vx.torch_positions` is kept - it is still consumed by the
        // particle spawn at ~line 3251 (smoke + flame sources) and by
        // the emitter_lights module. Only this render block is gone.
        let decor_cube = *vx
            .player_cube
            .get_or_insert_with(|| renderer.cube_handle());
        crate::render::decor::draw(
            renderer,
            decor_cube,
            &vx.world,
            &vx.portal_positions,
            &vx.fires,
            &vx.liquid_mesh_handles,
            vx.day_time,
            &vx.effect_meshes,
        );
        // Floating drop entities. Each renders as a small cube
        // (~0.3 voxel) bobbing in place. Pulled off
        // `dropped_items` so both host + client see the same
        // set; bob phase comes from the drop's `age` so
        // neighbouring items don't oscillate in lockstep.
        // W47.16: dropped items render moved into render::drops.
        let drops_cube = *vx
            .player_cube
            .get_or_insert_with(|| renderer.cube_handle());
        crate::render::drops::draw(
            renderer,
            drops_cube,
            &vx.dropped_items,
            player_pos,
            &vx.registry,
        );
        // W47.17: remote + local player cube render moved into
        // render::players. own_id derives from net role; remote
        // sampling skipped offline; local override added in 3rd-person.
        // W293: name the pre-render split when it spikes. `sky` covers
        // camera+fog+tint, `celestial` the sun/moon draws, `plights`
        // the emitter point-light push, `tail` decor + drops.
        {
            let _ms_tail = _pr_t.elapsed().as_secs_f32() * 1000.0;
            // W326: threshold 30 -> 10 ms - the walking log showed
            // sustained 12-21 ms pre_render spikes that never tripped
            // this probe, so the split stayed invisible.
            if _ms_tail > 10.0 {
                tracing::warn!(
                    total_ms = _ms_tail,
                    light_tick_ms = _ms_light_tick,
                    liqmesh_ms = _ms_liqmesh - _ms_light_tick,
                    sky_cam_ms = _ms_sky - _ms_liqmesh,
                    celestial_ms = _ms_celestial - _ms_sky,
                    plights_ms = _ms_plights - _ms_celestial,
                    decor_drops_ms = _ms_tail - _ms_plights,
                    emitters = vx.emitter_positions.len(),
                    liquid_mesh_chunks = vx.liquid_mesh_handles.len(),
                    "W293: slow pre-render split"
                );
            }
        }
        // W198.4: cumulative time up to the start of the entity/particle
        // render-prep block, to localize the untimed sim gap.
        crate::perf::store(
            &crate::perf::CHK_MID_MS,
            _frame_start.elapsed().as_secs_f32() * 1000.0,
        );
        let own_id: u32 = match &vx.net {
            NetRole::Offline => u32::MAX,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let include_remote = !matches!(vx.net, NetRole::Offline);
        let local_render = (vx.camera_mode != 0).then_some(
            crate::render::players::LocalPlayerRender {
                id: own_id,
                pos: player_pos,
                yaw: vx.player_yaw,
                pitch: vx.player_pitch,
            },
        );
        let players_cube = *vx
            .player_cube
            .get_or_insert_with(|| renderer.cube_handle());
        crate::render::players::draw(
            renderer,
            players_cube,
            &vx.remote_interp,
            own_id,
            include_remote,
            local_render,
        );
        // W28.11: Build frustum for mob culling. Mobs behind the player
        // or off to the side don't need draw_mesh_pbr calls. Re-use the
        // same Frustum type as the chunk culling path.
        let mob_frustum = {
            let cam = renderer.camera_3d();
            Frustum::from_view_proj(cam.view_projection())
        };

        // Render every known sheep (server: from `mobs`, client:
        // from `remote_mobs` populated by `vx.mob`). Sheep are
        // boxy white cubes with a small head cube on top for
        // silhouette. Scaled wider than tall so they read as
        // four-legged rather than human-shaped.
        let player_cube_for_mobs = *vx.player_cube.get_or_insert_with(|| renderer.cube_handle());
        render::mobs::draw_remote_mobs(
            renderer,
            player_cube_for_mobs,
            &mob_frustum,
            &vx.remote_mobs,
            vx.sheep_mesh,
            vx.sheep_head_mesh,
            &vx.mob_meshes,
            &vx.mob_hurt_flash,
            &vx.mob_swell,
            &vx.mob_dying,
            &vx.mob_lunge,
            vx.day_time,
            &mut vx.mob_anim,
            dt,
            &vx.mob_prev_pos,
            // W204: fraction of the 20 Hz mob tick elapsed this frame.
            vx.mob_tick_accum / crate::mob::ai::MOB_TICK_DT,
        );
        // W250: the miniature display mob spinning inside nearby
        // spawner cages (MC SpawnerRenderer). Kind comes from the
        // worker-shipped map; the pure dungeon-table fallback keeps
        // the ctx cheap (no worldgen caches on the main thread).
        if !vx.spawner_positions.is_empty() {
            // Disjoint-field seed derivation - `current_worldgen_ctx`
            // would borrow all of vx under the live `ui` borrow.
            let eff_seed = if vx.current_dim.is_empty() {
                vx.base_seed
            } else {
                crate::worldgen::dim_seed(vx.base_seed, &vx.current_dim)
            };
            let player_pos = vx.player_pos;
            render::mobs::draw_spawner_displays(
                renderer,
                &mob_frustum,
                &vx.spawner_positions,
                &vx.spawner_kinds,
                &vx.spawner_states,
                &mut vx.spawner_spin,
                &vx.mob_meshes,
                player_pos,
                eff_seed,
                dt,
            );
        }
        // W126: chest block entities (entity sheets, facing,
        // double-chest halves, lid animation).
        render::chests::draw_chests(
            renderer,
            &vx.chest_meshes,
            &vx.chests,
            &vx.chest_facing,
            vx.chest_open,
            &vx.chest_viewers,
            &mut vx.chest_lid_anim,
            vx.player_pos,
            dt,
            &vx.world,
            &vx.chest_render_positions,
        );
        // W369: crack overlay follows the mining session (MC
        // destroy_stage_0..9 - the visible mining progress).
        {
            let mining = vx
                .mining
                .as_ref()
                .map(|m| (m.voxel, m.progress, m.total));
            let tex_root = if vx.resource_pack.is_placeholder() {
                None
            } else {
                Some(vx.resource_pack.source.join("textures"))
            };
            crate::render::break_overlay::update_break_overlay(
                renderer,
                &mut vx.break_overlay,
                mining,
                tex_root.as_deref(),
            );
        }
        // W366: bed block entities (real entity/bed sheets,
        // head/foot pieces, per-color).
        render::beds::draw_beds(
            renderer,
            &vx.bed_meshes,
            &vx.bed_states,
            &vx.world,
            vx.player_pos,
        );
        // W400: bell bodies (block entities over the chunk-mesh
        // stands).
        render::bells::draw_bells(
            renderer,
            &vx.bell_mesh,
            &vx.bell_positions,
            vx.player_pos,
        );
        // W134: minecart entities (local sim + remote snapshots).
        render::minecarts::draw_minecarts(
            renderer,
            &vx.minecart_meshes,
            &vx.carts,
            &vx.remote_carts,
        );
        // W140: arrows in flight / stuck in walls.
        render::projectiles::draw_arrows(
            renderer,
            &vx.arrow_meshes,
            &vx.arrows,
            &vx.remote_arrows,
        );
        // W337: ghast fireballs in flight.
        render::projectiles::draw_fireballs(
            renderer,
            &vx.arrow_meshes,
            &vx.fireballs,
            &vx.remote_fireballs,
        );
        // W338: primed TNT blocks (flashing, about to pop).
        {
            let tnt_tex_root = if vx.resource_pack.is_placeholder() {
                None
            } else {
                Some(vx.resource_pack.source.join("textures"))
            };
            let primed = std::mem::take(&mut vx.primed_tnt);
            crate::tnt::draw(
                renderer,
                &mut vx.held_mesh_cache,
                tnt_tex_root.as_deref(),
                &primed,
                &vx.remote_tnt,
            );
            vx.primed_tnt = primed;
        }
        // Render any live debris particles before the world cubes
        // so chunk faces draw on top when a particle ends up
        // inside a block.
        renderer.draw_particles(&vx.particles);
        // W18.3: flush textured billboard particles (torch flame,
        // smoke wisps, lava drips) accumulated since the last tick.
        // queue_textured_particle is O(1) per call; the Renderer
        // uploads them all at once at the start of the particle pass.
        for tp in &vx.textured_particles {
            renderer.queue_textured_particle(*tp);
        }
        vx.textured_particles.clear();
        // W198.4: cumulative time after the render-prep block. The delta
        // CHK_RPREP - CHK_MID is the cost of players/mobs/chests/minecarts/
        // arrows/particles draw-prep.
        crate::perf::store(
            &crate::perf::CHK_RPREP_MS,
            _frame_start.elapsed().as_secs_f32() * 1000.0,
        );
        let model = Mat4::IDENTITY.to_cols_array();
        // W13.4: build frustum from the current view-projection so we
        // can skip draw_mesh_pbr for chunks outside the visible cone.
        let frustum = {
            let cam = renderer.camera_3d();
            Frustum::from_view_proj(cam.view_projection())
        };
        // W26.4 / W32: chunk_meshes is HashMap<ChunkCoord, Vec<(BlockId, MeshHandle)>>.
        // W32: draw_mesh_pbr replaced with queue_chunk_draw - all visible chunk
        // submeshes are batched into a single multi_draw_indexed_indirect call
        // at render() time instead of one draw call per submesh.
        let _drawq_t = std::time::Instant::now();
        for (coord, subs) in chunk_meshes {
            // W13.4: skip chunks that are fully outside the view
            // frustum (behind the camera, off to the sides, beyond
            // the far plane). Typical win: ~60-70% of chunks culled.
            if !frustum.is_chunk_visible(*coord) {
                continue;
            }
            // W240: submeshes are merged per render layer; per-block
            // colour / PBR / emissive resolve in the shader from the
            // block-params table (vertex anim_id high bits carry the
            // block id). The instance is identity/white.
            for (_layer, handle) in subs {
                renderer.queue_chunk_draw_emissive(
                    *handle,
                    model,
                    [1.0, 1.0, 1.0, 1.0],
                    0.0,
                    0.5,
                    0.0,
                );
            }
        }
        // W321: far surface shells (own AABB cull per tile).
        if !vx.lod_meshes.is_empty() {
            crate::lod::queue_lod_draws(renderer, &vx.lod_meshes, &frustum);
        }
        // W333: the FAR ring (4x4-chunk tiles).
        if !vx.lod2_meshes.is_empty() {
            crate::lod::queue_lod2_draws(renderer, &vx.lod2_meshes, &frustum);
        }
        DIAG_DRAWQ_US.fetch_add(_drawq_t.elapsed().as_micros() as u64, DiagOrd::Relaxed);
        // Wire outline previewing the next action. The highlight
        // mode (H key, persisted in PlayerSave) gates which
        // preview wins:
        //   Auto  -> placeable held = build preview,
        //            otherwise the break wireframe.
        //   Build -> always the build preview (uses the held
        //            block's tint if any, else a neutral cyan so
        //            the cell is still visible without a block).
        //   Break -> always the break wireframe at target.voxel.
        if let Some(hit) = target {
            let highlight = vx.highlight_mode;
            // W46.17: Auto mode picks Build only when the held item is a
            // SOLID placeable block. Tools (pickaxe / shovel / axe / sword
            // / flint+steel / etc) drop into Break so the user sees what
            // they will MINE. Decoration items that pass raycast (torch,
            // flowers, water bucket, ...) also drop into Break because in
            // MC they don't "replace" the targeted voxel - they sit next
            // to it. Auto mode is about reading the user's intent: if you
            // hold a stack of dirt, you are building; otherwise you are
            // looking at what to break / what to attach to.
            let held_is_build_block = selected_block
                .map(|b| !is_tool_block(b) && !block_is_ray_passable(b))
                .unwrap_or(false);
            let show_build = match highlight {
                HighlightMode::Auto => held_is_build_block,
                HighlightMode::Build => true,
                HighlightMode::Break => false,
            };
            let (cell, color) = if show_build {
                let tint = selected_block
                    .and_then(|block| registry.get(block))
                    .map(|d| {
                        [
                            (d.color[0] * 1.4).min(1.0),
                            (d.color[1] * 1.4).min(1.0),
                            (d.color[2] * 1.4).min(1.0),
                            0.95,
                        ]
                    })
                    // Cyan fallback so Build-mode without a block in
                    // hand still draws something visible. Picked to
                    // contrast with the break wireframe's black.
                    .unwrap_or([0.3, 0.85, 1.0, 0.95]);
                (hit.place_at, tint)
            } else {
                (hit.voxel, [0.0, 0.0, 0.0, 0.95])
            };
            let min = [
                cell[0] as f32 - 0.005,
                cell[1] as f32 - 0.005,
                cell[2] as f32 - 0.005,
            ];
            let max = [
                cell[0] as f32 + 1.005,
                cell[1] as f32 + 1.005,
                cell[2] as f32 + 1.005,
            ];
            renderer.debug_box_3d(min, max, color);
        }
        // Mining-progress overlay: an extra wire box tinted from
        // bright yellow (just-started) through orange to red
        // (almost broken). Drawn slightly larger than the targeted
        // voxel so it doesn't z-fight with the regular outline.
        if let Some(m) = vx.mining {
            let t = (m.progress / m.total).clamp(0.0, 1.0);
            let r = 1.0;
            let g = 1.0 - t * 0.8;
            let b = 0.1 * (1.0 - t);
            let min = [
                m.voxel[0] as f32 - 0.02,
                m.voxel[1] as f32 - 0.02,
                m.voxel[2] as f32 - 0.02,
            ];
            let max = [
                m.voxel[0] as f32 + 1.02,
                m.voxel[1] as f32 + 1.02,
                m.voxel[2] as f32 + 1.02,
            ];
            renderer.debug_box_3d(min, max, [r, g, b, 1.0]);
        }
        // Chunk-grid overlay: wire boxes for the chunk the
        // player stands in + its 8 horizontal neighbours.
        // Cheap; only fires when toggled.
        if vx.show_chunk_grid {
            let cs = CHUNK_SIZE as i32;
            let pc = [
                (vx.player_pos.x.floor() as i32).div_euclid(cs),
                (vx.player_pos.y.floor() as i32).div_euclid(cs),
                (vx.player_pos.z.floor() as i32).div_euclid(cs),
            ];
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let cc = [pc[0] + dx, pc[1], pc[2] + dz];
                    let min = [
                        (cc[0] * cs) as f32,
                        (cc[1] * cs) as f32,
                        (cc[2] * cs) as f32,
                    ];
                    let max = [
                        min[0] + cs as f32,
                        min[1] + cs as f32,
                        min[2] + cs as f32,
                    ];
                    // Highlight the player's own chunk in
                    // bright yellow; neighbours dim cyan.
                    let color = if dx == 0 && dz == 0 {
                        [1.0, 0.85, 0.1, 0.95]
                    } else {
                        [0.2, 0.7, 1.0, 0.6]
                    };
                    renderer.debug_box_3d(min, max, color);
                }
            }
        }
        // W157: first-person held-item viewmodel. Advance the swing +
        // equip animation every frame (so it stays coherent across
        // camera modes), then draw the item only in first-person view
        // (MC hides the hand in third person). Reads the live camera so
        // the item rides with pitch + head-bob.
        if let Some((held, _)) = vx.inventory[vx.selected_slot] {
            // W176: don't swing the hand when a UI panel is open - a
            // click in the inventory / chest / crafting grid is not an
            // attack. (The mouse buttons are still "held" while the
            // panel eats them.) `any_panel_open` was snapshot above the
            // renderer borrow.
            crate::render::held_item::tick_held_anim(
                &mut vx.held_anim,
                dt,
                vx.lmb_held && !any_panel_open,
                vx.rmb_held && !any_panel_open,
                held.0,
            );
            if vx.camera_mode == 0 {
                let cam = renderer.camera_3d();
                let (cam_pos, cam_tgt) = (cam.position, cam.target);
                let (swing, equip) =
                    (vx.held_anim.swing_progress, vx.held_anim.equip_progress);
                // W370: eating progress drives the chew viewmodel.
                let eat = vx
                    .eating
                    .map(|(_, t)| (t / 1.6).min(1.0))
                    .unwrap_or(0.0);
                let tex_root = if vx.resource_pack.is_placeholder() {
                    None
                } else {
                    Some(vx.resource_pack.source.join("textures"))
                };
                crate::render::held_item::draw_held_item(
                    renderer,
                    &mut vx.held_mesh_cache,
                    tex_root.as_deref(),
                    held.0,
                    cam_pos,
                    cam_tgt,
                    swing,
                    equip,
                    eat,
                );
            }
        }
        // W103: everything from frame start to here is the per-frame sim cost
        // (input, ticks, streaming, lighting, draw-queue build) — the part NOT
        // counted by render_ms. Lets the diag attribute a frame collapse.
        let _diag_rt = std::time::Instant::now();
        let sim_frame_us = _diag_rt.duration_since(_frame_start).as_micros() as u64;
        DIAG_SIM_US.fetch_add(sim_frame_us, DiagOrd::Relaxed);
        // W198.14: keep the worst single-frame sim cost so the F3 overlay
        // can expose stutter the average buries.
        DIAG_SIM_PEAK_US.fetch_max(sim_frame_us, DiagOrd::Relaxed);
        // W198.16: if this is the worst sim frame of the window, snapshot its
        // full phase breakdown for the deduped low-FPS auto-log. The CHK_*
        // checkpoints were perf::store'd earlier this frame (cumulative ms
        // from frame start); stream/light are this frame's discrete costs.
        if sim_frame_us > DIAG_WSIM_US.load(DiagOrd::Relaxed) {
            let us = |ms: f32| (ms * 1000.0) as u64;
            DIAG_WSIM_US.store(sim_frame_us, DiagOrd::Relaxed);
            DIAG_WHEAD_US.store(us(crate::perf::load(&crate::perf::CHK_HEAD_MS)), DiagOrd::Relaxed);
            DIAG_WWORLD_US.store(us(crate::perf::load(&crate::perf::CHK_WORLD_MS)), DiagOrd::Relaxed);
            DIAG_WMID_US.store(us(crate::perf::load(&crate::perf::CHK_MID_MS)), DiagOrd::Relaxed);
            DIAG_WRPREP_US.store(us(crate::perf::load(&crate::perf::CHK_RPREP_MS)), DiagOrd::Relaxed);
            DIAG_WSTREAM_US.store(stream_us_frame, DiagOrd::Relaxed);
            DIAG_WLIGHT_US.store(light_us_frame, DiagOrd::Relaxed);
        }
        let render_result = if let Some(ui) = ui {
            renderer.render(|d, q, e, v, s| ui.render(d, q, e, v, s))
        } else {
            renderer.render(|_, _, _, _, _| {})
        };
        DIAG_REND_US.fetch_add(_diag_rt.elapsed().as_micros() as u64, DiagOrd::Relaxed);
        if let Err(err) = render_result {
            warn!(error = ?err, "frame render failed");
        }

        if vx.requested_quit {
            std::process::exit(0);
        }
    }
