//! Hand-built `BlockModel` descriptors for blocks whose MC source
//! file ships per-cuboid geometry instead of a plain 6-face cube.
//!
//! The Wave 9.x.1 JSON parser can read MC's `block/<name>.json`
//! files when they're present on disk, but the engine ALSO needs to
//! render these shapes when the user doesn't have MC installed
//! (placeholder pack) AND while we're still building out the
//! chunk-mesher integration. This module returns a fully-resolved
//! `BlockModel` directly from in-code constants, keyed by the same
//! `block/<name>` strings the JSON loader uses.
//!
//! Each shape is cited against the upstream MC source so future
//! contributors can verify the geometry matches the vanilla look.
//! Citations refer to decompiled MC `26.1.2-sources` JSON files
//! under `assets/minecraft/models/block/`.
//!
//! Shapes shipped in W9.x.3:
//! - `torch` - thin stick `[7, 0, 7] -> [9, 10, 9]`
//!   (cite: `models/block/template_torch.json`)
//! - `wall_torch` - same stick tilted `-22.5deg` around Z
//!   (cite: `models/block/template_torch_wall.json`)
//! - `slab` (bottom) - `[0, 0, 0] -> [16, 8, 16]`
//!   (cite: `models/block/slab.json`)
//! - `slab_top` - `[0, 8, 0] -> [16, 16, 16]`
//!   (cite: `models/block/slab_top.json`)
//! - `fence_post` - centre post `[6, 0, 6] -> [10, 16, 10]`
//!   (cite: `models/block/fence_post.json` - connections deferred)
//! - `door_lower` - thin slab `[0, 0, 0] -> [3, 16, 16]`
//!   (cite: `models/block/door_bottom_left.json` - thickness 3/16)
//! - `bed_foot` - `[0, 0, 0] -> [16, 9, 16]` with side trim
//!   (cite: `models/block/bed_foot.json` - simplified to one cuboid)
//!
//! Deferred for later waves:
//! - Bed head (two-voxel bed split)
//! - Chest entity model (full LayerEntity render path)
//! - Fence side / wall side connection arms (multipart blockstate)
//! - Stairs (2-element shape - works with the existing parser
//!   once enabled in the mesher)

use std::collections::HashMap;

use super::models::{
    BlockModel, CubeFace, ElementFaces, ElementRotation, ModelElement,
    ModelFace, RotationAxis,
};

/// Resolve a model name to a hand-built `BlockModel`. Returns
/// `None` for names that should fall through to the JSON loader
/// (or the default cube_all fallback). The W9.x.1 parser's lookup
/// path is the source of truth when the MC pack is on disk; this
/// table is the fallback when the pack is absent OR when the
/// W9.x scope deliberately overrides the JSON (e.g. simplified
/// slab without inheriting `block/block`'s display transforms).
pub(crate) fn lookup(name: &str) -> Option<BlockModel> {
    match name {
        "block/torch" | "torch" => Some(build_torch()),
        "block/wall_torch" | "wall_torch" => Some(build_wall_torch()),
        "block/slab" | "slab" => Some(build_slab_bottom()),
        "block/slab_top" | "slab_top" => Some(build_slab_top()),
        "block/fence_post" | "fence_post" => Some(build_fence_post()),
        "block/door_lower" | "door_lower" => Some(build_door_lower()),
        "block/door_upper" | "door_upper" => Some(build_door_upper()),
        "block/bed_foot" | "bed_foot" => Some(build_bed_foot()),
        // W12.2 - stairs / pane / bars / wall.
        "block/stairs" | "stairs" => Some(build_stairs()),
        "block/glass_pane" | "glass_pane" => Some(build_thin_pane()),
        "block/iron_bars" | "iron_bars" => Some(build_thin_pane()),
        "block/wall_post" | "wall_post" => Some(build_wall_post()),
        _ => None,
    }
}

