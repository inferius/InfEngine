//! Inventory + hotbar + crafting-grid slot logic.
//!
//! W472: the inventory screen (E) is a real MC menu now - the
//! egui drag&drop path (`SplitMode`, `drag_stacks`,
//! `inv_apply_drag`) is GONE. This module owns the shared
//! "InventoryMenu" screen core both the local UI panel
//! (`ui::inventory_screen`) and the server wire applier
//! (`net::container_apply::apply_inv_op_msgs`) route through, so
//! local prediction and authority use one rule set:
//!
//! - `SlotKind` addresses a cell of the screen (36 inventory
//!   cells, 4 craft cells, 4 armor cells, the virtual craft
//!   output). Bridges to `net::NetSlot` via the two `From` impls.
//! - `inv_screen_sections` / `inv_screen_gi` express the screen in
//!   MC `InventoryMenu` slot order: result(virtual), craft 1..5,
//!   armor 5..9, main 9..36, hotbar 36..45 (we drop the virtual
//!   result from the section list, so gi = MC index - 1).
//! - `inv_screen_apply_op` applies one wire-shaped op
//!   (`net::InvOpMsg`) against the screen + cursor through the
//!   shared click core; `inv_screen_apply_drag` is the QUICK_CRAFT
//!   release.
//! - `inv_pickup` / `inv_consume` remain the small "+1 / -1"
//!   helpers used by mining + crafting outcomes.
//!
//! The 2x2 recipe lookup (`match_recipe`) still lives in
//! `ui/mod.rs`; this module calls it through `crate::match_recipe`.

use infengine_voxel::BlockId;

use crate::container::click::{self, SectionMut};
use crate::item::ItemStack;
use crate::match_recipe;
use crate::net::NetSlot;

/// Hotbar occupies the first row of the inventory grid; the
/// remaining `INVENTORY_ROWS - 1` rows are the open-inventory
/// extension Minecraft players expect. Every slot is a normal
/// stack slot - mining is driven by LMB regardless of what's
/// held (the held tool only affects mining speed + drops).
pub(crate) const HOTBAR_SLOTS: usize = 9;
pub(crate) const INVENTORY_COLS: usize = 9;
pub(crate) const INVENTORY_ROWS: usize = 4;
pub(crate) const INVENTORY_SLOTS: usize = INVENTORY_COLS * INVENTORY_ROWS;
/// Default stack cap - only the drop-merge tests reference it
/// directly now (`item_max_stack` is the live source of truth).
#[cfg(test)]
pub(crate) const STACK_MAX: u8 = 64;

/// W472: armor section size (helmet / chest / legs / boots).
pub(crate) const ARMOR_SLOTS: usize = 4;

// W470: the click-core screen adapter duplicates the two geometry
// consts (the headless lib mounts `container` but not this
// module). Keep them in lockstep.
const _: () = assert!(
    crate::container::screen::HOTBAR_SLOTS == HOTBAR_SLOTS
        && crate::container::screen::INVENTORY_SLOTS == INVENTORY_SLOTS
);

/// Identifier for an inventory-screen cell. Doubles as the wire
/// address via the `NetSlot` bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SlotKind {
    /// One of the 36 main inventory slots (0..HOTBAR_SLOTS = hotbar,
    /// HOTBAR_SLOTS..INVENTORY_SLOTS = storage rows).
    Inv(usize),
    /// One of the 4 cells in the 2x2 crafting grid.
    Craft(usize),
    /// The single crafting-output cell. Virtual (take-only).
    CraftOut,
    /// W472: one of the 4 armor slots (0 helmet .. 3 boots),
    /// matching MC `InventoryMenu` armor slots 5..9 (HEAD..FEET).
    Armor(usize),
}

impl From<SlotKind> for NetSlot {
    fn from(k: SlotKind) -> Self {
        match k {
            SlotKind::Inv(i) => NetSlot::Inv(i as u32),
            SlotKind::Craft(i) => NetSlot::Craft(i as u32),
            SlotKind::CraftOut => NetSlot::CraftOut,
            SlotKind::Armor(i) => NetSlot::Armor(i as u32),
        }
    }
}

