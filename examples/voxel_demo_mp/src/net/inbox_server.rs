//! W472 - the server-side inbox drain, split out of `net/mod.rs`
//! to keep the wire-type module under the size cap. Owns
//! `ServerInboxCtx` and `process_server_inbox`: the per-topic
//! decode plus collector routing for every message a hosting
//! instance receives. See the module docs in `net/mod.rs` for the
//! protocol overview.

use std::collections::{HashMap, HashSet};

use infengine_net::NetServer;

use crate::HOTBAR_SLOTS;
use crate::item::ItemStack;

use super::{
    ChestActMsg, ChestCloseMsg, ChestOpenMsg, ContainerDragMsg, ClientInv,
    DamageMsg, EditMsg, FurnaceActMsg, FurnaceCloseMsg, FurnaceOpenMsg,
    InvDragMsg, InvOpMsg, MobHitMsg, PlayerMsg, PvPHitMsg, SEL_NONE,
    wear_client_weapon,
};

/// Aggregate for the host-side inbox drain. Wraps the subset of
/// `Voxel` state the server arm of `tick_net` mutates (`day_time`,
/// `last_day_broadcast`, plus the same per-tick collectors the
/// client arm uses), along with three local outputs the surrounding
/// `tick_net` consumes after the drain releases (`raw_edits`,
/// `path_requests`, `raw_drags`).
///
/// The `NetRole::Server { server, poses, inventories,
/// pending_inv_sends }` borrow held by `tick_net` is still passed
/// as separate `&mut` parameters - those four references are
/// mutually disjoint from the `Voxel` field refs in this ctx, so
/// the split-borrow shape stays unambiguous to the compiler.
pub(crate) struct ServerInboxCtx<'a> {
    /// Authoritative day clock - bumped by `vx.sleep` when the
    /// host accepts a client's bed request.
    pub day_time: &'a mut f32,
    /// Last-broadcast timestamp of `vx.day`. Cleared so the next
    /// tick definitely re-emits the time of day after a sleep
    /// skip moves the clock.
    pub last_day_broadcast: &'a mut Option<std::time::Instant>,
    /// True while the host has /worldlock engaged - client edits
    /// are silently dropped.
    pub world_locked: bool,
    /// Snapshot of the host's eye position for PvP-hit reach
    /// validation against id 0.
    pub host_player_pos: [f32; 3],
    /// Outputs filled by the drain + drained after the match arm
    /// closes for validated world / inventory mutation.
    pub raw_edits: &'a mut Vec<(u32, EditMsg)>,
    pub path_requests: &'a mut Vec<(u32, [i32; 3])>,
    /// Shared collectors (same shape as `ClientInboxCtx`).
    pub interp_updates: &'a mut Vec<PlayerMsg>,
    pub mob_hits_to_apply: &'a mut Vec<(u32, u32)>,
    pub torch_orient_updates: &'a mut Vec<([i32; 3], u8)>,
    /// W125: door place/toggle state updates (lower pos, facing,
    /// open) received this tick. Applied + remeshed by dispatch.
    pub door_state_updates: &'a mut Vec<([i32; 3], u8, bool)>,
    /// W126: chest facing records (pos, facing) from places.
    pub chest_facing_updates: &'a mut Vec<([i32; 3], u8)>,
    /// W143: stored rail shape updates (pos, shape; 0xFF = removed).
    pub rail_shape_updates: &'a mut Vec<([i32; 3], u8)>,
    pub chat_inbox: &'a mut Vec<String>,
    pub pvp_self_damage: &'a mut f32,
    /// W5.3a: pending chest open / act / close events for the
    /// host's main loop to apply against the live `chests` map +
    /// `chest_viewers` registration.
    pub chest_opens: &'a mut Vec<(u32, ChestOpenMsg)>,
    pub chest_acts: &'a mut Vec<(u32, ChestActMsg)>,
    pub chest_closes: &'a mut Vec<(u32, ChestCloseMsg)>,
    /// W5.3b: same for furnaces.
    pub furnace_opens: &'a mut Vec<(u32, FurnaceOpenMsg)>,
    pub furnace_acts: &'a mut Vec<(u32, FurnaceActMsg)>,
    pub furnace_closes: &'a mut Vec<(u32, FurnaceCloseMsg)>,
    /// W470: completed drag-paints against open containers.
    pub chest_drags: &'a mut Vec<(u32, ContainerDragMsg)>,
    pub furnace_drags: &'a mut Vec<(u32, ContainerDragMsg)>,
    /// W470/W472: inventory-screen ops (the full doClick set) +
    /// the no-panel selected-slot throw.
    pub inv_ops: &'a mut Vec<(u32, InvOpMsg)>,
    /// W472: completed E-screen drag-paints.
    pub inv_drags: &'a mut Vec<(u32, InvDragMsg)>,
    /// W472: game-mode announcements (`vx.gamemode`, instabuild
    /// bool) from the join handshake + /gamemode changes. Applied
    /// to `Voxel::client_gamemodes` after the drain.
    pub gamemode_updates: &'a mut Vec<(u32, bool)>,
}

