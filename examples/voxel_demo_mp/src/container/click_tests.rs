//! W470 - unit tests for the container-click core, with the
//! expected values derived from `AbstractContainerMenu.doClick`
//! (MC 26.1.2 decompile). Every op is also wrapped in a
//! conservation assert: no interaction may create or destroy
//! items.

use super::*;

/// Shorthand: a worn variant of `ItemStack::new`.
fn worn(id: u16, count: u8, damage: u16) -> ItemStack {
    let mut st = ItemStack::new(id, count);
    st.components.damage = damage;
    st
}

/// Total item count of `id` across sections + cursor (+ extras like
/// thrown stacks). The property-style conservation helper.
fn total_of(
    sections: &[SectionMut<'_>],
    cursor: &Option<ItemStack>,
    extra: &[Option<ItemStack>],
    id: u16,
) -> u32 {
    let mut n: u32 = 0;
    for s in sections.iter() {
        for st in s.slots.iter().flatten() {
            if st.id == id {
                n += st.count as u32;
            }
        }
    }
    for st in cursor.iter().chain(extra.iter().flatten()) {
        if st.id == id {
            n += st.count as u32;
        }
    }
    n
}

fn accept_never(_id: u16) -> bool {
    false
}

fn accept_only_7(id: u16) -> bool {
    id == 7
}

// ---- PICKUP -------------------------------------------------

#[test]
fn pickup_rmb_empty_cursor_takes_ceil_half() {
    // Source line 445: (count + 1) / 2 -> cursor 3, slot 2 for 5.
    let mut a = [Some(ItemStack::new(1, 5))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = None;
    pickup_click(&mut sections, &mut cursor, 0, true);
    assert_eq!(cursor, Some(ItemStack::new(1, 3)));
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 2)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 5);
}

#[test]
fn pickup_lmb_takes_whole_and_places_whole() {
    let mut a = [Some(worn(9, 4, 3)), None];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = None;
    pickup_click(&mut sections, &mut cursor, 0, false);
    assert_eq!(cursor, Some(worn(9, 4, 3)));
    assert_eq!(sections[0].slots[0], None);
    // Place the whole stack into the empty slot 1; components ride.
    pickup_click(&mut sections, &mut cursor, 1, false);
    assert_eq!(cursor, None);
    assert_eq!(sections[0].slots[1], Some(worn(9, 4, 3)));
}

#[test]
fn pickup_rmb_with_cursor_places_exactly_one() {
    let mut a = [None, Some(ItemStack::new(1, 10))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(1, 8));
    // Onto empty.
    pickup_click(&mut sections, &mut cursor, 0, true);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 1)));
    assert_eq!(cursor, Some(ItemStack::new(1, 7)));
    // Onto same_stackable with room.
    pickup_click(&mut sections, &mut cursor, 1, true);
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(1, 11)));
    assert_eq!(cursor, Some(ItemStack::new(1, 6)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 18);
}

#[test]
fn pickup_rmb_onto_foreign_stack_swaps_like_lmb() {
    // Source lines 455-458: the swap branch is button-independent -
    // RMB with a cursor over a foreign stack swaps exactly like
    // LMB (only the same-stackable merge branch reads the button).
    let mut a = [Some(ItemStack::new(2, 5))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(3, 4));
    pickup_click(&mut sections, &mut cursor, 0, true);
    // Swap happened (source lines 455-458 run for both buttons).
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(3, 4)));
    assert_eq!(cursor, Some(ItemStack::new(2, 5)));
}

#[test]
fn pickup_lmb_merges_up_to_cap_remainder_stays_on_cursor() {
    let mut a = [Some(ItemStack::new(1, 60))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(1, 10));
    pickup_click(&mut sections, &mut cursor, 0, false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 64)));
    assert_eq!(cursor, Some(ItemStack::new(1, 6)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 70);
}

