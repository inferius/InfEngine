//! W47.8 - chest + furnace network message appliers, relocated from
//! main.rs. The net dispatch pass (`net::dispatch::tick`) collects
//! chest/furnace messages into per-tick buckets; this module owns
//! the actual application of those buckets onto the Voxel runtime.
//!
//! W468: the host keeps an authoritative per-viewer cursor
//! (`Voxel::container_cursors`, keyed by client id). Every act
//! applies against that cursor and every state snapshot bundles it
//! back, so a remote Take can no longer evaporate the taken stack;
//! close / disconnect returns the held stack to the client's
//! inventory (spilling at the container on overflow - MC rule).
//! `ClientInv` is stack-native, so components ride through every
//! remote container session without a tuple detour.

#![allow(clippy::too_many_lines)]

use crate::*;
use crate::net::NetRole;

/// W468: return `client_id`'s authoritative cursor to their
/// server-side inventory; whatever doesn't fit spills as a floor
/// drop at `spill_pos`. Shared by close, re-open (stale cursor
/// from a lost close message) and disconnect, so a held stack can
/// never evaporate.
pub(crate) fn recover_viewer_cursor(
    vx: &mut crate::Voxel,
    client_id: u32,
    spill_pos: [i32; 3],
) {
    let Some(held) = vx.container_cursors.remove(&client_id).flatten() else {
        return;
    };
    let mut leftover = held.count;
    if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
        && let Some(inv) = inventories.get_mut(&client_id)
    {
        leftover = crate::item::stack::drop_into_first_free_stacks(
            &mut inv.inventory,
            held.clone(),
        );
        pending_inv_sends.insert(client_id);
    }
    if leftover > 0 {
        crate::drops::spawn_stack(vx, held.with_count(leftover), spill_pos);
    }
}

pub(crate) fn apply_chest_open_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ChestOpenMsg)]) {
        for (client_id, m) in msgs {
            // A stale cursor from a lost close message returns to
            // the opener's inventory before the fresh session
            // starts (the client's local cursor is empty on open).
            recover_viewer_cursor(vx, *client_id, m.pos);
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
    /// the sender is the registered viewer; mutates the live chest,
    /// the sender's inventory and the sender's authoritative
    /// cursor; re-broadcasts the new state (cursor included).
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
            let action = m.action.to_grid_action();
            // W468: the viewer's cursor persists across acts in
            // `container_cursors` and rides back on the state
            // snapshot below - a Take no longer drops the stack
            // into a discarded temporary.
            let cursor = vx.container_cursors.entry(*client_id).or_default();
            let mut no_inv: [Option<crate::item::ItemStack>; 0] = [];
            if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
                && let Some(inv) = inventories.get_mut(client_id)
            {
                // ClientInv is stack-native (W468): the apply
                // helpers mutate it directly, components intact.
                if m.player_side {
                    block::chest::apply_player_action(
                        &mut inv.inventory,
                        chest,
                        cursor,
                        action,
                    );
                } else {
                    block::chest::apply_chest_action(
                        chest,
                        &mut inv.inventory,
                        cursor,
                        action,
                    );
                }
                pending_inv_sends.insert(*client_id);
            } else if !m.player_side {
                // No per-client inventory (offline test harness):
                // chest-side acts still run against the cursor;
                // Transfer has no partner and no-ops.
                block::chest::apply_chest_action(chest, &mut no_inv, cursor, action);
            }
            // Pos is needed beyond the borrow scope above.
            let pos = m.pos;
            vx.broadcast_chest_state(pos);
        }
    }

    /// W5.3a: handle a batch of `vx.chest_close` requests. Clears
    /// the viewer registration + returns the viewer's cursor to
    /// their inventory (spill at the chest on overflow).
pub(crate) fn apply_chest_close_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ChestCloseMsg)]) {
        for (client_id, m) in msgs {
            if vx.chest_viewers.get(&m.pos).copied() == Some(*client_id) {
                vx.chest_viewers.remove(&m.pos);
                recover_viewer_cursor(vx, *client_id, m.pos);
            }
        }
    }

    /// W5.3b: mirror of `apply_chest_open_msgs` for furnaces.
pub(crate) fn apply_furnace_open_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::FurnaceOpenMsg)]) {
        for (client_id, m) in msgs {
            recover_viewer_cursor(vx, *client_id, m.pos);
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
            let action = m.action.to_grid_action();
            // W468: same per-viewer cursor as the chest apply.
            let cursor = vx.container_cursors.entry(*client_id).or_default();
            let mut no_inv: [Option<crate::item::ItemStack>; 0] = [];
            if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
                && let Some(inv) = inventories.get_mut(client_id)
            {
                if m.player_side {
                    block::furnace::apply_player_action(
                        &mut inv.inventory,
                        furnace,
                        cursor,
                        action,
                    );
                } else {
                    block::furnace::apply_furnace_action(
                        furnace,
                        &mut inv.inventory,
                        cursor,
                        action,
                    );
                }
                pending_inv_sends.insert(*client_id);
            } else if !m.player_side {
                block::furnace::apply_furnace_action(
                    furnace,
                    &mut no_inv,
                    cursor,
                    action,
                );
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
                recover_viewer_cursor(vx, *client_id, m.pos);
            }
        }
    }

    /// W5.3a: send a fresh `ChestStateMsg` snapshot for `pos` to
    /// the registered viewer (if any). Called by the server-side
    /// chest mutation paths so a remote viewer sees the host's
    /// updated contents. W468: stack-native slots + the viewer's
    /// authoritative cursor ride the message.
pub(crate) fn broadcast_chest_state(vx: &mut crate::Voxel, pos: [i32; 3]) {
        let Some(&viewer) = vx.chest_viewers.get(&pos) else {
            return;
        };
        let Some(chest) = vx.chests.get(&pos) else {
            return;
        };
        let msg = crate::net::ChestStateMsg {
            pos,
            slots: chest.slots().to_vec(),
            cursor: vx.container_cursors.get(&viewer).cloned().flatten(),
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
        let msg = crate::net::FurnaceStateMsg {
            pos,
            slots: furnace.slots().to_vec(),
            fuel_left_ms: runtime.fuel_left_ms,
            fuel_full_ms: runtime.fuel_full_ms,
            cook_progress_ms: runtime.cook_progress_ms,
            cursor: vx.container_cursors.get(&viewer).cloned().flatten(),
        };
        if let NetRole::Server { server, .. } = &mut vx.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(&msg, cfg) {
                server.send_bytes_to_client(viewer, "vx.furnace_state", buf);
            }
        }
    }
