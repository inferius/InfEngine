//! Tests for multi-dim save layout (Phase F.2).
//!
//! These exercise the on-disk shape directly via the `_in_dir`
//! variants of the chunks / entities / containers writers, so we
//! don't have to spin up a Voxel. The runtime save / load
//! orchestration (`runtime::dims::*`) is exercised separately by
//! the integration-style tests on top of `Voxel::new()`; those
//! sit in `dim_save_tests.rs` next to main.rs.
//!
//! Coverage:
//! - Per-dim chunk paths land in `chunks/<dim>/...` (overworld
//!   and nether don't share files).
//! - Per-dim entity files don't stomp each other.
//! - Per-dim container files don't stomp each other.
//! - A Phase E save (only `dimensions = ["overworld"]`) still
//!   loads cleanly: missing nether files are not an error.

use tempfile::TempDir;

use super::chunks::{
    CHUNK_PAYLOAD_VERSION, ChunkPayload, ChunkVoxels, chunk_path_in_dir,
    read_chunk_in_dir, write_chunk_in_dir,
};
use super::containers::{
    CONTAINERS_FORMAT_VERSION, ChestSave, DimensionContainers, FurnaceSave,
    read_containers_in_dir, write_containers_in_dir,
};
use super::entities::{
    DimensionEntities, ENTITIES_FORMAT_VERSION, MobSave, read_entities_in_dir,
    write_entities_in_dir,
};
use super::world_json::{
    NETHER_DIMENSION, WORLD_FORMAT_VERSION, create_world, read_world_in_dir,
    write_world_in_dir,
};

const SLUG: &str = "twodims";

fn empty_payload(coord: [i32; 3]) -> ChunkPayload {
    let chunk = infengine_voxel::Chunk::empty();
    ChunkPayload {
        format_version: CHUNK_PAYLOAD_VERSION,
        coord,
        voxels: ChunkVoxels::from_chunk(&chunk),
        liquid_levels: Vec::new(),
        torch_orient: Vec::new(),
        snow_layers: Vec::new(),
    }
}

#[test]
fn fresh_world_json_ships_both_dimensions() {
    let tmp = TempDir::new().unwrap();
    let wj = create_world("realistic", 42);
    write_world_in_dir(tmp.path(), SLUG, &wj).unwrap();
    let loaded = read_world_in_dir(tmp.path(), SLUG).unwrap();
    assert_eq!(loaded.format_version, WORLD_FORMAT_VERSION);
    assert_eq!(loaded.dimensions.len(), 2);
    assert!(loaded.dimensions.iter().any(|d| d == "overworld"));
    assert!(loaded.dimensions.iter().any(|d| d == NETHER_DIMENSION));
    assert_eq!(
        loaded.dimension_worldgens.get("overworld").map(String::as_str),
        Some("realistic")
    );
    assert_eq!(
        loaded.dimension_worldgens.get(NETHER_DIMENSION).map(String::as_str),
        Some("nether")
    );
}

#[test]
fn chunks_for_two_dims_dont_share_files() {
    let tmp = TempDir::new().unwrap();
    // Same coord in both dims; the file paths must differ.
    let ow_path = chunk_path_in_dir(tmp.path(), SLUG, "overworld", [0, 0, 0]);
    let nt_path = chunk_path_in_dir(tmp.path(), SLUG, "nether", [0, 0, 0]);
    assert_ne!(ow_path, nt_path);
    // Write distinct content to each so we can prove the round-trip.
    write_chunk_in_dir(tmp.path(), SLUG, "overworld", &empty_payload([0, 0, 0])).unwrap();
    write_chunk_in_dir(tmp.path(), SLUG, "nether", &empty_payload([0, 0, 0])).unwrap();
    let ow = read_chunk_in_dir(tmp.path(), SLUG, "overworld", [0, 0, 0])
        .unwrap()
        .expect("overworld payload");
    let nt = read_chunk_in_dir(tmp.path(), SLUG, "nether", [0, 0, 0])
        .unwrap()
        .expect("nether payload");
    assert_eq!(ow.coord, [0, 0, 0]);
    assert_eq!(nt.coord, [0, 0, 0]);
}