#[test]
fn pickup_full_place_onto_different_components_swaps() {
    // The W467 semantic, confirmed by source lines 455-458: a
    // full place onto same-id-different-components SWAPS.
    let mut a = [Some(worn(7, 1, 9))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(7, 1));
    pickup_click(&mut sections, &mut cursor, 0, false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(7, 1)));
    assert_eq!(cursor, Some(worn(7, 1, 9)));
}

#[test]
fn pickup_place_into_rejecting_slot_is_noop() {
    let mut a = [None];
    let accept = [Some(accept_never as SlotAccept)];
    let mut sections = [SectionMut { slots: &mut a, accept: &accept, caps: &[] }];
    let mut cursor = Some(ItemStack::new(1, 4));
    pickup_click(&mut sections, &mut cursor, 0, false);
    assert_eq!(sections[0].slots[0], None);
    assert_eq!(cursor, Some(ItemStack::new(1, 4)));
}

#[test]
fn pickup_click_on_restricted_slot_pulls_into_matching_cursor() {
    // Source lines 459-464: slot rejects placement (furnace
    // output) but holds a same_stackable stack -> the click pulls
    // it INTO the cursor up to the cursor's max stack.
    let mut a = [Some(ItemStack::new(1, 30))];
    let accept = [Some(accept_never as SlotAccept)];
    let mut sections = [SectionMut { slots: &mut a, accept: &accept, caps: &[] }];
    let mut cursor = Some(ItemStack::new(1, 40));
    pickup_click(&mut sections, &mut cursor, 0, false);
    // Cursor caps at 64: pulls 24, leaves 6.
    assert_eq!(cursor, Some(ItemStack::new(1, 64)));
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 6)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 70);
}

// ---- THROW --------------------------------------------------

#[test]
fn throw_from_cursor_lmb_whole_rmb_one() {
    let mut cursor = Some(worn(9, 5, 2));
    let one = throw_from_cursor(&mut cursor, false);
    assert_eq!(one, Some(worn(9, 1, 2)));
    assert_eq!(cursor, Some(worn(9, 4, 2)));
    let rest = throw_from_cursor(&mut cursor, true);
    assert_eq!(rest, Some(worn(9, 4, 2)));
    assert_eq!(cursor, None);
    assert!(throw_from_cursor(&mut cursor, true).is_none());
}

#[test]
fn throw_from_slot_preserves_components_and_needs_empty_cursor() {
    let mut a = [Some(worn(9, 6, 4))];
    let mut sections = [SectionMut::open(&mut a)];
    // Non-empty cursor: gate refuses (source line 514).
    let busy = Some(ItemStack::new(1, 1));
    assert!(throw_from_slot(&mut sections, &busy, 0, false).is_none());
    // Q throws one; Ctrl+Q throws the rest.
    let none = None;
    assert_eq!(
        throw_from_slot(&mut sections, &none, 0, false),
        Some(worn(9, 1, 4))
    );
    assert_eq!(
        throw_from_slot(&mut sections, &none, 0, true),
        Some(worn(9, 5, 4))
    );
    assert_eq!(sections[0].slots[0], None);
}

// ---- SWAP ---------------------------------------------------

#[test]
fn swap_click_exchanges_and_respects_filters_both_ways() {
    // Section 0 = filtered container slot (only id 7); section 1 =
    // open player inv with a hotbar stack.
    let mut cont = [Some(ItemStack::new(7, 2))];
    let accept = [Some(accept_only_7 as SlotAccept)];
    let mut inv = [Some(ItemStack::new(3, 5))];
    {
        let mut sections = [
            SectionMut { slots: &mut cont, accept: &accept, caps: &[] },
            SectionMut::open(&mut inv),
        ];
        // Hotbar holds id 3 which the container slot refuses ->
        // swap is a no-op.
        swap_click(&mut sections, 0, 1, &[]);
        assert_eq!(sections[0].slots[0], Some(ItemStack::new(7, 2)));
        assert_eq!(sections[1].slots[0], Some(ItemStack::new(3, 5)));
    }
    // Replace hotbar with id 7 -> swap succeeds.
    inv[0] = Some(worn(7, 1, 8));
    let mut sections = [
        SectionMut { slots: &mut cont, accept: &accept, caps: &[] },
        SectionMut::open(&mut inv),
    ];
    swap_click(&mut sections, 0, 1, &[]);
    assert_eq!(sections[0].slots[0], Some(worn(7, 1, 8)));
    assert_eq!(sections[1].slots[0], Some(ItemStack::new(7, 2)));
}

