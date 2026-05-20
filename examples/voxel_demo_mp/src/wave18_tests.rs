//! W18.3 regression tests for the textured particle emitters.
//!
//! Covers:
//! - `emit_textured_flame` emits 1 particle with atlas-resolved UV
//!   when a "flame" strip is registered.
//! - `emit_textured_lava` emits a particle with the "lava" sprite UV.
//! - `emit_textured_smoke` picks a smoke frame within the 0..7 range.

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

    assert_eq!(out.len(), 1, "exactly one particle emitted per call");
    let p = &out[0];
    // Center y should be near pos[1] + 0.8 = 64.8 (with jitter <= 0.2).
    assert!(
        (p.center_world[1] - 64.8).abs() < 0.25,
        "center_y={} should be near 64.8",
        p.center_world[1],
    );
    // UV must not be the fallback [0,0,1,1]. The flame strip in
    // synth_atlas has v_max = 0.25 for frame 0; any selected frame has
    // v_max <= 0.25 * frames_picked + 0.25 but the strip UVs are all
    // sub-1.0 in v.  A simple bound: uv_max[1] < 0.4 (well within the
    // strip but safely below the no-atlas fallback of 1.0).
    let v_max = p.uv_max[1];
    assert!(
        v_max < 0.4,
        "uv_max.v={v_max} should be within flame strip (< 0.4), not white fallback 1.0",
    );
    // Alpha should be ~0.95.
    assert!((p.color[3] - 0.95).abs() < 0.01, "flame alpha should be 0.95");
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
