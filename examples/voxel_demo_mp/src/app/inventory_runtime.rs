//! W47.28 - inventory_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {

    /// W5.2b/W466: register one use of the currently-held
    /// flint+steel after a successful ignition (portal frame OR
    /// fire block). Damage counts UP on the stack's components;
    /// when it reaches `FLINT_AND_STEEL_MAX_USES` the tool burns
    /// out and the slot empties (same 64-use budget as before the
    /// ItemStack fold - a fresh tool has damage 0).
    ///
    /// Idempotent across non-flint+steel held items - a Cobble in
    /// hand never triggered ignition in the first place, so the
    /// fn only fires when the caller already verified `held.id ==
    /// FLINT_AND_STEEL_ID`.
    pub(crate) fn consume_flint_and_steel_use(&mut self) {
        let slot_idx = self.selected_slot;
        let Some(stack) = self.inventory[slot_idx].as_mut() else {
            return;
        };
        let damage = stack.components.damage.saturating_add(1);
        if damage >= block::FLINT_AND_STEEL_MAX_USES {
            // Burn out: clear the slot.
            self.inventory[slot_idx] = None;
        } else {
            stack.components.damage = damage;
        }
    }

    /// W175/W466: register one use of the held tool (a block mined /
    /// a mob hit). Damage counts UP from 0 on the stack's
    /// components; at the tool's max the tool breaks and the slot
    /// clears. No-op when the held item isn't a tool (or is
    /// unbreakable). Generalises `consume_flint_and_steel_use` to
    /// the whole tool/weapon set via `item::tool_max_durability`.
    pub(crate) fn consume_tool_durability(&mut self) {
        let slot_idx = self.selected_slot;
        let Some(stack) = self.inventory[slot_idx].as_mut() else {
            return;
        };
        let Some(max) = crate::item::tool_max_durability(stack.id) else {
            return;
        };
        let damage = stack.components.damage.saturating_add(1);
        if damage >= max {
            self.inventory[slot_idx] = None;
        } else {
            stack.components.damage = damage;
        }
    }

    /// Register a successful pickup of `n` items for the floating
    /// "+N wood" toast HUD. Collapses adjacent picks of the same
    /// block into a single entry so spamming the same stack doesn't
    /// bury the HUD in a wall of one-liners. W468: takes the whole
    /// landed count in one call (stack pickups arrive as one event).
    pub(crate) fn note_pickup(&mut self, block: BlockId, n: u8) {
        if n == 0 {
            return;
        }
        let now = std::time::Instant::now();
        // W123: MC pickup pop (entity.item.pickup), rate-limited so a
        // mass vacuum doesn't machine-gun; variants shuffle per fire.
        let pop_ok = self
            .last_pickup_sfx
            .map(|t| now.duration_since(t).as_secs_f32() >= 0.09)
            .unwrap_or(true);
        if pop_ok {
            self.last_pickup_sfx = Some(now);
            let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
            self.sfx_at("item/pickup", pos, |_, _| {});
        }
        if let Some(last) = self.pickup_toasts.last_mut()
            && last.1 == block
            && now.duration_since(last.0).as_secs_f32() < 1.5
        {
            last.0 = now;
            last.2 = last.2.saturating_add(n);
            return;
        }
        self.pickup_toasts.push((now, block, n));
        // Cap the buffer so a long mining session doesn't grow
        // unbounded. Oldest entries are the first to roll off the
        // top of the HUD anyway.
        if self.pickup_toasts.len() > 16 {
            let drop = self.pickup_toasts.len() - 16;
            self.pickup_toasts.drain(0..drop);
        }
    }

    /// Read the stack currently sitting in `kind`. The crafting
    /// output is virtual: it's whatever recipe matches the current
    /// 2x2 grid (or `None` if no match).
    ///
    /// W469: inventory AND craft cells carry full `ItemStack`s -
    /// no tuple lift left. The virtual craft output is always a
    /// fresh stack (crafted results start with empty components).
    pub(crate) fn slot_read(&self, kind: SlotKind) -> Option<crate::item::ItemStack> {
        match kind {
            SlotKind::Inv(i) => self.inventory.get(i).cloned().flatten(),
            SlotKind::Craft(i) => self.craft_grid.get(i).cloned().flatten(),
            SlotKind::CraftOut => match_recipe(&self.craft_grid)
                .map(|(b, c)| crate::item::ItemStack::new(b.0, c)),
        }
    }

    /// Write `value` into `kind`. Never writes to `CraftOut`
    /// (that's a virtual cell - taking from it goes through
    /// `consume_craft` instead). W469: craft cells store the full
    /// stack - components survive a park in the grid.
    pub(crate) fn slot_write(&mut self, kind: SlotKind, value: Option<crate::item::ItemStack>) {
        match kind {
            SlotKind::Inv(i) => {
                if i < self.inventory.len() {
                    self.inventory[i] = value;
                }
            }
            SlotKind::Craft(i) => {
                if i < self.craft_grid.len() {
                    self.craft_grid[i] = value;
                }
            }
            SlotKind::CraftOut => {}
        }
    }

    /// Drain one item from every populated craft cell. Called when
    /// the player takes the craft output. Components stay on the
    /// remainder (a worn ingredient consumed to 0 just vanishes).
    pub(crate) fn consume_craft(&mut self) {
        for slot in self.craft_grid.iter_mut() {
            if let Some(st) = slot.as_mut() {
                st.count = st.count.saturating_sub(1);
                if st.count == 0 {
                    *slot = None;
                }
            }
        }
    }

    /// Dispatch one inventory-UI action. Drag&drop semantics: a
    /// drag from `from` to `to` either swaps the two stacks or
    /// (when the blocks match) merges into the destination.
    /// Hotbar slot 0 is the break-tool slot and refuses incoming
    /// stacks so its X icon never disappears.
    pub(crate) fn apply_inv_action(&mut self, action: InvAction) {
        match action {
            InvAction::SelectHotbar(idx) => {
                if idx < HOTBAR_SLOTS {
                    self.selected_slot = idx;
                }
            }
            InvAction::Drag { from, to, split } => {
                if from == to {
                    return;
                }
                // Can't drop INTO the crafting output - it's a
                // virtual cell.
                if matches!(to, SlotKind::CraftOut) {
                    return;
                }
                // Protect the break-tool slot.
                if matches!(to, SlotKind::Inv(0)) {
                    return;
                }
                // W466: stacks carry their components (damage,
                // ...) with them, so a drag needs no parallel
                // bookkeeping - moves, swaps and splits below all
                // clone the source components; merges only fire
                // when `same_stackable` says the components match.
                let Some(from_stack) = self.slot_read(from) else {
                    return;
                };
                let to_stack = self.slot_read(to);
                let craft_take = matches!(from, SlotKind::CraftOut);
                // CraftOut drag has special rules: must land in an
                // empty slot or merge into a same-block stack with
                // room for the full output. Splitting is a no-op
                // here - the output is a single fixed packet.
                if craft_take {
                    match to_stack {
                        None => {
                            self.consume_craft();
                            self.slot_write(to, Some(from_stack));
                        }
                        Some(ts)
                            if ts.same_stackable(&from_stack)
                                && (ts.count as u16 + from_stack.count as u16)
                                    <= STACK_MAX as u16 =>
                        {
                            self.consume_craft();
                            let mut merged = ts;
                            merged.count += from_stack.count;
                            self.slot_write(to, Some(merged));
                        }
                        _ => {}
                    }
                    return;
                }
                // W468: the split/merge/swap rules live in the
                // shared `drag_stacks` helper - the server-side
                // `inv_apply_drag` routes through the SAME function
                // so local prediction and authority never drift.
                if let Some((new_from, new_to)) =
                    crate::inventory::drag_stacks(from_stack, to_stack, split)
                {
                    self.slot_write(to, new_to);
                    self.slot_write(from, new_from);
                }
            }
        }
    }
}
