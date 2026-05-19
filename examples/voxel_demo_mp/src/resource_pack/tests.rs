//! Integration tests for the resource_pack module - cross-cutting
//! behaviour that doesn't fit inside a single sub-module's tests.

use super::*;

#[test]
fn load_returns_a_pack_with_known_atlas_size() {
    let pack = ResourcePack::load_with_override(None);
    // Without an explicit override + no MC install on the test host,
    // we always end up at the placeholder pack.
    assert!(pack.is_fallback);
    assert!(pack.atlas_size.0 >= 64 && pack.atlas_size.1 >= 64);
    assert_eq!(
        pack.atlas_rgba.len(),
        (pack.atlas_size.0 * pack.atlas_size.1 * 4) as usize,
    );
}

#[test]
fn placeholder_pack_block_texture_lookup_works() {
    let pack = ResourcePack::load();
    let stone = pack.block_texture("stone");
    assert!(stone.is_some());
    let s = stone.unwrap();
    // Stone is the 4x4-atlas tile at (0,0), so the UV rect is the
    // top-left quarter row.
    assert!((s.atlas_uv[0] - 0.0).abs() < 1e-6);
    assert!((s.atlas_uv[1] - 0.0).abs() < 1e-6);
}

#[test]
fn placeholder_pack_reports_placeholder_source() {
    let pack = ResourcePack::load();
    assert_eq!(pack.source.to_string_lossy(), "<placeholder>");
    assert!(pack.is_placeholder());
}

#[test]
fn override_path_to_nonexistent_dir_still_falls_back() {
    let pack = ResourcePack::load_with_override(Some(
        std::path::PathBuf::from("Z:/not/a/real/path/anywhere"),
    ));
    assert!(pack.is_fallback);
}
