//! Integration tests for the per-dimension container persistence
//! layer (`world_save::containers`).
//!
//! Each test spins up a fresh `tempfile::TempDir` and drives the
//! `_in_dir` variants so OS data-dir state never bleeds between
//! runs. Mirrors `entities_tests.rs` / `players_tests.rs`.
//!
//! Coverage:
//! - Round trips: empty, single chest, multi chest, populated slots.
//! - Missing file -> `Ok(None)`.
//! - Refusal paths: unknown envelope version, wrong magic.
//! - Compression toggle on a populated payload.
//! - `.bak` recovery: corrupt primary surfaces backup content.
//! - Slot count is preserved (so future double-chest 54-slot saves
//!   round-trip alongside 27-slot small chests).

use tempfile::TempDir;

use super::containers::{
    CONTAINERS_FORMAT_VERSION, CONTAINERS_MAGIC, ChestSave, DimensionContainers,
    containers_path_in_dir, read_containers_in_dir, write_containers_in_dir,
};

const SLUG: &str = "slot";
const DIM: &str = "overworld";

// Item-id literals used in the chest fixtures (independent of any
// runtime `BlockId` mapping - the on-disk schema is just `u16`).
const ITEM_STONE: u16 = 1;
const ITEM_DIRT: u16 = 2;
const ITEM_PLANKS: u16 = 7;
const ITEM_SWORD_IRON: u16 = 25;
const ITEM_CHEST: u16 = 41;

fn empty_chest_at(pos: [i32; 3]) -> ChestSave {
    ChestSave {
        pos,
        slots: vec![None; 27],
    }
}

fn populated_chest_at(pos: [i32; 3]) -> ChestSave {
    let mut slots: Vec<Option<(u16, u8)>> = vec![None; 27];
    slots[0] = Some((ITEM_STONE, 64));
    slots[1] = Some((ITEM_DIRT, 32));
    slots[5] = Some((ITEM_SWORD_IRON, 1));
    slots[13] = Some((ITEM_PLANKS, 17));
    slots[26] = Some((ITEM_CHEST, 4));
    ChestSave { pos, slots }
}

#[test]
fn containers_roundtrip_empty() {
    // Empty bundle round trips through the envelope byte-for-byte.
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers::empty();
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM)
        .unwrap()
        .expect("containers file must exist after write");
    assert_eq!(loaded, original);
    assert_eq!(loaded.format_version, CONTAINERS_FORMAT_VERSION);
}

#[test]
fn containers_roundtrip_single_empty_chest() {
    // Even an empty chest gets a save row - the world block alone
    // doesn't know the slot count to mint on load.
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![empty_chest_at([10, 64, -3])],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.chests.len(), 1);
    assert_eq!(loaded.chests[0].pos, [10, 64, -3]);
    assert_eq!(loaded.chests[0].slots.len(), 27);
    assert!(loaded.chests[0].slots.iter().all(|s| s.is_none()));
}

#[test]
fn containers_roundtrip_populated_chest() {
    // Five slots filled across the 27-slot range. Every slot has
    // to survive the bincode + zstd hop including stack counts.
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![populated_chest_at([0, 65, 0])],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.chests.len(), 1);
    let chest = &loaded.chests[0];
    assert_eq!(chest.slots[0], Some((ITEM_STONE, 64)));
    assert_eq!(chest.slots[1], Some((ITEM_DIRT, 32)));
    assert_eq!(chest.slots[5], Some((ITEM_SWORD_IRON, 1)));
    assert_eq!(chest.slots[13], Some((ITEM_PLANKS, 17)));
    assert_eq!(chest.slots[26], Some((ITEM_CHEST, 4)));
}

#[test]
fn containers_roundtrip_multi_chest() {
    // Three chests at distinct coords. The chest list preserves
    // order so the on-load HashMap rebuild has a stable iteration.
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![
            populated_chest_at([0, 64, 0]),
            empty_chest_at([5, 64, 5]),
            populated_chest_at([-10, 64, 10]),
        ],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.chests.len(), 3);
    assert_eq!(loaded.chests[0].pos, [0, 64, 0]);
    assert_eq!(loaded.chests[1].pos, [5, 64, 5]);
    assert_eq!(loaded.chests[2].pos, [-10, 64, 10]);
}

#[test]
fn containers_missing_returns_none() {
    // Server boot treats "no containers file yet" as a fresh
    // dimension. Must not surface that as an io error.
    let tmp = TempDir::new().unwrap();
    let result = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap();
    assert!(result.is_none());
}

