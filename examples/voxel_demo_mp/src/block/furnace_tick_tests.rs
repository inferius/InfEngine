//! Unit tests for the furnace cook step + catch-up adapter +
//! GridAction handlers. Split out of `furnace_tests.rs` so each
//! file stays under the 500-LOC soft cap.
//!
//! Covers:
//! - `step_one_furnace` cook progression + stall conditions.
//! - `furnace_tick` multi-furnace dispatch + zero-dt guard.
//! - `apply_catch_up_output` offline -> live bridge.
//! - `apply_furnace_action` / `apply_player_action` GridAction
//!   plumbing through the accept filters.

use super::{
    FURNACE_COOK_MS_PER_OUTPUT, FURNACE_FUEL_SLOT, FURNACE_INPUT_SLOT,
    FURNACE_OUTPUT_SLOT, FurnaceRuntime, TICK_MS, apply_catch_up_output,
    apply_furnace_action, apply_player_action, furnace_tick, new_furnace,
    place_furnace, step_one_furnace,
};
use crate::container::Container;
use crate::container::ui::GridAction;
use crate::item::ItemStack;

/// Build a furnace pre-loaded with `inputs` in slot 0 and `fuel`
/// in slot 1; output slot left empty. Returns the container + a
/// zeroed runtime.
fn loaded_furnace(
    input: Option<ItemStack>,
    fuel: Option<ItemStack>,
) -> (Container, FurnaceRuntime) {
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, input);
    f.set_slot(FURNACE_FUEL_SLOT, fuel);
    (f, FurnaceRuntime::default())
}

// ---- furnace_tick cook step ----

#[test]
fn tick_advances_fuel_and_cook_in_lockstep() {
    let (mut f, mut rt) = loaded_furnace(Some(ItemStack::new(14, 1)), Some(ItemStack::new(325, 1)));
    step_one_furnace(&mut f, &mut rt, 100);
    assert_eq!(rt.fuel_full_ms, 1600 * TICK_MS);
    assert_eq!(rt.fuel_left_ms, rt.fuel_full_ms - 100);
    assert_eq!(rt.cook_progress_ms, 100);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), None);
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some(ItemStack::new(14, 1)));
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), None);
}

#[test]
fn tick_emits_output_at_cook_threshold() {
    let (mut f, mut rt) = loaded_furnace(Some(ItemStack::new(14, 1)), Some(ItemStack::new(325, 8)));
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some(ItemStack::new(43, 1)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), None);
    assert_eq!(rt.cook_progress_ms, 0);
}

#[test]
fn tick_preserves_cook_overflow() {
    let (mut f, mut rt) = loaded_furnace(Some(ItemStack::new(14, 2)), Some(ItemStack::new(325, 8)));
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT + 50);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some(ItemStack::new(43, 1)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some(ItemStack::new(14, 1)));
    assert_eq!(rt.cook_progress_ms, 50);
}

#[test]
fn tick_stalls_when_input_is_empty() {
    let (mut f, mut rt) = loaded_furnace(None, Some(ItemStack::new(325, 1)));
    step_one_furnace(&mut f, &mut rt, 100);
    assert_eq!(rt.fuel_left_ms, 0);
    assert_eq!(rt.cook_progress_ms, 0);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some(ItemStack::new(325, 1)));
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), None);
}

#[test]
fn tick_stalls_when_no_recipe() {
    // W476: dirt (2) is the canonical junk input now - stone gained
    // the MC stone -> smooth_stone recipe. The accept filter would
    // block dirt from ever landing in slot 0 via the UI; set_slot
    // bypasses it, which is exactly the defensive path under test.
    let (mut f, mut rt) = loaded_furnace(Some(ItemStack::new(2, 1)), Some(ItemStack::new(325, 1)));
    step_one_furnace(&mut f, &mut rt, 100);
    assert_eq!(rt.cook_progress_ms, 0);
    assert_eq!(rt.fuel_left_ms, 0);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some(ItemStack::new(325, 1)));
}

