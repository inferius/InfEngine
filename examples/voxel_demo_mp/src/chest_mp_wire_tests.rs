//! W5.3a/b regression suite: chest + furnace MP wire.
//!
//! The full GUI demo's `tick_net` orchestrates the live socket;
//! these tests drive the apply methods on `Voxel` directly so we
//! can validate the wire contract (open registers a viewer, act
//! mutates the chest + drives the per-client inventory, close
//! clears the viewer, furnace state throttles to ~5 Hz) without
//! standing up a real UDP socket. Wire-level encode/decode is
//! covered by the round-trip bincode test at the bottom.

use crate::net::{
    ChestAct, ChestActMsg, ChestCloseMsg, ChestOpenMsg, ChestStateMsg,
    FurnaceCloseMsg, FurnaceOpenMsg, FurnaceStateMsg,
};
use crate::Voxel;

use crate::item::ItemStack;

#[test]
fn chest_open_registers_viewer() {
    let mut v = Voxel::new();
    let pos = [4, 8, 4];
    v.apply_chest_open_msgs(&[(7, ChestOpenMsg { pos })]);
    assert_eq!(v.chest_viewers.get(&pos).copied(), Some(7));
    // First open also mints an empty chest at pos so subsequent
    // acts have a slot vec to mutate against.
    assert!(v.chests.contains_key(&pos));
}

#[test]
fn chest_open_replaces_previous_viewer() {
    // Single-viewer for Wave 5: a new client opening the same
    // chest displaces the previous viewer.
    let mut v = Voxel::new();
    let pos = [0, 8, 0];
    v.apply_chest_open_msgs(&[(1, ChestOpenMsg { pos })]);
    assert_eq!(v.chest_viewers.get(&pos).copied(), Some(1));
    v.apply_chest_open_msgs(&[(2, ChestOpenMsg { pos })]);
    assert_eq!(v.chest_viewers.get(&pos).copied(), Some(2));
}

#[test]
fn chest_close_clears_viewer() {
    let mut v = Voxel::new();
    let pos = [1, 2, 3];
    v.apply_chest_open_msgs(&[(9, ChestOpenMsg { pos })]);
    assert!(v.chest_viewers.contains_key(&pos));
    v.apply_chest_close_msgs(&[(9, ChestCloseMsg { pos })]);
    assert!(!v.chest_viewers.contains_key(&pos));
}

#[test]
fn chest_close_from_non_viewer_is_noop() {
    // A stray close from a client that wasn't registered must not
    // evict the legitimate viewer.
    let mut v = Voxel::new();
    let pos = [1, 2, 3];
    v.apply_chest_open_msgs(&[(9, ChestOpenMsg { pos })]);
    v.apply_chest_close_msgs(&[(99, ChestCloseMsg { pos })]);
    assert_eq!(v.chest_viewers.get(&pos).copied(), Some(9));
}

#[test]
fn chest_act_from_non_viewer_is_ignored() {
    // Only the registered viewer's acts touch the chest. A spoofed
    // act from another client must be dropped silently.
    let mut v = Voxel::new();
    let pos = [0, 8, 0];
    v.apply_chest_open_msgs(&[(5, ChestOpenMsg { pos })]);
    // Seed the chest with one stone stack so we can check the
    // act wasn't applied.
    if let Some(c) = v.chests.get_mut(&pos) {
        c.set_slot(0, Some(ItemStack::new(1, 16)));
    }
    v.apply_chest_act_msgs(&[(
        99,
        ChestActMsg {
            pos,
            player_side: false,
            action: ChestAct::Take(0),
        },
    )]);
    assert_eq!(v.chests.get(&pos).unwrap().slot(0), Some(ItemStack::new(1, 16)));
}

#[test]
fn furnace_open_registers_viewer() {
    let mut v = Voxel::new();
    let pos = [2, 4, 2];
    v.apply_furnace_open_msgs(&[(3, FurnaceOpenMsg { pos })]);
    assert_eq!(v.furnace_viewers.get(&pos).copied(), Some(3));
    assert!(v.furnaces.contains_key(&pos));
}

