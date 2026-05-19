//! W11.6 - Wave 11 lock test.
//!
//! Aggregates the W11.1 flammability table, W11.2 emission table,
//! and W11.4 command-tree audits into a single regression test
//! that catches any silent drift in the MC-fidelity tables.
//!
//! The W11.{1,2,4} modules already have their own per-table
//! checksum tests; this file's `wave_11_locked` repeats the size
//! and checksum checks side-by-side so a single CI failure points
//! at the whole wave instead of one module at a time.

#![cfg(test)]

use crate::block::{flammability, light_emission};
use crate::chat::autocomplete;
use crate::chat::cmd_level;

#[test]
fn wave_11_locked() {
    // W11.1 flammability table.
    assert_eq!(
        flammability::MC_FLAMMABLES.len(),
        207,
        "MC flammability row count drift",
    );
    assert_eq!(
        flammability::flammability_checksum(),
        0x6B584F067ED80FE6,
        "MC flammability checksum drift",
    );

    // W11.2 emission table.
    assert_eq!(
        light_emission::MC_EMITTERS.len(),
        65,
        "MC emission row count drift",
    );
    assert_eq!(
        light_emission::emission_checksum(),
        0xEF3D3956269C5D2A,
        "MC emission checksum drift",
    );

    // W11.4 command-tree audit. The autocomplete module owns a
    // per-tree test (`tree_contains_every_cmd_meta_verb`) that
    // already locks the verb coverage; here we just sanity-check
    // the root width to catch a refactor that drops a chunk of
    // commands.
    let tree = autocomplete::build_tree();
    assert!(
        tree.children.len() >= 70,
        "command tree root has only {} children",
        tree.children.len()
    );
}

#[test]
fn wave_11_completion_basics() {
    // Sanity: typing "/" with full perms returns at least 70
    // candidates (the verb count).
    let r = autocomplete::complete(
        "/",
        1,
        4,
        &autocomplete::CompletionContext::default(),
    );
    assert!(
        r.len() >= 70,
        "expected >= 70 completion candidates at /, got {}",
        r.len()
    );
}

#[test]
fn wave_11_emission_table_covers_w11_3_emitter_ids() {
    // Every emitter id added in W11.3 has a non-zero emission
    // entry routed through the W11.2 resolver.
    use crate::block::{
        CRYING_OBSIDIAN_ID, END_ROD_ID, ENDER_CHEST_ID, JACK_O_LANTERN_ID,
        LANTERN_ID, LIGHT_BLOCK_ID, LIT_REDSTONE_LAMP_ID, LIT_REDSTONE_ORE_ID,
        MAGMA_BLOCK_ID, REDSTONE_TORCH_ID, REDSTONE_WALL_TORCH_ID,
        SEA_LANTERN_ID, SHROOMLIGHT_ID, SOUL_FIRE_ID, SOUL_LANTERN_ID,
        SOUL_TORCH_ID, SOUL_WALL_TORCH_ID,
    };
    for id in [
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
        ENDER_CHEST_ID,
        LIT_REDSTONE_LAMP_ID,
        LIT_REDSTONE_ORE_ID,
        LIGHT_BLOCK_ID,
    ] {
        assert!(
            light_emission::light_emission_for_id(id) > 0,
            "emitter id {id} resolves to 0"
        );
    }
}

#[test]
fn wave_11_flammability_engine_id_mappings() {
    // Engine ids that map to MC names produce the right values.
    use crate::block::{LEAVES_ID, PLANKS_ID, WOOD_ID, WOOL_ID};
    assert_eq!(flammability::flammability_for_id(WOOD_ID), (5, 5));
    assert_eq!(flammability::flammability_for_id(PLANKS_ID), (5, 20));
    assert_eq!(flammability::flammability_for_id(LEAVES_ID), (30, 60));
    assert_eq!(flammability::flammability_for_id(WOOL_ID), (30, 60));
}

#[test]
fn wave_11_command_perms_intact() {
    // Spot check: every command kept the expected perm level.
    assert_eq!(cmd_level("help"), Some(0));
    assert_eq!(cmd_level("tp"), Some(2));
    assert_eq!(cmd_level("setblock"), Some(3));
    assert_eq!(cmd_level("save"), Some(4));
}
