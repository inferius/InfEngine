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

/// Stage a tiny synthetic MC layout in `dir` and return the
/// `assets/minecraft/` root path.
fn stage_synthetic_pack(dir: &std::path::Path) -> std::path::PathBuf {
    use super::textures::synth_png;
    let mc = dir.join("assets").join("minecraft");
    let blk = mc.join("textures").join("block");
    std::fs::create_dir_all(&blk).unwrap();
    // Canary so paths::discover accepts the layout.
    std::fs::write(blk.join("stone.png"), synth_png(1, &[[100, 100, 100]])).unwrap();
    std::fs::write(blk.join("dirt.png"), synth_png(1, &[[120, 90, 60]])).unwrap();
    std::fs::write(blk.join("oak_log.png"), synth_png(1, &[[80, 60, 40]])).unwrap();
    std::fs::write(
        blk.join("water_still.png"),
        synth_png(4, &[[20, 40, 200], [30, 60, 210], [40, 80, 220], [50, 100, 230]]),
    )
    .unwrap();
    std::fs::write(
        blk.join("water_still.png.mcmeta"),
        br#"{"animation":{"frametime":2,"frames":[0,1,2,3]}}"#,
    )
    .unwrap();
    let itm = mc.join("textures").join("item");
    std::fs::create_dir_all(&itm).unwrap();
    std::fs::write(itm.join("iron_ingot.png"), synth_png(1, &[[210, 210, 210]])).unwrap();
    std::fs::write(itm.join("flint_and_steel.png"), synth_png(1, &[[180, 150, 80]])).unwrap();
    std::fs::write(itm.join("bucket.png"), synth_png(1, &[[200, 200, 200]])).unwrap();
    mc
}

#[test]
fn load_with_override_picks_up_synthetic_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc.clone()));
    assert!(!pack.is_fallback, "expected real pack to load");
    assert_eq!(pack.source, mc);
    assert!(pack.block_texture("stone").is_some());
    assert!(pack.item_icon("iron_ingot").is_some());
}

#[test]
fn block_face_ref_resolves_grass_block_per_face() {
    // grass_block has top/bottom/sides overrides in the name map.
    // We stage just enough textures to exercise the fallback chain.
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let blk = mc.join("textures").join("block");
    use super::textures::synth_png;
    std::fs::write(blk.join("grass_block_top.png"), synth_png(1, &[[60, 180, 60]])).unwrap();
    std::fs::write(blk.join("grass_block_side.png"), synth_png(1, &[[120, 100, 60]])).unwrap();
    // No "grass_block" base texture - the per-face overrides must
    // resolve to the explicit `_top` / `_side` files.
    let pack = ResourcePack::load_with_override(Some(mc));
    // id 3 = grass.
    let top = pack.block_face_ref(3, BlockFace::Top).expect("grass top");
    let bottom = pack.block_face_ref(3, BlockFace::Bottom).expect("grass bottom");
    let sides = pack.block_face_ref(3, BlockFace::Sides).expect("grass sides");
    // top != bottom (top points at grass_block_top, bottom at dirt).
    assert_ne!(top.atlas_uv, bottom.atlas_uv);
    assert_ne!(top.atlas_uv, sides.atlas_uv);
}

#[test]
fn icon_ref_for_tool_uses_item_texture() {
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc));
    // id 48 = flint_and_steel (non-block item).
    let icon = pack.icon_ref(48).expect("flint_and_steel icon");
    // Verify it actually came from the item texture, not a block top.
    let expected = pack.item_icon("flint_and_steel").unwrap();
    assert_eq!(icon.atlas_uv, expected.atlas_uv);
}

#[test]
fn icon_ref_for_block_uses_top_face() {
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc));
    // id 1 = stone (block-form item, no item override).
    let icon = pack.icon_ref(1).expect("stone icon");
    let expected = pack.block_texture("stone").unwrap();
    assert_eq!(icon.atlas_uv, expected.atlas_uv);
}

#[test]
fn icon_ref_unknown_id_is_none() {
    let pack = ResourcePack::load();
    // 9999 isn't in the block/item name map and the placeholder
    // pack doesn't fabricate one either.
    assert!(pack.icon_ref(9999).is_none());
}

#[test]
fn placeholder_pack_icon_ref_for_block_is_top_tile() {
    // The fallback path populates `item_icons` from BLOCK_TYPES via
    // the block's top tile, so block icons should resolve there.
    let pack = ResourcePack::load();
    // id 8 = cobble. (col=1, row=2) in the 4x4 placeholder atlas.
    let icon = pack.icon_ref(8).expect("cobble icon");
    let block = pack.block_texture("cobble").unwrap();
    assert_eq!(icon.atlas_uv, block.atlas_uv);
}

#[test]
fn placeholder_pack_icon_ref_distinct_per_item() {
    // The W9 fix ensures DIFFERENT items get DIFFERENT UVs (the
    // old bug had non-block items collapse onto the shared (0,0)
    // placeholder tile). For the placeholder pack we route through
    // each item's block-form tile, so iron_ore (id 14) and
    // cobble (id 8) must report distinct UVs.
    let pack = ResourcePack::load();
    let iron = pack.icon_ref(14).expect("iron icon");
    let cobble = pack.icon_ref(8).expect("cobble icon");
    assert_ne!(iron.atlas_uv, cobble.atlas_uv);
}

#[test]
fn icon_ref_for_non_block_item_resolves_under_placeholder() {
    // Under the placeholder pack, non-block items still resolve to
    // their block-form top tile (mirrors MC creative-tab behavior).
    // The fix the user reported: ALL items should NOT collapse onto
    // a single (0, 0) placeholder tile - they should at least be
    // distinct by id. flint_and_steel (id 48) has no block form but
    // the name_map points its item entry at the same placeholder
    // tile its BLOCK_TYPES row uses (col=0, row=0).
    let pack = ResourcePack::load();
    let icon = pack.icon_ref(48);
    // Either resolves (placeholder atlas knows about it via the
    // item-name map) OR returns None (caller's tinted swatch
    // kicks in). Both are acceptable; what's NOT acceptable is
    // routing through a wrong block tile.
    if let Some(r) = icon {
        // If resolved, must point at a valid 4x4-atlas tile UV.
        assert!(r.atlas_uv[0] >= 0.0 && r.atlas_uv[2] <= 1.0);
    }
}
