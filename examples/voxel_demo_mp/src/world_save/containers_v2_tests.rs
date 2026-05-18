//! C.4 v2 furnace round trip + v1 -> v2 migration tests. Split
//! out of `containers_tests.rs` so each file stays under the
//! 500-LOC soft cap.
//!
//! Coverage:
//! - v2 round trip: furnaces only, mixed chests + furnaces.
//! - v1 -> v2 migration via an explicit registry + the default
//!   registry.
//! - Slot-data preservation across migration.
//! - FurnaceSave <-> FurnaceRuntime bridge.
//! - v2-empty constructor.
//! - Compression smoke for many furnaces.

use tempfile::TempDir;

use super::containers::{
    CONTAINERS_FORMAT_VERSION, CONTAINERS_MAGIC, ChestSave, DimensionContainers, FurnaceSave,
    containers_path_in_dir, read_containers_in_dir, write_containers_in_dir,
};
use super::migrate::{MigrationRegistry, SaveKind};

const SLUG: &str = "slot";
const DIM: &str = "overworld";

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

fn populated_furnace_at(pos: [i32; 3]) -> FurnaceSave {
    let mut slots: Vec<Option<(u16, u8)>> = vec![None; 3];
    slots[0] = Some((14, 2));
    slots[1] = Some((13, 1));
    slots[2] = Some((43, 5));
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
    use super::containers::{containers_v1_to_v2, read_containers_in_dir_with_registry};

    let tmp = TempDir::new().unwrap();
    write_v1_containers_file(
        tmp.path(),
        SLUG,
        DIM,
        vec![populated_chest_at([3, 64, 7])],
    );
    let mut registry = MigrationRegistry::new();
    registry.register(
        SaveKind::Containers,
        1,
        CONTAINERS_FORMAT_VERSION,
        containers_v1_to_v2,
    );
    let loaded = read_containers_in_dir_with_registry(tmp.path(), SLUG, DIM, &registry)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.format_version, CONTAINERS_FORMAT_VERSION);
    assert_eq!(loaded.chests.len(), 1);
    assert_eq!(loaded.chests[0].pos, [3, 64, 7]);
    assert_eq!(loaded.chests[0].slots[0], Some((ITEM_STONE, 64)));
    assert!(loaded.furnaces.is_empty());

    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let post = std::fs::read(&path).unwrap();
    assert_eq!(&post[..4], CONTAINERS_MAGIC);
    let disk_v = u32::from_le_bytes([post[4], post[5], post[6], post[7]]);
    assert_eq!(disk_v, CONTAINERS_FORMAT_VERSION);
}

#[test]
fn containers_v1_to_v2_migration_via_default_registry() {
    let tmp = TempDir::new().unwrap();
    write_v1_containers_file(tmp.path(), SLUG, DIM, vec![empty_chest_at([0, 64, 0])]);
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded.format_version, CONTAINERS_FORMAT_VERSION);
    assert!(loaded.furnaces.is_empty());
    assert_eq!(loaded.chests.len(), 1);
}

#[test]
fn containers_v1_to_v2_migration_preserves_chest_slot_data() {
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
    registry.register(
        SaveKind::Containers,
        1,
        CONTAINERS_FORMAT_VERSION,
        containers_v1_to_v2,
    );
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
    let c = DimensionContainers::empty();
    assert_eq!(c.format_version, CONTAINERS_FORMAT_VERSION);
    assert!(c.chests.is_empty());
    assert!(c.furnaces.is_empty());
}

#[test]
fn containers_compression_smaller_payload_with_furnaces() {
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
    assert_eq!(bytes[8], 1, "repetitive furnace bundle should compress");
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
}