/// Map an infengine block id to its special-model name, if any.
/// Used by the future chunk mesher to override the default 6-face
/// cube emission for known specials. Today this is consumed only
/// by tests + by W9.x.3's `model_for_block` resolver.
pub(crate) fn name_for_block(id: u16) -> Option<&'static str> {
    use crate::block::{
        COBBLESTONE_SLAB_ID, COBBLESTONE_STAIRS_ID, COBBLESTONE_WALL_ID, GLASS_PANE_ID,
        IRON_BARS_ID, IRON_DOOR_ID, NETHER_BRICK_FENCE_ID, OAK_DOOR_ID,
        OAK_DOOR_UPPER_ID, OAK_FENCE_ID, OAK_SLAB_ID, OAK_SLAB_TOP_ID, OAK_STAIRS_ID,
        STONE_SLAB_ID, STONE_STAIRS_ID, TORCH_ID,
    };
    match id {
        x if x == TORCH_ID => Some("block/torch"),
        // W12.2 - slab / fence / door / stairs / pane / bars / wall.
        x if x == OAK_SLAB_ID
            || x == STONE_SLAB_ID
            || x == COBBLESTONE_SLAB_ID =>
        {
            Some("block/slab")
        }
        x if x == OAK_SLAB_TOP_ID => Some("block/slab_top"),
        x if x == OAK_STAIRS_ID
            || x == STONE_STAIRS_ID
            || x == COBBLESTONE_STAIRS_ID =>
        {
            Some("block/stairs")
        }
        x if x == OAK_FENCE_ID || x == NETHER_BRICK_FENCE_ID => {
            Some("block/fence_post")
        }
        x if x == OAK_DOOR_ID || x == IRON_DOOR_ID => Some("block/door_lower"),
        x if x == OAK_DOOR_UPPER_ID => Some("block/door_upper"),
        x if x == GLASS_PANE_ID || x == IRON_BARS_ID => Some("block/glass_pane"),
        x if x == COBBLESTONE_WALL_ID => Some("block/wall_post"),
        _ => None,
    }
}

/// Convenience: resolve `name_for_block` then `lookup`, returning
/// the `BlockModel` if the block has a registered special shape.
pub(crate) fn model_for_block(id: u16) -> Option<BlockModel> {
    name_for_block(id).and_then(lookup)
}

// ---------------------------------------------------------------
// Builders. Each returns a `BlockModel` with textures already
// resolved against the W9 atlas key shape (file stems, not
// `block/<name>` paths). The chunk mesher feeds the file stem
// into `ResourcePack::block_texture` for the final UV rect.
// ---------------------------------------------------------------

/// Build a face with the standard `[0, 0, 16, 16]` full-tile UV.
fn full_face(texture: &str) -> ModelFace {
    ModelFace {
        uv: [0.0, 0.0, 16.0, 16.0],
        texture: texture.to_string(),
        cullface: None,
        tintindex: None,
        rotation: 0,
    }
}

/// Build a face with an explicit UV rect (in 16ths of a tile).
fn uv_face(uv: [f32; 4], texture: &str, cullface: Option<CubeFace>) -> ModelFace {
    ModelFace {
        uv,
        texture: texture.to_string(),
        cullface,
        tintindex: None,
        rotation: 0,
    }
}

/// Full-cube faces - all six sample the same texture stem with the
/// `[0, 0, 16, 16]` full tile. Used by slab + fence-post side
/// faces.
fn cube_all_faces(texture: &str) -> ElementFaces {
    ElementFaces {
        down: Some(full_face(texture)),
        up: Some(full_face(texture)),
        north: Some(full_face(texture)),
        south: Some(full_face(texture)),
        west: Some(full_face(texture)),
        east: Some(full_face(texture)),
    }
}

