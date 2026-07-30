//! W47.8 - chest + furnace network message appliers, relocated from
//! main.rs. The net dispatch pass (`net::dispatch::tick`) collects
//! chest/furnace messages into per-tick buckets; this module owns
//! the actual application of those buckets onto the Voxel runtime.

#![allow(clippy::too_many_lines)]

use crate::*;
use crate::net::NetRole;
use infengine_voxel::BlockId;
pub(crate) fn apply_chest_open_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ChestOpenMsg)]) {
        for (client_id, m) in msgs {
            // Mint an empty chest if the slot doesn't exist yet -
            // matches the host-side RMB-on-chest path.
            vx.chests
                .entry(m.pos)
                .or_insert_with(block::chest::new_chest);
            vx.chest_viewers.insert(m.pos, *client_id);
            vx.broadcast_chest_state(m.pos);
        }
    }

    /// W5.3a: handle a batch of `vx.chest_act` requests. Validates
    /// the sender is the registered viewer; mutates the live chest
    /// + sender's inventory; re-broadcasts the new state.
pub(crate) fn apply_chest_act_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ChestActMsg)]) {
        for (client_id, m) in msgs {
            // Validate the viewer; ignore stray acts from clients
            // that didn't go through the open handshake.
            if vx.chest_viewers.get(&m.pos).copied() != Some(*client_id) {
                continue;
            }
            let Some(chest) = vx.chests.get_mut(&m.pos) else {
                continue;
            };
            // Mutate against the per-client inventory in the
            // NetRole::Server inventories map. Pull it out, mutate,
            // put it back. The host doesn't yet track per-viewer
            // cursors; a transient local cursor lives for the
            // duration of the apply call.
            //
            // W468: `ClientInv` is still tuple-shaped, so the
            // remote viewer's inventory converts at this edge -
            // components moving chest -> remote inventory are
            // dropped here (same loss as pre-W467; the host-side
            // chest itself keeps its components through every op).
            let mut cursor: Option<crate::item::ItemStack> = None;
            if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
                && let Some(inv) = inventories.get_mut(client_id)
            {
                let mut inv_stacks: Vec<Option<crate::item::ItemStack>> = inv
                    .inventory
                    .iter()
                    .map(|s| s.map(|(b, c)| crate::item::ItemStack::new(b.0, c)))
                    .collect();
                let action = m.action.to_grid_action();
                if m.player_side {
                    block::chest::apply_player_action(
                        &mut inv_stacks,
                        chest,
                        &mut cursor,
                        action,
                    );
                } else {
                    block::chest::apply_chest_action(
                        chest,
                        &mut inv_stacks,
                        &mut cursor,
                        action,
                    );
                }
                for (i, slot) in inv_stacks.iter().enumerate() {
                    if i >= inv.inventory.len() {
                        break;
                    }
                    inv.inventory[i] =
                        slot.as_ref().map(|st| (BlockId(st.id), st.count));
                }
                pending_inv_sends.insert(*client_id);
            }
            // Pos is needed beyond the borrow scope above.
            let pos = m.pos;
            vx.broadcast_chest_state(pos);
        }
    }

    /// W5.3a: handle a batch of `vx.chest_close` requests. Clears
    /// the viewer registration so subsequent host-side mutations
    /// stop fanning out a `ChestStateMsg` to the previous viewer.
pub(crate) fn apply_chest_close_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ChestCloseMsg)]) {
        for (client_id, m) in msgs {
            if vx.chest_viewers.get(&m.pos).copied() == Some(*client_id) {
                vx.chest_viewers.remove(&m.pos);
            }
        }
    }

    /// W5.3b: mirror of `apply_chest_open_msgs` for furnaces.
pub(crate) fn apply_furnace_open_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::FurnaceOpenMsg)]) {
        for (client_id, m) in msgs {
            vx.furnaces
                .entry(m.pos)
                .or_insert_with(block::furnace::new_furnace);
            vx.furnace_viewers.insert(m.pos, *client_id);
            // Open emits an immediate state snapshot (not subject
            // to the 5 Hz throttle; that gate only applies to the
            // cook-progress steady-state broadcasts).
            vx.broadcast_furnace_state(m.pos);
        }
    }

    /// W5.3b: mirror of `apply_chest_act_msgs` for furnaces.
