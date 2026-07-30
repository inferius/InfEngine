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

/// W472: canonicalise a wire chest pos to the double-chest
/// PRIMARY (+ the secondary half when paired). Mints the container
/// entries for both halves so a freshly-streamed client can open
/// either side.
pub(crate) fn chest_canonical(
    vx: &mut crate::Voxel,
    pos: [i32; 3],
) -> ([i32; 3], Option<[i32; 3]>) {
    vx.chests.entry(pos).or_insert_with(block::chest::new_chest);
    // A paired neighbour with no container entry yet gets one, so
    // partner_for's contains_key gate passes.
    let f = vx.chest_facing.get(&pos).copied().unwrap_or(3);
    let ext = block::chest::ext_pos_dir(f);
    for dir in [ext, [-ext[0], 0, -ext[2]]] {
        let p = [pos[0] + dir[0], pos[1], pos[2] + dir[2]];
        if vx.world.get(p).0 == crate::block::CHEST_ID
            && vx.chest_facing.get(&p).copied().unwrap_or(3) == f
        {
            vx.chests.entry(p).or_insert_with(block::chest::new_chest);
        }
    }
    block::chest::canonical_pair(
        &vx.world,
        &vx.chests,
        &vx.chest_facing,
        pos,
    )
}

pub(crate) fn apply_chest_open_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ChestOpenMsg)]) {
        for (client_id, m) in msgs {
            // W472: the menu is keyed by the double-chest PRIMARY.
            let (primary, _) = chest_canonical(vx, m.pos);
            // A stale cursor from a lost close message returns to
            // the opener's inventory before the fresh session
            // starts (the client's local cursor is empty on open).
            recover_viewer_cursor(vx, *client_id, primary);
            vx.chest_viewers.insert(primary, *client_id);
            vx.broadcast_chest_state(primary);
        }
    }

/// W470: the sender's eye position + engine look vector, for
/// spawning thrown stacks at the right spot with MC toss velocity.
/// Falls back to the host pose when the id has no tracked pose
/// (offline test harness).
fn client_eye_and_look(vx: &crate::Voxel, client_id: u32) -> ([f32; 3], [f32; 3]) {
    if let NetRole::Server { poses, .. } = &vx.net
        && let Some(p) = poses.get(&client_id)
    {
        let look = [
            p.yaw.cos() * p.pitch.cos(),
            p.pitch.sin(),
            -p.yaw.sin() * p.pitch.cos(),
        ];
        return (p.pos, look);
    }
    (
        [vx.player_pos.x, vx.player_pos.y, vx.player_pos.z],
        [1.0, 0.0, 0.0],
    )
}

    /// W5.3a: handle a batch of `vx.chest_act` requests. Validates
    /// the sender is the registered viewer; mutates the live chest,
    /// the sender's inventory and the sender's authoritative
    /// cursor; re-broadcasts the new state (cursor included).
    /// W470: routes through the shared click core (all doClick
    /// ops); thrown stacks spawn at the sender's pose with MC toss
    /// velocity.
pub(crate) fn apply_chest_act_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ChestActMsg)]) {
        for (client_id, m) in msgs {
            // W472: acts address the menu by the double-chest
            // PRIMARY; the client sends the canonical pos, but a
            // stale/hostile half canonicalises again here.
            let (primary, secondary) = chest_canonical(vx, m.pos);
            // Validate the viewer; ignore stray acts from clients
            // that didn't go through the open handshake.
            if vx.chest_viewers.get(&primary).copied() != Some(*client_id) {
                continue;
            }
            // W472: the working menu view - 54 stitched slots for
            // a double, written back to both halves after the op.
            let Some(mut view) =
                block::chest::menu_view(&vx.chests, primary, secondary)
            else {
                continue;
            };
            // W468: the viewer's cursor persists across acts in
            // `container_cursors` and rides back on the state
            // snapshot below - a Take no longer drops the stack
            // into a discarded temporary.
            // W472: creative-only ops validate against the
            // sender's announced game mode.
            let creative = vx
                .client_gamemodes
                .get(client_id)
                .copied()
                .unwrap_or(false);
            let cursor = vx.container_cursors.entry(*client_id).or_default();
            let mut thrown: Vec<crate::item::ItemStack> = Vec::new();
            match m.action {
                // W470: click-outside cursor throws (MC slotId
                // -999) act on the cursor alone.
                crate::net::ChestAct::ThrowCursorAll => {
                    thrown.extend(crate::container::click::throw_from_cursor(
                        cursor, true,
                    ));
                }
                crate::net::ChestAct::ThrowCursorOne => {
                    thrown.extend(crate::container::click::throw_from_cursor(
                        cursor, false,
                    ));
                }
                _ => {
                    let Some(action) = m.action.to_grid_action() else {
                        continue;
                    };
                    let mut no_inv: [Option<crate::item::ItemStack>; 0] = [];
                    if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
                        && let Some(inv) = inventories.get_mut(client_id)
                    {
                        // ClientInv is stack-native (W468): the apply
                        // helpers mutate it directly, components intact.
                        thrown = crate::container::screen::apply_grid_action(
                            &mut view,
                            &mut inv.inventory,
                            cursor,
                            m.player_side,
                            action,
                            &crate::container::screen::CHEST_CFG,
                            creative,
                        );
                        pending_inv_sends.insert(*client_id);
                    } else if !m.player_side {
                        // No per-client inventory (offline test harness):
                        // chest-side acts still run against the cursor;
                        // Transfer has no partner and no-ops.
                        thrown = crate::container::screen::apply_grid_action(
                            &mut view,
                            &mut no_inv,
                            cursor,
                            false,
                            action,
                            &crate::container::screen::CHEST_CFG,
                            creative,
                        );
                    }
                }
            }
            block::chest::menu_writeback(
                &mut vx.chests,
                primary,
                secondary,
                &view,
            );
            let cid = *client_id;
            vx.broadcast_chest_state(primary);
            let (eye, look) = client_eye_and_look(vx, cid);
            for st in thrown {
                crate::drops::spawn_thrown_stack(vx, st, eye, look);
            }
        }
    }

    /// W470: apply a batch of `vx.chest_drag` requests (completed
    /// drag-paints) through the shared click core against the
    /// per-viewer cursor.
