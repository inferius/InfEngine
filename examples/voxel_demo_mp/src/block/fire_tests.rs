//! Q.4b sanity tests for the fire block flags + lookup helpers.
//!
//! Real fire-spread + extinguish behaviour ships in Q.4c (see
//! `block/fire.rs`); this file pins the static registry shape.

#![cfg(test)]

use infengine_voxel::BlockId;

use super::{
    FIRE_ID, block_has_flag, block_is_aabb_passable, block_is_opaque,
    block_is_placement_blocked, block_is_ray_passable, block_is_replaceable,
    block_is_transparent_render, block_skips_chunk_mesh, block_type,
    F_DEDICATED_RENDER, F_REPLACEABLE, F_TRANSPARENT_RENDER, is_tool_block,
};

#[test]
fn fire_block_registered_at_id_49() {
    let bt = block_type(FIRE_ID).expect("fire block must be registered");
    assert_eq!(bt.id, FIRE_ID);
    assert_eq!(bt.name, "fire");
    assert!(bt.transparent);
}

#[test]
fn fire_block_has_portal_like_flags() {
    // Spec: F_RAY_PASS | F_AABB_PASS | F_REPLACEABLE
    //       | F_TRANSPARENT_RENDER | F_DEDICATED_RENDER.
    let id = BlockId(FIRE_ID);
    assert!(block_is_ray_passable(id), "fire must be ray-passable");
    assert!(block_is_aabb_passable(id), "fire must not collide");
    assert!(
        block_is_replaceable(id),
        "fire must opt into F_REPLACEABLE so water can extinguish it"
    );
    assert!(block_is_transparent_render(id));
    assert!(block_skips_chunk_mesh(id));
}

#[test]
fn fire_block_is_not_opaque() {
    // Opacity drives the mesher's face-culling decision. Fire
    // must not occlude adjacent chunk faces.
    assert!(!block_is_opaque(BlockId(FIRE_ID)));
}

#[test]
fn fire_block_is_not_placement_blocked() {
    // The fire cell can be replaced - F_NO_PLACE is off by
    // design so water spreading into a fire cell wins.
    assert!(!block_is_placement_blocked(BlockId(FIRE_ID)));
}

#[test]
fn fire_block_flag_bits_match_spec() {
    let bt = block_type(FIRE_ID).unwrap();
    let expected = F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_REPLACEABLE;
    // Spot-check the three "render + replaceability" bits since
    // the others have dedicated helpers above.
    assert_eq!(bt.flags & expected, expected);
}

#[test]
fn fire_block_id_is_not_a_tool() {
    // Fire is a block (placed in the world); never a tool slot.
    assert!(!is_tool_block(BlockId(FIRE_ID)));
}

#[test]
fn fire_has_block_flag_helper() {
    assert!(block_has_flag(BlockId(FIRE_ID), F_TRANSPARENT_RENDER));
    assert!(block_has_flag(BlockId(FIRE_ID), F_REPLACEABLE));
}
