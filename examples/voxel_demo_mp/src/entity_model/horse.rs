//! W322 - the horse, ported faithfully from MC
//! `AbstractEquineModel.createBodyMesh` (26.1.2), texture
//! `entity/horse/horse_brown.png` 64x64 (variant rolls follow up).
//!
//! MC hierarchy: root -> body -> tail; root -> head_parts (the neck
//! box, resting at 30 deg) -> { head, mane, upper_mouth, ears }.
//! `head` and its siblings sit at `PartPose.ZERO`, so every child
//! parents straight onto `horse_head` here with a zero pivot - the
//! accumulated transform is identical and the whole neck+head
//! assembly follows the look pose. The equine gait (front legs 0.8
//! amplitude, hind legs 0.5, tail lift) lives in
//! `animation::pose_for_bone` under the `horse_` prefix.

use super::{Bone, EntityModel, McCube};

const FRAC_PI_6: f32 = std::f32::consts::FRAC_PI_6;

const BODY: &[McCube] =
    &[McCube::new([-5.0, -8.0, -17.0], [10.0, 10.0, 22.0], [0, 32]).inflated(0.05)];
const NECK: &[McCube] = &[McCube::new([-2.05, -6.0, -2.0], [4.0, 12.0, 7.0], [0, 35])];
const SKULL: &[McCube] = &[McCube::new([-3.0, -11.0, -2.0], [6.0, 5.0, 7.0], [0, 13])];
const MANE: &[McCube] = &[McCube::new([-1.0, -11.0, 5.01], [2.0, 16.0, 2.0], [56, 36])];
const MOUTH: &[McCube] = &[McCube::new([-2.0, -11.0, -7.0], [4.0, 5.0, 5.0], [0, 25])];
const EAR_L: &[McCube] =
    &[McCube::new([0.55, -13.0, 4.0], [2.0, 3.0, 1.0], [19, 16]).inflated(-0.001)];
const EAR_R: &[McCube] =
    &[McCube::new([-2.55, -13.0, 4.0], [2.0, 3.0, 1.0], [19, 16]).inflated(-0.001)];
const TAIL: &[McCube] = &[McCube::new([-1.5, 0.0, 0.0], [3.0, 14.0, 4.0], [42, 36])];
const LEG_HIND_R: &[McCube] = &[McCube::new([-1.0, -1.01, -1.0], [4.0, 11.0, 4.0], [48, 21])];
const LEG_HIND_L: &[McCube] =
    &[McCube::new([-3.0, -1.01, -1.0], [4.0, 11.0, 4.0], [48, 21]).mirrored()];
const LEG_FRONT_R: &[McCube] = &[McCube::new([-1.0, -1.01, -1.9], [4.0, 11.0, 4.0], [48, 21])];
const LEG_FRONT_L: &[McCube] =
    &[McCube::new([-3.0, -1.01, -1.9], [4.0, 11.0, 4.0], [48, 21]).mirrored()];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "horse_body", pivot: [0.0, 11.0, 5.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    // body child: PartPose.offsetAndRotation(0, -5, 2, PI/6, 0, 0).
    Bone { parent: Some("horse_body"), name: "horse_tail", pivot: [0.0, -5.0, 2.0], rotation: [FRAC_PI_6, 0.0, 0.0], cubes: TAIL },
    // head_parts: offsetAndRotation(0, 4, -12, PI/6, 0, 0).
    Bone { parent: None, name: "horse_head", pivot: [0.0, 4.0, -12.0], rotation: [FRAC_PI_6, 0.0, 0.0], cubes: NECK },
    Bone { parent: Some("horse_head"), name: "horse_skull", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: SKULL },
    Bone { parent: Some("horse_head"), name: "horse_mane", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: MANE },
    Bone { parent: Some("horse_head"), name: "horse_mouth", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: MOUTH },
    Bone { parent: Some("horse_head"), name: "horse_ear_l", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: EAR_L },
    Bone { parent: Some("horse_head"), name: "horse_ear_r", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: EAR_R },
    Bone { parent: None, name: "horse_leg_lh", pivot: [4.0, 14.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND_L },
    Bone { parent: None, name: "horse_leg_rh", pivot: [-4.0, 14.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND_R },
    Bone { parent: None, name: "horse_leg_lf", pivot: [4.0, 14.0, -10.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT_L },
    Bone { parent: None, name: "horse_leg_rf", pivot: [-4.0, 14.0, -10.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT_R },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0 };
