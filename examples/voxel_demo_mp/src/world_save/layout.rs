//! Cross-platform on-disk layout for multi-world saves.
//!
//! Resolves the per-user `worlds/` directory and builds the
//! `meta.json` + `state.bin` paths for each save slot. The layout
//! is plumbed in this phase but the runtime still drives the
//! legacy single-file save (see `super::save` / `super::load`);
//! Phase G ("Worlds" menu) is what actually creates / lists slots
//! through these helpers.
//!
//! Target shape (Windows):
//!
//! ```text
//! %APPDATA%/InfEngine/voxel_demo_mp/worlds/<slug>/
//!   meta.json
//!   world/
//!     state.bin
//! ```
//!
//! On Linux: `~/.local/share/infengine/voxel_demo_mp/worlds/<slug>/`.
//! On macOS: `~/Library/Application Support/com.InfEngine.voxel_demo_mp/worlds/<slug>/`.
//!
//! Fallback when the `directories` crate can't resolve a data dir
//! (rare - sandboxed environments, missing $HOME): the current
//! working directory's `worlds/` sub-folder.
//!
//! Most of this module is dead code right now: only
//! `state_path("legacy")` is reached, by the legacy-save migration
//! in `super::load`. The rest (`slug`, `meta_path`, `WorldMeta`,
//! ...) is what Phase G's Worlds menu will reach for. Silenced
//! with `#[allow(dead_code)]` rather than `#[cfg(test)]`-fenced so
//! the next phase can wire it up without juggling cfg gates.
#![allow(dead_code)]

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// Project-dir qualifier triple passed to `ProjectDirs::from`.
///
/// - `qualifier`: reverse-DNS-ish bucket, empty for InfEngine.
/// - `organization`: top-level vendor folder ("InfEngine").
/// - `application`: per-binary sub-folder ("voxel_demo_mp") so
///   each example owns its own save tree.
const PROJECT_QUALIFIER: &str = "";
const PROJECT_ORG: &str = "InfEngine";
const PROJECT_APP: &str = "voxel_demo_mp";

/// Schema version baked into every fresh `meta.json`. Bumped when
/// the struct fields change; Phase G's load path refuses (or
/// migrates) older slots.
pub(crate) const META_FORMAT_VERSION: u32 = 1;

/// Root of the per-user worlds tree, e.g.
/// `%APPDATA%/InfEngine/voxel_demo_mp/worlds/`. Falls back to
/// `./worlds/` if `directories` can't resolve a data dir.
pub(crate) fn worlds_dir() -> PathBuf {
    match ProjectDirs::from(PROJECT_QUALIFIER, PROJECT_ORG, PROJECT_APP) {
        Some(dirs) => dirs.data_dir().join("worlds"),
        None => PathBuf::from("worlds"),
    }
}

/// Slugify a human-typed world name into something filesystem-safe.
///
/// Rules (deliberately strict so two players can't end up with
/// platform-divergent slugs):
/// - Lowercase ASCII letters and digits pass through.
/// - Spaces + underscores + every other character collapse to `-`.
/// - Leading / trailing dashes trimmed.
/// - Empty result becomes `"world"` so an all-symbol name still
///   produces a usable slot.
pub(crate) fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true; // start "after a dash" so leading
    // non-ascii characters don't emit a leading `-`
    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    // Strip trailing dash from the collapse pass above.
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return "world".to_string();
    }
    out
}

/// Per-slot directory: `worlds_dir()/<slug>/`.
pub(crate) fn world_dir(slug: &str) -> PathBuf {
    worlds_dir().join(slug)
}

/// On-disk world payload path: `<world_dir>/world/state.bin`.
///
/// Mirrors the eventual Phase B / G layout where chunks land
/// under a `world/` sub-dir alongside `mobs.bin`, `timers.bin`,
/// `history/edits.log`, etc.
pub(crate) fn state_path(slug: &str) -> PathBuf {
    world_dir(slug).join("world").join("state.bin")
}

/// Human-readable save header path: `<world_dir>/meta.json`.
pub(crate) fn meta_path(slug: &str) -> PathBuf {
    world_dir(slug).join("meta.json")
}

/// Hand-readable save header serialised next to `world/state.bin`.
///
/// Lives as JSON so the user can inspect a slot without us writing
/// a viewer. Future fields (worldgen variant struct, mods list, ...)
/// add to this with `#[serde(default)]` to keep older slots loadable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorldMeta {
    /// User-typed display name. Free-form UTF-8.
    pub(crate) name: String,
    /// Filesystem-safe identifier derived via `slug()`.
    pub(crate) slug: String,
    /// Worldgen variant tag (e.g. "classic", "flat", "amplified").
    /// Read by the chunk-source pick on load.
    pub(crate) worldgen: String,
    /// Noise seed for the worldgen. 0 = the loader picks a fresh
    /// random value on first load.
    pub(crate) seed: u32,
    /// RFC-3339 timestamp of the world creation (UTC). Stored as a
    /// string so a human can read it without parsing serde-output
    /// integer epoch seconds.
    pub(crate) created_at: String,
    /// RFC-3339 timestamp of the last `world_save::save` call.
    /// Drives the "sort by last played" order in the Worlds menu.
    pub(crate) last_played: String,
    /// Aggregate wall-clock seconds the slot has been actively
    /// played. Incremented by the autosave loop.
    pub(crate) play_time_seconds: u64,
    /// `META_FORMAT_VERSION` at the time this slot was written.
    pub(crate) format_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_lowercases_and_collapses_separators() {
        assert_eq!(slug("First World"), "first-world");
        assert_eq!(slug("My   World!!"), "my-world");
        assert_eq!(slug("__Caves__"), "caves");
        assert_eq!(slug("HELLO 2026"), "hello-2026");
    }

    #[test]
    fn slug_empty_or_pure_symbols_falls_back() {
        assert_eq!(slug(""), "world");
        assert_eq!(slug("!!!"), "world");
        assert_eq!(slug("---"), "world");
    }

    #[test]
    fn slug_strips_diacritics_into_dashes() {
        // Non-ASCII collapses to a single separator dash. Strict by
        // design - the Worlds menu shows `name` separately, the
        // slug is only used for the filesystem path.
        assert_eq!(slug("Žluťoučký kůň"), "lu-ou-k-k");
    }

    #[test]
    fn state_and_meta_paths_sit_under_world_dir() {
        let s = slug("First World");
        let sp = state_path(&s);
        let mp = meta_path(&s);
        assert!(sp.ends_with(std::path::Path::new("first-world/world/state.bin")));
        assert!(mp.ends_with(std::path::Path::new("first-world/meta.json")));
    }
}
