//! W469 - pre-load save-version check (the R39 dev-save policy).
//!
//! During development, format bumps ship WITHOUT migration arrows:
//! a world written by an older (or newer) dev build must refuse to
//! load - cleanly, before any world state streams in, with a
//! user-visible notice. This module is the cheap up-front probe
//! `enter_world` runs against a slot: it peeks the
//! `format_version` of every save family present on disk and
//! reports the first one the current build cannot read (neither
//! current nor reachable through a registered migration chain).
//!
//! Cost: a handful of 8-byte header reads + two small JSON parses;
//! chunks are SAMPLED (first file per dimension dir) since every
//! chunk in a save shares one format version.
//!
//! The deep read paths keep their own per-file refusal (see
//! `migrate::migrate_to`) - this probe exists so the failure
//! surfaces as a menu notice instead of a half-loaded world.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::chunks::CHUNK_FORMAT_VERSION;
use super::containers::CONTAINERS_FORMAT_VERSION;
use super::entities::ENTITIES_FORMAT_VERSION;
use super::layout::{world_dir, world_dir_in};
use super::meta::META_FORMAT_VERSION;
use super::migrate::{self, SaveKind, family_label};
use super::players::PLAYER_FORMAT_VERSION;
use super::timers::TIMERS_FORMAT_VERSION;
use super::world_json::WORLD_FORMAT_VERSION;

/// One unreadable save family: which file, what version it carries
/// and what this build expects. `family` is the human label from
/// `migrate::family_label`.
#[derive(Debug, Clone)]
pub(crate) struct VersionMismatch {
    pub(crate) family: &'static str,
    pub(crate) path: PathBuf,
    pub(crate) found: u32,
    pub(crate) expected: u32,
}

impl VersionMismatch {
    /// User-facing notice line (Czech - matches the menu strings).
    /// Names the family, both versions, and the cause so the
    /// player knows the world is from an incompatible dev build.
    pub(crate) fn notice_text(&self) -> String {
        format!(
            "Svet nelze nacist: soubor rodiny '{}' ma verzi {} , ale \
             tato verze hry ocekava {}. Svet byl vytvoren \
             nekompatibilni vyvojovou verzi hry (behem vyvoje se \
             stare formaty nemigruji - R39).",
            self.family, self.found, self.expected
        )
    }
}

/// Probe every save family under `worlds/<slug>/`. Returns
/// `Ok(None)` when everything present is readable (missing files
/// are fine - a fresh slot has none), `Ok(Some(..))` for the first
/// unreadable family found. IO errors from unreadable directories
/// bubble; a corrupt individual file is NOT flagged here (the load
/// path's own recovery + refusal handles corruption).
pub(crate) fn check_world_versions(
    slug: &str,
) -> io::Result<Option<VersionMismatch>> {
    check_world_dir(&world_dir(slug))
}

/// Test-friendly variant rooted at an explicit base dir.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn check_world_versions_in(
    base: &Path,
    slug: &str,
) -> io::Result<Option<VersionMismatch>> {
    check_world_dir(&world_dir_in(base, slug))
}

fn check_world_dir(world: &Path) -> io::Result<Option<VersionMismatch>> {
    if !world.is_dir() {
        // Fresh slot: nothing to refuse.
        return Ok(None);
    }
    // JSON-headed families (whole-file parse; both are small).
    for (name, kind, target) in [
        ("meta.json", SaveKind::Meta, META_FORMAT_VERSION),
        ("world.json", SaveKind::WorldJson, WORLD_FORMAT_VERSION),
    ] {
        let path = world.join(name);
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok(found) = migrate::peek_json_version(&bytes)
            && let Some(m) = mismatch(kind, target, found, &path)
        {
            return Ok(Some(m));
        }
    }
    // Envelope families: every file in the family dir.
    if let Some(m) = check_bin_dir(
        &world.join("players"),
        SaveKind::Player,
        PLAYER_FORMAT_VERSION,
        false,
    )? {
        return Ok(Some(m));
    }
    if let Some(m) = check_bin_dir(
        &world.join("entities"),
        SaveKind::Entities,
        ENTITIES_FORMAT_VERSION,
        false,
    )? {
        return Ok(Some(m));
    }
    if let Some(m) = check_bin_dir(
        &world.join("containers"),
        SaveKind::Containers,
        CONTAINERS_FORMAT_VERSION,
        false,
    )? {
        return Ok(Some(m));
    }
    if let Some(m) = check_bin_dir(
        &world.join("timers"),
        SaveKind::Timers,
        TIMERS_FORMAT_VERSION,
        false,
    )? {
        return Ok(Some(m));
    }
    // Chunks: one sample per dimension dir - a save's chunks all
    // share one format version, and a travelled world holds tens
    // of thousands of files.
    let chunks_root = world.join("chunks");
    if chunks_root.is_dir() {
        for entry in std::fs::read_dir(&chunks_root)?.flatten() {
            let dim_dir = entry.path();
            if !dim_dir.is_dir() {
                continue;
            }
            if let Some(m) = check_bin_dir(
                &dim_dir,
                SaveKind::Chunk,
                CHUNK_FORMAT_VERSION,
                true,
            )? {
                return Ok(Some(m));
            }
        }
    }
    Ok(None)
}

