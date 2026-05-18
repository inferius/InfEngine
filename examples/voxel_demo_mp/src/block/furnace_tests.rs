//! Unit tests for `block::furnace`: accept-filter wiring, place/
//! break round trip, drain-on-break, runtime defaults, and the
//! recover-held-on-close behaviour. Pulled out of `furnace.rs` so
//! the module body stays under the 500-LOC soft cap.

use std::collections::HashMap;

use super::{
    FURNACE_COOK_MS_PER_OUTPUT, FURNACE_COOK_TICKS_PER_OUTPUT, FURNACE_FUEL_SLOT,
    FURNACE_INPUT_SLOT, FURNACE_OUTPUT_SLOT, FURNACE_SLOTS, FurnaceRuntime, TICK_MS,
    accept_fuel, accept_nothing, accept_smeltable, break_furnace, drain_drops,
    furnace_tick, new_furnace, place_furnace, recover_held_on_close,
    step_one_furnace,
};
use crate::container::Container;

// ---- Accept-filter wiring ----

#[test]
fn new_furnace_has_three_slots() {
    let c = new_furnace();
    assert_eq!(c.len(), FURNACE_SLOTS);
    assert!(c.is_empty());
}

#[test]
fn input_slot_accepts_smeltables_only() {
    let c = new_furnace();
    assert!(c.accepts(FURNACE_INPUT_SLOT, 8)); // cobble (smeltable)
    assert!(c.accepts(FURNACE_INPUT_SLOT, 14)); // iron ore
    assert!(!c.accepts(FURNACE_INPUT_SLOT, 1)); // stone (no recipe)
    assert!(!c.accepts(FURNACE_INPUT_SLOT, 0)); // air
}

#[test]
fn fuel_slot_accepts_fuels_only() {
    let c = new_furnace();
    assert!(c.accepts(FURNACE_FUEL_SLOT, 13)); // coal
    assert!(c.accepts(FURNACE_FUEL_SLOT, 7)); // planks
    assert!(c.accepts(FURNACE_FUEL_SLOT, 5)); // wood
    assert!(!c.accepts(FURNACE_FUEL_SLOT, 1)); // stone
    assert!(!c.accepts(FURNACE_FUEL_SLOT, 14)); // iron ore
}

#[test]
fn output_slot_rejects_every_id() {
    let c = new_furnace();
    for id in 0u16..=64 {
        assert!(
            !c.accepts(FURNACE_OUTPUT_SLOT, id),
            "output slot must reject id {id}",
        );
    }
}

#[test]
fn output_slot_blocks_player_insert() {
    // The accept filter on the output slot rejects player-driven
    // inserts via insert_at: full count comes back as leftover and
    // the slot stays empty.
    let mut c = new_furnace();
    let leftover = c.insert_at(FURNACE_OUTPUT_SLOT, (1, 4));
    assert_eq!(leftover, 4);
    assert_eq!(c.slot(FURNACE_OUTPUT_SLOT), None);
}

#[test]
fn set_slot_bypasses_filter_for_cook_path() {
    // set_slot is the raw setter (docs call it admin-path) so the
    // furnace cook step can write the output without colliding with
    // accept_nothing.
    let mut c = new_furnace();
    c.set_slot(FURNACE_OUTPUT_SLOT, Some((43, 1)));
    assert_eq!(c.slot(FURNACE_OUTPUT_SLOT), Some((43, 1)));
}

#[test]
fn accept_predicates_match_smelting_module() {
    // Sanity: the predicates exposed here are the exact gates the
    // brief specifies. Belt-and-suspenders test.
    assert!(accept_smeltable(8));
    assert!(!accept_smeltable(1));
    assert!(accept_fuel(13));
    assert!(!accept_fuel(1));
    assert!(!accept_nothing(0));
    assert!(!accept_nothing(43));
}

// ---- place_furnace / break_furnace ----

#[test]
fn place_furnace_creates_empty_entry() {
    let mut furnaces: HashMap<[i32; 3], Container> = HashMap::new();
    let mut runtimes: HashMap<[i32; 3], FurnaceRuntime> = HashMap::new();
    place_furnace(&mut furnaces, &mut runtimes, [3, 64, 7]);
    assert_eq!(furnaces.len(), 1);
    let f = furnaces.get(&[3, 64, 7]).expect("furnace must exist");
    assert_eq!(f.len(), FURNACE_SLOTS);
    assert!(f.is_empty());
    // Runtime defaults to all-zero.
    let rt = runtimes.get(&[3, 64, 7]).expect("runtime must exist");
    assert_eq!(*rt, FurnaceRuntime::default());
}

