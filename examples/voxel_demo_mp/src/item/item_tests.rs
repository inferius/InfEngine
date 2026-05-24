//! Tests for the `item/` module: registry coverage, max-stack
//! contract, place mapping, icon fallback, and the stack-arithmetic
//! helpers.
//!
//! Co-located with the source per the file-size policy. Mounted from
//! `item/mod.rs` under `#[cfg(test)]`.

use super::icons::{IconSpec, icon_for};
use super::stack::{
    drop_into_first_free, merge_into, split_half, take_n,
};
use super::{
    ITEM_TYPES, ItemCategory, item_category, item_display_name,
    item_max_stack, item_places_block, item_type,
};
use crate::block::BLOCK_TYPES;

// ---------------------------------------------------------------
// Registry coverage.
// ---------------------------------------------------------------

#[test]
fn item_type_for_every_block_with_inventory_slot() {
    // Every BLOCK_TYPES row (none are air; air = id 0 isn't in
    // the table) must have a matching ITEM_TYPES entry with the
    // same id + name.
    for bt in BLOCK_TYPES {
        let it = item_type(bt.id).unwrap_or_else(|| {
            panic!("no ItemType for block id {} ({})", bt.id, bt.name)
        });
        assert_eq!(
            it.id, bt.id,
            "id mismatch for {}: item {} vs block {}",
            bt.name, it.id, bt.id,
        );
        assert_eq!(
            it.name, bt.name,
            "name mismatch for id {}: item {:?} vs block {:?}",
            bt.id, it.name, bt.name,
        );
    }
}

#[test]
fn item_count_at_least_block_count() {
    // Every block has an ItemType (the 1:1 mirror checked above)
    // but ITEM_TYPES is allowed to grow with non-block items
    // (ingredients, smelt outputs, ...). C.4 lands `iron_ingot`
    // as the first item-only row.
    assert!(
        ITEM_TYPES.len() >= BLOCK_TYPES.len(),
        "ITEM_TYPES ({}) must mirror BLOCK_TYPES ({}) at minimum",
        ITEM_TYPES.len(),
        BLOCK_TYPES.len(),
    );
}

// W37.2 - bulk MC item/block registry import brings total to >= 150.
#[test]
fn w37_item_types_at_least_150() {
    assert!(
        ITEM_TYPES.len() >= 150,
        "ITEM_TYPES has {} entries, need >= 150 after W37.2 bulk import",
        ITEM_TYPES.len(),
    );
}

// W40.2 - bulk MC item/block registry import brings total to >= 350.
#[test]
fn w40_item_types_at_least_350() {
    assert!(
        ITEM_TYPES.len() >= 350,
        "ITEM_TYPES has {} entries, need >= 350 after W40.2 bulk import",
        ITEM_TYPES.len(),
    );
}

// ---------------------------------------------------------------
// max_stack contract.
// ---------------------------------------------------------------

#[test]
fn max_stack_tools_is_one() {
    // Tool ids 16..=19 (pickaxes + feather), 23..=25 (swords),
    // 29..=34 (shovels + axes), 38..=40 (buckets).
    // EXCEPT: feather (19) is an ingredient that should stack to
    // 64; the spec lists it among "non-placeable item" ids but
    // not as a single-stack tool. We verify the actual tool /
    // weapon / bucket entries here.
    let one_stack_ids: &[u16] = &[
        16, 17, 18, // pickaxes
        23, 24, 25, // swords
        29, 30, 31, // shovels
        32, 33, 34, // axes
        39, 40,     // water + lava buckets (empty = 16 in MC)
        48,         // flint+steel (Phase F)
    ];
    for &id in one_stack_ids {
        assert_eq!(
            item_max_stack(id),
            1,
            "expected max_stack=1 for tool id {id}",
        );
    }
}

#[test]
fn max_stack_blocks_is_64() {
    // Building blocks + decorations + ores + bed + the liquid
    // source ids (water/lava are in the block table even though
    // they enter the world via buckets).
    let block_stack_ids: &[u16] = &[
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, // basic
        26, 27, 28, // sandstone, cactus, glowstone
        36, 37,     // water, lava (bed=1 per MC, dropped)
        44, 45, 46, // netherrack, bedrock, obsidian (Phase F)
    ];
    for &id in block_stack_ids {
        assert_eq!(
            item_max_stack(id),
            64,
            "expected max_stack=64 for block id {id}",
        );
    }
}

#[test]
fn max_stack_unknown_id_defaults_to_64() {
    // Future-proof: unknown ids land on the standard 64 stack so
    // callers that haven't been migrated yet don't blow up.
    assert_eq!(item_max_stack(9999), 64);
    assert_eq!(item_max_stack(0), 64);
}

