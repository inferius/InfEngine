//! Wave 4 main-menu module. Replaces the flat picker that the
//! pre-G builds rendered inline in `main.rs` with a Minecraft-style
//! three-screen hierarchy: Main -> Singleplayer / Multiplayer /
//! Options.
//!
//! The module is intentionally a small surface: a `MenuScreen` enum
//! plus form buffers held by `Menu`, and a `MenuAction` enum that
//! each screen emits when the user clicks something that needs the
//! runtime to react. `main.rs` drains the actions after the egui
//! pass exits and routes them through `Voxel::apply_menu_action`
//! (still an inline impl in `main.rs`).
//!
//! Per-screen draw fns live in their own sub-modules so the file
//! cap (500 LOC soft, 1000 LOC hard) keeps room to grow. The state
//! machine + dispatcher live here; the egui draw helpers live in
//! `shared.rs`; the screens themselves live in `main_screen.rs`,
//! `singleplayer.rs`, `multiplayer.rs`, `options.rs`.
//!
//! The `Menu` struct also owns a `worlds_cache` that holds the last
//! `list_worlds()` result + a timestamp. The Singleplayer screen
//! reads from the cache and triggers a fresh scan at most every
//! 500 ms; this caps the previous per-frame scan that was running
//! at 60+ Hz and spamming `tracing::warn` on unreadable slots.

// The bin wires each entry point in the G.6 commit; until then the
// dispatcher + draw fns are dead from main.rs's point of view.
// Silence the warnings at the module root rather than scattering
// allow attributes through each file.
#![allow(dead_code)]

use std::time::Instant;

use crate::VIEW_DISTANCE_MAX;
use crate::game_mode::GameMode;
use crate::world_save::layout::WorldSummary;

pub(crate) mod apply;
pub(crate) mod loading;
pub(crate) mod main_screen;
pub(crate) mod multiplayer;
pub(crate) mod new_world;
pub(crate) mod options;
pub(crate) mod pause;
pub(crate) mod shared;
pub(crate) mod singleplayer;

#[cfg(test)]
mod tests;

/// Top-level screen the menu is currently rendering. `InGame`
/// covers every gameplay-adjacent app state (Playing, Paused,
/// Settings-from-pause); the actual sub-state lives on `Voxel`
/// and the menu module never tries to inspect it.
///
/// Sub-screens carry their own form buffers: the world-picker's
/// search field, the server-list's selection, etc. Transitioning
/// into a screen always starts from the screen's `Default`, so the
/// user re-enters with a clean slate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum MenuScreen {
    /// Root screen. Four big buttons + footer.
    #[default]
    Main,
    /// World picker - browses on-disk slots, lets the user start
    /// or create one. `selected_slug` highlights the row the next
    /// action will operate on.
    Singleplayer {
        selected_slug: Option<String>,
        search: String,
        /// When `Some`, the new-world inline form is open. The
        /// string holds the typed world name.
        new_world_open: Option<String>,
        /// When `Some(slug)`, the delete-confirm modal is open and
        /// will operate on this slug if the user confirms.
        confirm_delete: Option<String>,
    },
    /// Server browser. Same shape as Singleplayer.
    Multiplayer {
        selected_addr: Option<String>,
        search: String,
        /// When `Some`, the direct-connect inline form is open.
        direct_connect_open: Option<DirectConnectForm>,
        /// When `Some`, the add-server inline form is open.
        add_server_open: Option<AddServerForm>,
        /// When `Some(addr)`, the delete-confirm modal is open.
        confirm_remove: Option<String>,
    },
    /// Options screen. Editable copies of `Voxel::settings` plus
    /// player name + master-server IP.
    Options,
    /// W8.2: dedicated new-world screen. Replaces the inline popup
    /// that the Singleplayer screen used in Wave 4. Carries the
    /// editable form buffers so re-entry doesn't lose the user's
    /// pending pick.
    NewWorld {
        /// Display name typed by the user. Validated via
        /// `validate_world_name` on submit.
        name: String,
        /// Seed text field. Empty = random; numeric = pinned.
        seed: String,
        /// Generator tag picked from the dropdown (Classic /
        /// Flat / Islands / Amplified / Realistic).
        generator: String,
        /// Per-world default game mode. Wave 10 honours the
        /// flag at runtime; Wave 8 just persists it.
        game_mode: GameMode,
    },
    /// Bucket for "not on a menu screen" - actual gameplay state
    /// lives on the `Voxel` struct (Playing / Paused / chest UI
    /// etc.). Used so the menu's transition matrix has a unique
    /// terminal state.
    InGame,
}

