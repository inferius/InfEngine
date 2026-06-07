//! W92 - the skeleton, ported faithfully from MC `SkeletonModel` /
//! `HumanoidModel`, texture 64x32.
//!
//! Classic (64x32) humanoids mirror the left limbs from the right limb's
//! `texOffs`, and the skeleton overrides the arms/legs to be thin
//! (2x12x2) - matching the engine's proven BoxUv table.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[McCube::new([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0, 0])];
const BODY: &[McCube] = &[McCube::new([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16, 16])];
// Thin limbs; left mirrors the right's texOffs.
const ARM_R: &[McCube] = &[McCube::new([-1.0, -2.0, -1.0], [2.0, 12.0, 2.0], [40, 16])];
const ARM_L: &[McCube] = &[McCube::new([-1.0, -2.0, -1.0], [2.0, 12.0, 2.0], [40, 16]).mirrored()];
const LEG_R: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 12.0, 2.0], [0, 16])];
const LEG_L: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 12.0, 2.0], [0, 16]).mirrored()];

const BONES: &[Bone] = &[
    Bone { name: "skeleton_head", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { name: "skeleton_body", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { name: "skeleton_arm_r", pivot: [-5.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_R },
    Bone { name: "skeleton_arm_l", pivot: [5.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_L },
    Bone { name: "skeleton_leg_r", pivot: [-2.0, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { name: "skeleton_leg_l", pivot: [2.0, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
