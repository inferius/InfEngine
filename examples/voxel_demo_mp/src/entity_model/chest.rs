//! W126 - the chest, ported from MC `ChestModel` (the chest is a
//! BLOCK ENTITY in MC - `ChestRenderer` draws ModelPart boxes with
//! the `entity/chest/*.png` sheets, which is why the old static
//! block-mesh chest had "wrong" plank textures and no lid).
//!
//! MC source data (`ChestModel`): bottom 14x10x14 texOffs(0,19),
//! lid 14x5x14 texOffs(0,0) hinged at the back edge (y=9, z=1),
//! lock 2x4x1 texOffs(0,0) riding on the lid's front; the double
//! chest splits into 15-wide left/right halves with their own
//! sheets (`normal_left.png` / `normal_right.png`).
//!
//! Coordinates here are expressed in the engine's entity-model
//! convention (the same Y-down MC pixel space every mob model
//! uses; `bone_matrix` flips X+Y and scales 1/16). The boxes were
//! derived so the world-space result matches the W59.1 static
//! chest exactly: base (1,0,1)..(15,10,15), lid (1,9,1)..(15,14,15),
//! latch on the front face. The entity position is the block's
//! bottom-center; the FRONT (latch) faces local -Z at yaw 0.
//!
//! Lid animation: rotate the `chest_lid` bone around X at its
//! hinge pivot (MC: `lidAngle` eased by `1-(1-t)^3`, max PI/2).

use super::{Bone, EntityModel, McCube};

// --- Single chest (entity/chest/normal.png, 64x64) -----------------

const BASE: &[McCube] = &[McCube::new([-7.0, -10.0, -7.0], [14.0, 10.0, 14.0], [0, 19])];

const LID: &[McCube] = &[
    // Lid slab: world (1,9,1)..(15,14,15) relative to the hinge
    // pivot at the back-top of the base.
    McCube::new([-7.0, -5.0, -14.0], [14.0, 5.0, 14.0], [0, 0]),
    // Latch knob on the front face, swinging with the lid.
    McCube::new([-1.0, -2.0, -15.0], [2.0, 4.0, 1.0], [0, 0]),
];

const BONES: &[Bone] = &[
    Bone { parent: None, name: "chest_base", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BASE },
    // Hinge: world (x, 9/16, +7/16) = the top-back edge of the base.
    Bone { parent: None, name: "chest_lid", pivot: [0.0, -9.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LID },
];

pub(crate) const MODEL: EntityModel = EntityModel { bones: BONES, scale: 1.0 };

// --- Double-chest halves (normal_left.png / normal_right.png) ------
//
// Each half is 15 px wide and runs flush to the cell edge that
// touches its partner. "EXT_POS" extends toward world +X at yaw 0
// (partner on that side); "EXT_NEG" toward -X. Which MC sheet
// (left/right) belongs to which side follows MC's naming as seen
// facing the chest front.

const BASE_EXT_POS: &[McCube] = &[McCube::new([-8.0, -10.0, -7.0], [15.0, 10.0, 14.0], [0, 19])];
const LID_EXT_POS: &[McCube] = &[
    McCube::new([-8.0, -5.0, -14.0], [15.0, 5.0, 14.0], [0, 0]),
    // Half-latch at the seam edge.
    McCube::new([-8.0, -2.0, -15.0], [1.0, 4.0, 1.0], [0, 0]),
];

const BASE_EXT_NEG: &[McCube] = &[McCube::new([-7.0, -10.0, -7.0], [15.0, 10.0, 14.0], [0, 19])];
const LID_EXT_NEG: &[McCube] = &[
    McCube::new([-7.0, -5.0, -14.0], [15.0, 5.0, 14.0], [0, 0]),
    McCube::new([7.0, -2.0, -15.0], [1.0, 4.0, 1.0], [0, 0]),
];

const BONES_EXT_POS: &[Bone] = &[
    Bone { parent: None, name: "chest_base", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BASE_EXT_POS },
    Bone { parent: None, name: "chest_lid", pivot: [0.0, -9.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LID_EXT_POS },
];

const BONES_EXT_NEG: &[Bone] = &[
    Bone { parent: None, name: "chest_base", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: BASE_EXT_NEG },
    Bone { parent: None, name: "chest_lid", pivot: [0.0, -9.0, 7.0], rotation: [0.0, 0.0, 0.0], cubes: LID_EXT_NEG },
];

/// Half whose 15-wide box extends toward local +X (world +X at
/// yaw 0). Bakes with `normal_left.png`.
pub(crate) const MODEL_EXT_POS: EntityModel = EntityModel { bones: BONES_EXT_POS, scale: 1.0 };

/// Half extending toward local -X. Bakes with `normal_right.png`.
pub(crate) const MODEL_EXT_NEG: EntityModel = EntityModel { bones: BONES_EXT_NEG, scale: 1.0 };
