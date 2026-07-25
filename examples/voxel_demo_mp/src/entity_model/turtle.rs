//! W392 - turtle, MC `AdultTurtleModel.createBodyLayer` verbatim
//! (sheet `entity/turtle/big_sea_turtle.png`, 128x64). PI/2-pitched
//! shell+belly column, a thin extra egg-belly plate, paddle-flat
//! legs (cat-suffix names for the shared gait). Egg laying + beach
//! nesting are follow-ups.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] =
    &[McCube::new([-3.0, -1.0, -3.0], [6.0, 5.0, 6.0], [3, 0])];
const BODY: &[McCube] = &[
    McCube::new([-9.5, 3.0, -10.0], [19.0, 20.0, 6.0], [7, 37]),
    McCube::new([-5.5, 3.0, -13.0], [11.0, 18.0, 3.0], [31, 1]),
];
const EGG_BELLY: &[McCube] =
    &[McCube::new([-4.5, 3.0, -14.0], [9.0, 18.0, 1.0], [70, 33])];
const LEG_HIND_R: &[McCube] =
    &[McCube::new([-2.0, 0.0, 0.0], [4.0, 1.0, 10.0], [1, 23])];
const LEG_HIND_L: &[McCube] =
    &[McCube::new([-2.0, 0.0, 0.0], [4.0, 1.0, 10.0], [1, 12])];
const LEG_FRONT_R: &[McCube] =
    &[McCube::new([-13.0, 0.0, -2.0], [13.0, 1.0, 5.0], [27, 30])];
const LEG_FRONT_L: &[McCube] =
    &[McCube::new([0.0, 0.0, -2.0], [13.0, 1.0, 5.0], [27, 24])];

const FRAC_PI_2: f32 = core::f32::consts::FRAC_PI_2;

const BONES: &[Bone] = &[
    Bone { parent: None, name: "turtle_head", pivot: [0.0, 19.0, -10.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: None, name: "turtle_body", pivot: [0.0, 11.0, -10.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "turtle_egg_belly", pivot: [0.0, 11.0, -10.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: EGG_BELLY },
    Bone { parent: None, name: "turtle_leg_rh", pivot: [-3.5, 22.0, 11.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND_R },
    Bone { parent: None, name: "turtle_leg_lh", pivot: [3.5, 22.0, 11.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND_L },
    Bone { parent: None, name: "turtle_leg_rf", pivot: [-5.0, 21.0, -4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT_R },
    Bone { parent: None, name: "turtle_leg_lf", pivot: [5.0, 21.0, -4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT_L },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
