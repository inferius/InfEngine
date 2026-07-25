//! W391 - parrot, MC `ParrotModel.createBodyLayer` verbatim (sheet
//! `entity/parrot/parrot_red_blue.png`, 32x32). Tiny perched pose:
//! pitched 3x6x3 body, folded wings (yawed PI so the feathers face
//! out), two-part beak + crest feather riding the head. Flight flap
//! + shoulder perching are follow-ups.

use super::{Bone, EntityModel, McCube};

const BODY: &[McCube] =
    &[McCube::new([-1.5, 0.0, -1.5], [3.0, 6.0, 3.0], [2, 8])];
const TAIL: &[McCube] =
    &[McCube::new([-1.5, -1.0, -1.0], [3.0, 4.0, 1.0], [22, 1])];
const WING: &[McCube] =
    &[McCube::new([-0.5, 0.0, -1.5], [1.0, 5.0, 3.0], [19, 8])];
const HEAD: &[McCube] =
    &[McCube::new([-1.0, -1.5, -1.0], [2.0, 3.0, 2.0], [2, 2])];
const HEAD2: &[McCube] =
    &[McCube::new([-1.0, -0.5, -2.0], [2.0, 1.0, 4.0], [10, 0])];
const BEAK1: &[McCube] =
    &[McCube::new([-0.5, -1.0, -0.5], [1.0, 2.0, 1.0], [11, 7])];
const BEAK2: &[McCube] =
    &[McCube::new([-0.5, 0.0, -0.5], [1.0, 2.0, 1.0], [16, 7])];
const FEATHER: &[McCube] =
    &[McCube::new([0.0, -4.0, -2.0], [0.0, 5.0, 4.0], [2, 18])];
const LEG: &[McCube] =
    &[McCube::new([-0.5, 0.0, -0.5], [1.0, 2.0, 1.0], [14, 18])];

const PI: f32 = core::f32::consts::PI;

const BONES: &[Bone] = &[
    Bone { parent: None, name: "parrot_body", pivot: [0.0, 16.5, -3.0], rotation: [0.4937, 0.0, 0.0], cubes: BODY },
    Bone { parent: None, name: "parrot_tail", pivot: [0.0, 21.07, 1.16], rotation: [1.015, 0.0, 0.0], cubes: TAIL },
    Bone { parent: None, name: "parrot_wing_l", pivot: [1.5, 16.94, -2.76], rotation: [-0.6981, -PI, 0.0], cubes: WING },
    Bone { parent: None, name: "parrot_wing_r", pivot: [-1.5, 16.94, -2.76], rotation: [-0.6981, -PI, 0.0], cubes: WING },
    Bone { parent: None, name: "parrot_head", pivot: [0.0, 15.69, -2.76], rotation: [0.0, 0.0, 0.0], cubes: HEAD },
    Bone { parent: Some("parrot_head"), name: "parrot_head2", pivot: [0.0, -2.0, -1.0], rotation: [0.0, 0.0, 0.0], cubes: HEAD2 },
    Bone { parent: Some("parrot_head"), name: "parrot_beak1", pivot: [0.0, -0.5, -1.5], rotation: [0.0, 0.0, 0.0], cubes: BEAK1 },
    Bone { parent: Some("parrot_head"), name: "parrot_beak2", pivot: [0.0, -1.75, -2.45], rotation: [0.0, 0.0, 0.0], cubes: BEAK2 },
    Bone { parent: Some("parrot_head"), name: "parrot_feather", pivot: [0.0, -2.15, 0.15], rotation: [-0.2214, 0.0, 0.0], cubes: FEATHER },
    Bone { parent: None, name: "parrot_leg_l", pivot: [1.0, 22.0, -1.05], rotation: [-0.0299, 0.0, 0.0], cubes: LEG },
    Bone { parent: None, name: "parrot_leg_r", pivot: [-1.0, 22.0, -1.05], rotation: [-0.0299, 0.0, 0.0], cubes: LEG },
];

pub(crate) const MODEL: EntityModel =
    EntityModel { bones: BONES, scale: 1.0, tex_grid: None };
