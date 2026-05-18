//! Integration tests for the per-player persistence layer
//! (`world_save::players`).
//!
//! Pulled out of `tests.rs` to keep that file under the 500-LOC soft
//! cap; mirrors the `chunks_tests.rs` split. Each test spins up a
//! fresh `tempfile::TempDir` and drives the `_in_dir` variants so OS
//! data-dir state never bleeds between runs.
//!
//! Coverage:
//! - Round trips: fresh save + populated inventory + craft grid.
//! - Missing file -> Ok(None).
//! - Refusal: unknown envelope version, wrong magic.
//! - Sanitize rules: lowercasing + path-traversal containment.
//! - Compression toggle: zstd when populated, raw on the tiny fresh
//!   save.
//! - `list_players`: missing dir, alphabetical ordering.

use tempfile::TempDir;

use super::players::{
    AIR_MAX_DEFAULT, CRAFT_GRID_SLOTS_DEFAULT, INVENTORY_SLOTS_DEFAULT, PLAYER_FORMAT_VERSION,
    PLAYER_MAGIC, fresh_player, list_players_in_dir, player_path_in_dir, read_player_in_dir,
    sanitize_name, write_player_in_dir,
};

const SLUG: &str = "slot";

#[test]
fn player_roundtrip_fresh() {
    // A freshly minted save round trips byte-for-byte through the
    // envelope: encode + zstd-attempt + atomic write + read back.
    let tmp = TempDir::new().unwrap();
    let original = fresh_player([0, 64, 0]);
    write_player_in_dir(tmp.path(), SLUG, "Alice", &original).unwrap();

    let loaded = read_player_in_dir(tmp.path(), SLUG, "Alice")
        .unwrap()
        .expect("player file must exist after write");
    assert_eq!(loaded, original);
    // Confirms `fresh_player` produced the expected slot count + air
    // baseline so the load side does not have to re-pad.
    assert_eq!(loaded.inventory.len(), INVENTORY_SLOTS_DEFAULT);
    assert_eq!(loaded.craft_grid.len(), CRAFT_GRID_SLOTS_DEFAULT);
    assert_eq!(loaded.air, AIR_MAX_DEFAULT);
}

#[test]
fn player_roundtrip_populated_inventory() {
    // Realistic-ish save: 5 inventory stacks + 2 craft cells +
    // non-default pose, home, perms. Each field has to survive the
    // bincode + zstd hop.
    let tmp = TempDir::new().unwrap();
    let mut save = fresh_player([12, 80, -8]);
    save.pos = [12.25, 80.5, -7.9];
    save.yaw = 1.57;
    save.pitch = -0.5;
    save.selected_slot = 3;
    save.air = 11.0;
    save.health = 17.5;
    save.home_pos = Some([100.5, 65.0, 100.5]);
    save.spawn_pos = Some([0.5, 70.0, 0.5]);
    save.permission_level = 2;
    save.play_time_seconds = 3825;
    save.last_dimension = "nether".to_string();
    // Stone in hotbar slot 0, dirt in 1, log in 2, plus two storage
    // stacks in row 1.
    save.inventory[0] = Some((1, 64));
    save.inventory[1] = Some((3, 32));
    save.inventory[2] = Some((17, 64));
    save.inventory[9] = Some((4, 12));
    save.inventory[10] = Some((5, 7));
    // Craft grid: two cells filled (corner + centre).
    save.craft_grid[0] = Some((1, 1));
    save.craft_grid[4] = Some((3, 1));

    write_player_in_dir(tmp.path(), SLUG, "Notch", &save).unwrap();
    let loaded = read_player_in_dir(tmp.path(), SLUG, "Notch")
        .unwrap()
        .unwrap();
    assert_eq!(loaded, save);
    // Spot-check the inventory slots survived in order.
    assert_eq!(loaded.inventory[0], Some((1, 64)));
    assert_eq!(loaded.inventory[2], Some((17, 64)));
    assert_eq!(loaded.inventory[9], Some((4, 12)));
    assert_eq!(loaded.craft_grid[4], Some((3, 1)));
}