#[test]
fn swap_click_moves_into_empty_sides() {
    let mut a = [Some(ItemStack::new(2, 3)), None];
    let mut sections = [SectionMut::open(&mut a)];
    // Hovered occupied, hotbar empty -> stack moves to hotbar.
    swap_click(&mut sections, 0, 1, &[]);
    assert_eq!(sections[0].slots[0], None);
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(2, 3)));
    // Hovered empty, hotbar occupied -> stack moves to hovered.
    swap_click(&mut sections, 0, 1, &[]);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(2, 3)));
    assert_eq!(sections[0].slots[1], None);
}

// ---- CLONE --------------------------------------------------

#[test]
fn clone_click_creative_gets_full_stack() {
    let mut a = [Some(worn(9, 3, 6))];
    let sections = [SectionMut::open(&mut a)];
    let mut cursor = None;
    // Survival: no-op.
    clone_click(&sections, &mut cursor, 0, false);
    assert_eq!(cursor, None);
    // Creative: full max-stack copy, slot untouched, components
    // cloned (source line 512: copyWithCount(maxStackSize)).
    clone_click(&sections, &mut cursor, 0, true);
    assert_eq!(cursor, Some(worn(9, 64, 6)));
    assert_eq!(sections[0].slots[0], Some(worn(9, 3, 6)));
    // Non-empty cursor: no-op.
    clone_click(&sections, &mut cursor, 0, true);
    assert_eq!(cursor, Some(worn(9, 64, 6)));
}

// ---- PICKUP_ALL ---------------------------------------------

#[test]
fn pickup_all_collects_across_sections_up_to_cap() {
    // Chest section + inv section; double-clicked slot (gi 0) is
    // empty (the first click picked its stack up onto the cursor).
    let mut chest = [None, Some(ItemStack::new(1, 30)), Some(ItemStack::new(2, 9))];
    let mut inv = [Some(ItemStack::new(1, 25)), Some(ItemStack::new(1, 20))];
    let mut sections = [
        SectionMut::open(&mut chest),
        SectionMut::open(&mut inv),
    ];
    let mut cursor = Some(ItemStack::new(1, 10));
    pickup_all(&mut sections, &mut cursor, 0, false);
    // 10 + 30 + 25 (partial from inv[0]... order: chest[1]=30 ->
    // 40, inv[0]=25 -> 64 caps after 24, inv[1] untouched at pass
    // cap). Cursor hits 64; leftover stays where it was.
    assert_eq!(cursor, Some(ItemStack::new(1, 64)));
    let remaining = total_of(&sections, &None, &[], 1);
    assert_eq!(remaining, 30 + 25 + 20 - 54);
    // The foreign id-2 stack is untouched.
    assert_eq!(sections[0].slots[2], Some(ItemStack::new(2, 9)));
}

#[test]
fn pickup_all_skips_different_components_stacks() {
    let mut a = [None, Some(ItemStack::new(7, 2)), Some(worn(7, 3, 5))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(7, 1));
    pickup_all(&mut sections, &mut cursor, 0, false);
    assert_eq!(cursor, Some(ItemStack::new(7, 3)));
    // The worn stack stays: different components never merge.
    assert_eq!(sections[0].slots[2], Some(worn(7, 3, 5)));
}

#[test]
fn pickup_all_partial_stacks_first_then_full() {
    // Pass 0 skips FULL stacks; with cursor room for only part of
    // the pool, the partial stack must be consumed before the full
    // one is touched (source line 549).
    let mut a = [None, Some(ItemStack::new(1, 64)), Some(ItemStack::new(1, 8))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(1, 40));
    pickup_all(&mut sections, &mut cursor, 0, false);
    assert_eq!(cursor, Some(ItemStack::new(1, 64)));
    // Partial (8) fully drained first; the full stack only lost
    // the remaining 16.
    assert_eq!(sections[0].slots[2], None);
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(1, 48)));
}