impl From<NetSlot> for SlotKind {
    fn from(k: NetSlot) -> Self {
        match k {
            NetSlot::Inv(i) => SlotKind::Inv(i as usize),
            NetSlot::Craft(i) => SlotKind::Craft(i as usize),
            NetSlot::CraftOut => SlotKind::CraftOut,
            NetSlot::Armor(i) => SlotKind::Armor(i as usize),
        }
    }
}

// ---------------------------------------------------------------
// W472: the InventoryMenu screen model.
// ---------------------------------------------------------------

/// MC `ArmorSlot.mayPlace` - each armor slot accepts only items
/// whose `armor_props` slot index matches (HEAD..FEET = 0..3).
fn accept_helmet(id: u16) -> bool {
    matches!(crate::item::armor_props(id), Some((0, ..)))
}
fn accept_chestplate(id: u16) -> bool {
    matches!(crate::item::armor_props(id), Some((1, ..)))
}
fn accept_leggings(id: u16) -> bool {
    matches!(crate::item::armor_props(id), Some((2, ..)))
}
fn accept_boots(id: u16) -> bool {
    matches!(crate::item::armor_props(id), Some((3, ..)))
}

/// Armor section accept table (parallel to the 4 armor slots).
pub(crate) static ARMOR_ACCEPT: [Option<crate::container::click::SlotAccept>;
    ARMOR_SLOTS] = [
    Some(accept_helmet),
    Some(accept_chestplate),
    Some(accept_leggings),
    Some(accept_boots),
];

/// Armor per-slot max-stack caps: `ArmorSlot.getMaxStackSize() = 1`.
pub(crate) static ARMOR_CAPS: [u8; ARMOR_SLOTS] = [1; ARMOR_SLOTS];

/// Section base offsets in the E-screen gi space (MC InventoryMenu
/// slot order with the virtual result slot dropped: every MC index
/// shifts down by one).
pub(crate) const GI_CRAFT: usize = 0;
pub(crate) const GI_ARMOR: usize = GI_CRAFT + 4;
pub(crate) const GI_MAIN: usize = GI_ARMOR + ARMOR_SLOTS;
pub(crate) const GI_HOTBAR: usize = GI_MAIN + (INVENTORY_SLOTS - HOTBAR_SLOTS);
pub(crate) const GI_END: usize = GI_HOTBAR + HOTBAR_SLOTS;

/// Global index of a `SlotKind` on the E screen. The virtual craft
/// OUTPUT is not addressable (`None`).
pub(crate) fn inv_screen_gi(slot: SlotKind) -> Option<usize> {
    match slot {
        SlotKind::Craft(i) if i < 4 => Some(GI_CRAFT + i),
        SlotKind::Armor(i) if i < ARMOR_SLOTS => Some(GI_ARMOR + i),
        SlotKind::Inv(i) if i < HOTBAR_SLOTS => Some(GI_HOTBAR + i),
        SlotKind::Inv(i) if i < INVENTORY_SLOTS => {
            Some(GI_MAIN + (i - HOTBAR_SLOTS))
        }
        _ => None,
    }
}

/// The E screen as click-core sections in MC menu order:
/// [craft 2x2, armor, main rows, hotbar]. The armor section
/// carries the type filters + the 1-item caps.
pub(crate) fn inv_screen_sections<'a>(
    craft: &'a mut [Option<ItemStack>],
    armor: &'a mut [Option<ItemStack>],
    inv: &'a mut [Option<ItemStack>],
) -> [SectionMut<'a>; 4] {
    let (main_sec, hotbar_sec) =
        crate::container::screen::player_sections(inv);
    [
        SectionMut::open(craft),
        SectionMut {
            slots: armor,
            accept: &ARMOR_ACCEPT,
            caps: &ARMOR_CAPS,
        },
        main_sec,
        hotbar_sec,
    ]
}

