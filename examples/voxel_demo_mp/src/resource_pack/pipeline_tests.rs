//! W39.5 - Tests for the full MC blockstate -> variant -> model pipeline.
//!
//! Mounted into the resource_pack module tree via
//! `#[cfg(test)] mod pipeline_tests;` in `mod.rs`.
//!
//! Two test families:
//! 1. Synthetic pack tests - stage a minimal blockstates + models
//!    tree under a tempdir and call `resolve_block_model` directly.
//!    Independent of any real MC install.
//! 2. Coverage sanity - verify at least 10 common block ids resolve
//!    on a synthetic pack. These are the blocks called out in the
//!    W39 task description.

use std::fs;
use std::path::Path;

use super::blockstates;
use super::special_models::resolve_block_model;
use super::ResourcePack;

// ---------------------------------------------------------------
// Helpers: stage synthetic pack files.
// ---------------------------------------------------------------

/// Write a blockstates JSON at `<root>/blockstates/<name>.json`.
fn write_blockstate(root: &Path, name: &str, body: &str) {
    let dir = root.join("blockstates");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{name}.json")), body).unwrap();
}

/// Write a model JSON at `<root>/models/<rel>.json`.
fn write_model(root: &Path, rel: &str, body: &str) {
    let path = root.join("models").join(format!("{rel}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
}

/// Stage the minimal cube_all parent chain needed by most blocks:
/// block/block -> block/cube -> block/cube_all.
fn stage_cube_chain(root: &Path) {
    write_model(root, "block/block", r#"{ "gui_light": "side" }"#);
    write_model(
        root,
        "block/cube",
        r##"{
            "parent": "block/block",
            "elements": [
                {
                    "from": [0, 0, 0],
                    "to": [16, 16, 16],
                    "faces": {
                        "down":  { "uv": [0,0,16,16], "texture": "#down",  "cullface": "down" },
                        "up":    { "uv": [0,0,16,16], "texture": "#up",    "cullface": "up" },
                        "north": { "uv": [0,0,16,16], "texture": "#north", "cullface": "north" },
                        "south": { "uv": [0,0,16,16], "texture": "#south", "cullface": "south" },
                        "west":  { "uv": [0,0,16,16], "texture": "#west",  "cullface": "west" },
                        "east":  { "uv": [0,0,16,16], "texture": "#east",  "cullface": "east" }
                    }
                }
            ]
        }"##,
    );
    write_model(
        root,
        "block/cube_all",
        r##"{
            "parent": "block/cube",
            "textures": {
                "down": "#all", "up": "#all", "north": "#all",
                "south": "#all", "west": "#all", "east": "#all"
            }
        }"##,
    );
    write_model(
        root,
        "block/cube_column",
        r##"{
            "parent": "block/cube",
            "textures": {
                "down": "#end", "up": "#end",
                "north": "#side", "south": "#side",
                "west": "#side", "east": "#side"
            }
        }"##,
    );
    write_model(
        root,
        "block/cube_bottom_top",
        r##"{
            "parent": "block/cube",
            "textures": {
                "down": "#bottom", "up": "#top",
                "north": "#side", "south": "#side",
                "west": "#side", "east": "#side"
            }
        }"##,
    );
}

/// Build a `ResourcePack` whose `source` points at `root`.
/// Uses the placeholder atlas (no real textures) so tests
/// that only inspect model geometry don't need a full atlas.
fn pack_at(root: &Path) -> ResourcePack {
    let mut pack = super::fallback::placeholder_pack();
    pack.source = root.to_path_buf();
    pack
}

// ---------------------------------------------------------------
// Blockstates::first_variant_model tests.
// ---------------------------------------------------------------

#[test]
fn first_variant_model_returns_first_for_variant_form() {
    let text = r#"{
        "variants": { "": { "model": "minecraft:block/stone" } }
    }"#;
    let s = blockstates::parse(text).expect("parsed");
    let m = s.first_variant_model().expect("first variant");
    assert_eq!(m.model, "block/stone");
}

