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

// Leg cube is shared by all four legs (texOffs 0,16; left legs mirror).
const LEG_R: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16])];
const LEG_L: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16]).mirrored()];

const HEAD: &[McCube] = &[
    // Main head box.
    McCube::new([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0, 0]),
    // Right + left horns (texOffs 22,0). These are the "missing nuance"
    // the hand-coded head left off.
    McCube::new([-5.0, -5.0, -4.0], [1.0, 3.0, 1.0], [22, 0]),
    McCube::new([4.0, -5.0, -4.0], [1.0, 3.0, 1.0], [22, 0]).mirrored(),
];

const BODY: &[McCube] = &[McCube::new([-6.0, -10.0, -7.0], [12.0, 18.0, 10.0], [18, 4])];

const BONES: &[Bone] = &[
    Bone { name: "cow_head", pivot: [0.0, 4.0, -8.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Body: MC offsetAndRotation(0, 5, 2, PI/2, 0, 0) - the xRot lays the
    // 18-tall box horizontal and puts the udder face on the belly.
    Bone { name: "cow_body", pivot: [0.0, 5.0, 2.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { name: "cow_leg_rh", pivot: [-3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { name: "cow_leg_lh", pivot: [3.0, 12.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
    Bone { name: "cow_leg_rf", pivot: [-3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { name: "cow_leg_lf", pivot: [3.0, 12.0, -7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
