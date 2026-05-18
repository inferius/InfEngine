//! Behaviour tests for the Phase F blocks + items:
//! netherrack / bedrock / obsidian / portal / flint+steel.

#![cfg(test)]

use infengine_voxel::BlockId;

use super::{
    BEDROCK_ID, FLINT_AND_STEEL_ID, NETHERRACK_ID, OBSIDIAN_ID, PORTAL_ID,
    block_category, block_is_aabb_passable, block_is_opaque,
    block_is_placement_blocked, block_is_ray_passable, block_is_replaceable,
    block_is_transparent_render, block_required_tier, is_tool_block,
    tool_effectiveness, tool_label,
};
use crate::block::BlockCategory;

#[test]
fn netherrack_is_stone_category_pickaxe_tier_1() {
    assert_eq!(block_category(BlockId(NETHERRACK_ID)), BlockCategory::Stone);
    assert_eq!(block_required_tier(BlockId(NETHERRACK_ID)), 1);
}

#[test]
fn bedrock_required_tier_is_4_and_blocked_from_place() {
    // tier 4 -> no shipped tool reaches it, so the block is
    // effectively unbreakable. F_NO_PLACE keeps RMB from
    // dropping bedrock onto another voxel.
    assert_eq!(block_required_tier(BlockId(BEDROCK_ID)), 4);
    assert!(block_is_placement_blocked(BlockId(BEDROCK_ID)));
}

#[test]
fn obsidian_required_tier_is_3_only_iron_pick_drops_it() {
    // Iron pickaxe = tier 3. tool_effectiveness must report
    // drop_ok = true with iron and false with stone.
    assert_eq!(block_required_tier(BlockId(OBSIDIAN_ID)), 3);
    let (_mult_iron, drop_iron) =
        tool_effectiveness(BlockId(18), BlockId(OBSIDIAN_ID));
    assert!(drop_iron, "iron pickaxe should drop obsidian");
    let (_mult_stone, drop_stone) =
        tool_effectiveness(BlockId(17), BlockId(OBSIDIAN_ID));
    assert!(!drop_stone, "stone pickaxe must not drop obsidian");
    let (_mult_wood, drop_wood) =
        tool_effectiveness(BlockId(16), BlockId(OBSIDIAN_ID));
    assert!(!drop_wood, "wood pickaxe must not drop obsidian");
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