/// Torch stick - mirrors `template_torch.json` exactly. Note that
/// MC samples a sub-rect of the torch texture for the side faces
/// (the lit tip), so we keep the `[7, 6, 9, 16]` UV for sides.
pub(crate) fn build_torch() -> BlockModel {
    let mut textures = HashMap::new();
    textures.insert("torch".to_string(), "torch".to_string());
    BlockModel {
        elements: vec![ModelElement {
            from: [7.0, 0.0, 7.0],
            to: [9.0, 10.0, 9.0],
            rotation: None,
            shade: false,
            faces: ElementFaces {
                // Cite: template_torch.json down/up/sides.
                down: Some(uv_face([7.0, 13.0, 9.0, 15.0], "torch", Some(CubeFace::Down))),
                up: Some(uv_face([7.0, 6.0, 9.0, 8.0], "torch", None)),
                north: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                east: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                south: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                west: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
            },
        }],
        textures,
        ambient_occlusion: false,
    }
}

/// Wall torch - the `template_torch_wall.json` shape: a 2-wide
/// torch tilted `-22.5deg` around Z so the flame leans away from
/// the wall. Origin sits at the wall mount, NOT the floor.
pub(crate) fn build_wall_torch() -> BlockModel {
    let mut textures = HashMap::new();
    textures.insert("torch".to_string(), "torch".to_string());
    BlockModel {
        elements: vec![ModelElement {
            // Cite: template_torch_wall.json from/to/rotation.
            from: [-1.0, 3.5, 7.0],
            to: [1.0, 13.5, 9.0],
            rotation: Some(ElementRotation {
                origin: [0.0, 3.5, 8.0],
                axis: RotationAxis::Z,
                angle_deg: -22.5,
                rescale: false,
            }),
            shade: false,
            faces: ElementFaces {
                down: Some(uv_face([7.0, 13.0, 9.0, 15.0], "torch", None)),
                up: Some(uv_face([7.0, 6.0, 9.0, 8.0], "torch", None)),
                north: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                east: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                south: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                west: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
            },
        }],
        textures,
        ambient_occlusion: false,
    }
}

/// Slab bottom-half. Cite: `slab.json` `from = [0,0,0] to =
/// [16,8,16]` + UV layout reuses the lower half of the side
/// texture (so a half-slab in stone reads "the bottom 8 pixels of
/// the stone texture wrap the sides").
pub(crate) fn build_slab_bottom() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 8.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", Some(CubeFace::Down))),
                up: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", None)),
                north: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::North))),
                south: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::South))),
                west: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::West))),
                east: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::East))),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Slab top-half. Cite: `slab_top.json` `from = [0,8,0] to =
/// [16,16,16]`. Side UVs reverse so the lower 8 pixels of the
/// texture wrap the top half's sides (MC mirrors the slab to
/// keep the visible cube edge consistent).
pub(crate) fn build_slab_top() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 8.0, 0.0],
            to: [16.0, 16.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", None)),
                up: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", Some(CubeFace::Up))),
                north: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::North))),
                south: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::South))),
                west: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::West))),
                east: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::East))),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Fence post (the centre 4x16x4 cuboid). Cite: `fence_post.json`.