/// Drain the server socket's inbound queue and dispatch each
/// message to the right collector / inline broadcast. Mirrors the
/// per-topic match block from `Voxel::tick_net`'s server arm.
///
/// `server`, `poses`, `inventories`, `pending_inv_sends` are
/// passed as plain `&mut` parameters because they live inside the
/// `NetRole::Server { .. }` destructuring at the call site - the
/// `&mut self.net` borrow keeps them disjoint from the `Voxel`
/// refs inside `ctx`.
///
/// Wide ABI on purpose: every coupling between the wire decode
/// and the host's authoritative state lives explicitly in this
/// signature, so the call site in `tick_net` reads as a single
/// data-flow declaration.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_server_inbox(
    server: &mut NetServer,
    poses: &mut HashMap<u32, PlayerMsg>,
    inventories: &mut HashMap<u32, ClientInv>,
    pending_inv_sends: &mut HashSet<u32>,
    pvp_reach: f32,
    pvp_damage: f32,
    day_length_s: f32,
    recipes_lookup: &dyn Fn(usize) -> Option<&'static [(u16, u8)]>,
    ctx: &mut ServerInboxCtx<'_>,
) {
    let cfg = bincode::config::standard();
    for (client_id, kind, data) in server.drain_bytes() {
        match kind.as_str() {
            "vx.edit" => {
                // World lock: ignore client edits
                // entirely. The host's own edits go
                // through the local pipeline + still
                // broadcast.
                if ctx.world_locked {
                    continue;
                }
                if let Ok((e, _)) =
                    bincode::serde::decode_from_slice::<EditMsg, _>(&data, cfg)
                {
                    ctx.raw_edits.push((client_id, e));
                }
            }
            "vx.player" => {
                if let Ok((mut p, _)) =
                    bincode::serde::decode_from_slice::<PlayerMsg, _>(&data, cfg)
                {
                    p.id = client_id;
                    poses.insert(client_id, p);
                    ctx.interp_updates.push(p);
                }
            }
            "vx.chest_open" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<ChestOpenMsg, _>(&data, cfg)
                {
                    ctx.chest_opens.push((client_id, m));
                }
            }
            "vx.chest_act" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<ChestActMsg, _>(&data, cfg)
                {
                    ctx.chest_acts.push((client_id, m));
                }
            }
            "vx.chest_close" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<ChestCloseMsg, _>(&data, cfg)
                {
                    ctx.chest_closes.push((client_id, m));
                }
            }
            "vx.furnace_open" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<FurnaceOpenMsg, _>(&data, cfg)
                {
                    ctx.furnace_opens.push((client_id, m));
                }
            }
            "vx.furnace_act" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<FurnaceActMsg, _>(&data, cfg)
                {
                    ctx.furnace_acts.push((client_id, m));
                }
            }
            "vx.furnace_close" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<FurnaceCloseMsg, _>(&data, cfg)
                {
                    ctx.furnace_closes.push((client_id, m));
                }
            }
            "vx.chest_drag" => {
                if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                    ContainerDragMsg,
                    _,
                >(&data, cfg)
                {
                    ctx.chest_drags.push((client_id, m));
                }
            }
            "vx.furnace_drag" => {
                if let Ok((m, _)) = bincode::serde::decode_from_slice::<
                    ContainerDragMsg,
                    _,
                >(&data, cfg)
                {
                    ctx.furnace_drags.push((client_id, m));
                }
            }
            "vx.inv_op" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<InvOpMsg, _>(&data, cfg)
                {
                    ctx.inv_ops.push((client_id, m));
                }
            }
            "vx.inv_drag" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<InvDragMsg, _>(&data, cfg)
                {
                    ctx.inv_drags.push((client_id, m));
                }
            }
            "vx.gamemode" => {
                // W472: client announces its instabuild flag (join
                // handshake + /gamemode changes). Client-asserted
                // trust - consistent with /gamemode being
                // client-local; the point is protocol capability
                // for creative-only ops, not anti-cheat.
                if let Ok((creative, _)) =
                    bincode::serde::decode_from_slice::<bool, _>(&data, cfg)
                {
                    ctx.gamemode_updates.push((client_id, creative));
                }
            }
            "vx.armor" => {
                // W472: client asserts its armor state (damage
                // wear happens client-side; the authoritative
                // E-screen armor section mirrors it so snapshots
                // never revert wear).
                if let Ok((slots, _)) = bincode::serde::decode_from_slice::<
                    Vec<Option<ItemStack>>,
                    _,
                >(&data, cfg)
                    && let Some(inv) = inventories.get_mut(&client_id)
                {
                    for (i, s) in slots.into_iter().enumerate() {
                        if i < inv.armor.len() {
                            inv.armor[i] = s;
                        }
                    }
                }
            }
            "vx.recipe" => {
                // Client asked the server to auto-
                // fill its craft grid with recipe
                // index `idx`. Validate the sender's
                // inventory carries every input,
                // move them into craft_grid, and
                // queue an inv snapshot back.
                if let Ok((idx, _)) =
                    bincode::serde::decode_from_slice::<u32, _>(&data, cfg)
                    && let Some(recipe_inputs) = recipes_lookup(idx as usize)
                    && let Some(inv) = inventories.get_mut(&client_id)
                {
                    let mut taken: Vec<(usize, ItemStack)> = Vec::new();
                    let mut placed: [Option<ItemStack>; 4] =
                        std::array::from_fn(|_| None);
                    let mut ok = true;
                    for (i, (bid, n)) in recipe_inputs.iter().enumerate() {
                        if i >= 4 {
                            break;
                        }
                        let need = *n;
                        // W469: the craft grid is stack-typed, so
                        // any matching stack feeds it losslessly -
                        // the slice into the grid clones the source
                        // components (the W468 plain-only gate is
                        // gone).
                        let mut found: Option<usize> = None;
                        for (si, slot) in inv.inventory.iter().enumerate() {
                            if let Some(st) = slot
                                && st.id == *bid
                                && st.count >= need
                            {
                                found = Some(si);
                                break;
                            }
                        }
                        match found {
                            Some(si) => {
                                let original = inv.inventory[si]
                                    .clone()
                                    .expect("verified");
                                let remaining =
                                    original.count.saturating_sub(need);
                                inv.inventory[si] = if remaining == 0 {
                                    None
                                } else {
                                    Some(original.with_count(remaining))
                                };
                                placed[i] = Some(original.with_count(need));
                                taken.push((si, original));
                            }
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        // Return any existing craft-grid stacks to
                        // the inventory before overwriting
                        // (components intact).
                        let returns: Vec<ItemStack> = inv
                            .craft_grid
                            .iter_mut()
                            .filter_map(|s| s.take())
                            .collect();
                        for st in returns {
                            let _ =
                                crate::item::stack::drop_into_first_free_stacks(
                                    &mut inv.inventory,
                                    st,
                                );
                        }
                        inv.craft_grid = placed;
                    } else {
                        for (si, original) in taken {
                            inv.inventory[si] = Some(original);
                        }
                    }
                    pending_inv_sends.insert(client_id);
                }
            }
            "vx.mob_hit" => {
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<MobHitMsg, _>(&data, cfg)
                {
                    ctx.mob_hits_to_apply.push((client_id, m.mob_id));
                }
            }
            "vx.sleep" => {
                // Client requested a time skip (bed
                // RMB). Authoritative side validates
                // it's night before honouring; force
                // a vx.day broadcast next tick so
                // every peer sees the morning jump.
                let phase = (*ctx.day_time / day_length_s).rem_euclid(1.0);
                let elev = (phase * std::f32::consts::TAU).sin();
                if elev < -0.05 {
                    *ctx.day_time = day_length_s * 0.20;
                    *ctx.last_day_broadcast = None;
                    let sender = server
                        .client_name(client_id)
                        .unwrap_or_else(|| format!("p{client_id}"));
                    let line =
                        format!("* {sender} sleeps through the night");
                    ctx.chat_inbox.push(line.clone());
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec(&line, cfg)
                    {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
            }
            "vx.path" => {
                // Defer processing until the match
                // arm releases self.net so we can
                // call world_set_tracked + broadcast.
                if let Ok((v, _)) =
                    bincode::serde::decode_from_slice::<[i32; 3], _>(&data, cfg)
                {
                    ctx.path_requests.push((client_id, v));
                }
            }
            "vx.torch_orient" => {
                // Cosmetic lean info that rides
                // alongside a torch place. Edit itself
                // was already reach-validated via
                // `vx.edit`; here we just record + fan
                // out to every other peer so they
                // render the matching wall / floor
                // mount.
                if let Ok(((v, orient), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.torch_orient_updates.push((v, orient));
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec((v, orient), cfg)
                    {
                        server.broadcast_bytes("vx.torch_orient", buf);
                    }
                }
            }
            "vx.door_state" => {
                // W125: a client placed/toggled a door. Record
                // locally + fan out so every peer flips the same
                // panel. (Reach was validated by the paired
                // vx.edit for places; toggles are cosmetic+door
                // -local so a light touch is fine here.)
                if let Ok(((v, facing, open), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8, bool), _>(
                        &data, cfg,
                    )
                {
                    ctx.door_state_updates.push((v, facing, open));
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec((v, facing, open), cfg)
                    {
                        server.broadcast_bytes("vx.door_state", buf);
                    }
                }
            }
            "vx.chest_facing" => {
                // W126: cosmetic chest yaw riding alongside a
                // chest place (reach validated by the vx.edit).
                if let Ok(((v, facing), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.chest_facing_updates.push((v, facing));
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec((v, facing), cfg)
                    {
                        server.broadcast_bytes("vx.chest_facing", buf);
                    }
                }
            }
            "vx.rail_shape" => {
                // W143: a client placed/broke a rail; its placement
                // already computed the stateful shapes (reach was
                // validated by the paired vx.edit). Record + fan out.
                if let Ok(((v, shape), _)) =
                    bincode::serde::decode_from_slice::<([i32; 3], u8), _>(
                        &data, cfg,
                    )
                {
                    ctx.rail_shape_updates.push((v, shape));
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec((v, shape), cfg)
                    {
                        server.broadcast_bytes("vx.rail_shape", buf);
                    }
                }
            }
            "vx.fellout" => {
                // Wipe the storage rows + craft grid -
                // matches the local client penalty. Keep
                // the hotbar so the player isn't bare.
                if let Some(inv) = inventories.get_mut(&client_id) {
                    for s in inv.inventory[HOTBAR_SLOTS..].iter_mut() {
                        *s = None;
                    }
                    for s in inv.craft_grid.iter_mut() {
                        *s = None;
                    }
                    pending_inv_sends.insert(client_id);
                }
            }
            "vx.leave" => {
                // Explicit disconnect: drop the pose
                // straight away so the reaper's 5 s
                // grace doesn't strand a frozen ghost.
                poses.remove(&client_id);
                tracing::info!(
                    client_id,
                    "client requested explicit leave"
                );
            }
            "vx.chat" => {
                // Decode + tag with the sender's
                // client name, then broadcast so every
                // peer (including the originator) sees
                // the same final line. The whisper
                // marker (\u{1}whisper\u{1}<id>\u{1}<text>)
                // is intercepted and re-routed to a
                // single client instead.
                if let Ok((text, _)) =
                    bincode::serde::decode_from_slice::<String, _>(&data, cfg)
                {
                    let sender = server
                        .client_name(client_id)
                        .unwrap_or_else(|| format!("p{client_id}"));
                    if let Some(rest) =
                        text.strip_prefix("\u{1}whisper\u{1}")
                    {
                        let mut parts = rest.splitn(2, '\u{1}');
                        let tid_str = parts.next().unwrap_or("");
                        let body = parts.next().unwrap_or("");
                        if let Ok(tid) = tid_str.parse::<u32>()
                            && !body.is_empty()
                        {
                            let line =
                                format!("[whisper from {sender}] {body}");
                            if tid == 0 {
                                ctx.chat_inbox.push(line);
                            } else if let Ok(buf) =
                                bincode::serde::encode_to_vec(&line, cfg)
                            {
                                server.send_bytes_to_client(
                                    tid, "vx.chat", buf,
                                );
                            }
                        }
                    } else {
                        let line = format!("<{sender}> {text}");
                        ctx.chat_inbox.push(line.clone());
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(&line, cfg)
                        {
                            server.broadcast_bytes("vx.chat", buf);
                        }
                    }
                }
            }
            "vx.pvp_hit" => {
                // Validate the attacker's reach + the
                // existence of the target, then forward
                // a `vx.damage` to the target client.
                // Host (id 0) attacks are handled
                // client-side (see the lmb dispatch);
                // this path only fires for remote
                // clients hitting other peers.
                if let Ok((m, _)) =
                    bincode::serde::decode_from_slice::<PvPHitMsg, _>(&data, cfg)
                {
                    let attacker = poses.get(&client_id).copied();
                    let attacker_pos = attacker.map(|p| p.pos);
                    let target_pos = if m.target_id == 0 {
                        Some(ctx.host_player_pos)
                    } else {
                        poses.get(&m.target_id).copied().map(|p| p.pos)
                    };
                    if let (Some(ap), Some(tp)) =
                        (attacker_pos, target_pos)
                    {
                        let dx = ap[0] - tp[0];
                        let dy = ap[1] - tp[1];
                        let dz = ap[2] - tp[2];
                        let d2 = dx * dx + dy * dy + dz * dz;
                        if d2 <= (pvp_reach + 1.0).powi(2) {
                            // W471: wear the remote attacker's
                            // weapon on the AUTHORITATIVE copy
                            // (the attacker's own client applies
                            // the same wear as prediction).
                            let sel = attacker
                                .map(|p| p.sel)
                                .unwrap_or(SEL_NONE);
                            wear_client_weapon(
                                inventories,
                                pending_inv_sends,
                                client_id,
                                sel,
                            );
                            if m.target_id == 0 {
                                // Host is the target.
                                *ctx.pvp_self_damage += pvp_damage;
                            } else {
                                let dmg = DamageMsg { amount: pvp_damage };
                                if let Ok(buf) =
                                    bincode::serde::encode_to_vec(dmg, cfg)
                                {
                                    server.send_bytes_to_client(
                                        m.target_id,
                                        "vx.damage",
                                        buf,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
