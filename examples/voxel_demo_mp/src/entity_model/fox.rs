//! W390 - fox, MC `AdultFoxModel.createBodyLayer` verbatim (sheet
//! `entity/fox/fox.png`, 48x32). The whole model rides MC's odd
//! -1 px x offset; body is the usual quadruped PI/2-pitched column
//! with the tail as its child at the rump. Legs reuse the cat's
//! `_leg_lh/rh/lf/rf` suffixes so the shared walk gait drives them.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    McCube::new([-3.0, -2.0, -5.0], [8.0, 6.0, 6.0], [1, 5]),
    // right ear / left ear / nose ride the head bone directly.
    McCube::new([-3.0, -4.0, -4.0], [2.0, 2.0, 1.0], [8, 1]),
    McCube::new([3.0, -4.0, -4.0], [2.0, 2.0, 1.0], [15, 1]),
    McCube::new([-1.0, 2.01, -8.0], [4.0, 2.0, 3.0], [6, 18]),
];
const BODY: &[McCube] =
    &[McCube::new([-3.0, 3.999, -3.5], [6.0, 11.0, 6.0], [24, 15])];
const LEG_L: &[McCube] =
    &[McCube::new([2.0, 0.5, -1.0], [2.0, 6.0, 2.0], [4, 24]).inflated(0.001)];
const LEG_R: &[McCube] =
    &[McCube::new([2.0, 0.5, -1.0], [2.0, 6.0, 2.0], [13, 24]).inflated(0.001)];
const TAIL: &[McCube] =
    &[McCube::new([2.0, 0.0, -1.0], [4.0, 9.0, 5.0], [30, 0])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "fox_head", pivot: [-1.0, 16.5, -3.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: None, name: "fox_body", pivot: [0.0, 16.0, -6.0], rotation: [core::f32::consts::FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { parent: Some("fox_body"), name: "fox_tail", pivot: [-4.0, 15.0, -1.0], rotation: [-0.052_359_88, 0.0, 0.0], cubes: TAIL },
    Bone { parent: None, name: "fox_leg_rh", pivot: [-5.0, 17.5, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { parent: None, name: "fox_leg_lh", pivot: [-1.0, 17.5, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
    Bone { parent: None, name: "fox_leg_rf", pivot: [-5.0, 17.5, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { parent: None, name: "fox_leg_lf", pivot: [-1.0, 17.5, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
