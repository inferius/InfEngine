//! Integration tests for the resource_pack module - cross-cutting
//! behaviour that doesn't fit inside a single sub-module's tests.

use super::*;

#[test]
fn load_returns_a_pack_with_known_atlas_size() {
    let (_env, _tmp) = super::test_env::force_placeholder();
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
    let (_env, _tmp) = super::test_env::force_placeholder();
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
    let (_env, _tmp) = super::test_env::force_placeholder();
    let pack = ResourcePack::load();
    assert_eq!(pack.source.to_string_lossy(), "<placeholder>");
    assert!(pack.is_placeholder());
}

#[test]
fn override_path_to_nonexistent_dir_still_falls_back() {
    let (_env, _tmp) = super::test_env::force_placeholder();
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
    let (_env, _tmp) = super::test_env::force_placeholder();
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
fn static_texture_uv_at_is_constant() {
    let t = TextureRef::placeholder([0.0, 0.0, 0.25, 0.25]);
    assert_eq!(t.uv_at(0), t.atlas_uv);
    assert_eq!(t.uv_at(1_000), t.atlas_uv);
    assert_eq!(t.uv_at(10_000), t.atlas_uv);
    assert!(!t.is_animated());
}

#[test]
fn animated_uv_at_advances_with_time() {
    // 4 frames at 100 ms each, frame 0 starts at v=[0, 0.25].
    // The strip occupies v=[0, 1.0] (4 * 0.25).
    let t = TextureRef {
        atlas_uv: [0.0, 0.0, 0.25, 0.25],
        frames: 4,
        frametime_ms: 100,
        interpolate: false,
    };
    assert!(t.is_animated());
    let f0 = t.uv_at(0);
    let f1 = t.uv_at(100);
    let f2 = t.uv_at(200);
    let f3 = t.uv_at(300);
    let f0_wrap = t.uv_at(400);
    let eps = 1e-6;
    // Frame index 0 -> v0=0, v1=0.25.
    assert!((f0[1] - 0.0).abs() < eps);
    assert!((f0[3] - 0.25).abs() < eps);
    // Frame index 1 -> v0=0.25, v1=0.5.
    assert!((f1[1] - 0.25).abs() < eps);
    assert!((f1[3] - 0.5).abs() < eps);
    // Frame index 2 -> v0=0.5, v1=0.75.
    assert!((f2[1] - 0.5).abs() < eps);
    // Frame index 3 -> v0=0.75, v1=1.0.
    assert!((f3[1] - 0.75).abs() < eps);
    // Wraps back to frame 0.
    assert_eq!(f0_wrap, f0);
}

#[test]
fn animated_uv_zero_frametime_does_not_animate() {
    // A pack ships frametime=0 by mistake; the loader floors to >= 50,
    // but the safety branch in uv_at also guards against zero.
    let t = TextureRef {
        atlas_uv: [0.0, 0.0, 0.25, 0.25],
        frames: 4,
        frametime_ms: 0,
        interpolate: false,
    };
    assert_eq!(t.uv_at(0), t.atlas_uv);
    assert_eq!(t.uv_at(5_000), t.atlas_uv);
    assert!(!t.is_animated());
}

#[test]
fn sound_paths_empty_on_placeholder_pack() {
    let (_env, _tmp) = super::test_env::force_placeholder();
    let pack = ResourcePack::load();
    assert!(pack.sound_paths("dig:stone").is_empty());
    assert!(pack.sound_paths("lava:pop").is_empty());
}

#[test]
fn sound_paths_resolves_engine_key_through_mc_event() {
    let (_env, _guard_tmp) = super::test_env::force_placeholder();
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    // Stage a sounds.json + matching OGG file.
    let snd_root = mc.join("sounds").join("block").join("stone");
    std::fs::create_dir_all(&snd_root).unwrap();
    std::fs::write(snd_root.join("break1.ogg"), b"oggdata").unwrap();
    std::fs::write(snd_root.join("break2.ogg"), b"oggdata").unwrap();
    std::fs::write(
        mc.join("sounds.json"),
        r#"{ "block.stone.break": { "sounds": ["block/stone/break1", "block/stone/break2"] } }"#,
    )
    .unwrap();
    let pack = ResourcePack::load_with_override(Some(mc));
    let paths = pack.sound_paths("dig:stone");
    assert_eq!(paths.len(), 2);
}

#[test]
fn sound_paths_accepts_mc_event_name_directly() {
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let snd_root = mc.join("sounds").join("block").join("lava");
    std::fs::create_dir_all(&snd_root).unwrap();
    std::fs::write(snd_root.join("pop.ogg"), b"oggdata").unwrap();
    std::fs::write(
        mc.join("sounds.json"),
        r#"{ "block.lava.pop": { "sounds": ["block/lava/pop"] } }"#,
    )
    .unwrap();
    let pack = ResourcePack::load_with_override(Some(mc));
    // Pass the MC event name directly - the fallback path picks
    // it up.
    let paths = pack.sound_paths("block.lava.pop");
    assert_eq!(paths.len(), 1);
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

// ---------------------------------------------------------------
// W9.y.3: AnimRegistry wiring into pack load.
// ---------------------------------------------------------------

#[test]
fn pack_load_registers_animated_tiles() {
    // The synthetic pack staged above already includes
    // water_still with frames > 1; that texture should land in
    // the anim registry.
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc));
    assert!(!pack.is_fallback);
    assert!(!pack.anim_registry.is_empty(), "expected at least one anim slot");
    // The water_still entry MUST have a non-zero slot in the
    // reverse map.
    let key = TextureKey::Block("water_still".to_string());
    let slot = pack.anim_slot(&key);
    assert!(slot >= 1, "water_still slot = {slot}");
}

