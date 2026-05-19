//! Wave 12.5 - Subtle point-light halo around emitters.
//!
//! The MC-style "halo" the user asked for is the warm glow falling
//! on surfaces near a torch / lantern / lava block. The chunk
//! lighting BFS already raises the surface light (block-light
//! channel) for those cells, but the actual visible WARMTH on
//! adjacent blocks - the camera-side amber tint - comes from a
//! point light. The render crate already has a shader-side slot
//! array (`lights: array<PointLight, 8>`) plumbed through
//! `queue_point_light`; this module picks which 8 emitters to
//! feed it from the active dim.
//!
//! Scope:
//! - `emitter_color(id)` returns a per-kind warm RGB (torch =
//!   orange-amber, soul_torch = cyan, glowstone = warm yellow,
//!   lava = orange-red, ...).
//! - `pick_nearest(emitters, player_pos, max)` orders the input
//!   by squared distance to the player and returns up to `max`
//!   slots. The shader-side cap is 8 (see
//!   `mesh3d.wgsl::lights: array<PointLight, 8>`).
//! - `EmitterSample` carries the world-pos + colour + intensity
//!   the caller wraps into a `PointLight` for the render queue.
//!
//! Cite `J:\Temp\template-mod-template-26.1.2\sources\
//! minecraft-merged-52430b475d-26.1.2-sources\
//! net\minecraft\client\renderer\LightTexture.java` - MC blends
//! block-light against sky-light per cell; the warm tint here is
//! the block-light contribution, fixed-amplitude per emitter
//! kind so the engine doesn't have to do a per-pixel sample.

use crate::block::{
    light_emission::light_emission_for_id, CRYING_OBSIDIAN_ID, END_ROD_ID,
    ENDER_CHEST_ID, GLOWSTONE_ID, JACK_O_LANTERN_ID, LANTERN_ID, LAVA_ID,
    LIT_REDSTONE_LAMP_ID, LIT_REDSTONE_ORE_ID, MAGMA_BLOCK_ID, REDSTONE_TORCH_ID,
    REDSTONE_WALL_TORCH_ID, SEA_LANTERN_ID, SHROOMLIGHT_ID, SOUL_FIRE_ID,
    SOUL_LANTERN_ID, SOUL_TORCH_ID, SOUL_WALL_TORCH_ID, TORCH_ID,
};

/// Max point lights the shader binding supports. Matches
/// `mesh3d.wgsl::lights: array<PointLight, 8>`. Picking N > 8
/// drops the furthest until <= 8 remain.
pub(crate) const MAX_POINT_LIGHTS: usize = 8;

/// World-space sample fed into `infengine_render::queue_point_light`.
/// Builds-up vs `PointLight` directly because the engine doesn't
/// import the render crate's types from this module - the caller
/// in main.rs translates `EmitterSample` -> `PointLight`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EmitterSample {
    /// World-space position (voxel centre).
    pub(crate) position: [f32; 3],
    /// Warm RGB tint specific to the emitter kind.
    pub(crate) color: [f32; 3],
    /// Intensity in `[0, 1]`. Scales with the MC lightLevel so a
    /// torch (14) reads dimmer than glowstone (15).
    pub(crate) intensity: f32,
}

