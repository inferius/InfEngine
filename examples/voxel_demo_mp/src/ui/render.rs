//! W47.21 - render_ui body relocated from main.rs.
//!
//! Big egui closure for every AppState branch (menu, loading, paused,
//! settings, playing HUD + inventory + chat + chest + furnace + debug
//! panel). Moved out of `impl Voxel` for the < 500 LOC main.rs goal;
//! method on `Voxel` survives as a one-line thin wrapper.
//!
//! All `self.` references rewritten to `vx.` and `Self::` to `Voxel::`.
//! Body is otherwise unchanged.

#![allow(clippy::too_many_lines)]

use crate::*;
use infengine_window::EngineWindow;

pub(crate) fn render(vx: &mut Voxel, window: &EngineWindow) {
        let state = vx.state;
        let fps = vx.fps_ema;
        let show_fps = vx.settings.show_fps;
        let dim = vx.current_dim.clone();
        let day_time = vx.day_time;
        let player_health = vx.player_health;
        let stamina_frac =
            (vx.stamina / STAMINA_MAX).clamp(0.0, 1.0);
        let air_frac =
            (vx.player_air / AIR_MAX_SEC).clamp(0.0, 1.0);
        // Underwater / lava view tint. Triggered only when the
        // EYE voxel is in a liquid (head fully submerged); wading
        // through ankle-deep water doesn't paint the screen. The
        // body-AABB swim mechanics still kick in earlier - drag
        // / buoyancy / air drain react to any overlap, only the
        // overlay needs full submersion.
        let submerged_tint: Option<[f32; 4]> = if vx.fly_mode {
            None
        } else {
            submerged_liquid_eye(&vx.world, vx.player_pos)
                .map(|l| l.submerged_tint)
        };
        let player_hurt_flash = vx.hurt_flash;
        let invuln_remaining = vx.invuln_remaining;
        let show_debug = vx.show_debug;
        let hud_visible = vx.hud_visible;
        let kill_count = vx.kill_count;
        let chat_open = vx.chat_open;
        let held_tool_label: Option<&'static str> = vx
            .inventory
            .get(vx.selected_slot)
            .copied()
            .unwrap_or(None)
            .and_then(|(b, _)| tool_label(b));
        let weather_label: Option<&'static str> = match vx.weather {
            1 => Some("Rain"),
            2 => Some("Snow"),
            _ => None,
        };
        // W5.1: ctx-driven seed lookup so the HUD label tracks the
        // active dim's effective seed without a global atomic.
        let biome_seed = vx.current_worldgen_ctx().effective_seed();
        let biome_at_player = biome_at(vx.player_pos.x, vx.player_pos.z, biome_seed);
        let spawn_xz: (f32, f32) = match vx.spawn_pos {
            Some(p) => (p[0], p[2]),
            None => (0.0, 0.0),
        };
        // `biome_at` only returns Plains/Mountain/Desert under the
        // Classic + Amplified generators (which are the only path
        // that reaches this HUD-overlay block today). The extended
        // biomes the Realistic generator produces still fall back
        // to a label via `Biome::label()` so future climate-driven
        // HUD work doesn't have to thread a second enum.
        let biome_name = biome_at_player.label();
        // Snapshot recent chat lines (newest last) along with their
        // age so the renderer can fade older entries.
        let chat_now = std::time::Instant::now();
        let chat_lines: Vec<(String, f32, bool)> = {
            let own_lower = vx.player_name.to_lowercase();
            vx.chat_log
                .iter()
                .map(|(t, line)| {
                    let mention =
                        !own_lower.is_empty() && line.to_lowercase().contains(&own_lower);
                    (
                        line.clone(),
                        chat_now.saturating_duration_since(*t).as_secs_f32(),
                        mention,
                    )
                })
                .collect()
        };
        let mut chat_send_now = false;
        // View-projection for projecting mob HP bars to screen.
        // Built from the same camera state the world pass uses;
        // captured here so the egui closure doesn't need renderer
        // access.
        let hud_cam_pos = vx.player_pos;
        let hud_forward = Vec3::new(
            vx.player_yaw.cos() * vx.player_pitch.cos(),
            vx.player_pitch.sin(),
            -vx.player_yaw.sin() * vx.player_pitch.cos(),
        );
        let hud_aspect =
            vx.viewport.0.max(1) as f32 / vx.viewport.1.max(1) as f32;
        let hud_view_proj = Mat4::perspective_rh(
            vx.fov_y_rad,
            hud_aspect,
            0.1,
            500.0,
        ) * Mat4::look_at_rh(hud_cam_pos, hud_cam_pos + hud_forward, Vec3::Y);
        // W8.3: `sky_blocked` + `night_factor` lived here only to
        // gate the 2D star field. The star field was deleted; if a
        // future wave reintroduces overhead-only HUD bits these
        // helpers can come back.
        // Mob HP snapshot: (pos, current, max). Only mobs that
        // are damaged + not dying show a bar so the world doesn't
        // get cluttered with full-health bars.
        // Targeted-mob info shown next to the crosshair: kind name
        // + current/max HP. Drives a small label so the player can
        // see how many more hits a sheep / zombie needs.
        let aim_forward = Vec3::new(
            vx.player_yaw.cos() * vx.player_pitch.cos(),
            vx.player_pitch.sin(),
            -vx.player_yaw.sin() * vx.player_pitch.cos(),
        );
        let aim_origin = vx.player_pos;
        // Targeted-block label: name + (if actively mining) the
        // progress percentage. Hidden when the crosshair isn't on
        // anything breakable.
        let held_for_label = vx
            .inventory
            .get(vx.selected_slot)
            .copied()
            .unwrap_or(None)
            .map(|(b, _)| b)
            .unwrap_or(BlockId::AIR);
        let targeted_block_info: Option<(String, Option<u8>)> =
            vx.target.and_then(|hit| {
                let b = vx.world.get(hit.voxel);
                if b == BlockId::AIR {
                    return None;
                }
                // W124: localized block name on the HUD target label.
                let name = crate::lang::localized_block_name(b.0);
                let pct = vx.mining.as_ref().and_then(|m| {
                    if m.voxel == hit.voxel && m.block == b {
                        Some(((m.progress / m.total).clamp(0.0, 1.0) * 100.0)
                            as u8)
                    } else {
                        None
                    }
                });
                // Append a hint about the recommended tool when
                // the held one doesn't match the block category.
                let cat = block_category(b);
                let hint = match cat {
                    BlockCategory::Stone
                        if tool_tier(held_for_label) == 0 =>
                    {
                        " (need pickaxe)"
                    }
                    BlockCategory::Soil
                        if shovel_tier(held_for_label) == 0 =>
                    {
                        " (use shovel)"
                    }
                    BlockCategory::Wood
                        if axe_tier(held_for_label) == 0 =>
                    {
                        " (use axe)"
                    }
                    _ => "",
                };
                Some((format!("{name}{hint}"), pct))
            });
        let targeted_mob_info: Option<(&'static str, u8, u8)> = vx
            .raycast_mob(aim_origin, aim_forward, MOB_ATTACK_REACH * 1.5)
            .and_then(|(id, _)| vx.remote_mobs.get(&id).copied())
            .map(|m| {
                let max = match m.kind {
                    KIND_ZOMBIE => ZOMBIE_MAX_HEALTH as u8,
                    KIND_CHICKEN => 4u8,
                    KIND_PIG => 10u8,
                    _ => MOB_MAX_HEALTH as u8,
                };
                let name = match m.kind {
                    KIND_ZOMBIE => "Zombie",
                    KIND_CHICKEN => "Chicken",
                    KIND_PIG => "Pig",
                    KIND_COW => "Cow",
                    KIND_SKELETON => "Skeleton",
                    KIND_SPIDER => "Spider",
                    KIND_SHEEP => "Sheep",
                    _ => "Mob",
                };
                (name, m.health, max)
            });
        let mob_bars: Vec<([f32; 3], u8, u8)> = vx
            .remote_mobs
            .values()
            .filter(|m| {
                let dying = vx.mob_dying.get(&m.id).copied().unwrap_or(0.0);
                let max = match m.kind {
                    KIND_ZOMBIE => ZOMBIE_MAX_HEALTH as u8,
                    KIND_CHICKEN => 4u8,
                    KIND_PIG => 10u8,
                    _ => MOB_MAX_HEALTH as u8,
                };
                m.health > 0 && m.health < max && dying <= 0.0
            })
            .map(|m| {
                let max = match m.kind {
                    KIND_ZOMBIE => ZOMBIE_MAX_HEALTH as u8,
                    KIND_CHICKEN => 4u8,
                    KIND_PIG => 10u8,
                    _ => MOB_MAX_HEALTH as u8,
                };
                (m.pos, m.health, max)
            })
            .collect();
        // Remote nameplate snapshot: world-space pose + display
        // name for each other player so the egui pass can project
        // them onto the screen. Skips own_id so the local player
        // never sees a tag on their own head.
        let own_id_for_plates: u32 = match &vx.net {
            NetRole::Offline => u32::MAX,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let plate_now = std::time::Instant::now();
        let mut nameplates: Vec<([f32; 3], String, [u8; 3])> = vx
            .remote_interp
            .iter()
            .filter(|(id, _)| **id != own_id_for_plates)
            .map(|(id, r)| {
                let p = r.sample(plate_now);
                let name = vx
                    .player_names
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| format!("p{id}"));
                // Stable name colour per id: hash-derived hue,
                // brightened so the text reads against the sky.
                let h = (id.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                let tau = std::f32::consts::TAU;
                let r_c = (0.6 + 0.4 * (h * tau).sin().abs()).min(1.0);
                let g_c =
                    (0.6 + 0.4 * ((h + 0.33) * tau).sin().abs()).min(1.0);
                let b_c =
                    (0.6 + 0.4 * ((h + 0.66) * tau).sin().abs()).min(1.0);
                let color = [
                    (r_c * 255.0) as u8,
                    (g_c * 255.0) as u8,
                    (b_c * 255.0) as u8,
                ];
                (p.pos, name, color)
            })
            .collect();
        // In third-person we also draw the local player's
        // nameplate so the camera-pull-back body has its label.
        if vx.camera_mode != 0 {
            nameplates.push((
                [
                    vx.player_pos.x,
                    vx.player_pos.y,
                    vx.player_pos.z,
                ],
                vx.player_name.clone(),
                [180, 230, 180],
            ));
        }
        // Pickup toast snapshot: trim entries older than the
        // fade window so the HUD doesn't keep stale text alive.
        // We do the housekeeping here so the egui closure sees a
        // clean list.
        let toast_now = std::time::Instant::now();
        let toast_lifetime = 2.4_f32;
        vx.pickup_toasts
            .retain(|(t, _, _)| toast_now.duration_since(*t).as_secs_f32() <= toast_lifetime);
        let pickup_snapshot: Vec<(f32, String, [f32; 3])> = vx
            .pickup_toasts
            .iter()
            .rev()
            .take(6)
            .map(|(t, b, c)| {
                let age = toast_now.duration_since(*t).as_secs_f32();
                let alpha = (1.0 - (age / toast_lifetime)).clamp(0.0, 1.0);
                let name = block_type(b.0).map(|t| t.name).unwrap_or("?");
                let color = vx
                    .registry
                    .get(*b)
                    .map(|d| d.color)
                    .unwrap_or([1.0, 1.0, 1.0]);
                (alpha, format!("+{c} {name}"), color)
            })
            .collect();
        // Tab playerlist snapshot: pairs of (display_name,
        // is_self). Drawn as a modal centered overlay while Tab
        // is held.
        let playerlist_visible = vx.tab_held && !matches!(vx.net, NetRole::Offline);
        let playerlist: Vec<(String, bool)> = if playerlist_visible {
            let mut list: Vec<(String, bool)> = vx
                .player_names
                .iter()
                .map(|(id, n)| (n.clone(), *id == own_id_for_plates))
                .collect();
            // Self first, then alphabetical so the order is stable
            // across ticks.
            list.sort_by(|a, b| match (a.1, b.1) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
            });
            list
        } else {
            Vec::new()
        };
        let dbg_player = vx.player_pos;
        let dbg_yaw = vx.player_yaw;
        let dbg_pitch = vx.player_pitch;
        let dbg_chunks = vx.chunk_meshes.len();
        let dbg_grounded = vx.grounded;
        let dbg_mobs = vx.remote_mobs.len();
        // H.7: mob count + nearest distance line + per-kind breakdown
        // pulled from the new mob::hud helpers. Pre-format here so the
        // egui closure stays borrow-free of `&vx`.
        let dbg_mob_count_line = mob::hud::format_mob_count_line(vx);
        let dbg_mob_kinds_line = mob::hud::format_mob_kinds_line(vx);
        let dbg_particles = vx.particles.live_count();
        let dbg_fly = vx.fly_mode;
        let dbg_cam = match vx.camera_mode {
            0 => "1st",
            1 => "3rd back",
            _ => "3rd front",
        };
        let dbg_playtime = vx
            .session_started
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        // W5.1: capture the effective seed for the F3 HUD label so
        // the egui closure doesn't have to borrow &vx.
        let dbg_seed = vx.current_worldgen_ctx().effective_seed();
        // W10.7: snapshot the current game mode for the F3 line.
        let dbg_game_mode = vx.game_mode;
        // W10.7: cache whether the player's in Creative so the
        // hotbar tint + heart-row hide logic can branch without
        // borrowing vx inside the egui closure.
        let dbg_is_creative = vx.game_mode.is_creative();
        // W10.7: snapshot the fly state so the HUD chip can read
        // it without the closure borrowing vx.
        let dbg_fly_state = vx.fly_mode;
        let dbg_pending = vx.pending_chunks.len();
        let dbg_net = match &vx.net {
            NetRole::Offline => "offline".to_string(),
            NetRole::Server { server, poses, .. } => {
                format!(
                    "server (cli={}, poses={})",
                    server.client_count(),
                    poses.len(),
                )
            }
            NetRole::Client {
                client,
                pending_edits,
                pending_inv_actions,
                ..
            } => format!(
                "client (id={:?}, edits={}, inv_acts={})",
                client.client_id,
                pending_edits.len(),
                pending_inv_actions.len(),
            ),
        };
        // Wave 4 (Phase G.6): refresh the menu's worlds +
        // saved-servers caches if they've gone stale (>= 500 ms).
        // Caps the `list_worlds()` rate at <= 2 Hz so the
        // OnceLock-deduped "unreadable slot" warning doesn't fire
        // repeatedly while the picker is open. Done BEFORE the
        // `&mut vx.X` capture forest so the helper has a clean
        // &mut vx to work with.
        vx.refresh_menu_caches();
        vx.sync_menu_options();
        // W5.1: snapshot the active dim's effective seed BEFORE the
        // `&mut vx.settings` capture so the immutable borrow on
        // vx doesn't collide with the mutable capture chain.
        let active_world_seed = vx.current_worldgen_ctx().effective_seed();
        let settings_edit = &mut vx.settings;
        // Live-edit flag the F6 panel writes back when the user
        // clicks its window close button. Mirrors back to
        // `vx.show_debug_panel` after the egui closure exits.
        let mut dbg_panel_open = vx.show_debug_panel;
        let dbg_panel_was_open = dbg_panel_open;
        // Mutable refs the debug panel writes to. Snapshot the
        // weather pre-edit so we can detect changes after the
        // closure and rebroadcast.
        let dbg_day_time = &mut vx.day_time;
        let dbg_weather = &mut vx.weather;
        let dbg_fov_rad = &mut vx.fov_y_rad;
        let dbg_freeze_time = &mut vx.freeze_time;
        let dbg_fly_mode = &mut vx.fly_mode;
        let dbg_god_mode = &mut vx.god_mode;
        let dbg_mob_cap = &mut vx.mob_cap;
        let dbg_snow_rate = &mut vx.snow_rate;
        let dbg_liquid_bias = &mut vx.liquid_bias_drops;
        let weather_before = *dbg_weather;
        // Lazy-load the egui atlas texture on first render after
        // on_start has populated the pixel cache. We use NEAREST
        // sampling so the 16x16 tiles stay crisp at slot size.
        if vx.egui_atlas.is_none()
            && let (Some(ui), Some((aw, ah, rgba))) = (vx.ui.as_ref(), vx.atlas_rgba.as_ref())
        {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [*aw as usize, *ah as usize],
                rgba,
            );
            let handle = ui.context().load_texture(
                "voxel_atlas_egui",
                img,
                egui::TextureOptions::NEAREST,
            );
            vx.egui_atlas = Some(handle);
        }
        let atlas_tex_id: Option<egui::TextureId> =
            vx.egui_atlas.as_ref().map(|h| h.id());
        // W12.6: lazy-load the MC menu panorama strip (horizontally
        // stitched from 4 horizon faces) into an egui texture on
        // first menu draw. Falls back to None when no MC assets
        // present, in which case the menu still works with the
        // existing dark-navy clear color.
        if vx.egui_panorama.is_none()
            && let Some(ui) = vx.ui.as_ref()
            && !vx.resource_pack.is_fallback
            && let Some((w, h, rgba)) =
                panorama::stitch_horizontal_strip(&vx.resource_pack.source)
        {
            let img =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            let handle = ui.context().load_texture(
                "voxel_panorama_egui",
                img,
                egui::TextureOptions::LINEAR,
            );
            vx.egui_panorama = Some(handle);
        }
        // W12.6: advance the scroll offset by the MC 1-RPM rate.
        // A full 360-deg rotation = 60 s; the stitched strip is 4
        // tiles wide, so one full scroll = 60 s and per-frame
        // advance at 60 fps = `1/3600` in UV units. We use a fixed
        // 1/60 s tick assumption rather than threading `dt` through
        // every render path - the menu backdrop tolerates frame-rate
        // jitter (slight scroll-speed variance is invisible at
        // 1 RPM).
        vx.panorama_scroll =
            (vx.panorama_scroll + 1.0 / 3600.0).rem_euclid(1.0);
        let panorama_egui_tex: Option<egui::TextureId> =
            vx.egui_panorama.as_ref().map(|h| h.id());
        let panorama_scroll_local = vx.panorama_scroll;
        // W9.3 + W9.4: per-item UV resolver that the container UI
        // threads into its slot painter. Bound up here so the egui
        // closure can borrow it alongside the rest of the per-frame
        // snapshots. Animated items advance to the current frame via
        // `TextureRef::uv_at`.
        // W28.9: rebuild the icon_uv_table only when the 50 ms animation
        // tick changes (MC uses ~50 ms / 20fps for item icon anims).
        // This saves ~200 HashMap inserts per frame at 60 fps.
        let app_time_ms = vx.app_start.elapsed().as_millis() as u32;
        let anim_frame = app_time_ms / 50;
        if anim_frame != vx.icon_uv_table_frame {
            vx.icon_uv_table_frame = anim_frame;
            vx.icon_uv_table.clear();
            for it in crate::item::ITEM_TYPES.iter() {
                if let Some(r) = vx.resource_pack.icon_ref(it.id) {
                    vx.icon_uv_table.insert(it.id, r.uv_at(app_time_ms));
                }
            }
        }
        let icon_uv_resolver = |id: u16| -> Option<[f32; 4]> {
            vx.icon_uv_table.get(&id).copied()
        };
        // Snapshot tuple: (color, count, uv_rect_for_top_face). UV
        // is only used when the egui atlas texture is loaded;
        // otherwise we fall back to a solid color swatch.
        // W9.3: prefer the MC resource-pack's per-item icon UV when
        // one resolves. Non-block items (tools, ingots, buckets,
        // food, flint+steel) get dedicated MC icons; block items
        // pick up the block's top face. Falls through to the legacy
        // (0, 0)-placeholder + tinted swatch when no pack loaded.
        let resource_pack = &vx.resource_pack;
        let icon = |slot: Option<(BlockId, u8)>| -> Option<SlotIcon> {
            slot.map(|(b, c)| {
                let bt = block_type(b.0);
                // W20.2: use icon_uv_front which prefers the live MC pack
                // atlas entry and falls back to the legacy 4x4 grid.
                let uv = resource_pack.icon_uv_front(b);
                // W153: ITEM-ONLY ids (hoes, bow, arrow, minecart,
                // dyes, spawn eggs, ...) have no block row in the
                // voxel registry - they used to drop to None here and
                // their slots rendered EMPTY even though the icon
                // texture resolved fine. The icon carries the visual;
                // fall back to a neutral swatch + the ITEM_TYPES name.
                match vx.registry.get(b) {
                    Some(d) => (d.color, c, uv, bt.map(|t| t.name).unwrap_or("?")),
                    None => (
                        [0.78, 0.78, 0.78],
                        c,
                        uv,
                        crate::item::item_type(b.0).map(|it| it.name).unwrap_or("?"),
                    ),
                }
            })
        };
        let hotbar_snapshot: Vec<Option<SlotIcon>> = vx.inventory
            [..HOTBAR_SLOTS]
            .iter()
            .map(|s| icon(*s))
            .collect();
        // W5.2b: per-slot durability snapshot for the slot widgets.
        // Cloned out of `item_durability` so the egui closure can
        // borrow it alongside the mutable `&mut vx.inventory`
        // capture without colliding.
        let item_durability_snapshot: HashMap<usize, u16> =
            vx.item_durability.clone();
        let durability_for_slot = |idx: usize| -> Option<(u16, u16)> {
            item_durability_snapshot
                .get(&idx)
                .copied()
                .map(|cur| (cur, block::FLINT_AND_STEEL_MAX_USES))
        };
        let inventory_open = vx.inventory_open;
        let mut inv_open_local = vx.inventory_open;
        // W10.6: snapshot the creative tab state so the egui
        // closure can paint the panel + mutate `query` + record
        // click intents. We re-apply the mutations after the
        // closure exits.
        let creative_tab_open = vx.creative_tab.open;
        let mut creative_query_local = vx.creative_tab.query.clone();
        // W12.9 - live category from the tab strip; mutated by the
        // selectable labels inside the egui closure.
        let mut creative_cat_local = vx.creative_tab.category;
        let mut creative_close_clicked = false;
        // W20.3: snapshot the just_opened flag so the egui closure can
        // request search-box focus on the first frame. The flag is
        // cleared in the post-egui section.
        let creative_just_opened = vx.creative_tab.just_opened;
        // Pending pick-up intents queued from the grid. Each
        // entry is the (block, count) to drop into the hotbar.
        let mut creative_picks: Vec<(BlockId, u8)> = Vec::new();
        let mp_menu_open = vx.mp_menu_open;
        let mut mp_menu_open_local = vx.mp_menu_open;
        // Chest UI snapshot. Cloned here so the egui closure can
        // render a grid against a stable view; applied actions
        // mutate `vx.chests` after the closure finishes.
        let chest_open_pos = vx.chest_open;
        let chest_snapshot: Option<container::Container> = vx
            .chest_open
            .as_ref()
            .and_then(|p| vx.chests.get(p).cloned());
        // W55.2b: detect a paired chest neighbour for the open
        // chest. When found, the UI renders a 54-slot composite
        // (this chest above, partner below) and the apply path
        // routes actions to the matching backing container.
        let chest_partner_pos: Option<[i32; 3]> = chest_open_pos
            .and_then(|p| {
                block::chest::partner_for(
                    &vx.world,
                    &vx.chests,
                    &vx.chest_facing,
                    p,
                )
            });
        let chest_partner_snapshot: Option<container::Container> =
            chest_partner_pos.and_then(|p| vx.chests.get(&p).cloned());
        // Sentinel that the egui close-button + ESC inside the
        // chest panel can flip; we read it back after the closure
        // to clear `chest_open`.
        let mut chest_open_local = chest_open_pos.is_some();
        let chest_held_snapshot = vx.chest_held;
        // Furnace UI snapshot. Same shape as the chest snapshot
        // above, plus a clone of the live runtime triple so the
        // panel can draw the fuel + cook bars.
        let furnace_open_pos = vx.furnace_open;
        let furnace_snapshot: Option<container::Container> = vx
            .furnace_open
            .as_ref()
            .and_then(|p| vx.furnaces.get(p).cloned());
        let furnace_runtime_snapshot: block::furnace::FurnaceRuntime = vx
            .furnace_open
            .as_ref()
            .and_then(|p| vx.furnace_runtimes.get(p).copied())
            .unwrap_or_default();
        let mut furnace_open_local = furnace_open_pos.is_some();
        let furnace_held_snapshot = vx.furnace_held;
        let mut furnace_grid_actions: Vec<(bool, container::ui::GridAction)> = Vec::new();
        let mut furnace_held_local = furnace_held_snapshot;
        // W50.3: 3x3 crafting table snapshot. Local copies that the
        // panel mutates inside the egui closure; reconciled with vx
        // after the closure exits.
        let crafting_table_open_pos = vx.crafting_table_open;
        let mut crafting_table_open_local = crafting_table_open_pos.is_some();
        let mut crafting_table_grid_local: [Option<(BlockId, u8)>; 9] =
            vx.craft_grid_3x3;
        let mut crafting_table_held_local = vx.crafting_table_held;
        // Set when the player clicks the 3x3 output slot.
        let mut crafting_table_take_output = false;
        // W171: stonecutter snapshot (client-side, like the crafting
        // table). The panel mutates the input + reports a clicked cut;
        // reconciled with vx after the egui closure.
        let stonecutter_open_pos = vx.stonecutter_open;
        let mut stonecutter_open_local = stonecutter_open_pos.is_some();
        let mut stonecutter_input_local = vx.stonecutter_input;
        let mut stonecutter_craft: Option<usize> = None;
        // W50.5: brewing stand snapshot mirrors the chest/furnace
        // pattern - clone the live container so the panel paints
        // a stable view; collected actions land on vx after the
        // egui closure exits.
        let brewing_open_pos = vx.brewing_open;
        let brewing_snapshot: Option<container::Container> = vx
            .brewing_open
            .as_ref()
            .and_then(|p| vx.brewing_stands.get(p).cloned());
        let brewing_runtime_snapshot: block::brewing::BrewingRuntime = vx
            .brewing_open
            .as_ref()
            .and_then(|p| vx.brewing_runtimes.get(p).copied())
            .unwrap_or_default();
        let mut brewing_open_local = brewing_open_pos.is_some();
        let brewing_held_snapshot = vx.brewing_held;
        let mut brewing_held_local = brewing_held_snapshot;
        let mut brewing_grid_actions: Vec<(bool, container::ui::GridAction)> =
            Vec::new();
        // Plain inventory snapshot as Option<(u16,u8)> for the
        // chest-UI's player-side grid - independent of the
        // existing DnD widget snapshot which carries icon meta.
        //
        // W50.3: mutable so the 3x3 crafting table panel can
        // apply player-inv clicks in place; render.rs copies the
        // result back into vx.inventory after the egui closure
        // when the crafting table was the active panel.
        let mut inv_plain_snapshot: Vec<Option<(u16, u8)>> = vx
            .inventory
            .iter()
            .map(|s| s.map(|(b, c)| (b.0, c)))
            .collect();
        let mp_role_label: Option<&'static str> = match &vx.net {
            NetRole::Offline => None,
            NetRole::Server { .. } => Some("hosting server"),
            NetRole::Client { .. } => Some("connected to server"),
        };
        let inventory_snapshot: Vec<Option<SlotIcon>> = if inventory_open {
            vx.inventory[HOTBAR_SLOTS..]
                .iter()
                .map(|s| icon(*s))
                .collect()
        } else {
            Vec::new()
        };
        let craft_snapshot: [Option<SlotIcon>; 4] =
            std::array::from_fn(|i| icon(vx.craft_grid[i]));
        let craft_match: Option<(BlockId, u8, &'static str)> =
            match_recipe_named(&vx.craft_grid);
        let craft_output_snapshot = craft_match.map(|(b, c, _)| {
            let bt = block_type(b.0);
            // W20.2: use icon_uv_front for MC atlas compatibility.
            let uv = vx.resource_pack.icon_uv_front(b);
            // W153: same item-only fallback as the `icon` closure.
            match vx.registry.get(b) {
                Some(d) => (d.color, c, uv, bt.map(|t| t.name).unwrap_or("?")),
                None => (
                    [0.78, 0.78, 0.78],
                    c,
                    uv,
                    crate::item::item_type(b.0).map(|it| it.name).unwrap_or("?"),
                ),
            }
        });
        let craft_recipe_name: Option<&'static str> = craft_match.map(|(_, _, n)| n);
        let selected_slot = vx.selected_slot;
        // Click actions to apply after the egui closure exits (the
        // closure can't mutate `vx` directly because the renderer
        // already borrowed it).
        let mut inv_actions: Vec<InvAction> = Vec::new();
        // Chest-UI side channels. The egui closure pushes one
        // `(side, GridAction)` per click; we drain them post-egui
        // and route to `block::chest::apply_chest_action` /
        // `apply_player_action`. `side` = false means a click on
        // the chest grid, `true` = a click on the player-inv grid.
        // W55.2b: partner-chest action queue for double chests.
        let mut chest_partner_actions: Vec<container::ui::GridAction> =
            Vec::new();
        let mut chest_grid_actions: Vec<(bool, container::ui::GridAction)> =
            Vec::new();
        let mut chest_held_local = chest_held_snapshot;
        let mut action_resume = false;
        // Set by the recipe-book output slot click handler; drained
        // after the egui closure exits so the craft-grid mutation
        // can borrow `vx` exclusively.
        let mut prefill_recipe_idx: Option<usize> = None;
        // W20.4: recipe book search query snapshot.
        let mut recipe_book_query_local = vx.recipe_book_query.clone();
        let mut action_pause_to_settings = false;
        let mut action_back_settings = false;
        let mut action_main_menu = false;
        // MP overlay actions: continue playing (close overlay) /
        // explicit disconnect-and-return-to-main-menu.
        let mut action_mp_resume = false;
        let mut action_mp_disconnect = false;
        let mut action_mp_quit = false;
        let current_slug = vx.current_world_slug.clone();
        let browser_servers = &vx.browser_servers;
        let browser_error = &vx.browser_error;
        let browser_querying = vx.browser_query_rx.is_some();
        // Wave 4 menu wiring. The closure mutates `menu` directly
        // (form buffers, screen variant) + pushes actions into
        // `menu_actions` for the outer apply pass.
        let mut menu_actions: Vec<menu::MenuAction> = Vec::new();
        // W5.1: menu HUD shows the active world's effective seed.
        let menu_world_seed = active_world_seed;
        let menu_engine_version = env!("CARGO_PKG_VERSION");
        // Snapshot the menu's caches as locals so the closure can
        // borrow them alongside the disjoint `&mut menu.screen` +
        // `&mut menu.options` mutable borrows without tripping
        // Rust's aliasing rules.
        let worlds_snapshot: Vec<world_save::layout::WorldSummary> =
            vx.menu.worlds_cache.clone();
        let saved_servers_snapshot: Vec<menu::SavedServer> =
            vx.menu.saved_servers.clone();
        // W21.1: snapshot splash text before borrowing so the
        // closure can carry it into MenuContext without a live
        // borrow on `vx.menu.splash_text`.
        let menu_splash_text: Option<String> = vx.menu.splash_text.clone();
        // W21.4: resource-pack display info for the footer.
        let menu_pack_block_count = if vx.resource_pack.is_fallback {
            0usize
        } else {
            vx.resource_pack.textures.len()
        };
        let menu_pack_name: String = if vx.resource_pack.is_fallback {
            String::new()
        } else {
            vx.resource_pack
                .source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Vanilla")
                .to_string()
        };
        // Split `&mut vx.menu` into two disjoint `&mut` borrows
        // of `screen` + `options` so the dispatcher can take one
        // while the `MenuContext` carries the other.
        let menu::Menu {
            screen: menu_screen_ref,
            options: menu_options_ref,
            ..
        } = &mut vx.menu;
        // W11.5: snapshot of every known player name + the local
        // player. Built up here so the autocomplete CompletionContext
        // can pull from a Vec<String> the closure owns - avoids
        // re-borrowing `vx.player_names` inside the egui callback.
        let mut online_players: Vec<String> =
            vx.player_names.values().cloned().collect();
        if !vx.player_name.is_empty()
            && !online_players.iter().any(|n| n == &vx.player_name)
        {
            online_players.push(vx.player_name.clone());
        }
        online_players.sort_by_key(|n| n.to_lowercase());
        online_players.dedup();
        // Mutable borrow for the chat input box captured by the
        // egui closure. Taken last so earlier vx.* reads
        // (raycast_mob, registry, etc.) finish before this
        // exclusive borrow begins.
        let chat_buf = &mut vx.chat_input;
        let chat_history_ref = &vx.chat_history;
        let chat_history_idx = &mut vx.chat_history_idx;
        let chat_completion = &mut vx.chat_completion;
        let player_name_buf = &mut vx.player_name;

        let Some(ui) = vx.ui.as_mut() else {
            return;
        };
        ui.run(window.raw(), |ctx| match state {
            // Wave 4 (Phase G.6): all four pre-Play AppStates now
            // funnel through the menu module. The new state
            // machine carries its own screen variant inside
            // `menu_ref.screen`; AppState only flips to Playing /
            // Paused / Settings when the user actually starts the
            // game. The DirectConnect / ServerBrowser / HostSetup
            // variants survive on `AppState` purely as legacy
            // type-tags so the rest of the state-transition logic
            // (Esc handler, Pause path) doesn't need to change.
            AppState::MainMenu
            | AppState::DirectConnect
            | AppState::ServerBrowser
            | AppState::HostSetup => {
                // Menu backdrop + draw. Use a background-layer painter
                // for the dark fill + panorama, then an egui Area to
                // host the menu widgets. CentralPanel produced an
                // empty-area ui under this egui version + window
                // setup; Area paints reliably.
                let panorama_tex = panorama_egui_tex;
                let scroll = panorama_scroll_local;
                let screen = ctx.content_rect();
                let bg_layer = egui::LayerId::new(
                    egui::Order::Background,
                    egui::Id::new("menu_bg"),
                );
                let bg_painter = ctx.layer_painter(bg_layer);
                bg_painter.rect_filled(
                    screen,
                    0.0,
                    egui::Color32::from_rgb(13, 15, 26),
                );
                if let Some(tex_id) = panorama_tex {
                    let u0 = scroll.rem_euclid(1.0);
                    let uv = egui::Rect::from_min_max(
                        egui::pos2(u0, 0.0),
                        egui::pos2(u0 + 0.25, 1.0),
                    );
                    bg_painter.image(
                        tex_id,
                        screen,
                        uv,
                        egui::Color32::WHITE,
                    );
                    bg_painter.rect_filled(
                        screen,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 110),
                    );
                }
                egui::Area::new(egui::Id::new("menu_root_area"))
                    .anchor(egui::Align2::CENTER_TOP, [0.0, 40.0])
                    .show(ctx, |ui| {
                        ui.set_min_width(420.0);
                        let mut mctx = menu::MenuContext {
                            worlds: &worlds_snapshot,
                            browser_servers,
                            browser_error: browser_error.as_deref(),
                            saved_servers: &saved_servers_snapshot,
                            options: menu_options_ref,
                            current_slug: current_slug.as_deref(),
                            world_seed: menu_world_seed,
                            engine_version: menu_engine_version,
                            browser_querying,
                            // W21.1: splash text snapshot.
                            splash_text: menu_splash_text
                                .as_deref(),
                            // W21.4: pack info for footer.
                            pack_name: &menu_pack_name,
                            pack_block_count: menu_pack_block_count,
                        };
                        menu::draw(
                            ui,
                            menu_screen_ref,
                            &mut mctx,
                            &mut menu_actions,
                        );
                    });
            }
            // L.3: loading screen. Same Area approach as the main
            // menu (CentralPanel causes a blank-frame artefact with
            // this egui version + window setup).
            AppState::Loading {
                target_chunks,
                loaded_chunks,
                phase,
                started_at,
            } => {
                let screen = ctx.content_rect();
                let bg_layer = egui::LayerId::new(
                    egui::Order::Background,
                    egui::Id::new("loading_bg"),
                );
                ctx.layer_painter(bg_layer).rect_filled(
                    screen,
                    0.0,
                    egui::Color32::from_rgb(10, 13, 23),
                );
                egui::Area::new(egui::Id::new("loading_area"))
                    .anchor(egui::Align2::CENTER_TOP, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.set_min_width(480.0);
                        menu::loading::draw(
                            ui,
                            target_chunks,
                            loaded_chunks,
                            phase,
                            started_at,
                        );
                    });
            }
            AppState::Paused => {
                // W21.5: delegate to menu::pause::draw so the
                // pause overlay uses the same MC-style buttons as
                // the main menu.
                let mut pause_actions: Vec<menu::pause::PauseAction> =
                    Vec::new();
                menu::pause::draw(ctx, menu_world_seed, &mut pause_actions);
                for pa in pause_actions {
                    use menu::pause::PauseAction;
                    match pa {
                        PauseAction::Resume => {
                            action_resume = true;
                        }
                        PauseAction::Settings => {
                            action_pause_to_settings = true;
                        }
                        PauseAction::MainMenu => {
                            action_main_menu = true;
                        }
                    }
                }
            }
            AppState::Settings { .. } => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.horizontal(|ui| {
                        ui.label("Player name:");
                        ui.text_edit_singleline(player_name_buf);
                    });
                    ui.add(
                        egui::Slider::new(&mut settings_edit.mouse_sensitivity, 0.1..=2.0)
                            .text("Mouse sensitivity"),
                    );
                    ui.add(
                        egui::Slider::new(
                            &mut settings_edit.view_distance,
                            2..=VIEW_DISTANCE_MAX as u8,
                        )
                        .step_by(1.0)
                        .text("Render distance (chunks)"),
                    );
                    ui.checkbox(&mut settings_edit.show_fps, "Show FPS");
                    ui.checkbox(
                        &mut settings_edit.lock_cursor,
                        "Lock cursor in window (Play)",
                    );
                    ui.checkbox(&mut settings_edit.hide_cursor, "Hide cursor (Play)");
                    ui.checkbox(&mut settings_edit.head_bob, "Camera head bob");
                    ui.checkbox(
                        &mut settings_edit.invert_mouse_y,
                        "Invert mouse Y",
                    );
                    ui.checkbox(
                        &mut settings_edit.invert_wheel,
                        "Invert hotbar scroll",
                    );
                    ui.separator();
                    if ui.button("Back").clicked() {
                        action_back_settings = true;
                    }
                });
            }
            AppState::Playing => {
                if hud_visible {
                egui::Area::new(egui::Id::new("voxel_hud"))
                    .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label("Mouse aim, WASD move, Space jump");
                            ui.label("Shift sneak, Ctrl sprint");
                            ui.label("LMB break, RMB place, E inventory, F eat, Esc menu");
                            ui.label("T chat, Tab playerlist, Q drop, Z undo, F1 hide HUD, F3 debug, F5 camera");
                            // Day clock - map [0..DAY_LENGTH_S) onto
                            // 24h so the player has a sense of where
                            // the sun is.
                            let frac = (day_time / DAY_LENGTH_S).rem_euclid(1.0);
                            let hours_total = frac * 24.0;
                            let hh = hours_total.floor() as i32;
                            let mm = ((hours_total - hh as f32) * 60.0) as i32;
                            // Cardinal direction the player is
                            // facing. Yaw=0 -> east (+X); rotate
                            // by quadrants for the standard
                            // N/E/S/W readout.
                            let yaw_norm = dbg_yaw.rem_euclid(std::f32::consts::TAU);
                            let octant = (yaw_norm / std::f32::consts::FRAC_PI_4)
                                .round() as i32
                                & 7;
                            let dir = match octant {
                                0 => "E",
                                1 => "NE",
                                2 => "N",
                                3 => "NW",
                                4 => "W",
                                5 => "SW",
                                6 => "S",
                                _ => "SE",
                            };
                            ui.label(format!(
                                "Time: {hh:02}:{mm:02}    Kills: {kill_count}    Facing: {dir}    Biome: {biome_name}{}",
                                match weather_label {
                                    Some(w) => format!("    Weather: {w}"),
                                    None => String::new(),
                                }
                            ));
                            ui.label(format!(
                                "XYZ: {:.0}, {:.0}, {:.0}",
                                dbg_player.x,
                                dbg_player.y,
                                dbg_player.z
                            ));
                            if let Some(tool) = held_tool_label {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Held: {tool}"
                                    ))
                                    .color(egui::Color32::from_rgb(
                                        200, 230, 255,
                                    )),
                                );
                            }
                            // Spawn-direction hint: distance to
                            // the configured respawn point + the
                            // approximate compass bearing toward
                            // it. Hidden when standing on spawn.
                            let sp = spawn_xz;
                            let dxs = sp.0 - dbg_player.x;
                            let dzs = sp.1 - dbg_player.z;
                            let dist_sp = (dxs * dxs + dzs * dzs).sqrt();
                            if dist_sp > 4.0 {
                                let yaw_to =
                                    (-dzs).atan2(dxs).rem_euclid(
                                        std::f32::consts::TAU,
                                    );
                                let oct = (yaw_to
                                    / std::f32::consts::FRAC_PI_4)
                                    .round() as i32
                                    & 7;
                                let dir_sp = match oct {
                                    0 => "E",
                                    1 => "NE",
                                    2 => "N",
                                    3 => "NW",
                                    4 => "W",
                                    5 => "SW",
                                    6 => "S",
                                    _ => "SE",
                                };
                                ui.label(format!(
                                    "Spawn: {dir_sp}  {dist_sp:.0}m"
                                ));
                            }
                            // W10.7: hide HP + stamina bars in
                            // Creative - they always read full
                            // (invuln=true), so the row is just
                            // visual noise. Survival keeps them.
                            if !dbg_is_creative {
                            // Health row: 10 hearts of 2 HP each.
                            // A heart is full if `player_health` >=
                            // 2 * (i + 1), half if at the half-step,
                            // empty otherwise.
                            ui.horizontal(|ui| {
                                ui.label("HP:");
                                for i in 0..10 {
                                    let need_full = 2.0 * (i as f32 + 1.0);
                                    let need_half = need_full - 1.0;
                                    let glyph = if player_health >= need_full {
                                        "\u{2665}"
                                    } else if player_health >= need_half {
                                        "\u{2661}"
                                    } else {
                                        " "
                                    };
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 50, 50),
                                        glyph,
                                    );
                                }
                            });
                            // Stamina bar. Width tracks the
                            // current fraction; tint shifts from
                            // green at full down to orange at
                            // low. Hidden when sprinting is
                            // impossible (full + not in use).
                            if stamina_frac < 0.999 || vx.sprinting {
                                ui.horizontal(|ui| {
                                    ui.label("Stamina:");
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(120.0, 8.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    painter.rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_rgba_unmultiplied(
                                            30, 30, 30, 220,
                                        ),
                                    );
                                    let fill = egui::Rect::from_min_size(
                                        rect.min,
                                        egui::vec2(
                                            rect.width() * stamina_frac,
                                            rect.height(),
                                        ),
                                    );
                                    let col = if stamina_frac > 0.5 {
                                        egui::Color32::from_rgb(100, 200, 80)
                                    } else if stamina_frac > 0.2 {
                                        egui::Color32::from_rgb(220, 180, 60)
                                    } else {
                                        egui::Color32::from_rgb(220, 90, 60)
                                    };
                                    painter.rect_filled(fill, 2.0, col);
                                });
                            }
                            // Air bar - only visible while the
                            // breath meter is below full. Drains
                            // submerged, refills on the surface.
                            // Tints from cyan -> orange -> red as
                            // the meter empties.
                            if air_frac < 0.999 {
                                ui.horizontal(|ui| {
                                    ui.label("Air:");
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(120.0, 8.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    painter.rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_rgba_unmultiplied(
                                            30, 30, 30, 220,
                                        ),
                                    );
                                    let fill = egui::Rect::from_min_size(
                                        rect.min,
                                        egui::vec2(
                                            rect.width() * air_frac,
                                            rect.height(),
                                        ),
                                    );
                                    let col = if air_frac > 0.5 {
                                        egui::Color32::from_rgb(100, 180, 230)
                                    } else if air_frac > 0.2 {
                                        egui::Color32::from_rgb(220, 180, 60)
                                    } else {
                                        egui::Color32::from_rgb(220, 60, 60)
                                    };
                                    painter.rect_filled(fill, 2.0, col);
                                });
                            }
                            } // W10.7: end of `!dbg_is_creative` HP/stamina/air block.
                            // W10.7: Creative gets a small mode
                            // chip in place of the HP rows so the
                            // player can still spot the active
                            // mode at a glance. Also doubles as the
                            // "Fly" status indicator when flying.
                            if dbg_is_creative {
                                let fly_tag = if dbg_fly_state {
                                    " (Fly)"
                                } else {
                                    ""
                                };
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 215, 80),
                                    format!("Mode: Creative{fly_tag}"),
                                );
                            }
                            if show_fps {
                                ui.separator();
                                ui.label(format!("FPS: {fps:.0}"));
                                // W198: per-phase ms breakdown (updated
                                // ~2.5s) so a perf report is one screenshot.
                                ui.separator();
                                ui.label(format!(
                                    "ms r{:.1} sim{:.1} str{:.1} liq{:.1} dq{:.1} mob{:.1}",
                                    crate::perf::load(&crate::perf::RENDER_MS),
                                    crate::perf::load(&crate::perf::SIM_MS),
                                    crate::perf::load(&crate::perf::STREAM_MS),
                                    crate::perf::load(&crate::perf::LIQUIDS_MS),
                                    crate::perf::load(&crate::perf::DRAWQ_MS),
                                    crate::perf::load(&crate::perf::MOBS_MS),
                                ));
                            }
                            // Phase F.7: show the active dim so
                            // the player has feedback after a
                            // /dim or portal teleport. Cheap
                            // ASCII; no separator unless FPS is
                            // visible (keeps the HUD tidy).
                            if show_fps {
                                ui.separator();
                            }
                            ui.label(format!("Dim: {dim}"));
                        });
                    });
                }
                // Multiplayer in-game menu. Opens on Esc when a
                // net role is active. World keeps simulating
                // underneath (other peers must not see a frozen
                // ghost), so this is a thin overlay rather than a
                // pause state.
                if mp_menu_open
                    && let Some(role) = mp_role_label
                {
                    egui::Window::new("Game menu")
                        .open(&mut mp_menu_open_local)
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(format!("Status: {role}"));
                            ui.separator();
                            if ui
                                .add_sized([220.0, 32.0], egui::Button::new("Resume"))
                                .clicked()
                            {
                                action_mp_resume = true;
                            }
                            if ui
                                .add_sized(
                                    [220.0, 32.0],
                                    egui::Button::new("Disconnect to main menu"),
                                )
                                .clicked()
                            {
                                action_mp_disconnect = true;
                            }
                            if ui
                                .add_sized([220.0, 32.0], egui::Button::new("Quit"))
                                .clicked()
                            {
                                action_mp_quit = true;
                            }
                        });
                }
                // F3 debug panel: top-right corner, lots of state in
                // one place. Mirrors Minecraft's debug overlay.
                // F6 live debug panel: sliders + toggles for
                // common runtime tweaks. Lives in its own
                // resizable window so it's draggable.
                if dbg_panel_was_open {
                    egui::Window::new("Debug panel (F6)")
                        .open(&mut dbg_panel_open)
                        .collapsible(true)
                        .resizable(true)
                        .default_pos([20.0, 180.0])
                        .default_size([280.0, 320.0])
                        .show(ctx, |ui| {
                            // Time of day slider 0..24h.
                            let mut hours =
                                (*dbg_day_time / DAY_LENGTH_S) * 24.0;
                            if ui
                                .add(
                                    egui::Slider::new(&mut hours, 0.0..=24.0)
                                        .text("Time (h)")
                                        .step_by(0.01),
                                )
                                .changed()
                            {
                                *dbg_day_time = (hours / 24.0)
                                    * DAY_LENGTH_S;
                            }
                            ui.checkbox(dbg_freeze_time, "Freeze time");
                            // Weather dropdown.
                            ui.horizontal(|ui| {
                                ui.label("Weather:");
                                ui.selectable_value(
                                    dbg_weather,
                                    0,
                                    "Clear",
                                );
                                ui.selectable_value(
                                    dbg_weather,
                                    1,
                                    "Rain",
                                );
                                ui.selectable_value(
                                    dbg_weather,
                                    2,
                                    "Snow",
                                );
                            });
                            // FOV slider in degrees. step_by(1.0) prevents
                            // the 1-frame display lag on integer snapping.
                            let mut fov_deg = dbg_fov_rad.to_degrees();
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut fov_deg,
                                        30.0..=110.0,
                                    )
                                    .step_by(1.0)
                                    .text("FOV"),
                                )
                                .changed()
                            {
                                *dbg_fov_rad = fov_deg.to_radians();
                            }
                            ui.add(
                                egui::Slider::new(
                                    &mut settings_edit.view_distance,
                                    2..=VIEW_DISTANCE_MAX as u8,
                                )
                                .step_by(1.0)
                                .text("Render distance"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    dbg_mob_cap,
                                    0..=64,
                                )
                                .text("Mob cap"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    dbg_snow_rate,
                                    0.05..=2.0,
                                )
                                .text("Snow density (s/layer)"),
                            );
                            ui.checkbox(dbg_fly_mode, "Fly mode");
                            ui.checkbox(dbg_god_mode, "Godmode");
                            ui.checkbox(
                                dbg_liquid_bias,
                                "Liquid drop bias (pathfinding)",
                            );
                            ui.separator();
                            ui.label(
                                "Hotkeys: F3 stats, F4 weather, F5 camera, G chunk grid",
                            );
                        });
                }
                if show_debug {
                    egui::Area::new(egui::Id::new("voxel_debug"))
                        .anchor(egui::Align2::RIGHT_TOP, [-12.0, 12.0])
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.label("[F3] Debug");
                                ui.separator();
                                ui.label(format!(
                                    "pos: {:.2}, {:.2}, {:.2}",
                                    dbg_player.x, dbg_player.y, dbg_player.z
                                ));
                                ui.label(format!(
                                    "look: yaw {:.2} pitch {:.2}",
                                    dbg_yaw, dbg_pitch
                                ));
                                ui.label(format!(
                                    "grounded: {}",
                                    if dbg_grounded { "yes" } else { "no" }
                                ));
                                ui.label(format!("chunk meshes: {dbg_chunks}"));
                                ui.label(format!("pending chunks: {dbg_pending}"));
                                ui.label(format!("mobs (remote): {dbg_mobs}"));
                                // H.7 mob HUD: count vs cap, nearest
                                // kind + distance, per-kind tally.
                                ui.label(&dbg_mob_count_line);
                                if !dbg_mob_kinds_line.is_empty() {
                                    ui.label(&dbg_mob_kinds_line);
                                }
                                ui.label(format!("particles: {dbg_particles}"));
                                ui.label(format!(
                                    "camera: {dbg_cam}{}",
                                    if dbg_fly { "  fly" } else { "" }
                                ));
                                ui.label(format!("fps: {fps:.0}"));
                                ui.label(format!("net: {dbg_net}"));
                                let h = dbg_playtime / 3600;
                                let m = (dbg_playtime % 3600) / 60;
                                let s = dbg_playtime % 60;
                                ui.label(format!("session: {h:02}:{m:02}:{s:02}"));
                                ui.label(format!("seed: {dbg_seed}"));
                                // W10.7: current game mode line.
                                ui.label(format!(
                                    "Mode: {}",
                                    dbg_game_mode.label_cs()
                                ));
                            });
                        });
                }
                // Bottom hotbar - 9 normal slots. Mining is driven
                // by LMB regardless of what's held; the held tool
                // only affects mining speed + drops. Highlighted
                // slot is the active selection (driven by mouse
                // wheel). Clicks here change the active slot;
                // drag&drop is wired through the inventory window.
                if hud_visible {
                egui::Area::new(egui::Id::new("voxel_hotbar"))
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0])
                    .show(ctx, |ui| {
                        // W10.7: tint the hotbar frame yellow in
                        // Creative so the player can tell modes
                        // apart at a glance. Survival keeps the
                        // default neutral grey.
                        let mut frame = egui::Frame::popup(ui.style());
                        if dbg_is_creative {
                            frame = frame.stroke(egui::Stroke::new(
                                1.5,
                                egui::Color32::from_rgb(255, 215, 80),
                            ));
                        }
                        frame.show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.horizontal(|ui| {
                                    for (i, slot) in hotbar_snapshot.iter().enumerate() {
                                        let resp = crate::ui::slot_widget::slot_with_dnd_dur(
                                            ui,
                                            *slot,
                                            atlas_tex_id,
                                            SlotKind::Inv(i),
                                            i == selected_slot,
                                            inventory_open,
                                            &mut inv_actions,
                                            durability_for_slot(i),
                                        );
                                        if resp.clicked() {
                                            inv_actions.push(InvAction::SelectHotbar(i));
                                        }
                                    }
                                });
                                // Row of digit hints under each
                                // slot. Highlights the active slot
                                // so the player can map their key
                                // press to the selected stack at a
                                // glance.
                                ui.horizontal(|ui| {
                                    for i in 0..HOTBAR_SLOTS {
                                        let label = if i < 9 {
                                            format!("{}", i + 1)
                                        } else {
                                            String::new()
                                        };
                                        let color = if i == selected_slot {
                                            egui::Color32::from_rgb(255, 220, 100)
                                        } else {
                                            egui::Color32::from_rgb(170, 170, 170)
                                        };
                                        ui.add_sized(
                                            [38.0, 14.0],
                                            egui::Label::new(
                                                egui::RichText::new(label)
                                                    .color(color)
                                                    .monospace(),
                                            ),
                                        );
                                    }
                                });
                            });
                        });
                    });
                }

                // Inventory overlay: 3 extra rows of slots + a hotbar
                // mirror + a 2x2 crafting grid + a 1-slot output.
                // Only drawn when the player has the menu open. The
                // Window's title bar is the only drag handle (egui
                // default behaviour with `.resizable(false)`).
                if inventory_open {
                    let screen = ctx.content_rect();
                    egui::Window::new("Inventory")
                        .open(&mut inv_open_local)
                        .collapsible(false)
                        .resizable(false)
                        .default_pos([
                            screen.center().x - 220.0,
                            screen.center().y - 220.0,
                        ])
                        .default_size([440.0, 420.0])
                        .show(ctx, |ui| {
                            ui.label("E to close - drag stacks between slots");
                            ui.separator();
                            // Storage rows (rows 1..INVENTORY_ROWS).
                            for row in 0..(INVENTORY_ROWS - 1) {
                                ui.horizontal(|ui| {
                                    for col in 0..INVENTORY_COLS {
                                        let snap_idx = row * INVENTORY_COLS + col;
                                        let inv_idx = HOTBAR_SLOTS + snap_idx;
                                        let slot = inventory_snapshot
                                            .get(snap_idx)
                                            .copied()
                                            .unwrap_or(None);
                                        crate::ui::slot_widget::slot_with_dnd_dur(
                                            ui,
                                            slot,
                                            atlas_tex_id,
                                            SlotKind::Inv(inv_idx),
                                            false,
                                            true,
                                            &mut inv_actions,
                                            durability_for_slot(inv_idx),
                                        );
                                    }
                                });
                            }
                            ui.add_space(8.0);
                            // Hotbar row reflected inside the window
                            // so the player can shuffle stacks
                            // between storage and the hotbar without
                            // closing the menu (matches Minecraft).
                            ui.horizontal(|ui| {
                                for (i, slot) in hotbar_snapshot.iter().enumerate() {
                                    let resp = crate::ui::slot_widget::slot_with_dnd_dur(
                                        ui,
                                        *slot,
                                        atlas_tex_id,
                                        SlotKind::Inv(i),
                                        i == selected_slot,
                                        true,
                                        &mut inv_actions,
                                        durability_for_slot(i),
                                    );
                                    if resp.clicked() {
                                        inv_actions.push(InvAction::SelectHotbar(i));
                                    }
                                }
                            });
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label("Craft (2x2)");
                                    for row in 0..2 {
                                        ui.horizontal(|ui| {
                                            for col in 0..2 {
                                                let idx = row * 2 + col;
                                                crate::ui::slot_widget::slot_with_dnd(
                                                    ui,
                                                    craft_snapshot[idx],
                                                    atlas_tex_id,
                                                    SlotKind::Craft(idx),
                                                    false,
                                                    true,
                                                    &mut inv_actions,
                                                );
                                            }
                                        });
                                    }
                                });
                                ui.vertical(|ui| {
                                    ui.add_space(20.0);
                                    ui.label(" ->");
                                });
                                ui.vertical(|ui| {
                                    ui.label("Output");
                                    let resp = crate::ui::slot_widget::slot_with_dnd(
                                        ui,
                                        craft_output_snapshot,
                                        atlas_tex_id,
                                        SlotKind::CraftOut,
                                        false,
                                        true,
                                        &mut inv_actions,
                                    );
                                    if let Some(name) = craft_recipe_name {
                                        resp.on_hover_text(name);
                                    }
                                });
                            });
                            // Carry preview is unnecessary - egui's
                            // dnd_drag_source paints the dragged
                            // stack at the cursor while held. The
                            // recipe book lives in its own window
                            // beside this one (see below) so it can
                            // grow / scroll without pushing the
                            // crafting grid off-screen.
                        });
                    // Recipe book as a separate beside-inventory
                    // window. Resizable + scrollable so it can fit
                    // the full recipe table even on a small display
                    // without cramping the Inventory layout.
                    let rb_default = [
                        screen.center().x + 230.0,
                        screen.center().y - 220.0,
                    ];
                    egui::Window::new("Recipe book")
                        .collapsible(true)
                        .resizable(true)
                        .default_pos(rb_default)
                        .default_size([320.0, 420.0])
                        .min_width(280.0)
                        .min_height(200.0)
                        .show(ctx, |ui| {
                            ui.label(
                                "Drop blocks into the 2x2 grid to craft. \
                                 Hold Shift for half-stacks, Ctrl/Cmd for one.",
                            );
                            // W20.4: show which recipe matches the current
                            // craft grid (if any).
                            if let Some(name) = craft_recipe_name {
                                ui.horizontal(|ui| {
                                    ui.label("Crafting:");
                                    ui.strong(name);
                                });
                            }
                            ui.separator();
                            // W20.4: search box - filters recipes by output
                            // item display name.
                            ui.horizontal(|ui| {
                                ui.label("Hledat:");
                                ui.text_edit_singleline(
                                    &mut recipe_book_query_local,
                                );
                            });
                            ui.separator();
                            // Build the filtered + grouped recipe list.
                            // Group by derived category (output item's
                            // ItemCategory). Within each group recipes
                            // are shown in RECIPES table order.
                            let q = recipe_book_query_local
                                .trim()
                                .to_lowercase();
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // W20.4: category headers. Group
                                    // recipes by ItemCategory of output.
                                    // Categories appear in the order
                                    // they first occur in RECIPES.
                                    use crate::item::ItemCategory;
                                    let cats = [
                                        ("Bloky",     ItemCategory::BuildingBlock),
                                        ("Dekorace",  ItemCategory::Decoration),
                                        ("Nastroje",  ItemCategory::Tool),
                                        ("Zbrane",    ItemCategory::Weapon),
                                        ("Jidlo",     ItemCategory::Food),
                                        ("Ostatni",   ItemCategory::Bucket),
                                        ("Ostatni",   ItemCategory::Ingredient),
                                    ];
                                    // Collect unique category labels in
                                    // first-seen order so the header
                                    // sequence is deterministic.
                                    let mut seen_cats: Vec<&str> = Vec::new();
                                    for (ri, r) in RECIPES.iter().enumerate() {
                                        let ic = crate::item::item_category(
                                            r.output.0,
                                        );
                                        let cat_label = cats
                                            .iter()
                                            .find(|(_, c)| *c == ic)
                                            .map(|(l, _)| *l)
                                            .unwrap_or("Ostatni");
                                        // W124: search the multilingual
                                        // haystack (localized + English
                                        // + raw names + keywords).
                                        let pass_filter = q.is_empty()
                                            || crate::lang::search_haystack(
                                                r.output.0,
                                            )
                                            .contains(&q);
                                        if !pass_filter {
                                            continue;
                                        }
                                        // Emit a header the first time
                                        // this category label appears.
                                        if !seen_cats.contains(&cat_label) {
                                            seen_cats.push(cat_label);
                                            if !seen_cats.is_empty() {
                                                ui.separator();
                                            }
                                            ui.strong(cat_label);
                                        }
                                        ui.horizontal(|ui| {
                                            for (bid, n) in r.inputs {
                                                let bt = block_type(*bid);
                                                // W20.2: icon_uv_front prefers
                                                // the live MC atlas entry.
                                                let uv = vx
                                                    .resource_pack
                                                    .icon_uv_front(BlockId(*bid));
                                                // W153: item-only fallback
                                                // (no block row != no icon).
                                                let icon = Some(match vx
                                                    .registry
                                                    .get(BlockId(*bid))
                                                {
                                                    Some(d) => (
                                                        d.color,
                                                        *n,
                                                        uv,
                                                        bt.map(|t| t.name)
                                                            .unwrap_or("?"),
                                                    ),
                                                    None => (
                                                        [0.78, 0.78, 0.78],
                                                        *n,
                                                        uv,
                                                        crate::item::item_type(*bid)
                                                            .map(|it| it.name)
                                                            .unwrap_or("?"),
                                                    ),
                                                });
                                                crate::ui::slot_widget::draw_slot_widget(
                                                    ui,
                                                    icon,
                                                    atlas_tex_id,
                                                    false,
                                                    egui::Sense::hover(),
                                                );
                                            }
                                            ui.label(" -> ");
                                            let out_bt = block_type(r.output.0);
                                            // W20.2: icon_uv_front for recipe
                                            // output icon.
                                            let out_uv = vx
                                                .resource_pack
                                                .icon_uv_front(BlockId(r.output.0));
                                            // W153: item-only fallback.
                                            let out_icon = Some(match vx
                                                .registry
                                                .get(BlockId(r.output.0))
                                            {
                                                Some(d) => (
                                                    d.color,
                                                    r.output.1,
                                                    out_uv,
                                                    out_bt
                                                        .map(|t| t.name)
                                                        .unwrap_or("?"),
                                                ),
                                                None => (
                                                    [0.78, 0.78, 0.78],
                                                    r.output.1,
                                                    out_uv,
                                                    crate::item::item_type(r.output.0)
                                                        .map(|it| it.name)
                                                        .unwrap_or("?"),
                                                ),
                                            });
                                            // W20.4: highlight the recipe
                                            // that currently matches the
                                            // craft grid.
                                            let is_active = craft_recipe_name
                                                == Some(r.name);
                                            let out_resp = crate::ui::slot_widget::draw_slot_widget(
                                                ui,
                                                out_icon,
                                                atlas_tex_id,
                                                is_active,
                                                egui::Sense::click(),
                                            )
                                            .on_hover_text(format!(
                                                "{} - click to fill craft grid",
                                                r.name
                                            ));
                                            if out_resp.clicked() {
                                                prefill_recipe_idx = Some(ri);
                                            }
                                            ui.label(r.name);
                                        });
                                    }
                                });
                        });
                }

                // W10.6: Creative-mode searchable tab. Opens on E
                // when `abilities.mayfly = true`. Layout: search
                // box at the top, scrollable grid of every item
                // below, a row of hotbar previews at the bottom.
                if creative_tab_open {
                    let screen = ctx.content_rect();
                    let mut open_local = true;
                    egui::Window::new("Creative")
                        .open(&mut open_local)
                        .collapsible(false)
                        .resizable(true)
                        .default_pos([
                            screen.center().x - 230.0,
                            screen.center().y - 240.0,
                        ])
                        .default_size([460.0, 480.0])
                        .min_width(420.0)
                        .min_height(360.0)
                        .show(ctx, |ui| {
                            ui.label("E or Esc to close - klik na polozku ji vlozi do hotbaru");
                            ui.horizontal(|ui| {
                                ui.label("Hledat:");
                                // W20.3: request keyboard focus on the search
                                // box on the first frame after the tab opens.
                                let resp =
                                    ui.text_edit_singleline(&mut creative_query_local);
                                if creative_just_opened {
                                    resp.request_focus();
                                }
                            });
                            // W12.9 tab strip: All / Blocks / Decoration /
                            // Tools / Combat / Food / Misc. Click to filter.
                            ui.horizontal(|ui| {
                                for cat in creative_tab::CreativeCategory::all() {
                                    let selected = creative_cat_local == cat;
                                    if ui.selectable_label(selected, cat.label()).clicked() {
                                        creative_cat_local = cat;
                                    }
                                }
                            });
                            ui.separator();
                            let entries = creative_tab::available_items();
                            // W20.3: sort filtered entries by display name
                            // within the active category.
                            let mut filtered = creative_tab::filter_items_in_category(
                                &entries,
                                creative_cat_local,
                                &creative_query_local,
                            );
                            filtered.sort_by(|a, b| {
                                crate::lang::localized_item_name(a.id)
                                    .cmp(&crate::lang::localized_item_name(b.id))
                            });
                            egui::ScrollArea::vertical()
                                .max_height(360.0)
                                .show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        for entry in filtered.iter() {
                                            // W20.2/W20.3: icon resolve via
                                            // icon_uv_front (MC atlas first,
                                            // 4x4 fallback). Falls back to a
                                            // tinted swatch via the
                                            // draw_slot_widget code path.
                                            let icon_slot: Option<SlotIcon> = {
                                                let bt = block_type(entry.id);
                                                let uv = vx
                                                    .resource_pack
                                                    .icon_uv_front(BlockId(entry.id));
                                                // W153: item-only ids (hoes,
                                                // bow, arrow, minecart, dyes,
                                                // spawn eggs) have no block
                                                // row - the grid tile used to
                                                // render EMPTY for them.
                                                Some(match vx
                                                    .registry
                                                    .get(BlockId(entry.id))
                                                {
                                                    Some(d) => (
                                                        d.color,
                                                        1u8,
                                                        uv,
                                                        bt.map(|t| t.name).unwrap_or("?"),
                                                    ),
                                                    None => (
                                                        [0.78, 0.78, 0.78],
                                                        1u8,
                                                        uv,
                                                        crate::item::item_type(entry.id)
                                                            .map(|it| it.name)
                                                            .unwrap_or("?"),
                                                    ),
                                                })
                                            };
                                            let name =
                                                crate::lang::localized_item_name(entry.id);
                                            let resp = crate::ui::slot_widget::draw_slot_widget(
                                                ui,
                                                icon_slot,
                                                atlas_tex_id,
                                                false,
                                                // W20.3: click_and_drag so right-click
                                                // lands in secondary_clicked().
                                                egui::Sense::click_and_drag(),
                                            )
                                            .on_hover_text(format!(
                                                "{} (max stack {})\nLMB: vozmout stack / RMB: vozmout 1",
                                                name, entry.max_stack
                                            ));
                                            // W20.3: LMB = full max_stack
                                            // (existing behaviour).
                                            if resp.clicked() {
                                                creative_picks.push((
                                                    BlockId(entry.id),
                                                    entry.max_stack,
                                                ));
                                            }
                                            // W20.3: RMB = count 1 (matches
                                            // MC creative-tab right-click).
                                            if resp.secondary_clicked() {
                                                creative_picks.push((
                                                    BlockId(entry.id),
                                                    1,
                                                ));
                                            }
                                        }
                                    });
                                });
                        });
                    if !open_local {
                        creative_close_clicked = true;
                    }
                }

                // Chest UI panel. Shows a 27-slot chest grid on
                // top + the player's 36-slot inventory below.
                // Click semantics live in `container::ui::draw_grid`:
                // LMB = take/place, RMB = take half / place one,
                // Shift+LMB = transfer to partner. The cursor-
                // held stack is rendered as a small floating
                // indicator near the cursor; the actual mutation
                // happens after the egui closure exits so the
                // borrow checker is happy.
                if let Some(chest) = &chest_snapshot {
                    let screen = ctx.content_rect();
                    egui::Window::new("Chest")
                        .open(&mut chest_open_local)
                        .collapsible(false)
                        .resizable(false)
                        .default_pos([
                            screen.center().x - 220.0,
                            screen.center().y - 240.0,
                        ])
                        .default_size([440.0, 460.0])
                        .show(ctx, |ui| {
                            ui.label("ESC to close - LMB take/place, RMB half/one, Shift+LMB transfer");
                            if let Some((id, n)) = chest_held_local {
                                ui.label(format!(
                                    "Holding: item {} x{}",
                                    crate::lang::localized_item_name(id),
                                    n
                                ));
                            }
                            ui.separator();
                            // W56.3: id_salt makes each grid's
                            // egui Id unique even though title +
                            // (cols, rows) match across the
                            // partner / main / inv stack.
                            if let Some(partner) = &chest_partner_snapshot {
                                ui.label("Chest (paired - top)");
                                let partner_cfg = container::ui::GridConfig {
                                    cols: 9,
                                    rows: 3,
                                    tile_px: 36.0,
                                    title: None,
                                    id_salt: "chest_partner",
                                };
                                let actions = container::ui::draw_grid_with_uv(
                                    ui,
                                    &partner_cfg,
                                    partner,
                                    chest_held_local,
                                    atlas_tex_id,
                                    &icon_uv_resolver,
                                );
                                for a in actions {
                                    chest_partner_actions.push(a);
                                }
                                ui.label("Chest (paired - bottom)");
                            } else {
                                ui.label("Chest (27)");
                            }
                            let chest_cfg = container::ui::GridConfig {
                                cols: 9,
                                rows: 3,
                                tile_px: 36.0,
                                title: None,
                                id_salt: "chest_main",
                            };
                            let actions = container::ui::draw_grid_with_uv(
                                ui,
                                &chest_cfg,
                                chest,
                                chest_held_local,
                                atlas_tex_id,
                                &icon_uv_resolver,
                            );
                            for a in actions {
                                chest_grid_actions.push((false, a));
                            }
                            ui.separator();
                            ui.label("Inventory (36)");
                            // Inventory rendered through an
                            // on-the-fly Container view so the
                            // generic widget can drive clicks on
                            // it too. The on-screen contents come
                            // straight from `inv_plain_snapshot`,
                            // mirroring the host's authoritative
                            // inventory at the start of the frame.
                            let mut inv_view =
                                container::Container::with_size(
                                    inv_plain_snapshot.len(),
                                );
                            for (i, s) in inv_plain_snapshot.iter().enumerate() {
                                inv_view.set_slot(i, *s);
                            }
                            // W56.3: id_salt for the player-inv
                            // grid below the chest grids.
                            let inv_cfg = container::ui::GridConfig {
                                cols: 9,
                                rows: 4,
                                tile_px: 36.0,
                                title: None,
                                id_salt: "chest_player_inv",
                            };
                            let inv_actions_g = container::ui::draw_grid_with_uv(
                                ui,
                                &inv_cfg,
                                &inv_view,
                                chest_held_local,
                                atlas_tex_id,
                                &icon_uv_resolver,
                            );
                            for a in inv_actions_g {
                                chest_grid_actions.push((true, a));
                            }
                        });
                }

                // Furnace UI panel. Three slots laid out as
                // [input] - flame icon - [fuel] - cook arrow -
                // [output]. The flame bar shows fuel_left /
                // fuel_full; the arrow shows cook_progress /
                // FURNACE_COOK_MS_PER_OUTPUT. The player inv grid
                // sits below, mirroring the chest panel layout.
                if let Some(furnace) = &furnace_snapshot {
                    let screen = ctx.content_rect();
                    egui::Window::new("Furnace")
                        .open(&mut furnace_open_local)
                        .collapsible(false)
                        .resizable(false)
                        .default_pos([
                            screen.center().x - 220.0,
                            screen.center().y - 240.0,
                        ])
                        .default_size([440.0, 380.0])
                        .show(ctx, |ui| {
                            ui.label("ESC to close - LMB take/place, RMB half/one, Shift+LMB transfer");
                            if let Some((id, n)) = furnace_held_local {
                                ui.label(format!(
                                    "Holding: item {} x{}",
                                    crate::lang::localized_item_name(id),
                                    n,
                                ));
                            }
                            ui.separator();
                            // Top row: input slot + fuel slot
                            // + flame icon + cook arrow + output
                            // slot. Render the three slots as
                            // single-slot 1x1 grids so we can
                            // intersperse the bars.
                            ui.horizontal(|ui| {
                                ui.label("Input");
                                let cfg_input = container::ui::GridConfig {
                                    cols: 1, rows: 1, tile_px: 36.0, title: None,
                                    id_salt: "furnace_input",
                                };
                                let mut single = container::Container::with_size(1);
                                single.set_slot(0, furnace.slot(0));
                                let acts = container::ui::draw_grid_with_uv(
                                    ui, &cfg_input, &single, furnace_held_local, atlas_tex_id, &icon_uv_resolver,
                                );
                                for a in acts {
                                    // Remap idx 0 -> furnace
                                    // slot 0 (input).
                                    furnace_grid_actions.push((false, remap_action(a, 0)));
                                }
                                ui.label(" ");
                                // Fuel bar. Vertical filled rect
                                // proportional to fuel_left_ms /
                                // fuel_full_ms.
                                let fuel_frac = if furnace_runtime_snapshot.fuel_full_ms > 0 {
                                    furnace_runtime_snapshot.fuel_left_ms as f32
                                        / furnace_runtime_snapshot.fuel_full_ms as f32
                                } else {
                                    0.0
                                };
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(16.0, 36.0),
                                    egui::Sense::hover(),
                                );
                                let painter = ui.painter_at(rect);
                                painter.rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                                let fill_h = rect.height() * fuel_frac.clamp(0.0, 1.0);
                                let fill_rect = egui::Rect::from_min_max(
                                    egui::pos2(rect.min.x, rect.max.y - fill_h),
                                    rect.max,
                                );
                                painter.rect_filled(
                                    fill_rect,
                                    2.0,
                                    egui::Color32::from_rgb(220, 110, 30),
                                );
                                ui.label("Fuel");
                                let cfg_fuel = container::ui::GridConfig {
                                    cols: 1, rows: 1, tile_px: 36.0, title: None,
                                    id_salt: "furnace_fuel",
                                };
                                let mut single_f = container::Container::with_size(1);
                                single_f.set_slot(0, furnace.slot(1));
                                let acts_f = container::ui::draw_grid_with_uv(
                                    ui, &cfg_fuel, &single_f, furnace_held_local, atlas_tex_id, &icon_uv_resolver,
                                );
                                for a in acts_f {
                                    furnace_grid_actions.push((false, remap_action(a, 1)));
                                }
                                // Cook arrow. Horizontal filled
                                // rect: cook_progress_ms /
                                // FURNACE_COOK_MS_PER_OUTPUT.
                                let cook_frac = furnace_runtime_snapshot.cook_progress_ms as f32
                                    / block::furnace::FURNACE_COOK_MS_PER_OUTPUT as f32;
                                let (arect, _) = ui.allocate_exact_size(
                                    egui::vec2(48.0, 16.0),
                                    egui::Sense::hover(),
                                );
                                let apainter = ui.painter_at(arect);
                                apainter.rect_filled(arect, 2.0, egui::Color32::from_gray(40));
                                let fill_w = arect.width() * cook_frac.clamp(0.0, 1.0);
                                let afill_rect = egui::Rect::from_min_max(
                                    arect.min,
                                    egui::pos2(arect.min.x + fill_w, arect.max.y),
                                );
                                apainter.rect_filled(
                                    afill_rect,
                                    2.0,
                                    egui::Color32::from_rgb(220, 200, 70),
                                );
                                ui.label("Out");
                                let cfg_out = container::ui::GridConfig {
                                    cols: 1, rows: 1, tile_px: 36.0, title: None,
                                    id_salt: "furnace_output",
                                };
                                let mut single_o = container::Container::with_size(1);
                                single_o.set_slot(0, furnace.slot(2));
                                let acts_o = container::ui::draw_grid_with_uv(
                                    ui, &cfg_out, &single_o, furnace_held_local, atlas_tex_id, &icon_uv_resolver,
                                );
                                for a in acts_o {
                                    furnace_grid_actions.push((false, remap_action(a, 2)));
                                }
                            });
                            ui.separator();
                            ui.label("Inventory (36)");
                            let mut inv_view = container::Container::with_size(
                                inv_plain_snapshot.len(),
                            );
                            for (i, s) in inv_plain_snapshot.iter().enumerate() {
                                inv_view.set_slot(i, *s);
                            }
                            let inv_cfg = container::ui::GridConfig {
                                cols: 9,
                                rows: 4,
                                tile_px: 36.0,
                                title: None,
                                id_salt: "furnace_player_inv",
                            };
                            let inv_actions_g = container::ui::draw_grid_with_uv(
                                ui,
                                &inv_cfg,
                                &inv_view,
                                furnace_held_local,
                                atlas_tex_id,
                                &icon_uv_resolver,
                            );
                            for a in inv_actions_g {
                                furnace_grid_actions.push((true, a));
                            }
                        });
                }

                // W50.3: 3x3 crafting table panel. Client-side
                // only - the grid lives on Voxel directly so the
                // panel doesn't snapshot from a HashMap. Player
                // inventory clicks below the grid funnel through
                // the same chest_grid_actions pipeline so the
                // existing player-side mutations apply unchanged.
                if crafting_table_open_pos.is_some() {
                    ui::crafting_panel::render(
                        ui::crafting_panel::CraftingPanelInputs {
                            ctx,
                            grid: &mut crafting_table_grid_local,
                            held: &mut crafting_table_held_local,
                            inv_view: &mut inv_plain_snapshot,
                            atlas_tex_id,
                            icon_uv_resolver: &icon_uv_resolver,
                            open_local: &mut crafting_table_open_local,
                            take_output_out: &mut crafting_table_take_output,
                        },
                    );
                }

                // W171: stonecutter cut-select panel.
                if stonecutter_open_pos.is_some() {
                    ui::stonecutter_panel::render(
                        ui::stonecutter_panel::StonecutterPanelInputs {
                            ctx,
                            input: &mut stonecutter_input_local,
                            inv_view: &mut inv_plain_snapshot,
                            atlas_tex_id,
                            icon_uv_resolver: &icon_uv_resolver,
                            open_local: &mut stonecutter_open_local,
                            craft_out: &mut stonecutter_craft,
                        },
                    );
                }

                // W50.5: brewing stand panel.
                if let Some(stand) = &brewing_snapshot {
                    ui::brewing_panel::render(
                        ui::brewing_panel::BrewingPanelInputs {
                            ctx,
                            stand_snapshot: stand,
                            runtime_snapshot: brewing_runtime_snapshot,
                            inv_plain_snapshot: &inv_plain_snapshot,
                            atlas_tex_id,
                            icon_uv_resolver: &icon_uv_resolver,
                            open_local: &mut brewing_open_local,
                            held_local: brewing_held_local,
                            stand_actions_out: &mut brewing_grid_actions,
                        },
                    );
                }

                // Hurt-flash: full-screen red overlay that fades
                // out over ~0.5 s. Painted on the Foreground layer
                // so it sits above the world but below the
                // crosshair / inventory windows.
                if player_hurt_flash > 0.0 {
                    let alpha =
                        (player_hurt_flash / 0.5).clamp(0.0, 1.0).powf(1.3);
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_hurt_flash"),
                    ));
                    let rect = ctx.content_rect();
                    layer.rect_filled(
                        rect,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(
                            180,
                            0,
                            0,
                            (110.0 * alpha) as u8,
                        ),
                    );
                }
                // Underwater / lava view tint. Full-screen
                // composite under the foreground HUD so the
                // hotbar and bars stay readable. Pulled off the
                // submerged liquid's `submerged_tint` field so
                // every liquid kind paints in its own colour.
                if let Some(tint) = submerged_tint {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_submerged_tint"),
                    ));
                    let rect = ctx.content_rect();
                    let r = (tint[0].clamp(0.0, 1.0) * 255.0) as u8;
                    let g = (tint[1].clamp(0.0, 1.0) * 255.0) as u8;
                    let b = (tint[2].clamp(0.0, 1.0) * 255.0) as u8;
                    let a = (tint[3].clamp(0.0, 1.0) * 255.0) as u8;
                    layer.rect_filled(
                        rect,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                    );
                }
                // Post-respawn invulnerability indicator: small
                // toast at the top centre while the timer is
                // positive. Pulses so the player knows the
                // window is finite.
                if hud_visible && invuln_remaining > 0.0 {
                    let alpha =
                        (0.6 + 0.4 * (invuln_remaining * 8.0).sin().abs())
                            .clamp(0.0, 1.0);
                    egui::Area::new(egui::Id::new("voxel_invuln"))
                        .anchor(egui::Align2::CENTER_TOP, [0.0, 60.0])
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    20, 20, 60, 220,
                                ))
                                .show(ui, |ui| {
                                    let a = (255.0 * alpha) as u8;
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Invulnerable: {:.1}s",
                                            invuln_remaining
                                        ))
                                        .color(
                                            egui::Color32::from_rgba_unmultiplied(
                                                160, 200, 255, a,
                                            ),
                                        )
                                        .strong(),
                                    );
                                });
                        });
                }
                // Low-HP vignette: a pulsing red border that
                // ramps in below 6 HP and intensifies the closer
                // the player is to dying. The pulse uses day_time
                // as a continuous clock so we don't have to plumb
                // dt into the egui closure.
                if hud_visible && player_health > 0.0 && player_health < 6.0 {
                    let danger = ((6.0 - player_health) / 6.0).clamp(0.0, 1.0);
                    let pulse = 0.55
                        + 0.45
                            * (day_time * std::f32::consts::TAU * 1.6).sin()
                                .abs();
                    let base_alpha = (40.0 + 110.0 * danger) * pulse;
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_low_hp_vignette"),
                    ));
                    let rect = ctx.content_rect();
                    let thickness = (rect.width().min(rect.height()) * 0.22)
                        .max(40.0);
                    // Four edge bands so the centre stays clear -
                    // a darker corner read than a flat tint.
                    let top = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.max.x, rect.min.y + thickness),
                    );
                    let bottom = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x, rect.max.y - thickness),
                        rect.max,
                    );
                    let left = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.min.x + thickness, rect.max.y),
                    );
                    let right = egui::Rect::from_min_max(
                        egui::pos2(rect.max.x - thickness, rect.min.y),
                        rect.max,
                    );
                    for r in [top, bottom, left, right] {
                        layer.rect_filled(
                            r,
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(
                                160,
                                0,
                                0,
                                (base_alpha as u8).min(180),
                            ),
                        );
                    }
                }
                // W8.3: the 2D star field was deleted per the user's
                // feedback ("hnusne"). The sky reads as near-black
                // at midnight via `day_night`'s new anchors instead.
                // A future wave adds a proper sun + moon sprite pair
                // + a tasteful star field using the real celestial
                // sphere.
                // Floating mob HP bars. Projected onto the screen
                // from each mob's world-space pos + a vertical
                // offset so the bar sits a bit above the head.
                // We hide bars at full HP + during the death fade.
                let screen = ctx.content_rect();
                if hud_visible && !mob_bars.is_empty() {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_mob_hp"),
                    ));
                    for (pos, cur, max) in &mob_bars {
                        let world = glam::Vec3::new(pos[0], pos[1] + 1.55, pos[2]);
                        let clip = hud_view_proj
                            * glam::Vec4::new(world.x, world.y, world.z, 1.0);
                        if clip.w <= 0.0 {
                            continue;
                        }
                        let ndc = glam::Vec3::new(
                            clip.x / clip.w,
                            clip.y / clip.w,
                            clip.z / clip.w,
                        );
                        if !(0.0..=1.0).contains(&ndc.z) {
                            continue;
                        }
                        let sx = (ndc.x * 0.5 + 0.5) * screen.width()
                            + screen.min.x;
                        let sy = (-ndc.y * 0.5 + 0.5) * screen.height()
                            + screen.min.y;
                        // Scale bar width with depth so close
                        // enemies get a chunkier readout.
                        let depth_scale = (1.0 / clip.w * 30.0).clamp(0.25, 1.5);
                        let w = 50.0 * depth_scale;
                        let h = 5.0 * depth_scale;
                        let bg = egui::Rect::from_center_size(
                            egui::pos2(sx, sy),
                            egui::vec2(w, h),
                        );
                        layer.rect_filled(
                            bg,
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(40, 0, 0, 200),
                        );
                        let frac = (*cur as f32 / *max as f32).clamp(0.0, 1.0);
                        let fill = egui::Rect::from_min_size(
                            bg.min,
                            egui::vec2(bg.width() * frac, bg.height()),
                        );
                        let bar_col = if frac > 0.5 {
                            egui::Color32::from_rgb(100, 200, 80)
                        } else if frac > 0.25 {
                            egui::Color32::from_rgb(220, 180, 60)
                        } else {
                            egui::Color32::from_rgb(220, 70, 60)
                        };
                        layer.rect_filled(fill, 1.0, bar_col);
                        layer.rect_stroke(
                            bg,
                            1.0,
                            egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                // Floating nameplates above remote players. Same
                // projection trick as the mob HP bars; rendered
                // slightly higher than a sheep bar so it sits
                // clearly above the head cube. Hidden when the
                // player has toggled the HUD off (F1).
                if hud_visible && !nameplates.is_empty() {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_nameplates"),
                    ));
                    for (pos, name, color) in &nameplates {
                        let world = glam::Vec3::new(pos[0], pos[1] + 0.55, pos[2]);
                        let clip = hud_view_proj
                            * glam::Vec4::new(world.x, world.y, world.z, 1.0);
                        if clip.w <= 0.0 {
                            continue;
                        }
                        let ndc = glam::Vec3::new(
                            clip.x / clip.w,
                            clip.y / clip.w,
                            clip.z / clip.w,
                        );
                        if !(0.0..=1.0).contains(&ndc.z) {
                            continue;
                        }
                        let sx = (ndc.x * 0.5 + 0.5) * screen.width()
                            + screen.min.x;
                        let sy = (-ndc.y * 0.5 + 0.5) * screen.height()
                            + screen.min.y;
                        // Fade text size with depth so far-away
                        // names don't dominate the screen.
                        let depth_scale = (1.0 / clip.w * 30.0).clamp(0.4, 1.4);
                        let font = egui::FontId::proportional(13.0 * depth_scale);
                        // Draw a 1px black outline by stamping the
                        // text four times before the foreground
                        // pass, then the main text on top.
                        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                            layer.text(
                                egui::pos2(sx + dx, sy + dy),
                                egui::Align2::CENTER_BOTTOM,
                                name,
                                font.clone(),
                                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 220),
                            );
                        }
                        layer.text(
                            egui::pos2(sx, sy),
                            egui::Align2::CENTER_BOTTOM,
                            name,
                            font,
                            egui::Color32::from_rgb(color[0], color[1], color[2]),
                        );
                    }
                }
                // Tab playerlist overlay: centered modal listing
                // every known peer (incl. vx) while Tab is held.
                if playerlist_visible && !playerlist.is_empty() {
                    egui::Area::new(egui::Id::new("voxel_playerlist"))
                        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 40.0))
                        .order(egui::Order::Foreground)
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    20, 20, 20, 210,
                                ))
                                .show(ui, |ui| {
                                    ui.set_min_width(180.0);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Players ({})",
                                            playerlist.len()
                                        ))
                                        .strong()
                                        .color(egui::Color32::from_rgb(
                                            255, 220, 100,
                                        )),
                                    );
                                    ui.separator();
                                    for (name, is_self) in &playerlist {
                                        let mut txt = egui::RichText::new(name);
                                        if *is_self {
                                            txt = txt
                                                .color(egui::Color32::from_rgb(
                                                    160, 230, 160,
                                                ))
                                                .strong();
                                        } else {
                                            txt = txt.color(
                                                egui::Color32::from_rgb(
                                                    230, 230, 230,
                                                ),
                                            );
                                        }
                                        ui.label(txt);
                                    }
                                });
                        });
                }
                // Targeted-block label: name + mining progress
                // (when actively breaking the same voxel). Skipped
                // when the crosshair already shows a mob label
                // since both would overlap.
                if hud_visible
                    && targeted_mob_info.is_none()
                    && let Some((name, pct)) = &targeted_block_info
                {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_block_label"),
                    ));
                    let center = screen.center();
                    let text = match pct {
                        Some(p) => format!("{name}  {p}%"),
                        None => name.to_string(),
                    };
                    layer.text(
                        egui::pos2(center.x, center.y + 22.0),
                        egui::Align2::CENTER_TOP,
                        text,
                        egui::FontId::proportional(13.0),
                        egui::Color32::from_rgb(220, 220, 220),
                    );
                }
                // Targeted-mob label: small text just under the
                // crosshair when the player is aiming at a mob.
                // Hidden when no mob is in line of sight.
                if hud_visible
                    && let Some((name, cur, max)) = targeted_mob_info
                {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_mob_label"),
                    ));
                    let center = screen.center();
                    layer.text(
                        egui::pos2(center.x, center.y + 22.0),
                        egui::Align2::CENTER_TOP,
                        format!("{name}  {cur}/{max}"),
                        egui::FontId::proportional(14.0),
                        egui::Color32::from_rgb(240, 240, 240),
                    );
                }
                // Chunk-loading toast at the top centre while
                // a non-trivial batch is in flight. Cheaply
                // signals that the world is streaming in (e.g.
                // right after spawn or a /seed cycle).
                if hud_visible && dbg_pending > 8 {
                    egui::Area::new(egui::Id::new("voxel_loading"))
                        .anchor(egui::Align2::CENTER_TOP, [0.0, 14.0])
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    20, 20, 20, 200,
                                ))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Generating world... ({dbg_pending} chunks pending)"
                                        ))
                                        .color(egui::Color32::from_rgb(
                                            255, 215, 120,
                                        )),
                                    );
                                });
                        });
                }
                // Pickup toasts on the right edge so the chat
                // log + hotbar stay readable on the left + bottom.
                if hud_visible && !pickup_snapshot.is_empty() {
                    egui::Area::new(egui::Id::new("voxel_pickup_toasts"))
                        .anchor(egui::Align2::RIGHT_BOTTOM, [-18.0, -120.0])
                        .show(ctx, |ui| {
                            for (alpha, line, color) in &pickup_snapshot {
                                let a = (255.0 * alpha) as u8;
                                let r = (color[0] * 255.0) as u8;
                                let g = (color[1] * 255.0) as u8;
                                let b = (color[2] * 255.0) as u8;
                                // Brighten the swatch a touch so
                                // darker block colours (e.g. coal)
                                // still pop against the night sky.
                                let r2 = (r as u16 * 5 / 4).min(255) as u8;
                                let g2 = (g as u16 * 5 / 4).min(255) as u8;
                                let b2 = (b as u16 * 5 / 4).min(255) as u8;
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(12.0, 12.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_rgba_unmultiplied(
                                            r2, g2, b2, a,
                                        ),
                                    );
                                    ui.colored_label(
                                        egui::Color32::from_rgba_unmultiplied(
                                            245, 245, 245, a,
                                        ),
                                        line,
                                    );
                                });
                            }
                        });
                }
                // Chat: log of recent lines at bottom-left + an
                // input box when `chat_open`. Each log entry
                // fades out over ~10 s; messages older than that
                // are dropped after this pass.
                if hud_visible || chat_open {
                    egui::Area::new(egui::Id::new("voxel_chat"))
                        .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -90.0])
                        .show(ctx, |ui| {
                            for (line, age, mention) in &chat_lines {
                                let alpha = ((10.0 - age) / 2.0).clamp(0.0, 1.0);
                                if alpha <= 0.0 {
                                    continue;
                                }
                                let a = (255.0 * alpha) as u8;
                                // Mentions of the local player
                                // name highlight in warm yellow
                                // so they stand out of the chat
                                // stream.
                                let col = if *mention {
                                    egui::Color32::from_rgba_unmultiplied(
                                        255, 220, 100, a,
                                    )
                                } else {
                                    egui::Color32::from_rgba_unmultiplied(
                                        240, 240, 240, a,
                                    )
                                };
                                ui.colored_label(col, line);
                            }
                            if chat_open {
                                // W11.5 - autocomplete popup is
                                // drawn ABOVE the input box so the
                                // user sees candidates float over
                                // the chat history while typing.
                                if let Some(popup) = chat_completion.as_ref() {
                                    egui::Frame::popup(ui.style())
                                        .fill(egui::Color32::from_rgba_unmultiplied(
                                            20, 20, 24, 230,
                                        ))
                                        .show(ui, |ui| {
                                            let max =
                                                autocomplete::POPUP_MAX_VISIBLE;
                                            // Scroll-window the
                                            // candidates around the
                                            // selected index so the
                                            // highlight stays visible.
                                            let total = popup.candidates.len();
                                            let start = if total > max
                                                && popup.selected_index
                                                    >= max
                                            {
                                                popup.selected_index + 1 - max
                                            } else {
                                                0
                                            };
                                            let end = (start + max).min(total);
                                            for (i, c) in popup.candidates
                                                [start..end]
                                                .iter()
                                                .enumerate()
                                            {
                                                let abs_idx = start + i;
                                                let selected = abs_idx
                                                    == popup.selected_index;
                                                let col = if selected {
                                                    egui::Color32::from_rgb(
                                                        255, 220, 100,
                                                    )
                                                } else {
                                                    egui::Color32::from_rgb(
                                                        220, 220, 220,
                                                    )
                                                };
                                                ui.colored_label(col, &c.label);
                                            }
                                            if total > end {
                                                ui.colored_label(
                                                    egui::Color32::from_rgb(
                                                        120, 120, 120,
                                                    ),
                                                    format!(
                                                        "... +{} more",
                                                        total - end
                                                    ),
                                                );
                                            }
                                        });
                                }
                                // W14: use a stable id_salt so we
                                // can load/store the TextEditState
                                // to set the cursor position after
                                // an autocomplete accept.
                                // `add_sized` forces the widget to
                                // a hard 720x36 rect; the bare
                                // `TextEdit::desired_width` hint
                                // gets squashed by the parent
                                // `Area`'s auto-shrink so the box
                                // ended up tiny regardless.
                                // W101: while the completion popup is open,
                                // intercept Up/Down BEFORE the TextEdit so
                                // egui doesn't move the text cursor to
                                // line-start/end. consume_key removes the
                                // event (TextEdit never sees it) and tells us
                                // it fired, so we cycle the suggestion here.
                                if chat_completion.is_some() {
                                    let down = ui.input_mut(|i| {
                                        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                                    });
                                    let up = ui.input_mut(|i| {
                                        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                                    });
                                    if let Some(popup) = chat_completion.as_mut() {
                                        if down {
                                            popup.select_next();
                                        } else if up {
                                            popup.select_prev();
                                        }
                                    }
                                }
                                let te_id = ui.id().with("chat_input_te");
                                let resp = ui.add_sized(
                                    [720.0, 36.0],
                                    egui::TextEdit::singleline(chat_buf)
                                        .id(te_id)
                                        .font(egui::FontId::proportional(18.0))
                                        .margin(egui::Vec2::new(8.0, 6.0))
                                        .hint_text("press T to chat, Enter sends"),
                                );
                                resp.request_focus();

                                // W14.1/W14.2: if a previous frame
                                // recorded a desired cursor position
                                // (after accept), apply it now via
                                // egui's TextEditState so the cursor
                                // doesn't snap to the middle or stay
                                // at the old position.
                                if let Some(popup) = chat_completion.as_mut()
                                    && let Some(pos) = popup.pending_cursor.take()
                                    && let Some(mut te_state) =
                                        egui::TextEdit::load_state(ctx, te_id)
                                {
                                    let cc = egui::text::CCursor::new(pos);
                                    te_state.cursor.set_char_range(Some(
                                        egui::text::CCursorRange::one(cc),
                                    ));
                                    egui::TextEdit::store_state(
                                        ctx, te_id, te_state,
                                    );
                                }

                                // W11.5/W14 - autocomplete state + key
                                // handlers. Recompute popup when
                                // input changes; route Tab / Esc /
                                // Enter / arrows to the popup when
                                // it's open instead of to the
                                // history scroll.
                                let up_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::ArrowUp));
                                let down_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::ArrowDown));
                                let tab_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::Tab));
                                let esc_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::Escape));
                                let enter_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::Enter));

                                // W14.2: read the actual egui cursor
                                // position from TextEditState so we
                                // pass the real cursor offset to
                                // `recompute_popup` instead of always
                                // using buf.len(). This fixes
                                // completions when the user edits in
                                // the middle of the line.
                                let te_cursor: usize =
                                    egui::TextEdit::load_state(ctx, te_id)
                                        .and_then(|s| {
                                            s.cursor
                                                .char_range()
                                                .map(|r| r.primary.index)
                                        })
                                        .unwrap_or(chat_buf.len());

                                // Recompute popup on input change or
                                // cursor move. W14.2: also recompute
                                // when cursor moves so completions
                                // track the token under the cursor.
                                let needs_recompute = match chat_completion
                                    .as_ref()
                                {
                                    Some(p) => {
                                        p.input_snapshot.as_str()
                                            != chat_buf.as_str()
                                            || p.cursor_snapshot != te_cursor
                                    }
                                    None => chat_buf.starts_with('/'),
                                };
                                if needs_recompute {
                                    let ctx_ac = CompletionContext {
                                        online_players: online_players.clone(),
                                    };
                                    *chat_completion =
                                        autocomplete::recompute_popup(
                                            chat_buf,
                                            te_cursor,
                                            4, // host = full perm
                                            &ctx_ac,
                                            chat_completion.as_ref(),
                                        );
                                }
                                let popup_active = chat_completion.is_some();
                                if popup_active && esc_pressed {
                                    // W14.3: Esc closes popup only,
                                    // does NOT send the line.
                                    *chat_completion = None;
                                } else if popup_active && tab_pressed {
                                    // W14.3: Tab accepts the highlighted
                                    // candidate + closes popup. The
                                    // trailing space (added by
                                    // apply_completion, W14.1) puts the
                                    // cursor after the token so the user
                                    // can type the next arg immediately.
                                    let mut tab_accepted_cursor: Option<usize> =
                                        None;
                                    if let Some(popup) =
                                        chat_completion.as_mut()
                                    {
                                        if let Some(c) = popup.selected()
                                            && !c.replacement.is_empty()
                                        {
                                            let repl = c.replacement.clone();
                                            let (out, new_cur) =
                                                autocomplete::apply_completion(
                                                    chat_buf,
                                                    te_cursor,
                                                    &repl,
                                                );
                                            *chat_buf = out;
                                            tab_accepted_cursor = Some(new_cur);
                                        } else {
                                            // Hint-only candidate (empty
                                            // replacement): just cycle.
                                            popup.select_next();
                                        }
                                    }
                                    if let Some(new_cur) = tab_accepted_cursor {
                                        // Apply cursor now (persisted in
                                        // egui data; takes effect when
                                        // TextEdit next renders). W14.1.
                                        if let Some(mut te_state) =
                                            egui::TextEdit::load_state(
                                                ctx, te_id,
                                            )
                                        {
                                            let cc =
                                                egui::text::CCursor::new(new_cur);
                                            te_state.cursor.set_char_range(
                                                Some(
                                                    egui::text::CCursorRange::one(
                                                        cc,
                                                    ),
                                                ),
                                            );
                                            egui::TextEdit::store_state(
                                                ctx, te_id, te_state,
                                            );
                                        }
                                        // Close popup after Tab accept.
                                        // W14.3: do NOT submit on Tab.
                                        *chat_completion = None;
                                    }
                                } else if popup_active && enter_pressed {
                                    // W14.3: Enter w/ popup open ->
                                    // accept highlighted candidate,
                                    // close popup, AND submit the
                                    // command (single keystroke UX).
                                    let mut accepted = false;
                                    if let Some(popup) =
                                        chat_completion.as_ref()
                                        && let Some(c) = popup.selected()
                                        && !c.replacement.is_empty()
                                    {
                                        let (out, new_cur) =
                                            autocomplete::apply_completion(
                                                chat_buf,
                                                te_cursor,
                                                &c.replacement,
                                            );
                                        *chat_buf = out;
                                        // Apply cursor pos. W14.1.
                                        if let Some(mut te_state) =
                                            egui::TextEdit::load_state(
                                                ctx, te_id,
                                            )
                                        {
                                            let cc =
                                                egui::text::CCursor::new(
                                                    new_cur,
                                                );
                                            te_state.cursor.set_char_range(
                                                Some(
                                                    egui::text::CCursorRange::one(
                                                        cc,
                                                    ),
                                                ),
                                            );
                                            egui::TextEdit::store_state(
                                                ctx, te_id, te_state,
                                            );
                                        }
                                        accepted = true;
                                    }
                                    *chat_completion = None;
                                    // Submit immediately after accept
                                    // so Enter = accept + send. W14.3.
                                    if accepted {
                                        chat_send_now = true;
                                    }
                                } else if popup_active
                                    && (up_pressed || down_pressed)
                                {
                                    if let Some(popup) =
                                        chat_completion.as_mut()
                                    {
                                        if down_pressed {
                                            popup.select_next();
                                        } else {
                                            popup.select_prev();
                                        }
                                    }
                                } else if !popup_active {
                                    // No popup - fall back to
                                    // history scrolling + Enter
                                    // send.
                                    if !chat_history_ref.is_empty() {
                                        if up_pressed {
                                            let new_idx =
                                                match *chat_history_idx {
                                                    None => {
                                                        chat_history_ref.len()
                                                            - 1
                                                    }
                                                    Some(i) => {
                                                        i.saturating_sub(1)
                                                    }
                                                };
                                            *chat_history_idx =
                                                Some(new_idx);
                                            *chat_buf = chat_history_ref
                                                [new_idx]
                                                .clone();
                                        } else if down_pressed {
                                            match *chat_history_idx {
                                                Some(i)
                                                    if i + 1
                                                        < chat_history_ref
                                                            .len() =>
                                                {
                                                    *chat_history_idx =
                                                        Some(i + 1);
                                                    *chat_buf =
                                                        chat_history_ref
                                                            [i + 1]
                                                            .clone();
                                                }
                                                Some(_) => {
                                                    *chat_history_idx =
                                                        None;
                                                    chat_buf.clear();
                                                }
                                                None => {}
                                            }
                                        }
                                    }
                                    if enter_pressed {
                                        chat_send_now = true;
                                    }
                                }
                            }
                        });
                }
                // Crosshair drawn directly on the egui screen layer.
                // Two thin segments through the screen centre.
                if hud_visible {
                    let center = screen.center();
                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_crosshair"),
                    ));
                    let stroke =
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(240, 240, 240));
                    let half = 8.0_f32;
                    painter.line_segment(
                        [
                            egui::pos2(center.x - half, center.y),
                            egui::pos2(center.x + half, center.y),
                        ],
                        stroke,
                    );
                    painter.line_segment(
                        [
                            egui::pos2(center.x, center.y - half),
                            egui::pos2(center.x, center.y + half),
                        ],
                        stroke,
                    );
                }
            }
        });

        // Drop chat lines older than `CHAT_FADE_SECS`. Done after
        // the render pass so the egui closure had a chance to
        // display them one last frame.
        vx.chat_log.retain(|(t, _)| {
            chat_now.saturating_duration_since(*t).as_secs_f32() < CHAT_FADE_SECS
        });
        // Send the chat line if the user pressed Enter.
        if chat_send_now {
            let text = vx.chat_input.trim().to_string();
            vx.chat_input.clear();
            vx.chat_open = false;
            vx.chat_history_idx = None;
            // W11.5: closing chat hides the autocomplete popup.
            vx.chat_completion = None;
            if !text.is_empty() {
                // Push to history, dedup adjacent repeats so
                // hitting Up + Enter twice doesn't double-fill
                // the log.
                if vx.chat_history.last() != Some(&text) {
                    vx.chat_history.push(text.clone());
                    if vx.chat_history.len() > CHAT_HISTORY_MAX {
                        let drop = vx.chat_history.len() - CHAT_HISTORY_MAX;
                        vx.chat_history.drain(0..drop);
                    }
                }
                // Wave 7: fire on_chat_msg for every loaded
                // plugin BEFORE slash-command dispatch so a
                // plugin can observe the raw user input + react.
                // speaker_id 0 = local sender (the engine isn't
                // multi-player-aware in this hook yet).
                plugin::events::dispatch_chat(vx, 0, &text);
                // Slash-command dispatch. Commands run locally
                // (no broadcast); everything else routes to chat
                // normally below.
                let handled_locally = chat::dispatch(vx, &text);
                if !handled_locally {
                    match &vx.net {
                        NetRole::Offline => {
                            // No-op offline: chat only makes sense
                            // in MP. Drop the input silently.
                        }
                        NetRole::Server { server, .. } => {
                            // Host prepends its own name + appends
                            // to the local log, then broadcasts so
                            // every client renders the same line.
                            let line =
                                format!("<{}> {text}", vx.player_name);
                            vx.chat_log
                                .push((std::time::Instant::now(), line.clone()));
                            if let Ok(buf) = bincode::serde::encode_to_vec(
                                &line,
                                bincode::config::standard(),
                            ) {
                                server.broadcast_bytes("vx.chat", buf);
                            }
                        }
                        NetRole::Client { client, .. } => {
                            // Client sends the raw text; the server
                            // tags it with the sender name before
                            // broadcasting back.
                            if let Ok(buf) = bincode::serde::encode_to_vec(
                                &text,
                                bincode::config::standard(),
                            ) {
                                let _ = client.send_bytes("vx.chat", buf);
                            }
                        }
                    }
                }
            }
        }
        // Apply inventory clicks captured during the egui pass. The
        // egui closure can't touch `vx.inventory` directly (the
        // renderer already holds a `&mut vx`), so we buffer the
        // intents and run them out here.
        //
        // Multiplayer client mode: every drag is forwarded to the
        // server as `vx.inv_act`. The local view is reconciled when
        // the server's authoritative snapshot lands. Hotbar selection
        // stays local for snappy feedback (it's purely cosmetic).
        let is_client = matches!(vx.net, NetRole::Client { .. });
        for action in inv_actions {
            match action {
                InvAction::SelectHotbar(idx) => {
                    if idx < HOTBAR_SLOTS {
                        vx.selected_slot = idx;
                    }
                }
                InvAction::Drag { from, to, split } => {
                    if is_client {
                        let msg = InvActionMsg {
                            from: from.into(),
                            to: to.into(),
                            split,
                        };
                        if let NetRole::Client {
                            client,
                            pending_inv_actions,
                            ..
                        } = &mut vx.net
                        {
                            let cfg = bincode::config::standard();
                            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                                let _ = client.send_bytes("vx.inv_act", buf);
                            }
                            pending_inv_actions.push(msg);
                        }
                    } else {
                        vx.apply_inv_action(InvAction::Drag { from, to, split });
                    }
                }
            }
        }

        // If the debug panel mutated weather, broadcast the new
        // value to keep clients in sync.
        if vx.weather != weather_before
            && let NetRole::Server { server, .. } = &mut vx.net
            && let Ok(buf) = bincode::serde::encode_to_vec(
                vx.weather,
                bincode::config::standard(),
            )
        {
            server.broadcast_bytes("vx.weather", buf);
        }
        // Reflect window-close-button clicks back to the state
        // flags they belong to.
        if dbg_panel_was_open && !dbg_panel_open {
            vx.show_debug_panel = false;
        }
        if vx.inventory_open && !inv_open_local {
            vx.inventory_open = false;
        }
        if vx.mp_menu_open && !mp_menu_open_local {
            vx.mp_menu_open = false;
        }
        // W10.6: re-apply creative tab mutations captured during
        // the egui closure. The X button closes the panel; clicks
        // on grid tiles produced (block, count) intents that we
        // deposit into the hotbar here.
        vx.creative_tab.query = creative_query_local;
        // W12.9 - apply the active category selection (clicked on the
        // tab strip during the egui closure).
        vx.creative_tab.category = creative_cat_local;
        // W20.3 - clear the just_opened one-shot flag so future
        // frames don't repeatedly try to focus the search box.
        if creative_just_opened {
            vx.creative_tab.just_opened = false;
        }
        if creative_close_clicked {
            vx.creative_tab.close();
        }
        if !creative_picks.is_empty() {
            // Slice off the first 9 inventory entries as the
            // hotbar view + run the deposit policy on it.
            let mut hotbar_view: [Option<(BlockId, u8)>; HOTBAR_SLOTS] =
                [None; HOTBAR_SLOTS];
            hotbar_view.copy_from_slice(&vx.inventory[..HOTBAR_SLOTS]);
            let sel = vx.selected_slot.min(HOTBAR_SLOTS - 1);
            for pick in creative_picks {
                let (block, max_stack) = pick;
                let stack = (block, max_stack);
                let deposited = creative_tab::deposit_in_hotbar(
                    &mut hotbar_view,
                    stack,
                    max_stack,
                );
                if !deposited {
                    // W34 fix: when the hotbar is full of other items,
                    // creative click should OVERWRITE the currently
                    // selected slot. MC's creative click semantics:
                    // "pick replaces what's in your hand". Without this
                    // fallback the click was silently rejected if no
                    // empty/topup slot was available.
                    hotbar_view[sel] = Some((block, max_stack));
                }
            }
            vx.inventory[..HOTBAR_SLOTS]
                .copy_from_slice(&hotbar_view[..HOTBAR_SLOTS]);
        }
        // Chest UI close-button reflected back. If the egui window
        // was closed via its X handle, drop the chest_open + held
        // state. Also drop the held cursor stack back to the
        // first free chest slot if the chest is still in range
        // (the simplest "don't lose items" recovery).
        if chest_open_pos.is_some() && !chest_open_local {
            vx.close_chest_ui();
        }
        // Same close-button handling for the furnace UI.
        if furnace_open_pos.is_some() && !furnace_open_local {
            vx.close_furnace_ui();
        }
        // W50.3: crafting-table panel apply.
        if crafting_table_open_pos.is_some() {
            // 1. Sync grid + held back from the local mutations
            //    inside the egui closure.
            vx.craft_grid_3x3 = crafting_table_grid_local;
            vx.crafting_table_held = crafting_table_held_local;
            // 2. Output click: consume 1 of every non-empty grid
            //    cell + push the resolved output onto the held
            //    stack (or merge with existing held).
            if crafting_table_take_output {
                let grid_u16: [Option<(u16, u8)>; 9] =
                    std::array::from_fn(|i| {
                        vx.craft_grid_3x3[i].map(|(b, c)| (b.0, c))
                    });
                if let Some((out_id, out_n, _)) =
                    crate::recipes::matcher::match_3x3(&grid_u16)
                {
                    // Merge / set held.
                    let new_held = match vx.crafting_table_held {
                        None => Some((out_id, out_n)),
                        Some((hid, hn)) if hid == out_id => {
                            let cap = inventory::STACK_MAX;
                            let sum = (hn as u16) + (out_n as u16);
                            if sum <= cap as u16 {
                                Some((hid, sum as u8))
                            } else {
                                // Output doesn't fit - don't craft
                                // (matches MC).
                                Some((hid, hn))
                            }
                        }
                        Some(h) => Some(h), // different item held; skip
                    };
                    // Only consume inputs if the held merge
                    // succeeded (i.e. we actually produced output).
                    let consumed = match (vx.crafting_table_held, new_held) {
                        (None, Some(_)) => true,
                        (Some((_, before)), Some((_, after))) if after > before => true,
                        _ => false,
                    };
                    if consumed {
                        for cell in vx.craft_grid_3x3.iter_mut() {
                            if let Some((b, c)) = *cell {
                                let nc = c.saturating_sub(1);
                                *cell = if nc == 0 { None } else { Some((b, nc)) };
                            }
                        }
                        vx.crafting_table_held = new_held;
                    }
                }
            }
            // 3. Copy the mutated inv snapshot back into
            //    vx.inventory (the panel's player-inv grid
            //    operated directly on inv_plain_snapshot).
            for (i, slot) in inv_plain_snapshot.iter().enumerate() {
                if i >= vx.inventory.len() {
                    break;
                }
                vx.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
            }
            // 4. Close-via-X: route through close_crafting_table_ui
            //    so the grid + held stack get returned to the inv
            //    (or spawn as drops when the inv is full).
            if !crafting_table_open_local {
                vx.close_crafting_table_ui();
            }
        }
        // W171: stonecutter apply. Sync the input back, run the
        // clicked cut (consume 1 input + add the output to the inv),
        // copy the inv snapshot back, and on close return the input.
        if stonecutter_open_pos.is_some() {
            vx.stonecutter_input = stonecutter_input_local;
            if let Some(i) = stonecutter_craft
                && let Some((in_id, in_n)) = vx.stonecutter_input
            {
                let cuts = crate::recipes::stonecutter::cuts_for(in_id);
                if let Some(cut) = cuts.get(i) {
                    ui::stonecutter_panel::give_to_inv(
                        &mut inv_plain_snapshot,
                        (cut.output, cut.count),
                    );
                    let n = in_n.saturating_sub(1);
                    vx.stonecutter_input =
                        if n == 0 { None } else { Some((in_id, n)) };
                }
            }
            // Close-via-X: return the remaining input to the inventory.
            if !stonecutter_open_local
                && let Some(stack) = vx.stonecutter_input.take()
            {
                ui::stonecutter_panel::give_to_inv(&mut inv_plain_snapshot, stack);
            }
            for (i, slot) in inv_plain_snapshot.iter().enumerate() {
                if i >= vx.inventory.len() {
                    break;
                }
                vx.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
            }
            if !stonecutter_open_local {
                vx.stonecutter_open = None;
            }
        }
        // W50.5: brewing panel apply.
        if brewing_open_pos.is_some() && !brewing_open_local {
            vx.close_brewing_ui();
        }
        if let Some(pos) = vx.brewing_open
            && !brewing_grid_actions.is_empty()
        {
            if let Some(stand) = vx.brewing_stands.get_mut(&pos) {
                let mut inv_u16: Vec<Option<(u16, u8)>> = vx
                    .inventory
                    .iter()
                    .map(|s| s.map(|(b, c)| (b.0, c)))
                    .collect();
                for (player_side, action) in brewing_grid_actions {
                    if player_side {
                        // Reuse the generic chest player-side
                        // helper; it's container-agnostic.
                        block::chest::apply_player_action(
                            &mut inv_u16,
                            stand,
                            &mut brewing_held_local,
                            action,
                        );
                    } else {
                        block::chest::apply_chest_action(
                            stand,
                            &mut inv_u16,
                            &mut brewing_held_local,
                            action,
                        );
                    }
                }
                for (i, slot) in inv_u16.iter().enumerate() {
                    if i >= vx.inventory.len() {
                        break;
                    }
                    vx.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
                }
            }
            vx.brewing_held = brewing_held_local;
        } else {
            vx.brewing_held = brewing_held_local;
        }
        // Apply queued chest-UI actions. Each entry is
        // `(true_if_player_side, GridAction)`.
        if let Some(pos) = vx.chest_open
            && !chest_grid_actions.is_empty()
        {
            // W5.3a: as a client, forward each grid action to the
            // host. The local mutation below STILL runs so the
            // client UI stays responsive (optimistic apply); the
            // next `vx.chest_state` broadcast reconciles by
            // overwriting both the chest and the local inv slot.
            if let NetRole::Client { client, .. } = &mut vx.net {
                let cfg = bincode::config::standard();
                for (player_side, action) in &chest_grid_actions {
                    let msg = crate::net::ChestActMsg {
                        pos,
                        player_side: *player_side,
                        action: crate::net::ChestAct::from_grid_action(*action),
                    };
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        let _ = client.send_bytes("vx.chest_act", buf);
                    }
                }
            }
            if let Some(chest) = vx.chests.get_mut(&pos) {
                // Mirror the actions onto a temp u16-shaped
                // player-inv buffer so the helpers (which expect
                // `[Option<(u16, u8)>]`) can mutate freely; then
                // copy back into the BlockId-shaped real inv.
                let mut inv_u16: Vec<Option<(u16, u8)>> = vx
                    .inventory
                    .iter()
                    .map(|s| s.map(|(b, c)| (b.0, c)))
                    .collect();
                for (player_side, action) in chest_grid_actions {
                    if player_side {
                        block::chest::apply_player_action(
                            &mut inv_u16,
                            chest,
                            &mut chest_held_local,
                            action,
                        );
                    } else {
                        block::chest::apply_chest_action(
                            chest,
                            &mut inv_u16,
                            &mut chest_held_local,
                            action,
                        );
                    }
                }
                // Copy mutated inv back. Length matches because
                // we never resize.
                for (i, slot) in inv_u16.iter().enumerate() {
                    if i >= vx.inventory.len() {
                        break;
                    }
                    vx.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
                }
            }
            vx.chest_held = chest_held_local;
        } else {
            // No actions but still surface the latest cursor
            // state for the next frame.
            vx.chest_held = chest_held_local;
        }
        // W55.2b: apply partner-chest actions (double-chest top
        // grid). Routes through the same chest apply helpers but
        // against the partner pos.
        if let Some(partner_pos) = chest_partner_pos
            && !chest_partner_actions.is_empty()
            && let Some(chest) = vx.chests.get_mut(&partner_pos)
        {
                let mut inv_u16: Vec<Option<(u16, u8)>> = vx
                    .inventory
                    .iter()
                    .map(|s| s.map(|(b, c)| (b.0, c)))
                    .collect();
                for action in chest_partner_actions {
                    block::chest::apply_chest_action(
                        chest,
                        &mut inv_u16,
                        &mut vx.chest_held,
                        action,
                    );
                }
                for (i, slot) in inv_u16.iter().enumerate() {
                    if i >= vx.inventory.len() {
                        break;
                    }
                    vx.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
                }
        }
        // Apply queued furnace-UI actions. Mirrors the chest path
        // but routes through `block::furnace::apply_furnace_action`
        // / `apply_player_action` so the accept-filter checks fire
        // before any slot mutation.
        if let Some(pos) = vx.furnace_open
            && !furnace_grid_actions.is_empty()
        {
            // W5.3b: forward each grid action to the host (client
            // role). Local optimistic apply still runs below for
            // UI responsiveness; reconcile on next state msg.
            if let NetRole::Client { client, .. } = &mut vx.net {
                let cfg = bincode::config::standard();
                for (player_side, action) in &furnace_grid_actions {
                    let msg = crate::net::FurnaceActMsg {
                        pos,
                        player_side: *player_side,
                        action: crate::net::ChestAct::from_grid_action(*action),
                    };
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        let _ = client.send_bytes("vx.furnace_act", buf);
                    }
                }
            }
            if let Some(furnace) = vx.furnaces.get_mut(&pos) {
                let mut inv_u16: Vec<Option<(u16, u8)>> = vx
                    .inventory
                    .iter()
                    .map(|s| s.map(|(b, c)| (b.0, c)))
                    .collect();
                for (player_side, action) in furnace_grid_actions {
                    if player_side {
                        block::furnace::apply_player_action(
                            &mut inv_u16,
                            furnace,
                            &mut furnace_held_local,
                            action,
                        );
                    } else {
                        block::furnace::apply_furnace_action(
                            furnace,
                            &mut inv_u16,
                            &mut furnace_held_local,
                            action,
                        );
                    }
                }
                for (i, slot) in inv_u16.iter().enumerate() {
                    if i >= vx.inventory.len() {
                        break;
                    }
                    vx.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
                }
            }
            vx.furnace_held = furnace_held_local;
        } else {
            vx.furnace_held = furnace_held_local;
        }
        // `action_start` lived on the legacy flat picker. The Wave 4
        // menu emits `MenuAction::PlayOffline { slug }` instead, which
        // `apply_menu_action` consumes below.
        // W20.4: persist recipe book search query from the egui closure.
        vx.recipe_book_query = recipe_book_query_local;
        // Recipe-book auto-fill: try to pull every input stack
        // from `vx.inventory` into the 2x2 craft grid. Returns
        // any earlier withdrawals if the recipe demands more
        // than the player carries.
        // Clients ship the recipe index to the server which
        // mutates the authoritative inventory + replies with a
        // fresh `vx.inv` snapshot. Offline / host runs the
        // helper locally below.
        if let Some(idx) = prefill_recipe_idx
            && let NetRole::Client { client, .. } = &vx.net
            && let Ok(buf) = bincode::serde::encode_to_vec(
                idx as u32,
                bincode::config::standard(),
            )
        {
            let _ = client.send_bytes("vx.recipe", buf);
        }
        if let Some(idx) = prefill_recipe_idx
            && !matches!(vx.net, NetRole::Client { .. })
            && let Some(recipe) = RECIPES.get(idx)
        {
            // Inventory of (slot_idx, original_stack) we may
            // have to refund on failure.
            let mut taken: Vec<(usize, (BlockId, u8))> = Vec::new();
            let mut placed: [Option<(BlockId, u8)>; 4] = [None; 4];
            let mut ok = true;
            for (i, (bid, n)) in recipe.inputs.iter().enumerate() {
                if i >= 4 {
                    break;
                }
                let need = *n;
                let target = BlockId(*bid);
                // Look for a slot that has the needed block.
                let mut found_slot: Option<usize> = None;
                for (si, slot) in vx.inventory.iter().enumerate() {
                    if let Some((b, c)) = slot
                        && *b == target
                        && *c >= need
                    {
                        found_slot = Some(si);
                        break;
                    }
                }
                match found_slot {
                    Some(si) => {
                        let original =
                            vx.inventory[si].expect("verified above");
                        taken.push((si, original));
                        let remaining =
                            original.1.saturating_sub(need);
                        vx.inventory[si] = if remaining == 0 {
                            None
                        } else {
                            Some((original.0, remaining))
                        };
                        placed[i] = Some((target, need));
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                // Return whatever was in the craft grid before
                // the auto-fill so it's not silently lost.
                let returns: Vec<(BlockId, u8)> = vx
                    .craft_grid
                    .iter_mut()
                    .filter_map(|s| s.take())
                    .collect();
                for (b, c) in returns {
                    for _ in 0..c {
                        vx.pickup(b);
                    }
                }
                vx.craft_grid = placed;
            } else {
                // Roll back.
                for (si, original) in taken {
                    vx.inventory[si] = Some(original);
                }
                vx.chat_log.push((
                    std::time::Instant::now(),
                    format!(
                        "Not enough materials for {}",
                        recipe.name
                    ),
                ));
            }
        }
        if action_resume {
            vx.state = AppState::Playing;
            vx.mouse_dx = 0.0;
            vx.mouse_dy = 0.0;
        }
        // `action_quit` lived on the legacy flat picker; the Wave 4
        // menu emits `MenuAction::Quit` instead.
        if action_mp_resume {
            vx.mp_menu_open = false;
        }
        if action_mp_disconnect {
            // Best-effort vx.leave so the server pre-empts its
            // 5s silence reaper + broadcasts the removal right
            // away. Then drop the net role + return to the main
            // menu so the user can rejoin / host again.
            vx.notify_server_leaving();
            vx.net = NetRole::Offline;
            vx.mp_menu_open = false;
            // W8.1: tear the world down so the simulation halts
            // while the user is back on the menu. Drops chunks,
            // workers, mobs, lighting. Auto-saves first when the
            // current role is offline / server.
            vx.teardown_world();
            // Same fix as `action_main_menu`: bounce the menu state
            // back to the top screen so the user actually sees the
            // main menu after disconnecting.
            vx.menu.screen = menu::MenuScreen::Main;
            vx.state = AppState::MainMenu;
            vx.remote_mobs.clear();
            vx.mobs.clear();
        }
        if action_mp_quit {
            vx.notify_server_leaving();
            vx.requested_quit = true;
        }
        // The legacy "Settings" button on the flat picker emitted
        // `action_menu_to_settings`. Wave 4 routes the same intent
        // through `MenuScreen::Options` (handled by the menu module).
        if action_pause_to_settings {
            vx.state = AppState::Settings {
                back_to: BackTo::Pause,
            };
        }
        if action_back_settings && let AppState::Settings { back_to } = vx.state {
            // Persist whatever the user just edited so the next
            // launch picks it up. Cheap (a few dozen bytes), so
            // doing it on Back is fine.
            vx.save_config();
            vx.state = match back_to {
                BackTo::MainMenu => AppState::MainMenu,
                BackTo::Pause => AppState::Paused,
            };
        }
        if action_main_menu {
            // W8.1: tear the world down on return-to-main so the
            // simulation halts + the next world load runs the
            // proper enter_world entry. Auto-saves first when the
            // current net role is offline / server.
            vx.teardown_world();
            // Reset the menu state machine too. Wave 4 (G.x) decoupled
            // AppState from `menu::MenuScreen`; flipping AppState alone
            // would leave the menu showing whatever sub-screen the user
            // last visited (Singleplayer / Multiplayer / Options) so
            // the Pause panel "Main Menu" button appeared to do nothing.
            vx.menu.screen = menu::MenuScreen::Main;
            // W21.1: pick a fresh splash line on return from gameplay.
            vx.menu.enter_main();
            vx.state = AppState::MainMenu;
        }
        // Wave 4 (Phase G.6 + G.7): the legacy `action_to_direct /
        // action_to_browser / action_to_host / action_join_direct /
        // action_join_browser / action_browser_refresh /
        // action_host_start / action_new_world / action_load_world /
        // action_delete_world` blocks are gone - the new menu emits
        // `MenuAction` variants that `apply_menu_action` (below)
        // routes through the same runtime helpers. Drain the
        // collected actions here so the egui borrow scope has
        // ended.
        for action in menu_actions.drain(..) {
            vx.apply_menu_action(action);
        }
    }