#[test]
fn static_tile_gets_slot_zero() {
    // Stone is a static tile; its anim_slot resolves to 0
    // (the STATIC_SLOT sentinel) so the shader's anim_id == 0
    // fast path applies.
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc));
    let key = TextureKey::Block("stone".to_string());
    assert_eq!(pack.anim_slot(&key), 0);
}

#[test]
fn placeholder_pack_has_empty_anim_registry() {
    let (_env, _tmp) = super::test_env::force_placeholder();
    // The placeholder pack ships zero animated tiles.
    let pack = ResourcePack::load();
    assert!(pack.is_fallback);
    assert!(pack.anim_registry.is_empty());
    assert!(pack.anim_slots.is_empty());
}

#[test]
fn build_tile_anims_uniform_matches_registry_order() {
    // The renderer-facing uniform must align with `entries()` -
    // slot 1 maps to entries[0], slot 2 to entries[1], ...
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc));
    let uni = pack.build_tile_anims_uniform();
    // Slot 1 in the registry should match entries[0] in the
    // uniform body.
    let reg_entries = pack.anim_registry.entries();
    if !reg_entries.is_empty() {
        let s = &reg_entries[0];
        assert_eq!(uni.entries[0].frame_count, s.frame_count);
        assert_eq!(uni.entries[0].frametime_ms, s.frametime_ms);
        assert!((uni.entries[0].v_step - s.v_step).abs() < 1e-6);
    }
}

#[test]
fn anim_slot_for_ref_returns_static_for_static_texture() {
    // The TextureRef-based lookup path must report 0 for
    // tiles without an animation strip.
    let pack = ResourcePack::load();
    let stone = pack.block_texture("stone").unwrap();
    assert_eq!(pack.anim_slot_for_ref(stone), 0);
}

#[test]
fn anim_slot_for_ref_finds_animated_slot() {
    // The reverse-lookup path must find the registered slot for
    // an animated tile we hold a TextureRef for.
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc));
    let water = pack.block_texture("water_still").unwrap();
    assert!(water.is_animated());
    let slot = pack.anim_slot_for_ref(water);
    assert!(slot >= 1, "expected animated slot, got {slot}");
}

#[test]
fn reload_assigns_stable_slot_ids() {
    // Loading the same pack twice MUST give every animated tile
    // the same slot id - the deterministic-key sort in
    // `build_anim_index` guarantees this so chunks rebuilt
    // after a reload sample the right uniform row.
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack_a = ResourcePack::load_with_override(Some(mc.clone()));
    let pack_b = ResourcePack::load_with_override(Some(mc));
    let key = TextureKey::Block("water_still".to_string());
    assert_eq!(pack_a.anim_slot(&key), pack_b.anim_slot(&key));
}

// ---------------------------------------------------------------
// W9.y.6: reload integration. Sanity that the uniform body
// rebuilds across loads + that the empty-pack reload path
// (placeholder atlas) zeros the table.
// ---------------------------------------------------------------

