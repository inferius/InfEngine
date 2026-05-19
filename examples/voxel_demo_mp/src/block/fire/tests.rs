//! Q.4c: fire random-tick algorithm tests.
//!
//! Drives `plan_fire_cell` against a hand-laid world snapshot.
//! Determinism comes from the hash3_for-based gating - tests
//! pick a `fire_clock` value at which the relevant random
//! decisions fire, mirroring how MC's RNG-seed-based tests in
//! `FireBlockTest` work.

use std::collections::HashMap;

use infengine_voxel::{BlockId, Chunk, World};

use super::*;
use crate::block::{FIRE_ID, LEAVES_ID, PLANKS_ID, STONE_ID, WOOD_ID, WOOL_ID};

fn world_with_chunks() -> World {
    let mut w = World::new();
    for cx in -1..=1 {
        for cy in -1..=1 {
            for cz in -1..=1 {
                w.insert_chunk([cx, cy, cz], Chunk::empty());
            }
        }
    }
    w
}

/// Place a fire cell at `pos` with `age`. Sets the world block
/// id + the `fires` age entry to mirror the canonical state shape.
fn place_fire(w: &mut World, fires: &mut HashMap<[i32; 3], u8>, pos: [i32; 3], age: u8) {
    w.set(pos, BlockId(FIRE_ID));
    fires.insert(pos, age);
}

// ----------------------------------------------------------------
// Flammability lookup.
// ----------------------------------------------------------------

#[test]
fn planks_flammability_matches_mc_5_20() {
    assert_eq!(block_flammability(PLANKS_ID), (5, 20));
}

#[test]
fn wood_flammability_matches_mc_5_5() {
    assert_eq!(block_flammability(WOOD_ID), (5, 5));
}

#[test]
fn leaves_flammability_matches_mc_30_60() {
    // Per MC FireBlock.bootStrap line 393.
    assert_eq!(block_flammability(LEAVES_ID), (30, 60));
}

#[test]
fn wool_flammability_matches_mc_30_60() {
    assert_eq!(block_flammability(WOOL_ID), (30, 60));
}

#[test]
fn stone_is_unburnable() {
    assert_eq!(block_flammability(STONE_ID), (0, 0));
}

#[test]
fn unknown_block_is_unburnable() {
    assert_eq!(block_flammability(0xFFFF), (0, 0));
}

// ----------------------------------------------------------------
// has_flammable_neighbour + has_fire_support.
// ----------------------------------------------------------------

#[test]
fn lone_air_cell_has_no_flammable_neighbour() {
    let w = world_with_chunks();
    assert!(!has_flammable_neighbour(&w, [0, 0, 0]));
}

#[test]
fn cell_next_to_planks_has_flammable_neighbour() {
    let mut w = world_with_chunks();
    w.set([1, 0, 0], BlockId(PLANKS_ID));
    assert!(has_flammable_neighbour(&w, [0, 0, 0]));
}

#[test]
fn cell_with_stone_neighbour_only_is_not_flammable() {
    let mut w = world_with_chunks();
    w.set([1, 0, 0], BlockId(STONE_ID));
    assert!(!has_flammable_neighbour(&w, [0, 0, 0]));
}

#[test]
fn fire_on_stone_floor_is_supported() {
    let mut w = world_with_chunks();
    w.set([0, -1, 0], BlockId(STONE_ID));
    assert!(has_fire_support(&w, [0, 0, 0]));
}

#[test]
fn fire_with_planks_neighbour_is_supported_even_without_floor() {
    let mut w = world_with_chunks();
    w.set([1, 0, 0], BlockId(PLANKS_ID));
    assert!(has_fire_support(&w, [0, 0, 0]));
}

#[test]
fn fire_in_open_air_with_no_neighbours_is_unsupported() {
    let w = world_with_chunks();
    assert!(!has_fire_support(&w, [0, 0, 0]));
}

// ----------------------------------------------------------------
// try_ignite_block: simple gating.
// ----------------------------------------------------------------

#[test]
fn try_ignite_air_with_support_succeeds() {
    let mut w = world_with_chunks();
    w.set([0, -1, 0], BlockId(STONE_ID));
    w.set([1, 0, 0], BlockId(PLANKS_ID));
    assert_eq!(try_ignite_block(&w, [0, 0, 0]), Some(0));
}

#[test]
fn try_ignite_air_without_support_fails() {
    let w = world_with_chunks();
    assert!(try_ignite_block(&w, [0, 0, 0]).is_none());
}

#[test]
fn try_ignite_solid_block_fails() {
    let mut w = world_with_chunks();
    w.set([0, -1, 0], BlockId(STONE_ID));
    w.set([0, 0, 0], BlockId(STONE_ID));
    assert!(try_ignite_block(&w, [0, 0, 0]).is_none());
}

