//! W147 - mob animation (the MC walk + head-look port).
//!
//! Two halves:
//! - `MobAnim` + `advance`: the per-mob walk-animation state MC keeps
//!   in `WalkAnimationState` (`position` accumulates while moving,
//!   `speed` eases toward `min(1, dist_per_tick * 4)` at 0.4/tick).
//!   Derived client-side from observed positions so it works for the
//!   host's own mobs and for 10 Hz remote snapshots alike.
//! - `pose_for_bone`: the `setupAnim` formulas keyed by our bone-name
//!   suffixes (`*_leg_rf`, `*_arm_l`, ...). One universal function
//!   covers quadrupeds (cow/pig/sheep), humanoids (zombie/skeleton),
//!   the chicken and the spider - exactly the per-model math from
//!   `QuadrupedModel` / `HumanoidModel` / `ChickenModel` /
//!   `SpiderModel`, with the zombie's raised arms on top.
//!
//! Sign conventions verified against `bone_matrix` in the unit tests
//! below: pose pitch follows MC (`positive = look DOWN`) and the head
//! pose-yaw is the NEGATED world delta (the MC->engine X/Y flip
//! mirrors the rotation direction).

use super::BonePose;

/// MC walk-animation frequency (`limbSwing * 0.6662`).
const SWING_FREQ: f32 = 0.6662;
/// MC quadruped/humanoid leg amplitude.
const LEG_AMP: f32 = 1.4;
/// MC humanoid arm amplitude (2.0 * 0.5).
const ARM_AMP: f32 = 1.0;
/// MC `LookAtPlayerGoal` head turn clamp (+-75 deg as radians)
/// exposed for the AI side.
pub(crate) const HEAD_YAW_CLAMP: f32 = 75.0 * std::f32::consts::PI / 180.0;
/// MC look-goal pitch clamp (+-40 deg).
pub(crate) const HEAD_PITCH_CLAMP: f32 = 40.0 * std::f32::consts::PI / 180.0;

/// Per-mob walk animation state, advanced from observed motion.
#[derive(Clone, Copy, Default)]
pub(crate) struct MobAnim {
    /// MC `WalkAnimationState.position` - the accumulated swing phase.
    pub(crate) limb_pos: f32,
    /// MC `WalkAnimationState.speed` - eased 0..1 stride strength.
    pub(crate) limb_amount: f32,
    /// Last observed world position (None right after spawn).
    pub(crate) last_pos: Option<[f32; 3]>,
}

/// Advance the walk state from the newly observed `pos`. Mirrors
/// `LivingEntity.tickAnimations` at our variable frame rate: the
/// per-tick target is `min(1, horizontal_dist * 4)` and the easing
/// constant is 0.4/tick at 20 TPS.
pub(crate) fn advance(anim: &mut MobAnim, pos: [f32; 3], dt: f32) {
    let ticks = (dt * 20.0).clamp(0.0, 4.0);
    if ticks <= 0.0 {
        return;
    }
    let target = match anim.last_pos {
        Some(prev) => {
            let dx = pos[0] - prev[0];
            let dz = pos[2] - prev[2];
            let per_tick = (dx * dx + dz * dz).sqrt() / ticks.max(1e-3);
            (per_tick * 4.0).min(1.0)
        }
        None => 0.0,
    };
    anim.last_pos = Some(pos);
    anim.limb_amount += (target - anim.limb_amount) * (0.4 * ticks).min(1.0);
    anim.limb_pos += anim.limb_amount * ticks;
}

/// Wrap an angle to (-PI, PI].
pub(crate) fn wrap_angle(a: f32) -> f32 {
    let mut a = a.rem_euclid(std::f32::consts::TAU);
    if a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    a
}

