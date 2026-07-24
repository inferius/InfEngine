//! W380 - guardian, MC `GuardianModel.createBodyLayer` (body, eye
//! and the three tail segments verbatim; the twelve rotated spikes
//! are a follow-up - their pose math is per-direction and the fish
//! reads fine without them). Sheet `entity/guardian.png` (also the
//! elder tint variant `entity/guardian_elder.png`, drawn at MC's
//! 2.35x scale).

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    McCube::new([-6.0, 10.0, -8.0], [12.0, 12.0, 16.0], [0, 0]),
    McCube::new([-8.0, 10.0, -6.0], [2.0, 12.0, 12.0], [0, 28]),
    McCube::new([6.0, 10.0, -6.0], [2.0, 12.0, 12.0], [0, 28]).mirrored(),
    McCube::new([-6.0, 8.0, -6.0], [12.0, 2.0, 12.0], [16, 40]),
    McCube::new([-6.0, 22.0, -6.0], [12.0, 2.0, 12.0], [16, 40]),
];
const EYE: &[McCube] =
    &[McCube::new([-1.0, 15.0, 0.0], [2.0, 2.0, 1.0], [8, 0])];
const TAIL0: &[McCube] =
    &[McCube::new([-2.0, 14.0, 7.0], [4.0, 4.0, 8.0], [40, 0])];
const TAIL1: &[McCube] =
    &[McCube::new([0.0, 14.0, 0.0], [3.0, 3.0, 7.0], [0, 54])];
const TAIL2: &[McCube] = &[
    McCube::new([0.0, 14.0, 0.0], [2.0, 2.0, 6.0], [41, 32]),
    McCube::new([1.0, 10.5, 3.0], [1.0, 9.0, 9.0], [25, 19]),
];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "guardian_head", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: Some("guardian_head"), name: "guardian_eye", pivot: [0.0, 0.0, -8.25], rotation: [0.0, 0.0, 0.0], cubes: EYE },
    Bone { parent: Some("guardian_head"), name: "guardian_tail0", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: TAIL0 },
    Bone { parent: Some("guardian_tail0"), name: "guardian_tail1", pivot: [-1.5, 0.5, 14.0], rotation: [0.0, 0.0, 0.0], cubes: TAIL1 },
    Bone { parent: Some("guardian_tail1"), name: "guardian_tail2", pivot: [0.5, 0.5, 6.0], rotation: [0.0, 0.0, 0.0], cubes: TAIL2 },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