/// Peek every (or, with `sample_one`, the first) `.bin` file in
/// `dir` and report the first version this build cannot read.
fn check_bin_dir(
    dir: &Path,
    kind: SaveKind,
    target: u32,
    sample_one: bool,
) -> io::Result<Option<VersionMismatch>> {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("bin") {
            continue;
        }
        // Only the 8-byte envelope head is needed: magic + LE
        // version. Short / unreadable files are corruption, not a
        // version mismatch - the load path's recovery owns those.
        let Some(found) = peek_head_version(&path) else {
            continue;
        };
        if let Some(m) = mismatch(kind, target, found, &path) {
            return Ok(Some(m));
        }
        if sample_one {
            break;
        }
    }
    Ok(None)
}

/// Read bytes 4..8 (the LE format version of the shared envelope
/// layout). `None` on any read problem.
fn peek_head_version(path: &Path) -> Option<u32> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 8];
    f.read_exact(&mut head).ok()?;
    Some(u32::from_le_bytes([head[4], head[5], head[6], head[7]]))
}

/// `Some(mismatch)` when `found` is neither current nor reachable
/// through the registered migration chain for `kind`.
fn mismatch(
    kind: SaveKind,
    target: u32,
    found: u32,
    path: &Path,
) -> Option<VersionMismatch> {
    if found == target {
        return None;
    }
    if migrate::default_registry().plan(kind, found, target).is_some() {
        return None;
    }
    Some(VersionMismatch {
        family: family_label(kind),
        path: path.to_path_buf(),
        found,
        expected: target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SLUG: &str = "verworld";

    fn world(tmp: &TempDir) -> PathBuf {
        world_dir_in(tmp.path(), SLUG)
    }

    /// Craft a minimal envelope head: 4-byte magic + LE version.
    fn write_envelope(path: &Path, magic: &[u8; 4], version: u32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(magic);
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 16]);
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn missing_slot_passes() {
        let tmp = TempDir::new().unwrap();
        assert!(
            check_world_versions_in(tmp.path(), SLUG)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn current_players_file_passes() {
        let tmp = TempDir::new().unwrap();
        let save = super::super::players::fresh_player([0, 64, 0]);
        super::super::players::write_player_in_dir(
            tmp.path(),
            SLUG,
            "host",
            &save,
        )
        .unwrap();
        assert!(
            check_world_versions_in(tmp.path(), SLUG)
                .unwrap()
                .is_none()
        );
    }

    /// The mission-critical case: a far-future players file must be
    /// flagged BEFORE the world loads, naming family + versions.
    #[test]
    fn future_players_version_is_flagged() {
        let tmp = TempDir::new().unwrap();
        write_envelope(
            &world(&tmp).join("players").join("host.bin"),
            b"INFP",
            99,
        );
        let m = check_world_versions_in(tmp.path(), SLUG)
            .unwrap()
            .expect("v99 players must be refused");
        assert_eq!(m.family, "player");
        assert_eq!(m.found, 99);
        assert_eq!(m.expected, PLAYER_FORMAT_VERSION);
        let text = m.notice_text();
        assert!(text.contains("player"), "notice names family: {text}");
        assert!(text.contains("99"), "notice names found version: {text}");
        assert!(
            text.contains(&PLAYER_FORMAT_VERSION.to_string()),
            "notice names expected version: {text}"
        );
    }

    /// A v5 players file (the pre-W469 dev format) has no 5 -> 6
    /// arrow under R39 - it must be flagged too, not half-migrated.
    #[test]
    fn previous_dev_players_version_is_flagged() {
        let tmp = TempDir::new().unwrap();
        write_envelope(
            &world(&tmp).join("players").join("host.bin"),
            b"INFP",
            5,
        );
        let m = check_world_versions_in(tmp.path(), SLUG)
            .unwrap()
            .expect("v5 players must be refused (no dev arrows)");
        assert_eq!(m.found, 5);
        assert_eq!(m.expected, PLAYER_FORMAT_VERSION);
    }

    /// Old versions REACHABLE through registered arrows still pass
    /// (containers v6 -> v7 exists, so a v6 containers file loads).
    #[test]
    fn migratable_containers_version_passes() {
        let tmp = TempDir::new().unwrap();
        write_envelope(
            &world(&tmp).join("containers").join("overworld.bin"),
            b"INFB",
            6,
        );
        assert!(
            check_world_versions_in(tmp.path(), SLUG)
                .unwrap()
                .is_none(),
            "v6 containers reach v7 through the registered arrow"
        );
    }

    #[test]
    fn future_entities_version_is_flagged() {
        let tmp = TempDir::new().unwrap();
        write_envelope(
            &world(&tmp).join("entities").join("overworld.bin"),
            b"INFE",
            420,
        );
        let m = check_world_versions_in(tmp.path(), SLUG)
            .unwrap()
            .expect("future entities must be refused");
        assert_eq!(m.family, "entities");
        assert_eq!(m.found, 420);
    }
}