#[test]
fn try_ignite_already_burning_fails() {
    let mut w = world_with_chunks();
    w.set([0, -1, 0], BlockId(STONE_ID));
    w.set([0, 0, 0], BlockId(FIRE_ID));
    assert!(try_ignite_block(&w, [0, 0, 0]).is_none());
}

// ----------------------------------------------------------------
// plan_fire_cell behaviour.
// ----------------------------------------------------------------

#[test]
fn fire_with_no_support_extinguishes_immediately() {
    // Fire in pure air with no flammable neighbours -> die.
    let mut w = world_with_chunks();
    let mut fires = HashMap::new();
    place_fire(&mut w, &mut fires, [0, 5, 0], 0);
    let mut step = FireStep::default();
    plan_fire_cell(&w, &fires, [0, 5, 0], 1, &mut step);
    assert_eq!(step.extinguish, vec![[0, 5, 0]]);
}

#[test]
fn fire_on_stone_with_no_flammables_may_die_within_n_ticks() {
    // Supported by stone but isolated -> 1/3 die chance per
    // tick. Across 30 deterministic clocks it MUST hit at
    // least once (probability 1 - (2/3)^30 ~= 1).
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    let mut fires = HashMap::new();
    place_fire(&mut w, &mut fires, [0, 1, 0], 0);
    let mut hit = false;
    for clock in 1..=30u64 {
        let mut step = FireStep::default();
        plan_fire_cell(&w, &fires, [0, 1, 0], clock, &mut step);
        if step.extinguish.contains(&[0, 1, 0]) {
            hit = true;
            break;
        }
    }
    assert!(hit, "fire on stone with no flammables never died across 30 ticks");
}

#[test]
fn fire_age_15_no_flammable_below_may_extinguish() {
    // age == 15 + below isn't flammable -> 25% die chance. Run
    // 10 clocks; expect at least one hit (1 - 0.75^10 ~= 94%).
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    // Add a planks neighbour so the cell IS supported but the
    // "below flammable" check still misses.
    w.set([1, 1, 0], BlockId(PLANKS_ID));
    let mut fires = HashMap::new();
    place_fire(&mut w, &mut fires, [0, 1, 0], 15);
    let mut hit = false;
    for clock in 1..=20u64 {
        let mut step = FireStep::default();
        plan_fire_cell(&w, &fires, [0, 1, 0], clock, &mut step);
        if step.extinguish.contains(&[0, 1, 0]) {
            hit = true;
            break;
        }
    }
    assert!(hit, "age-15 fire never extinguished across 20 ticks");
}

#[test]
fn fire_next_to_planks_spreads_within_n_ticks() {
    // Fire on stone, planks neighbour. Spread or burn-consume
    // both write something to the step.
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    w.set([1, 0, 0], BlockId(STONE_ID));
    w.set([2, 0, 0], BlockId(STONE_ID));
    w.set([1, 1, 0], BlockId(PLANKS_ID));
    w.set([2, 1, 0], BlockId(PLANKS_ID));
    let mut fires = HashMap::new();
    place_fire(&mut w, &mut fires, [0, 1, 0], 0);
    let mut hit = false;
    for clock in 1..=200u64 {
        let mut step = FireStep::default();
        plan_fire_cell(&w, &fires, [0, 1, 0], clock, &mut step);
        if !step.ignite.is_empty() || !step.consume.is_empty() {
            hit = true;
            break;
        }
    }
    assert!(hit, "no spread or consume across 200 ticks next to planks");
}

#[test]
fn fire_next_to_leaves_burns_or_spreads_fast() {
    // Leaves catch fire FAST (30, 60) - we expect a hit within
    // 60 ticks vs ~200 for planks.
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    w.set([1, 0, 0], BlockId(STONE_ID));
    w.set([1, 1, 0], BlockId(LEAVES_ID));
    let mut fires = HashMap::new();
    place_fire(&mut w, &mut fires, [0, 1, 0], 0);
    let mut hit_clock = None;
    for clock in 1..=60u64 {
        let mut step = FireStep::default();
        plan_fire_cell(&w, &fires, [0, 1, 0], clock, &mut step);
        if !step.ignite.is_empty() || !step.consume.is_empty() {
            hit_clock = Some(clock);
            break;
        }
    }
    assert!(
        hit_clock.is_some(),
        "leaves never burned across 60 ticks (expected fast)"
    );
}

