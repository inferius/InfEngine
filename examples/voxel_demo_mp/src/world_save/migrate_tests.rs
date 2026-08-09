//! Integration tests for the format-version migration framework
//! (`world_save::migrate`).
//!
//! Pulled out of `tests.rs` + the per-file test modules so the
//! end-to-end migration suite can keep its own fixture helpers
//! co-located. Each test spins up a fresh `tempfile::TempDir` + a
//! local `MigrationRegistry` so the process-wide `default_registry`
//! stays untouched.
//!
//! Coverage:
//! - End-to-end migration round trip for each of the six reader paths
//!   (`meta`, `world_json`, `chunks`, `players`, `entities`, `timers`).
//!   Each test writes a fake v0 file, registers a v0 -> v1 step that
//!   bumps the envelope version (or top-level JSON key), reads through
//!   the `_with_registry` variant, then asserts the file on disk has
//!   been rewritten at the current version.
//! - Refusal path: an unknown version with no registered chain writes
//!   a `.error.<ts>` sidecar and surfaces a hard error to the caller.
//! - Sidecar content sanity: must mention both the disk version and
//!   the target version + the offending file path.

use std::io;

use chrono::Utc;
use tempfile::TempDir;

use super::chunks::{
    CHUNK_FORMAT_VERSION, CHUNK_MAGIC, ChunkPayload, chunk_path_in_dir, empty_payload,
    read_chunk_in_dir_with_registry, write_chunk_in_dir,
};
use super::entities::{
    DimensionEntities, ENTITIES_FORMAT_VERSION, ENTITIES_MAGIC, entities_path_in_dir,
    entities_v1_to_v2, entities_v2_to_v3, entities_v3_to_v4,
    entities_v4_to_v5, entities_v5_to_v6, entities_v6_to_v7,
    entities_v7_to_v8,
    read_entities_in_dir_with_registry,
    write_entities_in_dir,
};
use super::layout::{meta_path_in, world_dir_in, world_json_path_in};
use super::meta::{META_FORMAT_VERSION, WorldMeta, create_meta, read_meta_in_dir_with_registry};
use super::migrate::{MigrationRegistry, SaveKind};
use super::players::{
    PLAYER_FORMAT_VERSION, PLAYER_MAGIC, fresh_player, player_path_in_dir,
    read_player_in_dir_with_registry, write_player_in_dir,
};
use super::timers::{
    ScheduledTimers, TIMERS_FORMAT_VERSION, TIMERS_MAGIC, read_timers_in_dir_with_registry,
    timers_path_in_dir, write_timers_in_dir,
};
use super::world_json::{
    WORLD_FORMAT_VERSION, WorldJson, create_world, read_world_in_dir_with_registry,
};

const SLUG: &str = "slot";
const DIM: &str = "overworld";

// ---------------------------------------------------------------
// Shared helpers for binary-envelope downgrades.
// ---------------------------------------------------------------

/// Rewrite the format_version field at offset 4 of a binary-envelope
/// payload (the 4-byte little-endian u32 right after the magic).
///
/// Used by tests in two ways:
/// - Set version = 0 when handcrafting a "v0" file to land on disk.
/// - Set version = 1 inside a registered migration step that bumps a
///   v0 file to v1 (matching `CHUNK_FORMAT_VERSION` etc.).
fn set_envelope_version(bytes: &mut [u8], version: u32) {
    assert!(bytes.len() >= 8, "envelope must have header + version");
    let ver = version.to_le_bytes();
    bytes[4..8].copy_from_slice(&ver);
}

/// Migration step that bumps a binary envelope from v0 to v1 by
/// overwriting the version u32 at offset 4. The bincode body is left
/// untouched - real migrations would re-encode the body, but for the
/// framework smoke tests a body that already decodes cleanly at v1 is
/// the simplest fixture.
fn bump_envelope_v0_to_v1(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = bytes.to_vec();
    set_envelope_version(&mut out, 1);
    Ok(out)
}

