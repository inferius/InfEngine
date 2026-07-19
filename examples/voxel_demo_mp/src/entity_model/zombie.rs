//! W92 - the zombie, ported faithfully from MC `ZombieModel` /
//! `HumanoidModel.createMesh`, texture 64x64.
//!
//! W158: the modern 64x64 layout RESERVES left-limb regions at
//! (32,48)/(16,48), but this pack's zombie.png leaves them fully
//! transparent (it relies on the classic mirror behaviour) - the
//! left arm + leg rendered INVISIBLE. The left limbs now mirror the
//! right-limb tiles like MC's legacy path; caught by the
//! `w158_every_model_unwrap_lands_on_art` coverage audit.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[McCube::new([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0, 0])];
const BODY: &[McCube] = &[McCube::new([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16, 16])];
const ARM_R: &[McCube] = &[McCube::new([-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], [40, 16])];
const ARM_L: &[McCube] =
    &[McCube::new([-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], [40, 16]).mirrored()];
const LEG_R: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16])];
const LEG_L: &[McCube] =
    &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16]).mirrored()];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "zombie_head", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: None, name: "zombie_body", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "zombie_arm_r", pivot: [-5.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_R },
    Bone { parent: None, name: "zombie_arm_l", pivot: [5.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_L },
    Bone { parent: None, name: "zombie_leg_r", pivot: [-1.9, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { parent: None, name: "zombie_leg_l", pivot: [1.9, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0 };
