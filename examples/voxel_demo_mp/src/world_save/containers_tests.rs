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
    CONTAINERS_FORMAT_VERSION, CONTAINERS_MAGIC, ChestSave, DimensionContainers, FurnaceSave,
    containers_path_in_dir, read_containers_in_dir, write_containers_in_dir,
};
use super::migrate::{MigrationRegistry, SaveKind};

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
        furnaces: Vec::new(),
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
        furnaces: Vec::new(),
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
        furnaces: Vec::new(),
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
        furnaces: Vec::new(),
    };
    let second = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![populated_chest_at([1, 64, 1]), empty_chest_at([2, 64, 2])],
        furnaces: Vec::new(),
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
        furnaces: Vec::new(),
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
fn containers_chest_save_to_container_roundtrip() {
    // Cross-module conversion: a ChestSave with mixed empty + full
    // slots round-trips through the generic Container ops. This is
    // the same conversion `apply_dimension_containers` does at
    // load time.
    use crate::container::Container;
    let mut slots: Vec<Option<(u16, u8)>> = vec![None; 27];
    slots[0] = Some((ITEM_STONE, 64));
    slots[10] = Some((ITEM_DIRT, 8));
    slots[26] = Some((ITEM_PLANKS, 1));
    let chest = ChestSave {
        pos: [3, 64, 7],
        slots: slots.clone(),
    };
    // Mint a Container from the ChestSave (mirrors what
    // apply_dimension_containers does).
    let mut c = Container::with_size(chest.slots.len());
    for (i, slot) in chest.slots.iter().enumerate() {
        c.set_slot(i, *slot);
    }
    assert_eq!(c.len(), 27);
    assert_eq!(c.slot(0), Some((ITEM_STONE, 64)));
    assert_eq!(c.slot(10), Some((ITEM_DIRT, 8)));
    assert_eq!(c.slot(26), Some((ITEM_PLANKS, 1)));
    // And back to a ChestSave (mirrors build_dimension_containers).
    let round_trip = ChestSave {
        pos: chest.pos,
        slots: c.slots().to_vec(),
    };
    assert_eq!(round_trip, chest);
}

#[test]
fn containers_full_save_load_through_typed_apis() {
    // End-to-end: build a DimensionContainers from a "virtual"
    // chest map (mimicking voxel.chests), write -> read -> rebuild
    // the chest map, assert positions + slot contents survive.
    let tmp = TempDir::new().unwrap();

    // Phase 1: simulate live state - three chests with mixed
    // contents at distinct positions.
    use std::collections::HashMap;
    let mut live: HashMap<[i32; 3], Vec<Option<(u16, u8)>>> = HashMap::new();
    live.insert(
        [10, 64, 10],
        {
            let mut s = vec![None; 27];
            s[0] = Some((ITEM_STONE, 64));
            s[1] = Some((ITEM_DIRT, 32));
            s
        },
    );
    live.insert([0, 64, 0], vec![None; 27]); // empty chest
    live.insert(
        [-5, 65, 5],
        {
            let mut s = vec![None; 27];
            s[26] = Some((ITEM_SWORD_IRON, 1));
            s
        },
    );

    // Phase 2: build DimensionContainers from the live state.
    let mut chests: Vec<ChestSave> = live
        .iter()
        .map(|(p, s)| ChestSave { pos: *p, slots: s.clone() })
        .collect();
    chests.sort_by_key(|c| c.pos);
    let bundle = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests,
        furnaces: Vec::new(),
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &bundle).unwrap();

    // Phase 3: read back + rebuild the chest map (mirrors
    // apply_dimension_containers).
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    let mut rebuilt: HashMap<[i32; 3], Vec<Option<(u16, u8)>>> = HashMap::new();
    for chest in &loaded.chests {
        rebuilt.insert(chest.pos, chest.slots.clone());
    }

    // Phase 4: verify each live entry survived.
    assert_eq!(rebuilt.len(), live.len());
    for (pos, slots) in &live {
        let got = rebuilt.get(pos).expect("missing chest pos after reload");
        assert_eq!(got, slots, "slots mismatch at {pos:?}");
    }
}

// ---------------------------------------------------------------
// v2 furnace round trip + v1 -> v2 migration.
// ---------------------------------------------------------------