/// `InventoryMenu.quickMoveStack` destination routing for a
/// NON-result source at `gi` holding `item_id`. Armor auto-equip:
/// an armor item shift-clicked from the main/hotbar rows targets
/// its EMPTY armor slot first (source line 123); craft + armor
/// sources spread over main-then-hotbar forward; main <-> hotbar
/// otherwise. `armor` is the live armor section (for the
/// empty-slot check).
fn inv_screen_quick_order(
    gi: usize,
    item_id: u16,
    armor: &[Option<ItemStack>],
) -> Vec<usize> {
    if gi < GI_MAIN {
        // Craft cells + armor slots -> whole player inv forward
        // (`moveItemStackTo(9, 45, false)`).
        return (GI_MAIN..GI_END).collect();
    }
    if let Some((slot, _, _)) = crate::item::armor_props(item_id)
        && armor.get(slot).is_none_or(|s| s.is_none())
    {
        // Armor auto-equip: the single matching armor slot.
        return vec![GI_ARMOR + slot];
    }
    if gi < GI_HOTBAR {
        // Main rows -> hotbar forward.
        (GI_HOTBAR..GI_END).collect()
    } else {
        // Hotbar -> main rows forward.
        (GI_MAIN..GI_HOTBAR).collect()
    }
}

/// Craft-once helper: resolve the 2x2 recipe, hand the packet to
/// `sink`; when `sink` reports the packet was fully placed
/// (returns true), consume one item from every craft cell.
fn craft_once(
    craft: &mut [Option<ItemStack>],
    sink: impl FnOnce(ItemStack) -> bool,
) {
    let grid: [Option<ItemStack>; 4] =
        std::array::from_fn(|i| craft.get(i).cloned().flatten());
    let Some((out_id, out_n)) = match_recipe(&grid) else {
        return;
    };
    if !sink(ItemStack::new(out_id.0, out_n)) {
        return;
    }
    for cell in craft.iter_mut() {
        if let Some(st) = cell.as_mut() {
            st.count = st.count.saturating_sub(1);
            if st.count == 0 {
                *cell = None;
            }
        }
    }
}

