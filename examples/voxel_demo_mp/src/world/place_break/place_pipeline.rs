//! W47.14 - place pipeline submodule (split from W47.13 monolith).
//!
//! Contains `apply_block_place` (W47.13e), the full RMB
//! block-placement path including target voxel resolution, AABB /
//! torch-support / occupied gates, torch -> wall_torch auto-swap,
//! edit + torch-orientation broadcasts, container / runtime entry
//! mint, held-stack decrement, place SFX, dust puff, and counters.

/// W47.13e: apply the RMB block-place edit at the right destination
/// for `block`, given the ray hit + the client/host role.
///
/// The caller has already established that `predicted ==
/// RmbAction::PlaceBlock` (see `classify_rmb_action`) and that
/// `selected_block` is `Some(block)`. This helper reproduces every
/// side effect the original inline block did:
/// - resolves the destination voxel (hit.voxel for replaceable
///   cells, hit.place_at otherwise)
/// - rejects when the destination overlaps the player AABB
/// - rejects torches without a solid floor or wall neighbour
/// - swaps torch -> wall_torch when the hit normal is horizontal
/// - writes the edit locally + broadcasts (server) or pushes to
///   the client's pending edits
/// - records + broadcasts torch orientation
/// - mints empty chest / furnace containers
/// - decrements the held stack (host only, not creative)
/// - plays the place SFX + place dust puff
/// - bumps `blocks_placed` counter + appends to `place_history`
pub(crate) fn apply_block_place(
    vx: &mut crate::Voxel,
    hit: &infengine_voxel::RayHit,
    block: crate::BlockId,
    is_client: bool,
) {
    use crate::net::NetRole;
    use crate::{
        BlockId, CHEST_ID, EditMsg, FURNACE_ID, REDSTONE_TORCH_ID,
        REDSTONE_WALL_TORCH_ID, SOUL_TORCH_ID, SOUL_WALL_TORCH_ID,
        TORCH_ID, Voxel, block, block_is_placement_blocked,
        block_is_replaceable, block_material, body_aabb,
    };
    // Pick the destination voxel.
    let target_voxel = if block_is_replaceable(vx.world.get(hit.voxel))
        && vx.world.get(hit.voxel).0 != block.0
    {
        hit.voxel
    } else {
        hit.place_at
    };
    // Reject when the new voxel would intersect the player capsule.
    let (min, max) = body_aabb(vx.player_pos);
    let p = target_voxel;
    let voxel_min = [p[0] as f32, p[1] as f32, p[2] as f32];
    let voxel_max = [p[0] as f32 + 1.0, p[1] as f32 + 1.0, p[2] as f32 + 1.0];
    let overlaps = max.x > voxel_min[0]
        && min.x < voxel_max[0]
        && max.y > voxel_min[1]
        && min.y < voxel_max[1]
        && max.z > voxel_min[2]
        && min.z < voxel_max[2];
    // Block-occupied check.
    let target_block = vx.world.get(target_voxel);
    let occupied = block_is_placement_blocked(target_block)
        || (!target_block.is_air() && !block_is_replaceable(target_block));
    // Torch support: floor or wall neighbour must be solid.
    let is_torch_item = matches!(
        block.0,
        x if x == TORCH_ID
            || x == REDSTONE_TORCH_ID
            || x == SOUL_TORCH_ID
    );
    let torch_unsupported = is_torch_item && {
        let q = target_voxel;
        let unsupportive = |b: BlockId| {
            b.is_air() || block_is_placement_blocked(b)
        };
        let below_blk = vx.world.get([q[0], q[1] - 1, q[2]]);
        let below_air = unsupportive(below_blk);
        let walls_clear = [
            [q[0] - 1, q[1], q[2]],
            [q[0] + 1, q[1], q[2]],
            [q[0], q[1], q[2] - 1],
            [q[0], q[1], q[2] + 1],
        ]
        .iter()
        .all(|v| unsupportive(vx.world.get(*v)));
        below_air && walls_clear
    };
    // Auto-switch torch -> wall_torch on horizontal placements.
    let placed_block = if hit.normal[1] == 0 {
        match block.0 {
            x if x == REDSTONE_TORCH_ID => BlockId(REDSTONE_WALL_TORCH_ID),
            x if x == SOUL_TORCH_ID => BlockId(SOUL_WALL_TORCH_ID),
            _ => block,
        }
    } else {
        block
    };
    if overlaps || torch_unsupported || occupied {
        return;
    }
    let edit = EditMsg {
        x: target_voxel[0],
        y: target_voxel[1],
        z: target_voxel[2],
        block: placed_block.0,
    };
    // Sticky torch orientation from the ray hit normal.
    let torch_orient_msg: Option<([i32; 3], u8)> = if block.0 == TORCH_ID {
        let n = hit.normal;
        let orient = if n[1] > 0 {
            0u8
        } else if n[0] > 0 {
            1
        } else if n[0] < 0 {
            2
        } else if n[2] > 0 {
            3
        } else if n[2] < 0 {
            4
        } else {
            0
        };
        vx.torch_orient.insert(target_voxel, orient);
        Some((target_voxel, orient))
    } else {
        None
    };
    if is_client {
        vx.world_set_tracked(target_voxel, placed_block);
        let coords = Voxel::affected_chunks(target_voxel);
        vx.rebuild_chunks(&coords);
        if let NetRole::Client {
            client,
            pending_edits,
            ..
        } = &mut vx.net
        {
            pending_edits.push(edit);
            if let Some(payload) = torch_orient_msg
                && let Ok(buf) = bincode::serde::encode_to_vec(
                    payload,
                    bincode::config::standard(),
                )
            {
                let _ = client.send_bytes("vx.torch_orient", buf);
            }
        }
    } else {
        vx.world_set_tracked(target_voxel, placed_block);
        let coords = Voxel::affected_chunks(target_voxel);
        vx.rebuild_chunks(&coords);
        if let NetRole::Server { server, .. } = &vx.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                server.broadcast_bytes("vx.edit", buf);
            }
            if let Some(payload) = torch_orient_msg
                && let Ok(buf) = bincode::serde::encode_to_vec(payload, cfg)
            {
                server.broadcast_bytes("vx.torch_orient", buf);
            }
        }
        vx.edit_history.push(edit);
    }
    // W62.2b - door 2-block-tall place. When the placed block is
    // a door, also stamp the upper-half block in the cell above.
    // Wooden oak door (77) uses the dedicated upper id 78; every
    // other door variant (W48 birch/spruce/jungle/...) reuses the
    // same id for both halves since we don't ship per-variant
    // upper ids yet.
    if crate::block::door::is_door_block(placed_block.0) {
        let upper_pos = [target_voxel[0], target_voxel[1] + 1, target_voxel[2]];
        if vx.world.get(upper_pos).is_air() {
            let upper_id = if placed_block.0 == 77 { 78 } else { placed_block.0 };
            vx.world_set_tracked(upper_pos, BlockId(upper_id));
            let upper_coords = Voxel::affected_chunks(upper_pos);
            vx.rebuild_chunks(&upper_coords);
            let upper_edit = EditMsg {
                x: upper_pos[0],
                y: upper_pos[1],
                z: upper_pos[2],
                block: upper_id,
            };
            match &mut vx.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    let cfg = bincode::config::standard();
                    if let Ok(buf) = bincode::serde::encode_to_vec(upper_edit, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                    vx.edit_history.push(upper_edit);
                }
                NetRole::Client { pending_edits, .. } => {
                    pending_edits.push(upper_edit);
                }
            }
        }
    }
    // Chest place: empty 27-slot container.
    if block.0 == CHEST_ID {
        vx.chests
            .entry(target_voxel)
            .or_insert_with(block::chest::new_chest);
    }
    // Furnace place: empty 3-slot container + zeroed runtime.
    if block.0 == FURNACE_ID {
        block::furnace::place_furnace(
            &mut vx.furnaces,
            &mut vx.furnace_runtimes,
            target_voxel,
        );
    }
    // Decrement the active stack (host only, not creative).
    if !is_client
        && !vx.abilities.instabuild
        && let Some((_, count)) = vx.inventory[vx.selected_slot].as_mut()
    {
        *count = count.saturating_sub(1);
        if *count == 0 {
            vx.inventory[vx.selected_slot] = None;
        }
    }
    // Place SFX (re-uses the `dig:*` material bank).
    let key = format!("dig:{}", block_material(block));
    let freq = 380.0 + (block.0 as f32) * 22.0;
    let place_center = [
        target_voxel[0] as f32 + 0.5,
        target_voxel[1] as f32 + 0.5,
        target_voxel[2] as f32 + 0.5,
    ];
    vx.sfx_at(&key, place_center, move |a, g| {
        let _ = a.play_click(freq, 0.18 * g);
    });
    // Albedo-tinted dust puff.
    let color = vx
        .registry
        .get(block)
        .map(|d| d.color)
        .unwrap_or([0.7, 0.7, 0.7]);
    vx.spawn_place_particles(target_voxel, color);
    // W64: a freshly placed falling block starts falling if the
    // cell beneath it is empty. Host-only enqueue (the tick gates
    // on NetRole); the destination + chain are re-queued by the
    // tick itself.
    if !is_client && crate::world::falling::is_falling_block(placed_block.0) {
        vx.falling_pending.insert(target_voxel);
    }
    vx.blocks_placed = vx.blocks_placed.saturating_add(1);
    vx.place_history.push(target_voxel);
    if vx.place_history.len() > 32 {
        let drop = vx.place_history.len() - 32;
        vx.place_history.drain(0..drop);
    }
}