fn populated_furnace_at(pos: [i32; 3]) -> FurnaceSave {
    let mut slots: Vec<Option<(u16, u8)>> = vec![None; 3];
    slots[0] = Some((14, 2)); // 2 iron ore at input
    slots[1] = Some((13, 1)); // 1 coal at fuel
    slots[2] = Some((43, 5)); // 5 iron ingots already cooked
    FurnaceSave {
        pos,
        slots,
        fuel_left_ms: 12_345,
        fuel_full_ms: 80_000,
        cook_progress_ms: 4_321,
    }
}

#[test]
fn containers_roundtrip_furnaces_v2() {
    // The v2 schema must round-trip every FurnaceSave field
    // including the runtime triple.
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: Vec::new(),
        furnaces: vec![
            populated_furnace_at([10, 64, 10]),
            populated_furnace_at([-5, 65, 5]),
        ],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.furnaces.len(), 2);
    assert_eq!(loaded.furnaces[0].fuel_left_ms, 12_345);
    assert_eq!(loaded.furnaces[0].fuel_full_ms, 80_000);
    assert_eq!(loaded.furnaces[0].cook_progress_ms, 4_321);
    assert_eq!(loaded.furnaces[0].slots[0], Some((14, 2)));
    assert_eq!(loaded.furnaces[0].slots[2], Some((43, 5)));
}

#[test]
fn containers_roundtrip_chests_and_furnaces_together() {
    // Mixed bundle: chests + furnaces in the same file.
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![populated_chest_at([0, 64, 0])],
        furnaces: vec![populated_furnace_at([1, 64, 1])],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
}

/// Construct a v1-shape on-disk envelope (chests only) so the
/// v1 -> v2 migration tests can decode it through the production
/// migration arrow registered in `default_registry`.
fn write_v1_containers_file(
    base: &std::path::Path,
    slug: &str,
    dim: &str,
    chests: Vec<ChestSave>,
) {
    use serde::{Deserialize, Serialize};
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct DimensionContainersV1 {
        pub format_version: u32,
        pub chests: Vec<ChestSave>,
    }
    let v1 = DimensionContainersV1 {
        format_version: 1,
        chests,
    };
    let cfg = bincode::config::standard();
    let raw = bincode::serde::encode_to_vec(&v1, cfg).unwrap();
    let uncompressed_len = raw.len() as u64;
    // Skip compression so the envelope's compression_kind stays at
    // 0 - simpler test fixture, exercises the COMP_NONE branch in
    // the migration step.
    let body = raw;
    let mut bytes: Vec<u8> = Vec::with_capacity(body.len() + 17);
    bytes.extend_from_slice(CONTAINERS_MAGIC);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0); // COMP_NONE
    bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
    bytes.extend_from_slice(&body);
    let path = containers_path_in_dir(base, slug, dim);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
}

#[test]
fn containers_v1_to_v2_migration_round_trip() {
    // Hand-craft a v1 file (chests only) on disk + read it
    // through `read_containers_in_dir_with_registry` with a
    // registered v1 -> v2 arrow. The loaded bundle must surface
    // at v2 with `furnaces: Vec::new()`.
    use super::containers::{containers_v1_to_v2, read_containers_in_dir_with_registry};

    let tmp = TempDir::new().unwrap();
    write_v1_containers_file(
        tmp.path(),
        SLUG,
        DIM,
        vec![populated_chest_at([3, 64, 7])],
    );
    let mut registry = MigrationRegistry::new();
    registry.register(SaveKind::Containers, 1, CONTAINERS_FORMAT_VERSION, containers_v1_to_v2);
    let loaded = read_containers_in_dir_with_registry(tmp.path(), SLUG, DIM, &registry)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.format_version, CONTAINERS_FORMAT_VERSION);
    assert_eq!(loaded.chests.len(), 1);
    assert_eq!(loaded.chests[0].pos, [3, 64, 7]);
    assert_eq!(loaded.chests[0].slots[0], Some((ITEM_STONE, 64)));
    // Migration must inject an empty furnaces vec, NOT pull
    // anything from the v1 payload.
    assert!(loaded.furnaces.is_empty());

    // File on disk has been rewritten at the new version: the
    // reader writes the migrated payload back so subsequent
    // loads skip the chain.
    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let post = std::fs::read(&path).unwrap();
    assert_eq!(&post[..4], CONTAINERS_MAGIC);
    let disk_v = u32::from_le_bytes([post[4], post[5], post[6], post[7]]);
    assert_eq!(disk_v, CONTAINERS_FORMAT_VERSION);
}

