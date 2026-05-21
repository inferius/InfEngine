//! W18.3 + W18.4 regression tests for the textured particle emitters
//! and chunk-mesh anim_id assignment.
//!
//! W18.3 covers:
//! - `emit_textured_flame` emits 1 particle with atlas-resolved UV
//!   when a "flame" strip is registered.
//! - `emit_textured_lava` emits a particle with the "lava" sprite UV.
//! - `emit_textured_smoke` picks a smoke frame within the 0..7 range.
//!
//! W18.4 covers:
//! - Animated water_still face -> anim_id > 0 when pack registers it.
//! - Static stone face -> anim_id == 0 always.
//! - Pack with water_still/lava_still/fire_0 registers all 3 in anim_registry.

#![cfg(test)]

use infengine_render::TexturedParticleInstance;

use crate::resource_pack::particle_atlas::tests::synth_atlas;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Cheap xorshift seed matching the emitter helpers.
fn mk_seed(v: u32) -> u32 {
    v ^ 0xDEADBEEF
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

/// `emit_textured_flame` must push exactly one particle per call.
/// When a real "flame" strip is in the atlas the UV must resolve to
/// within that strip (v_max <= per-frame height).
#[test]
fn torch_emits_flame_textured_particle_with_atlas_uv() {
    // Four-frame flame strip. `synth_atlas` stores the UV for frame 0
    // as [u0, 0.0, u1, 1/frames] = [0.0, 0.0, 1.0, 0.25] (only entry).
    let atlas = synth_atlas(&[("flame", 4)]);
    let pos = [5i32, 64, 7];
    let mut out: Vec<TexturedParticleInstance> = Vec::new();
    let mut s = mk_seed(42);
    crate::emit_textured_flame(&mut out, Some(&atlas), &pos, &mut s, [1.0, 0.78, 0.40]);

    // W45.4: emit_textured_flame now emits FLAME_LAYERS=3 overlapping
    // quads per call (anti-jump fix, Option C). Verify we get 3.
    assert_eq!(out.len(), 3, "W45.4: three overlapping frame layers per call");

    // Primary layer (index 0) carries the highest alpha.
    let p0 = &out[0];
    // Center y should be near pos[1] + 0.8 = 64.8 (with jitter <= 0.2).
    assert!(
        (p0.center_world[1] - 64.8).abs() < 0.25,
        "center_y={} should be near 64.8",
        p0.center_world[1],
    );
    // UV must not be the fallback [0,0,1,1]. The flame strip in
    // synth_atlas has v_max = 0.25 for frame 0; any selected frame has
    // v_max <= 0.25 (or 0.50 or 0.75 for subsequent frames, all < 1.0).
    // A simple bound: uv_max[1] < 1.0 (within the strip, not the fallback).
    let v_max = p0.uv_max[1];
    assert!(
        v_max < 1.0,
        "uv_max.v={v_max} should be within flame strip (< 1.0), not white fallback 1.0",
    );
    // Alpha of layer 0 should be ~0.95.
    assert!((p0.color[3] - 0.95).abs() < 0.01, "primary layer alpha should be 0.95");
    // Alpha of layer 1 should be ~0.55.
    assert!((out[1].color[3] - 0.55).abs() < 0.01, "layer 1 alpha should be 0.55");
    // Alpha of layer 2 should be ~0.30.
    assert!((out[2].color[3] - 0.30).abs() < 0.01, "layer 2 alpha should be 0.30");
}

/// `emit_textured_lava` must push one particle and resolve the "lava"
/// sprite UV when the atlas contains that entry.
#[test]
fn lava_emits_lava_drip_textured_particle() {
    // Single-frame lava sprite. synth_atlas maps it to [0, 0, 1, 1] for
    // a one-entry atlas (step = 1.0 / 1 = 1.0).
    let atlas = synth_atlas(&[("lava", 1)]);
    let pos = [10i32, 40, 10];
    let mut out: Vec<TexturedParticleInstance> = Vec::new();
    let mut s = mk_seed(7);
    crate::emit_textured_lava(&mut out, Some(&atlas), &pos, &mut s);

    assert_eq!(out.len(), 1, "one lava drip per call");
    let p = &out[0];
    // uv_min and uv_max must match the "lava" sprite registered.
    let expected_uv = atlas.sprite_uv("lava", 0).expect("lava sprite in atlas");
    assert_eq!(p.uv_min, [expected_uv[0], expected_uv[1]], "uv_min matches atlas");
    assert_eq!(p.uv_max, [expected_uv[2], expected_uv[3]], "uv_max matches atlas");
    // MC lava is red-orange: R > G and R > B.
    assert!(p.color[0] > p.color[1], "lava tint R > G");
    assert!(p.color[0] > p.color[2], "lava tint R > B");
}

/// `emit_textured_smoke` must select a smoke sprite index in [0..7].
/// With all eight smoke sprites registered the resolved UV must come
/// from one of them (not the white fallback).
#[test]
fn smoke_particle_sprite_selection_stays_within_eight_frame_range() {
    // Eight smoke sprites, each occupying 1/8 of U space.
    let entries: Vec<(&str, u32)> = vec![
        ("smoke_0", 1), ("smoke_1", 1), ("smoke_2", 1), ("smoke_3", 1),
        ("smoke_4", 1), ("smoke_5", 1), ("smoke_6", 1), ("smoke_7", 1),
    ];
    let atlas = synth_atlas(&entries);

    // Emit 64 smoke particles with varied seeds.  Every resolved
    // UV should map back to one of the eight registered sprites.
    let known_uvs: Vec<[f32; 4]> = (0..8u32)
        .map(|i| {
            let name = format!("smoke_{i}");
            atlas.sprite_uv(&name, 0).expect("smoke sprite registered")
        })
        .collect();

    for seed_v in 0u32..64 {
        let mut out: Vec<TexturedParticleInstance> = Vec::new();
        let mut s = mk_seed(seed_v * 17 + 3);
        crate::emit_textured_smoke(&mut out, Some(&atlas), &[0, 64, 0], &mut s);
        assert_eq!(out.len(), 1, "one smoke per call (seed {seed_v})");
        let p = &out[0];
        let uv = [p.uv_min[0], p.uv_min[1], p.uv_max[0], p.uv_max[1]];
        assert!(
            known_uvs.iter().any(|k| {
                (k[0] - uv[0]).abs() < 1e-5
                    && (k[1] - uv[1]).abs() < 1e-5
                    && (k[2] - uv[2]).abs() < 1e-5
                    && (k[3] - uv[3]).abs() < 1e-5
            }),
            "seed {seed_v}: uv {:?} did not match any smoke_0..7 entry",
            uv,
        );
    }
}

// -----------------------------------------------------------------------
// W18.4: chunk-mesh anim_id tests
// -----------------------------------------------------------------------

use crate::resource_pack::{ResourcePack, TextureKey, TextureRef};

/// Build a minimal `ResourcePack` for anim_id tests.  Inserts the given
/// block textures as animated (frames > 1) or static (frames == 1) entries
/// and runs `build_anim_index` so `anim_slot()` / `anim_slot_for_ref()`
/// are populated.
fn synth_pack_with_textures(
    animated: &[(&str, [f32; 4], u32)],
    static_: &[(&str, [f32; 4])],
) -> ResourcePack {
    use std::collections::HashMap;
    use std::path::PathBuf;
    let mut textures: HashMap<TextureKey, TextureRef> = HashMap::new();
    for (name, uv, frames) in animated {
        textures.insert(
            TextureKey::Block((*name).to_string()),
            TextureRef {
                atlas_uv: *uv,
                frames: *frames,
                frametime_ms: 100,
                interpolate: false,
            },
        );
    }
    for (name, uv) in static_ {
        textures.insert(
            TextureKey::Block((*name).to_string()),
            TextureRef {
                atlas_uv: *uv,
                frames: 1,
                frametime_ms: 0,
                interpolate: false,
            },
        );
    }
    // Use the same index-building path as the production loader.
    let (anim_registry, anim_slots) =
        crate::resource_pack::build_anim_index(&textures);
    ResourcePack {
        textures,
        item_icons: HashMap::new(),
        sounds: HashMap::new(),
        source: PathBuf::from("<test>"),
        is_fallback: false,
        atlas_size: (256, 256),
        atlas_rgba: vec![0u8; 256 * 256 * 4],
        anim_registry,
        anim_slots,
        particle_atlas: None,
        // W41.1: test helper; cache not needed for anim tests.
        block_models: HashMap::new(),
    }
}

/// Animated `water_still` face must produce `anim_id > 0` from
/// `anim_slot_for_ref` when the pack loaded that texture as animated.
#[test]
fn water_block_face_has_anim_id_nonzero_when_pack_animates_water_still() {
    // water_still: 8-frame animated strip.
    let pack = synth_pack_with_textures(
        &[("water_still", [0.0, 0.0, 1.0, 0.125], 8)],
        &[],
    );
    let tex_key = TextureKey::Block("water_still".to_string());
    let slot = pack.anim_slot(&tex_key);
    assert!(slot > 0, "water_still must be in an animated slot (got {slot})");

    // Also verify via anim_slot_for_ref: given the TextureRef directly,
    // the helper must return the same non-zero slot.
    let water_ref = pack.textures[&tex_key];
    let slot_via_ref = pack.anim_slot_for_ref(&water_ref);
    assert_eq!(slot_via_ref, slot, "anim_slot_for_ref must agree with anim_slot");
}

/// Static stone face must always produce `anim_id == 0`.
#[test]
fn static_stone_face_has_anim_id_zero() {
    let pack = synth_pack_with_textures(
        &[("water_still", [0.0, 0.0, 1.0, 0.125], 8)],
        &[("stone", [0.0, 0.0, 0.0625, 0.0625])],
    );
    let stone_key = TextureKey::Block("stone".to_string());
    let slot = pack.anim_slot(&stone_key);
    assert_eq!(slot, 0, "static stone face must have anim_id 0");

    let stone_ref = pack.textures[&stone_key];
    assert_eq!(
        pack.anim_slot_for_ref(&stone_ref),
        0,
        "anim_slot_for_ref must return 0 for non-animated stone",
    );
}

/// Pack with water_still, lava_still, and fire_0 as animated textures
/// must register all three in the anim_registry with distinct non-zero
/// slots.
#[test]
fn anim_registry_pack_load_registers_water_lava_fire_textures() {
    let pack = synth_pack_with_textures(
        &[
            ("water_still", [0.0, 0.0, 1.0, 0.125], 8),
            ("lava_still",  [0.0, 0.125, 1.0, 0.25], 16),
            ("fire_0",      [0.0, 0.25, 1.0, 0.375], 8),
        ],
        &[("stone", [0.0, 0.875, 0.0625, 0.9375])],
    );
    let water_slot = pack.anim_slot(&TextureKey::Block("water_still".to_string()));
    let lava_slot  = pack.anim_slot(&TextureKey::Block("lava_still".to_string()));
    let fire_slot  = pack.anim_slot(&TextureKey::Block("fire_0".to_string()));

    assert!(water_slot > 0, "water_still must be animated (slot {water_slot})");
    assert!(lava_slot  > 0, "lava_still must be animated (slot {lava_slot})");
    assert!(fire_slot  > 0, "fire_0 must be animated (slot {fire_slot})");

    // All three must have distinct slot ids (no collision).
    assert_ne!(water_slot, lava_slot, "water_still and lava_still collide");
    assert_ne!(water_slot, fire_slot, "water_still and fire_0 collide");
    assert_ne!(lava_slot,  fire_slot, "lava_still and fire_0 collide");
}