#[test]
fn furnace_close_clears_viewer_and_throttle() {
    let mut v = Voxel::new();
    let pos = [0, 0, 0];
    v.apply_furnace_open_msgs(&[(1, FurnaceOpenMsg { pos })]);
    // Simulate one throttle stamp from a prior broadcast.
    v.furnace_state_last_sent_ms.insert(pos, 12345);
    v.apply_furnace_close_msgs(&[(1, FurnaceCloseMsg { pos })]);
    assert!(!v.furnace_viewers.contains_key(&pos));
    assert!(!v.furnace_state_last_sent_ms.contains_key(&pos));
}

#[test]
fn furnace_state_5hz_throttle() {
    // The throttle gate accepts the first broadcast (no prior
    // stamp), rejects calls inside the 200 ms window, and accepts
    // again once the window expires.
    let mut v = Voxel::new();
    let pos = [9, 9, 9];
    let t0 = 1_000_000u64;
    assert!(v.should_send_furnace_state(pos, t0));
    // Calls at t0 + 100ms and t0 + 199ms are inside the window.
    assert!(!v.should_send_furnace_state(pos, t0 + 100));
    assert!(!v.should_send_furnace_state(pos, t0 + 199));
    // Call at t0 + 200ms is on the edge; the gate uses `<` so
    // exactly 200 ms passes.
    assert!(v.should_send_furnace_state(pos, t0 + 200));
}

#[test]
fn furnace_state_throttle_simulates_5hz_under_high_freq_calls() {
    // Spec: "5 Hz throttle: send 20 cook-tick events, expect at
    // most ~5 state snapshots." We drive the gate at 10 Hz (every
    // 100 ms) for 20 ticks - 2 seconds total. The gate should
    // accept at t=0, t=200, t=400, ... = 10 broadcasts.
    let mut v = Voxel::new();
    let pos = [0, 0, 0];
    let mut accepted = 0u32;
    for i in 0..20 {
        let now = i * 100;
        if v.should_send_furnace_state(pos, now) {
            accepted += 1;
        }
    }
    // 20 ticks at 10 Hz = 2 seconds; at 5 Hz that's 10 accepts.
    // Range allows +/-1 for boundary effects.
    assert!((9..=11).contains(&accepted), "expected ~10, got {accepted}");
}

#[test]
fn chest_state_msg_round_trips() {
    // Wire-level: encode + decode a ChestStateMsg through bincode
    // and verify the stack-native slots + cursor survive - W468:
    // components included.
    let cfg = bincode::config::standard();
    let mut worn = ItemStack::new(7, 1);
    worn.components.damage = 13;
    let original = ChestStateMsg {
        pos: [5, 10, 15],
        slots: vec![Some(ItemStack::new(1, 32)), None, Some(worn.clone())],
        cursor: Some(ItemStack::new(3, 12)),
    };
    let buf = bincode::serde::encode_to_vec(&original, cfg).expect("encode");
    let (decoded, _): (ChestStateMsg, _) =
        bincode::serde::decode_from_slice(&buf, cfg).expect("decode");
    assert_eq!(decoded.pos, original.pos);
    assert_eq!(decoded.slots, original.slots);
    assert_eq!(decoded.slots[2].as_ref().unwrap().components.damage, 13);
    assert_eq!(decoded.cursor, original.cursor);
}

#[test]
fn furnace_state_msg_round_trips() {
    let cfg = bincode::config::standard();
    let original = FurnaceStateMsg {
        pos: [0, 0, 0],
        slots: vec![Some(ItemStack::new(2, 8)), None, None],
        fuel_left_ms: 1234,
        fuel_full_ms: 5000,
        cook_progress_ms: 333,
        cursor: None,
    };
    let buf = bincode::serde::encode_to_vec(&original, cfg).expect("encode");
    let (decoded, _): (FurnaceStateMsg, _) =
        bincode::serde::decode_from_slice(&buf, cfg).expect("decode");
    assert_eq!(decoded.pos, original.pos);
    assert_eq!(decoded.slots, original.slots);
    assert_eq!(decoded.fuel_left_ms, 1234);
    assert_eq!(decoded.cook_progress_ms, 333);
}

