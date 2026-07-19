//! W134 - minecart, ported faithfully from MC `MinecartModel`
//! (`createBodyLayer`, texture 64x32): five 2-px plates - a flat
//! bottom (a wall plate laid down by a +90 X rotation) and four
//! walls yawed around the rim. Pure `ModelPart` data through the
//! shared W91 pipeline; no eyeballed numbers.

use std::f32::consts::{FRAC_PI_2, PI};

use super::{Bone, EntityModel, McCube};

/// MC `(float)Math.PI * 1.5F` (the front wall's yaw).
const PI_1_5: f32 = PI + FRAC_PI_2;

// texOffs(0,10), addBox(-10, -8, -1, 20, 16, 2).
const BOTTOM: &[McCube] =
    &[McCube::new([-10.0, -8.0, -1.0], [20.0, 16.0, 2.0], [0, 10])];
// texOffs(0,0), addBox(-8, -9, -1, 16, 8, 2) - shared by all walls.
const WALL: &[McCube] =
    &[McCube::new([-8.0, -9.0, -1.0], [16.0, 8.0, 2.0], [0, 0])];

const BONES: &[Bone] = &[
    // offsetAndRotation(0, 4, 0, PI/2, 0, 0).
    Bone {
        parent: None, name: "cart_bottom",
        pivot: [0.0, 4.0, 0.0],
        rotation: [FRAC_PI_2, 0.0, 0.0],
        cubes: BOTTOM,
    },
    // offsetAndRotation(-9, 4, 0, 0, PI*1.5, 0).
    Bone {
        parent: None, name: "cart_front",
        pivot: [-9.0, 4.0, 0.0],
        rotation: [0.0, PI_1_5, 0.0],
        cubes: WALL,
    },
    // offsetAndRotation(9, 4, 0, 0, PI/2, 0).
    Bone {
        parent: None, name: "cart_back",
        pivot: [9.0, 4.0, 0.0],
        rotation: [0.0, FRAC_PI_2, 0.0],
        cubes: WALL,
    },
    // offsetAndRotation(0, 4, -7, 0, PI, 0).
    Bone {
        parent: None, name: "cart_left",
        pivot: [0.0, 4.0, -7.0],
        rotation: [0.0, PI, 0.0],
        cubes: WALL,
    },
    // offset(0, 4, 7).
    Bone {
        parent: None, name: "cart_right",
        pivot: [0.0, 4.0, 7.0],
        rotation: [0.0, 0.0, 0.0],
        cubes: WALL,
    },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