/// Inline form buffer for the direct-connect dialog. Lives on the
/// `Multiplayer` screen state so it survives re-renders without
/// the caller having to thread it through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirectConnectForm {
    pub(crate) addr: String,
}

/// Inline form buffer for the "Pridat server" dialog. `addr` is
/// `host:port`; `name` is a free-form label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AddServerForm {
    pub(crate) name: String,
    pub(crate) addr: String,
}

/// One user-saved persistent server entry. Mirrors
/// `infengine_net::master::ServerInfo` but trimmed to the fields a
/// manually-added entry can populate before the first connect.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SavedServer {
    pub(crate) name: String,
    pub(crate) addr: String,
}

/// Actions the menu emits when the user clicks something. Drained
/// by `main.rs` after the egui pass; each variant routes through
/// `Voxel::apply_menu_action` to do the actual work.
///
/// Note: derives `PartialEq` (not `Eq`) because `OptionsSnapshot`
/// carries an `f32` (`fov_deg`) - the standard library refuses
/// `Eq` for any float-bearing type. Direct equality checks land
/// inside the menu tests where the snapshot is built with exact
/// values, so the partial-equality semantics are sufficient.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MenuAction {
    /// Navigate to a different screen. Always resets the target
    /// screen's form buffers so re-entry is clean.
    SwitchScreen(MenuScreen),
    /// Top-level quit.
    Quit,
    /// Load `slug` from disk and start playing offline.
    PlayOffline { slug: String },
    /// Mint a new world slot from the new-world form. W8.2: now
    /// carries a parsed seed (None = random) + the picked game
    /// mode so the runtime persists both on the WorldJson at
    /// creation time.
    NewWorld {
        name: String,
        worldgen: String,
        seed: Option<u32>,
        game_mode: GameMode,
    },
    /// Delete `slug` and remove it from the picker.
    DeleteWorld { slug: String },
    /// Connect to `addr` (`host:port`) as a client.
    DirectConnect { addr: String },
    /// Re-query LAN + master server.
    BrowseServersRefresh,
    /// Append a manual entry to `servers.json` and rescan.
    AddSavedServer { name: String, addr: String },
    /// Drop a manual entry from `servers.json`.
    RemoveSavedServer { addr: String },
    /// Connect to `addr` as a client.
    JoinServer { addr: String },
    /// Apply options-screen changes back to `Voxel::settings` +
    /// persist via the existing `save_config`.
    ApplyOptions { snapshot: OptionsSnapshot },
    /// W9.7: re-run resource-pack discovery using the path in
    /// `OptionsSnapshot::mc_assets_path` (or the discovered
    /// default when that field is empty). Rebuilds the atlas +
    /// re-uploads it to the GPU.
    ReloadResourcePack { path: String },
}