/// Apply one inventory-screen op (the `net::InvOpMsg` wire shape)
/// against the live screen + cursor through the shared click core.
/// Used identically by the local panel (host / offline / client
/// optimistic apply) and the server-side applier, so the two can
/// never drift. Returns stacks to spawn as thrown drops.
///
/// `creative` gates the CLONE arm.
pub(crate) fn inv_screen_apply_op(
    craft: &mut [Option<ItemStack>],
    armor: &mut [Option<ItemStack>],
    inv: &mut [Option<ItemStack>],
    cursor: &mut Option<ItemStack>,
    op: crate::net::InvOpMsg,
    creative: bool,
) -> Vec<ItemStack> {
    use crate::net::InvOpMsg as Op;
    let mut thrown: Vec<ItemStack> = Vec::new();
    // The virtual result slot: ops addressed at CraftOut are
    // resolved before the section walk (MC's ResultSlot is real;
    // ours is computed - the observable rules match).
    let result_op = |slot: NetSlot| matches!(slot, NetSlot::CraftOut);
    match op {
        Op::Click { slot, secondary } if result_op(slot) => {
            // PICKUP on the result: craft once INTO the cursor
            // (empty cursor or a same-stackable cursor with room
            // for the full packet). LMB and RMB behave the same on
            // a take-only slot.
            let _ = secondary;
            craft_once(craft, |packet| match cursor.as_mut() {
                None => {
                    *cursor = Some(packet);
                    true
                }
                Some(cur)
                    if cur.same_stackable(&packet)
                        && (cur.count as u16 + packet.count as u16)
                            <= crate::item::item_max_stack(cur.id) as u16 =>
                {
                    cur.count += packet.count;
                    true
                }
                Some(_) => false,
            });
        }
        Op::QuickMove { slot } if result_op(slot) => {
            // Result shift-click: `moveItemStackTo(9, 45, true)` -
            // player inv BACKWARDS (hotbar-end first); a remainder
            // the inventory can't hold is dropped (`player.drop`).
            // Single application (the source's refill loop
            // collapses - W470 note).
            let mut sections = inv_screen_sections(craft, armor, inv);
            let order: Vec<usize> = (GI_MAIN..GI_END).rev().collect();
            let mut leftover: Option<ItemStack> = None;
            // Split borrows: craft is inside `sections`, so resolve
            // the recipe from the section view.
            let grid: [Option<ItemStack>; 4] = std::array::from_fn(|i| {
                click::slot_ref(&sections, GI_CRAFT + i).cloned()
            });
            if let Some((out_id, out_n)) = match_recipe(&grid) {
                let mut packet = ItemStack::new(out_id.0, out_n);
                let moved =
                    click::move_stack_to_order(&mut sections, &mut packet, &order);
                if moved || packet.count == 0 {
                    if packet.count > 0 {
                        leftover = Some(packet);
                    }
                    for i in 0..4 {
                        if let Some(cell) =
                            click::slot_mut(&mut sections, GI_CRAFT + i)
                            && let Some(st) = cell.as_mut()
                        {
                            st.count = st.count.saturating_sub(1);
                            if st.count == 0 {
                                *cell = None;
                            }
                        }
                    }
                }
            }
            thrown.extend(leftover);
        }
        Op::Swap { slot, hotbar } if result_op(slot) => {
            // SWAP on the result with an EMPTY hotbar slot: craft
            // once straight into it (MC's hotbar-empty branch; the
            // occupied branches are blocked by mayPlace=false).
            if (hotbar as usize) < HOTBAR_SLOTS
                && inv
                    .get(hotbar as usize)
                    .is_none_or(|s| s.is_none())
            {
                craft_once(craft, |packet| {
                    if let Some(s) = inv.get_mut(hotbar as usize) {
                        *s = Some(packet);
                        return true;
                    }
                    false
                });
            }
        }
        Op::Throw { slot, .. } if result_op(slot) => {
            // THROW on the result: craft one packet and toss it
            // (single application of the source's Ctrl+Q loop).
            if cursor.is_none() {
                craft_once(craft, |packet| {
                    thrown.push(packet);
                    true
                });
            }
        }
        Op::Collect { slot } if result_op(slot) => {
            // PICKUP_ALL guard requires the clicked slot to hold
            // nothing pickable; a matching result slot blocks the
            // collect entirely (and InventoryMenu's
            // canTakeItemForPickAll skips the result container).
        }
        Op::CloneSlot { slot } if result_op(slot) => {
            let grid: [Option<ItemStack>; 4] =
                std::array::from_fn(|i| craft.get(i).cloned().flatten());
            if creative
                && cursor.is_none()
                && let Some((out_id, _)) = match_recipe(&grid)
            {
                *cursor = Some(ItemStack::new(
                    out_id.0,
                    crate::item::item_max_stack(out_id.0),
                ));
            }
        }
        Op::Click { slot, secondary } => {
            if let Some(gi) = inv_screen_gi(slot.into()) {
                let mut sections = inv_screen_sections(craft, armor, inv);
                click::pickup_click(&mut sections, cursor, gi, secondary);
            }
        }
        Op::Collect { slot } => {
            if let Some(gi) = inv_screen_gi(slot.into()) {
                let mut sections = inv_screen_sections(craft, armor, inv);
                click::pickup_all(&mut sections, cursor, gi, false);
            }
        }
        Op::QuickMove { slot } => {
            if let Some(gi) = inv_screen_gi(slot.into()) {
                let id = {
                    let sections = inv_screen_sections(craft, armor, inv);
                    click::slot_ref(&sections, gi).map(|st| st.id)
                };
                if let Some(id) = id {
                    let order = inv_screen_quick_order(gi, id, armor);
                    let mut sections =
                        inv_screen_sections(craft, armor, inv);
                    click::quick_move(&mut sections, gi, &order);
                }
            }
        }
        Op::Swap { slot, hotbar } => {
            if let Some(gi) = inv_screen_gi(slot.into())
                && (hotbar as usize) < HOTBAR_SLOTS
            {
                // `Inventory.add` order for a displaced overflow
                // stack: hotbar array order, then main rows.
                let add_order: Vec<usize> =
                    (GI_HOTBAR..GI_END).chain(GI_MAIN..GI_HOTBAR).collect();
                let mut sections = inv_screen_sections(craft, armor, inv);
                thrown.extend(click::swap_click(
                    &mut sections,
                    gi,
                    GI_HOTBAR + hotbar as usize,
                    &add_order,
                ));
            }
        }
        Op::Throw { slot, all } => {
            if let Some(gi) = inv_screen_gi(slot.into()) {
                let mut sections = inv_screen_sections(craft, armor, inv);
                thrown.extend(click::throw_from_slot(
                    &mut sections,
                    cursor,
                    gi,
                    all,
                ));
            }
        }
        Op::ThrowSelected { slot, all } => {
            if (slot as usize) < HOTBAR_SLOTS {
                let gi = GI_HOTBAR + slot as usize;
                let none_cursor: Option<ItemStack> = None;
                let mut sections = inv_screen_sections(craft, armor, inv);
                thrown.extend(click::throw_from_slot(
                    &mut sections,
                    &none_cursor,
                    gi,
                    all,
                ));
            }
        }
        Op::CloneSlot { slot } => {
            if let Some(gi) = inv_screen_gi(slot.into()) {
                let sections = inv_screen_sections(craft, armor, inv);
                click::clone_click(&sections, cursor, gi, creative);
            }
        }
        Op::CursorThrow { all } => {
            thrown.extend(click::throw_from_cursor(cursor, all));
        }
        Op::CloseScreen => {
            // Cursor recovery is the caller's job (it owns the
            // spill position / snapshot plumbing).
        }
    }
    thrown
}

