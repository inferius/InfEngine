//! W388 - squid, MC `SquidModel.createBodyLayer` verbatim (sheet
//! `entity/squid/squid.png`, 64x32; the glow squid reuses the mesh
//! with `entity/glow_squid/glow_squid.png`). Body 12x16x12 inflated
//! 0.02 at pivot (0,8,0); 8 tentacles 2x18x2 on a radius-5 ring at
//! y=15, each yawed `PI/2 - i*PI/4` so its flat side faces out.
//! Tentacle sway rides `ambient_pose` ("squid_tentacle_N").

use super::{Bone, EntityModel, McCube};

const BODY: &[McCube] = &[McCube::new(
    [-6.0, -8.0, -6.0],
    [12.0, 16.0, 12.0],
    [0, 0],
)
.inflated(0.02)];
const TENTACLE: &[McCube] =
    &[McCube::new([-1.0, 0.0, -1.0], [2.0, 18.0, 2.0], [48, 0])];

// cos/sin of i*PI/4 times 5 (the MC ring), yRot = PI/2 - i*PI/4.
const R1: f32 = 3.535_534;
const FRAC_PI_4: f32 = core::f32::consts::FRAC_PI_4;

const BONES: &[Bone] = &[
    Bone { parent: None, name: "squid_body", pivot: [0.0, 8.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "squid_tentacle_0", pivot: [5.0, 15.0, 0.0], rotation: [0.0, 2.0 * FRAC_PI_4, 0.0], cubes: TENTACLE },
    Bone { parent: None, name: "squid_tentacle_1", pivot: [R1, 15.0, R1], rotation: [0.0, FRAC_PI_4, 0.0], cubes: TENTACLE },
    Bone { parent: None, name: "squid_tentacle_2", pivot: [0.0, 15.0, 5.0], rotation: [0.0, 0.0, 0.0], cubes: TENTACLE },
    Bone { parent: None, name: "squid_tentacle_3", pivot: [-R1, 15.0, R1], rotation: [0.0, -FRAC_PI_4, 0.0], cubes: TENTACLE },
    Bone { parent: None, name: "squid_tentacle_4", pivot: [-5.0, 15.0, 0.0], rotation: [0.0, -2.0 * FRAC_PI_4, 0.0], cubes: TENTACLE },
    Bone { parent: None, name: "squid_tentacle_5", pivot: [-R1, 15.0, -R1], rotation: [0.0, -3.0 * FRAC_PI_4, 0.0], cubes: TENTACLE },
    Bone { parent: None, name: "squid_tentacle_6", pivot: [0.0, 15.0, -5.0], rotation: [0.0, -4.0 * FRAC_PI_4, 0.0], cubes: TENTACLE },
    Bone { parent: None, name: "squid_tentacle_7", pivot: [R1, 15.0, -R1], rotation: [0.0, 3.0 * FRAC_PI_4, 0.0], cubes: TENTACLE },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
