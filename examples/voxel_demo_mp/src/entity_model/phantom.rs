//! W382 - phantom, MC `PhantomModel.createBodyLayer` verbatim
//! (sheet `entity/phantom.png`, 64x64). Slim 5x3x9 body pitched
//! -0.1 rad, two-segment tail, and the wide two-segment wings
//! (base 6x2x9 + tip 13x1x9) canted +-0.1 rad. Flap animation is a
//! render follow-up; the glide silhouette reads instantly.

use super::{Bone, EntityModel, McCube};

const BODY: &[McCube] =
    &[McCube::new([-3.0, -2.0, -8.0], [5.0, 3.0, 9.0], [0, 8])];
const TAIL_BASE: &[McCube] =
    &[McCube::new([-2.0, 0.0, 0.0], [3.0, 2.0, 6.0], [3, 20])];
const TAIL_TIP: &[McCube] =
    &[McCube::new([-1.0, 0.0, 0.0], [1.0, 1.0, 6.0], [4, 29])];
const WING_BASE_R: &[McCube] =
    &[McCube::new([-6.0, 0.0, 0.0], [6.0, 2.0, 9.0], [23, 12]).mirrored()];
const WING_TIP_R: &[McCube] =
    &[McCube::new([-13.0, 0.0, 0.0], [13.0, 1.0, 9.0], [16, 24]).mirrored()];
const WING_BASE_L: &[McCube] =
    &[McCube::new([0.0, 0.0, 0.0], [6.0, 2.0, 9.0], [23, 12])];
const WING_TIP_L: &[McCube] =
    &[McCube::new([0.0, 0.0, 0.0], [13.0, 1.0, 9.0], [16, 24])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "phantom_body", pivot: [0.0, 0.0, 0.0], rotation: [-0.1, 0.0, 0.0], cubes: BODY },
    Bone { parent: Some("phantom_body"), name: "phantom_tail_base", pivot: [0.0, -2.0, 1.0], rotation: [0.0, 0.0, 0.0], cubes: TAIL_BASE },
    Bone { parent: Some("phantom_tail_base"), name: "phantom_tail_tip", pivot: [0.0, 0.5, 6.0], rotation: [0.0, 0.0, 0.0], cubes: TAIL_TIP },
    Bone { parent: Some("phantom_body"), name: "phantom_wing_l", pivot: [2.0, -2.0, -8.0], rotation: [0.0, 0.0, 0.1], cubes: WING_BASE_L },
    Bone { parent: Some("phantom_wing_l"), name: "phantom_wing_tip_l", pivot: [6.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.1], cubes: WING_TIP_L },
    Bone { parent: Some("phantom_body"), name: "phantom_wing_r", pivot: [-3.0, -2.0, -8.0], rotation: [0.0, 0.0, -0.1], cubes: WING_BASE_R },
    Bone { parent: Some("phantom_wing_r"), name: "phantom_wing_tip_r", pivot: [-6.0, 0.0, 0.0], rotation: [0.0, 0.0, -0.1], cubes: WING_TIP_R },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
