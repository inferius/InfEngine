//! W392 - goat, MC `GoatModel.createBodyLayer` verbatim (sheet
//! `entity/goat/goat.png`, 64x64). Head bone at (1,14,0) carries
//! the ears + goatee with the horns and the 0.96-pitched snout as
//! children; twin-box body; short hind / long front legs with the
//! cat-suffix names so the shared quadruped gait drives them.
//! Ramming + screaming variant are follow-ups.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    McCube::new([-6.0, -11.0, -10.0], [3.0, 2.0, 1.0], [2, 61]),
    McCube::new([2.0, -11.0, -10.0], [3.0, 2.0, 1.0], [2, 61]).mirrored(),
    McCube::new([-0.5, -3.0, -14.0], [0.0, 7.0, 5.0], [23, 52]),
];
const HORN_L: &[McCube] =
    &[McCube::new([-0.01, -16.0, -10.0], [2.0, 7.0, 2.0], [12, 55])];
const HORN_R: &[McCube] =
    &[McCube::new([-2.99, -16.0, -10.0], [2.0, 7.0, 2.0], [12, 55])];
const NOSE: &[McCube] =
    &[McCube::new([-3.0, -4.0, -8.0], [5.0, 7.0, 10.0], [34, 46])];
const BODY: &[McCube] = &[
    McCube::new([-4.0, -17.0, -7.0], [9.0, 11.0, 16.0], [1, 1]),
    McCube::new([-5.0, -18.0, -8.0], [11.0, 14.0, 11.0], [0, 28]),
];
const LEG_HIND_L: &[McCube] =
    &[McCube::new([0.0, 4.0, 0.0], [3.0, 6.0, 3.0], [36, 29])];
const LEG_HIND_R: &[McCube] =
    &[McCube::new([0.0, 4.0, 0.0], [3.0, 6.0, 3.0], [49, 29])];
const LEG_FRONT_L: &[McCube] =
    &[McCube::new([0.0, 0.0, 0.0], [3.0, 10.0, 3.0], [49, 2])];
const LEG_FRONT_R: &[McCube] =
    &[McCube::new([0.0, 0.0, 0.0], [3.0, 10.0, 3.0], [35, 2])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "goat_head", pivot: [1.0, 14.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: Some("goat_head"), name: "goat_horn_l", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HORN_L },
    Bone { parent: Some("goat_head"), name: "goat_horn_r", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HORN_R },
    Bone { parent: Some("goat_head"), name: "goat_nose", pivot: [0.0, -8.0, -8.0], rotation: [0.9599, 0.0, 0.0], cubes: NOSE },
    Bone { parent: None, name: "goat_body", pivot: [0.0, 24.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "goat_leg_lh", pivot: [1.0, 14.0, 4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND_L },
    Bone { parent: None, name: "goat_leg_rh", pivot: [-3.0, 14.0, 4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND_R },
    Bone { parent: None, name: "goat_leg_lf", pivot: [1.0, 14.0, -6.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT_L },
    Bone { parent: None, name: "goat_leg_rf", pivot: [-3.0, 14.0, -6.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT_R },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