#[test]
fn fire_on_wool_burns() {
    // Wool's (30, 60) makes it fast-burning. Place the wool as
    // a FACE neighbour of the fire (the check_burn_out scan
    // only walks the 6 cardinal faces).
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    // wool directly east of fire
    w.set([1, 1, 0], BlockId(WOOL_ID));
    let mut fires = HashMap::new();
    place_fire(&mut w, &mut fires, [0, 1, 0], 0);
    let mut hit = false;
    for clock in 1..=200u64 {
        let mut step = FireStep::default();
        plan_fire_cell(&w, &fires, [0, 1, 0], clock, &mut step);
        if step.consume.contains(&[1, 1, 0])
            || step.ignite.iter().any(|(p, _)| *p == [1, 1, 0])
        {
            hit = true;
            break;
        }
    }
    assert!(hit, "wool face-neighbour never burned across 200 ticks");
}

#[test]
fn stone_neighbour_does_not_burn() {
    // Fire on stone with only stone neighbours can't spread
    // because the flammability table returns 0,0 for stone.
    let mut w = world_with_chunks();
    for dx in -2..=2 {
        for dz in -2..=2 {
            w.set([dx, 0, dz], BlockId(STONE_ID));
        }
    }
    let mut fires = HashMap::new();
    place_fire(&mut w, &mut fires, [0, 1, 0], 0);
    // Across many ticks the only allowed transition is
    // extinguish (isolated fire). No ignite / consume of stone.
    for clock in 1..=200u64 {
        let mut step = FireStep::default();
        plan_fire_cell(&w, &fires, [0, 1, 0], clock, &mut step);
        for (pos, _) in &step.ignite {
            // The only way ignite fires is via the spread scan
            // hitting air cells. Stone is never on the list.
            assert_ne!(*pos, [1, 0, 0], "stone block ignited (bug)");
        }
        assert!(
            step.consume.iter().all(|p| *p != [1, 0, 0]),
            "stone block consumed (bug)"
        );
    }
}

#[test]
fn fire_step_default_is_empty() {
    let s = FireStep::default();
    assert!(s.extinguish.is_empty());
    assert!(s.ignite.is_empty());
    assert!(s.consume.is_empty());
    assert!(s.age_bump.is_empty());
}

#[test]
fn hash3_is_deterministic_for_same_inputs() {
    let a = hash3_for([1, 2, 3], 42, 0xAA);
    let b = hash3_for([1, 2, 3], 42, 0xAA);
    assert_eq!(a, b);
}

#[test]
fn hash3_changes_with_clock() {
    let a = hash3_for([1, 2, 3], 42, 0xAA);
    let b = hash3_for([1, 2, 3], 43, 0xAA);
    assert_ne!(a, b);
}

#[test]
fn hash3_changes_with_salt() {
    let a = hash3_for([1, 2, 3], 42, 0xAA);
    let b = hash3_for([1, 2, 3], 42, 0xBB);
    assert_ne!(a, b);
}

#[test]
fn hash3_changes_with_pos() {
    let a = hash3_for([0, 0, 0], 42, 0xAA);
    let b = hash3_for([1, 0, 0], 42, 0xAA);
    assert_ne!(a, b);
}

// ----------------------------------------------------------------
// Q.4d: lava randomTick fire emission.
// MC reference: LavaFluid.java lines 78..=114.
// ----------------------------------------------------------------

#[test]
fn lava_with_no_flammable_nearby_does_not_ignite() {
    // Lava source over stone with no flammable in range -> no
    // ignite across many clocks.
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    w.set([0, 1, 0], BlockId(crate::block::LAVA_ID));
    for clock in 1..=100u64 {
        let mut step = FireStep::default();
        plan_lava_random_tick(&w, [0, 1, 0], clock, &mut step);
        assert!(step.ignite.is_empty(), "lava ignited with no flammable: clock {clock}");
    }
}

#[test]
fn lava_with_flammable_planks_ignites_within_n_ticks() {
    // Lava source + adjacent planks block at ground level. Expect
    // an ignite (either via the upward-drift branch or the
    // horizontal-sweep branch) within 100 clocks.
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    w.set([1, 0, 0], BlockId(STONE_ID));
    w.set([0, 1, 0], BlockId(crate::block::LAVA_ID));
    w.set([1, 1, 0], BlockId(PLANKS_ID));
    // Air at (1, 2, 0) is the "above planks" target for the
    // horizontal-sweep branch.
    let mut hit = false;
    for clock in 1..=100u64 {
        let mut step = FireStep::default();
        plan_lava_random_tick(&w, [0, 1, 0], clock, &mut step);
        if !step.ignite.is_empty() {
            hit = true;
            break;
        }
    }
    assert!(hit, "lava failed to ignite nearby planks across 100 ticks");
}

