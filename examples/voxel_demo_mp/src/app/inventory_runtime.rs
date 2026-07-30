//! W47.28 - inventory_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {

    /// W5.2b/W466/W471: register one use of the currently-held
    /// flint+steel after a successful ignition (portal frame OR
    /// fire block). MC `FlintAndSteelItem.useOn` calls
    /// `hurtAndBreak(1)`: one wear point per use, break at the
    /// table max (64), creative never wears (infinite-materials
    /// gate).
    ///
    /// Idempotent across non-flint+steel held items - a Cobble in
    /// hand never triggered ignition in the first place, so the
    /// fn only fires when the caller already verified `held.id ==
    /// FLINT_AND_STEEL_ID` (and a plain block has no max_damage
    /// row anyway).
    pub(crate) fn consume_flint_and_steel_use(&mut self) {
        self.wear_selected_stack(1);
    }

    /// W471: apply `amount` wear points to the SELECTED hotbar
    /// stack through the central `hurt_and_break` port. Plays the
    /// MC break feedback (`entity.item.break`) and leaves the slot
    /// empty when the item breaks. No-op in creative and for items
    /// without a max_damage row.
    pub(crate) fn wear_selected_stack(&mut self, amount: u16) {
        let slot_idx = self.selected_slot;
        let Some(slot) = self.inventory.get_mut(slot_idx) else {
            return;
        };
        let creative = self.abilities.instabuild;
        if crate::item::durability::hurt_and_break(slot, amount, creative) {
            self.play_item_break_fx();
        }
    }

    /// W471: MC item-break feedback (`LivingEntity.breakItem`):
    /// `entity.item.break` at the player. Synth opt-out - silence
    /// beats a beep when the pack misses the event (W56.1 rule).
    /// The source also sprays 5 item particles; skipped until an
    /// item-particle path exists.
    pub(crate) fn play_item_break_fx(&mut self) {
        let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
        self.sfx_at("item/break", pos, |_, _| {});
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

    /// W472: apply one E-screen op locally (host / offline apply +
    /// the client's optimistic prediction) through the SHARED
    /// screen core; thrown stacks spawn at the player (no-op in
    /// the client role - the host spawns from the wire op).
    pub(crate) fn apply_inv_screen_op(&mut self, op: crate::net::InvOpMsg) {
        let creative = self.abilities.instabuild;
        let mut cursor = self.inv_screen_held.take();
        let thrown = crate::inventory::inv_screen_apply_op(
            &mut self.craft_grid,
            &mut self.armor_slots,
            &mut self.inventory,
            &mut cursor,
            op,
            creative,
        );
        self.inv_screen_held = cursor;
        for st in thrown {
            self.throw_stack_from_player(st);
        }
    }

    /// W472: close the inventory screen - recover the cursor into
    /// the inventory (spill as a drop on overflow) and tell the
    /// host (client role) so the authoritative cursor follows.
    pub(crate) fn close_inventory_screen(&mut self) {
        self.inventory_open = false;
        if let NetRole::Client { client, .. } = &mut self.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(
                crate::net::InvOpMsg::CloseScreen,
                cfg,
            ) {
                let _ = client.send_bytes("vx.inv_op", buf);
            }
        }
        if let Some(held) = self.inv_screen_held.take() {
            let left = crate::item::stack::drop_into_first_free_stacks(
                &mut self.inventory,
                held.clone(),
            );
            if left > 0 {
                self.throw_stack_from_player(held.with_count(left));
            }
        }
    }

    /// W470: spawn a stack the player just threw (Q / Ctrl+Q) at
    /// the hand with MC toss velocity along the current look.
    pub(crate) fn throw_stack_from_player(
        &mut self,
        stack: crate::item::ItemStack,
    ) {
        let eye = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
        let look = [
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        ];
        crate::drops::spawn_thrown_stack(self, stack, eye, look);
    }
}
