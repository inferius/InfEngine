//! W47.10 - emitter point-light upload, relocated from main.rs.
//!
//! Walks `Voxel.emitter_positions`, picks the N closest to the player
//! via emitter_lights::pick_nearest, and pushes each as a point light
//! to the renderer. Flicker is hashed per-position so neighbouring
//! torches don't pulse in lockstep.

use std::collections::HashMap;

use glam::Vec3;
use infengine_render::Renderer;

use crate::emitter_lights;

/// Push the closest emitters from `emitter_positions` to the renderer
/// as point lights. `day_time` drives the per-light flicker so torches
/// look alive instead of static. Cap of returned set is the engine's
/// `MAX_POINT_LIGHTS` (W46.27 = 32).
pub(crate) fn push(
    renderer: &mut Renderer,
    emitter_positions: &HashMap<[i32; 3], u16>,
    player_pos: Vec3,
    day_time: f32,
) {
    // W12.5: build candidate set from emitter_positions
    // (world_set_tracked keeps this in sync). Sort by squared
    // distance; far emitters drop out at the cap.
    //
    // W198.8: pre-filter by distance BEFORE sample_for + collect. The old
    // code built a sample for EVERY emitter in EVERY loaded chunk and then
    // sorted them all, every frame. At high view distance the caves below
    // the player are full of lava (tens of thousands of emitters), so this
    // O(all-emitters) build + sort was the constant ~25ms sim_ms gap that
    // capped the frame rate regardless of where the player stood. A point
    // light only reaches 14 blocks, so anything past a generous cull radius
    // can never be picked - a cheap squared-distance test drops it before
    // the expensive sample_for / alloc / sort path.
    let pp = [player_pos.x, player_pos.y, player_pos.z];
    const CULL_R2: f32 = 40.0 * 40.0;
    let samples: Vec<emitter_lights::EmitterSample> = emitter_positions
        .iter()
        .filter(|(p, _)| {
            let dx = p[0] as f32 + 0.5 - pp[0];
            let dy = p[1] as f32 + 0.5 - pp[1];
            let dz = p[2] as f32 + 0.5 - pp[2];
            dx * dx + dy * dy + dz * dz <= CULL_R2
        })
        .filter_map(|(p, id)| {
            emitter_lights::sample_for(
                *id,
                [p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5],
            )
        })
        .collect();
    let picked =
        emitter_lights::pick_nearest(&samples, pp, emitter_lights::MAX_POINT_LIGHTS);
    for s in picked.iter() {
        // Subtle flicker on each light's intensity hashed by position
        // so the room reads as living torchlight, not a static lamp.
        let px = s.position[0] as u32;
        let py = s.position[1] as u32;
        let pz = s.position[2] as u32;
        let phase = px
            .wrapping_mul(2654435761)
            ^ (py.wrapping_mul(0x9E37_79B1))
            ^ (pz.wrapping_mul(0x85EB_CA6B));
        let phase_f =
            (phase as f32 / u32::MAX as f32) * std::f32::consts::TAU;
        let flicker = (day_time * 8.0 + phase_f).sin();
        // W46.18: scale to renderer convention. MC torches are VERY
        // bright in dark caves. Intensity multiplier 5.5 + flicker
        // amplitude 0.7 + min floor 0.3; radius 14 blocks.
        let intensity = (s.intensity * 5.5 + 0.7 * flicker).max(0.3);
        renderer.queue_point_light(s.position, s.color, intensity, 14.0);
    }
}
