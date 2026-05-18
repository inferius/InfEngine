//! `meta.json` - per-save human-readable header.
//!
//! See `docs/design/save_architecture_final.md` ("meta.json") for
//! the canonical field list. Schema v1 fields:
//!
//! ```json
//! {
//!   "name": "First world",
//!   "slug": "first-world-2026-05-19",
//!   "format_version": 1,
//!   "created_at": "...",
//!   "last_played": "...",
//!   "play_time_seconds": 3825,
//!   "host_machine_id": "...",
//!   "engine_version": "0.0.1",
//!   "thumbnail_path": "thumbnail.png"
//! }
//! ```
//!
//! All writes go through the local `atomic_write` helper:
//! write to `<path>.tmp`, then rename onto the destination. Keeps
//! the file readable from a crashed process. The full `.bak`
//! rolling backup lands in Phase B.4 (`atomic.rs`).
#![allow(dead_code)]

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::layout::{meta_path, meta_path_in, world_dir, world_dir_in};

/// Schema version baked into every fresh `meta.json`. Bumped when
/// the struct fields change in a breaking way; Phase B.6 adds the
/// migration registry that lets older slots upgrade in place.
pub(crate) const META_FORMAT_VERSION: u32 = 1;

/// Hand-readable save header serialised next to the world payload.
///
/// Lives as JSON so the user can inspect a slot without us writing
/// a viewer. New fields land with `#[serde(default)]` to keep older
/// slots loadable; structurally breaking changes bump
/// `META_FORMAT_VERSION` instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WorldMeta {
    /// User-typed display name. Free-form UTF-8.
    pub(crate) name: String,
    /// Filesystem-safe identifier derived via `layout::slug()`.
    pub(crate) slug: String,
    /// `META_FORMAT_VERSION` at the time this slot was written.
    pub(crate) format_version: u32,
    /// UTC timestamp of the world creation.
    pub(crate) created_at: chrono::DateTime<chrono::Utc>,
    /// UTC timestamp of the last `world_save::save` call. Drives
    /// the "sort by last played" order in the Worlds menu and the
    /// time-warp catch-up.
    pub(crate) last_played: chrono::DateTime<chrono::Utc>,
    /// Aggregate wall-clock seconds the slot has been actively
    /// played. Incremented by the autosave loop.
    pub(crate) play_time_seconds: u64,
    /// Stable per-installation UUID. Lets the migration framework
    /// detect "this save came from a different machine" and
    /// surface a warning before we touch it. Persisted to
    /// `<data_dir>/host_machine_id` so it survives reinstalls of
    /// the binary.
    pub(crate) host_machine_id: String,
    /// `CARGO_PKG_VERSION` at the time of the write. Pure
    /// telemetry - the migration framework keys off
    /// `format_version`, not this string.
    pub(crate) engine_version: String,
    /// Relative path (under `world_dir`) of a 320x180 PNG taken
    /// on the most recent quit. `None` until Phase G adds the
    /// capture step.
    pub(crate) thumbnail_path: Option<String>,
}

/// Build a fresh `WorldMeta` for a brand-new slot. `created_at` /
/// `last_played` are set to `now()`, `play_time_seconds` is zero,
/// `host_machine_id` is loaded from the per-installation file.
pub(crate) fn create_meta(name: &str, slug: &str) -> WorldMeta {
    let now = chrono::Utc::now();
    WorldMeta {
        name: name.to_string(),
        slug: slug.to_string(),
        format_version: META_FORMAT_VERSION,
        created_at: now,
        last_played: now,
        play_time_seconds: 0,
        host_machine_id: host_machine_id(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        thumbnail_path: None,
    }
}

/// Read `<world_dir(slug)>/meta.json` and parse into `WorldMeta`.
///
/// Refuses unknown `format_version` values with an
/// `io::ErrorKind::InvalidData`; Phase B.6 wires the migration
/// registry into this path.
pub(crate) fn read_meta(slug: &str) -> io::Result<WorldMeta> {
    read_meta_from_path(&meta_path(slug))
}

/// Test-friendly variant: read from a slot rooted at an explicit
/// base path instead of the OS data dir.
pub(crate) fn read_meta_in_dir(base: &Path, slug: &str) -> io::Result<WorldMeta> {
    read_meta_from_path(&meta_path_in(base, slug))
}

fn read_meta_from_path(path: &Path) -> io::Result<WorldMeta> {
    let bytes = std::fs::read(path)?;
    let meta: WorldMeta = serde_json::from_slice(&bytes).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("meta.json parse error: {err}"),
        )
    })?;
    if meta.format_version != META_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "meta.json format_version {} not supported (expected {})",
                meta.format_version, META_FORMAT_VERSION
            ),
        ));
    }
    Ok(meta)
}