#[test]
fn lava_bucket_burn_leaves_empty_bucket_in_fuel_slot() {
    // W476: MC crafting-remainder rule. Lighting the lava bucket
    // (fuel 20000 ticks) must swap the emptied fuel slot to the
    // EMPTY bucket, not clear it.
    use crate::block::{BUCKET_EMPTY_ID, BUCKET_LAVA_ID};
    let (mut f, mut rt) = loaded_furnace(
        Some(ItemStack::new(14, 1)),
        Some(ItemStack::new(BUCKET_LAVA_ID, 1)),
    );
    step_one_furnace(&mut f, &mut rt, 100);
    assert_eq!(rt.fuel_full_ms, 20000 * TICK_MS);
    assert_eq!(rt.fuel_left_ms, rt.fuel_full_ms - 100);
    assert_eq!(
        f.slot(FURNACE_FUEL_SLOT),
        Some(ItemStack::new(BUCKET_EMPTY_ID, 1)),
        "empty bucket stays behind as the crafting remainder"
    );
    // The bucket is NOT fuel - the furnace must idle on it once the
    // lava burn ends instead of consuming it.
    let mut rt2 = super::FurnaceRuntime::default();
    step_one_furnace(&mut f, &mut rt2, 100);
    assert_eq!(rt2.fuel_full_ms, 0, "empty bucket never lights");
    assert_eq!(
        f.slot(FURNACE_FUEL_SLOT),
        Some(ItemStack::new(BUCKET_EMPTY_ID, 1)),
        "bucket survives the failed relight"
    );
}

#[test]
fn coal_burn_leaves_no_remainder() {
    // W476: only the lava bucket has a crafting remainder - coal
    // still clears the slot like before.
    let (mut f, mut rt) = loaded_furnace(Some(ItemStack::new(14, 1)), Some(ItemStack::new(325, 1)));
    step_one_furnace(&mut f, &mut rt, 100);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), None);
}

#[test]
fn tick_stalls_when_fuel_runs_out_midway() {
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, Some(ItemStack::new(14, 1)));
    let mut rt = FurnaceRuntime {
        fuel_left_ms: 100,
        fuel_full_ms: 1000,
        cook_progress_ms: 0,
    };
    step_one_furnace(&mut f, &mut rt, 200);
    assert_eq!(rt.cook_progress_ms, 100);
    assert_eq!(rt.fuel_left_ms, 0);
    assert_eq!(rt.fuel_full_ms, 0);
}

#[test]
fn tick_stalls_when_output_full_of_different_item() {
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, Some(ItemStack::new(14, 1)));
    f.set_slot(FURNACE_FUEL_SLOT, Some(ItemStack::new(325, 1)));
    f.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(1, 1)));
    let mut rt = FurnaceRuntime::default();
    step_one_furnace(&mut f, &mut rt, 200);
    assert_eq!(rt.cook_progress_ms, 0);
    assert_eq!(rt.fuel_left_ms, 0);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some(ItemStack::new(325, 1)));
}

#[test]
fn tick_stalls_when_output_full_of_same_item() {
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, Some(ItemStack::new(14, 1)));
    f.set_slot(FURNACE_FUEL_SLOT, Some(ItemStack::new(325, 1)));
    f.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(43, 64)));
    let mut rt = FurnaceRuntime::default();
    step_one_furnace(&mut f, &mut rt, 1000);
    assert_eq!(rt.cook_progress_ms, 0);
    assert_eq!(rt.fuel_left_ms, 0);
}

#[test]
fn tick_merges_output_into_existing_same_id_stack() {
    let mut f = new_furnace();
    f.set_slot(FURNACE_INPUT_SLOT, Some(ItemStack::new(14, 1)));
    f.set_slot(FURNACE_FUEL_SLOT, Some(ItemStack::new(325, 1)));
    f.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(43, 5)));
    let mut rt = FurnaceRuntime::default();
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some(ItemStack::new(43, 6)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), None);
}