/// Apply a completed E-screen drag-paint (QUICK_CRAFT release).
/// `visited` lists the visited cells in order; the virtual result
/// cell can never be visited (take-only).
pub(crate) fn inv_screen_apply_drag(
    craft: &mut [Option<ItemStack>],
    armor: &mut [Option<ItemStack>],
    inv: &mut [Option<ItemStack>],
    cursor: &mut Option<ItemStack>,
    visited: &[SlotKind],
    one_per_slot: bool,
) {
    let mut gis: Vec<usize> = Vec::with_capacity(visited.len());
    for &k in visited {
        if let Some(gi) = inv_screen_gi(k)
            && !gis.contains(&gi)
        {
            gis.push(gi);
        }
    }
    let mut sections = inv_screen_sections(craft, armor, inv);
    click::quick_craft_apply(&mut sections, cursor, &gis, one_per_slot);
}

// ---------------------------------------------------------------
// Pickup / consume helpers (unchanged by W472).
// ---------------------------------------------------------------

/// Pop one of `block` into a stack-typed inventory. First tries to
/// top up an existing non-full PLAIN stack, then claims the first
/// empty slot. Returns `true` when the block was successfully
/// stored (false means the inventory was completely full + the
/// drop was lost). W468: stack-typed - the tuple inventory shape
/// is gone (`ClientInv` holds `Option<ItemStack>` slots now).
pub(crate) fn inv_pickup(
    inv: &mut [Option<crate::item::ItemStack>; INVENTORY_SLOTS],
    block: BlockId,
) -> bool {
    inv_pickup_n_stacks(inv, block, 1)
}