// ---------------------------------------------------------------
// Place mapping.
// ---------------------------------------------------------------

#[test]
fn places_block_for_user_placeable() {
    assert_eq!(item_places_block(1), Some(1)); // stone
    assert_eq!(item_places_block(2), Some(2)); // dirt
    assert_eq!(item_places_block(3), Some(3)); // grass
    assert_eq!(item_places_block(7), Some(7)); // planks
}

#[test]
fn places_block_none_for_tool_weapon_bucket_feather() {
    // Tools / weapons / buckets / the feather ingredient must
    // refuse to place.
    let non_placeable: &[u16] = &[
        16, 17, 18, // pickaxes
        19,         // feather
        23, 24, 25, // swords
        29, 30, 31, 32, 33, 34, // shovels + axes
        38, 39, 40, // buckets
    ];
    for &id in non_placeable {
        assert_eq!(
            item_places_block(id),
            None,
            "expected places=None for id {id}",
        );
    }
}

#[test]
fn places_block_none_for_liquid_source_ids() {
    // Water + lava are in the block table but the player can't
    // RMB-place them from the inventory.
    assert_eq!(item_places_block(36), None);
    assert_eq!(item_places_block(37), None);
}

#[test]
fn places_block_unknown_id_is_none() {
    assert_eq!(item_places_block(9999), None);
    assert_eq!(item_places_block(0), None);
}

// ---------------------------------------------------------------
// Display + category.
// ---------------------------------------------------------------

#[test]
fn item_display_name_uses_explicit_when_set() {
    assert_eq!(item_display_name(8), "Cobblestone");
    assert_eq!(item_display_name(16), "Wood Pickaxe");
    assert_eq!(item_display_name(38), "Bucket");
}

#[test]
fn item_display_name_falls_back_to_title_case() {
    assert_eq!(item_display_name(1), "Stone");
    assert_eq!(item_display_name(11), "Wool");
    assert_eq!(item_display_name(35), "Bed");
}

#[test]
fn item_category_routes_to_expected_buckets() {
    assert_eq!(item_category(1), ItemCategory::BuildingBlock);
    assert_eq!(item_category(16), ItemCategory::Tool);
    assert_eq!(item_category(23), ItemCategory::Weapon);
    assert_eq!(item_category(38), ItemCategory::Bucket);
    assert_eq!(item_category(19), ItemCategory::Ingredient);
    assert_eq!(item_category(12), ItemCategory::Food);
}

// ---------------------------------------------------------------
// Icons.
// ---------------------------------------------------------------

#[test]
fn icon_for_block_falls_back_to_top() {
    // Stone (id 1) has no explicit icon; resolves to the
    // BlockType's top face slot.
    let stone_top = BLOCK_TYPES.iter().find(|b| b.id == 1).unwrap();
    let want = IconSpec {
        col: stone_top.top.col,
        row: stone_top.top.row,
    };
    assert_eq!(icon_for(1), Some(want));
}

#[test]
fn icon_for_grass_uses_block_top_not_sides() {
    // Grass (id 3) has different top vs sides tiles. Confirm we
    // pick the top (the canonical "this is grass" tile) instead
    // of the side variant.
    let grass = BLOCK_TYPES.iter().find(|b| b.id == 3).unwrap();
    let icon = icon_for(3).expect("grass icon");
    assert_eq!(icon.col, grass.top.col);
    assert_eq!(icon.row, grass.top.row);
}

#[test]
fn icon_for_unknown_id_is_none() {
    assert!(icon_for(0).is_none());
    assert!(icon_for(9999).is_none());
}

// ---------------------------------------------------------------
// Stack arithmetic - merge_into.
// ---------------------------------------------------------------

#[test]
fn merge_into_empty_slot() {
    let mut slot: Option<(u16, u8)> = None;
    let leftover = merge_into(&mut slot, (3, 10));
    assert_eq!(leftover, 0);
    assert_eq!(slot, Some((3, 10)));
}

#[test]
fn merge_into_partial_stack_same_item() {
    // slot (3, 40) + incoming (3, 30), max_stack 64 -> slot
    // (3, 64), leftover 6.
    let mut slot = Some((3, 40));
    let leftover = merge_into(&mut slot, (3, 30));
    assert_eq!(leftover, 6);
    assert_eq!(slot, Some((3, 64)));
}

#[test]
fn merge_into_different_item() {
    // Different item leaves the slot untouched; full count is
    // leftover.
    let mut slot = Some((1, 10));
    let leftover = merge_into(&mut slot, (2, 20));
    assert_eq!(leftover, 20);
    assert_eq!(slot, Some((1, 10)));
}

