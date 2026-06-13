//! W161 - the polar bear, transcribed FAITHFULLY from MC
//! `PolarBearModel.createBodyLayer` (texture 128x64).
//!
//! Extracted directly from the client jar bytecode (the
//! `tools/mcmodel.py` .class reader), not guessed: head carries the
//! skull, mouth and two ears; the torso splits into the rotated body
//! plus the hump; four 4x10 legs reuse one builder per pair like
//! every MC quadruped. Bone names carry the quadruped leg suffixes
//! (rh, lh, rf, lf) so the W147 walk animation drives the gait
//! unchanged.
//!
//! Locked against the real texture by
//! `w158_every_model_unwrap_lands_on_art`.

use std::f32::consts::FRAC_PI_2;

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    McCube::new([-3.5, -3.0, -3.0], [7.0, 7.0, 7.0], [0, 0]),
    McCube::new([-2.5, 1.0, -6.0], [5.0, 3.0, 3.0], [0, 44]),
    McCube::new([-4.5, -4.0, -1.0], [2.0, 2.0, 1.0], [26, 0]),
    McCube::new([2.5, -4.0, -1.0], [2.0, 2.0, 1.0], [26, 0]).mirrored(),
];
const BODY: &[McCube] = &[
    McCube::new([-5.0, -13.0, -7.0], [14.0, 14.0, 11.0], [0, 19]),
    McCube::new([-4.0, -25.0, -7.0], [12.0, 12.0, 10.0], [39, 0]),
];
const LEG_HIND: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 10.0, 8.0], [50, 22])];
const LEG_FRONT: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 10.0, 6.0], [50, 40])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "polar_bear_head", pivot: [0.0, 10.0, -16.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // Body lies along Z (offsetAndRotation xRot = PI/2), MC-style.
    Bone { parent: None, name: "polar_bear_body", pivot: [-2.0, 9.0, 12.0], rotation: [FRAC_PI_2, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "polar_bear_leg_rh", pivot: [-4.5, 14.0, 6.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND },
    Bone { parent: None, name: "polar_bear_leg_lh", pivot: [4.5, 14.0, 6.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND },
    Bone { parent: None, name: "polar_bear_leg_rf", pivot: [-3.5, 14.0, -8.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT },
    Bone { parent: None, name: "polar_bear_leg_lf", pivot: [3.5, 14.0, -8.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
