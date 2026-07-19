//! W162 - the rabbit, transcribed FAITHFULLY from MC
//! `AdultRabbitModel.createBodyLayer` (texture 64x64) via the
//! `tools/mcmodel.py` extractor.
//!
//! This is the first NESTED model: MC parents the ears under the
//! head, the head/tail/front-legs under the (tilted) body, and the
//! hind haunches under hind-leg groups under their own root group.
//! The W162 bone-parent support composes those chains, so the small
//! relative offsets land correctly instead of underground.
//!
//! Group bones (`frontlegs`, `backlegs`, `*_hind_leg`) carry no
//! cubes - they are pure transform nodes, exactly like MC's empty
//! `PartDefinition` groups.

use std::f32::consts::FRAC_PI_8;

use super::{Bone, EntityModel, McCube};

// MC's `AdultRabbitModel` writes the rounded literal `0.3927F` for
// every part tilt; that is pi/8 to 4 decimals, so use the exact
// constant (clippy's `approx_constant` rejects the literal anyway).
const T: f32 = FRAC_PI_8;

const BODY: &[McCube] = &[McCube::new([-4.0, -6.0, -9.0], [8.0, 6.0, 10.0], [0, 0])];
const TAIL: &[McCube] = &[McCube::new([-2.0, -3.0084, -1.0125], [4.0, 4.0, 4.0], [20, 16])];
const HEAD: &[McCube] = &[McCube::new([-2.5, -3.0, -4.0], [5.0, 5.0, 5.0], [0, 16])];
const EAR_L: &[McCube] = &[McCube::new([-1.0, -4.2929, -0.1213], [2.0, 5.0, 1.0], [32, 0])];
const EAR_R: &[McCube] = &[McCube::new([-1.0, -4.2929, -0.1213], [2.0, 5.0, 1.0], [26, 0])];
const FRONT_R: &[McCube] = &[McCube::new([-0.9, -1.0, -0.9], [2.0, 4.0, 2.0], [36, 18])];
const FRONT_L: &[McCube] = &[McCube::new([-1.0, -1.0, -1.0], [2.0, 4.0, 2.0], [44, 18])];
const HAUNCH_R: &[McCube] = &[McCube::new([-1.0, 0.0, -5.0], [2.0, 1.0, 6.0], [20, 24])];
const HAUNCH_L: &[McCube] = &[McCube::new([-1.0, 0.0, -5.0], [2.0, 1.0, 6.0], [36, 24])];

const NONE: &[McCube] = &[];

const BONES: &[Bone] = &[
    // Body sits near the feet and tilts back (-0.3927 xRot); every
    // upper part hangs off it.
    Bone { parent: None, name: "rabbit_body", pivot: [0.0, 23.0, 4.0], rotation: [-T, 0.0, 0.0], cubes: BODY },
    Bone { parent: Some("rabbit_body"), name: "rabbit_tail", pivot: [0.0, -4.9916, 0.0125], rotation: [0.0, 0.0, 0.0], cubes: TAIL },
    Bone { parent: Some("rabbit_body"), name: "rabbit_head", pivot: [0.0, -5.2929, -8.1213], rotation: [T, 0.0, 0.0], cubes: HEAD },
    Bone { parent: Some("rabbit_head"), name: "rabbit_ear_l", pivot: [1.5, -3.7071, -0.8787], rotation: [0.0, 0.0, 0.0], cubes: EAR_L },
    Bone { parent: Some("rabbit_head"), name: "rabbit_ear_r", pivot: [-1.5, -3.7071, -0.8787], rotation: [0.0, 0.0, 0.0], cubes: EAR_R },
    Bone { parent: Some("rabbit_body"), name: "rabbit_frontlegs", pivot: [0.0, -1.5349, -6.3108], rotation: [0.0, 0.0, 0.0], cubes: NONE },
    Bone { parent: Some("rabbit_frontlegs"), name: "rabbit_front_r", pivot: [-2.0, 1.9239, 0.3827], rotation: [T, 0.0, 0.0], cubes: FRONT_R },
    Bone { parent: Some("rabbit_frontlegs"), name: "rabbit_front_l", pivot: [2.0, 1.9239, 0.4827], rotation: [T, 0.0, 0.0], cubes: FRONT_L },
    // Hind legs hang off their own root group (absolute offset).
    Bone { parent: None, name: "rabbit_backlegs", pivot: [0.0, 23.0, 4.0], rotation: [0.0, 0.0, 0.0], cubes: NONE },
    Bone { parent: Some("rabbit_backlegs"), name: "rabbit_hind_r", pivot: [-3.0, 0.5, 0.0], rotation: [0.0, 0.0, 0.0], cubes: NONE },
    Bone { parent: Some("rabbit_hind_r"), name: "rabbit_haunch_r", pivot: [0.0, -0.5, 0.0], rotation: [0.0, T, 0.0], cubes: HAUNCH_R },
    Bone { parent: Some("rabbit_backlegs"), name: "rabbit_hind_l", pivot: [3.0, 0.5, 0.0], rotation: [0.0, 0.0, 0.0], cubes: NONE },
    Bone { parent: Some("rabbit_hind_l"), name: "rabbit_haunch_l", pivot: [0.0, -0.5, 0.0], rotation: [0.0, -T, 0.0], cubes: HAUNCH_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
