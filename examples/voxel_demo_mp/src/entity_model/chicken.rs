//! W92 - the chicken, ported faithfully from MC `ChickenModel`,
//! texture 64x32.
//!
//! The head bone carries three MC cubes (head + beak + wattle), and the
//! two wings are restored - all "missing nuances" the old hand port
//! dropped (it kept only head/body/leg).

use std::f32::consts::FRAC_PI_2;

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    // Head, beak, and the red wattle, all on the head bone (pivot 0,15,-4).
    McCube::new([-2.0, -6.0, -2.0], [4.0, 6.0, 3.0], [0, 0]),
    McCube::new([-2.0, -4.0, -4.0], [4.0, 2.0, 2.0], [14, 0]),
    McCube::new([-1.0, -2.0, -3.0], [2.0, 2.0, 2.0], [14, 4]),
];
const BODY: &[McCube] = &[McCube::new([-3.0, -4.0, -3.0], [6.0, 8.0, 6.0], [0, 9])];
const LEG: &[McCube] = &[McCube::new([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [26, 0])];
// Wings share texOffs(24,13); MC uses different box origins (no mirror).
const WING_R: &[McCube] = &[McCube::new([0.0, 0.0, -3.0], [1.0, 4.0, 6.0], [24, 13])];
const WING_L: &[McCube] = &[McCube::new([-1.0, 0.0, -3.0], [1.0, 4.0, 6.0], [24, 13])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "chicken_head", pivot: [0.0, 15.0, -4.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Body: offsetAndRotation(0, 16, 0, PI/2, 0, 0).
    Bone { parent: None, name: "chicken_body", pivot: [0.0, 16.0, 0.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "chicken_leg_r", pivot: [-2.0, 19.0, 1.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "chicken_leg_l", pivot: [1.0, 19.0, 1.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "chicken_wing_r", pivot: [-4.0, 13.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: WING_R },
    Bone { parent: None, name: "chicken_wing_l", pivot: [4.0, 13.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: WING_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0 };
