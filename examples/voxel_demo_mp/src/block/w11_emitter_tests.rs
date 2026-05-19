//! W11.3 - Tests for the new emitter + cosmetic block types
//! added to BLOCK_TYPES (id range 50..=67).

#![cfg(test)]

use super::{
    BLOCK_TYPES, CRYING_OBSIDIAN_ID, END_ROD_ID, ENDER_CHEST_ID,
    JACK_O_LANTERN_ID, LANTERN_ID, LIGHT_BLOCK_ID, LIT_REDSTONE_LAMP_ID,
    LIT_REDSTONE_ORE_ID, MAGMA_BLOCK_ID, REDSTONE_TORCH_ID,
    REDSTONE_WALL_TORCH_ID, RESPAWN_ANCHOR_ID, SEA_LANTERN_ID, SHROOMLIGHT_ID,
    SOUL_FIRE_ID, SOUL_LANTERN_ID, SOUL_TORCH_ID, SOUL_WALL_TORCH_ID,
    block_type, light_emission,
};

/// Every new emitter id has a BlockType row in BLOCK_TYPES + a
/// non-zero `light_emission` value (or zero for the explicit
/// uncharged-respawn-anchor case). This catches a future edit
/// that adds a new id constant without a matching block row.
const NEW_EMITTER_IDS: &[u16] = &[
    REDSTONE_TORCH_ID,
    REDSTONE_WALL_TORCH_ID,
    SOUL_TORCH_ID,
    SOUL_WALL_TORCH_ID,
    SEA_LANTERN_ID,
    JACK_O_LANTERN_ID,
    SOUL_FIRE_ID,
    END_ROD_ID,
    MAGMA_BLOCK_ID,
    SHROOMLIGHT_ID,
    LANTERN_ID,
    SOUL_LANTERN_ID,
    CRYING_OBSIDIAN_ID,
    RESPAWN_ANCHOR_ID,
    ENDER_CHEST_ID,
    LIT_REDSTONE_LAMP_ID,
    LIT_REDSTONE_ORE_ID,
    LIGHT_BLOCK_ID,
];

#[test]
fn redstone_torch_in_block_types() {
    let bt = block_type(REDSTONE_TORCH_ID).expect("redstone_torch row");
    assert_eq!(bt.id, 50);
    assert_eq!(bt.name, "redstone_torch");
}

#[test]
fn redstone_wall_torch_in_block_types() {
    let bt = block_type(REDSTONE_WALL_TORCH_ID).expect("redstone_wall_torch row");
    assert_eq!(bt.id, 51);
    assert_eq!(bt.name, "redstone_wall_torch");
}

#[test]
fn soul_torch_in_block_types() {
    let bt = block_type(SOUL_TORCH_ID).expect("soul_torch row");
    assert_eq!(bt.id, 52);
    assert_eq!(bt.name, "soul_torch");
}

#[test]
fn soul_wall_torch_in_block_types() {
    let bt = block_type(SOUL_WALL_TORCH_ID).expect("soul_wall_torch row");
    assert_eq!(bt.id, 53);
    assert_eq!(bt.name, "soul_wall_torch");
}

#[test]
fn sea_lantern_in_block_types() {
    let bt = block_type(SEA_LANTERN_ID).expect("sea_lantern row");
    assert_eq!(bt.id, 54);
    assert_eq!(bt.name, "sea_lantern");
}

#[test]
fn jack_o_lantern_in_block_types() {
    let bt = block_type(JACK_O_LANTERN_ID).expect("jack_o_lantern row");
    assert_eq!(bt.id, 55);
    assert_eq!(bt.name, "jack_o_lantern");
}

#[test]
fn soul_fire_in_block_types() {
    let bt = block_type(SOUL_FIRE_ID).expect("soul_fire row");
    assert_eq!(bt.id, 56);
    assert_eq!(bt.name, "soul_fire");
}

#[test]
fn end_rod_in_block_types() {
    let bt = block_type(END_ROD_ID).expect("end_rod row");
    assert_eq!(bt.id, 57);
    assert_eq!(bt.name, "end_rod");
}

#[test]
fn magma_block_in_block_types() {
    let bt = block_type(MAGMA_BLOCK_ID).expect("magma_block row");
    assert_eq!(bt.id, 58);
    assert_eq!(bt.name, "magma_block");
}