pub(crate) fn apply_chest_drag_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ContainerDragMsg)]) {
        for (client_id, m) in msgs {
            // W472: same stitched-menu addressing as the acts.
            let (primary, secondary) = chest_canonical(vx, m.pos);
            if vx.chest_viewers.get(&primary).copied() != Some(*client_id) {
                continue;
            }
            let Some(mut view) =
                block::chest::menu_view(&vx.chests, primary, secondary)
            else {
                continue;
            };
            let cursor = vx.container_cursors.entry(*client_id).or_default();
            let visited: Vec<(bool, usize)> = m
                .slots
                .iter()
                .map(|&(side, i)| (side, i as usize))
                .collect();
            if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
                && let Some(inv) = inventories.get_mut(client_id)
            {
                crate::container::screen::apply_grid_drag(
                    &mut view,
                    &mut inv.inventory,
                    cursor,
                    &visited,
                    m.one_per_slot,
                );
                pending_inv_sends.insert(*client_id);
            } else {
                let mut no_inv: [Option<crate::item::ItemStack>; 0] = [];
                crate::container::screen::apply_grid_drag(
                    &mut view,
                    &mut no_inv,
                    cursor,
                    &visited,
                    m.one_per_slot,
                );
            }
            block::chest::menu_writeback(
                &mut vx.chests,
                primary,
                secondary,
                &view,
            );
            vx.broadcast_chest_state(primary);
        }
    }

    /// W5.3a: handle a batch of `vx.chest_close` requests. Clears
    /// the viewer registration + returns the viewer's cursor to
    /// their inventory (spill at the chest on overflow).
pub(crate) fn apply_chest_close_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ChestCloseMsg)]) {
        for (client_id, m) in msgs {
            // W472: viewer registration lives under the PRIMARY.
            let (primary, _) = chest_canonical(vx, m.pos);
            if vx.chest_viewers.get(&primary).copied() == Some(*client_id) {
                vx.chest_viewers.remove(&primary);
                recover_viewer_cursor(vx, *client_id, primary);
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
    /// W470: same shared-click-core routing + throw spawning.
pub(crate) fn apply_furnace_act_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::FurnaceActMsg)]) {
        for (client_id, m) in msgs {
            if vx.furnace_viewers.get(&m.pos).copied() != Some(*client_id) {
                continue;
            }
            let Some(furnace) = vx.furnaces.get_mut(&m.pos) else {
                continue;
            };
            // W472: creative flag for the wire CLONE arm.
            let creative = vx
                .client_gamemodes
                .get(client_id)
                .copied()
                .unwrap_or(false);
            // W468: same per-viewer cursor as the chest apply.
            let cursor = vx.container_cursors.entry(*client_id).or_default();
            let mut thrown: Vec<crate::item::ItemStack> = Vec::new();
            match m.action {
                crate::net::ChestAct::ThrowCursorAll => {
                    thrown.extend(crate::container::click::throw_from_cursor(
                        cursor, true,
                    ));
                }
                crate::net::ChestAct::ThrowCursorOne => {
                    thrown.extend(crate::container::click::throw_from_cursor(
                        cursor, false,
                    ));
                }
                _ => {
                    let Some(action) = m.action.to_grid_action() else {
                        continue;
                    };
                    let mut no_inv: [Option<crate::item::ItemStack>; 0] = [];
                    if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
                        && let Some(inv) = inventories.get_mut(client_id)
                    {
                        thrown = if m.player_side {
                            block::furnace::apply_player_action(
                                &mut inv.inventory,
                                furnace,
                                cursor,
                                action,
                                creative,
                            )
                        } else {
                            block::furnace::apply_furnace_action(
                                furnace,
                                &mut inv.inventory,
                                cursor,
                                action,
                                creative,
                            )
                        };
                        pending_inv_sends.insert(*client_id);
                    } else if !m.player_side {
                        thrown = block::furnace::apply_furnace_action(
                            furnace,
                            &mut no_inv,
                            cursor,
                            action,
                            creative,
                        );
                    }
                }
            }
            let pos = m.pos;
            let cid = *client_id;
            vx.broadcast_furnace_state(pos);
            let (eye, look) = client_eye_and_look(vx, cid);
            for st in thrown {
                crate::drops::spawn_thrown_stack(vx, st, eye, look);
            }
        }
    }

    /// W470: `vx.furnace_drag` applier - mirror of
    /// `apply_chest_drag_msgs`.