#[test]
fn pickup_all_requires_cursor_and_empty_clicked_slot() {
    let mut a = [Some(ItemStack::new(1, 5)), Some(ItemStack::new(1, 6))];
    let mut sections = [SectionMut::open(&mut a)];
    // Empty cursor: no-op.
    let mut cursor = None;
    pickup_all(&mut sections, &mut cursor, 0, false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 5)));
    // Clicked slot still occupied: no-op (source line 538).
    cursor = Some(ItemStack::new(1, 1));
    pickup_all(&mut sections, &mut cursor, 0, false);
    assert_eq!(cursor, Some(ItemStack::new(1, 1)));
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(1, 6)));
}

// ---- QUICK_CRAFT --------------------------------------------

#[test]
fn drag_lmb_7_over_3_slots_gives_2_each_1_back() {
    let mut a = [None, None, None, None];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(1, 7));
    quick_craft_apply(&mut sections, &mut cursor, &[0, 1, 2], false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 2)));
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(1, 2)));
    assert_eq!(sections[0].slots[2], Some(ItemStack::new(1, 2)));
    assert_eq!(sections[0].slots[3], None);
    assert_eq!(cursor, Some(ItemStack::new(1, 1)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 7);
}

#[test]
fn drag_rmb_places_one_per_slot() {
    let mut a = [None, Some(ItemStack::new(1, 2)), None];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(1, 5));
    quick_craft_apply(&mut sections, &mut cursor, &[0, 1, 2], true);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 1)));
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(1, 3)));
    assert_eq!(sections[0].slots[2], Some(ItemStack::new(1, 1)));
    assert_eq!(cursor, Some(ItemStack::new(1, 2)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 7);
}

#[test]
fn drag_skips_filtered_and_mismatched_slots() {
    // Slot 1 rejects everything (furnace output); slot 2 holds a
    // foreign stack. Their shares return to the cursor.
    let mut a = [None, None, Some(ItemStack::new(2, 1)), None];
    let accept = [None, Some(accept_never as SlotAccept), None, None];
    let mut sections = [SectionMut { slots: &mut a, accept: &accept, caps: &[] }];
    let mut cursor = Some(ItemStack::new(1, 8));
    quick_craft_apply(&mut sections, &mut cursor, &[0, 1, 2, 3], false);
    // floor(8/4) = 2 per eligible slot; slots 0 + 3 get 2 each,
    // slots 1 + 2 skipped -> 4 back on the cursor.
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 2)));
    assert_eq!(sections[0].slots[1], None);
    assert_eq!(sections[0].slots[2], Some(ItemStack::new(2, 1)));
    assert_eq!(sections[0].slots[3], Some(ItemStack::new(1, 2)));
    assert_eq!(cursor, Some(ItemStack::new(1, 4)));
}

#[test]
fn drag_single_slot_falls_back_to_pickup_click() {
    // Source lines 362-366: one visited slot -> plain PICKUP with
    // the drag's button. LMB drag over one slot = place all.
    let mut a = [None];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(1, 7));
    quick_craft_apply(&mut sections, &mut cursor, &[0], false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 7)));
    assert_eq!(cursor, None);
    // RMB drag over one slot = place one.
    let mut b = [None];
    let mut sections = [SectionMut::open(&mut b)];
    let mut cursor = Some(ItemStack::new(1, 7));
    quick_craft_apply(&mut sections, &mut cursor, &[0], true);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 1)));
    assert_eq!(cursor, Some(ItemStack::new(1, 6)));
}

