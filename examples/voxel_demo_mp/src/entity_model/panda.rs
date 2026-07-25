//! W393 - panda, MC `PandaModel.createBodyLayer` verbatim (sheet
//! `entity/panda/panda.png`, 64x64). Wide 13x10x9 head with nose +
//! ears in the same bone, the big PI/2-pitched 19x26x13 body, four
//! chunky 6x9x6 legs on the shared quadruped gait. Sitting, rolling
//! and the personality genes are follow-ups.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    McCube::new([-6.5, -5.0, -4.0], [13.0, 10.0, 9.0], [0, 6]),
    McCube::new([-3.5, 0.0, -6.0], [7.0, 5.0, 2.0], [45, 16]),
    McCube::new([3.5, -8.0, -1.0], [5.0, 4.0, 1.0], [52, 25]),
    McCube::new([-8.5, -8.0, -1.0], [5.0, 4.0, 1.0], [52, 25]),
];
const BODY: &[McCube] =
    &[McCube::new([-9.5, -13.0, -6.5], [19.0, 26.0, 13.0], [0, 25])];
const LEG: &[McCube] =
    &[McCube::new([-3.0, 0.0, -3.0], [6.0, 9.0, 6.0], [40, 0])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "panda_head", pivot: [0.0, 11.5, -17.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: None, name: "panda_body", pivot: [0.0, 10.0, 0.0], rotation: [core::f32::consts::FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "panda_leg_rh", pivot: [-5.5, 15.0, 9.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "panda_leg_lh", pivot: [5.5, 15.0, 9.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "panda_leg_rf", pivot: [-5.5, 15.0, -9.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "panda_leg_lf", pivot: [5.5, 15.0, -9.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
