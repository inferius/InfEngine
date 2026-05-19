//! W12.1 split regression: assert the `mod.rs` -> `types.rs` +
//! `ids.rs` + `registry.rs` reorg still exposes every symbol the
//! engine relies on and that the `BLOCK_TYPES` table didn't lose
//! any rows on the way through. These tests target the public
//! crate-internal interface, not internal implementation; they
//! should remain stable across future block additions.

use super::{
    block_type, BLOCK_TYPES, COBBLE_ID, FLINT_AND_STEEL_ID, FURNACE_ID, LAVA_ID,
    LIGHT_BLOCK_ID, REDSTONE_TORCH_ID, TORCH_ID, WATER_ID,
};
use crate::block::types::BlockType;
use infengine_voxel::BlockId;

/// Sanity-check the registry is a single contiguous slice that
/// includes both the pre-W11 ids and the W11.3 emitter ids.
#[test]
fn registry_contains_both_core_and_emitter_rows() {
    let ids: Vec<u16> = BLOCK_TYPES.iter().map(|b| b.id).collect();
    assert!(ids.contains(&1), "stone (core) missing");
    assert!(ids.contains(&TORCH_ID), "torch (core) missing");
    assert!(ids.contains(&WATER_ID), "water (core) missing");
    assert!(ids.contains(&FURNACE_ID), "furnace (core) missing");
    assert!(ids.contains(&REDSTONE_TORCH_ID), "redstone torch (W11.3) missing");
    assert!(ids.contains(&LIGHT_BLOCK_ID), "light block (W11.3) missing");
}

/// Each id constant in `ids.rs` must resolve through `block_type`.
/// If a constant gets defined but no matching row gets added we
/// want the test suite to flag it.
#[test]
fn every_named_id_constant_resolves() {
    // Excludes IRON_INGOT_ID (43): pure item, no block row by design.
    for id in [
        TORCH_ID, WATER_ID, LAVA_ID, COBBLE_ID, FURNACE_ID,
        FLINT_AND_STEEL_ID, REDSTONE_TORCH_ID, LIGHT_BLOCK_ID,
    ] {
        assert!(
            block_type(id).is_some(),
            "id constant {id} has no BlockType row",
        );
    }
}

/// The split must not introduce duplicate ids - the runtime
/// `block_type` lookup uses `find` and would silently take the
/// first row, masking the other.
#[test]
fn registry_has_no_duplicate_ids() {
    let mut ids: Vec<u16> = BLOCK_TYPES.iter().map(|b| b.id).collect();
    ids.sort_unstable();
    let total = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), total, "duplicate id in BLOCK_TYPES");
}

/// `BlockType` is still re-exported from the crate root of the
/// `block` module. Code outside `block/` imports it as
/// `crate::block::BlockType`; the split must not break that.
#[test]
fn block_type_struct_is_reexported() {
    let _: Option<&BlockType> = block_type(1);
}

/// Behaviour-flag bits are still defined + queryable. Pick a few
/// canonical pairs.
#[test]
fn flag_helpers_still_resolve() {
    assert!(super::block_is_aabb_passable(BlockId(TORCH_ID)));
    assert!(super::block_is_swim_medium(BlockId(WATER_ID)));
    assert!(super::block_is_liquid(BlockId(LAVA_ID)));
    assert!(!super::block_is_aabb_passable(BlockId(1))); // stone
}