#[test]
fn place_furnace_idempotent_keeps_existing() {
    // Two place calls at the same pos must not stomp the first
    // furnace's contents - mirrors chest::new_chest.
    let mut furnaces: HashMap<[i32; 3], Container> = HashMap::new();
    let mut runtimes: HashMap<[i32; 3], FurnaceRuntime> = HashMap::new();
    place_furnace(&mut furnaces, &mut runtimes, [0, 0, 0]);
    // Plant some content.
    furnaces.get_mut(&[0, 0, 0]).unwrap().set_slot(0, Some((14, 5)));
    runtimes.insert(
        [0, 0, 0],
        FurnaceRuntime {
            fuel_left_ms: 1000,
            fuel_full_ms: 2000,
            cook_progress_ms: 500,
        },
    );
    place_furnace(&mut furnaces, &mut runtimes, [0, 0, 0]);
    assert_eq!(
        furnaces.get(&[0, 0, 0]).unwrap().slot(0),
        Some((14, 5)),
        "second place must not wipe existing content",
    );
    let rt = runtimes.get(&[0, 0, 0]).unwrap();
    assert_eq!(rt.fuel_left_ms, 1000);
    assert_eq!(rt.cook_progress_ms, 500);
}

#[test]
fn break_furnace_returns_drops_and_removes_entry() {
    let mut furnaces: HashMap<[i32; 3], Container> = HashMap::new();
    let mut runtimes: HashMap<[i32; 3], FurnaceRuntime> = HashMap::new();
    place_furnace(&mut furnaces, &mut runtimes, [1, 2, 3]);
    {
        let f = furnaces.get_mut(&[1, 2, 3]).unwrap();
        f.set_slot(0, Some((14, 3))); // 3 iron ore
        f.set_slot(1, Some((13, 2))); // 2 coal
        f.set_slot(2, Some((43, 1))); // 1 iron ingot in output
    }
    let drops = break_furnace(&mut furnaces, &mut runtimes, [1, 2, 3]);
    assert_eq!(drops.len(), 6);
    assert_eq!(drops.iter().filter(|d| **d == 14).count(), 3);
    assert_eq!(drops.iter().filter(|d| **d == 13).count(), 2);
    assert_eq!(drops.iter().filter(|d| **d == 43).count(), 1);
    assert!(!furnaces.contains_key(&[1, 2, 3]));
    assert!(!runtimes.contains_key(&[1, 2, 3]));
}

#[test]
fn break_furnace_unknown_pos_is_empty_drops() {
    let mut furnaces: HashMap<[i32; 3], Container> = HashMap::new();
    let mut runtimes: HashMap<[i32; 3], FurnaceRuntime> = HashMap::new();
    let drops = break_furnace(&mut furnaces, &mut runtimes, [42, 0, 42]);
    assert!(drops.is_empty());
}

// ---- drain_drops / runtime constants ----

#[test]
fn drain_drops_empty_is_empty() {
    let c = new_furnace();
    assert!(drain_drops(&c).is_empty());
}

#[test]
fn drain_drops_expands_each_stack_to_per_item_drops() {
    let mut c = new_furnace();
    c.set_slot(0, Some((14, 4))); // iron ore (smeltable - filter passes)
    c.set_slot(1, Some((13, 2))); // coal (fuel - filter passes)
    // Output set via raw setter (bypasses filter).
    c.set_slot(2, Some((43, 3)));
    let drops = drain_drops(&c);
    assert_eq!(drops.len(), 9);
    assert_eq!(drops.iter().filter(|d| **d == 14).count(), 4);
    assert_eq!(drops.iter().filter(|d| **d == 13).count(), 2);
    assert_eq!(drops.iter().filter(|d| **d == 43).count(), 3);
}

#[test]
fn furnace_runtime_default_is_all_zero() {
    let rt = FurnaceRuntime::default();
    assert_eq!(rt.fuel_left_ms, 0);
    assert_eq!(rt.fuel_full_ms, 0);
    assert_eq!(rt.cook_progress_ms, 0);
}

