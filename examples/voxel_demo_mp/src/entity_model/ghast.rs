//! W327 - the ghast, ported faithfully from MC
//! `GhastModel.createBodyLayer` (26.1.2), texture
//! `entity/ghast/ghast.png` 64x32, whole-model scale 4.5
//! (`MeshTransformer.scaling(4.5F)`).
//!
//! Body is one 16^3 box; the nine tentacles hang from fixed grid
//! offsets with per-tentacle lengths MC rolls from a SEEDED
//! RandomSource (1660) at layer-build time - the sequence is
//! deterministic, so the lengths below are those exact rolls
//! (`random.nextInt(7) + 8` nine times): 8 13 9 11 11 10 12 9 12.
//! MC's setupAnim sways each tentacle around a 0.4 rad rest pitch
//! (`0.2*sin(age*0.3+i)+0.4`); the rest pitch is baked here and
//! W336's `ambient_pose` layers the time-driven sway on top.

use super::{Bone, EntityModel, McCube};

const BODY: &[McCube] = &[McCube::new([-8.0, -8.0, -8.0], [16.0, 16.0, 16.0], [0, 0])];

// One 2xLENx2 hanging box per tentacle, all sharing texOffs(0,0).
const T0: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], [0, 0])];
const T1: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 13.0, 2.0], [0, 0])];
const T2: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 9.0, 2.0], [0, 0])];
const T3: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 11.0, 2.0], [0, 0])];
const T4: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 11.0, 2.0], [0, 0])];
const T5: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 10.0, 2.0], [0, 0])];
const T6: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 12.0, 2.0], [0, 0])];
const T7: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 9.0, 2.0], [0, 0])];
const T8: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 12.0, 2.0], [0, 0])];

/// MC rest sway pitch (`+0.4` term of animateTentacles).
const TENTACLE_REST: [f32; 3] = [0.4, 0.0, 0.0];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "ghast_body", pivot: [0.0, 17.6, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    // Offsets from the Java grid formula: xo over {-3.75,1.25,6.25}
    // / {-6.25,-1.25,3.75} alternating rows, yo in {-5,0,5}.
    Bone { parent: None, name: "ghast_tentacle_0", pivot: [-3.75, 24.6, -5.0], rotation: TENTACLE_REST, cubes: T0 },
    Bone { parent: None, name: "ghast_tentacle_1", pivot: [1.25, 24.6, -5.0], rotation: TENTACLE_REST, cubes: T1 },
    Bone { parent: None, name: "ghast_tentacle_2", pivot: [6.25, 24.6, -5.0], rotation: TENTACLE_REST, cubes: T2 },
    Bone { parent: None, name: "ghast_tentacle_3", pivot: [-6.25, 24.6, 0.0], rotation: TENTACLE_REST, cubes: T3 },
    Bone { parent: None, name: "ghast_tentacle_4", pivot: [-1.25, 24.6, 0.0], rotation: TENTACLE_REST, cubes: T4 },
    Bone { parent: None, name: "ghast_tentacle_5", pivot: [3.75, 24.6, 0.0], rotation: TENTACLE_REST, cubes: T5 },
    Bone { parent: None, name: "ghast_tentacle_6", pivot: [-3.75, 24.6, 5.0], rotation: TENTACLE_REST, cubes: T6 },
    Bone { parent: None, name: "ghast_tentacle_7", pivot: [1.25, 24.6, 5.0], rotation: TENTACLE_REST, cubes: T7 },
    Bone { parent: None, name: "ghast_tentacle_8", pivot: [6.25, 24.6, 5.0], rotation: TENTACLE_REST, cubes: T8 },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 4.5, tex_grid: Some((64, 32)) };