#[test]
fn per_dim_entities_files_independent() {
    let tmp = TempDir::new().unwrap();
    let ow = DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: vec![MobSave {
            id: 1,
            kind: 0,
            pos: [10.0, 64.0, 10.0],
            yaw: 0.0,
            vel: [0.0; 3],
            health: 20.0,
            age_seconds: 0.0,
        }],
        drops: Vec::new(),
        next_mob_id: 2,
        next_drop_id: 1,
        carts: Vec::new(),
        next_cart_id: 0,
    };
    let nt = DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: vec![MobSave {
            id: 2,
            kind: 3, // zombie
            pos: [0.0, 40.0, 0.0],
            yaw: 0.0,
            vel: [0.0; 3],
            health: 15.0,
            age_seconds: 0.0,
        }],
        drops: Vec::new(),
        next_mob_id: 3,
        next_drop_id: 1,
        carts: Vec::new(),
        next_cart_id: 0,
    };
    write_entities_in_dir(tmp.path(), SLUG, "overworld", &ow).unwrap();
    write_entities_in_dir(tmp.path(), SLUG, "nether", &nt).unwrap();
    let lo = read_entities_in_dir(tmp.path(), SLUG, "overworld")
        .unwrap()
        .expect("overworld entities");
    let ln = read_entities_in_dir(tmp.path(), SLUG, "nether")
        .unwrap()
        .expect("nether entities");
    assert_eq!(lo.mobs.len(), 1);
    assert_eq!(ln.mobs.len(), 1);
    assert_eq!(lo.mobs[0].id, 1);
    assert_eq!(ln.mobs[0].id, 2);
    assert_eq!(ln.mobs[0].kind, 3);
}

#[test]
fn per_dim_containers_files_independent() {
    let tmp = TempDir::new().unwrap();
    let ow = DimensionContainers {
        doors: Vec::new(),
        chest_facings: Vec::new(),
        brewing: Vec::new(),
        levers_on: Vec::new(),
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![ChestSave {
            pos: [1, 2, 3],
            slots: vec![Some((1, 64)), None, None],
        }],
        furnaces: vec![FurnaceSave {
            pos: [4, 5, 6],
            slots: vec![Some((14, 8)), Some((13, 16)), None],
            fuel_left_ms: 1000,
            fuel_full_ms: 8000,
            cook_progress_ms: 200,
        }],
        fires: Vec::new(),
    };
    let nt = DimensionContainers {
        doors: Vec::new(),
        chest_facings: Vec::new(),
        brewing: Vec::new(),
        levers_on: Vec::new(),
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: vec![ChestSave {
            pos: [10, 20, 30],
            slots: vec![Some((44, 5)), None, None], // netherrack
        }],
        furnaces: Vec::new(),
        fires: Vec::new(),
    };
    write_containers_in_dir(tmp.path(), SLUG, "overworld", &ow).unwrap();
    write_containers_in_dir(tmp.path(), SLUG, "nether", &nt).unwrap();
    let lo = read_containers_in_dir(tmp.path(), SLUG, "overworld")
        .unwrap()
        .expect("overworld containers");
    let ln = read_containers_in_dir(tmp.path(), SLUG, "nether")
        .unwrap()
        .expect("nether containers");
    assert_eq!(lo.chests.len(), 1);
    assert_eq!(lo.furnaces.len(), 1);
    assert_eq!(lo.chests[0].pos, [1, 2, 3]);
    assert_eq!(ln.chests.len(), 1);
    assert_eq!(ln.chests[0].pos, [10, 20, 30]);
    assert_eq!(ln.chests[0].slots[0], Some((44, 5)));
}

#[test]
fn phase_e_save_loads_with_only_overworld_files() {
    // A Phase E save: write only overworld files, NO nether files.
    // The reader path for nether containers/entities must surface
    // None (treated as empty by the loader) instead of an error -
    // that's the Phase F backward-compat contract.
    let tmp = TempDir::new().unwrap();
    write_entities_in_dir(
        tmp.path(),
        SLUG,
        "overworld",
        &DimensionEntities::empty(),
    )
    .unwrap();
    write_containers_in_dir(
        tmp.path(),
        SLUG,
        "overworld",
        &DimensionContainers::empty(),
    )
    .unwrap();
    // Nether files are intentionally absent. read_entities_in_dir
    // / read_containers_in_dir must return Ok(None), not Err.
    let nt_ents = read_entities_in_dir(tmp.path(), SLUG, "nether").unwrap();
    assert!(nt_ents.is_none());
    let nt_conts = read_containers_in_dir(tmp.path(), SLUG, "nether").unwrap();
    assert!(nt_conts.is_none());
}