#[test]
fn tick_consumes_fuel_one_at_a_time() {
    let (mut f, mut rt) = loaded_furnace(Some(ItemStack::new(14, 2)), Some(ItemStack::new(325, 2)));
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some(ItemStack::new(43, 1)));
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some(ItemStack::new(325, 1)));
    assert_eq!(rt.fuel_left_ms, 80_000 - FURNACE_COOK_MS_PER_OUTPUT);
}

#[test]
fn tick_zero_dt_is_noop() {
    let (mut f, mut rt) = loaded_furnace(Some(ItemStack::new(14, 1)), Some(ItemStack::new(325, 1)));
    step_one_furnace(&mut f, &mut rt, 0);
    assert_eq!(rt.fuel_left_ms, rt.fuel_full_ms);
    assert_eq!(rt.cook_progress_ms, 0);
}

#[test]
fn furnace_tick_zero_dt_early_return() {
    let mut f: std::collections::HashMap<[i32; 3], Container> = std::collections::HashMap::new();
    let mut r: std::collections::HashMap<[i32; 3], FurnaceRuntime> = std::collections::HashMap::new();
    place_furnace(&mut f, &mut r, [0, 0, 0]);
    f.get_mut(&[0, 0, 0]).unwrap().set_slot(0, Some(ItemStack::new(14, 1)));
    f.get_mut(&[0, 0, 0]).unwrap().set_slot(1, Some(ItemStack::new(325, 1)));
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
        furnace.set_slot(0, Some(ItemStack::new(14, 1)));
        furnace.set_slot(1, Some(ItemStack::new(325, 1)));
    }
    furnace_tick(&mut f, &mut r, 100);
    for pos in [[0i32, 0, 0], [1, 0, 0]] {
        assert_eq!(r.get(&pos).unwrap().cook_progress_ms, 100);
    }
}

#[test]
fn full_cook_roundtrip_iron_ore_to_ingot() {
    let (mut f, mut rt) = loaded_furnace(Some(ItemStack::new(14, 3)), Some(ItemStack::new(325, 1)));
    let mut elapsed: u32 = 0;
    while elapsed < FURNACE_COOK_MS_PER_OUTPUT {
        step_one_furnace(&mut f, &mut rt, 100);
        elapsed += 100;
    }
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some(ItemStack::new(43, 1)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some(ItemStack::new(14, 2)));
}

#[test]
fn idle_fuel_drains_so_bar_eventually_empties() {
    let mut f = new_furnace();
    let mut rt = FurnaceRuntime {
        fuel_left_ms: 500,
        fuel_full_ms: 1000,
        cook_progress_ms: 0,
    };
    step_one_furnace(&mut f, &mut rt, 200);
    assert_eq!(rt.fuel_left_ms, 300);
    assert_eq!(rt.fuel_full_ms, 1000);
    step_one_furnace(&mut f, &mut rt, 400);
    assert_eq!(rt.fuel_left_ms, 0);
    assert_eq!(rt.fuel_full_ms, 0);
}

// ---- apply_catch_up_output (offline -> live bridge) ----

#[test]
fn catch_up_output_lands_in_empty_slot() {
    let mut furnaces: std::collections::HashMap<[i32; 3], Container> =
        std::collections::HashMap::new();
    furnaces.insert([1, 1, 1], new_furnace());
    let landed = apply_catch_up_output(&mut furnaces, [1, 1, 1], 43, 3);
    assert!(landed);
    assert_eq!(
        furnaces.get(&[1, 1, 1]).unwrap().slot(FURNACE_OUTPUT_SLOT),
        Some(ItemStack::new(43, 3)),
    );
}

#[test]
fn catch_up_output_merges_into_same_id_stack() {
    let mut furnaces: std::collections::HashMap<[i32; 3], Container> =
        std::collections::HashMap::new();
    let mut f = new_furnace();
    f.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(43, 4)));
    furnaces.insert([0, 0, 0], f);
    let landed = apply_catch_up_output(&mut furnaces, [0, 0, 0], 43, 5);
    assert!(landed);
    assert_eq!(
        furnaces.get(&[0, 0, 0]).unwrap().slot(FURNACE_OUTPUT_SLOT),
        Some(ItemStack::new(43, 9)),
    );
}

#[test]
fn catch_up_output_missing_furnace_is_no_op() {
    let mut furnaces: std::collections::HashMap<[i32; 3], Container> =
        std::collections::HashMap::new();
    let landed = apply_catch_up_output(&mut furnaces, [9, 9, 9], 43, 1);
    assert!(!landed);
}

#[test]
fn catch_up_output_mismatched_id_is_dropped() {
    let mut furnaces: std::collections::HashMap<[i32; 3], Container> =
        std::collections::HashMap::new();
    let mut f = new_furnace();
    f.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(1, 1)));
    furnaces.insert([0, 0, 0], f);
    let landed = apply_catch_up_output(&mut furnaces, [0, 0, 0], 43, 5);
    assert!(!landed);
    assert_eq!(
        furnaces.get(&[0, 0, 0]).unwrap().slot(FURNACE_OUTPUT_SLOT),
        Some(ItemStack::new(1, 1)),
    );
}

// ---- GridAction apply helpers ----

#[test]
fn action_place_into_input_accepts_smeltable() {
    let mut f = new_furnace();
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    let mut held: Option<ItemStack> = Some(ItemStack::new(14, 3));
    apply_furnace_action(&mut f, &mut inv, &mut held, GridAction::Place(FURNACE_INPUT_SLOT), false);
    assert_eq!(held, None);
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some(ItemStack::new(14, 3)));
}

#[test]
fn action_place_into_input_rejects_non_smeltable() {
    let mut f = new_furnace();
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    // W476: dirt (2) is the canonical non-smeltable probe (stone
    // gained the MC stone -> smooth_stone recipe).
    let mut held: Option<ItemStack> = Some(ItemStack::new(2, 3));
    apply_furnace_action(&mut f, &mut inv, &mut held, GridAction::Place(FURNACE_INPUT_SLOT), false);
    assert_eq!(held, Some(ItemStack::new(2, 3)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), None);
}

#[test]
fn action_place_into_fuel_accepts_coal_only() {
    let mut f = new_furnace();
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    let mut held: Option<ItemStack> = Some(ItemStack::new(325, 5));
    apply_furnace_action(&mut f, &mut inv, &mut held, GridAction::Place(FURNACE_FUEL_SLOT), false);
    assert_eq!(held, None);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some(ItemStack::new(325, 5)));
}

#[test]
fn action_place_into_fuel_rejects_the_coal_ore_block() {
    // W477: the id-13 legacy fuel fallback is gone - the ore BLOCK
    // is not a fuel, so the accept filter bounces it.
    let mut f = new_furnace();
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    let mut held: Option<ItemStack> = Some(ItemStack::new(13, 5));
    apply_furnace_action(&mut f, &mut inv, &mut held, GridAction::Place(FURNACE_FUEL_SLOT), false);
    assert_eq!(held, Some(ItemStack::new(13, 5)));
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), None);
}

#[test]
fn action_place_into_output_rejects_everything() {
    let mut f = new_furnace();
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    let mut held: Option<ItemStack> = Some(ItemStack::new(43, 2));
    apply_furnace_action(&mut f, &mut inv, &mut held, GridAction::Place(FURNACE_OUTPUT_SLOT), false);
    assert_eq!(held, Some(ItemStack::new(43, 2)));
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), None);
}

#[test]
fn action_take_from_output_works() {
    let mut f = new_furnace();
    f.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(43, 7)));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    let mut held: Option<ItemStack> = None;
    apply_furnace_action(&mut f, &mut inv, &mut held, GridAction::Take(FURNACE_OUTPUT_SLOT), false);
    assert_eq!(held, Some(ItemStack::new(43, 7)));
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), None);
}