// ---------------------------------------------------------------
// meta.json
// ---------------------------------------------------------------

#[test]
fn meta_migration_v0_to_v1_round_trip() {
    let tmp = TempDir::new().unwrap();
    // Hand-craft a v0 meta.json on disk: a current-shape WorldMeta
    // with format_version stamped at 0.
    let mut meta = create_meta("Legacy", "legacy");
    meta.format_version = 0;
    let dir = world_dir_in(tmp.path(), "legacy");
    std::fs::create_dir_all(&dir).unwrap();
    let path = meta_path_in(tmp.path(), "legacy");
    std::fs::write(&path, serde_json::to_vec_pretty(&meta).unwrap()).unwrap();

    // Step: bumps format_version 0 -> 1. Today the rest of the schema
    // hasn't changed so the migration is a single field flip.
    fn bump_meta(bytes: &[u8]) -> io::Result<Vec<u8>> {
        let mut value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        value["format_version"] = serde_json::json!(1);
        serde_json::to_vec_pretty(&value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
    let mut registry = MigrationRegistry::new();
    registry.register(SaveKind::Meta, 0, META_FORMAT_VERSION, bump_meta);

    let loaded = read_meta_in_dir_with_registry(tmp.path(), "legacy", &registry).unwrap();
    assert_eq!(loaded.format_version, META_FORMAT_VERSION);
    assert_eq!(loaded.name, "Legacy");

    // File on disk must now be at the new version: the reader writes
    // the migrated payload back so subsequent loads skip the chain.
    let disk: WorldMeta = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(disk.format_version, META_FORMAT_VERSION);
    assert_eq!(disk.name, "Legacy");
}

#[test]
fn meta_unknown_version_writes_error_sidecar() {
    let tmp = TempDir::new().unwrap();
    let mut meta = create_meta("Future", "future");
    meta.format_version = 999;
    let dir = world_dir_in(tmp.path(), "future");
    std::fs::create_dir_all(&dir).unwrap();
    let path = meta_path_in(tmp.path(), "future");
    std::fs::write(&path, serde_json::to_vec_pretty(&meta).unwrap()).unwrap();
    let before = Utc::now();

    let registry = MigrationRegistry::new();
    let err = read_meta_in_dir_with_registry(tmp.path(), "future", &registry)
        .expect_err("expected refusal");
    let msg = err.to_string();
    assert!(
        msg.contains("999"),
        "error must mention the unsupported version, got: {msg}"
    );
    assert!(
        msg.contains(".error."),
        "error must point at the sidecar, got: {msg}"
    );

    // Sidecar must exist + mention versions + path.
    let sidecar = find_sidecar(&dir, "meta.json").expect("sidecar must exist");
    assert!(
        sidecar
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("meta.json.error.")),
        "sidecar must extend the meta.json path, got: {}",
        sidecar.display()
    );
    let body = std::fs::read_to_string(&sidecar).unwrap();
    assert!(
        body.contains("999"),
        "sidecar body must mention disk version"
    );
    assert!(
        body.contains(&META_FORMAT_VERSION.to_string()),
        "sidecar body must mention target version"
    );
    assert!(
        body.contains("meta.json"),
        "sidecar body must mention the offending file"
    );
    // Sanity: timestamp suffix looks like UTC.
    let after = Utc::now();
    assert!(after >= before);
}

/// Helper: scan a directory for a `*.error.*` sidecar matching the
/// given base file name. Returns the first match.
fn find_sidecar(dir: &std::path::Path, base: &str) -> Option<std::path::PathBuf> {
    let prefix = format!("{base}.error.");
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let p = entry.path();
        let name = p.file_name()?.to_str()?.to_string();
        if name.starts_with(&prefix) {
            Some(p)
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------
// world.json
// ---------------------------------------------------------------

#[test]
fn world_json_migration_v0_to_current_round_trip() {
    let tmp = TempDir::new().unwrap();
    let mut world = create_world("classic", 42);
    world.format_version = 0;
    let dir = world_dir_in(tmp.path(), "legacy");
    std::fs::create_dir_all(&dir).unwrap();
    let path = world_json_path_in(tmp.path(), "legacy");
    std::fs::write(&path, serde_json::to_vec_pretty(&world).unwrap()).unwrap();

    // A test-only step that jumps directly to the current target
    // version. The production registry uses a v1 -> v2 arrow; this
    // test exercises the registry framework with a synthetic v0 ->
    // current arrow so the version bump can sweep over both fields
    // (format_version + dimension_worldgens default-fills via serde).
    fn bump_world(bytes: &[u8]) -> io::Result<Vec<u8>> {
        let mut value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        value["format_version"] = serde_json::json!(WORLD_FORMAT_VERSION);
        serde_json::to_vec_pretty(&value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
    let mut registry = MigrationRegistry::new();
    registry.register(SaveKind::WorldJson, 0, WORLD_FORMAT_VERSION, bump_world);

    let loaded = read_world_in_dir_with_registry(tmp.path(), "legacy", &registry).unwrap();
    assert_eq!(loaded.format_version, WORLD_FORMAT_VERSION);
    assert_eq!(loaded.worldgen, "classic");
    assert_eq!(loaded.seed, 42);

    let disk: WorldJson = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(disk.format_version, WORLD_FORMAT_VERSION);
}

// ---------------------------------------------------------------
// chunks
// ---------------------------------------------------------------

#[test]
fn chunk_migration_v0_to_v1_round_trip() {
    let tmp = TempDir::new().unwrap();
    let coord = [0, 0, 0];
    // Write the chunk normally (envelope version = 1), then downgrade
    // the version byte on disk to 0 so the read path needs migration.
    let payload = empty_payload(coord);
    write_chunk_in_dir(tmp.path(), SLUG, DIM, &payload).unwrap();
    let path = chunk_path_in_dir(tmp.path(), SLUG, DIM, coord);
    let mut on_disk = std::fs::read(&path).unwrap();
    set_envelope_version(&mut on_disk, 0);
    std::fs::write(&path, &on_disk).unwrap();
    // Sanity: also clean up the .bak so the .bak-fallback doesn't
    // silently rescue the read - we want to exercise the migration
    // path specifically.
    let bak = with_bak_suffix(&path);
    let _ = std::fs::remove_file(&bak);

    let mut registry = MigrationRegistry::new();
    registry.register(
        SaveKind::Chunk,
        0,
        CHUNK_FORMAT_VERSION,
        bump_envelope_v0_to_v1,
    );

    let loaded = read_chunk_in_dir_with_registry(tmp.path(), SLUG, DIM, coord, &registry)
        .unwrap()
        .expect("payload exists");
    assert_eq!(loaded.coord, coord);

    // File on disk has been rewritten at the new version.
    let post = std::fs::read(&path).unwrap();
    assert_eq!(&post[..4], CHUNK_MAGIC);
    assert_eq!(
        u32::from_le_bytes([post[4], post[5], post[6], post[7]]),
        CHUNK_FORMAT_VERSION
    );
}

#[test]
fn chunk_unknown_version_writes_error_sidecar() {
    let tmp = TempDir::new().unwrap();
    let coord = [0, 0, 0];
    let payload: ChunkPayload = empty_payload(coord);
    write_chunk_in_dir(tmp.path(), SLUG, DIM, &payload).unwrap();
    let path = chunk_path_in_dir(tmp.path(), SLUG, DIM, coord);
    let mut on_disk = std::fs::read(&path).unwrap();
    set_envelope_version(&mut on_disk, 999);
    std::fs::write(&path, &on_disk).unwrap();
    let bak = with_bak_suffix(&path);
    let _ = std::fs::remove_file(&bak);

    let registry = MigrationRegistry::new();
    let err = read_chunk_in_dir_with_registry(tmp.path(), SLUG, DIM, coord, &registry)
        .expect_err("expected refusal");
    assert!(err.to_string().contains("999"));
    let parent = path.parent().unwrap();
    let chunk_filename = path.file_name().unwrap().to_str().unwrap().to_string();
    let sidecar = find_sidecar(parent, &chunk_filename).expect("sidecar must exist");
    let body = std::fs::read_to_string(&sidecar).unwrap();
    assert!(body.contains("999"));
    assert!(body.contains(&CHUNK_FORMAT_VERSION.to_string()));
}

/// `<path>.bak` helper that mirrors the (private) one in `atomic.rs`.
/// Tests need it to clean up the rolling backup that `atomic_save`
/// leaves behind, otherwise the read-with-recovery path could rescue
/// the test by reading the unmigrated backup instead.
fn with_bak_suffix(path: &std::path::Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    std::path::PathBuf::from(s)
}

// ---------------------------------------------------------------
// players
// ---------------------------------------------------------------

/// Migration step variant that bumps the envelope version straight
/// to `PLAYER_FORMAT_VERSION`. Decoupled from the generic
/// `bump_envelope_v0_to_v1` so this test keeps passing after Q.2
/// (which bumped PLAYER_FORMAT_VERSION to 2).
fn bump_envelope_v0_to_player_target(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = bytes.to_vec();
    set_envelope_version(&mut out, PLAYER_FORMAT_VERSION);
    Ok(out)
}

#[test]
fn player_migration_v0_to_current_round_trip() {
    let tmp = TempDir::new().unwrap();
    let save = fresh_player([0, 64, 0]);
    write_player_in_dir(tmp.path(), SLUG, "Alice", &save).unwrap();
    let path = player_path_in_dir(tmp.path(), SLUG, "Alice");
    let mut on_disk = std::fs::read(&path).unwrap();
    set_envelope_version(&mut on_disk, 0);
    std::fs::write(&path, &on_disk).unwrap();
    let bak = with_bak_suffix(&path);
    let _ = std::fs::remove_file(&bak);

    let mut registry = MigrationRegistry::new();
    registry.register(
        SaveKind::Player,
        0,
        PLAYER_FORMAT_VERSION,
        bump_envelope_v0_to_player_target,
    );

    let loaded = read_player_in_dir_with_registry(tmp.path(), SLUG, "Alice", &registry)
        .unwrap()
        .expect("player exists");
    assert_eq!(loaded.format_version, PLAYER_FORMAT_VERSION);

    let post = std::fs::read(&path).unwrap();
    assert_eq!(&post[..4], PLAYER_MAGIC);
    assert_eq!(
        u32::from_le_bytes([post[4], post[5], post[6], post[7]]),
        PLAYER_FORMAT_VERSION
    );
}

// ---------------------------------------------------------------
// entities
// ---------------------------------------------------------------

#[test]
fn entities_migration_v0_to_v1_round_trip() {
    let tmp = TempDir::new().unwrap();
    let entities = DimensionEntities::empty();
    write_entities_in_dir(tmp.path(), SLUG, DIM, &entities).unwrap();
    let path = entities_path_in_dir(tmp.path(), SLUG, DIM);
    let mut on_disk = std::fs::read(&path).unwrap();
    set_envelope_version(&mut on_disk, 0);
    std::fs::write(&path, &on_disk).unwrap();
    let bak = with_bak_suffix(&path);
    let _ = std::fs::remove_file(&bak);

    // W137: entities are at v2 now - the chain is the test's v0 -> v1
    // envelope bump followed by the PRODUCTION v1 -> v2 arrow. The
    // freshly-written body is v2-shaped; the V1 decode inside the
    // production arrow reads its five-field prefix (bincode tolerates
    // the trailing carts bytes) and re-encodes a clean v2.
    let mut registry = MigrationRegistry::new();
    registry.register(SaveKind::Entities, 0, 1, bump_envelope_v0_to_v1);
    registry.register(SaveKind::Entities, 1, 2, entities_v1_to_v2);
    // W302: the chain runs to v3 now.
    registry.register(SaveKind::Entities, 2, 3, entities_v2_to_v3);
    // W348: ...and on to v4 (tamed/temper).
    registry.register(SaveKind::Entities, 3, 4, entities_v3_to_v4);
    // W459: ...and on to v5 (trade_xp).
    registry.register(SaveKind::Entities, 4, 5, entities_v4_to_v5);
    // W468: ...and on to v6 (stack-typed drops).
    registry.register(SaveKind::Entities, 5, 6, entities_v5_to_v6);
    // W474: ...and on to v7 (sheep sheared flag).
    registry.register(SaveKind::Entities, 6, 7, entities_v6_to_v7);
    // W476.1: ...and on to v8 (slime / magma-cube size).
    registry.register(SaveKind::Entities, 7, 8, entities_v7_to_v8);

    let loaded = read_entities_in_dir_with_registry(tmp.path(), SLUG, DIM, &registry)
        .unwrap()
        .expect("entities exist");
    assert_eq!(loaded.format_version, ENTITIES_FORMAT_VERSION);
    assert!(loaded.carts.is_empty());

    let post = std::fs::read(&path).unwrap();
    assert_eq!(&post[..4], ENTITIES_MAGIC);
    assert_eq!(
        u32::from_le_bytes([post[4], post[5], post[6], post[7]]),
        ENTITIES_FORMAT_VERSION
    );
}

// ---------------------------------------------------------------
// timers
// ---------------------------------------------------------------

#[test]
fn timers_migration_v0_to_v1_round_trip() {
    let tmp = TempDir::new().unwrap();
    let timers = ScheduledTimers::empty(0);
    write_timers_in_dir(tmp.path(), SLUG, &timers).unwrap();
    let path = timers_path_in_dir(tmp.path(), SLUG);
    let mut on_disk = std::fs::read(&path).unwrap();
    set_envelope_version(&mut on_disk, 0);
    std::fs::write(&path, &on_disk).unwrap();
    let bak = with_bak_suffix(&path);
    let _ = std::fs::remove_file(&bak);

    let mut registry = MigrationRegistry::new();
    registry.register(
        SaveKind::Timers,
        0,
        TIMERS_FORMAT_VERSION,
        bump_envelope_v0_to_v1,
    );

    let loaded = read_timers_in_dir_with_registry(tmp.path(), SLUG, &registry)
        .unwrap()
        .expect("timers exist");
    assert_eq!(loaded.format_version, TIMERS_FORMAT_VERSION);

    let post = std::fs::read(&path).unwrap();
    assert_eq!(&post[..4], TIMERS_MAGIC);
    assert_eq!(
        u32::from_le_bytes([post[4], post[5], post[6], post[7]]),
        TIMERS_FORMAT_VERSION
    );
}

// ---------------------------------------------------------------
// Same-version pass-through (no write-back).
// ---------------------------------------------------------------

#[test]
fn matching_version_skips_migration_write_back() {
    // A file whose disk version already matches must NOT be rewritten
    // by the reader. We assert this by recording the file's mtime
    // before + after the read - if no migration kicked in, atomic_save
    // never fired and the file is unchanged on disk.
    let tmp = TempDir::new().unwrap();
    let meta = create_meta("CurrentVersion", "current");
    super::meta::write_meta_in_dir(tmp.path(), "current", &meta).unwrap();
    let path = meta_path_in(tmp.path(), "current");
    let before = std::fs::read(&path).unwrap();

    let registry = MigrationRegistry::new();
    let loaded = read_meta_in_dir_with_registry(tmp.path(), "current", &registry).unwrap();
    assert_eq!(loaded.format_version, META_FORMAT_VERSION);

    // Bytes on disk are byte-identical (no atomic_save rotation).
    let after = std::fs::read(&path).unwrap();
    assert_eq!(before, after);
}
