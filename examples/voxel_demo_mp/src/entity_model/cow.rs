//! W91 - the cow, ported faithfully from MC `CowModel` +
//! `QuadrupedModel.createBodyMesh(legHeight = 12)`. `texOffs` are the
//! classic 64x32 layout; UVs normalize by the actual texture size at
//! bake time, so this works with modern 64x64 cow PNGs too.
//!
//! Everything here is the literal MC model data (boxes, pivots, texOffs,
//! the body's `xRot = PI/2`, and the two head horns the old hand port
//! dropped). The universal `entity_model` builder/renderer turns it into
//! the right meshes + transforms - no eyeballed scale/position magic.

use std::f32::consts::FRAC_PI_2;

use super::{Bone, EntityModel, McCube};

// Leg cube, shared by all four legs (texOffs 0,16). MC's QuadrupedModel
// uses ONE non-mirrored CubeListBuilder for all four, so there is no
// left/right mirror here.
const LEG: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16])];

const HEAD: &[McCube] = &[
    // Main head box.
    McCube::new([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0, 0]),
    // The two horns (both texOffs 22,0, no mirror - exactly as MC
    // `CowModel` adds them). These are the "missing nuance" the
    // hand-coded head dropped.
    McCube::new([-5.0, -5.0, -4.0], [1.0, 3.0, 1.0], [22, 0]),
    McCube::new([4.0, -5.0, -4.0], [1.0, 3.0, 1.0], [22, 0]),
];

const BODY: &[McCube] = &[
    McCube::new([-6.0, -10.0, -7.0], [12.0, 18.0, 10.0], [18, 4]),
    // W139: the udder - vanilla `CowModel` adds it as a SECOND thin
    // cube on the body bone (texOffs 52,0; 4x6x1 against the box's
    // -Z side, which the body's xRot = PI/2 turns into the rear
    // belly). The W91 port dropped it - the cow had no udder at all.
    McCube::new([-2.0, 2.0, -8.0], [4.0, 6.0, 1.0], [52, 0]),
];

const BONES: &[Bone] = &[
    Bone { name: "cow_head", pivot: [0.0, 4.0, -8.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Body: MC offsetAndRotation(0, 5, 2, PI/2, 0, 0) - the xRot lays the
    // 18-tall box horizontal and puts the udder face on the belly.
    Bone { name: "cow_body", pivot: [0.0, 5.0, 2.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { name: "cow_leg_rh", pivot: [-3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "cow_leg_lh", pivot: [3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "cow_leg_rf", pivot: [-3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "cow_leg_lf", pivot: [3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