#[test]
fn drag_merges_on_top_of_existing_carry() {
    // Source line 384-388: newCount = placeCount + carry, capped.
    let mut a = [Some(ItemStack::new(1, 60)), Some(ItemStack::new(1, 1))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(1, 10));
    quick_craft_apply(&mut sections, &mut cursor, &[0, 1], false);
    // floor(10/2) = 5 per slot. Slot 0: 60+5 capped at 64 -> +4.
    // Slot 1: 1+5 = 6 -> +5. Placed 9, 1 back on cursor.
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 64)));
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(1, 6)));
    assert_eq!(cursor, Some(ItemStack::new(1, 1)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 71);
}

#[test]
fn drag_components_clone_to_fresh_slots() {
    let mut a = [None, None];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(worn(9, 4, 11));
    quick_craft_apply(&mut sections, &mut cursor, &[0, 1], false);
    assert_eq!(sections[0].slots[0], Some(worn(9, 2, 11)));
    assert_eq!(sections[0].slots[1], Some(worn(9, 2, 11)));
    assert_eq!(cursor, None);
}

#[test]
fn can_add_drag_slot_matches_source_gates() {
    let cur = ItemStack::new(1, 2);
    // Empty slot + accept + count(2) > visited(1) -> ok.
    assert!(can_add_drag_slot(None, true, &cur, 1));
    // count(2) > visited(2) fails -> can't add a 3rd slot.
    assert!(!can_add_drag_slot(None, true, &cur, 2));
    // Reject filter fails.
    assert!(!can_add_drag_slot(None, false, &cur, 0));
    // Foreign stack fails quick-replace.
    let foreign = ItemStack::new(2, 1);
    assert!(!can_add_drag_slot(Some(&foreign), true, &cur, 0));
    // Same-stackable occupied slot qualifies.
    let same = ItemStack::new(1, 60);
    assert!(can_add_drag_slot(Some(&same), true, &cur, 0));
}

// ---- QUICK_MOVE ---------------------------------------------

#[test]
fn quick_move_merges_then_first_empty_along_order() {
    // moveItemStackTo phase order: top up matching stacks along
    // the order FIRST, then claim the first accepting empty.
    let mut src = [Some(ItemStack::new(1, 20))];
    let mut dst = [None, Some(ItemStack::new(1, 60)), None];
    let mut sections = [
        SectionMut::open(&mut src),
        SectionMut::open(&mut dst),
    ];
    // Order visits dst slots 1(gi 2), 0(gi 1), 2(gi 3).
    assert!(quick_move(&mut sections, 0, &[2, 1, 3]));
    assert_eq!(sections[0].slots[0], None);
    // 4 merged into the 60-stack, 16 into the first empty (gi 1).
    assert_eq!(sections[1].slots[1], Some(ItemStack::new(1, 64)));
    assert_eq!(sections[1].slots[0], Some(ItemStack::new(1, 16)));
    assert_eq!(sections[1].slots[2], None);
}

#[test]
fn quick_move_leftover_stays_in_source() {
    let mut src = [Some(worn(9, 10, 2))];
    let mut dst = [Some(worn(9, 60, 2))];
    let mut sections = [
        SectionMut::open(&mut src),
        SectionMut::open(&mut dst),
    ];
    assert!(quick_move(&mut sections, 0, &[1]));
    assert_eq!(sections[1].slots[0], Some(worn(9, 64, 2)));
    assert_eq!(sections[0].slots[0], Some(worn(9, 6, 2)));
}

#[test]
fn quick_move_respects_accept_on_empty_slots_only() {
    // Phase 2 (empty claim) honours accepts; a rejecting slot is
    // skipped and the next accepting one takes the stack.
    let mut src = [Some(ItemStack::new(1, 5))];
    let mut dst = [None, None];
    let accept = [Some(accept_never as SlotAccept), None];
    let mut sections = [
        SectionMut::open(&mut src),
        SectionMut { slots: &mut dst, accept: &accept, caps: &[] },
    ];
    assert!(quick_move(&mut sections, 0, &[1, 2]));
    assert_eq!(sections[1].slots[0], None);
    assert_eq!(sections[1].slots[1], Some(ItemStack::new(1, 5)));
}

