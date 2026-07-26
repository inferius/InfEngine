//! W400 - bell body, MC `BellModel.createBodyLayer` verbatim (sheet
//! `entity/bell/bell.png`, 32x32). The stand posts + crossbar live
//! in the chunk mesh (`bell_floor.json` elements); this model is
//! the swinging body MC draws as a block entity: 6x7x6 bell at
//! pivot (8,12,8) with the 8x2x8 lip as its child. The swing
//! animation on ring is a follow-up.

use super::{Bone, EntityModel, McCube};

const BODY: &[McCube] =
    &[McCube::new([-3.0, -6.0, -3.0], [6.0, 7.0, 6.0], [0, 0])];
const BASE: &[McCube] =
    &[McCube::new([4.0, 4.0, 4.0], [8.0, 2.0, 8.0], [0, 13])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "bell_body", pivot: [8.0, 12.0, 8.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: Some("bell_body"), name: "bell_base", pivot: [-8.0, -12.0, -8.0], rotation: [0.0, 0.0, 0.0], cubes: BASE },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
