//! Integration tests for the multi-world save layer foundations.
//!
//! Covers `meta.rs` + `world_json.rs` + `layout::list_worlds_in`
//! together. Each test spins up a fresh `tempfile::TempDir` and
//! drives the `_in_dir` variants so OS data-dir state never bleeds
//! between runs. The legacy single-file `voxel_server_state.bin`
//! path is exercised elsewhere (see `super::load` / `super::save`).

use std::path::Path;

use chrono::{Duration, Utc};
use tempfile::TempDir;

use super::layout::{list_worlds_in, world_dir_in};
use super::meta::{
    META_FORMAT_VERSION, WorldMeta, create_meta, read_meta_in_dir, write_meta_in_dir,
};
use super::world_json::{
    WORLD_FORMAT_VERSION, create_world, read_world_in_dir, write_world_in_dir,
};

/// Write a fake slot at `base/<slug>/` with the given timestamp.
/// Returns the `WorldMeta` so callers can assert on it later.
fn make_slot(
    base: &Path,
    name: &str,
    slug: &str,
    last_played: chrono::DateTime<chrono::Utc>,
) -> WorldMeta {
    let mut meta = create_meta(name, slug);
    meta.last_played = last_played;
    write_meta_in_dir(base, slug, &meta).expect("write_meta");
    let world = create_world("classic", 12345);
    write_world_in_dir(base, slug, &world).expect("write_world");
    meta
}

#[test]
fn meta_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let original = create_meta("Round Trip", "round-trip");
    write_meta_in_dir(tmp.path(), "round-trip", &original).unwrap();

    let loaded = read_meta_in_dir(tmp.path(), "round-trip").unwrap();
    assert_eq!(loaded, original);
}

#[test]
fn world_json_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let original = create_world("amplified", 17_539_214);
    write_world_in_dir(tmp.path(), "amp", &original).unwrap();

    let loaded = read_world_in_dir(tmp.path(), "amp").unwrap();
    assert_eq!(loaded, original);
}

#[test]
fn list_worlds_empty() {
    let tmp = TempDir::new().unwrap();
    let summaries = list_worlds_in(tmp.path());
    assert!(
        summaries.is_empty(),
        "fresh dir must list zero worlds, got {summaries:?}"
    );
}

#[test]
fn list_worlds_finds_meta() {
    let tmp = TempDir::new().unwrap();
    let meta = make_slot(tmp.path(), "Solo", "solo", Utc::now());

    let summaries = list_worlds_in(tmp.path());
    assert_eq!(summaries.len(), 1);
    let s = &summaries[0];
    assert_eq!(s.slug, "solo");
    assert_eq!(s.name, "Solo");
    assert_eq!(s.worldgen, "classic");
    assert_eq!(s.seed, 12345);
    assert_eq!(s.last_played, meta.last_played);
    assert_eq!(s.play_time_seconds, meta.play_time_seconds);
}

#[test]
fn list_worlds_sorted_by_last_played() {
    let tmp = TempDir::new().unwrap();
    let now = Utc::now();
    make_slot(tmp.path(), "Oldest", "oldest", now - Duration::days(7));
    make_slot(tmp.path(), "Middle", "middle", now - Duration::days(2));
    make_slot(tmp.path(), "Newest", "newest", now);

    let summaries = list_worlds_in(tmp.path());
    assert_eq!(summaries.len(), 3);
    // Descending by last_played: newest first.
    assert_eq!(summaries[0].slug, "newest");
    assert_eq!(summaries[1].slug, "middle");
    assert_eq!(summaries[2].slug, "oldest");
}

#[test]
fn list_worlds_skips_unparseable_slot() {
    let tmp = TempDir::new().unwrap();
    // Slot A: valid.
    make_slot(tmp.path(), "Good", "good", Utc::now());
    // Slot B: directory exists but meta.json is garbage. Must be
    // skipped, not crash list_worlds, and not hide slot A.
    let bad_dir = world_dir_in(tmp.path(), "bad");
    std::fs::create_dir_all(&bad_dir).unwrap();
    std::fs::write(bad_dir.join("meta.json"), b"not json").unwrap();

    let summaries = list_worlds_in(tmp.path());
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].slug, "good");
}