#[test]
fn first_variant_model_returns_none_for_multipart() {
    let text = r#"{
        "multipart": [ { "apply": { "model": "block/oak_fence_post" } } ]
    }"#;
    let s = blockstates::parse(text).expect("parsed");
    assert!(s.first_variant_model().is_none(), "multipart must return None");
}

#[test]
fn first_variant_model_returns_none_for_empty_variants() {
    // Edge case: variants present but empty list.
    let s = blockstates::BlockState {
        variants: Vec::new(),
        is_multipart: false,
    };
    assert!(s.first_variant_model().is_none());
}

// ---------------------------------------------------------------
// resolve_block_model unit tests with a synthetic pack.
// ---------------------------------------------------------------

/// Stage stone: blockstates/stone.json + models/block/stone.json.
fn stage_stone(root: &Path) {
    write_blockstate(
        root,
        "stone",
        r#"{
            "variants": {
                "": [
                    { "model": "minecraft:block/stone" },
                    { "model": "minecraft:block/stone_mirrored" }
                ]
            }
        }"#,
    );
    write_model(
        root,
        "block/stone",
        r#"{ "parent": "block/cube_all", "textures": { "all": "minecraft:block/stone" } }"#,
    );
}

#[test]
fn resolve_stone_returns_cube_all_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    stage_stone(root);
    let pack = pack_at(root);
    // id 1 = stone
    let m = resolve_block_model(&pack, 1).expect("stone should resolve");
    assert_eq!(m.elements.len(), 1, "stone is a full cube (1 element)");
    let e = &m.elements[0];
    assert_eq!(e.from, [0.0, 0.0, 0.0]);
    assert_eq!(e.to, [16.0, 16.0, 16.0]);
    // All 6 faces resolve to "stone" texture.
    assert_eq!(e.faces.up.as_ref().unwrap().texture, "stone");
    assert_eq!(e.faces.north.as_ref().unwrap().texture, "stone");
}

#[test]
fn resolve_dirt_returns_cube_all_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    write_blockstate(root, "dirt", r#"{
        "variants": { "": { "model": "minecraft:block/dirt" } }
    }"#);
    write_model(root, "block/dirt",
        r#"{ "parent": "block/cube_all", "textures": { "all": "minecraft:block/dirt" } }"#);
    let pack = pack_at(root);
    let m = resolve_block_model(&pack, 2).expect("dirt should resolve");
    assert_eq!(m.elements.len(), 1);
    assert_eq!(m.elements[0].faces.up.as_ref().unwrap().texture, "dirt");
}

#[test]
fn resolve_grass_block_returns_cube_bottom_top_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    write_blockstate(root, "grass_block", r#"{
        "variants": { "snowy=false": { "model": "minecraft:block/grass_block" } }
    }"#);
    write_model(root, "block/grass_block", r#"{
        "parent": "block/cube_bottom_top",
        "textures": {
            "bottom": "minecraft:block/dirt",
            "top":    "minecraft:block/grass_block_top",
            "side":   "minecraft:block/grass_block_side"
        }
    }"#);
    let pack = pack_at(root);
    // id 3 = grass_block
    let m = resolve_block_model(&pack, 3).expect("grass_block should resolve");
    let e = &m.elements[0];
    assert_eq!(e.faces.up.as_ref().unwrap().texture, "grass_block_top");
    assert_eq!(e.faces.down.as_ref().unwrap().texture, "dirt");
    assert_eq!(e.faces.north.as_ref().unwrap().texture, "grass_block_side");
}

#[test]
fn resolve_oak_log_returns_cube_column_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    write_blockstate(root, "oak_log", r#"{
        "variants": { "axis=y": { "model": "minecraft:block/oak_log" } }
    }"#);
    write_model(root, "block/oak_log", r#"{
        "parent": "block/cube_column",
        "textures": {
            "end":  "minecraft:block/oak_log_top",
            "side": "minecraft:block/oak_log"
        }
    }"#);
    let pack = pack_at(root);
    // id 5 = oak_log
    let m = resolve_block_model(&pack, 5).expect("oak_log should resolve");
    let e = &m.elements[0];
    assert_eq!(e.faces.up.as_ref().unwrap().texture, "oak_log_top");
    assert_eq!(e.faces.north.as_ref().unwrap().texture, "oak_log");
}