/// Editable mirror of the runtime settings that the Options
/// screen lets the user tweak. The menu draw fn mutates this in
/// place; `Apply` ships the whole struct back via
/// `MenuAction::ApplyOptions` so the runtime can re-validate +
/// persist.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OptionsSnapshot {
    pub(crate) player_name: String,
    pub(crate) master_addr: String,
    pub(crate) view_distance: u8,
    pub(crate) fov_deg: f32,
    pub(crate) mouse_sensitivity: f32,
    pub(crate) show_fps: bool,
    pub(crate) head_bob: bool,
    pub(crate) invert_mouse_y: bool,
    pub(crate) invert_wheel: bool,
    pub(crate) lock_cursor: bool,
    pub(crate) hide_cursor: bool,
    /// W9.7: optional override for the MC resource pack location.
    /// Empty string = use the discovered default. Set via the
    /// Options screen's "Resource Pack" section + persisted to
    /// the config so the setting survives a restart.
    pub(crate) mc_assets_path: String,
    /// W9.7: read-only display of the path the loader actually
    /// resolved to + a flag for whether it loaded real MC textures
    /// vs the synthetic placeholder. The Options screen surfaces
    /// these via labels; the user can't edit them directly.
    pub(crate) mc_assets_resolved: String,
    pub(crate) mc_assets_is_fallback: bool,
    /// W124: UI language code (MC lang stem, e.g. "cs_cz").
    pub(crate) language: String,
    /// W292: GUI scale multiplier (0.5..=3.0).
    pub(crate) ui_scale: f32,
    /// W321: far LOD ring radius in chunks (0 = off).
    pub(crate) lod_distance: u8,
    /// W333: extra-far ring radius in chunks (0 = off).
    pub(crate) lod2_distance: u8,
    /// W435: horizon ring radius (chunks; 0 = off).
    pub(crate) lod3_distance: u16,
}

impl OptionsSnapshot {
    /// Clamp every field to its valid range. Called inside
    /// `apply_menu_action` so manually-edited config files can't
    /// inject out-of-range values via the menu.
    pub(crate) fn clamp(&mut self) {
        self.view_distance = self.view_distance.clamp(2, VIEW_DISTANCE_MAX as u8);
        self.lod_distance =
            self.lod_distance.min(crate::lod::LOD_DISTANCE_MAX as u8);
        self.lod2_distance =
            self.lod2_distance.min(crate::lod::LOD2_DISTANCE_MAX as u8);
        self.lod3_distance =
            self.lod3_distance.min(crate::lod::LOD3_DISTANCE_MAX as u16);
        self.fov_deg = self.fov_deg.clamp(30.0, 120.0);
        self.mouse_sensitivity = self.mouse_sensitivity.clamp(0.05, 4.0);
        self.ui_scale = self.ui_scale.clamp(0.5, 3.0);
        // Truncate display name at 24 chars (same cap as
        // `save_config` uses) and drop control bytes so the user
        // can't paste in something that breaks the chat protocol.
        let sane: String = self
            .player_name
            .chars()
            .filter(|c| !c.is_control())
            .take(24)
            .collect();
        self.player_name = sane;
        // W124: lang codes are [a-z0-9_] stems.
        self.language = self
            .language
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(16)
            .collect();
        if self.language.is_empty() {
            self.language = "en_us".to_string();
        }
    }
}

/// Per-frame snapshot the menu reads to render. Built by `main.rs`
/// from `Voxel` state once per draw call; the menu module never
/// touches `Voxel` directly so the screens stay testable.
#[derive(Debug)]
pub(crate) struct MenuContext<'a> {
    /// Cached `list_worlds()` result. Borrowed from
    /// `Menu::worlds_cache` so multiple screens see the same
    /// snapshot during one frame.
    pub(crate) worlds: &'a [WorldSummary],
    /// LAN scan + master-server query results.
    pub(crate) browser_servers: &'a [infengine_net::master::ServerInfo],
    /// Last refresh latency / error from the master query.
    pub(crate) browser_error: Option<&'a str>,
    /// Cached saved-servers from `servers.json`.
    pub(crate) saved_servers: &'a [SavedServer],
    /// Editable options buffer. Lives on `Menu` so the screen
    /// remembers the user's edits across redraws.
    pub(crate) options: &'a mut OptionsSnapshot,
    /// Currently-active world slug (so the Singleplayer screen
    /// can show a "playing now" hint on the matching row).
    pub(crate) current_slug: Option<&'a str>,
    /// World seed used by the footer label.
    pub(crate) world_seed: u32,
    /// Engine version pulled from `CARGO_PKG_VERSION` so the
    /// menu footer doesn't drift on a release bump.
    pub(crate) engine_version: &'a str,
    /// "Refresh in progress" flag for the multiplayer screen.
    pub(crate) browser_querying: bool,
    /// W21.1: current splash text line from `Menu::splash_text`.
    /// `None` before the first `enter_main` call.
    pub(crate) splash_text: Option<&'a str>,
    /// W21.4: MC resource pack display name ("Vanilla", "Custom",
    /// ...). Shown in the footer when `pack_block_count > 0`.
    pub(crate) pack_name: &'a str,
    /// W21.4: number of block textures in the loaded resource
    /// pack. 0 when running on the fallback/placeholder atlas.
    pub(crate) pack_block_count: usize,
}