#[test]
fn cook_ms_matches_ticks() {
    // 200 ticks * 50 ms/tick = 10 000 ms. Pinned so a future
    // tweak to either constant has to come through both.
    assert_eq!(
        FURNACE_COOK_MS_PER_OUTPUT,
        FURNACE_COOK_TICKS_PER_OUTPUT * TICK_MS,
    );
    assert_eq!(FURNACE_COOK_MS_PER_OUTPUT, 10_000);
}

// ---- recover_held_on_close ----

#[test]
fn recover_held_no_stack_no_drops() {
    let mut c = new_furnace();
    let drops = recover_held_on_close(Some(&mut c), None);
    assert!(drops.is_empty());
}

#[test]
fn recover_held_smeltable_lands_in_input_slot() {
    // Iron ore is smeltable, so the recover path can drop it into
    // the input slot.
    let mut c = new_furnace();
    let drops = recover_held_on_close(Some(&mut c), Some((14, 5)));
    assert!(drops.is_empty());
    assert_eq!(c.slot(FURNACE_INPUT_SLOT), Some((14, 5)));
}

#[test]
fn recover_held_fuel_lands_in_fuel_slot() {
    let mut c = new_furnace();
    let drops = recover_held_on_close(Some(&mut c), Some((13, 8)));
    assert!(drops.is_empty());
    assert_eq!(c.slot(FURNACE_FUEL_SLOT), Some((13, 8)));
}

#[test]
fn recover_held_non_matching_item_falls_as_drops() {
    // Stone is neither smeltable nor a fuel; the output slot
    // rejects it too. The recover path can't find a home -> every
    // item lands on the floor.
    let mut c = new_furnace();
    let drops = recover_held_on_close(Some(&mut c), Some((1, 3)));
    assert_eq!(drops, vec![1, 1, 1]);
    assert!(c.is_empty());
}

#[test]
fn recover_held_no_furnace_dumps_everything() {
    let drops = recover_held_on_close(None, Some((14, 4)));
    assert_eq!(drops, vec![14, 14, 14, 14]);
}

// ---- furnace_tick cook step ----

/// Build a furnace pre-loaded with `inputs` in slot 0 and `fuel`
/// in slot 1; output slot left empty. Returns the container + a
/// zeroed runtime.
fn loaded_furnace(
    input: Option<(u16, u8)>,
    fuel: Option<(u16, u8)>,
) -> (Container, FurnaceRuntime) {
    let mut f = new_furnace();
    if let Some((id, n)) = input {
        f.set_slot(FURNACE_INPUT_SLOT, Some((id, n)));
    }
    if let Some((id, n)) = fuel {
        f.set_slot(FURNACE_FUEL_SLOT, Some((id, n)));
    }
    (f, FurnaceRuntime::default())
}

#[test]
fn tick_advances_fuel_and_cook_in_lockstep() {
    // Fresh light: input + fuel present, output empty.
    let (mut f, mut rt) = loaded_furnace(Some((14, 1)), Some((13, 1)));
    step_one_furnace(&mut f, &mut rt, 100);
    // Fuel was lit at start -> fuel_full = coal burn (1600 * 50 = 80000).
    assert_eq!(rt.fuel_full_ms, 1600 * TICK_MS);
    // 100 ms consumed from fuel + same into cook progress.
    assert_eq!(rt.fuel_left_ms, rt.fuel_full_ms - 100);
    assert_eq!(rt.cook_progress_ms, 100);
    // Fuel slot decremented from 1 -> empty (one item consumed).
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), None);
    // Input untouched.
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some((14, 1)));
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), None);
}

#[test]
fn tick_emits_output_at_cook_threshold() {
    // 1 iron ore + lots of coal. Drive 10 000 ms (one cook).
    let (mut f, mut rt) = loaded_furnace(Some((14, 1)), Some((13, 8)));
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT);
    // Output is 1 iron ingot.
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some((43, 1)));
    // Input emptied.
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), None);
    // Cook progress wraps to 0 (no overflow at exact threshold).
    assert_eq!(rt.cook_progress_ms, 0);
}

