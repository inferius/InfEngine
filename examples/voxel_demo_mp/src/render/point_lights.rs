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
    let samples: Vec<emitter_lights::EmitterSample> = emitter_positions
        .iter()
        .filter_map(|(p, id)| {
            emitter_lights::sample_for(
                *id,
                [p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5],
            )
        })
        .collect();
    let picked = emitter_lights::pick_nearest(
        &samples,
        [player_pos.x, player_pos.y, player_pos.z],
        emitter_lights::MAX_POINT_LIGHTS,
    );
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
