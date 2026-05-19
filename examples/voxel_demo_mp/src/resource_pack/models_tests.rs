//! Tests for `resource_pack::models` extracted into a sibling file
//! so the parser source itself stays under the 500 LOC soft cap.
//!
//! Mounted into the resource_pack module tree via
//! `#[cfg(test)] mod models_tests;` in `mod.rs`.

#![cfg(test)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::models::{
    CubeFace, RotationAxis, load_model_at, strip_namespace,
};

/// Stage `models/<rel>.json` under a temp MC root.
fn write_model(root: &Path, rel: &str, body: &str) {
    let path = root.join("models").join(format!("{rel}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
}

fn fixture_root() -> TempDir {
    tempfile::tempdir().unwrap()
}

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

#[test]
fn cube_all_resolves_six_faces_same_texture() {
    let dir = fixture_root();
    stage_cube_chain(dir.path());
    write_model(
        dir.path(),
        "block/stone",
        r#"{ "parent": "block/cube_all", "textures": { "all": "minecraft:block/stone" } }"#,
    );
    let m = load_model_at(dir.path(), "block/stone").expect("model");
    assert_eq!(m.elements.len(), 1);
    let e = &m.elements[0];
    assert_eq!(e.faces.up.as_ref().unwrap().texture, "stone");
    assert_eq!(e.faces.down.as_ref().unwrap().texture, "stone");
    assert_eq!(e.faces.north.as_ref().unwrap().texture, "stone");
    assert_eq!(e.faces.east.as_ref().unwrap().texture, "stone");
}

#[test]
fn cube_column_resolves_top_bottom_distinct() {
    let dir = fixture_root();
    stage_cube_chain(dir.path());
    write_model(
        dir.path(),
        "block/oak_log",
        r#"{
            "parent": "block/cube_column",
            "textures": {
                "end": "block/oak_log_top",
                "side": "block/oak_log"
            }
        }"#,
    );
    let m = load_model_at(dir.path(), "block/oak_log").expect("model");
    let e = &m.elements[0];
    assert_eq!(e.faces.up.as_ref().unwrap().texture, "oak_log_top");
    assert_eq!(e.faces.down.as_ref().unwrap().texture, "oak_log_top");
    assert_eq!(e.faces.north.as_ref().unwrap().texture, "oak_log");
    assert_eq!(e.faces.south.as_ref().unwrap().texture, "oak_log");
}

#[test]
fn cube_bottom_top_resolves_three_textures() {
    let dir = fixture_root();
    stage_cube_chain(dir.path());
    write_model(
        dir.path(),
        "block/sandstone",
        r#"{
            "parent": "block/cube_bottom_top",
            "textures": {
                "top": "block/sandstone_top",
                "bottom": "block/sandstone_bottom",
                "side": "block/sandstone"
            }
        }"#,
    );
    let m = load_model_at(dir.path(), "block/sandstone").expect("model");
    let e = &m.elements[0];
    assert_eq!(e.faces.up.as_ref().unwrap().texture, "sandstone_top");
    assert_eq!(e.faces.down.as_ref().unwrap().texture, "sandstone_bottom");
    assert_eq!(e.faces.east.as_ref().unwrap().texture, "sandstone");
}

#[test]
fn parent_chain_walks_three_levels() {
    let dir = fixture_root();
    stage_cube_chain(dir.path());
    write_model(
        dir.path(),
        "block/dirt",
        r#"{ "parent": "block/cube_all", "textures": { "all": "block/dirt" } }"#,
    );
    let m = load_model_at(dir.path(), "block/dirt").expect("model");
    assert_eq!(m.elements.len(), 1);
    assert_eq!(m.textures.get("all").map(String::as_str), Some("dirt"));
}

#[test]
fn texture_var_overridden_by_child() {
    let dir = fixture_root();
    stage_cube_chain(dir.path());
    write_model(
        dir.path(),
        "block/parent_with_default",
        r#"{
            "parent": "block/cube_all",
            "textures": { "all": "block/parent_default" }
        }"#,
    );
    write_model(
        dir.path(),
        "block/child_overrides",
        r#"{
            "parent": "block/parent_with_default",
            "textures": { "all": "block/child_winner" }
        }"#,
    );
    let m = load_model_at(dir.path(), "block/child_overrides").expect("model");
    let e = &m.elements[0];
    assert_eq!(e.faces.up.as_ref().unwrap().texture, "child_winner");
}