#[test]
fn lava_horizontal_sweep_lights_air_above_flammable() {
    // Force the passes==0 horizontal-sweep branch by picking a
    // clock value where hash3_for(pos, clock, 0x100) % 3 == 0.
    // Then check the ignite list contains an air cell ABOVE a
    // flammable block, not the flammable block itself.
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    w.set([1, 0, 0], BlockId(STONE_ID));
    w.set([-1, 0, 0], BlockId(STONE_ID));
    w.set([0, 0, 1], BlockId(STONE_ID));
    w.set([0, 0, -1], BlockId(STONE_ID));
    w.set([0, 1, 0], BlockId(crate::block::LAVA_ID));
    // Surround the lava with planks ALL around at the same Y so
    // any sweep direction hits something.
    w.set([1, 1, 0], BlockId(PLANKS_ID));
    w.set([-1, 1, 0], BlockId(PLANKS_ID));
    w.set([0, 1, 1], BlockId(PLANKS_ID));
    w.set([0, 1, -1], BlockId(PLANKS_ID));
    // Try many clocks; at least one should produce a horizontal-
    // sweep ignite.
    let mut sweep_ignite_hit = false;
    for clock in 1..=200u64 {
        let h_passes = hash3_for([0, 1, 0], clock, 0x100);
        if !h_passes.is_multiple_of(3) {
            continue;
        }
        let mut step = FireStep::default();
        plan_lava_random_tick(&w, [0, 1, 0], clock, &mut step);
        if step.ignite.is_empty() {
            continue;
        }
        // Every ignited cell should be AIR (the above-planks cell),
        // never the planks block itself.
        for (pos, _) in &step.ignite {
            assert!(
                w.get(*pos).is_air(),
                "horizontal-sweep ignited a non-air cell at {pos:?}"
            );
        }
        sweep_ignite_hit = true;
        break;
    }
    assert!(sweep_ignite_hit, "no horizontal-sweep ignite in 200 ticks (gate never fired?)");
}

#[test]
fn lava_upward_drift_ignites_air_above() {
    // Force the passes > 0 branch by finding a clock where the
    // hash satisfies it. Lava under a tall column of air with a
    // planks block at side -> the upward drift hits an air cell
    // with a flammable neighbour.
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    w.set([0, 1, 0], BlockId(crate::block::LAVA_ID));
    w.set([1, 2, 0], BlockId(PLANKS_ID));
    // Air at [0,2,0], [0,3,0], [-1,2,0] etc. so the drift can
    // land on a cell with the planks as a face neighbour.
    let mut hit = false;
    for clock in 1..=200u64 {
        let h_passes = hash3_for([0, 1, 0], clock, 0x100);
        if h_passes.is_multiple_of(3) {
            continue; // skip horizontal branch
        }
        let mut step = FireStep::default();
        plan_lava_random_tick(&w, [0, 1, 0], clock, &mut step);
        if !step.ignite.is_empty() {
            for (pos, _) in &step.ignite {
                assert!(
                    w.get(*pos).is_air(),
                    "upward-drift ignited a non-air cell at {pos:?}"
                );
            }
            hit = true;
            break;
        }
    }
    assert!(hit, "upward drift never ignited across 200 ticks");
}

#[test]
fn lava_horizontal_sweep_ignores_blocked_above_target() {
    // If the cell ABOVE the flammable is solid, the horizontal-
    // sweep does NOT ignite (MC's `level.isEmptyBlock(testPos.above())`
    // check). Build a layout where every flammable's above is
    // capped by stone.
    let mut w = world_with_chunks();
    w.set([0, 0, 0], BlockId(STONE_ID));
    w.set([0, 1, 0], BlockId(crate::block::LAVA_ID));
    w.set([1, 1, 0], BlockId(PLANKS_ID));
    w.set([1, 2, 0], BlockId(STONE_ID)); // cap
    w.set([-1, 1, 0], BlockId(PLANKS_ID));
    w.set([-1, 2, 0], BlockId(STONE_ID));
    w.set([0, 1, 1], BlockId(PLANKS_ID));
    w.set([0, 2, 1], BlockId(STONE_ID));
    w.set([0, 1, -1], BlockId(PLANKS_ID));
    w.set([0, 2, -1], BlockId(STONE_ID));
    for clock in 1..=100u64 {
        let h_passes = hash3_for([0, 1, 0], clock, 0x100);
        if !h_passes.is_multiple_of(3) {
            continue; // horizontal branch only
        }
        let mut step = FireStep::default();
        plan_lava_random_tick(&w, [0, 1, 0], clock, &mut step);
        // No ignite should be staged in this layout.
        assert!(
            step.ignite.is_empty(),
            "horizontal-sweep ignited despite capped flammables: {:?}",
            step.ignite
        );
    }
}
