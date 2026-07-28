//! W47.28 - save_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// Resolve the config file path used for settings persistence.
    /// Lives next to the exe in release builds so the user can nuke the
    /// file without finding a hidden appdata directory.
    ///
    /// W88: prefer an existing config, including the sibling build
    /// profile's. A `cargo run` (debug) build and a `--release` build
    /// live in sibling `target/{debug,release}` dirs; sharing one config
    /// means `mc_assets_path` (and every other setting) set while
    /// running one profile is seen by the other - so debug runs get the
    /// MC textures the release config points at, instead of falling back
    /// to the placeholder atlas. Falls back to a fresh file next to the
    /// current exe when none exists yet.
    pub(crate) fn config_path() -> std::path::PathBuf {
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let here = dir.join("voxel_demo_mp.config");
            if here.exists() {
                return here;
            }
            if let Some(target_dir) = dir.parent() {
                for sib in ["release", "debug"] {
                    let p = target_dir.join(sib).join("voxel_demo_mp.config");
                    if p.exists() {
                        return p;
                    }
                }
            }
            return here;
        }
        std::path::PathBuf::from("voxel_demo_mp.config")
    }

    /// Resolve the optional message-of-the-day file path. Lives
    /// next to the exe so a host can hand-edit `motd.txt`. None
    /// when the file isn't present.
    pub(crate) fn motd_lines() -> Vec<String> {
        let path = if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            dir.join("motd.txt")
        } else {
            std::path::PathBuf::from("motd.txt")
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(8)
                .map(|s| s.to_string())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// W10.3: process a single Space key event for the double-tap
    /// fly toggle. `now_pressed` is the current frame's
    /// `is_pressed(Key::Space)`; `dt` is the frame delta. Returns
    /// `true` when the toggle fired (so the caller can suppress
    /// the regular jump path for that frame).
    ///
    /// Logic:
    /// - Only the rising edge counts (`!space_prev && now_pressed`).
    /// - A first rising edge starts a window of
    ///   `DOUBLE_TAP_WINDOW_SECS`; a second rising edge inside
    ///   the window flips `abilities.flying`.
    /// - `abilities.mayfly = false` makes the helper a no-op so
    ///   Survival players can't accidentally toggle.
    ///
    /// Mirrors net/minecraft/client/player/LocalPlayer.java in MC.
    pub(crate) fn tick_double_tap_fly(&mut self, now_pressed: bool, dt: f32) -> bool {
        // Advance the existing window timer first so it expires
        // even on frames where Space is not pressed.
        if let Some(t) = self.last_space_press.as_mut() {
            *t += dt;
            if *t > DOUBLE_TAP_WINDOW_SECS {
                self.last_space_press = None;
            }
        }
        let rising = now_pressed && !self.space_prev;
        self.space_prev = now_pressed;
        if !rising {
            return false;
        }
        // Survival / Adventure ignores Space taps for fly.
        if !self.abilities.mayfly {
            // Still reset the window so the player can swap to
            // Creative and the next pair of taps fires cleanly.
            self.last_space_press = None;
            return false;
        }
        // Second rising edge inside the window = toggle.
        if self.last_space_press.is_some() {
            self.abilities.flying = !self.abilities.flying;
            self.fly_mode = self.abilities.flying;
            self.last_space_press = None;
            if self.abilities.flying {
                // Zero the fall tracker so the player doesn't
                // take fall damage on takeoff. Mirrors the /fly
                // chat command's existing behaviour.
                self.player_vel = glam::Vec3::ZERO;
                self.fall_start_y = None;
            }
            true
        } else {
            self.last_space_press = Some(0.0);
            false
        }
    }

    /// W10.2: test-only accessor for the `dropped_items` list
    /// length. The field itself stays private + non-test callers
    /// reach it via `drops::tick_physics` / `drops::spawn`. This
    /// avoids exposing the whole Vec just so the runtime test
    /// suite can count entries.
    #[cfg(test)]
    pub(crate) fn dropped_items_len(&self) -> usize {
        self.dropped_items.len()
    }

    /// W10.1: recompute the cached `Abilities` bundle from the
    /// current `game_mode`. Called whenever `game_mode` changes
    /// (world load + `/gamemode` command). Preserves the live
    /// `flying` toggle when switching INTO Creative (the player
    /// keeps soaring) but force-disables it when switching to
    /// Survival so gravity resumes immediately.
    pub(crate) fn refresh_abilities(&mut self) {
        let prev_flying = self.abilities.flying;
        self.abilities = game_mode::Abilities::for_mode(self.game_mode);
        // Carry over the live flight toggle only when the new
        // mode still allows it (mayfly). Survival force-clears.
        if self.abilities.mayfly {
            self.abilities.flying = prev_flying;
        }
        // Sync the existing fly_mode flag so the physics
        // integrator + HUD that already read it pick up the
        // mode switch without a separate branch.
        self.fly_mode = self.abilities.flying;
    }

    /// Persist user-facing settings (name, sensitivity, toggles)
    /// to disk as a tiny key=value text file. Called from the
    /// Settings panel's Back button + on shutdown so the next run
    /// keeps the player's identity.
    pub(crate) fn save_config(&self) {
        use std::fmt::Write;
        let mut text = String::new();
        let _ = writeln!(text, "player_name={}", self.player_name);
        let _ = writeln!(
            text,
            "mouse_sensitivity={}",
            self.settings.mouse_sensitivity
        );
        let _ = writeln!(text, "show_fps={}", self.settings.show_fps);
        let _ = writeln!(text, "lock_cursor={}", self.settings.lock_cursor);
        let _ = writeln!(text, "hide_cursor={}", self.settings.hide_cursor);
        let _ = writeln!(text, "view_distance={}", self.settings.view_distance);
        let _ = writeln!(text, "lod_distance={}", self.settings.lod_distance);
        // W350: v2 key - the W349 overflow wrote 0 into every
        // config under the old key; reads heal old-key zeros to the
        // default while the v2 key persists an explicit 0 (off).
        let _ = writeln!(text, "lod2_distance_v2={}", self.settings.lod2_distance);
        let _ = writeln!(text, "lod3_distance={}", self.settings.lod3_distance);
        let _ = writeln!(text, "head_bob={}", self.settings.head_bob);
        let _ = writeln!(
            text,
            "invert_mouse_y={}",
            self.settings.invert_mouse_y
        );
        let _ = writeln!(
            text,
            "invert_wheel={}",
            self.settings.invert_wheel
        );
        let _ = writeln!(text, "ui_scale={}", self.settings.ui_scale);
        if let Some(p) = self.home_pos {
            let _ = writeln!(text, "home={},{},{}", p[0], p[1], p[2]);
        }
        if let Some(p) = self.spawn_pos {
            let _ = writeln!(text, "spawn={},{},{}", p[0], p[1], p[2]);
        }
        // Optional debug toggles persisted so a dev workflow
        // doesn't have to retype /fly /godmode each launch.
        if self.fly_mode {
            let _ = writeln!(text, "fly_mode=true");
        }
        if self.god_mode {
            let _ = writeln!(text, "god_mode=true");
        }
        // World seed persists for solo play so re-runs share
        // the same terrain. Hosted sessions still rely on
        // ServerStateV2's own seed field. W5.1: written from
        // voxel.base_seed; per-dim split happens at gen time.
        let _ = writeln!(text, "world_seed={}", self.base_seed);
        // W9.7: resource pack override path. Skip the key entirely
        // when None so the auto-discovery path stays the default.
        if let Some(p) = self.settings.mc_assets_path.as_deref() {
            let _ = writeln!(text, "mc_assets_path={p}");
        }
        // W124: UI language code.
        let _ = writeln!(text, "language={}", self.settings.language);
        let path = Self::config_path();
        if let Err(err) = std::fs::write(&path, text) {
            warn!(error = %err, path = %path.display(), "config save failed");
        }
    }

    /// Inverse of `save_config`. Silently ignores missing /
    /// malformed files - first launch just writes defaults the
    /// next time the user opens Settings.
    pub(crate) fn load_config(&mut self) {
        let path = Self::config_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "player_name" => {
                    let sane: String = value
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(24)
                        .collect();
                    if !sane.is_empty() {
                        self.player_name = sane;
                    }
                }
                "mouse_sensitivity" => {
                    if let Ok(v) = value.parse::<f32>() {
                        self.settings.mouse_sensitivity = v.clamp(0.05, 4.0);
                    }
                }
                "language" => {
                    // W124: lang codes are [a-z0-9_] stems.
                    let sane: String = value
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .take(16)
                        .collect();
                    if !sane.is_empty() {
                        self.settings.language = sane;
                    }
                }
                "show_fps" => {
                    self.settings.show_fps = matches!(value, "true" | "1" | "yes");
                }
                "lock_cursor" => {
                    self.settings.lock_cursor =
                        matches!(value, "true" | "1" | "yes");
                }
                "hide_cursor" => {
                    self.settings.hide_cursor =
                        matches!(value, "true" | "1" | "yes");
                }
                "view_distance" => {
                    if let Ok(v) = value.parse::<u8>() {
                        // W229.2: settings saved before the default
                        // went 4 -> 12 carry the old default; treat a
                        // stored 4 as "never chose" and migrate it (a
                        // deliberate low pick can re-lower it in
                        // Options / via /viewdistance).
                        let v = if v == 4 { 12 } else { v };
                        self.settings.view_distance =
                            v.clamp(2, VIEW_DISTANCE_MAX as u8);
                    }
                }
                "lod_distance" => {
                    if let Ok(v) = value.parse::<u8>() {
                        self.settings.lod_distance =
                            v.min(crate::lod::LOD_DISTANCE_MAX as u8);
                    }
                }
                "lod2_distance" => {
                    // W350: legacy key - every existing config holds a
                    // bug-zero here (W349 overflow), heal it to the
                    // default; real values pass through.
                    if let Ok(v) = value.parse::<u8>() {
                        self.settings.lod2_distance = if v == 0 {
                            crate::lod::LOD2_DISTANCE_DEFAULT
                        } else {
                            v.min(crate::lod::LOD2_DISTANCE_MAX as u8)
                        };
                    }
                }
                "lod2_distance_v2" => {
                    if let Ok(v) = value.parse::<u8>() {
                        self.settings.lod2_distance =
                            v.min(crate::lod::LOD2_DISTANCE_MAX as u8);
                    }
                }
                "lod3_distance" => {
                    if let Ok(v) = value.parse::<u16>() {
                        self.settings.lod3_distance =
                            v.min(crate::lod::LOD3_DISTANCE_MAX as u16);
                    }
                }
                "head_bob" => {
                    self.settings.head_bob =
                        matches!(value, "true" | "1" | "yes");
                }
                "invert_mouse_y" => {
                    self.settings.invert_mouse_y =
                        matches!(value, "true" | "1" | "yes");
                }
                "ui_scale" => {
                    if let Ok(v) = value.parse::<f32>() {
                        self.settings.ui_scale = v.clamp(0.5, 3.0);
                    }
                }
                "invert_wheel" => {
                    self.settings.invert_wheel =
                        matches!(value, "true" | "1" | "yes");
                }
                "mc_assets_path" if !value.is_empty() => {
                    // W9.7: persisted resource-pack override. Skip
                    // when empty so a manually-cleared config falls
                    // back to auto-discovery.
                    self.settings.mc_assets_path = Some(value.to_string());
                }
                "home" => {
                    let parts: Vec<&str> = value.split(',').collect();
                    if parts.len() == 3
                        && let (Ok(x), Ok(y), Ok(z)) = (
                            parts[0].trim().parse::<f32>(),
                            parts[1].trim().parse::<f32>(),
                            parts[2].trim().parse::<f32>(),
                        )
                    {
                        self.home_pos = Some([x, y, z]);
                    }
                }
                "spawn" => {
                    let parts: Vec<&str> = value.split(',').collect();
                    if parts.len() == 3
                        && let (Ok(x), Ok(y), Ok(z)) = (
                            parts[0].trim().parse::<f32>(),
                            parts[1].trim().parse::<f32>(),
                            parts[2].trim().parse::<f32>(),
                        )
                    {
                        self.spawn_pos = Some([x, y, z]);
                    }
                }
                "fly_mode" => {
                    self.fly_mode = matches!(value, "true" | "1" | "yes");
                }
                "god_mode" => {
                    self.god_mode = matches!(value, "true" | "1" | "yes");
                }
                "world_seed" => {
                    if let Ok(v) = value.parse::<u32>()
                        && v != 0
                    {
                        // W5.1: load into voxel.base_seed.
                        self.base_seed = v;
                    }
                }
                _ => {}
            }
        }
    }

    /// Notify the player visually that an autosave just landed.
    /// A short-lived chat line is the cheapest path that doesn't
    /// require a new HUD layer.
    pub(crate) fn note_save(&mut self) {
        self.chat_log.push((
            std::time::Instant::now(),
            "[autosave] World state saved.".to_string(),
        ));
    }

    /// Thin wrapper around `world_save::save_world`. Phase B.7a wired
    /// in the multi-world layer; this shim resolves the slug + gates
    /// on `Server` / `Offline` (clients are never authoritative).
    pub(crate) fn save_server_state(&mut self) {
        // Only the host writes saves; clients see snapshots on join.
        if !matches!(self.net, NetRole::Server { .. } | NetRole::Offline) {
            return;
        }
        let slug = self
            .current_world_slug
            .clone()
            .unwrap_or_else(|| world_save::DEFAULT_SLUG.to_string());
        if let Err(err) = world_save::save_world(self, &slug) {
            warn!(error = %err, "world_save: save_world failed");
        }
    }
}
