//! Tests for `world_save::atomic` - the tmp + rename + `.bak`
//! rolling backup primitive every save format runs through.
//!
//! Coverage split:
//!
//! - `atomic_save_*` - asserts the write side: tmp consumption, no
//!   leftover `.tmp`, `.bak` created only on overwrite, parent dirs
//!   auto-created.
//! - `recovery_*` - asserts the read side: missing both files,
//!   primary clean, primary corrupt + `.bak` good (the recovery
//!   restore + `.broken.<ts>` stash), both corrupt -> Err.
//! - End-to-end `*_recovers_from_bak` - drives the recovery path
//!   through each caller (`meta`, `world_json`, `chunks`, `players`)
//!   so a future refactor of those modules can't accidentally bypass
//!   the recovery wiring.

use std::io;
use std::path::Path;

use tempfile::TempDir;

use super::atomic::{atomic_save, read_with_recovery};

/// Pass-through "parse" closure that just returns the bytes
/// verbatim. Used by the pure atomic.rs tests where the payload
/// shape doesn't matter - only the recovery semantics do.
fn identity(bytes: &[u8]) -> io::Result<Vec<u8>> {
    Ok(bytes.to_vec())
}

/// Strict parser that rejects any payload not starting with the
/// magic byte 0xAB. Lets us simulate "this file is corrupt" by
/// overwriting the live file with arbitrary garbage bytes.
fn strict(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.first() != Some(&0xAB) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic"));
    }
    Ok(bytes.to_vec())
}

fn bak_of(path: &Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    std::path::PathBuf::from(s)
}

fn tmp_of(path: &Path) -> std::path::PathBuf {
    path.with_extension("tmp")
}

// ----- atomic_save -----

#[test]
fn atomic_save_creates_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, b"hello").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"hello");
}

#[test]
fn atomic_save_overwrites_existing() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, b"first").unwrap();
    atomic_save(&path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
}

#[test]
fn atomic_save_creates_bak_on_overwrite() {
    // After a second write to the same path, the `.bak` sibling must
    // hold the previous payload - this is the rolling 1-deep history
    // slot the design doc requires.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, b"first").unwrap();
    atomic_save(&path, b"second").unwrap();
    let bak = bak_of(&path);
    assert!(bak.exists(), "second write must produce a .bak sibling");
    assert_eq!(std::fs::read(&bak).unwrap(), b"first");
}

#[test]
fn atomic_save_does_not_create_bak_on_first_write() {
    // Fresh slot: nothing to back up yet. The `.bak` slot must stay
    // unused so a half-flushed first write never lands here.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, b"first").unwrap();
    assert!(!bak_of(&path).exists());
}

#[test]
fn atomic_save_atomic_no_tmp_leftover() {
    // After a successful save the staging `.tmp` must be gone -
    // catches a regression where the rename pair leaks the tmp.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, b"first").unwrap();
    atomic_save(&path, b"second").unwrap();
    assert!(!tmp_of(&path).exists());
}

#[test]
fn atomic_save_creates_parent_dir() {
    // First save into a fresh slot must not require the caller to
    // pre-create the directory layout - the helper owns that.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("nested").join("deep").join("a.bin");
    atomic_save(&path, b"hi").unwrap();
    assert!(path.exists());
}

#[test]
fn atomic_save_two_overwrites_keep_bak_at_previous() {
    // After three writes, `.bak` must hold the SECOND payload (the
    // most recent pre-write live file), NOT the first. Confirms the
    // rolling-1-deep contract.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, b"first").unwrap();
    atomic_save(&path, b"second").unwrap();
    atomic_save(&path, b"third").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"third");
    assert_eq!(std::fs::read(bak_of(&path)).unwrap(), b"second");
}

// ----- read_with_recovery -----

#[test]
fn recovery_returns_none_when_neither_exists() {
    // The streaming layer (and the join handler, and the worlds
    // menu) treat Ok(None) as "fresh slot" - this is the critical
    // base case.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    let got: Option<Vec<u8>> = read_with_recovery(&path, identity).unwrap();
    assert!(got.is_none());
}

#[test]
fn recovery_reads_path_when_valid() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, &[0xAB, 1, 2, 3]).unwrap();
    let got = read_with_recovery(&path, strict).unwrap().unwrap();
    assert_eq!(got, vec![0xAB, 1, 2, 3]);
}

