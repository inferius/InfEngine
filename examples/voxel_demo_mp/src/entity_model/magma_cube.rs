//! W327 - the magma cube, ported faithfully from MC
//! `MagmaCubeModel.createBodyLayer` (26.1.2), texture
//! `entity/slime/magmacube.png` 64x64.
//!
//! Eight 8x1x8 lava segments stacked at MC y 16..24 plus the 4^3
//! molten core - the Java loop's texOffs walk (rows 0-3 at u 0,
//! rows 4-7 at u 32) is unrolled below. W336: the squish spread is
//! live via `ambient_pose` (per-segment bones); the whole model
//! ships at scale 2.0 = MC's medium (size 2) cube.

use super::{Bone, EntityModel, McCube};

// W336: one bone per segment (MC cube0..cube7) so the squish anim
// can spread them independently - `ambient_pose` writes each bone's
// y offset (-(4-i)*squish*1.7).
const S0: &[McCube] = &[McCube::new([-4.0, 16.0, -4.0], [8.0, 1.0, 8.0], [0, 0])];
const S1: &[McCube] = &[McCube::new([-4.0, 17.0, -4.0], [8.0, 1.0, 8.0], [0, 9])];
const S2: &[McCube] = &[McCube::new([-4.0, 18.0, -4.0], [8.0, 1.0, 8.0], [0, 18])];
const S3: &[McCube] = &[McCube::new([-4.0, 19.0, -4.0], [8.0, 1.0, 8.0], [0, 27])];
const S4: &[McCube] = &[McCube::new([-4.0, 20.0, -4.0], [8.0, 1.0, 8.0], [32, 0])];
const S5: &[McCube] = &[McCube::new([-4.0, 21.0, -4.0], [8.0, 1.0, 8.0], [32, 9])];
const S6: &[McCube] = &[McCube::new([-4.0, 22.0, -4.0], [8.0, 1.0, 8.0], [32, 18])];
const S7: &[McCube] = &[McCube::new([-4.0, 23.0, -4.0], [8.0, 1.0, 8.0], [32, 27])];
const CORE: &[McCube] = &[McCube::new([-2.0, 18.0, -2.0], [4.0, 4.0, 4.0], [24, 40])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "magma_cube_0", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: S0 },
    Bone { parent: None, name: "magma_cube_1", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: S1 },
    Bone { parent: None, name: "magma_cube_2", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: S2 },
    Bone { parent: None, name: "magma_cube_3", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: S3 },
    Bone { parent: None, name: "magma_cube_4", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: S4 },
    Bone { parent: None, name: "magma_cube_5", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: S5 },
    Bone { parent: None, name: "magma_cube_6", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: S6 },
    Bone { parent: None, name: "magma_cube_7", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: S7 },
    Bone { parent: None, name: "magma_core", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: CORE },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 2.0, tex_grid: None };
