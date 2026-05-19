//! W12.10 - Wave 12 lock test.
//!
//! Aggregates the W12.1 module split, W12.2 special-block roster,
//! and W12.9 creative-category partition into a single regression
//! test that catches silent drift across the wave.
//!
//! The W12.{1,2,3,4,5,6,7,8,9} per-module test files already lock
//! the individual pieces; this file's `wave_12_locked` re-asserts
//! the headline contracts side-by-side so a single CI failure
//! points at the whole wave rather than one module at a time.

#![cfg(test)]

use crate::block::{
    block_type, BLOCK_TYPES, COBBLESTONE_SLAB_ID, COBBLESTONE_STAIRS_ID,
    COBBLESTONE_WALL_ID, GLASS_PANE_ID, IRON_BARS_ID, IRON_DOOR_ID,
    NETHER_BRICK_FENCE_ID, OAK_DOOR_ID, OAK_DOOR_UPPER_ID, OAK_FENCE_ID,
    OAK_SLAB_ID, OAK_SLAB_TOP_ID, OAK_STAIRS_ID, STONE_SLAB_ID, STONE_STAIRS_ID,
    F_DEDICATED_RENDER,
};
use crate::creative_tab::CreativeCategory;
use crate::item::{ItemCategory, ITEM_TYPES};
use crate::resource_pack::special_models::{model_for_block, name_for_block};

/// Every W12.2 id. Pinned here so a future block addition that
/// shifts the slab/fence/door range raises an obvious diff.
const W12_2_IDS: &[u16] = &[
    OAK_SLAB_ID,
    STONE_SLAB_ID,
    COBBLESTONE_SLAB_ID,
    OAK_SLAB_TOP_ID,
    OAK_STAIRS_ID,
    STONE_STAIRS_ID,
    COBBLESTONE_STAIRS_ID,
    OAK_FENCE_ID,
    NETHER_BRICK_FENCE_ID,
    OAK_DOOR_ID,
    OAK_DOOR_UPPER_ID,
    IRON_DOOR_ID,
    GLASS_PANE_ID,
    IRON_BARS_ID,
    COBBLESTONE_WALL_ID,
];

#[test]
fn wave_12_locked() {
    // 1. W12.2 ids are contiguous 68..=82.
    let mut sorted: Vec<u16> = W12_2_IDS.to_vec();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        (68..=82).collect::<Vec<u16>>(),
        "W12.2 block ids not contiguous 68..=82",
    );

    // 2. Each W12.2 block has a BlockType row + F_DEDICATED_RENDER.
    for &id in W12_2_IDS {
        let bt = block_type(id)
            .unwrap_or_else(|| panic!("W12.2 id {id} missing BlockType row"));
        assert!(
            (bt.flags & F_DEDICATED_RENDER) != 0,
            "W12.2 id {} ({}) missing F_DEDICATED_RENDER", id, bt.name,
        );
    }

    // 3. Each W12.2 block routes through special_models. The chunk
    // mesher dispatches via this function; missing entries would
    // emit a default cube + lose the slab/stairs/door shape.
    for &id in W12_2_IDS {
        assert!(
            name_for_block(id).is_some(),
            "W12.2 id {id} has no special_models routing",
        );
        assert!(
            model_for_block(id).is_some(),
            "W12.2 id {id} model lookup falls through",
        );
    }

    // 4. block/mod.rs is below 500 LOC. The W12.1 split moved the
    // big tables out; we want the soft cap to stay enforced as
    // future blocks land.
    let mod_rs = std::fs::read_to_string(
        "examples/voxel_demo_mp/src/block/mod.rs",
    )
    .or_else(|_| std::fs::read_to_string("src/block/mod.rs"))
    .expect("read block/mod.rs");
    let lines = mod_rs.lines().count();
    assert!(
        lines < 500,
        "block/mod.rs grew past the 500 LOC soft cap ({lines} lines)",
    );

    // 5. CreativeCategory partition is total. Every ItemCategory
    // variant lands in exactly one non-All tab so the multi-tab
    // strip is exhaustive.
    let strip: Vec<CreativeCategory> = CreativeCategory::all()
        .iter()
        .copied()
        .filter(|c| !matches!(c, CreativeCategory::All))
        .collect();
    for it in ITEM_TYPES {
        let mut hits = 0;
        for cat in &strip {
            if cat.accepts(it.category) {
                hits += 1;
            }
        }
        assert_eq!(
            hits, 1,
            "ItemCategory::{:?} (id {}) lands in {hits} tabs, want 1",
            it.category, it.id,
        );
    }

    // 6. BLOCK_TYPES still has every W11.3 + W12.2 row. Loose lower
    // bound catches a refactor that accidentally drops part of
    // the registry.
    assert!(
        BLOCK_TYPES.len() >= 60,
        "BLOCK_TYPES shrank below the W11/W12 baseline",
    );

    // 7. Every ItemType row has a category set (the W12.9 strip
    // assumes this).
    for it in ITEM_TYPES {
        // ItemCategory is exhaustive over Item kinds. Every row
        // populates the field at decl time; the compiler enforces
        // it. This assertion is the runtime read-back; if a future
        // refactor swaps it to Option<ItemCategory> the test
        // catches the change.
        let _: ItemCategory = it.category;
    }
}