#[test]
fn action_take_half_from_output_works() {
    let mut f = new_furnace();
    f.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(43, 7)));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    let mut held: Option<ItemStack> = None;
    apply_furnace_action(
        &mut f, &mut inv, &mut held, GridAction::TakeHalf(FURNACE_OUTPUT_SLOT), false,
    );
    assert_eq!(held, Some(ItemStack::new(43, 4)));
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some(ItemStack::new(43, 3)));
}

#[test]
fn action_transfer_from_furnace_to_player() {
    let mut f = new_furnace();
    f.set_slot(FURNACE_OUTPUT_SLOT, Some(ItemStack::new(43, 9)));
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    let mut held: Option<ItemStack> = None;
    apply_furnace_action(
        &mut f, &mut inv, &mut held, GridAction::Transfer(FURNACE_OUTPUT_SLOT), false,
    );
    assert_eq!(held, None);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), None);
    // W470 (AbstractFurnaceMenu: the RESULT slot quick-moves
    // backwards): the stack lands in the hotbar's right end.
    assert_eq!(inv[8], Some(ItemStack::new(43, 9)));
}

#[test]
fn action_transfer_from_player_to_furnace_respects_filters() {
    let mut f = new_furnace();
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    // W476: dirt (2) - stone smelts now, dirt stays junk.
    inv[3] = Some(ItemStack::new(2, 5));
    let mut held: Option<ItemStack> = None;
    apply_player_action(&mut inv, &mut f, &mut held, GridAction::Transfer(3), false);
    // W470 (AbstractFurnaceMenu.quickMoveStack): dirt is neither
    // smeltable nor fuel, so no furnace slot takes it - the MC
    // fallback hops it from the hotbar into the main rows instead.
    assert_eq!(inv[3], None);
    assert_eq!(inv[9], Some(ItemStack::new(2, 5)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), None);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), None);
}

#[test]
fn action_place_one_into_input_lifts_single_item() {
    let mut f = new_furnace();
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    let mut held: Option<ItemStack> = Some(ItemStack::new(14, 5));
    apply_furnace_action(
        &mut f, &mut inv, &mut held, GridAction::PlaceOne(FURNACE_INPUT_SLOT), false,
    );
    assert_eq!(held, Some(ItemStack::new(14, 4)));
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some(ItemStack::new(14, 1)));
}

#[test]
fn full_ui_session_roundtrip() {
    // Simulate a 2-step user flow: place input + fuel via the
    // GridAction path, run a cook, take the ingot.
    // W476: the fuel is CHARCOAL (993) now - coal-ore 13 became
    // smeltable (MC coal_ore -> coal), so its quick-move targets
    // the input slot per AbstractFurnaceMenu.quickMoveStack and
    // would bounce off the occupied input here.
    let mut f = new_furnace();
    let mut inv: Vec<Option<ItemStack>> = vec![None; 36];
    inv[0] = Some(ItemStack::new(14, 2));
    inv[1] = Some(ItemStack::new(993, 1));
    let mut held: Option<ItemStack> = None;
    apply_player_action(&mut inv, &mut f, &mut held, GridAction::Transfer(0), false);
    assert_eq!(f.slot(FURNACE_INPUT_SLOT), Some(ItemStack::new(14, 2)));
    apply_player_action(&mut inv, &mut f, &mut held, GridAction::Transfer(1), false);
    assert_eq!(f.slot(FURNACE_FUEL_SLOT), Some(ItemStack::new(993, 1)));
    let mut rt = FurnaceRuntime::default();
    step_one_furnace(&mut f, &mut rt, FURNACE_COOK_MS_PER_OUTPUT);
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), Some(ItemStack::new(43, 1)));
    apply_furnace_action(
        &mut f, &mut inv, &mut held, GridAction::Transfer(FURNACE_OUTPUT_SLOT), false,
    );
    assert_eq!(f.slot(FURNACE_OUTPUT_SLOT), None);
    assert!(inv.contains(&Some(ItemStack::new(43, 1))));
}
