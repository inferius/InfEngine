//! Cross-platform on-disk layout for multi-world saves.
//!
//! Resolves the per-user `worlds/` directory and builds the paths
//! for each save slot. The layout is plumbed in this phase but the
//! runtime still drives the legacy single-file save
//! (see `super::save` / `super::load`); Phase G ("Worlds" menu) is
//! what actually creates / lists slots through these helpers.
//!
//! Target shape (Windows):
//!
//! ```text
//! %APPDATA%/InfEngine/voxel_demo_mp/worlds/<slug>/
//!   meta.json
//!   world.json
//!   chunks/<dim>/cx_cy_cz.chunk.bin
//!   entities/<dim>.bin
//!   players/<name>.bin
//!   timers/scheduled.bin
//!   history/edits.log
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
//! in `super::load`. The rest is what Phase B (`meta.rs`,
//! `world_json.rs`) and Phase G (Worlds menu) reach for. Silenced
//! with `#[allow(dead_code)]` rather than `#[cfg(test)]`-fenced so
//! the next phase can wire it up without juggling cfg gates.
#![allow(dead_code)]

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use super::meta::{WorldMeta, read_meta_in_dir};

/// Project-dir qualifier triple passed to `ProjectDirs::from`.
///
/// - `qualifier`: reverse-DNS-ish bucket, empty for InfEngine.
/// - `organization`: top-level vendor folder ("InfEngine").
/// - `application`: per-binary sub-folder ("voxel_demo_mp") so
///   each example owns its own save tree.
const PROJECT_QUALIFIER: &str = "";
const PROJECT_ORG: &str = "InfEngine";
const PROJECT_APP: &str = "voxel_demo_mp";

/// Root of the per-user worlds tree, e.g.
/// `%APPDATA%/InfEngine/voxel_demo_mp/worlds/`. Falls back to
/// `./worlds/` if `directories` can't resolve a data dir.
pub(crate) fn worlds_dir() -> PathBuf {
    match ProjectDirs::from(PROJECT_QUALIFIER, PROJECT_ORG, PROJECT_APP) {
        Some(dirs) => dirs.data_dir().join("worlds"),
        None => PathBuf::from("worlds"),
    }
}

