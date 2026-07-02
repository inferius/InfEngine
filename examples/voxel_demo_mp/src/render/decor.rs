//! W47.12 - decoration render (snow / portal / fire / liquid),
//! relocated from main.rs. Each pass walks the matching index
//! (snow_positions / portal_positions / fires / liquid_mesh_handles)
//! and draws a small per-cell mesh via the renderer's PBR or chunk
//! draw paths.

#![allow(clippy::too_many_lines)]

use glam::{Mat4, Vec3};
use infengine_render::Renderer;

use crate::block::{block_type, LAVA_ID, WATER_ID};

/// Draw snow slabs, portal swirl panels, fire crosses, and liquid
/// surface meshes. Called from on_update after the chunk + sky pass
/// and before the particle + UI overlays.
///
/// W47.12: signature takes explicit fields instead of `&mut Voxel`
/// so the caller in on_update can keep its earlier `&mut self.ui`
/// borrow alive across this call. `player_cube` comes from
/// `Voxel::player_cube.get_or_insert_with(|| renderer.cube_handle())`
/// done at the call site so this function stays renderer-only.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    renderer: &mut Renderer,
    player_cube: infengine_render::MeshHandle,
    snow_positions: &std::collections::HashMap<[i32; 3], u8>,
    portal_positions: &std::collections::HashSet<[i32; 3]>,
    fires: &std::collections::HashMap<[i32; 3], u8>,
    liquid_mesh_handles: &std::collections::HashMap<[i32; 3], Vec<(u16, infengine_render::MeshHandle)>>,
    day_time: f32,
    // W153: textured fire + portal cell meshes (None = legacy tint).
    effects: &crate::render::effect_meshes::EffectMeshes,
) {
    let _ = (block_type, LAVA_ID, WATER_ID); // silence unused on stripped paths
        // Snow layers: thin 1x0.12x1 slab covering the bottom
        // of the voxel cell. Atlas-tinted near-white so it
        // contrasts with whatever sits below.
        if !snow_positions.is_empty() {
            let cube = player_cube;
            for (p, layers) in snow_positions.iter() {
                let cx = p[0] as f32 + 0.5;
                let cy = p[1] as f32;
                let cz = p[2] as f32 + 0.5;
                let h = (*layers as f32 / 8.0).clamp(0.06, 1.0);
                let slab = Mat4::from_scale_rotation_translation(
                    Vec3::new(1.0, h, 1.0),
                    glam::Quat::IDENTITY,
                    Vec3::new(cx, cy + h * 0.5, cz),
                );
                renderer.draw_mesh_pbr(
                    cube,
                    slab.to_cols_array(),
                    [0.97, 0.98, 1.0, 1.0],
                    0.0,
                    0.9,
                );
            }
        }
        // W5.2a: animated portal swirl. Each `PORTAL_ID` voxel
        // draws as two thin quads (one Z-aligned, one X-aligned)
        // tinted procedurally so the panel reads as a swirling
        // purple curtain. The colour modulation comes from
        // `day_time` (the only frame-anchored f32 we have without
        // touching the renderer crate's shader uniforms) -> the
        // base purple `(0.6, 0.2, 0.9)` rolls between dim and
        // bright on a per-cell phase offset, so a 2x3 portal
        // doesn't strobe in lockstep.
        if !portal_positions.is_empty() {
            let cube = player_cube;
            let time = day_time;
            for p in portal_positions.iter() {
                let cx = p[0] as f32 + 0.5;
                let cy = p[1] as f32 + 0.5;
                let cz = p[2] as f32 + 0.5;
                // Per-cell phase so neighbouring cells swirl
                // out of sync; a coordinate hash gives a stable
                // offset that doesn't randomise between frames.
                let phase = ((p[0] as u32).wrapping_mul(0x9E37_79B1))
                    ^ ((p[1] as u32).wrapping_mul(0x85EB_CA6B))
                    ^ ((p[2] as u32).wrapping_mul(0xC2B2_AE35));
                let phase_f =
                    (phase as f32 / u32::MAX as f32) * std::f32::consts::TAU;
                let swirl = (time * 2.0 + phase_f).sin();
                // W153: the REAL nether_portal texture when baked -
                // the swirl now only pulses brightness on top of the
                // MC art instead of fabricating a solid purple.
                // W156: ONE flat sheet aligned with the frame, never
                // a cross. Portal interiors are at least 2 cells wide,
                // so a neighbouring portal cell along X means the
                // curtain runs along X (panel faces +-Z); otherwise
                // rotate the panel 90 deg.
                if let Some(mesh) = effects.portal {
                    let pulse = 0.85 + 0.15 * swirl;
                    let along_x = portal_positions
                        .contains(&[p[0] - 1, p[1], p[2]])
                        || portal_positions.contains(&[p[0] + 1, p[1], p[2]]);
                    let rot = if along_x {
                        glam::Quat::IDENTITY
                    } else {
                        glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
                    };
                    let m = Mat4::from_rotation_translation(
                        rot,
                        Vec3::new(cx, p[1] as f32, cz),
                    );
                    renderer.draw_mesh_pbr(
                        mesh,
                        m.to_cols_array(),
                        [pulse, pulse, pulse, 1.0],
                        0.0,
                        0.9,
                    );
                    continue;
                }
                let intensity = (0.55 + 0.35 * swirl).clamp(0.2, 1.0);
                let purple = [
                    0.6 * intensity,
                    0.2 * intensity,
                    0.9 * intensity,
                    0.75,
                ];
                // Two thin panels: one in the XY plane (Z thin)
                // and one in the ZY plane (X thin). Either covers
                // the visible face depending on the frame
                // orientation; rendering both keeps the rotation-
                // agnostic look without storing axis state.
                let xy_panel = Mat4::from_scale_rotation_translation(
                    Vec3::new(1.0, 1.0, 0.08),
                    glam::Quat::IDENTITY,
                    Vec3::new(cx, cy, cz),
                );
                let zy_panel = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.08, 1.0, 1.0),
                    glam::Quat::IDENTITY,
                    Vec3::new(cx, cy, cz),
                );
                renderer.draw_mesh_pbr(
                    cube,
                    xy_panel.to_cols_array(),
                    purple,
                    0.6,
                    0.2,
                );
                renderer.draw_mesh_pbr(
                    cube,
                    zy_panel.to_cols_array(),
                    purple,
                    0.6,
                    0.2,
                );
            }
        }
        // W8.4: fire cell rendering. Each FIRE_ID voxel draws as
        // two crossed flat quads tinted orange-red so the player
        // can see lava + flint+steel ignites. The dedicated-render
        // flag was set in the block table but no draw call existed
        // before W8 (the user-reported bug). A future wave swaps
        // the colour for the animated flame texture; for now the
        // placeholder is enough to surface the fire visually.
        if !fires.is_empty() {
            let cube = player_cube;
            let time = day_time;
            for p in fires.keys() {
                let cx = p[0] as f32 + 0.5;
                let cy = p[1] as f32 + 0.2;
                let cz = p[2] as f32 + 0.5;
                // Mild per-cell phase so neighbouring fires don't
                // strobe in lockstep.
                let phase = ((p[0] as u32).wrapping_mul(0x9E37_79B1))
                    ^ ((p[1] as u32).wrapping_mul(0x85EB_CA6B))
                    ^ ((p[2] as u32).wrapping_mul(0xC2B2_AE35));
                let phase_f =
                    (phase as f32 / u32::MAX as f32) * std::f32::consts::TAU;
                let flicker = (time * 6.0 + phase_f).sin();
                // W153: the REAL fire_0 texture when baked - flicker
                // pulses brightness on top of the MC art.
                if let Some(mesh) = effects.fire {
                    let pulse = 0.9 + 0.1 * flicker;
                    let m = Mat4::from_translation(Vec3::new(
                        cx,
                        p[1] as f32,
                        cz,
                    ));
                    renderer.draw_mesh_pbr(
                        mesh,
                        m.to_cols_array(),
                        [pulse, pulse, pulse, 1.0],
                        0.0,
                        0.9,
                    );
                    continue;
                }
                let intensity = (0.85 + 0.15 * flicker).clamp(0.5, 1.05);
                let orange = [
                    1.0 * intensity,
                    0.55 * intensity,
                    0.15 * intensity,
                    0.92,
                ];
                // Crossed pair of thin panels so the flame reads
                // from any angle. Matches the portal pattern; a
                // future wave swaps these for two billboard sprites
                // facing the camera + the animated MC flame.
                let xy_panel = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.9, 0.9, 0.06),
                    glam::Quat::IDENTITY,
                    Vec3::new(cx, cy, cz),
                );
                let zy_panel = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.06, 0.9, 0.9),
                    glam::Quat::IDENTITY,
                    Vec3::new(cx, cy, cz),
                );
                renderer.draw_mesh_pbr(
                    cube,
                    xy_panel.to_cols_array(),
                    orange,
                    0.0,
                    0.8,
                );
                renderer.draw_mesh_pbr(
                    cube,
                    zy_panel.to_cols_array(),
                    orange,
                    0.0,
                    0.8,
                );
                // Add a small point light so the fire glows nearby
                // surfaces - matches lava + torch behaviour.
                renderer.queue_point_light(
                    [cx, cy + 0.2, cz],
                    [1.0, 0.55, 0.15],
                    1.6 + 0.4 * flicker,
                    6.0,
                );
            }
        }
        // Liquids: one slope-aware mesh per kind (water + lava +
        // future). Each draws with its own albedo from the
        // matching `BlockType` row, so adding a new liquid means
        // a single table flip rather than another draw call here.
        // W111: per-chunk liquid meshes - flatten every chunk's Vec of
        // per-kind meshes into a single draw pass.
        for (liquid_id, handle) in liquid_mesh_handles.values().flatten() {
            let Some(bt) = block_type(*liquid_id) else {
                continue;
            };
            // W108: water is translucent again (alpha 0.65). It now routes
            // through the alpha-blend transparent pipeline (cull_mode=None)
            // and is drawn AFTER the opaque chunk terrain (see render()
            // ordering), so it blends over the terrain instead of the cleared
            // sky - the two conditions whose absence caused the old W34
            // "invisible from above / weird from below" artefacts. Lava stays
            // fully opaque (MC parity).
            // Water look knobs (the user iterates on these): `alpha` =
            // surface opacity (higher = less see-through), `rough` =
            // matte<->reflective (lower = more sky reflection).
            // W179 0.50/0.18(mirror) -> 0.70(too flat); W188 0.50->0.70
            // opacity; W194: alpha 0.70 -> 0.80 (still too see-through
            // from above) and rough 0.70 -> 0.55 (the flat matte read as
            // no reflection at all; 0.55 gives a soft sky reflection
            // without the old mirror). A flat alpha is the simple lever
            // until depth-based water fog lands.
            let (alpha, rough) = match *liquid_id {
                WATER_ID => (0.80_f32, 0.55_f32),
                LAVA_ID => (1.0, 0.55),
                _ => (1.0, 0.30),
            };
            // W200: liquid meshes carry real atlas UVs + anim slots for
            // MC's water_still / lava_still textures (bound to the block
            // atlas at mesh build). The draw tint follows MC's rules:
            // water_still is a GRAYSCALE texture tinted by the biome
            // water colour -> tint with the BLOCK_TYPES water blue;
            // lava_still is a fully COLOURED texture -> tint white (the
            // orange registry colour would double-saturate it).
            // draw_mesh_water routes through the material.w >= 0.5
            // marker, which now only drives the depth-based alpha
            // (W199) - sampling is the same animated-tile path as
            // chunk meshes.
            let tint = if *liquid_id == LAVA_ID {
                [1.0, 1.0, 1.0, alpha]
            } else {
                [bt.color[0], bt.color[1], bt.color[2], alpha]
            };
            if *liquid_id == WATER_ID || *liquid_id == LAVA_ID {
                renderer.draw_mesh_water(
                    *handle,
                    Mat4::IDENTITY.to_cols_array(),
                    tint,
                    0.0,
                    rough,
                );
            } else {
                renderer.draw_mesh_pbr(
                    *handle,
                    Mat4::IDENTITY.to_cols_array(),
                    tint,
                    0.0,
                    rough,
                );
            }
        }
}