#[test]
fn recovery_falls_back_to_bak_when_path_corrupt() {
    // Two clean writes -> path = "second", bak = "first". Stomp the
    // live file with garbage. The recovery must return the .bak
    // content + leave a `.broken.<ts>` snapshot of the garbage.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, &[0xAB, 1]).unwrap();
    atomic_save(&path, &[0xAB, 2]).unwrap();
    std::fs::write(&path, b"junk").unwrap();

    let got = read_with_recovery(&path, strict).unwrap().unwrap();
    assert_eq!(got, vec![0xAB, 1], "recovery must surface .bak content");

    // The corrupt original must have been stashed at .broken.<ts>.
    let dir = path.parent().unwrap();
    let broken: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("a.bin.broken.")
        })
        .collect();
    assert_eq!(broken.len(), 1, "exactly one .broken.<ts> snapshot");
    assert_eq!(std::fs::read(broken[0].path()).unwrap(), b"junk");
}

#[test]
fn recovery_restores_bak_to_path() {
    // After a successful recovery, the .bak must have been promoted
    // back to the live path slot - so the next atomic_save can mint
    // a fresh .bak from the now-recovered version.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, &[0xAB, 1]).unwrap();
    atomic_save(&path, &[0xAB, 2]).unwrap();
    std::fs::write(&path, b"junk").unwrap();

    let _ = read_with_recovery(&path, strict).unwrap().unwrap();

    // Live path now contains the recovered .bak content.
    assert_eq!(std::fs::read(&path).unwrap(), vec![0xAB, 1]);
    // The .bak slot has been consumed (renamed back to live path).
    assert!(
        !bak_of(&path).exists(),
        ".bak must be consumed after recovery"
    );
}

#[test]
fn recovery_returns_err_when_both_corrupt() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, &[0xAB, 1]).unwrap();
    atomic_save(&path, &[0xAB, 2]).unwrap();
    // Now both `path` and `.bak` exist. Trash both.
    std::fs::write(&path, b"junk-primary").unwrap();
    std::fs::write(bak_of(&path), b"junk-bak").unwrap();

    let err = read_with_recovery(&path, strict)
        .expect_err("both corrupt -> Err");
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn recovery_creates_broken_snapshot_with_timestamp() {
    // Snapshot name must follow `<stem>.broken.YYYYMMDD-HHMMSS-mmm`
    // and content must equal what was on disk before recovery.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, &[0xAB, 1]).unwrap();
    atomic_save(&path, &[0xAB, 2]).unwrap();
    std::fs::write(&path, b"junk-primary").unwrap();

    let _ = read_with_recovery(&path, strict).unwrap();

    let dir = path.parent().unwrap();
    let snap = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("a.bin.broken.")
        })
        .expect(".broken.<ts> snapshot missing");
    // YYYYMMDD-HHMMSS-mmm = 8 + 1 + 6 + 1 + 3 = 19 chars after
    // `.broken.`. We don't pin the literal value (it depends on the
    // wall clock), only that the suffix shape is right and the
    // file content matches the pre-recovery garbage.
    let name = snap.file_name().to_string_lossy().to_string();
    let suffix = name.strip_prefix("a.bin.broken.").unwrap();
    assert_eq!(suffix.len(), 19, "timestamp suffix wrong length: {name}");
    assert_eq!(std::fs::read(snap.path()).unwrap(), b"junk-primary");
}

#[test]
fn recovery_two_corruptions_in_same_second_dont_collide() {
    // Millisecond precision in the .broken.<ts> stamp means two
    // recoveries in the same second produce distinct filenames.
    // Drive it through two separate save+corrupt+recover cycles on
    // two paths so we can compare the two `.broken` names.
    let tmp = TempDir::new().unwrap();
    let p_a = tmp.path().join("a.bin");
    let p_b = tmp.path().join("b.bin");
    atomic_save(&p_a, &[0xAB, 1]).unwrap();
    atomic_save(&p_a, &[0xAB, 2]).unwrap();
    atomic_save(&p_b, &[0xAB, 3]).unwrap();
    atomic_save(&p_b, &[0xAB, 4]).unwrap();
    std::fs::write(&p_a, b"junk-a").unwrap();
    std::fs::write(&p_b, b"junk-b").unwrap();

    let _ = read_with_recovery(&p_a, strict).unwrap();
    let _ = read_with_recovery(&p_b, strict).unwrap();

    let dir = tmp.path();
    let snaps: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".broken."))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(snaps.len(), 2, "expected two distinct .broken snapshots");
    assert_ne!(snaps[0], snaps[1], "snapshot names must not collide");
}

#[test]
fn recovery_path_missing_but_bak_present_uses_bak() {
    // Adversarial edge: the live file is gone but the `.bak` is
    // still around (e.g. a crash between rotating bak and renaming
    // tmp). Recovery must still surface the slot from the backup.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("a.bin");
    atomic_save(&path, &[0xAB, 7]).unwrap();
    atomic_save(&path, &[0xAB, 8]).unwrap();
    // Manually drop the live file - simulates the rare mid-rename
    // crash window.
    std::fs::remove_file(&path).unwrap();
    assert!(bak_of(&path).exists());

    let got = read_with_recovery(&path, strict).unwrap().unwrap();
    assert_eq!(got, vec![0xAB, 7]);
    // After recovery, the .bak has been promoted to the live slot.
    assert_eq!(std::fs::read(&path).unwrap(), vec![0xAB, 7]);
    assert!(!bak_of(&path).exists());
}

