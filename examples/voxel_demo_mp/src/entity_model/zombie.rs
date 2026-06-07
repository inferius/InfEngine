//! W92 - the zombie, ported faithfully from MC `ZombieModel` /
//! `HumanoidModel.createMesh`, texture 64x64.
//!
//! Modern (64x64) humanoids have dedicated left-limb texture regions, so
//! the left arm/leg use their own `texOffs` (32,48 / 16,48) rather than
//! mirroring the right - matching the engine's proven BoxUv table.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[McCube::new([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0, 0])];
const BODY: &[McCube] = &[McCube::new([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16, 16])];
const ARM_R: &[McCube] = &[McCube::new([-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], [40, 16])];
const ARM_L: &[McCube] = &[McCube::new([-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], [32, 48])];
const LEG_R: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16])];
const LEG_L: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [16, 48])];

const BONES: &[Bone] = &[
    Bone { name: "zombie_head", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { name: "zombie_body", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { name: "zombie_arm_r", pivot: [-5.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_R },
    Bone { name: "zombie_arm_l", pivot: [5.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_L },
    Bone { name: "zombie_leg_r", pivot: [-1.9, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { name: "zombie_leg_l", pivot: [1.9, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
