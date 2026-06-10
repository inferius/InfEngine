//! W47.14 - break pipeline submodule (split from W47.13 monolith).
//!
//! Contains the two helpers that mutate the world toward a destructive
//! / degrading outcome:
//! - `apply_block_break` (W47.13d) - full block-break pipeline
//!   (world set + chunk rebuild + drops + broadcast + SFX) used by
//!   the LMB hold-to-mine completion path.
//! - `apply_shovel_grass_path` (W47.13f) - shovel-RMB on grass turns
//!   the cell into dirt (the MC "path" shortcut).

/// W47.13d: apply the final block-break edit for `voxel`.
///
/// Called once when `mine_break == Some(voxel)` (mining session
/// completed) and any panel guards have already passed in the
/// caller. Side effects:
/// - sets the voxel to AIR (locally) + rebuilds affected chunks
/// - sprays break particles tinted to the broken block's albedo
/// - broadcasts the edit on Server / queues it on Client
/// - on host: spawns the item drop (suppressed in instabuild or
///   when held-tool-tier < required), drains chest / furnace
///   contents as drops, dislodges any decorations losing support
/// - increments `blocks_broken` counter
/// - plays the final break SFX
///
/// `is_client` mirrors the caller's `matches!(self.net,
/// NetRole::Client { .. })`; passed in to keep the helper from
/// re-matching on `self.net` twice.
pub(crate) fn apply_block_break(
    vx: &mut crate::Voxel,
    voxel: [i32; 3],
    is_client: bool,
) {
    use crate::net::NetRole;
    use crate::{
        BlockId, CHEST_ID, EditMsg, FURNACE_ID, REDSTONE_TORCH_ID,
        REDSTONE_WALL_TORCH_ID, SOUL_TORCH_ID, SOUL_WALL_TORCH_ID, Voxel,
        block, block_material, block_required_tier, tool_tier,
    };
    let _ = std::marker::PhantomData::<Voxel>;
    // Capture the block we're about to break so we can
    // pop it into the inventory (greedy collection - air just no-ops).
    let broken = vx.world.get(voxel);
    // W63.3 - door 2-block break. If the broken voxel is a door
    // half, also break the sibling half (one cell above for the
    // lower half, one cell below for the upper half). Clear
    // door_open state too. Done BEFORE the normal break so both
    // edits get broadcast in sequence.
    if crate::block::door::is_door_block(broken.0) {
        let sibling = {
            let above = [voxel[0], voxel[1] + 1, voxel[2]];
            let below = [voxel[0], voxel[1] - 1, voxel[2]];
            if crate::block::door::is_door_block(vx.world.get(above).0) {
                Some(above)
            } else if crate::block::door::is_door_block(vx.world.get(below).0) {
                Some(below)
            } else {
                None
            }
        };
        if let Some(sib) = sibling {
            let lower = std::cmp::min(voxel[1], sib[1]);
            vx.door_open
                .remove(&[voxel[0], lower, voxel[2]]);
            // W125: drop the facing record with the door.
            vx.door_facing
                .remove(&[voxel[0], lower, voxel[2]]);
            vx.world_set_tracked(sib, BlockId::AIR);
            let coords = Voxel::affected_chunks(sib);
            vx.rebuild_chunks(&coords);
            let sib_edit = EditMsg {
                x: sib[0],
                y: sib[1],
                z: sib[2],
                block: 0,
            };
            match &mut vx.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    let cfg = bincode::config::standard();
                    if let Ok(buf) = bincode::serde::encode_to_vec(sib_edit, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                    vx.edit_history.push(sib_edit);
                }
                NetRole::Client { pending_edits, .. } => {
                    pending_edits.push(sib_edit);
                }
            }
        } else {
            vx.door_open.remove(&voxel);
        }
    }
    // Tint the debris with the broken block's albedo.
    let break_color = vx
        .registry
        .get(broken)
        .map(|d| d.color)
        .unwrap_or([0.75, 0.55, 0.30]);
    let edit = EditMsg {
        x: voxel[0],
        y: voxel[1],
        z: voxel[2],
        block: 0,
    };
    if is_client {
        vx.world_set_tracked(voxel, BlockId::AIR);
        let coords = Voxel::affected_chunks(voxel);
        vx.rebuild_chunks(&coords);
        vx.spawn_break_particles_tinted(voxel, break_color);
        if let NetRole::Client { pending_edits, .. } = &mut vx.net {
            pending_edits.push(edit);
        }
    } else {
        vx.world_set_tracked(voxel, BlockId::AIR);
        let coords = Voxel::affected_chunks(voxel);
        vx.rebuild_chunks(&coords);
        vx.spawn_break_particles_tinted(voxel, break_color);
        if let NetRole::Server { server, .. } = &vx.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                server.broadcast_bytes("vx.edit", buf);
            }
        }
        vx.edit_history.push(edit);
    }
    if !is_client && broken != BlockId::AIR {
        // W10.2: Creative `instabuild` suppresses block drops.
        // Container contents (chest, furnace) still drop below.
        let suppress_drops = vx.abilities.instabuild;
        // Tool gating: drop only when the selected slot's tool tier
        // meets the block's requirement.
        let held = vx
            .inventory
            .get(vx.selected_slot)
            .copied()
            .unwrap_or(None)
            .map(|(b, _)| b)
            .unwrap_or(BlockId::AIR);
        let tier = tool_tier(held);
        let needed = block_required_tier(broken);
        // W65.4: immature crops have no item form - harvest before
        // maturity drops nothing (MC drops only seeds early; we keep
        // it simple). Mature crops fall through to the normal drop.
        if !suppress_drops
            && tier >= needed
            && !crate::world::crop::is_immature_crop(broken.0)
        {
            // W35.2: wall-torch variants drop the floor-torch item.
            let drop_id = match broken.0 {
                x if x == REDSTONE_WALL_TORCH_ID => REDSTONE_TORCH_ID,
                x if x == SOUL_WALL_TORCH_ID => SOUL_TORCH_ID,
                _ => broken.0,
            };
            vx.spawn_drop(BlockId(drop_id), voxel);
        }
        // Chest break: drain contents as drops, drop the container.
        if broken.0 == CHEST_ID
            && let Some(chest_inv) = vx.chests.remove(&voxel)
        {
            for id in block::chest::drain_drops(&chest_inv) {
                vx.spawn_drop(BlockId(id), voxel);
            }
        }
        // Furnace break: drain 3 slots + drop the runtime entry.
        if broken.0 == FURNACE_ID {
            let drops = block::furnace::break_furnace(
                &mut vx.furnaces,
                &mut vx.furnace_runtimes,
                voxel,
            );
            for id in drops {
                vx.spawn_drop(BlockId(id), voxel);
            }
        }
        // Cascade: dislodge any F_NO_PLACE decoration whose support
        // we just removed.
        vx.dislodge_unsupported_decorations(voxel);
        // W64: a falling block (sand / gravel / concrete_powder)
        // resting directly on the removed cell just lost its floor -
        // wake it so the next falling tick drops it. Host-only; the
        // tick filters non-falling cells.
        vx.falling_pending.insert([voxel[0], voxel[1] + 1, voxel[2]]);
        // W67.1: breaking a redstone block (wire / source / lamp)
        // re-solves the local network so dependent lamps update. The
        // broken cell is already air (world_set_tracked above), so the
        // resolve seeds from its neighbours.
        if crate::world::redstone::is_redstone_block(broken.0) {
            crate::world::redstone::resolve(vx, voxel);
        }
    }
    if broken != BlockId::AIR {
        vx.blocks_broken = vx.blocks_broken.saturating_add(1);
    }
    // Final break SFX.
    let voxel_center = [
        voxel[0] as f32 + 0.5,
        voxel[1] as f32 + 0.5,
        voxel[2] as f32 + 0.5,
    ];
    let key = format!("dig:{}", block_material(broken));
    let freq = 220.0 + (broken.0 as f32) * 28.0;
    vx.sfx_at(&key, voxel_center, move |a, g| {
        let _ = a.play_thud(freq, 0.30 * g);
    });
}

/// W47.13f: degrade the grass voxel at `voxel` to dirt (id 2). Host
/// applies the edit locally + broadcasts `vx.edit`; client applies
/// the same local edit (so the path appears instantly under the
/// player's feet) and sends a `vx.path` request that the server will
/// replay or reject.
pub(crate) fn apply_shovel_grass_path(vx: &mut crate::Voxel, voxel: [i32; 3]) {
    use crate::net::NetRole;
    use crate::{BlockId, EditMsg, Voxel};
    let cfg = bincode::config::standard();
    vx.world_set_tracked(voxel, BlockId(2));
    let coords = Voxel::affected_chunks(voxel);
    vx.rebuild_chunks(&coords);
    let edit = EditMsg {
        x: voxel[0],
        y: voxel[1],
        z: voxel[2],
        block: 2,
    };
    match &mut vx.net {
        NetRole::Offline => {}
        NetRole::Server { server, .. } => {
            vx.edit_history.push(edit);
            if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                server.broadcast_bytes("vx.edit", buf);
            }
        }
        NetRole::Client { client, .. } => {
            if let Ok(buf) = bincode::serde::encode_to_vec(voxel, cfg) {
                let _ = client.send_bytes("vx.path", buf);
            }
        }
    }
}