#[test]
fn player_missing_returns_none() {
    // Join handler treats "no file" as "first time on the server".
    // Must not surface that as an io error.
    let tmp = TempDir::new().unwrap();
    let result = read_player_in_dir(tmp.path(), SLUG, "Stranger").unwrap();
    assert!(result.is_none());
}

#[test]
fn player_unknown_format_version_refuses_read() {
    let tmp = TempDir::new().unwrap();
    let save = fresh_player([0, 64, 0]);
    write_player_in_dir(tmp.path(), SLUG, "Alice", &save).unwrap();

    let path = player_path_in_dir(tmp.path(), SLUG, "Alice");
    let mut bytes = std::fs::read(&path).unwrap();
    // Patch the LE u32 at offset 4 to a far-future version (777).
    let bad = 777u32.to_le_bytes();
    bytes[4..8].copy_from_slice(&bad);
    std::fs::write(&path, &bytes).unwrap();

    let err = read_player_in_dir(tmp.path(), SLUG, "Alice")
        .expect_err("expected envelope version mismatch");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("777"),
        "error must mention the unsupported version, got: {err}"
    );
}

#[test]
fn player_wrong_magic_refuses_read() {
    let tmp = TempDir::new().unwrap();
    let save = fresh_player([0, 64, 0]);
    write_player_in_dir(tmp.path(), SLUG, "Alice", &save).unwrap();

    let path = player_path_in_dir(tmp.path(), SLUG, "Alice");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..4].copy_from_slice(b"XXXX");
    std::fs::write(&path, &bytes).unwrap();

    let err = read_player_in_dir(tmp.path(), SLUG, "Alice")
        .expect_err("expected magic mismatch");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().to_lowercase().contains("magic"));
}

#[test]
fn player_sanitize_name_lowercases() {
    // The on-disk file uses the sanitized name. Writing as "Alice"
    // must land at `players/alice.bin`, and reading either "Alice"
    // or "alice" must find the same file.
    let tmp = TempDir::new().unwrap();
    let save = fresh_player([0, 64, 0]);
    write_player_in_dir(tmp.path(), SLUG, "Alice", &save).unwrap();

    let path = player_path_in_dir(tmp.path(), SLUG, "Alice");
    assert!(path.ends_with(std::path::Path::new("players/alice.bin")));
    assert!(path.exists());

    let by_lower = read_player_in_dir(tmp.path(), SLUG, "alice")
        .unwrap()
        .expect("lowercase name must resolve to the same file");
    let by_mixed = read_player_in_dir(tmp.path(), SLUG, "Alice")
        .unwrap()
        .expect("mixed case must resolve to the same file");
    assert_eq!(by_lower, by_mixed);
}

#[test]
fn player_sanitize_name_strips_traversal() {
    // `..` + `/` + `\\` must all collapse to `_` so a hostile name
    // can never escape the `players/` directory. The resulting file
    // must sit strictly under `<world>/players/`.
    let tmp = TempDir::new().unwrap();
    let save = fresh_player([0, 64, 0]);
    let hostile = "../../etc/passwd";
    write_player_in_dir(tmp.path(), SLUG, hostile, &save).unwrap();

    let path = player_path_in_dir(tmp.path(), SLUG, hostile);
    let players_dir = path.parent().unwrap();
    assert!(
        players_dir.ends_with("players"),
        "sanitized path must stay under players/, got {}",
        path.display()
    );
    let stem = path.file_stem().unwrap().to_str().unwrap();
    assert!(
        !stem.contains('/') && !stem.contains('\\') && !stem.contains(".."),
        "stem must not retain traversal chars, got {stem}"
    );
    // Same string applied via sanitize_name yields the same stem.
    assert_eq!(stem, sanitize_name(hostile));
    // And the file is actually readable back.
    let loaded = read_player_in_dir(tmp.path(), SLUG, hostile)
        .unwrap()
        .unwrap();
    assert_eq!(loaded, save);
}

