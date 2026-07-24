//! W382 - bat, MC `BatModel.createBodyLayer` verbatim (sheet
//! `entity/bat.png`, 32x32). Tiny 3x5x2 body + 4x3x2 head with the
//! paper-thin ears, wings and feet. Rendered in the flying pose;
//! the hang-from-ceiling pose is a follow-up.

use super::{Bone, EntityModel, McCube};

const BODY: &[McCube] =
    &[McCube::new([-1.5, 0.0, -1.0], [3.0, 5.0, 2.0], [0, 0])];
const HEAD: &[McCube] =
    &[McCube::new([-2.0, -3.0, -1.0], [4.0, 3.0, 2.0], [0, 7])];
const EAR_R: &[McCube] =
    &[McCube::new([-2.5, -4.0, 0.0], [3.0, 5.0, 0.0], [1, 15])];
const EAR_L: &[McCube] =
    &[McCube::new([-0.1, -3.0, 0.0], [3.0, 5.0, 0.0], [8, 15])];
const WING_R: &[McCube] =
    &[McCube::new([-2.0, -2.0, 0.0], [2.0, 7.0, 0.0], [12, 0])];
const WING_TIP_R: &[McCube] =
    &[McCube::new([-6.0, -2.0, 0.0], [6.0, 8.0, 0.0], [16, 0])];
const WING_L: &[McCube] =
    &[McCube::new([0.0, -2.0, 0.0], [2.0, 7.0, 0.0], [12, 7])];
const WING_TIP_L: &[McCube] =
    &[McCube::new([0.0, -2.0, 0.0], [6.0, 8.0, 0.0], [16, 8])];
const FEET: &[McCube] =
    &[McCube::new([-1.5, 0.0, 0.0], [3.0, 2.0, 0.0], [16, 16])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "bat_body", pivot: [0.0, 17.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "bat_head", pivot: [0.0, 17.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: Some("bat_head"), name: "bat_ear_r", pivot: [-1.5, -2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: EAR_R },
    Bone { parent: Some("bat_head"), name: "bat_ear_l", pivot: [1.1, -3.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: EAR_L },
    Bone { parent: Some("bat_body"), name: "bat_wing_r", pivot: [-1.5, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: WING_R },
    Bone { parent: Some("bat_wing_r"), name: "bat_wing_tip_r", pivot: [-2.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: WING_TIP_R },
    Bone { parent: Some("bat_body"), name: "bat_wing_l", pivot: [1.5, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: WING_L },
    Bone { parent: Some("bat_wing_l"), name: "bat_wing_tip_l", pivot: [2.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: WING_TIP_L },
    Bone { parent: Some("bat_body"), name: "bat_feet", pivot: [0.0, 5.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: FEET },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