/// MC `setupAnim` for one bone, keyed by our `<mob>_<part>` name
/// suffixes. `head_pitch` is MC-style (positive looks down);
/// `head_yaw_delta` is the world-space head-minus-body yaw.
pub(crate) fn pose_for_bone(
    name: &str,
    limb_pos: f32,
    amount: f32,
    head_pitch: f32,
    head_yaw_delta: f32,
) -> BonePose {
    let mut rot = [0.0f32; 3];
    let swing = limb_pos * SWING_FREQ;
    let pi = std::f32::consts::PI;
    if name.ends_with("_head") {
        // The MC->engine flip mirrors yaw: pose y = -(world delta).
        // (`*_fur_head` matches too - the sheep wool follows.)
        rot[0] = head_pitch;
        rot[1] = -head_yaw_delta;
    } else if name.ends_with("_leg_rf") || name.ends_with("_leg_lh") {
        // Quadruped gait: diagonal pairs share a phase, the
        // front-right runs opposite the hind-right.
        rot[0] = (swing + pi).cos() * LEG_AMP * amount;
    } else if name.ends_with("_leg_lf") || name.ends_with("_leg_rh") {
        rot[0] = swing.cos() * LEG_AMP * amount;
    } else if name.ends_with("_leg_r") {
        // Humanoid + chicken right leg.
        rot[0] = swing.cos() * LEG_AMP * amount;
    } else if name.ends_with("_leg_l") {
        rot[0] = (swing + pi).cos() * LEG_AMP * amount;
    } else if name.ends_with("_arm_r") {
        if name.starts_with("zombie") {
            // ZombieModel: both arms raised forward with a slight
            // bob (`AnimationUtils.animateZombieArms`).
            rot[0] = -pi / 2.0 + (swing).cos() * 0.1 * amount;
        } else {
            rot[0] = (swing + pi).cos() * ARM_AMP * amount;
        }
    } else if name.ends_with("_arm_l") {
        if name.starts_with("zombie") {
            rot[0] = -pi / 2.0 + (swing + pi).cos() * 0.1 * amount;
        } else {
            rot[0] = swing.cos() * ARM_AMP * amount;
        }
    }
    BonePose { extra_rotation: rot }
}

/// Spider leg sway (`SpiderModel.setupAnim`): per-pair yaw sweep +
/// abs-sine z lift, mirrored left/right. Our spider bones already
/// carry the splayed default rotation; these add on top.
pub(crate) fn spider_leg_pose(name: &str, limb_pos: f32, amount: f32) -> BonePose {
    let pi = std::f32::consts::PI;
    let swing2 = limb_pos * SWING_FREQ * 2.0;
    let swing = limb_pos * SWING_FREQ;
    // (yaw phase, z phase) per leg pair, vanilla order.
    let (phase_y, phase_z, right) = match name {
        "spider_leg_rh" => (0.0, 0.0, true),
        "spider_leg_lh" => (0.0, 0.0, false),
        "spider_leg_rmh" => (pi, pi, true),
        "spider_leg_lmh" => (pi, pi, false),
        "spider_leg_rmf" => (pi / 2.0, pi / 2.0, true),
        "spider_leg_lmf" => (pi / 2.0, pi / 2.0, false),
        "spider_leg_rf" => (3.0 * pi / 2.0, 3.0 * pi / 2.0, true),
        "spider_leg_lf" => (3.0 * pi / 2.0, 3.0 * pi / 2.0, false),
        _ => return BonePose::default(),
    };
    let sweep = -((swing2 + phase_y).cos() * 0.4) * amount;
    let lift = ((swing + phase_z).sin() * 0.4).abs() * amount;
    let mut rot = [0.0f32; 3];
    rot[1] = if right { sweep } else { -sweep };
    rot[2] = if right { lift } else { -lift };
    BonePose { extra_rotation: rot }
}

/// Head pose helper for the render layer: takes the broadcast
/// absolute head yaw + body yaw, returns the bone pose.
pub(crate) fn head_pose(head_pitch: f32, head_yaw: f32, body_yaw: f32) -> (f32, f32) {
    (head_pitch, wrap_angle(head_yaw - body_yaw))
}

/// Smoothly steer `current` toward `target` (angles), at most
/// `max_step` radians. MC's look control moves ~10 deg/tick.
pub(crate) fn approach_angle(current: f32, target: f32, max_step: f32) -> f32 {
    let d = wrap_angle(target - current);
    current + d.clamp(-max_step, max_step)
}

/// Direction the head bone's MC front (-Z) points after the pose -
/// test helper shared by the sign tests below and reusable for
/// future look-at debugging.
#[cfg(test)]
fn head_world_front(body_yaw: f32, pose: BonePose) -> glam::Vec3 {
    use super::{Bone, bone_matrix};
    use glam::Vec3;
    let bone = Bone {
        name: "test_head",
        pivot: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0],
        cubes: &[],
    };
    let model_yaw = body_yaw - std::f32::consts::FRAC_PI_2;
    let m = bone_matrix(Vec3::ZERO, model_yaw, 0.0, &bone, pose);
    let p0 = m.transform_point3(Vec3::ZERO);
    let p1 = m.transform_point3(Vec3::new(0.0, 0.0, -1.0));
    (p1 - p0).normalize()
}

