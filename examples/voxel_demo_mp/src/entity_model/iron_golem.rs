//! W322 - the iron golem, ported faithfully from MC
//! `IronGolemModel.createBodyLayer` (26.1.2), texture
//! `entity/iron_golem/iron_golem.png` 128x128.
//!
//! Six flat bones: head (skull + nose), body (chest + the inflated
//! 0.5 kilt box), both arms, both legs (left leg mirrors the right's
//! unwrap direction like MC's `.mirror()` builder). The golem's
//! triangle-wave stride + arm swing live in
//! `animation::pose_for_bone` under the `golem_` prefix - MC's
//! `setupAnim` uses `Mth.triangleWave(pos, 13)` instead of the
//! humanoid cosine walk.

use super::{Bone, EntityModel, McCube};

const HEAD: &[McCube] = &[
    McCube::new([-4.0, -12.0, -5.5], [8.0, 10.0, 8.0], [0, 0]),
    // Nose.
    McCube::new([-1.0, -5.0, -7.5], [2.0, 4.0, 2.0], [24, 0]),
];
const BODY: &[McCube] = &[
    McCube::new([-9.0, -2.0, -6.0], [18.0, 12.0, 11.0], [0, 40]),
    // Kilt (CubeDeformation 0.5).
    McCube::new([-4.5, 10.0, -3.0], [9.0, 5.0, 6.0], [0, 70]).inflated(0.5),
];
const ARM_R: &[McCube] = &[McCube::new([-13.0, -2.5, -3.0], [4.0, 30.0, 6.0], [60, 21])];
const ARM_L: &[McCube] = &[McCube::new([9.0, -2.5, -3.0], [4.0, 30.0, 6.0], [60, 58])];
const LEG_R: &[McCube] = &[McCube::new([-3.5, -3.0, -3.0], [6.0, 16.0, 5.0], [37, 0])];
const LEG_L: &[McCube] =
    &[McCube::new([-3.5, -3.0, -3.0], [6.0, 16.0, 5.0], [60, 0]).mirrored()];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "golem_head", pivot: [0.0, -7.0, -2.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: None, name: "golem_body", pivot: [0.0, -7.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "golem_arm_r", pivot: [0.0, -7.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_R },
    Bone { parent: None, name: "golem_arm_l", pivot: [0.0, -7.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: ARM_L },
    Bone { parent: None, name: "golem_leg_r", pivot: [-4.0, 11.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_R },
    Bone { parent: None, name: "golem_leg_l", pivot: [5.0, 11.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: LEG_L },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES };
