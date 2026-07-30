//! Unit tests for `block::furnace` core surface: accept-filter
//! wiring, place/break round trip, drain-on-break, runtime
//! defaults, and recover-held-on-close behaviour. The cook step +
//! catch-up + GridAction tests live in `furnace_tick_tests.rs` to
//! keep both files under the 500-LOC soft cap.

use std::collections::HashMap;

use super::{
    FURNACE_COOK_MS_PER_OUTPUT, FURNACE_COOK_TICKS_PER_OUTPUT, FURNACE_FUEL_SLOT,
    FURNACE_INPUT_SLOT, FURNACE_OUTPUT_SLOT, FURNACE_SLOTS, FurnaceRuntime, TICK_MS,
    accept_fuel, accept_nothing, accept_smeltable, break_furnace, drain_drops,
    new_furnace, place_furnace, recover_held_on_close,
};
use crate::container::Container;
use crate::item::ItemStack;

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
    let mut c = new_furnace();
    let leftover = c.insert_at(FURNACE_OUTPUT_SLOT, ItemStack::new(1, 4));
    assert_eq!(leftover, 4);
    assert_eq!(c.slot(FURNACE_OUTPUT_SLOT), None);
}

#[test]
fn set_slot_bypasses_filter_for_cook_path() {
    let mut c = new_furnace();
    c.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(43, 1)));
    assert_eq!(c.slot(FURNACE_OUTPUT_SLOT), Some(ItemStack::new(43, 1)));
}

#[test]
fn accept_predicates_match_smelting_module() {
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
    let rt = runtimes.get(&[3, 64, 7]).expect("runtime must exist");
    assert_eq!(*rt, FurnaceRuntime::default());
}

#[test]
fn place_furnace_idempotent_keeps_existing() {
    let mut furnaces: HashMap<[i32; 3], Container> = HashMap::new();
    let mut runtimes: HashMap<[i32; 3], FurnaceRuntime> = HashMap::new();
    place_furnace(&mut furnaces, &mut runtimes, [0, 0, 0]);
    furnaces.get_mut(&[0, 0, 0]).unwrap().set_slot(0, Some(ItemStack::new(14, 5)));
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
        Some(ItemStack::new(14, 5)),
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
        f.set_slot(0, Some(ItemStack::new(14, 3)));
        f.set_slot(1, Some(ItemStack::new(13, 2)));
        f.set_slot(2, Some(ItemStack::new(43, 1)));
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
    c.set_slot(0, Some(ItemStack::new(14, 4)));
    c.set_slot(1, Some(ItemStack::new(13, 2)));
    c.set_slot(2, Some(ItemStack::new(43, 3)));
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
    let mut c = new_furnace();
    let drops = recover_held_on_close(Some(&mut c), Some(ItemStack::new(14, 5)));
    assert!(drops.is_empty());
    assert_eq!(c.slot(FURNACE_INPUT_SLOT), Some(ItemStack::new(14, 5)));
}

#[test]
fn recover_held_fuel_lands_in_fuel_slot() {
    let mut c = new_furnace();
    let drops = recover_held_on_close(Some(&mut c), Some(ItemStack::new(13, 8)));
    assert!(drops.is_empty());
    assert_eq!(c.slot(FURNACE_FUEL_SLOT), Some(ItemStack::new(13, 8)));
}

#[test]
fn recover_held_non_matching_item_falls_as_drops() {
    let mut c = new_furnace();
    let drops = recover_held_on_close(Some(&mut c), Some(ItemStack::new(1, 3)));
    assert_eq!(drops, vec![1, 1, 1]);
    assert!(c.is_empty());
}

#[test]
fn recover_held_no_furnace_dumps_everything() {
    let drops = recover_held_on_close(None, Some(ItemStack::new(14, 4)));
    assert_eq!(drops, vec![14, 14, 14, 14]);
}