#[test]
fn tick_preserves_cook_overflow() {
    // Drive 10 050 ms - one full cook + 50 ms overflow on the
    // next item. With only 1 iron ore the second item never
    // exists, so cook progress lands at 0 (the inner loop bails
    // when input is empty).
    let (mut f, mut rt) = loaded_furnace(Some((14, 2)), Some((13, 8)));
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT + 50);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some((43, 1)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some((14, 1)));
    // 50 ms preserved.
    assert_eq!(rt.cook_progress_ms, 50);
}

#[test]
fn tick_stalls_when_input_is_empty() {
    // No input - tick does nothing useful; fuel still burns off
    // idle so the bar drains.
    let (mut f, mut rt) = loaded_furnace(None, Some((13, 1)));
    step_one_furnace(&mut f, &mut rt, 100);
    assert_eq!(rt.fuel_left_ms, 0); // never lit
    assert_eq!(rt.cook_progress_ms, 0);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some((13, 1)));
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), None);
}

#[test]
fn tick_stalls_when_no_recipe() {
    // Slot 0 has stone (no recipe). Furnace stays idle.
    let (mut f, mut rt) = loaded_furnace(Some((1, 1)), Some((13, 1)));
    step_one_furnace(&mut f, &mut rt, 100);
    assert_eq!(rt.cook_progress_ms, 0);
    assert_eq!(rt.fuel_left_ms, 0);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some((13, 1)));
}

#[test]
fn tick_stalls_when_fuel_runs_out_midway() {
    // Pre-loaded runtime: 100 ms of fuel left, no spare fuel in
    // the slot. After 200 ms the fuel drains in 100 ms; the
    // remaining 100 ms of the step is wasted because no fresh
    // fuel exists. Cook progress accumulates only while fuel
    // burns.
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, Some((14, 1)));
    // Fuel slot empty.
    let mut rt = FurnaceRuntime {
        fuel_left_ms: 100,
        fuel_full_ms: 1000,
        cook_progress_ms: 0,
    };
    step_one_furnace(&mut f, &mut rt, 200);
    // Cook progress only grew by 100 ms (the fuel that was left).
    assert_eq!(rt.cook_progress_ms, 100);
    assert_eq!(rt.fuel_left_ms, 0);
    // No fresh fuel -> idle path cleared `fuel_full_ms`.
    assert_eq!(rt.fuel_full_ms, 0);
}

#[test]
fn tick_stalls_when_output_full_of_different_item() {
    // Output slot is occupied by a different item -> cook can't
    // emit, so fuel doesn't ignite either (idle branch).
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, Some((14, 1)));
    f.set_slot(FURNACE_FUEL_SLOT, Some((13, 1)));
    f.set_slot(FURNACE_OUTPUT_SLOT, Some((1, 1))); // stone, mismatch
    let mut rt = FurnaceRuntime::default();
    step_one_furnace(&mut f, &mut rt, 200);
    assert_eq!(rt.cook_progress_ms, 0);
    assert_eq!(rt.fuel_left_ms, 0);
    // Fuel still in the slot.
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some((13, 1)));
}

#[test]
fn tick_stalls_when_output_full_of_same_item() {
    // Output slot full of the right item -> can't fit more.
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, Some((14, 1)));
    f.set_slot(FURNACE_FUEL_SLOT, Some((13, 1)));
    f.set_slot(FURNACE_OUTPUT_SLOT, Some((43, 64))); // ingot stack full
    let mut rt = FurnaceRuntime::default();
    step_one_furnace(&mut f, &mut rt, 1000);
    assert_eq!(rt.cook_progress_ms, 0);
    assert_eq!(rt.fuel_left_ms, 0);
}

#[test]
fn tick_merges_output_into_existing_same_id_stack() {
    // Pre-load 5 ingots in the output slot. One cook adds one more.
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, Some((14, 1)));
    f.set_slot(FURNACE_FUEL_SLOT, Some((13, 1)));
    f.set_slot(FURNACE_OUTPUT_SLOT, Some((43, 5)));
    let mut rt = FurnaceRuntime::default();
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some((43, 6)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), None);
}

#[test]
fn tick_consumes_fuel_one_at_a_time() {
    // Two coal in the slot. First cook uses fuel #1 immediately
    // (1600 * 50 = 80 000 ms). 10 000 ms cook -> output emerges,
    // fuel #1 has 70 000 ms left, fuel slot still shows 1 left.
    let (mut f, mut rt) = loaded_furnace(Some((14, 2)), Some((13, 2)));
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some((43, 1)));
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some((13, 1)));
    // Fuel level: 80 000 - 10 000 = 70 000.
    assert_eq!(rt.fuel_left_ms, 80_000 - FURNACE_COOK_MS_PER_OUTPUT);
}