#[test]
fn player_compression_smaller_payload() {
    // A populated inventory with repetitive `(block_id, count)` runs
    // compresses well via zstd. The envelope must pick kind = 1 and
    // the body must end up shorter than the bincode payload.
    let tmp = TempDir::new().unwrap();
    let mut save = fresh_player([0, 64, 0]);
    // Fill every slot with the same stack so the payload is highly
    // repetitive - guaranteed zstd win.
    for slot in save.inventory.iter_mut() {
        *slot = Some((1, 64));
    }
    for slot in save.craft_grid.iter_mut() {
        *slot = Some((1, 1));
    }
    write_player_in_dir(tmp.path(), SLUG, "Heavy", &save).unwrap();

    let path = player_path_in_dir(tmp.path(), SLUG, "Heavy");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], PLAYER_MAGIC);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, PLAYER_FORMAT_VERSION);
    assert_eq!(
        bytes[8], 1,
        "repetitive inventory should compress with zstd (kind=1)"
    );
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]);
    let body_len = (bytes.len() - 17) as u64;
    assert!(
        body_len < uncompressed_len,
        "zstd body must be shorter than bincode payload: body={body_len}, raw={uncompressed_len}"
    );
    // And it still round trips.
    let loaded = read_player_in_dir(tmp.path(), SLUG, "Heavy")
        .unwrap()
        .unwrap();
    assert_eq!(loaded, save);
}

#[test]
fn list_players_empty() {
    // Missing players dir = empty list, not an error. Mirrors the
    // "missing world = no worlds" treatment in `list_worlds_in`.
    let tmp = TempDir::new().unwrap();
    let names = list_players_in_dir(tmp.path(), SLUG).unwrap();
    assert!(names.is_empty(), "fresh dir must list zero players");
}

#[test]
fn list_players_returns_sorted_names() {
    // Three saves written in non-alphabetical order. `list_players`
    // must return them sorted so the UI / admin tooling can show a
    // stable order without an extra sort step.
    let tmp = TempDir::new().unwrap();
    let save = fresh_player([0, 64, 0]);
    write_player_in_dir(tmp.path(), SLUG, "Charlie", &save).unwrap();
    write_player_in_dir(tmp.path(), SLUG, "Alice", &save).unwrap();
    write_player_in_dir(tmp.path(), SLUG, "Bob", &save).unwrap();

    let names = list_players_in_dir(tmp.path(), SLUG).unwrap();
    assert_eq!(names, vec!["alice", "bob", "charlie"]);
}

#[test]
fn list_players_skips_non_bin_files() {
    // A stray `.tmp` from a crashed write or a user-dropped note
    // must not show up in the list. Only `.bin` files count.
    let tmp = TempDir::new().unwrap();
    let save = fresh_player([0, 64, 0]);
    write_player_in_dir(tmp.path(), SLUG, "Alice", &save).unwrap();
    let dir = player_path_in_dir(tmp.path(), SLUG, "Alice")
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::write(dir.join("notes.txt"), b"hi").unwrap();
    std::fs::write(dir.join("crashed.tmp"), b"garbage").unwrap();

    let names = list_players_in_dir(tmp.path(), SLUG).unwrap();
    assert_eq!(names, vec!["alice"]);
}

#[test]
fn write_player_is_atomic_no_tmp_left_behind() {
    // After a successful write, the `.tmp` sibling must be gone -
    // the atomic rename consumed it. Mirrors the meta.json invariant.
    let tmp = TempDir::new().unwrap();
    let save = fresh_player([0, 64, 0]);
    write_player_in_dir(tmp.path(), SLUG, "Alice", &save).unwrap();

    let path = player_path_in_dir(tmp.path(), SLUG, "Alice");
    let dir = path.parent().unwrap();
    assert!(path.exists());
    let leftover = dir
        .read_dir()
        .unwrap()
        .flatten()
        .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("tmp"));
    assert!(!leftover, "atomic_write must not leave .tmp behind");
}

#[test]
fn player_format_version_matches_design_doc() {
    // Lock the published schema number - bumping is a deliberate
    // breaking change that needs a migration.
    assert_eq!(PLAYER_FORMAT_VERSION, 1);
}