/// Map a block id to its emitter colour. Returns the swatch every
/// kind would tint adjacent surfaces with. Picks warm orange for
/// torches/lava, cyan for soul-themed blocks, warm white for
/// glowstone/lanterns.
pub(crate) fn emitter_color(block_id: u16) -> [f32; 3] {
    match block_id {
        // Warm orange torches + fire.
        x if x == TORCH_ID => [1.00, 0.78, 0.40],
        // Redstone = red.
        x if x == REDSTONE_TORCH_ID || x == REDSTONE_WALL_TORCH_ID => [0.95, 0.20, 0.15],
        x if x == LIT_REDSTONE_ORE_ID => [0.95, 0.30, 0.30],
        // Cyan soul torches + soul fire + soul lantern.
        x if x == SOUL_TORCH_ID
            || x == SOUL_WALL_TORCH_ID
            || x == SOUL_FIRE_ID
            || x == SOUL_LANTERN_ID =>
        {
            [0.40, 0.85, 1.00]
        }
        // Warm yellow glowstone family.
        x if x == GLOWSTONE_ID
            || x == JACK_O_LANTERN_ID
            || x == LIT_REDSTONE_LAMP_ID
            || x == LANTERN_ID
            || x == SHROOMLIGHT_ID =>
        {
            [1.00, 0.92, 0.60]
        }
        // Sea lantern + end rod: cool white.
        x if x == SEA_LANTERN_ID => [0.85, 0.95, 1.00],
        x if x == END_ROD_ID => [0.95, 0.90, 0.95],
        // Lava + magma: orange-red.
        x if x == LAVA_ID => [1.00, 0.45, 0.10],
        x if x == MAGMA_BLOCK_ID => [0.85, 0.30, 0.15],
        // Crying obsidian + ender chest: purple-dim.
        x if x == CRYING_OBSIDIAN_ID || x == ENDER_CHEST_ID => [0.55, 0.20, 0.85],
        // Default: warm orange (matches MC's torch tint).
        _ => [1.00, 0.78, 0.40],
    }
}

/// Build a sample from a block id + world position. Returns `None`
/// when the block has no emission (so the caller's "iterate all
/// blocks" path can skip the wrap).
pub(crate) fn sample_for(block_id: u16, position: [f32; 3]) -> Option<EmitterSample> {
    let emission = light_emission_for_id(block_id);
    if emission == 0 {
        return None;
    }
    Some(EmitterSample {
        position,
        color: emitter_color(block_id),
        // MC emission goes 0..=15; normalise into 0..=1 for the
        // shader. Floor at 0.25 so even a magma-block (3) reads
        // visible without overpowering glowstone (15).
        intensity: (emission as f32 / 15.0).max(0.25),
    })
}

