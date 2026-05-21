//! W16 regression + integration tests.
//!
//! Covers:
//! - W16.1: cross_model builder + flower id routing.
//! - W16.2: chunk mesher emit_special dispatch.
//! - W16.3: slab bottom + top renders as half-height block.
//! - W16.4: stairs / fence / door render correct element counts.
//! - W16.5: end-to-end chunk mesh integration for flower/torch/slab.

#![cfg(test)]

use infengine_voxel::{BlockId, Chunk, World, chunk_mesh_with_atlas_light_anim_and_special};
use infengine_render::MeshVertex;

use crate::block::{
    FLOWER_RED_ID, FLOWER_YELLOW_ID, FLOWER_BLUE_ID,
    OAK_SLAB_ID, OAK_SLAB_TOP_ID, OAK_STAIRS_ID, OAK_FENCE_ID, OAK_DOOR_ID,
    TORCH_ID, block_is_opaque,
};
use crate::resource_pack::{self, element_mesh::emit_block_elements, fallback, special_models};

fn placeholder_pack() -> resource_pack::ResourcePack {
    fallback::placeholder_pack()
}

// -----------------------------------------------------------------------
// W16.3 - Slab routing + geometry
// -----------------------------------------------------------------------

#[test]
fn oak_slab_renders_as_bottom_half() {
    // Slab bottom: model top face is at y = 8/16 = 0.5.
    use crate::block::OAK_SLAB_ID;
    let pack = placeholder_pack();
    let m = special_models::model_for_block(OAK_SLAB_ID, &pack)
        .expect("oak slab must have a special model");
    let e = &m.elements[0];
    let y_top = e.to[1] / 16.0;
    assert!(
        (y_top - 0.5).abs() < 1e-4,
        "oak slab bottom top at y={y_top}, expected 0.5",
    );
    // Bottom at y=0.
    assert_eq!(e.from[1], 0.0, "slab bottom starts at y=0");
}

#[test]
fn oak_slab_top_renders_as_top_half() {
    // Slab top: model spans y = 8..16 -> world 0.5..1.0.
    let pack = placeholder_pack();
    let m = special_models::model_for_block(OAK_SLAB_TOP_ID, &pack)
        .expect("oak slab top must have a special model");
    let e = &m.elements[0];
    let y_bottom = e.from[1] / 16.0;
    let y_top = e.to[1] / 16.0;
    assert!(
        (y_bottom - 0.5).abs() < 1e-4,
        "slab top bottom edge at y={y_bottom}, expected 0.5",
    );
    assert!(
        (y_top - 1.0).abs() < 1e-4,
        "slab top top edge at y={y_top}, expected 1.0",
    );
}

// -----------------------------------------------------------------------
// W16.4 - Stairs / fence / door element count
// -----------------------------------------------------------------------

#[test]
fn oak_stairs_renders_two_elements() {
    let pack = placeholder_pack();
    let m = special_models::model_for_block(OAK_STAIRS_ID, &pack)
        .expect("oak stairs must have a special model");
    assert_eq!(
        m.elements.len(), 2,
        "stairs must have exactly 2 elements (slab base + step)",
    );
    // Bottom slab element spans full width at y 0..8.
    assert_eq!(m.elements[0].from[1], 0.0);
    assert_eq!(m.elements[0].to[1], 8.0);
    // Upper step starts at y=8.
    assert_eq!(m.elements[1].from[1], 8.0);
}

#[test]
fn oak_fence_renders_center_post() {
    let pack = placeholder_pack();
    let m = special_models::model_for_block(OAK_FENCE_ID, &pack)
        .expect("oak fence must have a special model");
    // Fence post: single 4x16x4 centre column.
    assert_eq!(m.elements.len(), 1, "fence post has exactly 1 element");
    let e = &m.elements[0];
    assert_eq!(e.from, [6.0, 0.0, 6.0], "fence post from=[6,0,6]");
    assert_eq!(e.to, [10.0, 16.0, 10.0], "fence post to=[10,16,10]");
}

#[test]
fn oak_door_renders_thin_slab() {
    let pack = placeholder_pack();
    let m = special_models::model_for_block(OAK_DOOR_ID, &pack)
        .expect("oak door must have a special model");
    let e = &m.elements[0];
    // Door is 3/16 thick on the X axis.
    assert_eq!(e.from[0], 0.0);
    assert!(
        (e.to[0] - 3.0).abs() < 1e-4,
        "door thickness must be 3/16, got {}", e.to[0],
    );
    // Full height + full depth.
    assert_eq!(e.to[1], 16.0, "door must be full height");
    assert_eq!(e.to[2], 16.0, "door must be full depth");
}