#[test]
fn list_worlds_surfaces_slot_with_only_meta() {
    // Mid-creation slot: meta.json written but world.json not yet.
    // Must still show up in the list, with worldgen / seed
    // defaulted to ("", 0).
    let tmp = TempDir::new().unwrap();
    let meta = create_meta("Half Built", "half-built");
    write_meta_in_dir(tmp.path(), "half-built", &meta).unwrap();

    let summaries = list_worlds_in(tmp.path());
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].slug, "half-built");
    assert_eq!(summaries[0].worldgen, "");
    assert_eq!(summaries[0].seed, 0);
}

#[test]
fn unknown_meta_version_refuses_read() {
    let tmp = TempDir::new().unwrap();
    // Hand-craft a meta.json with format_version = 999.
    let mut meta = create_meta("Future", "future");
    meta.format_version = 999;
    let dir = world_dir_in(tmp.path(), "future");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("meta.json"),
        serde_json::to_vec_pretty(&meta).unwrap(),
    )
    .unwrap();

    let err = read_meta_in_dir(tmp.path(), "future")
        .expect_err("expected version mismatch error");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    let msg = err.to_string();
    assert!(
        msg.contains("999"),
        "error must mention the unsupported version, got: {msg}"
    );
}

#[test]
fn unknown_world_version_refuses_read() {
    let tmp = TempDir::new().unwrap();
    let mut world = create_world("classic", 0);
    world.format_version = 999;
    let dir = world_dir_in(tmp.path(), "future");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("world.json"),
        serde_json::to_vec_pretty(&world).unwrap(),
    )
    .unwrap();

    let err = read_world_in_dir(tmp.path(), "future")
        .expect_err("expected version mismatch error");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("999"));
}

#[test]
fn format_versions_match_design_doc() {
    // Lock the published schema numbers - bumping either of these
    // is a deliberate breaking change that needs a migration.
    // - meta.json:  v1 (header only; bumped on rename or new key).
    // - world.json: v3 since Wave 8.2 (added default_game_mode
    //   tag; v1/v2 saves migrate via the registered arrows).
    assert_eq!(META_FORMAT_VERSION, 1);
    assert_eq!(WORLD_FORMAT_VERSION, 3);
}

#[test]
fn write_meta_is_atomic_no_tmp_left_behind() {
    // After a successful write, the `.tmp` sibling must be gone -
    // the rename consumed it. Catches a regression where atomic
    // write leaks tmp files on partial failure.
    let tmp = TempDir::new().unwrap();
    let meta = create_meta("Atomic", "atomic");
    write_meta_in_dir(tmp.path(), "atomic", &meta).unwrap();

    let dir = world_dir_in(tmp.path(), "atomic");
    assert!(dir.join("meta.json").exists());
    assert!(
        !dir.join("meta.json.tmp").exists(),
        "atomic_write must not leave .tmp behind"
    );
}

#[test]
fn write_meta_overwrites_existing() {
    // Second write to the same slug must succeed (the destination
    // already exists). Catches the Windows-rename-refuses-overwrite
    // edge that `atomic_write` works around.
    let tmp = TempDir::new().unwrap();
    let first = create_meta("First", "slug");
    write_meta_in_dir(tmp.path(), "slug", &first).unwrap();

    let mut second = first.clone();
    second.name = "Second".to_string();
    second.play_time_seconds = 99;
    write_meta_in_dir(tmp.path(), "slug", &second).unwrap();

    let loaded = read_meta_in_dir(tmp.path(), "slug").unwrap();
    assert_eq!(loaded.name, "Second");
    assert_eq!(loaded.play_time_seconds, 99);
}

// Phase B.2 per-chunk persistence (`chunks.rs`) is exercised by the
// sibling `chunks_tests.rs` module - kept separate so this file stays
// under the 500-LOC soft cap.
