//! W327 - the magma cube, ported faithfully from MC
//! `MagmaCubeModel.createBodyLayer` (26.1.2), texture
//! `entity/slime/magmacube.png` 64x64.
//!
//! Eight 8x1x8 lava segments stacked at MC y 16..24 plus the 4^3
//! molten core - the Java loop's texOffs walk (rows 0-3 at u 0,
//! rows 4-7 at u 32) is unrolled below. The squish animation
//! (segments spreading on land) is a follow-up; the whole model
//! ships at scale 2.0 = MC's medium (size 2) cube.

use super::{Bone, EntityModel, McCube};

const SEGMENTS: &[McCube] = &[
    McCube::new([-4.0, 16.0, -4.0], [8.0, 1.0, 8.0], [0, 0]),
    McCube::new([-4.0, 17.0, -4.0], [8.0, 1.0, 8.0], [0, 9]),
    McCube::new([-4.0, 18.0, -4.0], [8.0, 1.0, 8.0], [0, 18]),
    McCube::new([-4.0, 19.0, -4.0], [8.0, 1.0, 8.0], [0, 27]),
    McCube::new([-4.0, 20.0, -4.0], [8.0, 1.0, 8.0], [32, 0]),
    McCube::new([-4.0, 21.0, -4.0], [8.0, 1.0, 8.0], [32, 9]),
    McCube::new([-4.0, 22.0, -4.0], [8.0, 1.0, 8.0], [32, 18]),
    McCube::new([-4.0, 23.0, -4.0], [8.0, 1.0, 8.0], [32, 27]),
];
const CORE: &[McCube] = &[McCube::new([-2.0, 18.0, -2.0], [4.0, 4.0, 4.0], [24, 40])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "magma_body", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: SEGMENTS },
    Bone { parent: None, name: "magma_core", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: CORE },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 2.0, tex_grid: None };