pub(crate) fn apply_furnace_act_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::FurnaceActMsg)]) {
        for (client_id, m) in msgs {
            if vx.furnace_viewers.get(&m.pos).copied() != Some(*client_id) {
                continue;
            }
            let Some(furnace) = vx.furnaces.get_mut(&m.pos) else {
                continue;
            };
            // W468: same tuple-shaped `ClientInv` edge as the
            // chest apply above.
            let mut cursor: Option<crate::item::ItemStack> = None;
            if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
                && let Some(inv) = inventories.get_mut(client_id)
            {
                let mut inv_stacks: Vec<Option<crate::item::ItemStack>> = inv
                    .inventory
                    .iter()
                    .map(|s| s.map(|(b, c)| crate::item::ItemStack::new(b.0, c)))
                    .collect();
                let action = m.action.to_grid_action();
                if m.player_side {
                    block::furnace::apply_player_action(
                        &mut inv_stacks,
                        furnace,
                        &mut cursor,
                        action,
                    );
                } else {
                    block::furnace::apply_furnace_action(
                        furnace,
                        &mut inv_stacks,
                        &mut cursor,
                        action,
                    );
                }
                for (i, slot) in inv_stacks.iter().enumerate() {
                    if i >= inv.inventory.len() {
                        break;
                    }
                    inv.inventory[i] =
                        slot.as_ref().map(|st| (BlockId(st.id), st.count));
                }
                pending_inv_sends.insert(*client_id);
            }
            let pos = m.pos;
            vx.broadcast_furnace_state(pos);
        }
    }

    /// W5.3b: mirror of `apply_chest_close_msgs` for furnaces.
pub(crate) fn apply_furnace_close_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::FurnaceCloseMsg)]) {
        for (client_id, m) in msgs {
            if vx.furnace_viewers.get(&m.pos).copied() == Some(*client_id) {
                vx.furnace_viewers.remove(&m.pos);
                // Clear the throttle stamp so a re-open later
                // doesn't sit out the residual 200ms.
                vx.furnace_state_last_sent_ms.remove(&m.pos);
            }
        }
    }

    /// W5.3a: send a fresh `ChestStateMsg` snapshot for `pos` to
    /// the registered viewer (if any). Called by the server-side
    /// chest mutation paths so a remote viewer sees the host's
    /// updated contents.
pub(crate) fn broadcast_chest_state(vx: &mut crate::Voxel, pos: [i32; 3]) {
        let Some(&viewer) = vx.chest_viewers.get(&pos) else {
            return;
        };
        let Some(chest) = vx.chests.get(&pos) else {
            return;
        };
        // W468: the wire schema is tuple-shaped - project each
        // stack down to (id, count). Components stay host-side
        // until the message schemas carry them.
        let slots: Vec<Option<(u16, u8)>> = chest
            .slots()
            .iter()
            .map(|s| s.as_ref().map(|st| st.as_tuple()))
            .collect();
        // Host's `chest_held` is the cursor; on a remote viewer
        // the host doesn't track per-viewer cursors today, so we
        // pass `None` here. The next iteration of W5.3 can
        // promote `chest_held` to a per-viewer map; for Wave 5
        // the simpler shape ships.
        let msg = crate::net::ChestStateMsg {
            pos,
            slots,
            cursor: None,
        };
        if let NetRole::Server { server, .. } = &mut vx.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(&msg, cfg) {
                server.send_bytes_to_client(viewer, "vx.chest_state", buf);
            }
        }
    }

    /// W5.3b: same as `broadcast_chest_state` but for furnaces.
    /// Includes the live `FurnaceRuntime` triple so the client UI
    /// can render fuel + cook progress bars without a separate
    /// message. Caller is responsible for the 5 Hz throttle via
    /// `should_send_furnace_state`.
pub(crate) fn broadcast_furnace_state(vx: &mut crate::Voxel, pos: [i32; 3]) {
        let Some(&viewer) = vx.furnace_viewers.get(&pos) else {
            return;
        };
        let Some(furnace) = vx.furnaces.get(&pos) else {
            return;
        };
        let runtime = vx
            .furnace_runtimes
            .get(&pos)
            .copied()
            .unwrap_or_default();
        // W468: tuple-shaped wire projection (see chest broadcast).
        let slots: Vec<Option<(u16, u8)>> = furnace
            .slots()
            .iter()
            .map(|s| s.as_ref().map(|st| st.as_tuple()))
            .collect();
        let msg = crate::net::FurnaceStateMsg {
            pos,
            slots,
            fuel_left_ms: runtime.fuel_left_ms,
            fuel_full_ms: runtime.fuel_full_ms,
            cook_progress_ms: runtime.cook_progress_ms,
            cursor: None,
        };
        if let NetRole::Server { server, .. } = &mut vx.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(&msg, cfg) {
                server.send_bytes_to_client(viewer, "vx.furnace_state", buf);
            }
        }
    }
