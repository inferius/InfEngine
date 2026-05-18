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
//! All writes go through `atomic::atomic_save`: write to
//! `<path>.tmp`, rename the previous live file into `<path>.bak`,
//! then atomic-rename `<path>.tmp` onto `<path>`. Reads go through
//! `atomic::read_with_recovery` so a corrupt `meta.json` falls back
//! to the `.bak` snapshot without losing the slot.
#![allow(dead_code)]

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::atomic::{atomic_save, read_with_recovery};
use super::layout::{meta_path, meta_path_in, world_dir, world_dir_in};
use super::migrate::{self, MigrationRegistry, SaveKind};

/// Schema version baked into every fresh `meta.json`. Bumped when
/// the struct fields change in a breaking way; Phase B.6 adds the
/// migration registry that lets older slots upgrade in place.
pub const META_FORMAT_VERSION: u32 = 1;

/// Hand-readable save header serialised next to the world payload.
///
/// Lives as JSON so the user can inspect a slot without us writing
/// a viewer. New fields land with `#[serde(default)]` to keep older
/// slots loadable; structurally breaking changes bump
/// `META_FORMAT_VERSION` instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorldMeta {
    /// User-typed display name. Free-form UTF-8.
    pub name: String,
    /// Filesystem-safe identifier derived via `layout::slug()`.
    pub slug: String,
    /// `META_FORMAT_VERSION` at the time this slot was written.
    pub format_version: u32,
    /// UTC timestamp of the world creation.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// UTC timestamp of the last `world_save::save` call. Drives
    /// the "sort by last played" order in the Worlds menu and the
    /// time-warp catch-up.
    pub last_played: chrono::DateTime<chrono::Utc>,
    /// Aggregate wall-clock seconds the slot has been actively
    /// played. Incremented by the autosave loop.
    pub play_time_seconds: u64,
    /// Stable per-installation UUID. Lets the migration framework
    /// detect "this save came from a different machine" and
    /// surface a warning before we touch it. Persisted to
    /// `<data_dir>/host_machine_id` so it survives reinstalls of
    /// the binary.
    pub host_machine_id: String,
    /// `CARGO_PKG_VERSION` at the time of the write. Pure
    /// telemetry - the migration framework keys off
    /// `format_version`, not this string.
    pub engine_version: String,
    /// Relative path (under `world_dir`) of a 320x180 PNG taken
    /// on the most recent quit. `None` until Phase G adds the
    /// capture step.
    pub thumbnail_path: Option<String>,
}

/// Build a fresh `WorldMeta` for a brand-new slot. `created_at` /
/// `last_played` are set to `now()`, `play_time_seconds` is zero,
/// `host_machine_id` is loaded from the per-installation file.
pub fn create_meta(name: &str, slug: &str) -> WorldMeta {
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
/// Refuses unknown `format_version` values unless the migration
/// registry has a chain to `META_FORMAT_VERSION`. Hands off to
/// `migrate::default_registry`.
pub fn read_meta(slug: &str) -> io::Result<WorldMeta> {
    read_meta_from_path(&meta_path(slug), migrate::default_registry())
}

/// Test-friendly variant: read from a slot rooted at an explicit
/// base path instead of the OS data dir.
pub fn read_meta_in_dir(base: &Path, slug: &str) -> io::Result<WorldMeta> {
    read_meta_from_path(&meta_path_in(base, slug), migrate::default_registry())
}

/// Same as `read_meta_in_dir` but lets the caller inject a custom
/// migration registry - the canonical hook for migration unit tests.
pub fn read_meta_in_dir_with_registry(
    base: &Path,
    slug: &str,
    registry: &MigrationRegistry,
) -> io::Result<WorldMeta> {
    read_meta_from_path(&meta_path_in(base, slug), registry)
}

fn read_meta_from_path(path: &Path, registry: &MigrationRegistry) -> io::Result<WorldMeta> {
    let migrated_at_path = std::cell::Cell::new(false);
    let parse = |bytes: &[u8]| -> io::Result<WorldMeta> {
        let (meta, did_migrate) = decode_meta(bytes, path, registry)?;
        if did_migrate {
            migrated_at_path.set(true);
        }
        Ok(meta)
    };
    match read_with_recovery(path, parse)? {
        Some(meta) => {
            if migrated_at_path.get() {
                // Persist the migrated payload at the current version
                // so subsequent loads don't re-run the chain.
                let payload = encode_meta(&meta)?;
                atomic_save(path, &payload)?;
            }
            Ok(meta)
        }
        None => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("meta.json missing at {}", path.display()),
        )),
    }
}

/// Parse + version-gate a `meta.json` byte buffer. Pulled out as a
/// free fn so both the primary read and the `.bak` recovery path
/// (inside `read_with_recovery`) share the exact same validation.
///
/// On a `format_version` mismatch the registry plans a migration
/// chain to `META_FORMAT_VERSION`. No chain -> refusal with a
/// `.error.<ts>` sidecar via `migrate::migrate_json`. Returns the
/// parsed struct + a flag indicating whether migration was applied
/// (the caller writes the new version back when it was).
fn decode_meta(
    bytes: &[u8],
    path: &Path,
    registry: &MigrationRegistry,
) -> io::Result<(WorldMeta, bool)> {
    let disk_version = migrate::peek_json_version(bytes)?;
    let did_migrate = disk_version != META_FORMAT_VERSION;
    let migrated = migrate::migrate_to(
        registry,
        SaveKind::Meta,
        META_FORMAT_VERSION,
        path,
        disk_version,
        bytes,
    )?;
    let meta: WorldMeta = serde_json::from_slice(&migrated).map_err(|err| {
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
    Ok((meta, did_migrate))
}

/// Serialise a `WorldMeta` into the same pretty-printed JSON used by
/// the writer. Used by the migration write-back path so the post-
/// migration file shape matches a normal `write_meta`.
fn encode_meta(meta: &WorldMeta) -> io::Result<Vec<u8>> {
    serde_json::to_vec_pretty(meta).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("meta.json encode error: {err}"),
        )
    })
}

/// Write `meta.json` for a slot, creating the world directory if
/// missing. Uses `atomic_write` (tmp + rename) so a crashed write
/// never lands a half-flushed file on disk.
pub fn write_meta(slug: &str, meta: &WorldMeta) -> io::Result<()> {
    let dir = world_dir(slug);
    let path = meta_path(slug);
    write_meta_to(&dir, &path, meta)
}

/// Test-friendly variant: write to a slot rooted at an explicit
/// base path.
pub fn write_meta_in_dir(base: &Path, slug: &str, meta: &WorldMeta) -> io::Result<()> {
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
    atomic_save(path, &payload)
}

/// Stable per-installation UUID, persisted at
/// `<data_dir>/host_machine_id`. Generated lazily on first read
/// and reused thereafter. Falls back to a fresh UUID-shaped string
/// (not persisted) if the data dir is unwritable - acceptable for
/// tests + ephemeral environments.
fn host_machine_id() -> String {
    let Some(dirs) = directories::ProjectDirs::from("", "InfEngine", "voxel_demo_mp") else {
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
