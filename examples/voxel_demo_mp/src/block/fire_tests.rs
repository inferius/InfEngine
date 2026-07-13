//! Q.4b sanity tests for the fire block flags + lookup helpers.
//!
//! Real fire-spread + extinguish behaviour ships in Q.4c (see
//! `block/fire.rs`); this file pins the static registry shape.

#![cfg(test)]

use infengine_voxel::BlockId;

use super::{
    FIRE_ID, block_has_flag, block_is_aabb_passable, block_is_opaque,
    block_is_placement_blocked, block_is_ray_passable, block_is_replaceable,
    block_is_transparent_render, block_is_washable, block_skips_chunk_mesh, block_type,
    F_DEDICATED_RENDER, F_REPLACEABLE, F_TRANSPARENT_RENDER, F_WASHABLE, is_tool_block,
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
    let expected =
        F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_REPLACEABLE | F_WASHABLE;
    // Spot-check the four "render + replaceability + wash" bits
    // since the others have dedicated helpers above.
    assert_eq!(bt.flags & expected, expected);
}

#[test]
fn fire_block_is_washable_for_water_extinguish() {
    // Q.4e: water spreading INTO a fire cell must overwrite it.
    // F_WASHABLE drives the existing liquid_plan's "spread can
    // overwrite this block" gate.
    assert!(block_is_washable(BlockId(FIRE_ID)));
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

#[test]
fn fire_block_is_dedicated_render() {
    // W8.4: the registry flag has been there since Q.4b, but the
    // runtime had no draw call for FIRE_ID until W8 (the user
    // reported a "lava on grass = nothing visible" bug). Pinning
    // the flag in a dedicated test guards the contract for the
    // billboard-quad render path in main.rs.
    assert!(
        block_skips_chunk_mesh(BlockId(FIRE_ID)),
        "fire must be on the dedicated-render path so main.rs's billboard pass owns it"
    );
}

// W275: the per-cell LAVA_IGNITE_RATE_DENOM pin retired with the
// full-walk emitter - lava ignition now samples per column (see
// `fire_runtime::tick_lava_fire_emit`); the sampled-attempt gate is
// pinned by `fire::tests::lava_ignite_gate_fires_at_least_once...`.