/// Holds the screen + transient form buffers + the world-list
/// frame cache. One per `Voxel`.
pub(crate) struct Menu {
    pub(crate) screen: MenuScreen,
    /// Editable mirror of `Voxel::settings` for the Options
    /// screen. Initialised by the runtime on first construction
    /// + re-synced whenever the user opens Options.
    pub(crate) options: OptionsSnapshot,
    /// Cached `list_worlds()` snapshot. Refreshed when
    /// `worlds_cache_age()` exceeds `WORLDS_CACHE_TTL` so the
    /// picker doesn't re-scan the filesystem every frame.
    pub(crate) worlds_cache: Vec<WorldSummary>,
    pub(crate) worlds_cache_at: Option<Instant>,
    /// Cached saved-servers from `servers.json`. Same TTL as the
    /// worlds cache.
    pub(crate) saved_servers: Vec<SavedServer>,
    pub(crate) saved_servers_at: Option<Instant>,
    /// W21.1: current splash text line. Picked fresh in
    /// `enter_main()`; stays constant until the user leaves +
    /// re-enters the Main screen so the text doesn't flicker
    /// each frame.
    pub(crate) splash_text: Option<String>,
}

/// How often the menu re-scans the worlds dir + saved-servers
/// file. 500 ms is invisible to a user clicking through the menu
/// and well under the 60 Hz UI tick.
pub(crate) const CACHE_TTL: std::time::Duration =
    std::time::Duration::from_millis(500);

/// Pool of splash text lines shown on the Main screen.
/// Mirrors the pool in MC's `TitleScreen.java`. Pure ASCII only.
pub(crate) const SPLASH_LINES: &[&str] = &[
    "Now with mob skins!",
    "Open source!",
    "Made in Rust!",
    "Now Compiling!",
    "Wave 21!",
    "100% Procedural!",
    "Yo dawg I heard you like blocks",
    "Now with diamond pickaxes!",
    "Made by Michal!",
    "Caves and Cliffs - free DLC!",
    "Now with chunks!",
    "Creeper? Aw man.",
    "Also try InfEngine!",
    "No NaN voxels!",
    "Borrow checked!",
    "cargo clippy: no warnings!",
    "Multithreaded!",
    "Infinite terrain!",
    "Underground caves!",
    "Procedural dungeons!",
    "Day/night cycle!",
    "Mob physics!",
    "Multiplayer!",
    "LAN + master server!",
    "Cubic chunks!",
    "Written in safe Rust!",
    "Best voxel engine of 2026!",
    "Runs on Wayland AND X11!",
    "Windows too!",
    "Fast mesher!",
];

/// Pick a fresh splash line using a simple time-based index so
/// each call to `enter_main` gets a different line.
fn pick_splash() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let idx = (nanos as usize) % SPLASH_LINES.len();
    SPLASH_LINES[idx].to_string()
}

impl Menu {
    /// Build a fresh menu in the `Main` screen with the runtime's
    /// settings + an empty world cache. Callers populate the
    /// caches lazily via `refresh_caches_if_stale`.
    pub(crate) fn new(options: OptionsSnapshot) -> Self {
        Self {
            screen: MenuScreen::Main,
            options,
            worlds_cache: Vec::new(),
            worlds_cache_at: None,
            saved_servers: Vec::new(),
            saved_servers_at: None,
            splash_text: None,
        }
    }

