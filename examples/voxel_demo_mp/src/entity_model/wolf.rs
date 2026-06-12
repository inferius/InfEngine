//! W158 - the wolf, ported from MC `WolfModel` (texture 64x32).
//!
//! Classic quadruped layout: head bone carries the snout + both
//! ears, the torso splits into the rotated `body` + the chest
//! `mane` (MC's upperBody), four 2x8x2 legs share one tile, and
//! the tail hangs at MC's resting angle. Bone names follow the
//! `_leg_rh/lh/rf/lf` suffixes so the W147 walk animation drives
//! the gait with no extra wiring.
//!
//! Every cube's unwrap is locked against the real wolf.png by the
//! `w158_every_model_unwrap_lands_on_art` coverage test.

use std::f32::consts::FRAC_PI_2;

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    // Skull, snout, right ear, left ear (one bone, MC parts merged).
    McCube::new([-2.0, -3.0, -2.0], [6.0, 6.0, 4.0], [0, 0]),
    McCube::new([-1.5, 0.0, -5.0], [3.0, 3.0, 4.0], [0, 10]),
    McCube::new([-3.0, -5.0, 0.0], [2.0, 2.0, 1.0], [16, 14]),
    McCube::new([1.0, -5.0, 0.0], [2.0, 2.0, 1.0], [16, 14]),
];
const BODY: &[McCube] = &[McCube::new([-3.0, -2.0, -3.0], [6.0, 9.0, 6.0], [18, 14])];
// MC `upperBody` - the fluffy chest mane in front of the torso.
const MANE: &[McCube] = &[McCube::new([-3.0, -3.0, -3.0], [8.0, 6.0, 7.0], [21, 0])];
const LEG: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], [0, 18])];
const TAIL: &[McCube] = &[McCube::new([-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], [9, 18])];

const BONES: &[Bone] = &[
    Bone { name: "wolf_head", pivot: [-1.0, 13.5, -7.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Torso lies along Z (offsetAndRotation xRot = PI/2), MC-style.
    Bone { name: "wolf_body", pivot: [0.0, 14.0, 2.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { name: "wolf_mane", pivot: [-1.0, 14.0, -3.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: MANE },
    Bone { name: "wolf_leg_rh", pivot: [-2.5, 16.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "wolf_leg_lh", pivot: [0.5, 16.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "wolf_leg_rf", pivot: [-2.5, 16.0, -4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    Bone { name: "wolf_leg_lf", pivot: [0.5, 16.0, -4.0], rotation: [0.0, 0.0, 0.0], cubes: LEG },
    // Resting tail angle ~ PI/5 like MC's idle pose.
    Bone { name: "wolf_tail", pivot: [-1.0, 12.0, 8.0], rotation: [0.628, 0.0, 0.0], cubes: TAIL },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
