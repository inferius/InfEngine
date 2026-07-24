//! W381 - enderman, MC `EndermanModel.createBodyLayer` verbatim
//! (sheet `entity/enderman/enderman.png`, 64x32). The three-block
//! silhouette: 8^3 head (+ the -0.5 inflated hat overlay) riding a
//! 8x12x4 body at pivot y -14, with the trademark 2x30x2 limbs.
//! Carried-block + scream pose are follow-ups.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] =
    &[McCube::new([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0, 0])];
const HAT: &[McCube] =
    &[McCube::new([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0, 16]).inflated(-0.5)];
const BODY: &[McCube] =
    &[McCube::new([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [32, 16])];
const ARM_R: &[McCube] =
    &[McCube::new([-1.0, -2.0, -1.0], [2.0, 30.0, 2.0], [56, 0])];
const ARM_L: &[McCube] =
    &[McCube::new([-1.0, -2.0, -1.0], [2.0, 30.0, 2.0], [56, 0]).mirrored()];
const LEG_R: &[McCube] =
    &[McCube::new([-1.0, 0.0, -1.0], [2.0, 30.0, 2.0], [56, 0])];
const LEG_L: &[McCube] =
    &[McCube::new([-1.0, 0.0, -1.0], [2.0, 30.0, 2.0], [56, 0]).mirrored()];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "enderman_head", pivot: [0.0, -13.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: Some("enderman_head"), name: "enderman_hat", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HAT },
    Bone { parent: None, name: "enderman_body", pivot: [0.0, -14.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "enderman_arm_r", pivot: [-5.0, -12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_R },
    Bone { parent: None, name: "enderman_arm_l", pivot: [5.0, -12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_L },
    Bone { parent: None, name: "enderman_leg_r", pivot: [-2.0, -5.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { parent: None, name: "enderman_leg_l", pivot: [2.0, -5.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