#[test]
fn containers_v1_to_v2_migration_via_default_registry() {
    // Same v1 -> v2 migration but routed through
    // `default_registry()` - which `register_production_arrows`
    // populates at first use. This is the end-to-end "shipping"
    // path: a v1 save on disk + a stock binary should upgrade
    // transparently.
    let tmp = TempDir::new().unwrap();
    write_v1_containers_file(tmp.path(), SLUG, DIM, vec![empty_chest_at([0, 64, 0])]);
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded.format_version, CONTAINERS_FORMAT_VERSION);
    assert!(loaded.furnaces.is_empty());
    assert_eq!(loaded.chests.len(), 1);
}

#[test]
fn containers_v1_to_v2_migration_preserves_chest_slot_data() {
    // A populated v1 file must round-trip every chest slot
    // through the migration without dropping data.
    use super::containers::{containers_v1_to_v2, read_containers_in_dir_with_registry};
    let tmp = TempDir::new().unwrap();
    write_v1_containers_file(
        tmp.path(),
        SLUG,
        DIM,
        vec![
            populated_chest_at([1, 64, 1]),
            populated_chest_at([2, 64, 2]),
            empty_chest_at([3, 64, 3]),
        ],
    );
    let mut registry = MigrationRegistry::new();
    registry.register(SaveKind::Containers, 1, CONTAINERS_FORMAT_VERSION, containers_v1_to_v2);
    let loaded = read_containers_in_dir_with_registry(tmp.path(), SLUG, DIM, &registry)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.chests.len(), 3);
    assert_eq!(loaded.chests[0].slots[26], Some((ITEM_CHEST, 4)));
    assert_eq!(loaded.chests[1].slots[5], Some((ITEM_SWORD_IRON, 1)));
    assert!(loaded.chests[2].slots.iter().all(|s| s.is_none()));
}

#[test]
fn containers_furnace_save_to_runtime_roundtrip() {
    // Cross-module: a FurnaceSave's runtime triple must convert
    // 1:1 into a `block::furnace::FurnaceRuntime`. This is the
    // shape `apply_dimension_containers` will use on load.
    let save = populated_furnace_at([7, 7, 7]);
    let rt = crate::block::furnace::FurnaceRuntime {
        fuel_left_ms: save.fuel_left_ms,
        fuel_full_ms: save.fuel_full_ms,
        cook_progress_ms: save.cook_progress_ms,
    };
    assert_eq!(rt.fuel_left_ms, 12_345);
    assert_eq!(rt.fuel_full_ms, 80_000);
    assert_eq!(rt.cook_progress_ms, 4_321);
}

#[test]
fn containers_empty_v2_uses_empty_vecs() {
    // Construct via `DimensionContainers::empty()`; both vecs
    // must start empty.
    let c = DimensionContainers::empty();
    assert_eq!(c.format_version, CONTAINERS_FORMAT_VERSION);
    assert!(c.chests.is_empty());
    assert!(c.furnaces.is_empty());
}

#[test]
fn containers_compression_smaller_payload_with_furnaces() {
    // A bundle full of similar furnaces still compresses well via
    // zstd at the v2 schema.
    let tmp = TempDir::new().unwrap();
    let mut original = DimensionContainers::empty();
    for i in 0..32i32 {
        original.furnaces.push(populated_furnace_at([i, 64, i]));
    }
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();
    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], CONTAINERS_MAGIC);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, CONTAINERS_FORMAT_VERSION);
    // Repetitive payload -> zstd should win.
    assert_eq!(bytes[8], 1, "repetitive furnace bundle should compress");
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
}

#[test]
fn containers_two_writes_keep_bak() {
    // The atomic save layer should leave a `.bak` after the second
    // write - matches `entities_recover_from_bak` precondition.
    let tmp = TempDir::new().unwrap();
    let first = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![empty_chest_at([0, 64, 0])],
        furnaces: Vec::new(),
    };
    let second = DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![populated_chest_at([0, 64, 0])],
        furnaces: Vec::new(),
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
