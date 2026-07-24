//! W381 - silverfish, MC `SilverfishModel.createBodyLayer` verbatim
//! (flat `entity/silverfish.png`, 64x32). Seven body segments laid
//! nose-to-tail (sizes/texs from the vanilla tables, z chained from
//! -3.5 by the half-depth sums) + the three dorsal wing fins.

use super::{Bone, EntityModel, McCube};

// BODY_SIZES {w,h,d} / BODY_TEXS / z placements per the source:
// z = [-3.5, -1.5, 1.0, 4.0, 7.0, 9.5, 11.5], y = 24 - h.
const S0: &[McCube] = &[McCube::new([-1.5, 0.0, -1.0], [3.0, 2.0, 2.0], [0, 0])];
const S1: &[McCube] = &[McCube::new([-2.0, 0.0, -1.0], [4.0, 3.0, 2.0], [0, 4])];
const S2: &[McCube] = &[McCube::new([-3.0, 0.0, -1.5], [6.0, 4.0, 3.0], [0, 9])];
const S3: &[McCube] = &[McCube::new([-1.5, 0.0, -1.5], [3.0, 3.0, 3.0], [0, 16])];
const S4: &[McCube] = &[McCube::new([-1.0, 0.0, -1.5], [2.0, 2.0, 3.0], [0, 22])];
const S5: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 1.0, 2.0], [11, 0])];
const S6: &[McCube] = &[McCube::new([-0.5, 0.0, -1.0], [1.0, 1.0, 2.0], [13, 4])];
const WING_BIG: &[McCube] =
    &[McCube::new([-5.0, 0.0, -1.5], [10.0, 8.0, 3.0], [20, 0])];
const WING_MID: &[McCube] =
    &[McCube::new([-3.0, 0.0, -1.5], [6.0, 4.0, 3.0], [20, 11])];
const WING_SMALL: &[McCube] =
    &[McCube::new([-3.0, 0.0, -1.0], [6.0, 5.0, 2.0], [20, 18])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "sf_seg_0", pivot: [0.0, 22.0, -3.5], rotation: [0.0, 0.0, 0.0], cubes: S0 },
    Bone { parent: None, name: "sf_seg_1", pivot: [0.0, 21.0, -1.5], rotation: [0.0, 0.0, 0.0], cubes: S1 },
    Bone { parent: None, name: "sf_seg_2", pivot: [0.0, 20.0, 1.0], rotation: [0.0, 0.0, 0.0], cubes: S2 },
    Bone { parent: None, name: "sf_seg_3", pivot: [0.0, 21.0, 4.0], rotation: [0.0, 0.0, 0.0], cubes: S3 },
    Bone { parent: None, name: "sf_seg_4", pivot: [0.0, 22.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: S4 },
    Bone { parent: None, name: "sf_seg_5", pivot: [0.0, 23.0, 9.5], rotation: [0.0, 0.0, 0.0], cubes: S5 },
    Bone { parent: None, name: "sf_seg_6", pivot: [0.0, 23.0, 11.5], rotation: [0.0, 0.0, 0.0], cubes: S6 },
    Bone { parent: None, name: "sf_wing_big", pivot: [0.0, 16.0, 1.0], rotation: [0.0, 0.0, 0.0], cubes: WING_BIG },
    Bone { parent: None, name: "sf_wing_mid", pivot: [0.0, 20.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: WING_MID },
    Bone { parent: None, name: "sf_wing_small", pivot: [0.0, 19.0, -1.5], rotation: [0.0, 0.0, 0.0], cubes: WING_SMALL },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