/// Pop `count` of `block` into the player's `ItemStack`-typed
/// inventory. Tops up partial stacks first, then claims empty
/// slots. Returns `true` if the full count fit; `false` (and some
/// items already merged) when the inventory ran out of room before
/// `count` was exhausted.
///
/// W50.3: used by the crafting-table close handler to dump the
/// 3x3 grid contents back into the player inv when the panel is
/// closed via the X button.
///
/// W466: replaces the tuple-typed `inv_pickup_n` (the player
/// inventory holds stacks now). One components rule: merges only
/// land in PLAIN (default-component) stacks so a returned
/// craft-grid stack never piles onto a worn tool. Freshly claimed
/// slots mint stacks with empty components.
pub(crate) fn inv_pickup_n_stacks(
    inv: &mut [Option<crate::item::ItemStack>; INVENTORY_SLOTS],
    block: BlockId,
    count: u8,
) -> bool {
    if block == BlockId::AIR || count == 0 {
        return true;
    }
    let cap = crate::item::item_max_stack(block.0);
    let mut remaining = count;
    // Pass 1: top up matching plain partial stacks.
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return true;
        }
        if let Some(st) = slot.as_mut()
            && st.id == block.0
            && st.components.is_empty()
            && st.count < cap
        {
            let room = cap - st.count;
            let take = room.min(remaining);
            st.count += take;
            remaining -= take;
        }
    }
    // Pass 2: claim empty slots.
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return true;
        }
        if slot.is_none() {
            let take = cap.min(remaining);
            *slot = Some(crate::item::ItemStack::new(block.0, take));
            remaining -= take;
        }
    }
    remaining == 0
}