    /// W21.1: called whenever the Main screen is (re-)entered.
    /// Picks a fresh splash line from `SPLASH_LINES` so re-entry
    /// from a sub-screen shows a different quip.
    pub(crate) fn enter_main(&mut self) {
        self.splash_text = Some(pick_splash());
    }

    /// Returns true if the worlds cache is older than `CACHE_TTL`
    /// or has never been populated. Lets the call site refresh
    /// only when needed.
    pub(crate) fn worlds_cache_stale(&self) -> bool {
        match self.worlds_cache_at {
            None => true,
            Some(at) => at.elapsed() >= CACHE_TTL,
        }
    }

    /// Replace the worlds cache + stamp the refresh timestamp.
    pub(crate) fn store_worlds_cache(&mut self, fresh: Vec<WorldSummary>) {
        self.worlds_cache = fresh;
        self.worlds_cache_at = Some(Instant::now());
    }

    /// Mirror of `worlds_cache_stale` for the saved-servers cache.
    pub(crate) fn saved_servers_stale(&self) -> bool {
        match self.saved_servers_at {
            None => true,
            Some(at) => at.elapsed() >= CACHE_TTL,
        }
    }

    /// Replace the saved-servers cache + stamp the refresh.
    pub(crate) fn store_saved_servers(&mut self, fresh: Vec<SavedServer>) {
        self.saved_servers = fresh;
        self.saved_servers_at = Some(Instant::now());
    }

    /// Force-invalidate caches so the next draw call refreshes
    /// them. Called after `NewWorld` / `DeleteWorld` /
    /// `AddSavedServer` so the new row shows up immediately.
    pub(crate) fn invalidate_caches(&mut self) {
        self.worlds_cache_at = None;
        self.saved_servers_at = None;
    }

    /// `Esc`-key handler. Returns `true` if the menu consumed the
    /// keystroke + advanced to a different screen. Returns `false`
    /// when on `Main` or `InGame` so the caller's own handler
    /// (which knows about chat / chests / pause) takes over.
    pub(crate) fn on_escape(&mut self) -> bool {
        match self.screen {
            MenuScreen::Singleplayer { .. }
            | MenuScreen::Multiplayer { .. }
            | MenuScreen::Options => {
                self.screen = MenuScreen::Main;
                true
            }
            // W8.2: ESC from the new-world screen returns to the
            // Singleplayer picker so the user doesn't drop back to
            // Main mid-form.
            MenuScreen::NewWorld { .. } => {
                self.screen = default_singleplayer();
                true
            }
            MenuScreen::Main | MenuScreen::InGame => false,
        }
    }

    /// Apply a `SwitchScreen` action by overwriting `self.screen`.
    /// Kept as a free fn so call sites stay explicit + so tests
    /// don't have to set up the full action loop.
    pub(crate) fn apply_switch(&mut self, target: MenuScreen) {
        self.screen = target;
    }
}

/// Build a fresh `Singleplayer` screen with empty form buffers.
/// Used as the target of `SwitchScreen(default_singleplayer())`
/// from the Main screen so a re-entry starts clean.
pub(crate) fn default_singleplayer() -> MenuScreen {
    MenuScreen::Singleplayer {
        selected_slug: None,
        search: String::new(),
        new_world_open: None,
        confirm_delete: None,
    }
}

/// W8.2: build a fresh `NewWorld` screen. The form starts with
/// the documented defaults: empty name + seed, the faithful MC
/// generator (W184), Survival mode. Reset on each re-entry from the
/// Singleplayer screen.
pub(crate) fn default_new_world() -> MenuScreen {
    MenuScreen::NewWorld {
        name: String::new(),
        seed: String::new(),
        generator: "mc".to_string(),
        game_mode: GameMode::Survival,
    }
}

/// W8.2: parse a seed text field. Empty input lands as `None`
/// (= runtime picks a fresh random value); any string that parses
/// as a `u32` pins exactly that value. Strings that don't parse
/// also yield `None` so a typo doesn't accidentally lock the user
/// out of the form.
pub(crate) fn parse_seed_field(typed: &str) -> Option<u32> {
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u32>().ok()
}

