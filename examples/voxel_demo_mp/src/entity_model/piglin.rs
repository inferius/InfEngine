//! W300 - the zombified piglin, ported faithfully from MC
//! `AbstractPiglinModel.addHead` + `AdultPiglinModel.createBodyLayer`
//! (26.1.2), texture `entity/piglin/zombified_piglin.png` 64x64.
//!
//! The body/arms/legs come from `PlayerModel.createMesh` (non-slim
//! player layout; the left limbs have their OWN tiles at (32,48) /
//! (16,48) and the pack paints them - the zombified skin's rotted
//! cutouts render via the shared alpha-discard path like the zombie
//! limb holes). The head is the piglin's 10x8x8 wide box plus the
//! snout and both tusks; the ears are CHILD bones of the head with
//! the vanilla +-30 deg z-rest so they follow head look (W162 parent
//! chains). The RIGHT ear tile (39,6) is fully transparent in the
//! vanilla zombified skin - the rotted-off ear - and correctly
//! renders as nothing, exactly like MC's cutout pass.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    // Wide piglin skull.
    McCube::new([-5.0, -8.0, -4.0], [10.0, 8.0, 8.0], [0, 0]),
    // Snout.
    McCube::new([-2.0, -4.0, -5.0], [4.0, 4.0, 1.0], [31, 1]),
    // Left + right tusk.
    McCube::new([2.0, -2.0, -5.0], [1.0, 2.0, 1.0], [2, 4]),
    McCube::new([-3.0, -2.0, -5.0], [1.0, 2.0, 1.0], [2, 0]),
];
const EAR_L: &[McCube] = &[McCube::new([0.0, 0.0, -2.0], [1.0, 5.0, 4.0], [51, 6])];
const EAR_R: &[McCube] = &[McCube::new([-1.0, 0.0, -2.0], [1.0, 5.0, 4.0], [39, 6])];
const BODY: &[McCube] = &[McCube::new([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16, 16])];
const ARM_R: &[McCube] = &[McCube::new([-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], [40, 16])];
const ARM_L: &[McCube] = &[McCube::new([-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], [32, 48])];
const LEG_R: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16])];
const LEG_L: &[McCube] = &[McCube::new([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [16, 48])];

/// `ADULT_EAR_ANGLE_IN_DEGREES = 30` from `AbstractPiglinModel`.
const EAR_ANGLE: f32 = std::f32::consts::PI / 6.0;

const BONES: &[Bone] = &[
    Bone { parent: None, name: "piglin_head", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    // PartPose.offsetAndRotation(+-4.5, -6, 0, 0, 0, -+PI/6).
    Bone { parent: Some("piglin_head"), name: "piglin_ear_l", pivot: [4.5, -6.0, 0.0], rotation: [0.0, 0.0, -EAR_ANGLE], cubes: EAR_L },
    Bone { parent: Some("piglin_head"), name: "piglin_ear_r", pivot: [-4.5, -6.0, 0.0], rotation: [0.0, 0.0, EAR_ANGLE], cubes: EAR_R },
    Bone { parent: None, name: "piglin_body", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "piglin_arm_r", pivot: [-5.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_R },
    Bone { parent: None, name: "piglin_arm_l", pivot: [5.0, 2.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_L },
    Bone { parent: None, name: "piglin_leg_r", pivot: [-1.9, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { parent: None, name: "piglin_leg_l", pivot: [1.9, 12.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