#[test]
fn quick_move_nothing_possible_returns_false() {
    let mut src = [Some(ItemStack::new(1, 5))];
    let mut dst = [Some(ItemStack::new(2, 64))];
    let mut sections = [
        SectionMut::open(&mut src),
        SectionMut::open(&mut dst),
    ];
    assert!(!quick_move(&mut sections, 0, &[1]));
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 5)));
}

// ---- W472: per-slot max-stack caps --------------------------

#[test]
fn cap_limits_pickup_place_and_merge() {
    // `Slot.getMaxStackSize(stack)` = min(item max, slot cap):
    // a 16-cap slot takes 16 on a full place, remainder stays on
    // the cursor; a follow-up merge is refused at the cap.
    let mut a = [None];
    let caps = [16u8];
    let mut sections =
        [SectionMut { slots: &mut a, accept: &[], caps: &caps }];
    let mut cursor = Some(ItemStack::new(1, 40));
    pickup_click(&mut sections, &mut cursor, 0, false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 16)));
    assert_eq!(cursor, Some(ItemStack::new(1, 24)));
    // Merge attempt on the full-at-cap slot moves nothing.
    pickup_click(&mut sections, &mut cursor, 0, false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 16)));
    assert_eq!(cursor, Some(ItemStack::new(1, 24)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 40);
}

#[test]
fn cap_blocks_oversized_cursor_swap() {
    // Source line 455: the foreign-stack swap requires
    // `carried.getCount() <= slot.getMaxStackSize(carried)` - a
    // 2-stack cursor cannot swap into a 1-cap slot.
    let mut a = [Some(ItemStack::new(2, 1))];
    let caps = [1u8];
    let mut sections =
        [SectionMut { slots: &mut a, accept: &[], caps: &caps }];
    let mut cursor = Some(ItemStack::new(1, 2));
    pickup_click(&mut sections, &mut cursor, 0, false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(2, 1)));
    assert_eq!(cursor, Some(ItemStack::new(1, 2)));
    // A 1-count cursor swaps fine.
    cursor = Some(ItemStack::new(1, 1));
    pickup_click(&mut sections, &mut cursor, 0, false);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 1)));
    assert_eq!(cursor, Some(ItemStack::new(2, 1)));
}

#[test]
fn swap_overflow_splits_into_capped_empty_slot() {
    // Source line 486 (LIVE with caps): hotbar 40 into an empty
    // 16-cap slot leaves 24 in the hotbar slot.
    let mut cont = [None];
    let caps = [16u8];
    let mut inv = [Some(ItemStack::new(1, 40))];
    let mut sections = [
        SectionMut { slots: &mut cont, accept: &[], caps: &caps },
        SectionMut::open(&mut inv),
    ];
    assert!(swap_click(&mut sections, 0, 1, &[]).is_none());
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 16)));
    assert_eq!(sections[1].slots[0], Some(ItemStack::new(1, 24)));
}

#[test]
fn swap_overflow_rehomes_displaced_stack_via_add_order() {
    // Source line 495-500: overflow into an OCCUPIED capped slot -
    // the target's old stack re-homes through `Inventory.add`
    // (merge-then-first-empty along the add order); anything left
    // is returned for the caller to drop.
    let mut cont = [Some(ItemStack::new(2, 3))];
    let caps = [16u8];
    let mut inv = [Some(ItemStack::new(1, 40)), None];
    {
        let mut sections = [
            SectionMut { slots: &mut cont, accept: &[], caps: &caps },
            SectionMut::open(&mut inv),
        ];
        // add order: the free inv slot (gi 2).
        let dropped = swap_click(&mut sections, 0, 1, &[2]);
        assert!(dropped.is_none());
        assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 16)));
        assert_eq!(sections[1].slots[0], Some(ItemStack::new(1, 24)));
        assert_eq!(sections[1].slots[1], Some(ItemStack::new(2, 3)));
    }
    // No room in the add order -> the displaced stack is RETURNED
    // (MC: player.drop).
    cont[0] = Some(ItemStack::new(2, 3));
    inv[0] = Some(ItemStack::new(1, 40));
    inv[1] = Some(ItemStack::new(9, 1));
    let mut sections = [
        SectionMut { slots: &mut cont, accept: &[], caps: &caps },
        SectionMut::open(&mut inv),
    ];
    let dropped = swap_click(&mut sections, 0, 1, &[2]);
    assert_eq!(dropped, Some(ItemStack::new(2, 3)));
}