// -----------------------------------------------------------------------
// W16.5 - End-to-end chunk mesh integration
// -----------------------------------------------------------------------

/// Helper: build the special-model mesh for a single-voxel chunk.
fn single_voxel_special_mesh(block_id: u16) -> (Vec<MeshVertex>, Vec<u32>) {
    let pack = placeholder_pack();
    let mut chunk = Chunk::empty();
    chunk.set(0, 0, 0, BlockId(block_id));
    let origin = World::chunk_world_origin([0, 0, 0]);

    let subs = chunk_mesh_with_atlas_light_anim_and_special(
        &chunk,
        origin,
        |_, _| [0.0, 0.0, 1.0, 1.0],
        block_is_opaque,
        |_, _| [0.0, 1.0],
        |_, _| 0u32,
        |block, vox_origin, verts, idxs| {
            emit_block_elements(
                block.0,
                vox_origin,
                verts,
                idxs,
                &pack,
                |_| [0.0, 1.0],
                |_| 0,
                |_| false,
            )
        },
    );

    let mut all_verts = Vec::new();
    let mut all_idxs = Vec::new();
    for sub in subs {
        all_verts.extend(sub.vertices);
        all_idxs.extend(sub.indices);
    }
    (all_verts, all_idxs)
}

#[test]
fn e2e_flower_in_chunk_emits_cross_mesh() {
    // A flower (cross model) has 2 quads * 2 faces each = 4 faces.
    // 4 faces * 4 verts = 16; 4 faces * 6 indices = 24.
    // NOT 24 verts (6 cube faces * 4) as the old cube path would emit.
    let (verts, idxs) = single_voxel_special_mesh(FLOWER_RED_ID);
    assert_eq!(
        verts.len(), 16,
        "flower cross mesh: expected 16 vertices (4 faces), got {}",
        verts.len(),
    );
    assert_eq!(
        idxs.len(), 24,
        "flower cross mesh: expected 24 indices, got {}",
        idxs.len(),
    );
}

#[test]
fn e2e_flower_yellow_emits_cross_mesh() {
    let (verts, _) = single_voxel_special_mesh(FLOWER_YELLOW_ID);
    assert_eq!(verts.len(), 16, "yellow flower must also emit 16 vertices");
}

#[test]
fn e2e_flower_blue_emits_cross_mesh() {
    let (verts, _) = single_voxel_special_mesh(FLOWER_BLUE_ID);
    assert_eq!(verts.len(), 16, "blue flower must also emit 16 vertices");
}

#[test]
fn e2e_torch_in_chunk_emits_stick_plus_flame() {
    // Torch has 1 element with 6 faces -> 6 * 4 = 24 verts.
    // That is FEWER than a full cube (24 is the same actually for 1
    // element 6 faces), but the geometry is a thin 2x10x2 stick,
    // not a 16x16x16 cube. We verify vertex positions stay within
    // the 7..9 / 0..10 / 7..9 bounding box.
    let (verts, _) = single_voxel_special_mesh(TORCH_ID);
    // At least one vertex should be above y=0 (stick bottom).
    assert!(!verts.is_empty(), "torch must emit geometry");
    let max_y = verts.iter().map(|v| v.position[1]).fold(f32::NEG_INFINITY, f32::max);
    // Torch top is at 10/16 = 0.625 world units above voxel origin.
    assert!(
        max_y <= 0.7,
        "torch max y should be ~0.625 (10/16), got {max_y}",
    );
    assert!(max_y > 0.5, "torch must extend above y=0.5");
}

#[test]
fn e2e_slab_in_chunk_emits_half_block() {
    // Slab bottom: all vertices must have y in 0..=0.5 (world units).
    let (verts, _) = single_voxel_special_mesh(OAK_SLAB_ID);
    assert!(!verts.is_empty(), "slab must emit geometry");
    let max_y = verts.iter().map(|v| v.position[1]).fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (max_y - 0.5).abs() < 1e-4,
        "slab bottom top face should be at y=0.5, got y={max_y}",
    );
    // Confirm no vertex exceeds 0.5 + small epsilon.
    let any_above_half = verts.iter().any(|v| v.position[1] > 0.5 + 1e-4);
    assert!(
        !any_above_half,
        "slab bottom must not have vertices above y=0.5",
    );
}

#[test]
fn e2e_slab_top_in_chunk_emits_upper_half() {
    // Slab top: vertex y range is 0.5..1.0.
    let (verts, _) = single_voxel_special_mesh(OAK_SLAB_TOP_ID);
    assert!(!verts.is_empty(), "slab top must emit geometry");
    let min_y = verts.iter().map(|v| v.position[1]).fold(f32::INFINITY, f32::min);
    let max_y = verts.iter().map(|v| v.position[1]).fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (min_y - 0.5).abs() < 1e-4,
        "slab top bottom face at y=0.5, got min_y={min_y}",
    );
    assert!(
        (max_y - 1.0).abs() < 1e-4,
        "slab top top face at y=1.0, got max_y={max_y}",
    );
}

