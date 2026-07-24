//! W378 - illager family (MC `IllagerModel.createBodyLayer`
//! verbatim). One mesh serves the pillager, vindicator and evoker -
//! the sheets (`entity/illager/*.png`, 64x64) reskin it exactly
//! like MC. Crossed arms for everyone in v1 (the pillager's
//! crossbow-hold pose is a follow-up).
//!
//! Head 8x10x8 + hat overlay (inflate 0.45) + the long 2x4x2 nose;
//! body 8x12x6 with the 8x20x6 robe overlay (inflate 0.5); crossed
//! arm block at (0,3,-1) rotated -0.75 rad; 4x12x4 legs. Bone names
//! follow the villager convention so the shared walk gait applies.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] =
    &[McCube::new([-4.0, -10.0, -4.0], [8.0, 10.0, 8.0], [0, 0])];
const HAT: &[McCube] =
    &[McCube::new([-4.0, -10.0, -4.0], [8.0, 12.0, 8.0], [32, 0]).inflated(0.45)];
const NOSE: &[McCube] =
    &[McCube::new([-1.0, -1.0, -6.0], [2.0, 4.0, 2.0], [24, 0])];
const BODY: &[McCube] = &[
    McCube::new([-4.0, 0.0, -3.0], [8.0, 12.0, 6.0], [16, 20]),
    McCube::new([-4.0, 0.0, -3.0], [8.0, 20.0, 6.0], [0, 38]).inflated(0.5),
];
const ARMS: &[McCube] = &[
    McCube::new([-8.0, -2.0, -2.0], [4.0, 8.0, 4.0], [44, 22]),
    McCube::new([-4.0, 2.0, -2.0], [8.0, 4.0, 4.0], [40, 38]),
    McCube::new([4.0, -2.0, -2.0], [4.0, 8.0, 4.0], [44, 22]).mirrored(),
];
const LEG: &[McCube] =
    &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 22])];
const LEG_M: &[McCube] =
    &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 22]).mirrored()];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "illager_head", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: Some("illager_head"), name: "illager_hat", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HAT },
    Bone { parent: Some("illager_head"), name: "illager_nose", pivot: [0.0, -2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: NOSE },
    Bone { parent: None, name: "illager_body", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "illager_arms", pivot: [0.0, 3.0, -1.0], rotation: [-0.75, 0.0, 0.0], cubes: ARMS },
    Bone { parent: None, name: "illager_leg_r", pivot: [-2.0, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "illager_leg_l", pivot: [2.0, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_M },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