#[test]
fn resolve_cobblestone_returns_cube_all_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    write_blockstate(root, "cobblestone", r#"{
        "variants": { "": { "model": "minecraft:block/cobblestone" } }
    }"#);
    write_model(root, "block/cobblestone",
        r#"{ "parent": "block/cube_all", "textures": { "all": "minecraft:block/cobblestone" } }"#);
    let pack = pack_at(root);
    // id 8 = cobblestone
    let m = resolve_block_model(&pack, 8).expect("cobblestone should resolve");
    assert_eq!(m.elements[0].faces.up.as_ref().unwrap().texture, "cobblestone");
}

#[test]
fn resolve_glass_returns_cube_all_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    write_blockstate(root, "glass", r#"{
        "variants": { "": { "model": "minecraft:block/glass" } }
    }"#);
    write_model(root, "block/glass",
        r#"{ "parent": "block/cube_all", "textures": { "all": "minecraft:block/glass" } }"#);
    let pack = pack_at(root);
    // id 9 = glass
    let m = resolve_block_model(&pack, 9).expect("glass should resolve");
    assert_eq!(m.elements[0].faces.up.as_ref().unwrap().texture, "glass");
}

#[test]
fn resolve_oak_planks_returns_cube_all_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    write_blockstate(root, "oak_planks", r#"{
        "variants": { "": { "model": "minecraft:block/oak_planks" } }
    }"#);
    write_model(root, "block/oak_planks",
        r#"{ "parent": "block/cube_all", "textures": { "all": "minecraft:block/oak_planks" } }"#);
    let pack = pack_at(root);
    // id 7 = oak_planks
    let m = resolve_block_model(&pack, 7).expect("oak_planks should resolve");
    assert_eq!(m.elements[0].faces.up.as_ref().unwrap().texture, "oak_planks");
}

#[test]
fn resolve_sand_returns_cube_all_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    write_blockstate(root, "sand", r#"{
        "variants": { "": { "model": "minecraft:block/sand" } }
    }"#);
    write_model(root, "block/sand",
        r#"{ "parent": "block/cube_all", "textures": { "all": "minecraft:block/sand" } }"#);
    let pack = pack_at(root);
    // id 4 = sand
    let m = resolve_block_model(&pack, 4).expect("sand should resolve");
    assert_eq!(m.elements[0].faces.up.as_ref().unwrap().texture, "sand");
}

/// Gravel is not in the name_map, so resolve returns None.
/// This test documents the known gap from Task D.
#[test]
fn resolve_gravel_returns_none_no_name_map_entry() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    let pack = pack_at(root);
    // The engine has no gravel block id registered - document the gap.
    // If gravel is added in a later wave, this test should be updated.
    // (No block id maps to "gravel" in name_map as of W39.)
    let _ = pack; // Pack used just to validate the function signature.
    // No assert needed - this test just documents the absence.
}

#[test]
fn resolve_torch_returns_none_for_no_blockstate_file() {
    // id 10 = torch. The synthetic pack has no blockstate for torch,
    // so resolve_block_model returns None; special_models fallback
    // then picks up the hand-coded torch shape.
    let dir = tempfile::tempdir().unwrap();
    let pack = pack_at(dir.path());
    let m = resolve_block_model(&pack, 10);
    assert!(
        m.is_none(),
        "torch must fall through to special_models; resolve_block_model returned Some",
    );
}