// ----- End-to-end caller migrations -----
// One test per migrated caller. Each one writes twice (creating a
// `.bak`), stomps the live file with garbage, then asserts the
// read returns the previous version. Confirms every caller's
// parse closure feeds back through the `.bak` branch of
// `read_with_recovery`.

#[test]
fn meta_recovers_from_bak() {
    use super::layout::meta_path_in;
    use super::meta::{create_meta, read_meta_in_dir, write_meta_in_dir};

    let tmp = TempDir::new().unwrap();
    let slug = "slot";

    let mut first = create_meta("First", slug);
    first.play_time_seconds = 10;
    write_meta_in_dir(tmp.path(), slug, &first).unwrap();

    let mut second = create_meta("Second", slug);
    second.play_time_seconds = 20;
    write_meta_in_dir(tmp.path(), slug, &second).unwrap();

    let path = meta_path_in(tmp.path(), slug);
    std::fs::write(&path, b"not json at all").unwrap();

    let loaded = read_meta_in_dir(tmp.path(), slug).unwrap();
    assert_eq!(loaded.name, "First");
    assert_eq!(loaded.play_time_seconds, 10);
}

#[test]
fn world_json_recovers_from_bak() {
    use super::layout::world_json_path_in;
    use super::world_json::{create_world, read_world_in_dir, write_world_in_dir};

    let tmp = TempDir::new().unwrap();
    let slug = "slot";

    let first = create_world("classic", 11);
    write_world_in_dir(tmp.path(), slug, &first).unwrap();

    let second = create_world("amplified", 22);
    write_world_in_dir(tmp.path(), slug, &second).unwrap();

    let path = world_json_path_in(tmp.path(), slug);
    std::fs::write(&path, b"definitely not json").unwrap();

    let loaded = read_world_in_dir(tmp.path(), slug).unwrap();
    assert_eq!(loaded.worldgen, "classic");
    assert_eq!(loaded.seed, 11);
}

#[test]
fn chunk_recovers_from_bak() {
    use super::chunks::{
        ChunkPayload, ChunkVoxels, CHUNK_PAYLOAD_VERSION, chunk_path_in_dir,
        empty_payload, read_chunk_in_dir, write_chunk_in_dir,
    };
    use infengine_voxel::{BlockId, Chunk};

    let tmp = TempDir::new().unwrap();
    let slug = "slot";
    let dim = "overworld";
    let coord = [0, 0, 0];

    // First save: one stone block at (1,1,1) - distinguishable from
    // the empty second save.
    let mut chunk = Chunk::empty();
    chunk.set(1, 1, 1, BlockId(1));
    let first = ChunkPayload {
        format_version: CHUNK_PAYLOAD_VERSION,
        coord,
        voxels: ChunkVoxels::from_chunk(&chunk),
        liquid_levels: Vec::new(),
        torch_orient: Vec::new(),
        snow_layers: Vec::new(),
    };
    write_chunk_in_dir(tmp.path(), slug, dim, &first).unwrap();

    // Second save: empty chunk - this becomes the live file, the
    // first save rotates into `.bak`.
    let second = empty_payload(coord);
    write_chunk_in_dir(tmp.path(), slug, dim, &second).unwrap();

    let path = chunk_path_in_dir(tmp.path(), slug, dim, coord);
    std::fs::write(&path, b"garbage envelope").unwrap();

    let loaded = read_chunk_in_dir(tmp.path(), slug, dim, coord)
        .unwrap()
        .expect("recovery must return Some(first)");
    let decoded = loaded.voxels.into_chunk().unwrap();
    assert_eq!(decoded.get(1, 1, 1), BlockId(1), "recovered first save");
}

#[test]
fn player_recovers_from_bak() {
    use super::players::{
        fresh_player, player_path_in_dir, read_player_in_dir, write_player_in_dir,
    };

    let tmp = TempDir::new().unwrap();
    let slug = "slot";
    let name = "Alice";

    let mut first = fresh_player([0, 64, 0]);
    first.play_time_seconds = 111;
    write_player_in_dir(tmp.path(), slug, name, &first).unwrap();

    let mut second = fresh_player([0, 64, 0]);
    second.play_time_seconds = 222;
    write_player_in_dir(tmp.path(), slug, name, &second).unwrap();

    let path = player_path_in_dir(tmp.path(), slug, name);
    std::fs::write(&path, b"junk").unwrap();

    let loaded = read_player_in_dir(tmp.path(), slug, name)
        .unwrap()
        .expect("recovery must return Some(first)");
    assert_eq!(loaded.play_time_seconds, 111);
}
