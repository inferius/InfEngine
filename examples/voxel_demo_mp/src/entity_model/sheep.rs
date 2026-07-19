//! W92 - the sheep, ported faithfully from MC `SheepModel` +
//! `QuadrupedModel.createBodyMesh(legHeight = 12)`, texture 64x32.
//!
//! W139 adds the missing second layer: MC renders a sheep as the
//! skin model (`sheep.png` - the "sheared" look) PLUS the
//! `SheepFurModel` wool overlay (`sheep_wool.png`, the same bones
//! with inflated cubes, tinted by the dye colour in vanilla).
//! Without the overlay our sheep showed the bare skin texture.
//! `FUR_MODEL` below is the `SheepFurModel.createFurLayer` data:
//! the head wool is a SHORTER 6x6x6 box (the snout pokes out)
//! inflated 0.6, the body box inflates 1.75, and the legs wear
//! 4x6x4 wool "socks" on their upper half inflated 0.5. Pivots
//! mirror OUR base bones so the two layers stay aligned.

use std::f32::consts::FRAC_PI_2;

use super::{Bone, EntityModel, McCube};

const LEG: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16])];
const HEAD: &[McCube] = &[McCube::new([-3.0, -4.0, -6.0], [6.0, 6.0, 8.0], [0, 0])];
const BODY: &[McCube] = &[McCube::new([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [28, 8])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "sheep_head", pivot: [0.0, 6.0, -8.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Body: QuadrupedModel offsetAndRotation(0, 5, 2, PI/2, 0, 0).
    Bone { parent: None, name: "sheep_body", pivot: [0.0, 5.0, 2.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "sheep_leg_rh", pivot: [-3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "sheep_leg_lh", pivot: [3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "sheep_leg_rf", pivot: [-3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "sheep_leg_lf", pivot: [3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0, tex_grid: None };

// ---- W139: SheepFurModel (the wool overlay) ----------------------

const FUR_HEAD: &[McCube] =
    &[McCube::new([-3.0, -4.0, -4.0], [6.0, 6.0, 6.0], [0, 0]).inflated(0.6)];
const FUR_BODY: &[McCube] =
    &[McCube::new([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [28, 8]).inflated(1.75)];
const FUR_LEG: &[McCube] =
    &[McCube::new([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0, 16]).inflated(0.5)];

const FUR_BONES: &[Bone] = &[
    Bone { parent: None, name: "sheep_fur_head", pivot: [0.0, 6.0, -8.0], rotation: [0.0, 0.0, 0.0], cubes: FUR_HEAD },
    Bone { parent: None, name: "sheep_fur_body", pivot: [0.0, 5.0, 2.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: FUR_BODY },
    Bone { parent: None, name: "sheep_fur_leg_rh", pivot: [-3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: FUR_LEG },
    Bone { parent: None, name: "sheep_fur_leg_lh", pivot: [3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: FUR_LEG },
    Bone { parent: None, name: "sheep_fur_leg_rf", pivot: [-3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: FUR_LEG },
    Bone { parent: None, name: "sheep_fur_leg_lf", pivot: [3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: FUR_LEG },
];

pub(crate) const FUR_MODEL: EntityModel = EntityModel { bones: FUR_BONES, scale: 1.0, tex_grid: None };
