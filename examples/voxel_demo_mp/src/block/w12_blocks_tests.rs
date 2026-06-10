//! W12.2 regression tests: slab / fence / door / stairs / pane /
//! bars / wall block ids must be present in the registry, must
//! resolve as proper ItemType placeables, and must carry the
//! F_DEDICATED_RENDER flag so the chunk mesher routes through the
//! special-models emission path. The matching `name_for_block` ->
//! `model_for_block` tests live in `resource_pack::special_models`
//! since `resource_pack` is a `main.rs`-only module.
//!
//! Cite: `J:\Temp\template-mod-template-26.1.2\sources\
//! minecraft-merged-52430b475d-26.1.2-sources\
//! net\minecraft\world\level\block\state\properties\` -
//! per-block state properties (Slab `top`/`bottom`/`double`,
//! Door `open`/`hinge`/`facing`/`half`/`powered`).

use super::{
    block_skips_chunk_mesh, block_type, COBBLESTONE_SLAB_ID, COBBLESTONE_STAIRS_ID,
    COBBLESTONE_WALL_ID, F_DEDICATED_RENDER, GLASS_PANE_ID, IRON_BARS_ID, IRON_DOOR_ID,
    NETHER_BRICK_FENCE_ID, OAK_DOOR_ID, OAK_DOOR_UPPER_ID, OAK_FENCE_ID, OAK_SLAB_ID,
    OAK_SLAB_TOP_ID, OAK_STAIRS_ID, STONE_SLAB_ID, STONE_STAIRS_ID,
};
use infengine_voxel::BlockId;

const ALL_W12_IDS: &[u16] = &[
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
fn every_w12_block_has_a_registry_row() {
    for &id in ALL_W12_IDS {
        assert!(
            block_type(id).is_some(),
            "id {id} missing from BLOCK_TYPES",
        );
    }
}

#[test]
fn every_w12_block_carries_dedicated_render_flag() {
    // W60.1: block_skips_chunk_mesh now hard-codes the
    // dedicated-pass id list (torch / snow / liquid / portal /
    // fire). Slabs / stairs / fences / doors / panes / walls
    // emit special-model geometry into the chunk submesh and
    // need that submesh uploaded - they MUST NOT skip.
    // F_DEDICATED_RENDER flag still flips for legacy callers
    // that scan it; the rendering decision lives in
    // block_skips_chunk_mesh.
    for &id in ALL_W12_IDS {
        let bt = block_type(id).unwrap_or_else(|| panic!("id {id}"));
        assert!(
            (bt.flags & F_DEDICATED_RENDER) != 0,
            "id {} ({}) missing F_DEDICATED_RENDER", id, bt.name,
        );
        assert!(
            !block_skips_chunk_mesh(BlockId(id)),
            "id {} ({}) is special-model + must NOT skip chunk mesh", id, bt.name,
        );
    }
}

#[test]
fn w12_block_ids_are_contiguous_68_to_82() {
    let mut sorted: Vec<u16> = ALL_W12_IDS.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, (68..=82).collect::<Vec<u16>>());
}

#[test]
fn every_w12_block_has_a_name() {
    for &id in ALL_W12_IDS {
        let bt = block_type(id).unwrap();
        assert!(!bt.name.is_empty(), "id {id} blank name");
    }
}

#[test]
fn w12_thin_blocks_are_aabb_passable() {
    // Fences / glass panes / iron bars stay flag-passable (W12.2
    // simplification). W125: doors are NOT flag-passable any more -
    // a closed door is solid + ray-targetable; the OPEN state passes
    // through the door_open lookup in `voxel_is_solid_with_doors`
    // (the unconditional flags were why closed doors were walk-
    // through and untargetable - "doors don't work at all").
    use super::block_is_aabb_passable;
    for id in [OAK_FENCE_ID, NETHER_BRICK_FENCE_ID, GLASS_PANE_ID, IRON_BARS_ID] {
        assert!(
            block_is_aabb_passable(BlockId(id)),
            "id {id} should be AABB passable",
        );
    }
    for id in [OAK_DOOR_ID, OAK_DOOR_UPPER_ID, IRON_DOOR_ID] {
        assert!(
            !block_is_aabb_passable(BlockId(id)),
            "id {id} must be solid when closed (state-aware pass)",
        );
        assert!(
            !super::block_is_ray_passable(BlockId(id)),
            "id {id} must be ray-targetable",
        );
    }
}