#[cfg(test)]
mod model_coverage {
    //! W158: model-vs-texture coverage audit. For every entity
    //! model, every cube's box-unwrap region must be mostly OPAQUE
    //! in the real pack texture - wrong `texOffs` land in empty
    //! sheet space and fail loudly here instead of rendering
    //! garbage in game. Skips silently when no pack is on disk.
    use super::super::{Bone, McCube};

    const ENTITY_DIR: &str = r"C:\Users\micha\AppData\Roaming\InfEngine\voxel_demo_mp\data\cache\mc_assets\26.1.2\assets\minecraft\textures\entity";

    /// Opaque fraction of one cube's full unwrap (both strips).
    fn cube_opacity(img: &image::RgbaImage, c: &McCube) -> f32 {
        let (w, h, d) = (c.size[0], c.size[1], c.size[2]);
        let (u0, v0) = (c.uv[0] as f32, c.uv[1] as f32);
        // Top strip (top + bottom tiles) + side strip (4 tiles).
        let rects = [
            (u0 + d, v0, u0 + d + 2.0 * w, v0 + d),
            (u0, v0 + d, u0 + 2.0 * d + 2.0 * w, v0 + d + h),
        ];
        let mut opaque = 0u32;
        let mut total = 0u32;
        for (x0, y0, x1, y1) in rects {
            for y in (y0.max(0.0) as u32)..(y1.min(img.height() as f32) as u32) {
                for x in (x0.max(0.0) as u32)..(x1.min(img.width() as f32) as u32) {
                    total += 1;
                    if img.get_pixel(x, y).0[3] >= 128 {
                        opaque += 1;
                    }
                }
            }
        }
        if total == 0 {
            return 0.0;
        }
        opaque as f32 / total as f32
    }