pub(crate) fn apply_furnace_drag_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::ContainerDragMsg)]) {
        for (client_id, m) in msgs {
            if vx.furnace_viewers.get(&m.pos).copied() != Some(*client_id) {
                continue;
            }
            let Some(furnace) = vx.furnaces.get_mut(&m.pos) else {
                continue;
            };
            let cursor = vx.container_cursors.entry(*client_id).or_default();
            let visited: Vec<(bool, usize)> = m
                .slots
                .iter()
                .map(|&(side, i)| (side, i as usize))
                .collect();
            if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net
                && let Some(inv) = inventories.get_mut(client_id)
            {
                crate::container::screen::apply_grid_drag(
                    furnace,
                    &mut inv.inventory,
                    cursor,
                    &visited,
                    m.one_per_slot,
                );
                pending_inv_sends.insert(*client_id);
            } else {
                let mut no_inv: [Option<crate::item::ItemStack>; 0] = [];
                crate::container::screen::apply_grid_drag(
                    furnace,
                    &mut no_inv,
                    cursor,
                    &visited,
                    m.one_per_slot,
                );
            }
            let pos = m.pos;
            vx.broadcast_furnace_state(pos);
        }
    }

    /// W470/W472: `vx.inv_op` applier - the FULL E-screen doClick
    /// set against the sender's authoritative `ClientInv` (craft /
    /// armor / main / hotbar) + per-viewer cursor, via the SAME
    /// shared core the local panel uses
    /// (`inventory::inv_screen_apply_op`). Throws spawn at the
    /// sender's pose; a fresh inventory snapshot is queued after
    /// every op.
pub(crate) fn apply_inv_op_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::InvOpMsg)]) {
        use crate::net::InvOpMsg;
        for (client_id, m) in msgs {
            let mut thrown: Vec<crate::item::ItemStack> = Vec::new();
            // Screen-close: recover the viewer's cursor into their
            // inventory (spill at their feet on overflow).
            if matches!(m, InvOpMsg::CloseScreen) {
                let (eye, _) = client_eye_and_look(vx, *client_id);
                let spill = [eye[0] as i32, eye[1] as i32, eye[2] as i32];
                recover_viewer_cursor(vx, *client_id, spill);
                continue;
            }
            let creative = vx
                .client_gamemodes
                .get(client_id)
                .copied()
                .unwrap_or(false);
            let cursor = vx.container_cursors.entry(*client_id).or_default();
            if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net {
                let Some(inv) = inventories.get_mut(client_id) else {
                    continue;
                };
                thrown = crate::inventory::inv_screen_apply_op(
                    &mut inv.craft_grid,
                    &mut inv.armor,
                    &mut inv.inventory,
                    cursor,
                    *m,
                    creative,
                );
                pending_inv_sends.insert(*client_id);
            }
            let cid = *client_id;
            let (eye, look) = client_eye_and_look(vx, cid);
            for st in thrown {
                crate::drops::spawn_thrown_stack(vx, st, eye, look);
            }
        }
    }

    /// W472: `vx.inv_drag` applier - completed E-screen drag-paint
    /// against the sender's `ClientInv` + cursor.
pub(crate) fn apply_inv_drag_msgs(vx: &mut crate::Voxel, msgs: &[(u32, crate::net::InvDragMsg)]) {
        for (client_id, m) in msgs {
            let cursor = vx.container_cursors.entry(*client_id).or_default();
            if let NetRole::Server { inventories, pending_inv_sends, .. } = &mut vx.net {
                let Some(inv) = inventories.get_mut(client_id) else {
                    continue;
                };
                let visited: Vec<crate::inventory::SlotKind> =
                    m.slots.iter().map(|&s| s.into()).collect();
                crate::inventory::inv_screen_apply_drag(
                    &mut inv.craft_grid,
                    &mut inv.armor,
                    &mut inv.inventory,
                    cursor,
                    &visited,
                    m.one_per_slot,
                );
                pending_inv_sends.insert(*client_id);
            }
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
        // W472: canonicalise so mutations reported for EITHER half
        // reach the viewer registered under the primary; a double
        // chest ships all 54 stitched slots (remote partner-half
        // edits included - the old local-only partner gap).
        let (primary, secondary) = block::chest::canonical_pair(
            &vx.world,
            &vx.chests,
            &vx.chest_facing,
            pos,
        );
        let Some(&viewer) = vx.chest_viewers.get(&primary) else {
            return;
        };
        let Some(view) =
            block::chest::menu_view(&vx.chests, primary, secondary)
        else {
            return;
        };
        let msg = crate::net::ChestStateMsg {
            pos: primary,
            slots: view.slots().to_vec(),
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
