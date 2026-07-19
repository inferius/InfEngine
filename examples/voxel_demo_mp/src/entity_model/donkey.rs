//! W344 - donkey, MC `DonkeyModel` (26.1.2): the equine body mesh
//! with the long donkey ears swapped in and the two pannier chests
//! hung on the body, all under `MeshTransformer.scaling(0.87)`.
//! Texture `entity/horse/donkey.png` (64x64).
//!
//! Bones reuse the `horse_` prefixes so the equine gait + look pose
//! fire unchanged; the donkey-only parts (ears, chests) parent onto
//! the shared bones and simply follow.

use super::horse::{
    BODY, LEG_FRONT_L, LEG_FRONT_R, LEG_HIND_L, LEG_HIND_R, MANE, MOUTH,
    NECK, SKULL, TAIL,
};
use super::{Bone, EntityModel, McCube};

const FRAC_PI_6: f32 = std::f32::consts::FRAC_PI_6;
/// PI/12 - the donkey ear splay.
const EAR_TILT: f32 = std::f32::consts::PI / 12.0;

/// texOffs(0,12) box [-1,-7,0] 2x7x1 - the tall donkey ear.
const D_EAR: &[McCube] =
    &[McCube::new([-1.0, -7.0, 0.0], [2.0, 7.0, 1.0], [0, 12])];
/// texOffs(26,21) box [-4,0,-2] 8x8x3 - one pannier chest.
const CHEST: &[McCube] =
    &[McCube::new([-4.0, 0.0, -2.0], [8.0, 8.0, 3.0], [26, 21])];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "horse_body", pivot: [0.0, 11.0, 5.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: Some("horse_body"), name: "horse_tail", pivot: [0.0, -5.0, 2.0], rotation: [FRAC_PI_6, 0.0, 0.0], cubes: TAIL },
    // DonkeyModel.modifyMesh: chests at (+-6, -8, 0), yaw -+PI/2.
    Bone { parent: Some("horse_body"), name: "donkey_chest_l", pivot: [6.0, -8.0, 0.0], rotation: [0.0, -std::f32::consts::FRAC_PI_2, 0.0], cubes: CHEST },
    Bone { parent: Some("horse_body"), name: "donkey_chest_r", pivot: [-6.0, -8.0, 0.0], rotation: [0.0, std::f32::consts::FRAC_PI_2, 0.0], cubes: CHEST },
    Bone { parent: None, name: "horse_head", pivot: [0.0, 4.0, -12.0], rotation: [FRAC_PI_6, 0.0, 0.0], cubes: NECK },
    Bone { parent: Some("horse_head"), name: "horse_skull", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: SKULL },
    Bone { parent: Some("horse_head"), name: "horse_mane", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: MANE },
    Bone { parent: Some("horse_head"), name: "horse_mouth", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: MOUTH },
    // modifyMesh ears: (+-1.25, -10, 4), xRot PI/12, zRot -+PI/12.
    Bone { parent: Some("horse_head"), name: "donkey_ear_l", pivot: [1.25, -10.0, 4.0], rotation: [EAR_TILT, 0.0, EAR_TILT], cubes: D_EAR },
    Bone { parent: Some("horse_head"), name: "donkey_ear_r", pivot: [-1.25, -10.0, 4.0], rotation: [EAR_TILT, 0.0, -EAR_TILT], cubes: D_EAR },
    Bone { parent: None, name: "horse_leg_lh", pivot: [4.0, 14.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND_L },
    Bone { parent: None, name: "horse_leg_rh", pivot: [-4.0, 14.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_HIND_R },
    Bone { parent: None, name: "horse_leg_lf", pivot: [4.0, 14.0, -10.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT_L },
    Bone { parent: None, name: "horse_leg_rf", pivot: [-4.0, 14.0, -10.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_FRONT_R },
];

/// DONKEY_SCALE = 0.87.
pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 0.87, tex_grid: None };