/// Side-connection arms are a follow-up that needs neighbour-aware
/// multipart blockstate dispatch.
pub(crate) fn build_fence_post() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [6.0, 0.0, 6.0],
            to: [10.0, 16.0, 10.0],
            rotation: None,
            shade: true,
            faces: cube_all_faces("#texture"),
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Door lower half. Cite: `door_bottom_left.json` - a 3/16-thick
/// slab on the X axis with the full 16x16 texture mapped to the
/// big faces and a 3x16 strip mapped to the edge faces.
pub(crate) fn build_door_lower() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [3.0, 16.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#bottom", None)),
                up: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#top", None)),
                north: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#side", None)),
                south: Some(uv_face([13.0, 0.0, 16.0, 16.0], "#side", None)),
                west: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", Some(CubeFace::West))),
                east: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", Some(CubeFace::East))),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Door upper half. Cite: `door_top_left.json` - mirror of the
/// lower half with the texture sampled from the upper door image
/// (the engine resolves `door_oak_upper.png` per the W12.2
/// BlockType row's `top`/`bottom`/`sides` tile spec).
pub(crate) fn build_door_upper() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [3.0, 16.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#bottom", None)),
                up: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#top", None)),
                north: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#side", None)),
                south: Some(uv_face([13.0, 0.0, 16.0, 16.0], "#side", None)),
                west: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", None)),
                east: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", None)),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Stairs (east-facing default). Cite: `models/block/stairs.json`.
///
/// 2 elements:
/// - bottom slab `[0,0,0]->[16,8,16]`
/// - upper half step `[8,8,0]->[16,16,16]` (visible east-side step).
pub(crate) fn build_stairs() -> BlockModel {
    let bottom = ModelElement {
        from: [0.0, 0.0, 0.0],
        to: [16.0, 8.0, 16.0],
        rotation: None,
        shade: true,
        faces: ElementFaces {
            down: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", Some(CubeFace::Down))),
            up: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", None)),
            north: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::North))),
            south: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::South))),
            west: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::West))),
            east: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::East))),
        },
    };
    let step = ModelElement {
        from: [8.0, 8.0, 0.0],
        to: [16.0, 16.0, 16.0],
        rotation: None,
        shade: true,
        faces: ElementFaces {
            down: Some(uv_face([8.0, 0.0, 16.0, 16.0], "#top", None)),
            up: Some(uv_face([8.0, 0.0, 16.0, 16.0], "#top", Some(CubeFace::Up))),
            north: Some(uv_face([0.0, 0.0, 8.0, 8.0], "#side", Some(CubeFace::North))),
            south: Some(uv_face([0.0, 0.0, 8.0, 8.0], "#side", Some(CubeFace::South))),
            west: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", None)),
            east: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::East))),
        },
    };
    BlockModel {
        elements: vec![bottom, step],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Thin pane / iron bars centre post. Cite:
/// `models/block/glass_pane_post.json` + `models/block/iron_bars_post.json`.
/// 2x16x2 centre column; the 4 connection arms are a follow-up
/// (multipart blockstate). Glass pane and iron bars share the
/// same post geometry; only the texture differs.
pub(crate) fn build_thin_pane() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [7.0, 0.0, 7.0],
            to: [9.0, 16.0, 9.0],
            rotation: None,
            shade: true,
            faces: cube_all_faces("#texture"),
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Cobblestone wall post. Cite: `models/block/template_wall_post.json`
/// -> centre 8x16x8 column. Side arms (`template_wall_side.json`)
/// are a follow-up tied to the multipart blockstate dispatch.
pub(crate) fn build_wall_post() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [4.0, 0.0, 4.0],
            to: [12.0, 16.0, 12.0],
            rotation: None,
            shade: true,
            faces: cube_all_faces("#texture"),
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Bed foot. Cite: `bed_foot.json` (simplified to one cuboid -
/// the real MC bed splits foot vs head into two voxels with
/// shared `bed.json` textures).
pub(crate) fn build_bed_foot() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 9.0, 16.0],
            rotation: None,
            shade: true,
            faces: cube_all_faces("#texture"),
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torch_emits_one_element_with_six_faces() {
        let m = build_torch();
        assert_eq!(m.elements.len(), 1);
        let e = &m.elements[0];
        assert_eq!(e.from, [7.0, 0.0, 7.0]);
        assert_eq!(e.to, [9.0, 10.0, 9.0]);
        // All 6 faces present (the stick is visible from every side).
        assert!(e.faces.down.is_some());
        assert!(e.faces.up.is_some());
        assert!(e.faces.north.is_some());
        assert!(e.faces.south.is_some());
        assert!(e.faces.east.is_some());
        assert!(e.faces.west.is_some());
        assert!(!m.ambient_occlusion);
        assert!(!e.shade);
    }

    #[test]
    fn torch_stick_geometry_correct() {
        // Cross-reference: template_torch.json `from`/`to` MUST
        // match exactly so the rendered torch sits in the centre
        // of the voxel and stops at 10/16 height.
        let m = build_torch();
        let e = &m.elements[0];
        assert_eq!(e.from, [7.0, 0.0, 7.0]);
        assert_eq!(e.to, [9.0, 10.0, 9.0]);
    }

    #[test]
    fn wall_torch_applies_z_axis_rotation() {
        let m = build_wall_torch();
        let e = &m.elements[0];
        let r = e.rotation.expect("wall torch has rotation");
        assert_eq!(r.axis, RotationAxis::Z);
        assert!((r.angle_deg + 22.5).abs() < 1e-3);
        assert_eq!(r.origin, [0.0, 3.5, 8.0]);
        // Wall torch reaches into negative X (sticks out from the
        // wall in the -X direction before the rotation pulls it
        // back).
        assert_eq!(e.from, [-1.0, 3.5, 7.0]);
        assert_eq!(e.to, [1.0, 13.5, 9.0]);
    }

    #[test]
    fn slab_bottom_emits_top_face_at_y_8() {
        // Slab bottom: `from.y = 0, to.y = 8`. Renderer reads
        // `to.y / 16.0` for the world-space height; 8/16 = 0.5.
        let m = build_slab_bottom();
        let e = &m.elements[0];
        assert_eq!(e.from[1], 0.0);
        assert_eq!(e.to[1], 8.0);
        let up = e.faces.up.as_ref().expect("up face present");
        assert_eq!(up.uv, [0.0, 0.0, 16.0, 16.0]);
        // Top face has NO cullface (slab top in bottom-half is
        // exposed unless capped by another slab).
        assert!(up.cullface.is_none());
    }

    #[test]
    fn slab_top_emits_bottom_face_at_y_8() {
        let m = build_slab_top();
        let e = &m.elements[0];
        assert_eq!(e.from[1], 8.0);
        assert_eq!(e.to[1], 16.0);
        // Top face culls against the cell above (it sits at
        // full y=16/16).
        assert_eq!(e.faces.up.as_ref().unwrap().cullface, Some(CubeFace::Up));
    }

    #[test]
    fn fence_post_centre_only() {
        let m = build_fence_post();
        let e = &m.elements[0];
        // Centre 4x4 footprint, full height. No connections in W9.x.
        assert_eq!(e.from, [6.0, 0.0, 6.0]);
        assert_eq!(e.to, [10.0, 16.0, 10.0]);
    }

    #[test]
    fn door_lower_is_thin_slab() {
        let m = build_door_lower();
        let e = &m.elements[0];
        assert_eq!(e.from[0], 0.0);
        assert_eq!(e.to[0], 3.0);
        assert_eq!(e.to[1], 16.0);
        assert_eq!(e.to[2], 16.0);
    }

    #[test]
    fn bed_foot_emits_9_high_block() {
        let m = build_bed_foot();
        let e = &m.elements[0];
        assert_eq!(e.from, [0.0, 0.0, 0.0]);
        assert_eq!(e.to, [16.0, 9.0, 16.0]);
    }

    #[test]
    fn lookup_accepts_block_prefix() {
        assert!(lookup("block/torch").is_some());
        assert!(lookup("torch").is_some());
        assert!(lookup("block/slab").is_some());
        assert!(lookup("slab").is_some());
        assert!(lookup("nonsense").is_none());
    }

    #[test]
    fn name_for_torch_block_id_routes() {
        use crate::block::TORCH_ID;
        assert_eq!(name_for_block(TORCH_ID), Some("block/torch"));
        // Stone id 1 has no special model.
        assert_eq!(name_for_block(1), None);
    }

    #[test]
    fn model_for_block_returns_full_torch_model() {
        use crate::block::TORCH_ID;
        let m = model_for_block(TORCH_ID).expect("torch model");
        assert_eq!(m.elements.len(), 1);
        assert_eq!(m.elements[0].from, [7.0, 0.0, 7.0]);
    }

    // ---------------------------------------------------------------
    // W12.2 - Slab / fence / door / stairs / pane / wall routing.
    // ---------------------------------------------------------------

    fn w12_ids() -> &'static [u16] {
        use crate::block::{
            COBBLESTONE_SLAB_ID, COBBLESTONE_STAIRS_ID, COBBLESTONE_WALL_ID,
            GLASS_PANE_ID, IRON_BARS_ID, IRON_DOOR_ID, NETHER_BRICK_FENCE_ID,
            OAK_DOOR_ID, OAK_DOOR_UPPER_ID, OAK_FENCE_ID, OAK_SLAB_ID,
            OAK_SLAB_TOP_ID, OAK_STAIRS_ID, STONE_SLAB_ID, STONE_STAIRS_ID,
        };
        // Boxed to a 'static via `Box::leak`. Cheap; runs once.
        static IDS: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
        IDS.get_or_init(|| {
            vec![
                OAK_SLAB_ID, STONE_SLAB_ID, COBBLESTONE_SLAB_ID,
                OAK_SLAB_TOP_ID, OAK_STAIRS_ID, STONE_STAIRS_ID,
                COBBLESTONE_STAIRS_ID, OAK_FENCE_ID, NETHER_BRICK_FENCE_ID,
                OAK_DOOR_ID, OAK_DOOR_UPPER_ID, IRON_DOOR_ID,
                GLASS_PANE_ID, IRON_BARS_ID, COBBLESTONE_WALL_ID,
            ]
        })
    }

    #[test]
    fn slab_bottom_routes_to_block_slab() {
        use crate::block::{COBBLESTONE_SLAB_ID, OAK_SLAB_ID, STONE_SLAB_ID};
        assert_eq!(name_for_block(OAK_SLAB_ID), Some("block/slab"));
        assert_eq!(name_for_block(STONE_SLAB_ID), Some("block/slab"));
        assert_eq!(name_for_block(COBBLESTONE_SLAB_ID), Some("block/slab"));
    }

    #[test]
    fn slab_top_routes_to_block_slab_top() {
        use crate::block::OAK_SLAB_TOP_ID;
        assert_eq!(name_for_block(OAK_SLAB_TOP_ID), Some("block/slab_top"));
    }

    #[test]
    fn slab_bottom_emits_top_face_at_y_0_5() {
        use crate::block::OAK_SLAB_ID;
        // Slab model: `to[1] = 8.0`, which means top face sits
        // at y = 8/16 = 0.5 in voxel-local space.
        let m = model_for_block(OAK_SLAB_ID).expect("oak slab routes");
        let e = &m.elements[0];
        let y_top_frac = e.to[1] / 16.0;
        assert!(
            (y_top_frac - 0.5).abs() < 1e-3,
            "slab bottom top-face at y = {y_top_frac}, want 0.5",
        );
    }

    #[test]
    fn door_lower_emits_thin_slab_quad() {
        use crate::block::OAK_DOOR_ID;
        let m = model_for_block(OAK_DOOR_ID).expect("oak door routes");
        let e = &m.elements[0];
        assert!((e.to[0] - 3.0).abs() < 1e-3, "door not 3/16 thick");
        assert_eq!(e.to[1], 16.0);
        assert_eq!(e.to[2], 16.0);
    }

    #[test]
    fn fence_post_visible_in_chunk_mesh() {
        use crate::block::OAK_FENCE_ID;
        let m = model_for_block(OAK_FENCE_ID).expect("oak fence routes");
        let e = &m.elements[0];
        assert_eq!(e.from, [6.0, 0.0, 6.0]);
        assert_eq!(e.to, [10.0, 16.0, 10.0]);
    }

    #[test]
    fn stairs_emits_two_elements() {
        use crate::block::OAK_STAIRS_ID;
        let m = model_for_block(OAK_STAIRS_ID).expect("oak stairs routes");
        assert_eq!(m.elements.len(), 2);
        assert_eq!(m.elements[0].to, [16.0, 8.0, 16.0]);
        assert_eq!(m.elements[1].from, [8.0, 8.0, 0.0]);
    }

    #[test]
    fn glass_pane_and_iron_bars_share_post_shape() {
        use crate::block::{GLASS_PANE_ID, IRON_BARS_ID};
        let gp = model_for_block(GLASS_PANE_ID).expect("glass pane routes");
        let ib = model_for_block(IRON_BARS_ID).expect("iron bars routes");
        assert_eq!(gp.elements[0].from, [7.0, 0.0, 7.0]);
        assert_eq!(gp.elements[0].to, [9.0, 16.0, 9.0]);
        assert_eq!(ib.elements[0].from, [7.0, 0.0, 7.0]);
        assert_eq!(ib.elements[0].to, [9.0, 16.0, 9.0]);
    }

    #[test]
    fn cobblestone_wall_post_centre_only() {
        use crate::block::COBBLESTONE_WALL_ID;
        let m = model_for_block(COBBLESTONE_WALL_ID).expect("wall routes");
        let e = &m.elements[0];
        assert_eq!(e.from, [4.0, 0.0, 4.0]);
        assert_eq!(e.to, [12.0, 16.0, 12.0]);
    }

    #[test]
    fn door_upper_distinct_from_door_lower() {
        use crate::block::{OAK_DOOR_ID, OAK_DOOR_UPPER_ID};
        assert_eq!(name_for_block(OAK_DOOR_ID), Some("block/door_lower"));
        assert_eq!(
            name_for_block(OAK_DOOR_UPPER_ID),
            Some("block/door_upper"),
        );
    }

    #[test]
    fn iron_door_shares_lower_geometry() {
        use crate::block::{IRON_DOOR_ID, OAK_DOOR_ID};
        assert_eq!(name_for_block(IRON_DOOR_ID), name_for_block(OAK_DOOR_ID));
    }

    #[test]
    fn every_w12_block_resolves_a_model() {
        for &id in w12_ids() {
            assert!(
                model_for_block(id).is_some(),
                "id {id} has no resolvable special model",
            );
        }
    }

    #[test]
    fn nether_brick_fence_uses_post_geometry() {
        use crate::block::NETHER_BRICK_FENCE_ID;
        let m = model_for_block(NETHER_BRICK_FENCE_ID).expect("nb fence routes");
        let e = &m.elements[0];
        // Same fence-post 4x16x4 centre as oak.
        assert_eq!(e.from, [6.0, 0.0, 6.0]);
        assert_eq!(e.to, [10.0, 16.0, 10.0]);
    }

    #[test]
    fn full_face_uv_is_default_tile() {
        let f = full_face("dirt");
        assert_eq!(f.uv, [0.0, 0.0, 16.0, 16.0]);
        assert_eq!(f.texture, "dirt");
        assert!(f.cullface.is_none());
    }

    #[test]
    fn uv_face_preserves_rect() {
        let f = uv_face([1.0, 2.0, 3.0, 4.0], "stone", Some(CubeFace::Down));
        assert_eq!(f.uv, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(f.cullface, Some(CubeFace::Down));
    }

    #[test]
    fn slab_bottom_side_uvs_use_lower_half() {
        // Side faces of a bottom slab sample the LOWER half of
        // the texture (`v0 = 8` of 16) so the visible side edge
        // shows the bottom 8 pixels of the source.
        let m = build_slab_bottom();
        let e = &m.elements[0];
        let north = e.faces.north.as_ref().unwrap();
        assert_eq!(north.uv, [0.0, 8.0, 16.0, 16.0]);
    }

    #[test]
    fn slab_top_side_uvs_use_upper_half() {
        // Mirror of the bottom: side faces sample the UPPER half
        // (`v0 = 0` of 16, `v1 = 8`) because MC mirrors the
        // wrap so the visible cube edge stays consistent.
        let m = build_slab_top();
        let e = &m.elements[0];
        let north = e.faces.north.as_ref().unwrap();
        assert_eq!(north.uv, [0.0, 0.0, 16.0, 8.0]);
    }
}
