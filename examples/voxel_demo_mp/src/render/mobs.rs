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
    KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_POLAR_BEAR, KIND_RABBIT, KIND_SHEEP, KIND_SKELETON,
    KIND_WOLF, KIND_ZOMBIE,
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
    // W147: per-mob walk-animation state (advanced here every frame)
    // + the frame delta driving it.
    mob_anim: &mut HashMap<u32, crate::entity_model::animation::MobAnim>,
    dt: f32,
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

                // W147: advance the MC walk animation from observed
                // motion (works for the host sim and 10 Hz remote
                // snapshots alike) + unpack the head-look pose. One
                // universal closure serves every kind: bone-name
                // suffixes route legs/arms/head, spider legs get
                // their dedicated sway.
                let (limb_pos, limb_amount) = {
                    let a = mob_anim.entry(mob.id).or_default();
                    crate::entity_model::animation::advance(a, mob.pos, dt);
                    (a.limb_pos, a.limb_amount)
                };
                let (look_pitch, look_delta) =
                    crate::entity_model::animation::head_pose(
                        mob.head_pitch,
                        mob.head_yaw,
                        mob.yaw,
                    );
                let mob_pose = move |name: &str| -> crate::entity_model::BonePose {
                    if name.starts_with("spider_leg") {
                        crate::entity_model::animation::spider_leg_pose(
                            name, limb_pos, limb_amount,
                        )
                    } else {
                        crate::entity_model::animation::pose_for_bone(
                            name, limb_pos, limb_amount, look_pitch, look_delta,
                        )
                    }
                };

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
                        if let Some(baked) = &mob_meshes.sheep_model {
                            // W92 - universal MC SheepModel (the skin
                            // layer). Same pipeline + yaw offset.
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
                            // W139 - the SheepFurModel wool overlay at
                            // the same pose (vanilla layers the fur
                            // over the skin; dye tint is a follow-up -
                            // natural sheep render white).
                            if let Some(fur) = &mob_meshes.sheep_fur_model {
                                crate::entity_model::draw(
                                    renderer,
                                    fur,
                                    pos,
                                    model_yaw,
                                    apply_tint(white),
                                    mob_pose,
                                );
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
                        if let Some(baked) = &mob_meshes.zombie_model {
                            // W92 - universal MC ZombieModel. Arms raised
                            // forward (xRot = -PI/2) for the classic zombie
                            // pose, the rest at the MC rest pose.
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            // W147: the universal pose closure raises
                            // the zombie arms (animateZombieArms) and
                            // swings the legs.
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
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
                        if let Some(baked) = &mob_meshes.chicken_model {
                            // W92 - universal MC ChickenModel (beak, wattle,
                            // wings). Same pipeline + yaw offset as the rest.
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
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

                    // ---- Wolf (W158) ------------------------------------
                    KIND_WOLF => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        let grey = [0.75f32, 0.73, 0.70, 1.0];
                        if let Some(baked) = &mob_meshes.wolf_model {
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
                        } else {
                            // Fallback: grey 2-cube silhouette.
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.9, 0.5, 0.4), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.35 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(grey), 0.0, 0.9);
                            let head = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.35, 0.35, 0.35), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.65 - death_sink + walk_bob, base_z) + fwd * 0.5);
                            renderer.draw_mesh_pbr(player_cube, head.to_cols_array(), apply_tint(grey), 0.0, 0.9);
                        }
                    }

                    // ---- Polar bear (W161) ------------------------------
                    KIND_POLAR_BEAR => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        if let Some(baked) = &mob_meshes.polar_bear_model {
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
                        } else {
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(1.0, 0.9, 1.4), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.6 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(white), 0.0, 0.95);
                        }
                    }

                    // ---- Rabbit (W162) ----------------------------------
                    KIND_RABBIT => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        if let Some(baked) = &mob_meshes.rabbit_model {
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
                        } else {
                            // Tiny fallback cube (rabbits are small).
                            let body = Mat4::from_scale_rotation_translation(
                                Vec3::new(0.45, 0.5, 0.7), body_rot,
                                Vec3::new(base_x, mob.pos[1] + 0.25 - death_sink + walk_bob, base_z));
                            renderer.draw_mesh_pbr(player_cube, body.to_cols_array(), apply_tint(white), 0.0, 0.95);
                        }
                    }

                    // ---- Pig --------------------------------------------
                    KIND_PIG => {
                        let white = [1.0f32, 1.0, 1.0, 1.0];
                        let pink = [0.95f32, 0.65, 0.65, 1.0];
                        if let Some(baked) = &mob_meshes.pig_model {
                            // W92 - universal MC PigModel (snout + body xRot),
                            // same pipeline as the cow. MC faces -Z; engine
                            // forward is +X, hence the -PI/2 yaw offset.
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
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
                        if let Some(baked) = &mob_meshes.cow_model {
                            // W91 - universal MC `CowModel`: faithful boxes
                            // (incl. the two head horns), body `xRot=PI/2`,
                            // and MC->engine flip+1/16, all from model data
                            // (no per-part magic transforms). MC models look
                            // along -Z; the engine's yaw points forward at
                            // +X, so offset the model yaw by -PI/2 to align.
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
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
                        if let Some(baked) = &mob_meshes.skeleton_model {
                            // W92 - universal MC SkeletonModel (thin limbs,
                            // mirrored left side), arms at the MC rest pose.
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
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
                        if let Some(baked) = &mob_meshes.spider_model {
                            // W92 - universal MC SpiderModel: 2 body segments
                            // + 8 legs with their rest-pose splay baked into
                            // the bone rotations. Same pipeline + yaw offset.
                            let pos = Vec3::new(
                                base_x,
                                mob.pos[1] - death_sink,
                                base_z,
                            );
                            let model_yaw = mob.yaw - std::f32::consts::FRAC_PI_2;
                            crate::entity_model::draw(
                                renderer,
                                baked,
                                pos,
                                model_yaw,
                                apply_tint(white),
                                mob_pose,
                            );
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