/// W8.2: parse a seed text field with negative-number fallback.
/// Accepts `i32` so a user can type a seed like `-42` (lots of
/// MC-era saves do); we wrap the signed value into the matching
/// `u32` bit pattern.
#[allow(dead_code)] // kept for a future "accept negative seeds" toggle
pub(crate) fn parse_seed_field_signed(typed: &str) -> Option<u32> {
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(v) = trimmed.parse::<u32>() {
        return Some(v);
    }
    trimmed.parse::<i32>().ok().map(|v| v as u32)
}

/// Build a fresh `Multiplayer` screen with empty form buffers.
pub(crate) fn default_multiplayer() -> MenuScreen {
    MenuScreen::Multiplayer {
        selected_addr: None,
        search: String::new(),
        direct_connect_open: None,
        add_server_open: None,
        confirm_remove: None,
    }
}

/// Sanity-check + slugify a new-world name. Returns the cleaned
/// display name (trimmed, length-capped) when valid; `None` when
/// the name is empty / consists entirely of control bytes so the
/// "Create" button can stay disabled.
///
/// Called by the singleplayer screen draw fn AND by the menu
/// tests; lifted to module scope so both share the same rules.
pub(crate) fn validate_world_name(typed: &str) -> Option<String> {
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cleaned: String =
        trimmed.chars().filter(|c| !c.is_control()).take(64).collect();
    if cleaned.trim().is_empty() {
        return None;
    }
    Some(cleaned)
}

/// Top-level draw dispatcher. The egui closure in `main.rs` calls
/// this once per frame; the screens push their actions into
/// `actions`. Pulled out as a free fn (rather than a method) so
/// tests can hand it a synthetic context without spinning up a
/// real `Voxel`.
///
/// Takes `screen` separately from `MenuContext::options` so the
/// caller can hand us two disjoint sub-borrows of `Menu` without
/// tripping Rust's aliasing rules. The dispatcher restores the
/// active variant on the way out + applies the most recent
/// `SwitchScreen` action found in `actions`.
pub(crate) fn draw(
    ui: &mut egui::Ui,
    screen_ref: &mut MenuScreen,
    ctx: &mut MenuContext<'_>,
    actions: &mut Vec<MenuAction>,
) {
    // Snapshot the current screen so we can re-borrow `menu` for
    // each per-screen draw. The screens own their state (the
    // enum's payload), so we move/restore the variant rather
    // than threading two borrows in parallel.
    let screen = std::mem::replace(screen_ref, MenuScreen::Main);
    let next = match screen {
        MenuScreen::Main => {
            main_screen::draw(ui, ctx, actions);
            MenuScreen::Main
        }
        MenuScreen::Singleplayer {
            mut selected_slug,
            mut search,
            mut new_world_open,
            mut confirm_delete,
        } => {
            singleplayer::draw(
                ui,
                ctx,
                &mut selected_slug,
                &mut search,
                &mut new_world_open,
                &mut confirm_delete,
                actions,
            );
            MenuScreen::Singleplayer {
                selected_slug,
                search,
                new_world_open,
                confirm_delete,
            }
        }
        MenuScreen::Multiplayer {
            mut selected_addr,
            mut search,
            mut direct_connect_open,
            mut add_server_open,
            mut confirm_remove,
        } => {
            multiplayer::draw(
                ui,
                ctx,
                &mut selected_addr,
                &mut search,
                &mut direct_connect_open,
                &mut add_server_open,
                &mut confirm_remove,
                actions,
            );
            MenuScreen::Multiplayer {
                selected_addr,
                search,
                direct_connect_open,
                add_server_open,
                confirm_remove,
            }
        }
        MenuScreen::Options => {
            options::draw(ui, ctx, actions);
            MenuScreen::Options
        }
        MenuScreen::NewWorld {
            mut name,
            mut seed,
            mut generator,
            mut game_mode,
        } => {
            new_world::draw(
                ui,
                ctx,
                &mut name,
                &mut seed,
                &mut generator,
                &mut game_mode,
                actions,
            );
            MenuScreen::NewWorld {
                name,
                seed,
                generator,
                game_mode,
            }
        }
        MenuScreen::InGame => {
            // No-op: the menu isn't drawn while in game; the
            // dispatcher should never reach this branch but we
            // restore the variant rather than asserting so a
            // misuse doesn't crash a release build.
            MenuScreen::InGame
        }
    };
    // Apply any `SwitchScreen` action the screens emitted by
    // overwriting the restored variant. We do this here (instead
    // of inside each draw fn) so the menu's transition logic
    // sits in one place + tests can assert "after one
    // `SwitchScreen` action, the next-frame screen matches".
    let switch_target = actions.iter().rev().find_map(|a| match a {
        MenuAction::SwitchScreen(t) => Some(t.clone()),
        _ => None,
    });
    *screen_ref = switch_target.unwrap_or(next);
}