#[test]
fn missing_parent_is_non_fatal() {
    let dir = fixture_root();
    write_model(
        dir.path(),
        "block/stranded",
        r#"{ "parent": "block/does_not_exist", "textures": { "x": "block/orphan" } }"#,
    );
    let m = load_model_at(dir.path(), "block/stranded").expect("model");
    assert!(m.elements.is_empty());
    assert_eq!(m.textures.get("x").map(String::as_str), Some("orphan"));
}

#[test]
fn malformed_json_returns_none() {
    let dir = fixture_root();
    write_model(dir.path(), "block/bad", r#"{ this is not json"#);
    assert!(load_model_at(dir.path(), "block/bad").is_none());
}

#[test]
fn rotation_axis_parses_for_torch_wall() {
    let dir = fixture_root();
    stage_cube_chain(dir.path());
    write_model(
        dir.path(),
        "block/wall_torch",
        r#"{
            "ambientocclusion": false,
            "elements": [
                {
                    "from": [-1, 3.5, 7],
                    "to":   [1, 13.5, 9],
                    "rotation": { "origin": [0, 3.5, 8], "axis": "z", "angle": -22.5 },
                    "shade": false,
                    "faces": {
                        "up":   { "uv": [7, 6, 9, 8],  "texture": "block/torch" },
                        "down": { "uv": [7, 13, 9, 15], "texture": "block/torch" }
                    }
                }
            ]
        }"#,
    );
    let m = load_model_at(dir.path(), "block/wall_torch").expect("model");
    let e = &m.elements[0];
    let r = e.rotation.expect("rotation present");
    assert_eq!(r.axis, RotationAxis::Z);
    assert!((r.angle_deg + 22.5).abs() < 1e-3);
    assert!(!e.shade);
    assert!(!m.ambient_occlusion);
}

#[test]
fn unresolved_var_left_as_literal() {
    let dir = fixture_root();
    stage_cube_chain(dir.path());
    write_model(
        dir.path(),
        "block/bare_cube",
        r#"{ "parent": "block/cube" }"#,
    );
    let m = load_model_at(dir.path(), "block/bare_cube").expect("model");
    let e = &m.elements[0];
    assert_eq!(e.faces.up.as_ref().unwrap().texture, "#up");
}

#[test]
fn face_uv_defaults_to_full_tile_when_omitted() {
    let dir = fixture_root();
    write_model(
        dir.path(),
        "block/uvless",
        r#"{
            "elements": [
                { "from": [0,0,0], "to": [16,16,16],
                  "faces": { "up": { "texture": "block/stone" } } }
            ]
        }"#,
    );
    let m = load_model_at(dir.path(), "block/uvless").expect("model");
    let f = m.elements[0].faces.up.as_ref().unwrap();
    assert_eq!(f.uv, [0.0, 0.0, 16.0, 16.0]);
}

#[test]
fn cullface_parses_all_cardinal_directions() {
    for raw in ["down", "up", "north", "south", "west", "east"] {
        assert!(CubeFace::parse_pub(raw).is_some(), "{raw}");
    }
    assert!(CubeFace::parse_pub("nonsense").is_none());
}

#[test]
fn cullface_aliases_top_bottom() {
    assert_eq!(CubeFace::parse_pub("top"), Some(CubeFace::Up));
    assert_eq!(CubeFace::parse_pub("bottom"), Some(CubeFace::Down));
}

#[test]
fn strip_namespace_removes_minecraft_prefix() {
    assert_eq!(strip_namespace("minecraft:block/stone"), "block/stone");
    assert_eq!(strip_namespace("block/stone"), "block/stone");
}

#[test]
fn tintindex_parses_for_grass() {
    let dir = fixture_root();
    write_model(
        dir.path(),
        "block/grass_tinted",
        r#"{
            "elements": [
                { "from": [0,0,0], "to": [16,16,16],
                  "faces": { "up": { "texture": "block/grass_top", "tintindex": 0 } } }
            ]
        }"#,
    );
    let m = load_model_at(dir.path(), "block/grass_tinted").expect("model");
    assert_eq!(m.elements[0].faces.up.as_ref().unwrap().tintindex, Some(0));
}