/// Squared world-space distance between two points. Avoids the
/// sqrt the picker doesn't need.
fn dist_sq(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

/// Pick the `max` nearest emitters to the player. Sorts a copy of
/// the input by squared distance + truncates. Stable order across
/// ties matches `Vec::sort_by` (an emitter spawned earlier in the
/// scan wins the tie).
pub(crate) fn pick_nearest(
    emitters: &[EmitterSample],
    player_pos: [f32; 3],
    max: usize,
) -> Vec<EmitterSample> {
    let cap = max.min(MAX_POINT_LIGHTS);
    if emitters.len() <= cap {
        return emitters.to_vec();
    }
    let mut indexed: Vec<(f32, usize)> = emitters
        .iter()
        .enumerate()
        .map(|(i, e)| (dist_sq(player_pos, e.position), i))
        .collect();
    // Partial sort by distance ascending. Full sort is fine here -
    // the emitter count per chunk is small (< 100 in practice).
    indexed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(cap);
    indexed.into_iter().map(|(_, i)| emitters[i]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_8_emitters_picked() {
        // 12 emitters in a row along +X; player at origin. Pick
        // the 8 closest. They should be the 8 with the smallest x.
        let mut all = Vec::new();
        for i in 0..12u32 {
            all.push(EmitterSample {
                position: [i as f32, 0.0, 0.0],
                color: [1.0, 1.0, 1.0],
                intensity: 1.0,
            });
        }
        let picked = pick_nearest(&all, [0.0, 0.0, 0.0], 8);
        assert_eq!(picked.len(), 8);
        // First 8 by ascending x.
        for (i, e) in picked.iter().enumerate() {
            assert!(
                (e.position[0] - i as f32).abs() < 1e-3,
                "picked[{i}] = {:?}, want x = {i}", e.position,
            );
        }
    }

    #[test]
    fn pick_caps_at_max_point_lights() {
        // Asking for 100 must still return at most 8.
        let mut all = Vec::new();
        for i in 0..50u32 {
            all.push(EmitterSample {
                position: [i as f32, 0.0, 0.0],
                color: [1.0; 3],
                intensity: 1.0,
            });
        }
        let picked = pick_nearest(&all, [0.0, 0.0, 0.0], 100);
        assert_eq!(picked.len(), MAX_POINT_LIGHTS);
    }

    #[test]
    fn emitter_with_emission_0_is_skipped() {
        // Stone (id 1) has lightLevel 0. sample_for must return None.
        assert!(sample_for(1, [0.0; 3]).is_none());
        // Air (id 0) likewise.
        assert!(sample_for(0, [0.0; 3]).is_none());
    }

    #[test]
    fn torch_sample_resolves() {
        let s = sample_for(TORCH_ID, [1.0, 2.0, 3.0]).expect("torch sample");
        assert_eq!(s.position, [1.0, 2.0, 3.0]);
        // Torch emits 14 in MC. intensity = 14/15.
        assert!((s.intensity - 14.0 / 15.0).abs() < 1e-3);
    }

    #[test]
    fn point_light_color_matches_emitter_kind() {
        // Torch: warm orange.
        let torch = emitter_color(TORCH_ID);
        assert!(torch[0] > 0.8 && torch[1] > 0.5, "torch should be warm: {torch:?}");
        // Soul torch: cyan.
        let soul = emitter_color(SOUL_TORCH_ID);
        assert!(soul[2] > 0.8 && soul[1] > 0.5, "soul torch should be cyan: {soul:?}");
        // Glowstone: warm yellow.
        let glow = emitter_color(GLOWSTONE_ID);
        assert!(
            glow[0] > 0.8 && glow[1] > 0.7 && glow[2] < 0.7,
            "glowstone should be warm yellow: {glow:?}",
        );
        // Lava: orange.
        let lava = emitter_color(LAVA_ID);
        assert!(lava[0] > 0.8 && lava[2] < 0.3, "lava should be orange: {lava:?}");
    }

    #[test]
    fn redstone_torch_is_red() {
        let c = emitter_color(REDSTONE_TORCH_ID);
        assert!(c[0] > 0.8 && c[2] < 0.3, "redstone torch should be red: {c:?}");
    }

    #[test]
    fn sea_lantern_is_cool_white() {
        let c = emitter_color(SEA_LANTERN_ID);
        // Cool white = high blue + moderate-high RG.
        assert!(c[2] > 0.9 && c[0] > 0.7);
    }

    #[test]
    fn empty_input_yields_empty_picks() {
        let picked = pick_nearest(&[], [0.0; 3], 8);
        assert!(picked.is_empty());
    }

    #[test]
    fn input_shorter_than_max_passes_through() {
        let all = vec![
            EmitterSample {
                position: [1.0, 0.0, 0.0],
                color: [1.0; 3],
                intensity: 1.0,
            },
            EmitterSample {
                position: [2.0, 0.0, 0.0],
                color: [1.0; 3],
                intensity: 1.0,
            },
        ];
        let picked = pick_nearest(&all, [0.0; 3], 8);
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn intensity_floors_at_quarter_for_dim_emitters() {
        // Magma block has emission 3. intensity = max(3/15, 0.25)
        // = max(0.2, 0.25) = 0.25.
        let s = sample_for(MAGMA_BLOCK_ID, [0.0; 3]).expect("magma sample");
        assert!((s.intensity - 0.25).abs() < 1e-3);
    }

    #[test]
    fn glowstone_intensity_is_max() {
        // Glowstone emits 15 -> intensity = 1.0.
        let s = sample_for(GLOWSTONE_ID, [0.0; 3]).expect("glow sample");
        assert!((s.intensity - 1.0).abs() < 1e-3);
    }

    #[test]
    fn nearest_pick_prefers_close_over_far() {
        // Distractor at (100, 0, 0); winner at (0.5, 0, 0). Picker
        // returns the winner first.
        let all = vec![
            EmitterSample {
                position: [100.0, 0.0, 0.0],
                color: [1.0; 3],
                intensity: 1.0,
            },
            EmitterSample {
                position: [0.5, 0.0, 0.0],
                color: [1.0; 3],
                intensity: 1.0,
            },
        ];
        let picked = pick_nearest(&all, [0.0; 3], 1);
        assert_eq!(picked.len(), 1);
        assert!((picked[0].position[0] - 0.5).abs() < 1e-3);
    }
}
