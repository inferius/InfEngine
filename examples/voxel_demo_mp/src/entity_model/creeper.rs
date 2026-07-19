//! W343 - creeper, MC `CreeperModel.createBodyLayer` verbatim.
//!
//! Head 8^3 at pivot (0,6,0), the 8x12x4 body sharing the pivot,
//! four 4x6x4 legs at (+-2, 18, +-4) all sampling texOffs(0,16).
//! Texture `creeper/creeper.png` (64x32). The quadruped `_leg_rf`
//! etc. suffixes drive the vanilla diagonal gait via
//! `pose_for_bone`; `_head` gets the look pose. The swell flash +
//! inflate live in the render arm (side-channel `mob_swell`), not
//! the model.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] =
    &[McCube::new([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0, 0])];
const BODY: &[McCube] =
    &[McCube::new([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16, 16])];
const LEG: &[McCube] =
    &[McCube::new([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0, 16])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "creeper_head", pivot: [0.0, 6.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: None, name: "creeper_body", pivot: [0.0, 6.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "creeper_leg_rh", pivot: [-2.0, 18.0, 4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "creeper_leg_lh", pivot: [2.0, 18.0, 4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "creeper_leg_rf", pivot: [-2.0, 18.0, -4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "creeper_leg_lf", pivot: [2.0, 18.0, -4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
