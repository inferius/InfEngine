//! W47.19 - menu action dispatcher, relocated from `main.rs`.
//!
//! `apply_action` runs each `MenuAction` emitted by the egui menu
//! against the live `Voxel` runtime: screen transitions, world
//! create / delete / load, server connect, options apply, resource
//! pack reload. Free function with `&mut Voxel`; the method on
//! `Voxel` survives as a thin wrapper so existing call sites keep
//! working.

use std::sync::Arc;

use tracing::{info, warn};

use crate::Voxel;
use crate::menu::{self, MenuAction};
use crate::resource_pack;
use crate::world_save;
use crate::worldgen::WorldGen;

/// W47.19: drive one menu action against `vx`.
pub(crate) fn apply_action(vx: &mut Voxel, action: MenuAction) {
    match action {
        MenuAction::SwitchScreen(target) => {
            // W21.1: pick a fresh splash line when the user
            // returns to the Main screen from a sub-screen.
            let arriving_main = matches!(target, menu::MenuScreen::Main);
            vx.menu.apply_switch(target);
            if arriving_main {
                vx.menu.enter_main();
            }
            // Auto-refresh the server list when entering Multiplayer.
            if matches!(vx.menu.screen, menu::MenuScreen::Multiplayer { .. }) {
                vx.refresh_browser();
            }
        }
        MenuAction::Quit => {
            vx.requested_quit = true;
        }
        MenuAction::PlayOffline { slug } => {
            vx.current_world_slug = Some(slug);
            vx.reset_world();
            vx.last_built_worldgen = None;
            vx.mouse_dx = 0.0;
            vx.mouse_dy = 0.0;
            vx.enter_world();
        }
        MenuAction::NewWorld {
            name,
            worldgen,
            seed,
            game_mode,
        } => {
            let slug = if name.trim().is_empty() {
                world_save::DEFAULT_SLUG.to_string()
            } else {
                world_save::layout::slug(&name)
            };
            let display = if name.trim().is_empty() {
                slug.clone()
            } else {
                name
            };
            let meta = world_save::meta::create_meta(&display, &slug);
            if let Err(err) = world_save::meta::write_meta(&slug, &meta) {
                warn!(
                    error = %err,
                    slug = slug,
                    "world_save: write_meta on NewWorld failed"
                );
            }
            // W8.2: pin the seed from the menu form. Empty form input
            // lands as None which routes to a fresh random seed.
            vx.base_seed = seed.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
                    .wrapping_mul(0x9E37_79B1)
                    | 1
            });
            // W8.2: pin the worldgen on the new slot's overworld dim
            // from the form pick. Empty falls back to the current dim.
            let active_wg = if worldgen.is_empty() {
                vx.dim_worldgens
                    .get(&vx.current_dim)
                    .copied()
                    .unwrap_or(WorldGen::Classic)
            } else {
                WorldGen::from_str(&worldgen).unwrap_or(WorldGen::Classic)
            };
            vx.dim_worldgens.insert(vx.current_dim.clone(), active_wg);
            let wj = world_save::world_json::create_world_with_mode(
                world_save::worldgen_str(active_wg),
                vx.base_seed,
                game_mode.as_tag(),
            );
            if let Err(err) = world_save::world_json::write_world(&slug, &wj) {
                warn!(
                    error = %err,
                    slug = slug,
                    "world_save: write_world on NewWorld failed"
                );
            }
            vx.current_world_slug = Some(slug);
            vx.game_mode = game_mode;
            vx.refresh_abilities();
            vx.menu.invalidate_caches();
            vx.reset_world();
            vx.last_built_worldgen = None;
            vx.mouse_dx = 0.0;
            vx.mouse_dy = 0.0;
            vx.enter_world();
        }
        MenuAction::DeleteWorld { slug } => {
            let dir = world_save::layout::world_dir(&slug);
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => {
                    info!(slug = %slug, "world_save: deleted slot");
                    if vx.current_world_slug.as_deref() == Some(slug.as_str()) {
                        vx.current_world_slug =
                            Some(world_save::DEFAULT_SLUG.to_string());
                    }
                }
                Err(err) => warn!(
                    error = %err,
                    slug = %slug,
                    "world_save: delete slot failed"
                ),
            }
            vx.menu.invalidate_caches();
        }
        MenuAction::DirectConnect { addr } => {
            vx.connect_as_client(&addr);
        }
        MenuAction::JoinServer { addr } => {
            vx.connect_as_client(&addr);
        }
        MenuAction::BrowseServersRefresh => {
            vx.refresh_browser();
        }
        MenuAction::AddSavedServer { name, addr } => {
            let mut entries = menu::multiplayer::load_saved_servers();
            entries.retain(|s| s.addr != addr);
            entries.push(menu::SavedServer { name, addr });
            menu::multiplayer::save_saved_servers(&entries);
            vx.menu.invalidate_caches();
        }
        MenuAction::RemoveSavedServer { addr } => {
            let mut entries = menu::multiplayer::load_saved_servers();
            entries.retain(|s| s.addr != addr);
            menu::multiplayer::save_saved_servers(&entries);
            vx.menu.invalidate_caches();
        }
        MenuAction::ApplyOptions { mut snapshot } => {
            snapshot.clamp();
            vx.player_name = snapshot.player_name.clone();
            vx.master_addr = snapshot.master_addr.clone();
            vx.settings.view_distance = snapshot.view_distance;
            vx.fov_y_rad = snapshot.fov_deg.to_radians();
            vx.settings.mouse_sensitivity = snapshot.mouse_sensitivity;
            vx.settings.show_fps = snapshot.show_fps;
            vx.settings.head_bob = snapshot.head_bob;
            vx.settings.invert_mouse_y = snapshot.invert_mouse_y;
            vx.settings.invert_wheel = snapshot.invert_wheel;
            vx.settings.ui_scale = snapshot.ui_scale.clamp(0.5, 3.0);
            vx.settings.lock_cursor = snapshot.lock_cursor;
            vx.settings.hide_cursor = snapshot.hide_cursor;
            let trimmed = snapshot.mc_assets_path.trim();
            vx.settings.mc_assets_path = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
            // W124: language switch reloads the lang maps in place.
            if vx.settings.language != snapshot.language {
                vx.settings.language = snapshot.language.clone();
                crate::lang::load_and_set(&vx.resource_pack, &vx.settings.language);
            }
            vx.menu.options = snapshot;
            vx.save_config();
        }
        MenuAction::ReloadResourcePack { path } => {
            let trimmed = path.trim();
            let override_path = if trimmed.is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(trimmed))
            };
            vx.resource_pack =
                Arc::new(resource_pack::ResourcePack::load_with_override(override_path));
            vx.settings.mc_assets_path = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
            if let Some(r) = vx.renderer.as_mut() {
                let (w, h) = vx.resource_pack.atlas_size;
                let new_handle = r.make_mesh_albedo(w, h, &vx.resource_pack.atlas_rgba);
                vx.voxel_atlas = Some(new_handle);
                r.set_mesh_albedo(w, h, &vx.resource_pack.atlas_rgba);
                vx.atlas_rgba = Some((w, h, vx.resource_pack.atlas_rgba.clone()));
                r.upload_tile_anims(&vx.resource_pack.build_tile_anims_uniform());
                r.set_block_params(&crate::render::draw_params::block_param_entries());
                if let Some(pa) = &vx.resource_pack.particle_atlas {
                    r.load_particle_atlas_rgba(pa.width, pa.height, &pa.rgba);
                }
            }
            vx.egui_atlas = None;
            vx.rebuild_all_meshes();
            vx.save_config();
            tracing::info!(
                source = %vx.resource_pack.source.display(),
                fallback = vx.resource_pack.is_fallback,
                chunks = vx.chunk_meshes.len(),
                "resource pack reloaded",
            );
        }
    }
}
