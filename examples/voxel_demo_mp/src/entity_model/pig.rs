//! W92 - the pig, ported faithfully from MC `PigModel` +
//! `QuadrupedModel.createBodyMesh(legHeight = 6)`, texture 64x64.
//!
//! Same universal pipeline as the cow: pure MC `ModelPart` data, baked by
//! the shared cube->mesh + MC->engine transform. The snout cube
//! (texOffs 16,16) is the "missing nuance" the old hand port dropped, the
//! pig analogue of the cow's horns.

use std::f32::consts::FRAC_PI_2;

use super::{Bone, EntityModel, McCube};

// Shared non-mirrored leg (QuadrupedModel uses one builder for all four).
const LEG: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0, 16])];

const HEAD: &[McCube] = &[
    // Main head box (texOffs 0,0).
    McCube::new([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0, 0]),
    // Snout (texOffs 16,16) - the pig's defining nuance.
    McCube::new([-2.0, 0.0, -9.0], [4.0, 3.0, 1.0], [16, 16]),
];

const BODY: &[McCube] = &[McCube::new([-5.0, -10.0, -7.0], [10.0, 16.0, 8.0], [28, 8])];

const BONES: &[Bone] = &[
    Bone { name: "pig_head", pivot: [0.0, 12.0, -6.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Body: QuadrupedModel offsetAndRotation(0, 11, 2, PI/2, 0, 0).
    Bone { name: "pig_body", pivot: [0.0, 11.0, 2.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { name: "pig_leg_rh", pivot: [-3.0, 18.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "pig_leg_lh", pivot: [3.0, 18.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "pig_leg_rf", pivot: [-3.0, 18.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "pig_leg_lf", pivot: [3.0, 18.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