#[test]
fn quick_move_respects_per_slot_caps() {
    // moveItemStackTo with caps: merge tops up to the SLOT cap,
    // and the single empty-slot claim places at most the cap
    // (the source still breaks after one empty slot - line 688).
    let mut src = [Some(ItemStack::new(1, 30))];
    let mut dst = [Some(ItemStack::new(1, 10)), None, None];
    let caps = [16u8, 16, 16];
    let mut sections = [
        SectionMut::open(&mut src),
        SectionMut { slots: &mut dst, accept: &[], caps: &caps },
    ];
    assert!(quick_move(&mut sections, 0, &[1, 2, 3]));
    // Merge phase: 10 -> 16 (6 moved). Empty phase: ONE slot takes
    // 16; the remaining 8 stay in the source.
    assert_eq!(sections[1].slots[0], Some(ItemStack::new(1, 16)));
    assert_eq!(sections[1].slots[1], Some(ItemStack::new(1, 16)));
    assert_eq!(sections[1].slots[2], None);
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 8)));
}

#[test]
fn quick_craft_respects_per_slot_caps() {
    // Source line 385: maxSize = min(source max, slot cap).
    let mut a = [Some(ItemStack::new(1, 14)), None];
    let caps = [16u8, 16];
    let mut sections =
        [SectionMut { slots: &mut a, accept: &[], caps: &caps }];
    let mut cursor = Some(ItemStack::new(1, 20));
    quick_craft_apply(&mut sections, &mut cursor, &[0, 1], false);
    // floor(20/2) = 10 per slot. Slot 0: 14+10 capped at 16 (+2);
    // slot 1: 10. Placed 12, 8 back on the cursor.
    assert_eq!(sections[0].slots[0], Some(ItemStack::new(1, 16)));
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(1, 10)));
    assert_eq!(cursor, Some(ItemStack::new(1, 8)));
    assert_eq!(total_of(&sections, &cursor, &[], 1), 34);
}

#[test]
fn pickup_all_reverse_walks_end_first() {
    // Button-1 variant (no UI input wired - kept for source
    // parity): with room for one stack only, reverse order drains
    // the LAST slot instead of the first.
    let mut a = [None, Some(ItemStack::new(1, 30)), Some(ItemStack::new(1, 20))];
    let mut sections = [SectionMut::open(&mut a)];
    let mut cursor = Some(ItemStack::new(1, 34));
    pickup_all(&mut sections, &mut cursor, 0, true);
    assert_eq!(cursor, Some(ItemStack::new(1, 64)));
    assert_eq!(sections[0].slots[2], None, "last slot drained first");
    assert_eq!(sections[0].slots[1], Some(ItemStack::new(1, 20)));
}

#[test]
fn quick_move_unstackable_skips_merge_phase() {
    // Max-stack-1 items (shield id 642) never merge - phase 1 is
    // skipped entirely (source: itemStack.isStackable() gate) and
    // the item claims the first empty slot.
    let mut src = [Some(ItemStack::new(642, 1))];
    let mut dst = [Some(ItemStack::new(642, 1)), None];
    let mut sections = [
        SectionMut::open(&mut src),
        SectionMut::open(&mut dst),
    ];
    assert!(quick_move(&mut sections, 0, &[1, 2]));
    assert_eq!(sections[1].slots[0], Some(ItemStack::new(642, 1)));
    assert_eq!(sections[1].slots[1], Some(ItemStack::new(642, 1)));
    assert_eq!(sections[0].slots[0], None);
}