/// Write `meta.json` for a slot, creating the world directory if
/// missing. Uses `atomic_write` (tmp + rename) so a crashed write
/// never lands a half-flushed file on disk.
pub(crate) fn write_meta(slug: &str, meta: &WorldMeta) -> io::Result<()> {
    let dir = world_dir(slug);
    let path = meta_path(slug);
    write_meta_to(&dir, &path, meta)
}

/// Test-friendly variant: write to a slot rooted at an explicit
/// base path.
pub(crate) fn write_meta_in_dir(
    base: &Path,
    slug: &str,
    meta: &WorldMeta,
) -> io::Result<()> {
    let dir = world_dir_in(base, slug);
    let path = meta_path_in(base, slug);
    write_meta_to(&dir, &path, meta)
}

fn write_meta_to(dir: &Path, path: &Path, meta: &WorldMeta) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let payload = serde_json::to_vec_pretty(meta).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("meta.json encode error: {err}"),
        )
    })?;
    atomic_write(path, &payload)
}

/// Atomically write `payload` to `path`: serialise to
/// `<path>.tmp`, fsync, then `rename` over the destination. Phase
/// B.4 swaps this out for the full `atomic.rs` helper that also
/// keeps a `.bak` rolling backup.
pub(super) fn atomic_write(path: &Path, payload: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(path);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(payload)?;
        f.sync_all()?;
    }
    // Windows `rename` refuses to overwrite by default; remove the
    // destination first if present. Linux rename is atomic-over-
    // existing, so the explicit remove is a no-op there.
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

/// Stable per-installation UUID, persisted at
/// `<data_dir>/host_machine_id`. Generated lazily on first read
/// and reused thereafter. Falls back to a fresh UUID-shaped string
/// (not persisted) if the data dir is unwritable - acceptable for
/// tests + ephemeral environments.
fn host_machine_id() -> String {
    let Some(dirs) =
        directories::ProjectDirs::from("", "InfEngine", "voxel_demo_mp")
    else {
        return fresh_id();
    };
    let path = dirs.data_dir().join("host_machine_id");
    if let Ok(bytes) = std::fs::read(&path)
        && let Ok(s) = std::str::from_utf8(&bytes)
    {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let id = fresh_id();
    // Best-effort persist; ignore failures (read-only FS, etc).
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, id.as_bytes());
    id
}

/// Mint a UUID-shaped string without pulling in the `uuid` crate.
/// Combines the current nanosecond timestamp with a tiny xorshift
/// mix of the process id - good enough for "is this the same
/// install" identification, NOT a cryptographic UUID.
fn fresh_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mix = nanos ^ (pid.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    format!("{mix:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_meta_populates_required_fields() {
        let m = create_meta("My World", "my-world");
        assert_eq!(m.name, "My World");
        assert_eq!(m.slug, "my-world");
        assert_eq!(m.format_version, META_FORMAT_VERSION);
        assert_eq!(m.play_time_seconds, 0);
        assert!(!m.host_machine_id.is_empty());
        assert_eq!(m.engine_version, env!("CARGO_PKG_VERSION"));
        assert!(m.thumbnail_path.is_none());
        // created_at == last_played on a fresh slot.
        assert_eq!(m.created_at, m.last_played);
    }

    #[test]
    fn fresh_id_is_stable_format() {
        let a = fresh_id();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