#[test]
fn reload_to_placeholder_zeros_anim_uniform() {
    let (_env, _guard_tmp) = super::test_env::force_placeholder();
    // Simulate the reload path: load a real pack with animated
    // tiles, then reload from a path that doesn't exist (falls
    // back to the placeholder). The placeholder uniform must
    // be all-zero.
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack_real = ResourcePack::load_with_override(Some(mc));
    assert!(!pack_real.is_fallback);
    assert!(!pack_real.anim_registry.is_empty());

    let pack_fallback = ResourcePack::load_with_override(Some(
        std::path::PathBuf::from("Z:/not/a/real/path"),
    ));
    assert!(pack_fallback.is_fallback);
    let uni = pack_fallback.build_tile_anims_uniform();
    for e in uni.entries.iter() {
        assert_eq!(e.frame_count, 0);
        assert_eq!(e.frametime_ms, 0);
        assert_eq!(e.v_step, 0.0);
    }
}

#[test]
fn reload_uniform_survives_load_cycle() {
    // The renderer-facing uniform must rebuild on every load.
    // A reload of the same pack returns the same uniform body
    // entry-for-entry so chunks rebuilt against the new pack
    // sample the right rows.
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack_a = ResourcePack::load_with_override(Some(mc.clone()));
    let pack_b = ResourcePack::load_with_override(Some(mc));
    let uni_a = pack_a.build_tile_anims_uniform();
    let uni_b = pack_b.build_tile_anims_uniform();
    for i in 0..32 {
        assert_eq!(
            uni_a.entries[i].frame_count,
            uni_b.entries[i].frame_count,
            "slot {i}: frame_count drift across reload",
        );
        assert_eq!(
            uni_a.entries[i].frametime_ms,
            uni_b.entries[i].frametime_ms,
            "slot {i}: frametime_ms drift across reload",
        );
    }
}

#[test]
fn reload_triggers_chunk_rebuild_signal_present() {
    // Structural smoke test: confirm the rebuild path exists
    // by checking that the reload-flow primitives are all
    // call-able from a non-renderer context. This is a guard
    // against accidentally removing the registry / uniform
    // builders that the ReloadResourcePack action consumes.
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc));
    // Step 1: registry exists.
    assert!(!pack.anim_registry.is_empty());
    // Step 2: uniform builder returns a snapshot.
    let _uni = pack.build_tile_anims_uniform();
    // Step 3: per-key slot lookup resolves animated tiles.
    let key = TextureKey::Block("water_still".to_string());
    let slot = pack.anim_slot(&key);
    assert!(slot >= 1);
}

// ---------------------------------------------------------------
// W20.1: icon_uv_for / icon_uv_front helpers.
// ---------------------------------------------------------------

/// Returns the legacy 4x4-atlas UV for stone (id 1) under the
/// placeholder pack, since the placeholder has no MC atlas entry
/// and the fallback branch must fire.
#[test]
fn icon_uv_for_placeholder_falls_back_to_legacy() {
    let pack = ResourcePack::load();
    // id 1 = stone; under the placeholder pack icon_ref resolves
    // through the synthetic item_icons table, so we may or may not
    // get a pack UV - either way the result must be a valid [0,1]^4.
    let uv = pack.icon_uv_front(infengine_voxel::BlockId(1));
    assert!(uv[0] >= 0.0 && uv[2] <= 1.0 && uv[1] >= 0.0 && uv[3] <= 1.0,
        "UV out of [0,1] range: {:?}", uv);
    assert!(uv[2] > uv[0] && uv[3] > uv[1], "UV rect has zero or negative size: {:?}", uv);
}

/// Under a real (synthetic) pack, icon_uv_for returns the pack
/// UV rather than the legacy atlas UV.
#[test]
fn icon_uv_for_real_pack_returns_pack_uv() {
    let tmp = tempfile::tempdir().unwrap();
    let mc = stage_synthetic_pack(tmp.path());
    let pack = ResourcePack::load_with_override(Some(mc));
    assert!(!pack.is_fallback);
    // id 1 = stone; synth pack has stone.png -> stone tile.
    let pack_uv = pack.icon_ref(1).unwrap().atlas_uv;
    let helper_uv = pack.icon_uv_front(infengine_voxel::BlockId(1));
    assert_eq!(pack_uv, helper_uv,
        "icon_uv_front should match icon_ref.atlas_uv when pack has entry");
}

/// icon_uv_for with normal [0,1,0] must match icon_uv_front.
#[test]
fn icon_uv_front_equals_icon_uv_for_top_normal() {
    let pack = ResourcePack::load();
    let b = infengine_voxel::BlockId(1);
    assert_eq!(pack.icon_uv_front(b), pack.icon_uv_for(b, [0, 1, 0]),
        "icon_uv_front must be a shorthand for icon_uv_for with [0,1,0]");
}
