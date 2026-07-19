//! W315 - the cat, ported faithfully from MC
//! `AdultFelineModel.createBodyMesh` (26.1.2), texture
//! `entity/cat/cat_tabby.png` 64x32 (the village default variant).
//!
//! The body is a vertical 4x16x6 box rotated +90 deg around X into a
//! horizontal torso - the rotation bakes into the bone's rest pose.
//! `tail2` rests at MC's walking xRot (~1.728 rad); `tail1` leans 0.9.
//! Leg names follow the engine's quadruped convention so the shared
//! `pose_for_bone` diagonal gait drives them; sitting / lying / the
//! crouch poses are follow-ups (village cats stroll).

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    McCube::new([-2.5, -2.0, -3.0], [5.0, 4.0, 5.0], [0, 0]),
    // Nose / ears (uv from the named addBox calls).
    McCube::new([-1.5, 0.0, -4.0], [3.0, 2.0, 2.0], [0, 24]),
    McCube::new([-2.0, -3.0, 0.0], [1.0, 1.0, 2.0], [0, 10]),
    McCube::new([1.0, -3.0, 0.0], [1.0, 1.0, 2.0], [6, 10]),
];
const BODY: &[McCube] = &[McCube::new([-2.0, 3.0, -8.0], [4.0, 16.0, 6.0], [20, 0])];
const TAIL1: &[McCube] = &[McCube::new([-0.5, 0.0, 0.0], [1.0, 8.0, 1.0], [0, 15])];
const TAIL2: &[McCube] = &[McCube::new([-0.5, 0.0, 0.0], [1.0, 8.0, 1.0], [4, 15])];
const LEG_HIND: &[McCube] = &[McCube::new([-1.0, 0.0, 1.0], [2.0, 6.0, 2.0], [8, 13])];
const LEG_FRONT: &[McCube] = &[McCube::new([-1.0, 0.0, 0.0], [2.0, 10.0, 2.0], [40, 0])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "cat_head", pivot: [0.0, 15.0, -9.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // PartPose.offsetAndRotation(0, 12, -10, PI/2, 0, 0).
    Bone { parent: None, name: "cat_body", pivot: [0.0, 12.0, -10.0], rotation: [std::f32::consts::FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "cat_tail1", pivot: [0.0, 15.0, 8.0], rotation: [0.9, 0.0, 0.0], cubes: TAIL1 },
    // setupAnim keeps tail2 at ~1.7278761 while up; bake the rest.
    Bone { parent: None, name: "cat_tail2", pivot: [0.0, 20.0, 14.0], rotation: [1.727_876_1, 0.0, 0.0], cubes: TAIL2 },
    Bone { parent: None, name: "cat_leg_lh", pivot: [1.1, 18.0, 5.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND },
    Bone { parent: None, name: "cat_leg_rh", pivot: [-1.1, 18.0, 5.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND },
    Bone { parent: None, name: "cat_leg_lf", pivot: [1.2, 14.1, -5.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT },
    Bone { parent: None, name: "cat_leg_rf", pivot: [-1.2, 14.1, -5.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