/// Top-level draw dispatcher that also carries the `Menu` struct
/// so animated state (splash text) can be refreshed on screen
/// transitions. Called from `main.rs` in place of `draw`.
pub(crate) fn draw_with_menu(
    ui: &mut egui::Ui,
    menu: &mut Menu,
    ctx: &mut MenuContext<'_>,
    actions: &mut Vec<MenuAction>,
) {
    // Snapshot the current screen for the transition matrix.
    let was_main = matches!(menu.screen, MenuScreen::Main);
    // Borrow the screen ref separately so the menu can be
    // updated after the draw call.
    let screen = std::mem::replace(&mut menu.screen, MenuScreen::Main);
    let next = match screen {
        MenuScreen::Main => {
            main_screen::draw(ui, ctx, actions);
            MenuScreen::Main
        }
        MenuScreen::Singleplayer {
            mut selected_slug,
            mut search,
            mut new_world_open,
            mut confirm_delete,
        } => {
            singleplayer::draw(
                ui,
                ctx,
                &mut selected_slug,
                &mut search,
                &mut new_world_open,
                &mut confirm_delete,
                actions,
            );
            MenuScreen::Singleplayer {
                selected_slug,
                search,
                new_world_open,
                confirm_delete,
            }
        }
        MenuScreen::Multiplayer {
            mut selected_addr,
            mut search,
            mut direct_connect_open,
            mut add_server_open,
            mut confirm_remove,
        } => {
            multiplayer::draw(
                ui,
                ctx,
                &mut selected_addr,
                &mut search,
                &mut direct_connect_open,
                &mut add_server_open,
                &mut confirm_remove,
                actions,
            );
            MenuScreen::Multiplayer {
                selected_addr,
                search,
                direct_connect_open,
                add_server_open,
                confirm_remove,
            }
        }
        MenuScreen::Options => {
            options::draw(ui, ctx, actions);
            MenuScreen::Options
        }
        MenuScreen::NewWorld {
            mut name,
            mut seed,
            mut generator,
            mut game_mode,
        } => {
            new_world::draw(
                ui,
                ctx,
                &mut name,
                &mut seed,
                &mut generator,
                &mut game_mode,
                actions,
            );
            MenuScreen::NewWorld {
                name,
                seed,
                generator,
                game_mode,
            }
        }
        MenuScreen::InGame => MenuScreen::InGame,
    };
    let switch_target = actions.iter().rev().find_map(|a| match a {
        MenuAction::SwitchScreen(t) => Some(t.clone()),
        _ => None,
    });
    let resolved = switch_target.unwrap_or(next);
    // W21.1: if we just arrived at Main (either from a sub-screen
    // SwitchScreen or on the very first draw), pick a fresh
    // splash line so re-entry shows a different quip.
    let arriving_main = matches!(resolved, MenuScreen::Main) && !was_main;
    if arriving_main || menu.splash_text.is_none() {
        menu.enter_main();
    }
    menu.screen = resolved;
}
