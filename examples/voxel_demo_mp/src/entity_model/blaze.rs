//! W379 - blaze, MC `BlazeModel.createBodyLayer` verbatim.
//!
//! Head 8x8x8 at the origin + twelve 2x8x2 rods in three rings:
//! radius 9 (angles 0/90/180/270, y ~ -2+cos), radius 7 (45-degree
//! offset, y ~ 2+cos) and radius 5 (y ~ 11+cos). The baked pose
//! freezes each rod at its vanilla rest position - the per-tick
//! bob/spin is a render-side follow-up. Sheet `entity/blaze.png`.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] =
    &[McCube::new([-4.0, -4.0, -4.0], [8.0, 8.0, 8.0], [0, 0])];
const ROD: &[McCube] =
    &[McCube::new([0.0, 0.0, 0.0], [2.0, 8.0, 2.0], [0, 16])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "blaze_head", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Ring 1: radius 9, angles 0/90/180/270.
    Bone { parent: None, name: "blaze_rod_0", pivot: [9.0, -1.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_1", pivot: [0.0, -1.12, 9.0], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_2", pivot: [-9.0, -1.46, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_3", pivot: [0.0, -1.93, -9.0], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    // Ring 2: radius 7, 45-degree offset.
    Bone { parent: None, name: "blaze_rod_4", pivot: [4.95, 1.58, 4.95], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_5", pivot: [-4.95, 1.20, 4.95], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_6", pivot: [-4.95, 1.01, -4.95], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_7", pivot: [4.95, 1.06, -4.95], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    // Ring 3: radius 5, high band.
    Bone { parent: None, name: "blaze_rod_8", pivot: [3.54, 11.96, 3.54], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_9", pivot: [-3.54, 11.90, 3.54], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_10", pivot: [-3.54, 11.35, -3.54], rotation: [0.0, 0.0, 0.0], cubes: ROD },
    Bone { parent: None, name: "blaze_rod_11", pivot: [3.54, 10.62, -3.54], rotation: [0.0, 0.0, 0.0], cubes: ROD },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