    /// Audit one model against a texture file. `min_opaque` is the
    /// REQUIRED fraction per cube (entity sheets keep cutout gaps,
    /// so 0.35 catches off-by-a-tile errors without flagging the
    /// genuinely holey cubes like chicken legs - those pass via the
    /// explicit allow list).
    fn audit(model: &[Bone], tex_rel: &str, allow_sparse: &[&str]) {
        let path = std::path::Path::new(ENTITY_DIR).join(tex_rel);
        if !path.is_file() {
            return; // no pack on disk - skip silently
        }
        let img = image::open(&path).expect("entity sheet decodes").to_rgba8();
        let mut bad: Vec<String> = Vec::new();
        for bone in model {
            for (i, cube) in bone.cubes.iter().enumerate() {
                let frac = cube_opacity(&img, cube);
                let floor = if allow_sparse.contains(&bone.name) {
                    0.05
                } else {
                    0.35
                };
                if frac < floor {
                    bad.push(format!(
                        "{} cube#{i} uv({},{}) size {:?} opaque {:.0}%",
                        bone.name, cube.uv[0], cube.uv[1], cube.size, frac * 100.0
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "{tex_rel}: cubes off the art: {bad:?}");
    }

    #[test]
    fn w158_every_model_unwrap_lands_on_art() {
        audit(
            super::super::chicken::MODEL.bones,
            "chicken/chicken_temperate.png",
            &["chicken_leg_r", "chicken_leg_l"],
        );
        audit(super::super::pig::MODEL.bones, "pig/pig_temperate.png", &[]);
        audit(super::super::cow::MODEL.bones, "cow/cow_temperate.png", &[]);
        audit(super::super::sheep::MODEL.bones, "sheep/sheep.png", &[]);
        audit(super::super::zombie::MODEL.bones, "zombie/zombie.png", &[]);
        audit(
            super::super::skeleton::MODEL.bones,
            "skeleton/skeleton.png",
            // The skeleton sheet is mostly cutout by design.
            &[
                "skeleton_head",
                "skeleton_body",
                "skeleton_arm_r",
                "skeleton_arm_l",
                "skeleton_leg_r",
                "skeleton_leg_l",
            ],
        );
        audit(super::super::spider::MODEL.bones, "spider/spider.png", &[]);
        audit(super::super::wolf::MODEL.bones, "wolf/wolf.png", &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// Engine yaw of a world direction (matches mob yaw convention).
    fn yaw_of(v: Vec3) -> f32 {
        (-v.z).atan2(v.x)
    }

    /// W147 sign lock: a positive world head-yaw delta must rotate
    /// the rendered head front to `body_yaw + delta`, and a positive
    /// MC pitch must aim it DOWN. Verified through the real
    /// `bone_matrix` so the MC->engine flip is part of the test.
    #[test]
    fn head_pose_signs_match_world_yaw() {
        for body_yaw in [0.0f32, 1.1, -2.3] {
            for delta in [0.5f32, -0.7] {
                let (pitch, d) = head_pose(0.0, body_yaw + delta, body_yaw);
                let pose = pose_for_bone("cow_head", 0.0, 0.0, pitch, d);
                let front = head_world_front(body_yaw, pose);
                let got = wrap_angle(yaw_of(front) - (body_yaw + delta));
                assert!(
                    got.abs() < 1e-4,
                    "body {body_yaw} delta {delta}: head yaw off by {got}"
                );
            }
        }
        // MC pitch positive = down.
        let pose = pose_for_bone("pig_head", 0.0, 0.0, 0.5, 0.0);
        let front = head_world_front(0.0, pose);
        assert!(front.y < -0.3, "positive MC pitch must look down, got {front:?}");
    }

    /// Legs alternate: diagonal pairs share phase (quadruped gait).
    #[test]
    fn quadruped_legs_alternate() {
        let t = 0.6f32;
        let rf = pose_for_bone("cow_leg_rf", t, 1.0, 0.0, 0.0).extra_rotation[0];
        let lf = pose_for_bone("cow_leg_lf", t, 1.0, 0.0, 0.0).extra_rotation[0];
        let rh = pose_for_bone("cow_leg_rh", t, 1.0, 0.0, 0.0).extra_rotation[0];
        let lh = pose_for_bone("cow_leg_lh", t, 1.0, 0.0, 0.0).extra_rotation[0];
        assert!((rf + lf).abs() < 1e-5, "front pair opposite");
        assert!((rh + lh).abs() < 1e-5, "hind pair opposite");
        assert!((rf - lh).abs() < 1e-5, "diagonal pair in phase");
        assert!(rf.abs() > 0.5, "stride has real amplitude");
    }

    /// Standing still = no swing; the eased amount decays to zero.
    #[test]
    fn advance_decays_at_rest() {
        let mut a = MobAnim::default();
        advance(&mut a, [0.0, 64.0, 0.0], 0.05);
        // Walk east at 4 m/s: per-tick dist 0.2 -> target 0.8.
        let mut x = 0.0;
        for _ in 0..40 {
            x += 4.0 * 0.05;
            advance(&mut a, [x, 64.0, 0.0], 0.05);
        }
        assert!(a.limb_amount > 0.6, "walking builds amount, got {}", a.limb_amount);
        let moving_pos = a.limb_pos;
        for _ in 0..60 {
            advance(&mut a, [x, 64.0, 0.0], 0.05);
        }
        assert!(a.limb_amount < 0.05, "rest decays amount, got {}", a.limb_amount);
        assert!(a.limb_pos - moving_pos < 2.0, "phase stops accumulating at rest");
    }

    /// Zombie arms ride raised (MC animateZombieArms), others swing.
    #[test]
    fn zombie_arms_raised_skeleton_arms_swing() {
        let z = pose_for_bone("zombie_arm_r", 0.0, 0.0, 0.0, 0.0).extra_rotation[0];
        assert!((z + std::f32::consts::FRAC_PI_2).abs() < 0.2, "zombie arm ~ -PI/2");
        let s = pose_for_bone("skeleton_arm_r", 0.0, 0.0, 0.0, 0.0).extra_rotation[0];
        assert!(s.abs() < 1e-5, "skeleton arm hangs at rest");
    }

    /// Spider legs sweep mirrored left/right with per-pair phases.
    #[test]
    fn spider_legs_mirror() {
        let r = spider_leg_pose("spider_leg_rmf", 0.9, 1.0).extra_rotation;
        let l = spider_leg_pose("spider_leg_lmf", 0.9, 1.0).extra_rotation;
        assert!((r[1] + l[1]).abs() < 1e-5);
        assert!((r[2] + l[2]).abs() < 1e-5);
    }

    #[test]
    fn approach_angle_clamps_step() {
        let a = approach_angle(0.0, 1.0, 0.2);
        assert!((a - 0.2).abs() < 1e-6);
        // Wraps the short way across PI.
        let b = approach_angle(3.0, -3.0, 10.0);
        assert!((wrap_angle(b - -3.0)).abs() < 1e-4);
    }
}