/// W468: the per-viewer cursor keeps a remote take -> place-elsewhere
/// of a WORN item lossless: after Take the stack sits on the host's
/// per-client cursor (not a discarded temporary), after Place it
/// lands in the target slot with its damage intact - nothing
/// duplicated, nothing lost.
#[test]
fn remote_take_then_place_preserves_worn_stack() {
    let mut v = Voxel::new();
    let pos = [3, 8, 3];
    let mut worn = ItemStack::new(48, 1);
    worn.components.damage = 21;
    v.apply_chest_open_msgs(&[(7, ChestOpenMsg { pos })]);
    if let Some(c) = v.chests.get_mut(&pos) {
        c.set_slot(0, Some(worn.clone()));
    }
    // Take: the worn stack moves onto the viewer's cursor.
    v.apply_chest_act_msgs(&[(
        7,
        ChestActMsg {
            pos,
            player_side: false,
            action: ChestAct::Take(0),
        },
    )]);
    assert_eq!(v.chests.get(&pos).unwrap().slot(0), None);
    assert_eq!(
        v.container_cursors.get(&7).cloned().flatten(),
        Some(worn.clone()),
        "taken stack lives on the per-viewer cursor"
    );
    // Place elsewhere: cursor empties, slot 5 holds the worn stack.
    v.apply_chest_act_msgs(&[(
        7,
        ChestActMsg {
            pos,
            player_side: false,
            action: ChestAct::Place(5),
        },
    )]);
    assert_eq!(v.container_cursors.get(&7).cloned().flatten(), None);
    let chest = v.chests.get(&pos).unwrap();
    assert_eq!(chest.slot(5), Some(worn));
    // Nothing duplicated: exactly one occupied slot.
    let occupied = chest.slots().iter().flatten().count();
    assert_eq!(occupied, 1);
}

/// W468: closing the container recovers the viewer's cursor - with
/// no per-client inventory registered (offline harness) the held
/// stack spills as a floor drop at the chest instead of vanishing.
#[test]
fn viewer_close_recovers_cursor_stack() {
    let mut v = Voxel::new();
    let pos = [1, 8, 1];
    let mut worn = ItemStack::new(48, 1);
    worn.components.damage = 5;
    v.apply_chest_open_msgs(&[(4, ChestOpenMsg { pos })]);
    if let Some(c) = v.chests.get_mut(&pos) {
        c.set_slot(2, Some(worn.clone()));
    }
    v.apply_chest_act_msgs(&[(
        4,
        ChestActMsg {
            pos,
            player_side: false,
            action: ChestAct::Take(2),
        },
    )]);
    assert!(v.container_cursors.get(&4).cloned().flatten().is_some());
    v.apply_chest_close_msgs(&[(4, ChestCloseMsg { pos })]);
    assert!(
        v.container_cursors.get(&4).cloned().flatten().is_none(),
        "cursor cleared on close"
    );
    // No inventory to return into (offline harness): the stack
    // spilled as a drop at the chest, components intact.
    assert_eq!(v.dropped_items.len(), 1);
    assert_eq!(v.dropped_items[0].stack, worn);
}

#[test]
fn chest_act_to_grid_action_round_trips() {
    // The wire enum mirrors `container::ui::GridAction` 1:1.
    use crate::container::ui::GridAction;
    let cases = [
        (GridAction::Take(0), ChestAct::Take(0)),
        (GridAction::Place(3), ChestAct::Place(3)),
        (GridAction::TakeHalf(7), ChestAct::TakeHalf(7)),
        (GridAction::PlaceOne(2), ChestAct::PlaceOne(2)),
        (GridAction::Transfer(13), ChestAct::Transfer(13)),
    ];
    for (ga, expected_wire) in cases {
        let wire = ChestAct::from_grid_action(ga);
        assert_eq!(wire, expected_wire);
        let round = wire.to_grid_action();
        assert_eq!(round, ga);
    }
}