/// Engine-wide data dir (the parent of `worlds/`). Used by sibling
/// caches like the `.jar`-extract cache the resource-pack loader
/// writes to. Falls back to `./` if `directories` can't resolve.
pub(crate) fn data_dir() -> PathBuf {
    match ProjectDirs::from(PROJECT_QUALIFIER, PROJECT_ORG, PROJECT_APP) {
        Some(dirs) => dirs.data_dir().to_path_buf(),
        None => PathBuf::from("."),
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
pub fn slug(name: &str) -> String {
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
pub fn world_dir(slug: &str) -> PathBuf {
    worlds_dir().join(slug)
}

/// Same as `world_dir` but rooted at an explicit base path - used
/// by the integration tests to redirect save I/O at a `tempfile`
/// directory.
pub fn world_dir_in(base: &Path, slug: &str) -> PathBuf {
    base.join(slug)
}

/// On-disk world payload path: `<world_dir>/world/state.bin`.
///
/// Legacy layout from the pre-multi-world era. Kept reachable for
/// the one-shot migration in `super::load`; the new layout writes
/// chunks under `<world_dir>/chunks/<dim>/...` instead.
pub(crate) fn state_path(slug: &str) -> PathBuf {
    world_dir(slug).join("world").join("state.bin")
}

/// Human-readable save header path: `<world_dir>/meta.json`.
pub(crate) fn meta_path(slug: &str) -> PathBuf {
    world_dir(slug).join("meta.json")
}

/// Same as `meta_path` but rooted at an explicit base path.
pub(crate) fn meta_path_in(base: &Path, slug: &str) -> PathBuf {
    world_dir_in(base, slug).join("meta.json")
}

/// World configuration path: `<world_dir>/world.json`.
pub(crate) fn world_json_path(slug: &str) -> PathBuf {
    world_dir(slug).join("world.json")
}

/// Same as `world_json_path` but rooted at an explicit base path.
pub(crate) fn world_json_path_in(base: &Path, slug: &str) -> PathBuf {
    world_dir_in(base, slug).join("world.json")
}

/// One row in the Worlds menu list - flattened projection of
/// `WorldMeta` + `WorldJson` so the UI doesn't have to open both
/// files itself.
#[derive(Debug, Clone)]
pub(crate) struct WorldSummary {
    /// Display name from `meta.json`.
    pub(crate) name: String,
    /// Filesystem slug (directory name).
    pub(crate) slug: String,
    /// Worldgen variant from `world.json` (empty string if missing
    /// so a half-created slot still shows up in the list).
    pub(crate) worldgen: String,
    /// Worldgen seed from `world.json` (0 if missing).
    pub(crate) seed: u32,
    /// Last-played timestamp (UTC). Drives the sort order.
    pub(crate) last_played: chrono::DateTime<chrono::Utc>,
    /// Aggregate wall-clock play time across all sessions.
    pub(crate) play_time_seconds: u64,
}

impl WorldSummary {
    /// Build from a parsed `WorldMeta`. `worldgen` / `seed` come
    /// from `world.json` if present; otherwise default to
    /// `("", 0)` so the slot still lists.
    fn from_meta(meta: WorldMeta, worldgen: String, seed: u32) -> Self {
        Self {
            name: meta.name,
            slug: meta.slug,
            worldgen,
            seed,
            last_played: meta.last_played,
            play_time_seconds: meta.play_time_seconds,
        }
    }
}

/// Scan `worlds_dir()` and return one entry per directory that
/// contains a parseable `meta.json`. Sorted by `last_played`
/// descending (newest first), matching the Worlds menu order.
///
/// Silent on per-slot errors - a corrupt `meta.json` in slot A
/// must not hide slots B + C. Errors land in the log instead.
pub(crate) fn list_worlds() -> Vec<WorldSummary> {
    list_worlds_in(&worlds_dir())
}

/// Dedup set for `list_worlds: skipping unreadable slot` warnings.
/// The Worlds picker calls `list_worlds()` once per UI frame, so a
/// permanently-broken slot (e.g. a `legacy/` dir without a
/// `meta.json`) would otherwise spam the log at 60+ Hz. We log once
/// per `(slug, error-text)` pair and stay quiet afterwards; if the
/// error text changes (the slot was repaired then re-broke
/// differently) it re-fires once.
fn note_unreadable_once(slug: &str, err: &io::Error) {
    use std::sync::Mutex;
    use std::sync::OnceLock;
    static SEEN: OnceLock<Mutex<HashSet<(String, String)>>> =
        OnceLock::new();
    let set = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let key = (slug.to_string(), err.to_string());
    let mut guard = match set.lock() {
        Ok(g) => g,
        // Poisoned mutex: recover the inner value rather than panic.
        // A poisoned set just means we may double-log once; harmless.
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.insert(key) {
        tracing::warn!(
            slug = slug,
            error = %err,
            "list_worlds: skipping unreadable slot (logged once per slug)"
        );
    }
}

/// Like `list_worlds` but rooted at an explicit base path.
pub(crate) fn list_worlds_in(base: &Path) -> Vec<WorldSummary> {
    let read = match std::fs::read_dir(base) {
        Ok(r) => r,
        // Missing root = no worlds yet. Not an error.
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<WorldSummary> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(slug) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let meta = match read_meta_in_dir(base, slug) {
            Ok(m) => m,
            Err(err) => {
                note_unreadable_once(slug, &err);
                continue;
            }
        };
        // world.json is best-effort - a slot mid-creation may not
        // have it yet, but we still want to surface the entry.
        let (worldgen, seed) =
            match super::world_json::read_world_in_dir(base, slug) {
                Ok(w) => (w.worldgen, w.seed),
                Err(_) => (String::new(), 0),
            };
        out.push(WorldSummary::from_meta(meta, worldgen, seed));
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.last_played));
    out
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
        let wp = world_json_path(&s);
        assert!(sp.ends_with(std::path::Path::new("first-world/world/state.bin")));
        assert!(mp.ends_with(std::path::Path::new("first-world/meta.json")));
        assert!(wp.ends_with(std::path::Path::new("first-world/world.json")));
    }
}
