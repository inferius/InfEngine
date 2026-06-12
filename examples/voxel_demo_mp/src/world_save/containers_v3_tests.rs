//! Q.4f: v3 round trip + v2 -> v3 migration tests. Split out of
//! `containers_v2_tests.rs` so each file stays under the
//! 500-LOC soft cap.
//!
//! Coverage:
//! - v3 round trip: fires only, mixed chests + furnaces + fires.
//! - v2 -> v3 migration via an explicit registry + the default
//!   registry. Pre-Wave-2 saves load with `fires: vec![]`.
//! - FireSave position + age preservation across save/load.
//! - Compression smoke for many fires.

use tempfile::TempDir;

use super::containers::{
    CONTAINERS_FORMAT_VERSION, CONTAINERS_MAGIC, ChestSave, DimensionContainers,
    FireSave, FurnaceSave, containers_path_in_dir, read_containers_in_dir,
    write_containers_in_dir,
};
use super::migrate::{MigrationRegistry, SaveKind};

const SLUG: &str = "slot3";
const DIM: &str = "overworld";

fn populated_chest_at(pos: [i32; 3]) -> ChestSave {
    let mut slots: Vec<Option<(u16, u8)>> = vec![None; 27];
    slots[0] = Some((1, 64));
    slots[5] = Some((25, 1));
    ChestSave { pos, slots }
}

fn populated_furnace_at(pos: [i32; 3]) -> FurnaceSave {
    let mut slots: Vec<Option<(u16, u8)>> = vec![None; 3];
    slots[0] = Some((14, 2));
    FurnaceSave {
        pos,
        slots,
        fuel_left_ms: 1234,
        fuel_full_ms: 80_000,
        cook_progress_ms: 567,
    }
}

#[test]
fn containers_roundtrip_fires_v3() {
    // Pure-fires bundle survives a write/read.
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        doors: Vec::new(),
        chest_facings: Vec::new(),
        brewing: Vec::new(),
        levers_on: Vec::new(),
        rails: Vec::new(),
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: Vec::new(),
        furnaces: Vec::new(),
        fires: vec![
            FireSave {
                pos: [10, 64, 10],
                age: 0,
            },
            FireSave {
                pos: [-5, 65, 5],
                age: 7,
            },
            FireSave {
                pos: [3, 70, 3],
                age: 15,
            },
        ],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.fires.len(), 3);
    let by_pos: std::collections::HashMap<[i32; 3], u8> =
        loaded.fires.iter().map(|f| (f.pos, f.age)).collect();
    assert_eq!(by_pos.get(&[10, 64, 10]), Some(&0));
    assert_eq!(by_pos.get(&[-5, 65, 5]), Some(&7));
    assert_eq!(by_pos.get(&[3, 70, 3]), Some(&15));
}

#[test]
fn containers_roundtrip_chests_furnaces_fires_together() {
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        doors: Vec::new(),
        chest_facings: Vec::new(),
        brewing: Vec::new(),
        levers_on: Vec::new(),
        rails: Vec::new(),
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![populated_chest_at([0, 64, 0])],
        furnaces: vec![populated_furnace_at([1, 64, 1])],
        fires: vec![FireSave {
            pos: [2, 64, 2],
            age: 12,
        }],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
}

/// Construct a v2-shape on-disk envelope so the v2 -> v3 migration
/// can decode it through the production arrow.
fn write_v2_containers_file(
    base: &std::path::Path,
    slug: &str,
    dim: &str,
    chests: Vec<ChestSave>,
    furnaces: Vec<FurnaceSave>,
) {
    use serde::{Deserialize, Serialize};
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct DimensionContainersV2 {
        pub format_version: u32,
        pub chests: Vec<ChestSave>,
        pub furnaces: Vec<FurnaceSave>,
    }
    let v2 = DimensionContainersV2 {
        format_version: 2,
        chests,
        furnaces,
    };
    let cfg = bincode::config::standard();
    let raw = bincode::serde::encode_to_vec(&v2, cfg).unwrap();
    let uncompressed_len = raw.len() as u64;
    let body = raw;
    let mut bytes: Vec<u8> = Vec::with_capacity(body.len() + 17);
    bytes.extend_from_slice(CONTAINERS_MAGIC);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.push(0); // COMP_NONE
    bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
    bytes.extend_from_slice(&body);
    let path = containers_path_in_dir(base, slug, dim);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
}