/// Decrement one item of `block` somewhere in the inventory.
/// Returns `true` if a stack matched + was decremented (slot is
/// cleared when the count hits zero); `false` means the inventory
/// didn't have the block. Components are irrelevant for a place
/// consume - the first id match pays, like the tuple-era helper.
pub(crate) fn inv_consume(
    inv: &mut [Option<crate::item::ItemStack>; INVENTORY_SLOTS],
    block: BlockId,
) -> bool {
    for slot in inv.iter_mut() {
        if let Some(st) = slot.as_mut()
            && st.id == block.0
        {
            st.count = st.count.saturating_sub(1);
            if st.count == 0 {
                *slot = None;
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemStack;
    use crate::net::InvOpMsg;

    type Slots<const N: usize> = [Option<ItemStack>; N];

    fn screen() -> (Slots<4>, Slots<ARMOR_SLOTS>, Slots<INVENTORY_SLOTS>, Option<ItemStack>) {
        (
            std::array::from_fn(|_| None),
            std::array::from_fn(|_| None),
            std::array::from_fn(|_| None),
            None,
        )
    }

    #[test]
    fn e_screen_gi_matches_menu_order() {
        // MC InventoryMenu (result dropped): craft 0..4, armor
        // 4..8, main 8..35, hotbar 35..44.
        assert_eq!(inv_screen_gi(SlotKind::Craft(0)), Some(0));
        assert_eq!(inv_screen_gi(SlotKind::Armor(0)), Some(4));
        assert_eq!(inv_screen_gi(SlotKind::Armor(3)), Some(7));
        assert_eq!(inv_screen_gi(SlotKind::Inv(9)), Some(8));
        assert_eq!(inv_screen_gi(SlotKind::Inv(35)), Some(34));
        assert_eq!(inv_screen_gi(SlotKind::Inv(0)), Some(35));
        assert_eq!(inv_screen_gi(SlotKind::Inv(8)), Some(43));
        assert_eq!(inv_screen_gi(SlotKind::CraftOut), None);
        assert_eq!(GI_END, 44);
    }

    #[test]
    fn e_screen_click_picks_up_and_places() {
        let (mut craft, mut armor, mut inv, mut cursor) = screen();
        inv[0] = Some(ItemStack::new(1, 10));
        // LMB on hotbar slot 0: whole stack to cursor.
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::Click { slot: NetSlot::Inv(0), secondary: false },
            false,
        );
        assert_eq!(cursor, Some(ItemStack::new(1, 10)));
        assert!(inv[0].is_none());
        // RMB on storage slot 9: place one.
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::Click { slot: NetSlot::Inv(9), secondary: true },
            false,
        );
        assert_eq!(inv[9], Some(ItemStack::new(1, 1)));
        assert_eq!(cursor, Some(ItemStack::new(1, 9)));
    }

    #[test]
    fn armor_slot_rejects_non_matching_and_caps_at_one() {
        let (mut craft, mut armor, mut inv, _) = screen();
        // Stone on the cursor cannot land in the helmet slot.
        let mut cursor = Some(ItemStack::new(1, 5));
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::Click { slot: NetSlot::Armor(0), secondary: false },
            false,
        );
        assert!(armor[0].is_none());
        assert_eq!(cursor, Some(ItemStack::new(1, 5)));
        // Iron helmet (887) lands; boots slot refuses it.
        cursor = Some(ItemStack::new(887, 1));
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::Click { slot: NetSlot::Armor(3), secondary: false },
            false,
        );
        assert!(armor[3].is_none(), "boots slot must refuse a helmet");
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::Click { slot: NetSlot::Armor(0), secondary: false },
            false,
        );
        assert_eq!(armor[0], Some(ItemStack::new(887, 1)));
        assert!(cursor.is_none());
    }

    #[test]
    fn shift_click_armor_item_auto_equips() {
        // MC InventoryMenu.quickMoveStack line 123: an armor item
        // in the main rows shift-clicks into its EMPTY armor slot.
        let (mut craft, mut armor, mut inv, mut cursor) = screen();
        inv[10] = Some(ItemStack::new(890, 1)); // iron boots
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::QuickMove { slot: NetSlot::Inv(10) },
            false,
        );
        assert_eq!(armor[3], Some(ItemStack::new(890, 1)));
        assert!(inv[10].is_none());
        // Occupied armor slot: the next boots fall through to the
        // main->hotbar route instead.
        inv[10] = Some(ItemStack::new(885, 1)); // leather boots
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::QuickMove { slot: NetSlot::Inv(10) },
            false,
        );
        assert_eq!(armor[3], Some(ItemStack::new(890, 1)));
        assert_eq!(inv[0], Some(ItemStack::new(885, 1)), "hotbar slot 0");
    }

    #[test]
    fn shift_click_armor_slot_unequips_to_main_first() {
        // Armor sources route `moveItemStackTo(9, 45, false)` -
        // main rows before the hotbar.
        let (mut craft, mut armor, mut inv, mut cursor) = screen();
        armor[1] = Some(ItemStack::new(888, 1)); // iron chestplate
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::QuickMove { slot: NetSlot::Armor(1) },
            false,
        );
        assert!(armor[1].is_none());
        assert_eq!(inv[9], Some(ItemStack::new(888, 1)), "first MAIN slot");
    }

    #[test]
    fn result_quick_move_lands_hotbar_end_first() {
        // `moveItemStackTo(9, 45, true)`: the reversed player
        // range starts at hotbar slot 8 (gi 43).
        let (mut craft, mut armor, mut inv, mut cursor) = screen();
        // 2x2 recipe: 1 wood (5) -> 4 planks (7) per the RECIPES
        // table; resolve dynamically so table edits don't break
        // the assertion.
        craft[0] = Some(ItemStack::new(5, 2));
        let grid: [Option<ItemStack>; 4] =
            std::array::from_fn(|i| craft[i].clone());
        let (out, n) =
            crate::match_recipe(&grid).expect("wood->planks recipe");
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::QuickMove { slot: NetSlot::CraftOut },
            false,
        );
        assert_eq!(
            inv[8],
            Some(ItemStack::new(out.0, n)),
            "hotbar slot 8 = end of the reversed range"
        );
        assert_eq!(
            craft[0].as_ref().map(|s| s.count),
            Some(1),
            "one input consumed"
        );
    }

    #[test]
    fn pickup_all_walks_menu_order_main_before_hotbar() {
        // Collect with a partial cursor: pass 0 hoovers partial
        // stacks in gi order - main rows come BEFORE the hotbar,
        // so with cap 64 the main stack drains first.
        let (mut craft, mut armor, mut inv, _) = screen();
        inv[0] = Some(ItemStack::new(1, 30)); // hotbar (gi 35)
        inv[9] = Some(ItemStack::new(1, 30)); // main (gi 8)
        let mut cursor = Some(ItemStack::new(1, 10));
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::Collect { slot: NetSlot::Inv(11) },
            false,
        );
        assert_eq!(cursor.as_ref().map(|c| c.count), Some(64));
        assert!(inv[9].is_none(), "main stack drained first (gi order)");
        assert_eq!(
            inv[0].as_ref().map(|s| s.count),
            Some(6),
            "hotbar stack keeps the remainder"
        );
    }

    #[test]
    fn swap_key_equips_armor_from_hotbar() {
        // Number key over the helmet slot with a helmet in the
        // hotbar swaps it in (mayPlace honoured).
        let (mut craft, mut armor, mut inv, mut cursor) = screen();
        inv[2] = Some(ItemStack::new(887, 1));
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::Swap { slot: NetSlot::Armor(0), hotbar: 2 },
            false,
        );
        assert_eq!(armor[0], Some(ItemStack::new(887, 1)));
        assert!(inv[2].is_none());
        // Swap back: armor -> hotbar (raw setItem side).
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::Swap { slot: NetSlot::Armor(0), hotbar: 2 },
            false,
        );
        assert!(armor[0].is_none());
        assert_eq!(inv[2], Some(ItemStack::new(887, 1)));
    }

    #[test]
    fn clone_op_is_creative_gated() {
        let (mut craft, mut armor, mut inv, mut cursor) = screen();
        inv[3] = Some(ItemStack::new(7, 2));
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::CloneSlot { slot: NetSlot::Inv(3) },
            false,
        );
        assert!(cursor.is_none(), "survival clone refused");
        inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::CloneSlot { slot: NetSlot::Inv(3) },
            true,
        );
        assert_eq!(
            cursor,
            Some(ItemStack::new(7, crate::item::item_max_stack(7)))
        );
    }

    #[test]
    fn cursor_throw_and_drag_paint() {
        let (mut craft, mut armor, mut inv, _) = screen();
        let mut cursor = Some(ItemStack::new(1, 8));
        // RMB drag-paint over 2 empty craft cells + 2 inv slots.
        inv_screen_apply_drag(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            &[
                SlotKind::Craft(0),
                SlotKind::Craft(1),
                SlotKind::Inv(9),
                SlotKind::Inv(10),
            ],
            true,
        );
        assert_eq!(craft[0], Some(ItemStack::new(1, 1)));
        assert_eq!(inv[10], Some(ItemStack::new(1, 1)));
        assert_eq!(cursor.as_ref().map(|c| c.count), Some(4));
        // Click-outside RMB: throw one off the cursor.
        let thrown = inv_screen_apply_op(
            &mut craft,
            &mut armor,
            &mut inv,
            &mut cursor,
            InvOpMsg::CursorThrow { all: false },
            false,
        );
        assert_eq!(thrown, vec![ItemStack::new(1, 1)]);
        assert_eq!(cursor.as_ref().map(|c| c.count), Some(3));
    }

    #[test]
    fn slot_0_pickup_lands_normally() {
        // inv_pickup walks slot 0 like any other - an empty
        // inventory should land the first picked item in slot 0.
        let mut inv: [Option<ItemStack>; INVENTORY_SLOTS] =
            std::array::from_fn(|_| None);
        assert!(inv_pickup(&mut inv, BlockId(2)));
        assert_eq!(inv[0], Some(ItemStack::new(2, 1)));
    }
}
