//! W375 - bee, MC `AdultBeeModel.createBodyLayer` verbatim.
//!
//! Root bone at (0,19,0) - the bee hovers ~5 px over its position.
//! Body 7x7x10 texOffs(0,0) with the stinger + two antennae as
//! children; two flat 9x0x6 wings at (+-1.5,-4,-3) yawed +-15 deg;
//! three flat 7x2x0 leg strips. Texture `entity/bee/bee.png`
//! (64x64). Wing flap / bob animation is a render-side follow-up -
//! the baked pose already reads as a bee.

use super::{Bone, EntityModel, McCube};

const BODY: &[McCube] =
    &[McCube::new([-3.5, -4.0, -5.0], [7.0, 7.0, 10.0], [0, 0])];
const STINGER: &[McCube] =
    &[McCube::new([0.0, -1.0, 5.0], [0.0, 1.0, 2.0], [26, 7])];
const LEFT_ANTENNA: &[McCube] =
    &[McCube::new([1.5, -2.0, -3.0], [1.0, 2.0, 3.0], [2, 0])];
const RIGHT_ANTENNA: &[McCube] =
    &[McCube::new([-2.5, -2.0, -3.0], [1.0, 2.0, 3.0], [2, 3])];
const LEFT_WING: &[McCube] =
    &[McCube::new([0.0, 0.0, 0.0], [9.0, 0.0, 6.0], [0, 18]).mirrored()];
const RIGHT_WING: &[McCube] =
    &[McCube::new([-9.0, 0.0, 0.0], [9.0, 0.0, 6.0], [0, 18])];
const FRONT_LEGS: &[McCube] =
    &[McCube::new([-5.0, 0.0, 0.0], [7.0, 2.0, 0.0], [26, 1])];
const MIDDLE_LEGS: &[McCube] =
    &[McCube::new([-5.0, 0.0, 0.0], [7.0, 2.0, 0.0], [26, 3])];
const BACK_LEGS: &[McCube] =
    &[McCube::new([-5.0, 0.0, 0.0], [7.0, 2.0, 0.0], [26, 5])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "bee_bone", pivot: [0.0, 19.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: &[] },
    Bone { parent: Some("bee_bone"), name: "bee_body", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: Some("bee_body"), name: "bee_stinger", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: STINGER },
    Bone { parent: Some("bee_body"), name: "bee_antenna_l", pivot: [0.0, -2.0, -5.0], rotation: [0.0, 0.0, 0.0], cubes: LEFT_ANTENNA },
    Bone { parent: Some("bee_body"), name: "bee_antenna_r", pivot: [0.0, -2.0, -5.0], rotation: [0.0, 0.0, 0.0], cubes: RIGHT_ANTENNA },
    Bone { parent: Some("bee_bone"), name: "bee_wing_l", pivot: [1.5, -4.0, -3.0], rotation: [0.0, 0.2618, 0.0], cubes: LEFT_WING },
    Bone { parent: Some("bee_bone"), name: "bee_wing_r", pivot: [-1.5, -4.0, -3.0], rotation: [0.0, -0.2618, 0.0], cubes: RIGHT_WING },
    Bone { parent: Some("bee_bone"), name: "bee_legs_front", pivot: [1.5, 3.0, -2.0], rotation: [0.0, 0.0, 0.0], cubes: FRONT_LEGS },
    Bone { parent: Some("bee_bone"), name: "bee_legs_mid", pivot: [1.5, 3.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: MIDDLE_LEGS },
    Bone { parent: Some("bee_bone"), name: "bee_legs_back", pivot: [1.5, 3.0, 2.0], rotation: [0.0, 0.0, 0.0], cubes: BACK_LEGS },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
