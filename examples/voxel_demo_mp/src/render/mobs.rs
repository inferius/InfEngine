//! W47.3 - mob rendering, extracted from main.rs.
//!
//! `draw_remote_mobs` walks the App's `remote_mobs` map and dispatches
//! per-mob-kind to the right MobMeshes parts (zombie 6 cubes / chicken
//! 4 / pig 6 / cow 6 / skeleton 6 / spider 4) or to the textured sheep
//! body+head, falling back to plain `player_cube` cubes when an entity
//! texture wasn't found at pack-load time.

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use infengine_render::{MeshHandle, Renderer};

use crate::frustum::Frustum;
use crate::mob::kinds::{
    KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_SHEEP, KIND_SKELETON, KIND_ZOMBIE,
};
use crate::mob::state::{MOB_DEATH_FADE, MOB_LUNGE_TIME};
use crate::mob_meshes::MobMeshes;
use crate::net::MobMsg;

#[allow(clippy::too_many_arguments)]
pub fn draw_remote_mobs(
    renderer: &mut Renderer,
    player_cube: MeshHandle,
    mob_frustum: &Frustum,
    remote_mobs: &HashMap<u32, MobMsg>,
    sheep_mesh: Option<MeshHandle>,
    sheep_head_mesh: Option<MeshHandle>,
    mob_meshes: &MobMeshes,
    mob_hurt_flash: &HashMap<u32, f32>,
    mob_dying: &HashMap<u32, f32>,
    mob_lunge: &HashMap<u32, f32>,
    day_time: f32,
) {
        if !remote_mobs.is_empty() {
            // Body uses the dedicated textured sheep mesh when
            // available; head always uses the plain cube mesh
            // tinted skin-tone so the two parts can have different
            // albedos without needing two separate textured
            // meshes (the MC sheep model has a separate head
            // material too).
            let body_mesh = sheep_mesh.unwrap_or(player_cube);
            let body_tint: [f32; 4] = if sheep_mesh.is_some() {
                // White tint = let the wool texture show through
                // unmodulated.
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.92, 0.92, 0.92, 1.0]
            };
            // Head: prefer the textured `sheep_head_mesh` (cropped
            // face region of the MC entity texture) so the front
            // shows a real sheep face. Falls back to the
            // untextured player_cube with a skin tint when the
            // crop wasn't loaded.
            let head_mesh = sheep_head_mesh.unwrap_or(player_cube);
            let head_color: [f32; 4] = if sheep_head_mesh.is_some() {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.92, 0.86, 0.78, 1.0]
            };
            let mobs_snapshot: Vec<MobMsg> =
                remote_mobs.values().copied().collect();
            for mob in mobs_snapshot {
                let hurt_flash = mob_hurt_flash
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0f32)
                    .clamp(0.0, 1.0);
                // Death-fade progress in `[0, 1]`. Zero = alive.
                let dying_t = mob_dying
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0f32);
                let death_progress = if dying_t > 0.0 {
                    ((MOB_DEATH_FADE - dying_t) / MOB_DEATH_FADE).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let body_brightness = 1.0 - 0.55 * death_progress;
                let tilt = glam::Quat::from_rotation_z(
                    -std::f32::consts::FRAC_PI_2 * death_progress,
                );
                let death_sink = 0.18 * death_progress;
                // W19.3: per-kind dispatch. Match on mob.kind so adding
                // a new kind requires only one new arm, not a chain of
                // is_X booleans. Each arm either draws multi-part textured
                // meshes (when the entity texture was loaded) or falls back
                // to the two-cube silhouette (tinted body + head cube).
                //
                // Common animation state computed once above the match:
                let body_rot = glam::Quat::from_rotation_y(mob.yaw) * tilt;
                let lunge_t: f32 = mob_lunge
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, MOB_LUNGE_TIME);
                let lunge_phase =
                    (1.0 - lunge_t / MOB_LUNGE_TIME).clamp(0.0, 1.0);
                let lunge_amp = 0.35
                    * (lunge_phase * std::f32::consts::PI).sin().max(0.0);
                let lunge_fwd =
                    Vec3::new(mob.yaw.cos(), 0.0, -mob.yaw.sin()) * lunge_amp;
                let walk_bob = if death_progress <= 0.0 {
                    let phase = mob.id.wrapping_mul(2654435761) as f32
                        / u32::MAX as f32
                        * std::f32::consts::TAU;
                    (day_time * 4.0 + phase).sin() * 0.04
                } else {
                    0.0
                };
                let fwd = Vec3::new(mob.yaw.cos(), 0.0, -mob.yaw.sin());

                // Shorthand: build a transform at `pos` offset by
                // `(fx, fy, fz)` in mob-local space and draw a mesh.
                // `tint_base` is modulated by hurt_flash and brightness.
                let apply_tint =
                    |base: [f32; 4]| -> [f32; 4] {
                        [
                            (base[0] * (1.0 - hurt_flash) + 1.0 * hurt_flash)
                                * body_brightness,
                            (base[1] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                                * body_brightness,
                            (base[2] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                                * body_brightness,
                            base[3],
                        ]
                    };

                let base_x = mob.pos[0] + lunge_fwd.x;
                let base_z = mob.pos[2] + lunge_fwd.z;

                // W28.11: frustum cull mobs outside the view cone.
                if !mob_frustum.contains_point(Vec3::new(base_x, mob.pos[1], base_z)) {
                    continue;
                }

                match mob.kind {
                    // ---- Sheep ------------------------------------------
                    // W55.3: full MC QuadrupedModel (body + head + 4
                    // legs from the entity texture). Falls back to the
                    // legacy wool-cube + head-cube path when the entity
                    // texture isn't on disk.
                    KIND_SHEEP => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        if mob_meshes.sheep_loaded() {
                            let rot = body_rot;
                            // W60.2: SheepModel body 8x16x6 with
                            // xRot=PI/2. Final dims: length 1.0 (X
                            // = MC h=16), height 0.375 (Y = MC d=6),
                            // width 0.5 (Z = MC w=8). Body centre
                            // y=0.9375 (on 12-tall legs).
                            if let Some(m) = mob_meshes.sheep_body {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(1.0, 0.375, 0.5), rot,
                                    Vec3::new(base_x, mob.pos[1] + 0.9375 - death_sink + walk_bob, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(body_tint), 0.0, 0.9);
                            }
                            // W61.1: SheepHead 6x6x8 (MC w x h x d).
                            // Head depth (d=8) along X axis (forward
                            // direction), width (w=6) along Z, height
                            // (h=6) along Y. Mesh +X "front" face is
                            // the head's nose end so head looks the
                            // way the mob is walking.
                            // Y placement: body top sits at
                            // 0.9375 + 0.1875 = 1.125 voxel; head
                            // centre at ~1.30 so the 0.375-tall head
                            // sits on top with a bit of overlap into
                            // the body (matches MC reference where
                            // the head meets the front of the body).
                            if let Some(m) = mob_meshes.sheep_head {
                                let hp = Vec3::new(base_x, mob.pos[1] + 1.30 - death_sink + walk_bob, base_z) + fwd * 0.55;
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.5, 0.375, 0.375),
                                    glam::Quat::from_rotation_y(mob.yaw) * tilt,
                                    hp);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(head_color), 0.0, 0.9);
                            }
                            // Legs 4x12x4 -> 0.25 x 0.75 x 0.25,
                            // centre at y=0.375.
                            let side = Vec3::new(-mob.yaw.sin(), 0.0, -mob.yaw.cos());
                            let leg_scale = Vec3::new(0.25, 0.75, 0.25);
                            let leg_y = mob.pos[1] + 0.375 - death_sink + walk_bob;
                            for (mesh_opt, dx, dz) in [
                                (mob_meshes.sheep_right_front_leg,  0.15_f32, -0.40_f32),
                                (mob_meshes.sheep_left_front_leg,  -0.15,     -0.40),
                                (mob_meshes.sheep_right_back_leg,   0.15,      0.40),
                                (mob_meshes.sheep_left_back_leg,   -0.15,      0.40),
                            ] {
                                if let Some(m) = mesh_opt {
                                    let off = side * dx + fwd * (-dz);
                                    let t = Mat4::from_scale_rotation_translation(
                                        leg_scale, rot,
                                        Vec3::new(base_x, leg_y, base_z) + off);
                                    renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                                }
                            }
                        } else {
                            // Legacy fallback.
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(1.2, 0.8, 0.7),
                                body_rot,
                                Vec3::new(
                                    base_x,
                                    mob.pos[1] + 0.4 - death_sink + walk_bob,
                                    base_z,
                                ),
                            );
                            renderer.draw_mesh_pbr(
                                body_mesh,
                                body.to_cols_array(),
                                apply_tint(body_tint),
                                0.0,
                                0.9,
                            );
                            let head_pos = Vec3::new(base_x, mob.pos[1] + 0.8 - death_sink + walk_bob, base_z)
                                + fwd * 0.45;
                            let head = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.60, 0.45, 0.45),
                                glam::Quat::from_rotation_y(mob.yaw) * tilt,
                                head_pos,
                            );
                            renderer.draw_mesh_pbr(
                                head_mesh,
                                head.to_cols_array(),
                                apply_tint(head_color),
                                0.0,
                                0.9,
                            );
                        }
                    }

                    // ---- Zombie -----------------------------------------
                    KIND_ZOMBIE => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        let green = [0.30f32, 0.55, 0.30, 1.0];
                        let head_green = [0.22f32, 0.42, 0.22, 1.0];
                        if mob_meshes.zombie_loaded() {
                            // Textured humanoid: body centre at y=0.78,
                            // head at y=1.85, arms and legs flanking.
                            let rot = body_rot;
                            let by = mob.pos[1] + 0.78 - death_sink + walk_bob;
                            // body
                            if let Some(m) = mob_meshes.zombie_body {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.50, 0.75, 0.25), rot,
                                    Vec3::new(base_x, by, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // head
                            if let Some(m) = mob_meshes.zombie_head {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.50, 0.50, 0.50), rot,
                                    Vec3::new(base_x, mob.pos[1] + 1.60 - death_sink + walk_bob, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // right arm (offset to mob's right = -side)
                            let side = Vec3::new(-mob.yaw.sin(), 0.0, -mob.yaw.cos());
                            if let Some(m) = mob_meshes.zombie_right_arm {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.25, 0.75, 0.25), rot,
                                    Vec3::new(base_x, by, base_z) + side * 0.38);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // left arm
                            if let Some(m) = mob_meshes.zombie_left_arm {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.25, 0.75, 0.25), rot,
                                    Vec3::new(base_x, by, base_z) - side * 0.38);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // legs
                            let leg_y = mob.pos[1] + 0.0 - death_sink + walk_bob;
                            if let Some(m) = mob_meshes.zombie_right_leg {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.25, 0.75, 0.25), rot,
                                    Vec3::new(base_x, leg_y, base_z) + side * 0.12);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            if let Some(m) = mob_meshes.zombie_left_leg {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.25, 0.75, 0.25), rot,
                                    Vec3::new(base_x, leg_y, base_z) - side * 0.12);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                        } else {
                            // Fallback: tinted 2-cube silhouette.
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.65, 1.55, 0.45), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.78 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(green), 0.0, 0.9);
                            let head = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.55, 0.55, 0.55), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 1.85 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, head.to_cols_array(), apply_tint(head_green), 0.0, 0.9);
                        }
                    }

                    // ---- Chicken ----------------------------------------
                    KIND_CHICKEN => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        let feather = [0.95f32, 0.95, 0.9, 1.0];
                        let comb = [0.95f32, 0.35, 0.30, 1.0];
                        if mob_meshes.chicken_loaded() {
                            let rot = body_rot;
                            // Body centre at y=0.28
                            if let Some(m) = mob_meshes.chicken_body {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.37, 0.50, 0.37), rot,
                                    Vec3::new(base_x, mob.pos[1] + 0.28 - death_sink + walk_bob, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // Head forward-poked
                            if let Some(m) = mob_meshes.chicken_head {
                                let hp = Vec3::new(base_x, mob.pos[1] + 0.65 - death_sink + walk_bob, base_z) + fwd * 0.25;
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.25, 0.25, 0.25), rot, hp);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            let side = Vec3::new(-mob.yaw.sin(), 0.0, -mob.yaw.cos());
                            let leg_y = mob.pos[1] - 0.12 - death_sink + walk_bob;
                            if let Some(m) = mob_meshes.chicken_right_leg {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.18, 0.30, 0.18), rot,
                                    Vec3::new(base_x, leg_y, base_z) + side * 0.10);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            if let Some(m) = mob_meshes.chicken_left_leg {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.18, 0.30, 0.18), rot,
                                    Vec3::new(base_x, leg_y, base_z) - side * 0.10);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                        } else {
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.45, 0.55, 0.4), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.28 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(feather), 0.0, 0.9);
                            let head = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.3, 0.3, 0.3), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.65 - death_sink + walk_bob, base_z) + fwd * 0.25);
                            renderer.draw_mesh_pbr(player_cube, head.to_cols_array(), apply_tint(comb), 0.0, 0.9);
                        }
                    }

                    // ---- Pig --------------------------------------------
                    KIND_PIG => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        let pink = [0.95f32, 0.65, 0.65, 1.0];
                        if mob_meshes.pig_loaded() {
                            let rot = body_rot;
                            // W60.2: PigModel body 10x16x8 with
                            // xRot=PI/2. Length 1.0 (X = MC h=16),
                            // height 0.5 (Y = MC d=8), width 0.625
                            // (Z = MC w=10). Body centre y=0.625
                            // (on 6-tall legs).
                            if let Some(m) = mob_meshes.pig_body {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(1.0, 0.5, 0.625), rot,
                                    Vec3::new(base_x, mob.pos[1] + 0.625 - death_sink + walk_bob, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // PigHead 8x8x8 cube.
                            if let Some(m) = mob_meshes.pig_head {
                                let hp = Vec3::new(base_x, mob.pos[1] + 0.625 - death_sink + walk_bob, base_z) + fwd * 0.5;
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.5, 0.5, 0.5),
                                    glam::Quat::from_rotation_y(mob.yaw) * tilt,
                                    hp);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // Legs 4x6x4 -> 0.25 x 0.375 x 0.25,
                            // centre at y=0.1875.
                            let side = Vec3::new(-mob.yaw.sin(), 0.0, -mob.yaw.cos());
                            let leg_scale = Vec3::new(0.25, 0.375, 0.25);
                            let leg_y = mob.pos[1] + 0.1875 - death_sink + walk_bob;
                            for (mesh_opt, dx, dz) in [
                                (mob_meshes.pig_right_front_leg,  0.19_f32, -0.35_f32),
                                (mob_meshes.pig_left_front_leg,  -0.19,     -0.35),
                                (mob_meshes.pig_right_back_leg,   0.19,      0.35),
                                (mob_meshes.pig_left_back_leg,   -0.19,      0.35),
                            ] {
                                if let Some(m) = mesh_opt {
                                    let off = side * dx + fwd * (-dz);
                                    let t = Mat4::from_scale_rotation_translation(
                                        leg_scale, rot,
                                        Vec3::new(base_x, leg_y, base_z) + off);
                                    renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                                }
                            }
                        } else {
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(1.1, 0.75, 0.6), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.35 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(pink), 0.0, 0.9);
                            let head = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.6, 0.55, 0.55), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.65 - death_sink + walk_bob, base_z) + fwd * 0.40);
                            renderer.draw_mesh_pbr(player_cube, head.to_cols_array(), apply_tint(pink), 0.0, 0.9);
                        }
                    }

                    // ---- Cow --------------------------------------------
                    _ if mob.kind == KIND_COW => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        let brown = [0.55f32, 0.35, 0.20, 1.0];
                        if mob_meshes.cow_loaded() {
                            let rot = body_rot;
                            // W60.2: MC CowModel body 12x18x10 with
                            // xRot=PI/2 lays the 18-axis horizontal.
                            // Our engine's forward is +X, so body
                            // length runs along X (1.125), height
                            // along Y (0.625 = MC depth 10 post-rot),
                            // width along Z (0.75 = MC width 12).
                            // build_mob_part_mesh puts the "front"
                            // tile on the +X face, so the head end
                            // ends up at +X = forward direction.
                            if let Some(m) = mob_meshes.cow_body {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(1.125, 0.625, 0.75), rot,
                                    Vec3::new(base_x, mob.pos[1] + 1.06 - death_sink + walk_bob, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // W61.1: CowHead 8x8x6 (w h d). Depth
                            // (d=6) along X for forward direction;
                            // height (h=8) along Y; width (w=8)
                            // along Z. Centre y ~= 1.25 (top of
                            // body + half head height).
                            if let Some(m) = mob_meshes.cow_head {
                                let hp = Vec3::new(base_x, mob.pos[1] + 1.25 - death_sink + walk_bob, base_z) + fwd * 0.6;
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.375, 0.5, 0.5),
                                    glam::Quat::from_rotation_y(mob.yaw) * tilt,
                                    hp);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // 4 legs 4x12x4 -> 0.25 x 0.75 x 0.25,
                            // bottoms on ground, centre at y=0.375.
                            let side = Vec3::new(-mob.yaw.sin(), 0.0, -mob.yaw.cos());
                            let leg_scale = Vec3::new(0.25, 0.75, 0.25);
                            let leg_y = mob.pos[1] + 0.375 - death_sink + walk_bob;
                            for (mesh_opt, dx, dz) in [
                                (mob_meshes.cow_right_front_leg,  0.20_f32, -0.40_f32),
                                (mob_meshes.cow_left_front_leg,  -0.20,     -0.40),
                                (mob_meshes.cow_right_back_leg,   0.20,      0.40),
                                (mob_meshes.cow_left_back_leg,   -0.20,      0.40),
                            ] {
                                if let Some(m) = mesh_opt {
                                    let off = side * dx + fwd * (-dz);
                                    let t = Mat4::from_scale_rotation_translation(
                                        leg_scale, rot,
                                        Vec3::new(base_x, leg_y, base_z) + off);
                                    renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                                }
                            }
                        } else {
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(1.2, 0.9, 0.7), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.40 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(brown), 0.0, 0.9);
                            let head = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.55, 0.50, 0.40), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.80 - death_sink + walk_bob, base_z) + fwd * 0.45);
                            renderer.draw_mesh_pbr(player_cube, head.to_cols_array(), apply_tint(brown), 0.0, 0.9);
                        }
                    }

                    // ---- Skeleton ---------------------------------------
                    _ if mob.kind == KIND_SKELETON => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        let bone = [0.88f32, 0.87, 0.82, 1.0];
                        let bone_head = [0.80f32, 0.79, 0.74, 1.0];
                        if mob_meshes.skeleton_loaded() {
                            let rot = body_rot;
                            let by = mob.pos[1] + 0.78 - death_sink + walk_bob;
                            if let Some(m) = mob_meshes.skeleton_body {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.50, 0.75, 0.25), rot,
                                    Vec3::new(base_x, by, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            if let Some(m) = mob_meshes.skeleton_head {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.50, 0.50, 0.50), rot,
                                    Vec3::new(base_x, mob.pos[1] + 1.60 - death_sink + walk_bob, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            let side = Vec3::new(-mob.yaw.sin(), 0.0, -mob.yaw.cos());
                            // Arms: thinner than zombie (2x12x2 MC units)
                            if let Some(m) = mob_meshes.skeleton_right_arm {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.12, 0.75, 0.12), rot,
                                    Vec3::new(base_x, by, base_z) + side * 0.31);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            if let Some(m) = mob_meshes.skeleton_left_arm {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.12, 0.75, 0.12), rot,
                                    Vec3::new(base_x, by, base_z) - side * 0.31);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            let leg_y = mob.pos[1] + 0.0 - death_sink + walk_bob;
                            if let Some(m) = mob_meshes.skeleton_right_leg {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.12, 0.75, 0.12), rot,
                                    Vec3::new(base_x, leg_y, base_z) + side * 0.12);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            if let Some(m) = mob_meshes.skeleton_left_leg {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.12, 0.75, 0.12), rot,
                                    Vec3::new(base_x, leg_y, base_z) - side * 0.12);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                        } else {
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.45, 1.55, 0.30), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.78 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(bone), 0.0, 0.9);
                            let head = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.50, 0.50, 0.50), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 1.85 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, head.to_cols_array(), apply_tint(bone_head), 0.0, 0.9);
                        }
                    }

                    // ---- Spider -----------------------------------------
                    // Spider has 2 body segments + 1 head + 8 legs (all
                    // sharing the same leg UV tile). The head matches the
                    // mob's yaw; all other parts track the body orientation.
                    _ => {
                        // Spider (KIND_SPIDER) OR any future unknown kind
                        // that falls through here gets the spider/cube path.
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        let dark = [0.18f32, 0.12, 0.12, 1.0];
                        if mob_meshes.spider_loaded() {
                            let rot = body_rot;
                            let cy = mob.pos[1] + 0.10 - death_sink + walk_bob;
                            // Thorax (body0)
                            if let Some(m) = mob_meshes.spider_body0 {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.37, 0.37, 0.37), rot,
                                    Vec3::new(base_x, cy, base_z));
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // Abdomen (body1) - behind the thorax
                            if let Some(m) = mob_meshes.spider_body1 {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.62, 0.50, 0.75), rot,
                                    Vec3::new(base_x, cy, base_z) - fwd * 0.55);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // Head - forward
                            if let Some(m) = mob_meshes.spider_head {
                                let t = Mat4::from_scale_rotation_translation(
                                    Vec3::new(0.50, 0.50, 0.50), rot,
                                    Vec3::new(base_x, cy + 0.05, base_z) + fwd * 0.35);
                                renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                            }
                            // 8 legs as pairs: each leg is a thin elongated
                            // box extending sideways from the thorax.
                            if let Some(m) = mob_meshes.spider_leg {
                                let side = Vec3::new(-mob.yaw.sin(), 0.0, -mob.yaw.cos());
                                let leg_offsets: [(f32, f32, f32); 4] = [
                                    // (side_dist, forward, y_raise)
                                    (0.55, -0.20, 0.0),
                                    (0.55,  0.0,  0.0),
                                    (0.55,  0.20, 0.0),
                                    (0.55,  0.40, 0.0),
                                ];
                                for (sd, fwd_off, yr) in leg_offsets {
                                    let leg_scale = Vec3::new(1.0, 0.12, 0.12);
                                    // Right leg
                                    let t = Mat4::from_scale_rotation_translation(
                                        leg_scale, rot,
                                        Vec3::new(base_x, cy + yr, base_z)
                                            + side * sd + fwd * fwd_off);
                                    renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                                    // Left leg (mirror)
                                    let t = Mat4::from_scale_rotation_translation(
                                        leg_scale, rot,
                                        Vec3::new(base_x, cy + yr, base_z)
                                            - side * sd + fwd * fwd_off);
                                    renderer.draw_mesh_pbr(m, t.to_cols_array(), apply_tint(white), 0.0, 0.9);
                                }
                            }
                        } else {
                            // Fallback: flat dark elongated cube
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(1.4, 0.45, 0.65), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.10 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(dark), 0.0, 0.9);
                            let head = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.50, 0.50, 0.50), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.35 - death_sink + walk_bob, base_z) + fwd * 0.50);
                            renderer.draw_mesh_pbr(player_cube, head.to_cols_array(), apply_tint(dark), 0.0, 0.9);
                        }
                    }
                } // end match mob.kind
            }
        }
}