#[test]
fn containers_v2_to_v3_migration_round_trip() {
    use super::containers::{
        containers_v2_to_v3, containers_v3_to_v4, containers_v4_to_v5, read_containers_in_dir_with_registry,
    };

    let tmp = TempDir::new().unwrap();
    write_v2_containers_file(
        tmp.path(),
        SLUG,
        DIM,
        vec![populated_chest_at([3, 64, 7])],
        vec![populated_furnace_at([4, 64, 7])],
    );
    let mut registry = MigrationRegistry::new();
    registry.register(SaveKind::Containers, 2, 3, containers_v2_to_v3);
    // W138: the live format moved to v4 - chain the production
    // 3 -> 4 arrow so the read reaches the current version.
    registry.register(SaveKind::Containers, 3, 4, containers_v3_to_v4);
    // W143: the live format moved to v5 - chain the production
    // 4 -> 5 arrow so the read reaches the current version.
    registry.register(
        SaveKind::Containers,
        4,
        CONTAINERS_FORMAT_VERSION,
        containers_v4_to_v5,
    );
    let loaded = read_containers_in_dir_with_registry(tmp.path(), SLUG, DIM, &registry)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.format_version, CONTAINERS_FORMAT_VERSION);
    assert_eq!(loaded.chests.len(), 1);
    assert_eq!(loaded.furnaces.len(), 1);
    // Q.4f-specific invariant: a v2 file migrates to v3 with an
    // empty `fires` vec.
    assert!(loaded.fires.is_empty());

    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let post = std::fs::read(&path).unwrap();
    assert_eq!(&post[..4], CONTAINERS_MAGIC);
    let disk_v = u32::from_le_bytes([post[4], post[5], post[6], post[7]]);
    assert_eq!(disk_v, CONTAINERS_FORMAT_VERSION);
}

#[test]
fn containers_v2_to_v3_migration_via_default_registry() {
    let tmp = TempDir::new().unwrap();
    write_v2_containers_file(
        tmp.path(),
        SLUG,
        DIM,
        vec![populated_chest_at([0, 64, 0])],
        vec![populated_furnace_at([1, 64, 1])],
    );
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded.format_version, CONTAINERS_FORMAT_VERSION);
    assert_eq!(loaded.chests.len(), 1);
    assert_eq!(loaded.furnaces.len(), 1);
    // The Q.4f migration's core invariant: pre-Wave-2 saves lift
    // cleanly with `fires: vec![]`.
    assert!(loaded.fires.is_empty());
}

#[test]
fn containers_v2_to_v3_preserves_chest_and_furnace_data() {
    use super::containers::{
        containers_v2_to_v3, containers_v3_to_v4, containers_v4_to_v5, read_containers_in_dir_with_registry,
    };
    let tmp = TempDir::new().unwrap();
    write_v2_containers_file(
        tmp.path(),
        SLUG,
        DIM,
        vec![
            populated_chest_at([1, 64, 1]),
            populated_chest_at([2, 64, 2]),
        ],
        vec![
            populated_furnace_at([3, 64, 3]),
            populated_furnace_at([4, 64, 4]),
        ],
    );
    let mut registry = MigrationRegistry::new();
    registry.register(SaveKind::Containers, 2, 3, containers_v2_to_v3);
    // W138: the live format moved to v4 - chain the production
    // 3 -> 4 arrow so the read reaches the current version.
    registry.register(SaveKind::Containers, 3, 4, containers_v3_to_v4);
    // W143: the live format moved to v5 - chain the production
    // 4 -> 5 arrow so the read reaches the current version.
    registry.register(
        SaveKind::Containers,
        4,
        CONTAINERS_FORMAT_VERSION,
        containers_v4_to_v5,
    );
    let loaded = read_containers_in_dir_with_registry(tmp.path(), SLUG, DIM, &registry)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.chests.len(), 2);
    assert_eq!(loaded.furnaces.len(), 2);
    assert_eq!(loaded.chests[0].slots[0], Some((1, 64)));
    assert_eq!(loaded.furnaces[0].fuel_left_ms, 1234);
    assert!(loaded.fires.is_empty());
}

#[test]
fn fire_save_age_15_round_trips() {
    // MC's MAX_AGE is 15; verify the max value round-trips.
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        doors: Vec::new(),
        chest_facings: Vec::new(),
        brewing: Vec::new(),
        levers_on: Vec::new(),
        rails: Vec::new(),
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: Vec::new(),
        furnaces: Vec::new(),
        fires: vec![FireSave {
            pos: [0, 0, 0],
            age: 15,
        }],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded.fires[0].age, 15);
}

#[test]
fn fire_save_position_negative_coords_round_trip() {
    let tmp = TempDir::new().unwrap();
    let original = DimensionContainers {
        doors: Vec::new(),
        chest_facings: Vec::new(),
        brewing: Vec::new(),
        levers_on: Vec::new(),
        rails: Vec::new(),
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: Vec::new(),
        furnaces: Vec::new(),
        fires: vec![FireSave {
            pos: [-1000, -64, -1000],
            age: 3,
        }],
    };
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded.fires[0].pos, [-1000, -64, -1000]);
}

#[test]
fn containers_empty_v3_uses_empty_vecs() {
    let c = DimensionContainers::empty();
    assert_eq!(c.format_version, CONTAINERS_FORMAT_VERSION);
    assert!(c.chests.is_empty());
    assert!(c.furnaces.is_empty());
    assert!(c.fires.is_empty());
}

#[test]
fn containers_compression_smaller_payload_with_fires() {
    let tmp = TempDir::new().unwrap();
    let mut original = DimensionContainers::empty();
    // Many fires - the repetitive shape compresses well via zstd.
    for i in 0..64i32 {
        original.fires.push(FireSave {
            pos: [i, 64, i],
            age: (i as u8) % 16,
        });
    }
    write_containers_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();
    let path = containers_path_in_dir(tmp.path(), SLUG, DIM);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], CONTAINERS_MAGIC);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, CONTAINERS_FORMAT_VERSION);
    let loaded = read_containers_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
}