#[test]
fn containers_unknown_version_refuses_read() {
    let tmp = TempDir::new().unwrap();
    write_containers_in_dir(tmp.path(), SLUG, DIM, &DimensionContainers::empty()).unwrap();

    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let mut bytes = std::fs::read(&path).unwrap();
    // Patch the LE u32 at offset 4 to a far-future version (888).
    let bad = 888u32.to_le_bytes();
    bytes[4..8].copy_from_slice(&bad);
    std::fs::write(&path, &bytes).unwrap();
    // Remove the .bak so recovery can't paper over the bad primary.
    let bak = path.with_file_name(format!(
        "{}.bak",
        path.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&bak);

    let err = read_containers_in_dir(tmp.path(), SLUG, DIM)
        .expect_err("expected envelope version mismatch");
    assert!(
        err.to_string().contains("888"),
        "error must mention the unsupported version, got: {err}"
    );
}

#[test]
fn containers_wrong_magic_refuses_read() {
    let tmp = TempDir::new().unwrap();
    write_containers_in_dir(tmp.path(), SLUG, DIM, &DimensionContainers::empty()).unwrap();

    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..4].copy_from_slice(b"XXXX");
    std::fs::write(&path, &bytes).unwrap();
    // Drop .bak so we don't recover into the test's blind spot.
    let bak = path.with_file_name(format!(
        "{}.bak",
        path.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&bak);

    let err = read_containers_in_dir(tmp.path(), SLUG, DIM)
        .expect_err("expected magic mismatch");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().to_lowercase().contains("magic"));
}

#[test]
fn containers_compression_smaller_payload() {
    // A populated bundle with many identical chest rows compresses
    // well via zstd. The envelope must pick kind = 1 and the body
    // must end up shorter than the bincode payload.
    let tmp = TempDir::new().unwrap();
    let mut original = DimensionContainers::empty();
    for i in 0..64i32 {
        original.chests.push(populated_chest_at([i, 64, i]));
    }
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], CONTAINERS_MAGIC);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, CONTAINERS_FORMAT_VERSION);
    assert_eq!(
        bytes[8], 1,
        "repetitive container bundle should compress with zstd (kind=1)"
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
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
}

#[test]
fn containers_recover_from_bak() {
    // Two clean writes -> live = "second", bak = "first". Stomp the
    // live file with garbage; recovery must surface the .bak content
    // and the next read returns the older (but valid) bundle.
    let tmp = TempDir::new().unwrap();
    let first = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![empty_chest_at([1, 64, 1])],
    };
    let second = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![populated_chest_at([1, 64, 1]), empty_chest_at([2, 64, 2])],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &first).unwrap();
    write_containers_in_dir(tmp.path(), SLUG, DIM, &second).unwrap();

    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    // Sanity: live file currently parses as `second`.
    let live = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(live, second);

    // Stomp the live file with garbage so the primary parse fails.
    std::fs::write(&path, b"junk-not-a-valid-containers-file").unwrap();

    // Recovery surfaces the older `.bak` bundle.
    let recovered = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(recovered, first, "recovery must surface .bak content");
}

#[test]
fn containers_slot_count_is_preserved() {
    // A 54-slot future double-chest variant must round-trip
    // without any schema churn - the slot vec carries its own
    // length.
    let tmp = TempDir::new().unwrap();
    let double_chest = ChestSave {
        pos: [42, 64, 42],
        slots: {
            let mut slots: Vec<Option<(u16, u8)>> = vec![None; 54];
            slots[0] = Some((ITEM_STONE, 1));
            slots[53] = Some((ITEM_PLANKS, 64));
            slots
        },
    };
    let original = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![double_chest],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.chests[0].slots.len(), 54);
    assert_eq!(loaded.chests[0].slots[53], Some((ITEM_PLANKS, 64)));
}

#[test]
fn containers_path_lives_under_containers_subdir() {
    // The on-disk layout must be `worlds/<slug>/containers/<dim>.bin`.
    // The runtime's load + save paths derive from this so a regression
    // here would silently drop chest persistence.
    let tmp = TempDir::new().unwrap();
    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let s = path.to_string_lossy();
    assert!(s.contains("containers"), "path missing containers/: {s}");
    assert!(s.ends_with("overworld.bin"), "path doesn't end with dim.bin: {s}");
}

#[test]
fn containers_two_writes_keep_bak() {
    // The atomic save layer should leave a `.bak` after the second
    // write - matches `entities_recover_from_bak` precondition.
    let tmp = TempDir::new().unwrap();
    let first = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![empty_chest_at([0, 64, 0])],
    };
    let second = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![populated_chest_at([0, 64, 0])],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &first).unwrap();
    write_containers_in_dir(tmp.path(), SLUG, DIM, &second).unwrap();

    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let bak = path.with_file_name(format!(
        "{}.bak",
        path.file_name().unwrap().to_string_lossy()
    ));
    assert!(bak.exists(), ".bak must exist after a second write");
}