#[test]
fn merge_into_empty_respects_tool_max_stack_one() {
    // Pickaxe (id 16) caps at 1: merging (16, 5) into empty
    // leaves 4 leftover.
    let mut slot: Option<(u16, u8)> = None;
    let leftover = merge_into(&mut slot, (16, 5));
    assert_eq!(leftover, 4);
    assert_eq!(slot, Some((16, 1)));
}

// ---------------------------------------------------------------
// Stack arithmetic - split_half.
// ---------------------------------------------------------------

#[test]
fn split_half_odd_stack() {
    // slot (3, 17) -> moved out (3, 8), slot remains (3, 9).
    // Floor goes to the moved-out half, ceiling stays in the
    // source slot - misclicks leave the player with the bigger
    // half they started from.
    let mut slot = Some((3, 17));
    let moved = split_half(&mut slot);
    assert_eq!(moved, Some((3, 8)));
    assert_eq!(slot, Some((3, 9)));
}

#[test]
fn split_half_even_stack() {
    let mut slot = Some((3, 10));
    let moved = split_half(&mut slot);
    assert_eq!(moved, Some((3, 5)));
    assert_eq!(slot, Some((3, 5)));
}

#[test]
fn split_half_empty_returns_none() {
    let mut slot: Option<(u16, u8)> = None;
    assert!(split_half(&mut slot).is_none());
}

// ---------------------------------------------------------------
// Stack arithmetic - take_n.
// ---------------------------------------------------------------

#[test]
fn take_n_exact() {
    let mut slot = Some((3, 10));
    let taken = take_n(&mut slot, 4);
    assert_eq!(taken, Some((3, 4)));
    assert_eq!(slot, Some((3, 6)));
}

#[test]
fn take_n_too_many() {
    let mut slot = Some((3, 5));
    let taken = take_n(&mut slot, 10);
    assert!(taken.is_none());
    // Slot untouched.
    assert_eq!(slot, Some((3, 5)));
}

// ---------------------------------------------------------------
// Stack arithmetic - drop_into_first_free.
// ---------------------------------------------------------------

#[test]
fn drop_into_first_free_fills_first_empty() {
    let mut inv: [Option<(u16, u8)>; 4] = [
        Some((1, 3)),
        None,
        Some((2, 7)),
        None,
    ];
    // Item 4 (sand) doesn't match any existing slot, so it lands
    // in the first empty slot (index 1).
    let leftover = drop_into_first_free(&mut inv, (4, 5));
    assert_eq!(leftover, 0);
    assert_eq!(inv[0], Some((1, 3)));
    assert_eq!(inv[1], Some((4, 5)));
    assert_eq!(inv[2], Some((2, 7)));
    assert_eq!(inv[3], None);
}

#[test]
fn drop_into_first_free_tops_up_existing_stacks_first() {
    // Two existing partial stacks of the same item should fill
    // up before we claim an empty slot.
    let mut inv: [Option<(u16, u8)>; 3] = [
        Some((3, 60)), // room for 4 more
        None,
        Some((3, 50)), // room for 14 more
    ];
    let leftover = drop_into_first_free(&mut inv, (3, 10));
    // 4 into slot 0 -> (3, 64), 6 into slot 2 -> (3, 56), empty
    // slot untouched, leftover 0.
    assert_eq!(leftover, 0);
    assert_eq!(inv[0], Some((3, 64)));
    assert_eq!(inv[1], None);
    assert_eq!(inv[2], Some((3, 56)));
}

#[test]
fn drop_into_first_free_overflows_into_next() {
    // 3 partial same-item slots that together don't quite fit
    // the incoming stack. Remainder ends up in the next free
    // slot when one exists.
    let mut inv: [Option<(u16, u8)>; 4] = [
        Some((3, 62)), // room 2
        Some((3, 60)), // room 4
        Some((3, 58)), // room 6
        None,
    ];
    let leftover = drop_into_first_free(&mut inv, (3, 20));
    // 2 + 4 + 6 = 12 absorbed, 8 spills into slot 3.
    assert_eq!(leftover, 0);
    assert_eq!(inv[0], Some((3, 64)));
    assert_eq!(inv[1], Some((3, 64)));
    assert_eq!(inv[2], Some((3, 64)));
    assert_eq!(inv[3], Some((3, 8)));
}

#[test]
fn drop_into_first_free_returns_leftover_when_inv_full() {
    let mut inv: [Option<(u16, u8)>; 2] = [
        Some((1, 64)),
        Some((2, 64)),
    ];
    // No room for sand: full count comes back as leftover.
    let leftover = drop_into_first_free(&mut inv, (4, 5));
    assert_eq!(leftover, 5);
    assert_eq!(inv[0], Some((1, 64)));
    assert_eq!(inv[1], Some((2, 64)));
}
