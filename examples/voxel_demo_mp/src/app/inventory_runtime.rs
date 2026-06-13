//! W47.28 - inventory_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {

    /// W5.2b: decrement the currently-held flint+steel's durability
    /// after a successful ignition (portal frame OR fire block).
    /// When the counter hits 0, empty the slot + drop the parallel
    /// map entry so the next interaction sees no held tool.
    ///
    /// Idempotent across non-flint+steel held items - a Cobble in
    /// hand never triggered ignition in the first place, so the
    /// fn only fires when the caller already verified `held.0 ==
    /// FLINT_AND_STEEL_ID`.
    pub(crate) fn consume_flint_and_steel_use(&mut self) {
        let slot_idx = self.selected_slot;
        let remaining = self
            .item_durability
            .get(&slot_idx)
            .copied()
            .unwrap_or(block::FLINT_AND_STEEL_MAX_USES);
        let new = remaining.saturating_sub(1);
        if new == 0 {
            // Burn out: clear both slot + durability tracker.
            self.inventory[slot_idx] = None;
            self.item_durability.remove(&slot_idx);
        } else {
            self.item_durability.insert(slot_idx, new);
        }
    }

    /// W175: decrement the held tool's durability after a use (a
    /// block mined / a mob hit). Inits to the tool's max on first use;
    /// at 0 the tool breaks - the slot clears + the parallel tracker
    /// entry drops. No-op when the held item isn't a tool (or is
    /// unbreakable). Generalises `consume_flint_and_steel_use` to the
    /// whole tool/weapon set via `item::tool_max_durability`.
    pub(crate) fn consume_tool_durability(&mut self) {
        let slot_idx = self.selected_slot;
        let Some((held, _)) = self.inventory[slot_idx] else {
            return;
        };
        let Some(max) = crate::item::tool_max_durability(held.0) else {
            return;
        };
        let remaining = self.item_durability.get(&slot_idx).copied().unwrap_or(max);
        let new = remaining.saturating_sub(1);
        if new == 0 {
            self.inventory[slot_idx] = None;
            self.item_durability.remove(&slot_idx);
        } else {
            self.item_durability.insert(slot_idx, new);
        }
    }

    /// Register a successful pickup for the floating "+1 wood"
    /// toast HUD. Collapses adjacent picks of the same block into
    /// a single entry so spamming the same stack doesn't bury the
    /// HUD in a wall of one-liners.
    pub(crate) fn note_pickup(&mut self, block: BlockId) {
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
            last.2 = last.2.saturating_add(1);
            return;
        }
        self.pickup_toasts.push((now, block, 1));
        // Cap the buffer so a long mining session doesn't grow
        // unbounded. Oldest entries are the first to roll off the
        // top of the HUD anyway.
        if self.pickup_toasts.len() > 16 {
            let drop = self.pickup_toasts.len() - 16;
            self.pickup_toasts.drain(0..drop);
        }
    }

    /// Read the (BlockId, count) currently sitting in `kind`. The
    /// crafting output is virtual: it's whatever recipe matches the
    /// current 2x2 grid (or `None` if no match).
    pub(crate) fn slot_read(&self, kind: SlotKind) -> Option<(BlockId, u8)> {
        match kind {
            SlotKind::Inv(i) => self.inventory.get(i).copied().flatten(),
            SlotKind::Craft(i) => self.craft_grid.get(i).copied().flatten(),
            SlotKind::CraftOut => match_recipe(&self.craft_grid),
        }
    }

    /// Write `value` into `kind`. Never writes to `CraftOut`
    /// (that's a virtual cell - taking from it goes through
    /// `consume_craft` instead).
    pub(crate) fn slot_write(&mut self, kind: SlotKind, value: Option<(BlockId, u8)>) {
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
    /// the player takes the craft output.
    pub(crate) fn consume_craft(&mut self) {
        for slot in self.craft_grid.iter_mut() {
            if let Some((_, c)) = slot.as_mut() {
                *c = c.saturating_sub(1);
                if *c == 0 {
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
                // W5.2b: snapshot the durability values BEFORE the
                // mutation so we can move them with the stacks.
                // Drag is the only place a flint+steel slot can
                // travel; record the from/to slot durabilities,
                // then patch them after the mutation finishes.
                let from_idx_dura = match from {
                    SlotKind::Inv(i) => Some(i),
                    _ => None,
                };
                let to_idx_dura = match to {
                    SlotKind::Inv(i) => Some(i),
                    _ => None,
                };
                let from_dura =
                    from_idx_dura.and_then(|i| self.item_durability.get(&i).copied());
                let to_dura =
                    to_idx_dura.and_then(|i| self.item_durability.get(&i).copied());
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
                        Some((tb, tc))
                            if tb == from_stack.0
                                && (tc as u16 + from_stack.1 as u16) <= STACK_MAX as u16 =>
                        {
                            self.consume_craft();
                            self.slot_write(to, Some((tb, tc + from_stack.1)));
                        }
                        _ => {}
                    }
                    return;
                }
                // Partial move (`SplitMode::Half` or `One`).
                // `All` falls through to the whole-stack swap path.
                let take = match split {
                    SplitMode::All => 0u8,
                    SplitMode::Half => (from_stack.1 as u16).div_ceil(2) as u8,
                    SplitMode::One => 1u8,
                };
                if take > 0 {
                    let take = take.max(1).min(from_stack.1);
                    let rem = from_stack.1 - take;
                    match to_stack {
                        Some((tb, tc)) if tb == from_stack.0 => {
                            let merge = (STACK_MAX - tc).min(take);
                            let new_to = tc + merge;
                            let leftover = take - merge;
                            self.slot_write(to, Some((tb, new_to)));
                            // Any leftover from the take goes back
                            // into the source slot.
                            let back = rem + leftover;
                            self.slot_write(
                                from,
                                if back == 0 {
                                    None
                                } else {
                                    Some((from_stack.0, back))
                                },
                            );
                        }
                        None => {
                            self.slot_write(to, Some((from_stack.0, take)));
                            self.slot_write(
                                from,
                                if rem == 0 {
                                    None
                                } else {
                                    Some((from_stack.0, rem))
                                },
                            );
                        }
                        Some(_) => {
                            // Target occupied by a different block:
                            // a split into an unrelated slot would
                            // either swap (loses semantics) or
                            // refuse. We refuse.
                        }
                    }
                    return;
                }
                // Standard whole-stack drag between two real slots.
                match to_stack {
                    Some((tb, tc)) if tb == from_stack.0 => {
                        // Same block -> merge.
                        let take = (STACK_MAX - tc).min(from_stack.1);
                        let new_to = tc + take;
                        let rem = from_stack.1 - take;
                        self.slot_write(to, Some((tb, new_to)));
                        self.slot_write(
                            from,
                            if rem == 0 { None } else { Some((from_stack.0, rem)) },
                        );
                    }
                    _ => {
                        // Different blocks (or empty target) -> swap.
                        self.slot_write(to, Some(from_stack));
                        self.slot_write(from, to_stack);
                    }
                }
                // W5.2b: post-mutation durability fixup. The slot
                // contents above already moved; sync the parallel
                // map so flint+steel keeps its remaining-uses count
                // when the player drags it to a new slot.
                self.reconcile_slot_durabilities(
                    from_idx_dura,
                    to_idx_dura,
                    from_dura,
                    to_dura,
                );
            }
        }
    }

    /// W5.2b: after a Drag mutation, walk the from/to slot pair and
    /// re-anchor the durability map entries. The drag mutates the
    /// slot contents but the parallel map needs a manual swap /
    /// move to follow the stacks. This fn looks at the live
    /// inventory state to decide which side now holds the
    /// flint+steel and writes the captured durability back into
    /// the correct slot.
    pub(crate) fn reconcile_slot_durabilities(
        &mut self,
        from_idx: Option<usize>,
        to_idx: Option<usize>,
        from_dura: Option<u16>,
        to_dura: Option<u16>,
    ) {
        // Drop both old entries; we'll restore from the captured
        // values into whichever slot ended up holding the stack.
        if let Some(i) = from_idx {
            self.item_durability.remove(&i);
        }
        if let Some(i) = to_idx {
            self.item_durability.remove(&i);
        }
        // Look at the new state. A flint+steel in `from_idx` after
        // the drag means the previously-from stack stayed (or a
        // swap brought another flint+steel back); use whichever
        // captured durability matches the direction the data
        // travelled.
        if let Some(i) = from_idx
            && let Some((bid, _)) = self.inventory[i]
            && bid.0 == block::FLINT_AND_STEEL_ID
        {
            // After a swap the to_dura is now in the from slot.
            // After a leftover-back-write the from_dura stayed.
            // Pick the "to side carried back" value first when
            // present (swap path); otherwise keep the from value.
            let dura = to_dura.or(from_dura).unwrap_or(block::FLINT_AND_STEEL_MAX_USES);
            self.item_durability.insert(i, dura);
        }
        if let Some(i) = to_idx
            && let Some((bid, _)) = self.inventory[i]
            && bid.0 == block::FLINT_AND_STEEL_ID
        {
            // The from stack just moved here; use the from_dura
            // value.
            let dura = from_dura.or(to_dura).unwrap_or(block::FLINT_AND_STEEL_MAX_USES);
            self.item_durability.insert(i, dura);
        }
    }
}
