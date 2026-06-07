//! W92 - the sheep, ported faithfully from MC `SheepModel` +
//! `QuadrupedModel.createBodyMesh(legHeight = 12)`, texture 64x32.
//!
//! This engine renders the sheep with the full woolly entity texture (no
//! separate sheared-body + fur-overlay split), so it's a plain quadruped
//! port like the cow/pig - just the sheep's box dimensions.

use std::f32::consts::FRAC_PI_2;

use super::{Bone, EntityModel, McCube};

const LEG: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16])];
const HEAD: &[McCube] = &[McCube::new([-3.0, -4.0, -6.0], [6.0, 6.0, 8.0], [0, 0])];
const BODY: &[McCube] = &[McCube::new([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [28, 8])];

const BONES: &[Bone] = &[
    Bone { name: "sheep_head", pivot: [0.0, 6.0, -8.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Body: QuadrupedModel offsetAndRotation(0, 5, 2, PI/2, 0, 0).
    Bone { name: "sheep_body", pivot: [0.0, 5.0, 2.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { name: "sheep_leg_rh", pivot: [-3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "sheep_leg_lh", pivot: [3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "sheep_leg_rf", pivot: [-3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "sheep_leg_lf", pivot: [3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
