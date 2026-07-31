//! Behaviour tests for the Phase F blocks + items:
//! netherrack / bedrock / obsidian / portal / flint+steel.

#![cfg(test)]

use infengine_voxel::BlockId;

use super::{
    BEDROCK_ID, FLINT_AND_STEEL_ID, NETHERRACK_ID, OBSIDIAN_ID, PORTAL_ID,
    block_is_aabb_passable, block_is_opaque,
    block_is_placement_blocked, block_is_ray_passable, block_is_replaceable,
    block_is_transparent_render, is_tool_block, tool_label,
};
use crate::block::mining_math::{
    CLASS_PICKAXE, hardness_for, has_correct_tool_for_drops,
};

#[test]
fn netherrack_is_pickaxe_mineable_any_tier() {
    // W473: MC row - 0.4 destroy time, requires a pickaxe for the
    // drop but any tier qualifies (no needs_* tag).
    let dig = hardness_for(NETHERRACK_ID);
    assert_eq!(dig.destroy_time, 0.4);
    assert!(dig.requires_correct_tool);
    assert_eq!(dig.class_mask, CLASS_PICKAXE);
    assert_eq!(dig.min_tier, 0);
    assert!(has_correct_tool_for_drops(16, NETHERRACK_ID));
    assert!(!has_correct_tool_for_drops(0, NETHERRACK_ID));
}

#[test]
fn bedrock_is_unbreakable_and_blocked_from_place() {
    // W473: MC strength(-1) - the mining session never completes
    // in survival. F_NO_PLACE keeps RMB from dropping bedrock onto
    // another voxel.
    assert_eq!(hardness_for(BEDROCK_ID).destroy_time, -1.0);
    assert!(block_is_placement_blocked(BlockId(BEDROCK_ID)));
}

#[test]
fn obsidian_needs_diamond_tier_for_drops() {
    // W473: needs_diamond_tool = min_tier 4. Diamond (155) and
    // netherite (274) picks drop it; iron (18) and below do not.
    assert_eq!(hardness_for(OBSIDIAN_ID).min_tier, 4);
    assert!(has_correct_tool_for_drops(155, OBSIDIAN_ID));
    assert!(has_correct_tool_for_drops(274, OBSIDIAN_ID));
    assert!(!has_correct_tool_for_drops(18, OBSIDIAN_ID));
    assert!(!has_correct_tool_for_drops(17, OBSIDIAN_ID));
    assert!(!has_correct_tool_for_drops(16, OBSIDIAN_ID));
}

#[test]
fn portal_block_is_passable_for_ray_aabb_and_placement() {
    // The portal block is purely cosmetic + a teleport trigger.
    // Ray + AABB pass through; the place path refuses to spawn
    // it from inventory (F_NO_PLACE).
    assert!(block_is_ray_passable(BlockId(PORTAL_ID)));
    assert!(block_is_aabb_passable(BlockId(PORTAL_ID)));
    assert!(block_is_placement_blocked(BlockId(PORTAL_ID)));
    assert!(block_is_transparent_render(BlockId(PORTAL_ID)));
}

#[test]
fn portal_block_is_not_opaque() {
    // Opacity drives the mesher's face-culling decision. A
    // portal must NOT be opaque or the adjacent chunk faces
    // get hidden behind it.
    assert!(!block_is_opaque(BlockId(PORTAL_ID)));
}

#[test]
fn portal_block_is_not_replaceable() {
    // RMB-place should not overwrite a portal cell - that would
    // be a trivial way to destroy a portal without breaking
    // its frame. F_REPLACEABLE stays off.
    assert!(!block_is_replaceable(BlockId(PORTAL_ID)));
}

#[test]
fn flint_and_steel_is_tool_class_with_label() {
    // is_tool_block gates the place path: tool-class items
    // never plant themselves into the world on RMB.
    assert!(is_tool_block(BlockId(FLINT_AND_STEEL_ID)));
    assert_eq!(
        tool_label(BlockId(FLINT_AND_STEEL_ID)),
        Some("Flint and Steel")
    );
}

#[test]
fn obsidian_is_opaque_solid_block() {
    assert!(block_is_opaque(BlockId(OBSIDIAN_ID)));
    assert!(!block_is_aabb_passable(BlockId(OBSIDIAN_ID)));
    assert!(!block_is_ray_passable(BlockId(OBSIDIAN_ID)));
}

#[test]
fn bedrock_is_opaque_solid_block() {
    assert!(block_is_opaque(BlockId(BEDROCK_ID)));
    assert!(!block_is_aabb_passable(BlockId(BEDROCK_ID)));
}

#[test]
fn netherrack_is_opaque_and_solid() {
    assert!(block_is_opaque(BlockId(NETHERRACK_ID)));
    assert!(!block_is_aabb_passable(BlockId(NETHERRACK_ID)));
    assert!(!block_is_placement_blocked(BlockId(NETHERRACK_ID)));
}
