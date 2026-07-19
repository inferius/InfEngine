//! W302 - the villager, ported faithfully from MC
//! `VillagerModel.createBodyModel` (26.1.2), texture
//! `entity/villager/villager.png` 64x64.
//!
//! Head is a 10-px-tall box; the nose and the hat overlay (inflate
//! 0.51) fold into the head bone (their MC pivots are ZERO / a plain
//! offset, so the cubes inline with adjusted origins). The hat RIM is
//! its own child bone - MC rotates it -90 deg around X; the base
//! villager skin leaves both hat tiles transparent, so they render
//! only for profession skins (cutout, like the piglin's missing ear).
//! The arms are ONE bone at MC's folded rest pose (xRot -0.75) - the
//! `villager_arms` name matches no animation rule, so they stay
//! crossed while walking, exactly like vanilla. The 20-px jacket
//! overlay (the robe skirt) rides the body bone.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    McCube::new([-4.0, -10.0, -4.0], [8.0, 10.0, 8.0], [0, 0]),
    // Hat overlay - same box, CubeDeformation 0.51.
    McCube::new([-4.0, -10.0, -4.0], [8.0, 10.0, 8.0], [32, 0]).inflated(0.51),
    // Nose: MC child at offset (0,-2,0), cube (-1,-1,-6) -> inlined.
    McCube::new([-1.0, -3.0, -6.0], [2.0, 4.0, 2.0], [24, 0]),
];
const HAT_RIM: &[McCube] =
    &[McCube::new([-8.0, -8.0, -6.0], [16.0, 16.0, 1.0], [30, 47])];
const BODY: &[McCube] = &[
    McCube::new([-4.0, 0.0, -3.0], [8.0, 12.0, 6.0], [16, 20]),
    // The robe/jacket overlay - 20 px tall, skirt past the waist.
    McCube::new([-4.0, 0.0, -3.0], [8.0, 20.0, 6.0], [0, 38]).inflated(0.5),
];
const ARMS: &[McCube] = &[
    McCube::new([-8.0, -2.0, -2.0], [4.0, 8.0, 4.0], [44, 22]),
    McCube::new([4.0, -2.0, -2.0], [4.0, 8.0, 4.0], [44, 22]).mirrored(),
    McCube::new([-4.0, 2.0, -2.0], [8.0, 4.0, 4.0], [40, 38]),
];
const LEG_R: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 22])];
const LEG_L: &[McCube] =
    &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 22]).mirrored()];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "villager_head", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // MC: hat_rim under hat (pivot ZERO) rotated -PI/2 around X.
    Bone { parent: Some("villager_head"), name: "villager_hat_rim", pivot: [0.0, 0.0, 0.0], rotation: [-std::f32::consts::FRAC_PI_2, 0.0, 0.0], cubes: HAT_RIM },
    Bone { parent: None, name: "villager_body", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    // PartPose.offsetAndRotation(0, 3, -1, -0.75, 0, 0) - folded arms.
    Bone { parent: None, name: "villager_arms", pivot: [0.0, 3.0, -1.0], rotation: [-0.75, 0.0, 0.0], cubes: ARMS },
    Bone { parent: None, name: "villager_leg_r", pivot: [-2.0, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { parent: None, name: "villager_leg_l", pivot: [2.0, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
