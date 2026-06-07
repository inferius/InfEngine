//! W92 - the spider, ported faithfully from MC `SpiderModel`,
//! texture 64x32.
//!
//! Two body segments (thorax + abdomen) and eight legs. Each leg carries
//! its MC rest-pose rotation: a `zRot` splay (~45 deg outer, ~33 deg
//! inner) plus a `yRot` fore/aft spread. Right legs use the base leg
//! cube, left legs the mirrored one. Walk animation can later layer onto
//! these via `BonePose`.

use std::f32::consts::{FRAC_PI_4, FRAC_PI_8};

use super::{Bone, EntityModel, McCube};

// MC's inner-leg splay angle (the middles), between PI/8 spread legs.
const Z_MID: f32 = 0.581_776_44;

const HEAD: &[McCube] = &[McCube::new([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [32, 4])];
const BODY0: &[McCube] = &[McCube::new([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0, 0])];
const BODY1: &[McCube] = &[McCube::new([-5.0, -4.0, -6.0], [10.0, 8.0, 12.0], [0, 12])];
// Right legs extend toward -X; left legs are the mirrored +X version.
const LEG_R: &[McCube] = &[McCube::new([-15.0, -1.0, -1.0], [16.0, 2.0, 2.0], [18, 0])];
const LEG_L: &[McCube] = &[McCube::new([-1.0, -1.0, -1.0], [16.0, 2.0, 2.0], [18, 0]).mirrored()];

const BONES: &[Bone] = &[
    Bone { name: "spider_head", pivot: [0.0, 15.0, -3.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { name: "spider_body0", pivot: [0.0, 15.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY0 },
    Bone { name: "spider_body1", pivot: [0.0, 15.0, 9.0], rotation: [0.0, 0.0, 0.0], cubes: BODY1 },
    // 8 legs: pivots at +-4 on X, z stepping back-to-front; rest rotation
    // (yRot spread, zRot splay) straight from MC SpiderModel.setupAnim.
    Bone { name: "spider_leg_rh", pivot: [-4.0, 15.0, 2.0], rotation: [0.0, FRAC_PI_4, -FRAC_PI_4], cubes: LEG_R },
    Bone { name: "spider_leg_lh", pivot: [4.0, 15.0, 2.0], rotation: [0.0, -FRAC_PI_4, FRAC_PI_4], cubes: LEG_L },
    Bone { name: "spider_leg_rmh", pivot: [-4.0, 15.0, 1.0], rotation: [0.0, FRAC_PI_8, -Z_MID], cubes: LEG_R },
    Bone { name: "spider_leg_lmh", pivot: [4.0, 15.0, 1.0], rotation: [0.0, -FRAC_PI_8, Z_MID], cubes: LEG_L },
    Bone { name: "spider_leg_rmf", pivot: [-4.0, 15.0, 0.0], rotation: [0.0, -FRAC_PI_8, -Z_MID], cubes: LEG_R },
    Bone { name: "spider_leg_lmf", pivot: [4.0, 15.0, 0.0], rotation: [0.0, FRAC_PI_8, Z_MID], cubes: LEG_L },
    Bone { name: "spider_leg_rf", pivot: [-4.0, 15.0, -1.0], rotation: [0.0, -FRAC_PI_4, -FRAC_PI_4], cubes: LEG_R },
    Bone { name: "spider_leg_lf", pivot: [4.0, 15.0, -1.0], rotation: [0.0, FRAC_PI_4, FRAC_PI_4], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