#[test]
fn shroomlight_in_block_types() {
    let bt = block_type(SHROOMLIGHT_ID).expect("shroomlight row");
    assert_eq!(bt.id, 59);
    assert_eq!(bt.name, "shroomlight");
}

#[test]
fn lantern_in_block_types() {
    let bt = block_type(LANTERN_ID).expect("lantern row");
    assert_eq!(bt.id, 60);
    assert_eq!(bt.name, "lantern");
}

#[test]
fn soul_lantern_in_block_types() {
    let bt = block_type(SOUL_LANTERN_ID).expect("soul_lantern row");
    assert_eq!(bt.id, 61);
    assert_eq!(bt.name, "soul_lantern");
}

#[test]
fn crying_obsidian_in_block_types() {
    let bt = block_type(CRYING_OBSIDIAN_ID).expect("crying_obsidian row");
    assert_eq!(bt.id, 62);
    assert_eq!(bt.name, "crying_obsidian");
}

#[test]
fn respawn_anchor_in_block_types() {
    let bt = block_type(RESPAWN_ANCHOR_ID).expect("respawn_anchor row");
    assert_eq!(bt.id, 63);
    assert_eq!(bt.name, "respawn_anchor");
}

#[test]
fn ender_chest_in_block_types() {
    let bt = block_type(ENDER_CHEST_ID).expect("ender_chest row");
    assert_eq!(bt.id, 64);
    assert_eq!(bt.name, "ender_chest");
}

#[test]
fn lit_redstone_lamp_in_block_types() {
    let bt = block_type(LIT_REDSTONE_LAMP_ID).expect("lit_redstone_lamp row");
    assert_eq!(bt.id, 65);
    assert_eq!(bt.name, "lit_redstone_lamp");
}

#[test]
fn lit_redstone_ore_in_block_types() {
    let bt = block_type(LIT_REDSTONE_ORE_ID).expect("lit_redstone_ore row");
    assert_eq!(bt.id, 66);
    assert_eq!(bt.name, "lit_redstone_ore");
}

#[test]
fn light_block_in_block_types() {
    let bt = block_type(LIGHT_BLOCK_ID).expect("light_block row");
    assert_eq!(bt.id, 67);
    assert_eq!(bt.name, "light_block");
}

#[test]
fn every_new_emitter_id_has_light_emission() {
    // Each new emitter id has a non-zero MC emission. RESPAWN_ANCHOR
    // resolves to the MC table's MAX (15) because we store the MAX
    // value; the engine has no per-state respawn anchor today so any
    // placed respawn_anchor lights at 15 (the fully-charged value).
    // This is intentional: when state lands we'll route through a
    // per-state lookup that returns 0 for uncharged.
    for &id in NEW_EMITTER_IDS {
        let level = light_emission::light_emission_for_id(id);
        assert!(level > 0, "emitter id {id} has no light emission");
    }
}

#[test]
fn new_emitter_block_count_matches_table() {
    // Spot check the table layout: ids 50..=67 (18 rows). If a
    // future edit drops one without removing this test, the
    // assertion will fail.
    let new_rows = BLOCK_TYPES
        .iter()
        .filter(|b| (50..=67).contains(&b.id))
        .count();
    assert_eq!(new_rows, 18, "expected 18 new emitter+cosmetic rows");
}

#[test]
fn light_block_passes_ray_and_aabb() {
    use super::{block_is_aabb_passable, block_is_ray_passable};
    use infengine_voxel::BlockId;
    let light = BlockId(LIGHT_BLOCK_ID);
    assert!(block_is_ray_passable(light));
    assert!(block_is_aabb_passable(light));
}

#[test]
fn soul_fire_shares_fire_flags() {
    use super::{
        F_AABB_PASS, F_DEDICATED_RENDER, F_RAY_PASS, F_REPLACEABLE,
        F_TRANSPARENT_RENDER, F_WASHABLE, block_has_flag,
    };
    use infengine_voxel::BlockId;
    let sf = BlockId(SOUL_FIRE_ID);
    assert!(block_has_flag(sf, F_RAY_PASS));
    assert!(block_has_flag(sf, F_AABB_PASS));
    assert!(block_has_flag(sf, F_REPLACEABLE));
    assert!(block_has_flag(sf, F_TRANSPARENT_RENDER));
    assert!(block_has_flag(sf, F_DEDICATED_RENDER));
    assert!(block_has_flag(sf, F_WASHABLE));
}