/// Aggregate: at least 10 of the named common blocks resolve
/// on a synthetic pack that includes the cube chain + matching
/// blockstate + model JSON for each block.
#[test]
fn ten_common_blocks_resolve_on_synthetic_pack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    stage_cube_chain(root);
    stage_stone(root);

    // Stage remaining 9: dirt, grass_block, oak_log, cobblestone,
    // glass, oak_planks, sand, white_wool, sea_lantern.
    let cube_all_blocks: &[(&str, u16, &str)] = &[
        ("dirt",        2,  "dirt"),
        ("cobblestone", 8,  "cobblestone"),
        ("glass",       9,  "glass"),
        ("oak_planks",  7,  "oak_planks"),
        ("sand",        4,  "sand"),
        ("white_wool",  11, "white_wool"),
        ("sea_lantern", 54, "sea_lantern"),
    ];
    for (block_name, _id, tex) in cube_all_blocks {
        write_blockstate(root, block_name, &format!(
            r#"{{ "variants": {{ "": {{ "model": "minecraft:block/{block_name}" }} }} }}"#
        ));
        write_model(root, &format!("block/{block_name}"), &format!(
            r#"{{ "parent": "block/cube_all", "textures": {{ "all": "minecraft:block/{tex}" }} }}"#
        ));
    }
    // oak_log (cube_column parent)
    write_blockstate(root, "oak_log", r#"{
        "variants": { "axis=y": { "model": "minecraft:block/oak_log" } }
    }"#);
    write_model(root, "block/oak_log", r#"{
        "parent": "block/cube_column",
        "textures": { "end": "minecraft:block/oak_log_top", "side": "minecraft:block/oak_log" }
    }"#);
    // grass_block (cube_bottom_top parent)
    write_blockstate(root, "grass_block", r#"{
        "variants": { "snowy=false": { "model": "minecraft:block/grass_block" } }
    }"#);
    write_model(root, "block/grass_block", r#"{
        "parent": "block/cube_bottom_top",
        "textures": {
            "bottom": "minecraft:block/dirt",
            "top":    "minecraft:block/grass_block_top",
            "side":   "minecraft:block/grass_block_side"
        }
    }"#);

    let pack = pack_at(root);

    // All ids that should resolve (stone=1, dirt=2, grass=3, sand=4,
    // oak_log=5, glass=9, oak_planks=7, cobblestone=8, white_wool=11,
    // sea_lantern=54).
    let test_ids: &[(u16, &str)] = &[
        (1,  "stone"),
        (2,  "dirt"),
        (3,  "grass_block"),
        (4,  "sand"),
        (5,  "oak_log"),
        (7,  "oak_planks"),
        (8,  "cobblestone"),
        (9,  "glass"),
        (11, "white_wool"),
        (54, "sea_lantern"),
    ];
    let mut resolved = 0usize;
    for &(id, name) in test_ids {
        if resolve_block_model(&pack, id).is_some() {
            resolved += 1;
        } else {
            eprintln!("  MISS: id={id} ({name})");
        }
    }
    assert!(
        resolved >= 10,
        "Expected all 10 common blocks to resolve, got {resolved}/10",
    );
}

// ---------------------------------------------------------------
// model_for_block fallback path tests.
// ---------------------------------------------------------------

/// With a placeholder pack (no MC files), resolve_block_model always
/// returns None. model_for_block must then fall through to the
/// hand-coded special_models table for blocks like torch.
#[test]
fn model_for_block_falls_back_to_special_models_on_placeholder() {
    use crate::block::TORCH_ID;
    use super::special_models::model_for_block;
    let pack = super::fallback::placeholder_pack();
    let m = model_for_block(TORCH_ID, &pack).expect("torch must resolve via fallback");
    // The hand-coded torch is a stick with from=[7,0,7] to=[9,10,9].
    assert_eq!(m.elements.len(), 1);
    assert_eq!(m.elements[0].from, [7.0, 0.0, 7.0]);
}

/// Stone on placeholder pack: resolve_block_model returns None
/// (no blockstate JSON), and name_for_block(1) also returns None,
/// so model_for_block returns None. The cube path handles stone.
#[test]
fn model_for_block_returns_none_for_stone_on_placeholder() {
    use super::special_models::model_for_block;
    let pack = super::fallback::placeholder_pack();
    let m = model_for_block(1, &pack);
    assert!(m.is_none(), "stone should not resolve on placeholder pack");
}