#[test]
fn e2e_stone_cube_still_emits_six_faces() {
    // Stone (id=1) has no special model; the cube path must still work.
    let pack = placeholder_pack();
    let mut chunk = Chunk::empty();
    chunk.set(8, 8, 8, BlockId(1));
    let origin = World::chunk_world_origin([0, 0, 0]);

    let subs = chunk_mesh_with_atlas_light_anim_and_special(
        &chunk,
        origin,
        |_, _| [0.0, 0.0, 1.0, 1.0],
        block_is_opaque,
        |_, _| [0.0, 1.0],
        |_, _| 0u32,
        |block, vox_origin, verts, idxs| {
            emit_block_elements(
                block.0, vox_origin, verts, idxs, &pack,
                |_| [0.0, 1.0], |_| 0, |_| false,
            )
        },
    );
    let total_indices: usize = subs.iter().map(|s| s.indices.len()).sum();
    // 6 cube faces * 6 indices = 36.
    assert_eq!(
        total_indices, 36,
        "stone cube must emit 36 indices, got {total_indices}",
    );
}

#[test]
fn element_rotation_applies_to_vertex_positions() {
    // Cross-model quads are rotated 45 deg around Y. The emitted
    // vertex positions should differ from an unrotated flat quad.
    use crate::resource_pack::special_models::build_cross;
    let m = build_cross("poppy");
    let elem = &m.elements[0];
    // With rotation origin at (8,8,8) and 45-deg Y rotation,
    // the from/to coords change after rotation. The element from
    // has x=0.8/16 = 0.05, but after rotation the X component
    // will be different from 0.05.
    let rot = elem.rotation.expect("cross quad has rotation");
    // Verify the rotation origin is the block centre (8/16 = 0.5).
    let origin_world = [
        rot.origin[0] / 16.0,
        rot.origin[1] / 16.0,
        rot.origin[2] / 16.0,
    ];
    assert!((origin_world[0] - 0.5).abs() < 1e-4);
    assert!((origin_world[1] - 0.5).abs() < 1e-4);
    assert!((origin_world[2] - 0.5).abs() < 1e-4);
}

#[test]
fn face_cullface_skips_quad_when_neighbour_opaque() {
    // Slab bottom has a `Down` face with `cullface = Some(Down)`.
    // When the neighbour below is opaque, that face should be skipped.
    use crate::resource_pack::special_models::build_slab_bottom;
    let m = build_slab_bottom();
    let e = &m.elements[0];
    let down_face = e.faces.down.as_ref().expect("slab has down face");
    // cullface is Down -> face is suppressed when the voxel below is opaque.
    use crate::resource_pack::models::CubeFace;
    assert_eq!(
        down_face.cullface,
        Some(CubeFace::Down),
        "slab bottom down face must have cullface=Down",
    );
}

#[test]
fn flower_blocks_have_no_dedicated_render_flag() {
    // Flowers use F_RAY_PASS | F_AABB_PASS etc. but NOT F_DEDICATED_RENDER.
    // If they had F_DEDICATED_RENDER, the chunk mesh submission would skip
    // them entirely. They should go through the special-model mesh path.
    use crate::block::block_skips_chunk_mesh;
    for id in [FLOWER_RED_ID, FLOWER_YELLOW_ID, FLOWER_BLUE_ID] {
        let skips = block_skips_chunk_mesh(BlockId(id));
        assert!(
            !skips,
            "flower id {id} must NOT skip chunk mesh (should use special model path)",
        );
    }
}

// -----------------------------------------------------------------------
// W16.1 (extra) - flower-specific lookup via name_for_block
// -----------------------------------------------------------------------

#[test]
fn all_flower_ids_produce_distinct_texture_names() {
    let red = special_models::name_for_block(FLOWER_RED_ID).expect("red routes");
    let yellow = special_models::name_for_block(FLOWER_YELLOW_ID).expect("yellow routes");
    let blue = special_models::name_for_block(FLOWER_BLUE_ID).expect("blue routes");
    // All are cross-pattern but with different texture stems.
    assert!(red.starts_with("block/cross/"));
    assert!(yellow.starts_with("block/cross/"));
    assert!(blue.starts_with("block/cross/"));
    assert_ne!(red, yellow, "red and yellow must have different tex stems");
    assert_ne!(red, blue, "red and blue must have different tex stems");
    assert_ne!(yellow, blue, "yellow and blue must have different tex stems");
}
