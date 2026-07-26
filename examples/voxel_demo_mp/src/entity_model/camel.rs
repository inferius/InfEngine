//! W409 - camel, MC `AdultCamelModel.createBodyMesh` verbatim
//! (sheet `entity/camel/camel.png`, 128x128). Tall desert mount:
//! 15x12x27 body at y offset 4 with the hump + flat 3x14 tail +
//! long-necked head (3 cubes: snout deck, neck column, nose) as
//! body children, ears as head children, four 5x21x5 legs on the
//! shared quadruped gait. Sit / dash poses + riding are
//! follow-ups (the R23 report only needs the mob in the world).

use super::{Bone, EntityModel, McCube};

const BODY: &[McCube] =
    &[McCube::new([-7.5, -12.0, -23.5], [15.0, 12.0, 27.0], [0, 25])];
const HUMP: &[McCube] =
    &[McCube::new([-4.5, -5.0, -5.5], [9.0, 5.0, 11.0], [74, 0])];
const TAIL: &[McCube] =
    &[McCube::new([-1.5, 0.0, 0.0], [3.0, 14.0, 0.0], [122, 0])];
const HEAD: &[McCube] = &[
    McCube::new([-3.5, -7.0, -15.0], [7.0, 8.0, 19.0], [60, 24]),
    McCube::new([-3.5, -21.0, -15.0], [7.0, 14.0, 7.0], [21, 0]),
    McCube::new([-2.5, -21.0, -21.0], [5.0, 5.0, 6.0], [50, 0]),
];
const EAR_L: &[McCube] =
    &[McCube::new([-0.5, 0.5, -1.0], [3.0, 1.0, 2.0], [45, 0])];
const EAR_R: &[McCube] =
    &[McCube::new([-2.5, 0.5, -1.0], [3.0, 1.0, 2.0], [67, 0])];
const LEG_LH: &[McCube] =
    &[McCube::new([-2.5, 2.0, -2.5], [5.0, 21.0, 5.0], [58, 16])];
const LEG_RH: &[McCube] =
    &[McCube::new([-2.5, 2.0, -2.5], [5.0, 21.0, 5.0], [94, 16])];
const LEG_LF: &[McCube] =
    &[McCube::new([-2.5, 2.0, -2.5], [5.0, 21.0, 5.0], [0, 0])];
const LEG_RF: &[McCube] =
    &[McCube::new([-2.5, 2.0, -2.5], [5.0, 21.0, 5.0], [0, 26])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "camel_body", pivot: [0.0, 4.0, 9.5], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: Some("camel_body"), name: "camel_hump", pivot: [0.0, -12.0, -10.0], rotation: [0.0, 0.0, 0.0], cubes: HUMP },
    Bone { parent: Some("camel_body"), name: "camel_tail", pivot: [0.0, -9.0, 3.5], rotation: [0.0, 0.0, 0.0], cubes: TAIL },
    Bone { parent: Some("camel_body"), name: "camel_head", pivot: [0.0, -3.0, -19.5], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: Some("camel_head"), name: "camel_ear_l", pivot: [2.5, -21.0, -9.5], rotation: [0.0, 0.0, 0.0], cubes: EAR_L },
    Bone { parent: Some("camel_head"), name: "camel_ear_r", pivot: [-2.5, -21.0, -9.5], rotation: [0.0, 0.0, 0.0], cubes: EAR_R },
    Bone { parent: None, name: "camel_leg_lh", pivot: [4.9, 1.0, 9.5], rotation: [0.0, 0.0, 0.0], cubes: LEG_LH },
    Bone { parent: None, name: "camel_leg_rh", pivot: [-4.9, 1.0, 9.5], rotation: [0.0, 0.0, 0.0], cubes: LEG_RH },
    Bone { parent: None, name: "camel_leg_lf", pivot: [4.9, 1.0, -10.5], rotation: [0.0, 0.0, 0.0], cubes: LEG_LF },
    Bone { parent: None, name: "camel_leg_rf", pivot: [-4.9, 1.0, -10.5], rotation: [0.0, 0.0, 0.0], cubes: LEG_RF },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