#[test]
fn tick_zero_dt_is_noop() {
    let (mut f, mut rt) = loaded_furnace(Some((14, 1)), Some((13, 1)));
    step_one_furnace(&mut f, &mut rt, 0);
    // Fuel was NOT lit (zero dt -> early return inside the
    // dispatcher; the single-furnace stepper still runs but the
    // burn loop reads 0 ms).
    // (See furnace_tick wrapper for the global early return.)
    assert_eq!(rt.fuel_left_ms, rt.fuel_full_ms);
    // For zero dt, cook progress stays at 0.
    assert_eq!(rt.cook_progress_ms, 0);
}

#[test]
fn furnace_tick_zero_dt_early_return() {
    let mut f: std::collections::HashMap<[i32; 3], Container> = std::collections::HashMap::new();
    let mut r: std::collections::HashMap<[i32; 3], FurnaceRuntime> = std::collections::HashMap::new();
    place_furnace(&mut f, &mut r, [0, 0, 0]);
    f.get_mut(&[0, 0, 0]).unwrap().set_slot(0, Some((14, 1)));
    f.get_mut(&[0, 0, 0]).unwrap().set_slot(1, Some((13, 1)));
    furnace_tick(&mut f, &mut r, 0);
    let rt = r.get(&[0, 0, 0]).unwrap();
    assert_eq!(rt.fuel_left_ms, 0);
    assert_eq!(rt.cook_progress_ms, 0);
}

#[test]
fn furnace_tick_advances_multiple_furnaces() {
    let mut f: std::collections::HashMap<[i32; 3], Container> = std::collections::HashMap::new();
    let mut r: std::collections::HashMap<[i32; 3], FurnaceRuntime> = std::collections::HashMap::new();
    place_furnace(&mut f, &mut r, [0, 0, 0]);
    place_furnace(&mut f, &mut r, [1, 0, 0]);
    for pos in [[0i32, 0, 0], [1, 0, 0]] {
        let furnace = f.get_mut(&pos).unwrap();
        furnace.set_slot(0, Some((14, 1)));
        furnace.set_slot(1, Some((13, 1)));
    }
    furnace_tick(&mut f, &mut r, 100);
    for pos in [[0i32, 0, 0], [1, 0, 0]] {
        assert_eq!(r.get(&pos).unwrap().cook_progress_ms, 100);
    }
}

#[test]
fn full_cook_roundtrip_iron_ore_to_ingot() {
    // Integration-flavoured single-furnace test: place + load
    // + drive a full 10 s cook + assert the ingot lands in the
    // output slot.
    let (mut f, mut rt) = loaded_furnace(Some((14, 3)), Some((13, 1)));
    // Drive multiple sub-second ticks so the simulation looks
    // like the live runtime.
    let mut elapsed: u32 = 0;
    while elapsed < FURNACE_COOK_MS_PER_OUTPUT {
        step_one_furnace(&mut f, &mut rt, 100);
        elapsed += 100;
    }
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some((43, 1)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some((14, 2)));
}

#[test]
fn idle_fuel_drains_so_bar_eventually_empties() {
    // No input but fuel is mid-burn (e.g. player pulled the input
    // out). Tick should drain `fuel_left_ms` toward zero and clear
    // `fuel_full_ms` once the burn finishes.
    let mut f = new_furnace();
    f.set_slot(FURNACE_FUEL_SLOT, Some((13, 0))); // placeholder; not used
    f.set_slot(FURNACE_FUEL_SLOT, None);
    let mut rt = FurnaceRuntime {
        fuel_left_ms: 500,
        fuel_full_ms: 1000,
        cook_progress_ms: 0,
    };
    // 200 ms drain.
    step_one_furnace(&mut f, &mut rt, 200);
    assert_eq!(rt.fuel_left_ms, 300);
    assert_eq!(rt.fuel_full_ms, 1000);
    // Finish the burn.
    step_one_furnace(&mut f, &mut rt, 400);
    assert_eq!(rt.fuel_left_ms, 0);
    // Burn finished -> bar cleared.
    assert_eq!(rt.fuel_full_ms, 0);
}
