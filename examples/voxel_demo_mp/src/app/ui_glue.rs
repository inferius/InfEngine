//! W47.28 - ui_glue impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {

    /// Returns true while *any* in-game popup / modal is open.
    /// Central point used by the cursor + input + click gates so
    /// future popups only need to add one flag here instead of
    /// fanning out to every guard site.
    pub(crate) fn any_panel_open(&self) -> bool {
        self.inventory_open
            || self.mp_menu_open
            || self.chat_open
            || self.show_debug_panel
            || self.chest_open.is_some()
            || self.furnace_open.is_some()
            // W10.6: the creative tab also captures input.
            || self.creative_tab.open
            // W53.2: crafting table + brewing stand panels also
            // need the cursor + suppress WASD / hotkeys while open.
            || self.crafting_table_open.is_some()
            || self.brewing_open.is_some()
            // W171: stonecutter panel.
            || self.stonecutter_open.is_some()
            || self.trade_open.is_some()
    }

    pub(crate) fn apply_cursor_settings(&self, window: &EngineWindow) {
        use infengine_window::raw_winit::window::CursorGrabMode;
        let playing = matches!(self.state, AppState::Playing);
        let raw = window.raw();
        // Only grab when we have OS focus. Trying to grab an
        // unfocused window on Windows silently drops the lock and
        // can teleport the cursor onto a sibling window mid-click.
        // Inventory overlay needs a usable cursor + breaks the
        // lock so the mouse can leave the window without being
        // snapped back to the centre.
        let active =
            playing && self.has_focus && !self.any_panel_open();
        let grab = if active && self.settings.lock_cursor {
            if self.settings.hide_cursor {
                CursorGrabMode::Locked
            } else {
                CursorGrabMode::Confined
            }
        } else {
            CursorGrabMode::None
        };
        if raw.set_cursor_grab(grab).is_err() && grab == CursorGrabMode::Locked {
            let _ = raw.set_cursor_grab(CursorGrabMode::Confined);
        }
        raw.set_cursor_visible(!(active && self.settings.hide_cursor));
    }

    pub(crate) fn update_play(&mut self, ctx: &FrameContext<'_>, dt: f32) {
        crate::update_play::update_play(self, ctx, dt);
    }

    pub(crate) fn render_ui(&mut self, window: &EngineWindow) {
        crate::ui::render::render(self, window);
    }
    pub(crate) fn apply_menu_action(&mut self, action: menu::MenuAction) {
        crate::menu::apply::apply_action(self, action);
    }

    /// Sync `self.menu.options` from the runtime's authoritative
    /// state. Called once per UI frame before the menu draws so
    /// the screen reflects any external mutation (e.g. the chat
    /// `/name` command).
    pub(crate) fn sync_menu_options(&mut self) {
        self.menu.options.player_name = self.player_name.clone();
        self.menu.options.master_addr = self.master_addr.clone();
        self.menu.options.view_distance = self.settings.view_distance;
        self.menu.options.fov_deg = self.fov_y_rad.to_degrees();
        self.menu.options.mouse_sensitivity = self.settings.mouse_sensitivity;
        self.menu.options.show_fps = self.settings.show_fps;
        self.menu.options.head_bob = self.settings.head_bob;
        self.menu.options.invert_mouse_y = self.settings.invert_mouse_y;
        self.menu.options.invert_wheel = self.settings.invert_wheel;
        self.menu.options.ui_scale = self.settings.ui_scale;
        self.menu.options.lock_cursor = self.settings.lock_cursor;
        self.menu.options.hide_cursor = self.settings.hide_cursor;
        // W9.7: surface the live resource-pack state. `mc_assets_path`
        // is the user-typed override (mirrored back from
        // self.settings if set); the resolved + fallback fields are
        // read-only labels.
        self.menu.options.mc_assets_path = self
            .settings
            .mc_assets_path
            .clone()
            .unwrap_or_default();
        self.menu.options.mc_assets_resolved =
            self.resource_pack.source.display().to_string();
        self.menu.options.mc_assets_is_fallback = self.resource_pack.is_fallback;
        // W124: UI language.
        self.menu.options.language = self.settings.language.clone();
    }

    /// Refresh the menu's worlds + saved-servers caches if they've
    /// gone stale (>= 500 ms since the last scan). Caps the
    /// `list_worlds()` rate at <= 2 Hz so the OnceLock-deduped
    /// "unreadable slot" warning doesn't fire repeatedly while the
    /// picker is open.
    pub(crate) fn refresh_menu_caches(&mut self) {
        if self.menu.worlds_cache_stale() {
            let fresh = world_save::layout::list_worlds();
            self.menu.store_worlds_cache(fresh);
        }
        if self.menu.saved_servers_stale() {
            let fresh = menu::multiplayer::load_saved_servers();
            self.menu.store_saved_servers(fresh);
        }
    }
}

