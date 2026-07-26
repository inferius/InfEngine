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
        BlockId, CHEST_ID, EditMsg, REDSTONE_TORCH_ID,
        REDSTONE_WALL_TORCH_ID, SOUL_TORCH_ID, SOUL_WALL_TORCH_ID,
        TORCH_ID, Voxel, block, block_is_placement_blocked,
        block_is_replaceable, block_material, body_aabb,
    };
    // Pick the destination voxel.
    // W411: slab-on-slab merge (MC SlabBlock.canBeReplaced): the
    // same slab clicked on its exposed half fills the CLICKED cell
    // to a DOUBLE slab instead of placing into the next cell.
    if crate::block::is_slab_block(block.0)
        && vx.world.get(hit.voxel).0 == block.0
    {
        let st = vx
            .stair_states
            .get(&hit.voxel)
            .copied()
            .unwrap_or(0);
        let top = st & crate::block::door::TRAPDOOR_TOP_BIT != 0;
        let mergeable = st & crate::block::door::SLAB_DOUBLE_BIT == 0
            && ((!top && hit.normal[1] > 0) || (top && hit.normal[1] < 0));
        if mergeable {
            vx.stair_states
                .insert(hit.voxel, st | crate::block::door::SLAB_DOUBLE_BIT);
            let coords = Voxel::affected_chunks(hit.voxel);
            vx.rebuild_chunks(&coords);
            if !is_client
                && !vx.abilities.instabuild
                && let Some((_, count)) =
                    vx.inventory[vx.selected_slot].as_mut()
            {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    vx.inventory[vx.selected_slot] = None;
                }
            }
            let key = format!("dig:{}", block_material(block));
            let freq = 380.0 + (block.0 as f32) * 22.0;
            let c = [
                hit.voxel[0] as f32 + 0.5,
                hit.voxel[1] as f32 + 0.5,
                hit.voxel[2] as f32 + 0.5,
            ];
            vx.sfx_at(&key, c, move |a, g| {
                let _ = a.play_click(freq, 0.18 * g);
            });
            return;
        }
    }
    let target_voxel = if block_is_replaceable(vx.world.get(hit.voxel))
        && vx.world.get(hit.voxel).0 != block.0
    {
        hit.voxel
    } else {
        hit.place_at
    };
    // Reject when the new voxel would intersect the player capsule.
    // W127: blocks WITHOUT a collision box (rails, flowers, ...) may
    // place inside the player's space, exactly like MC - the check
    // only guards blocks the player would get stuck in.
    let (min, max) = body_aabb(vx.player_pos);
    let p = target_voxel;
    let voxel_min = [p[0] as f32, p[1] as f32, p[2] as f32];
    let voxel_max = [p[0] as f32 + 1.0, p[1] as f32 + 1.0, p[2] as f32 + 1.0];
    let overlaps = !crate::block::block_is_aabb_passable(block)
        && max.x > voxel_min[0]
        && min.x < voxel_max[0]
        && max.y > voxel_min[1]
        && min.y < voxel_max[1]
        && max.z > voxel_min[2]
        && min.z < voxel_max[2];
    // Block-occupied check. W144: a REPLACEABLE block (water, snow
    // layer, short grass / fern / dead bush) never blocks placement
    // even when it also carries F_NO_PLACE - building into a grass
    // cell simply overwrites the plant, like MC.
    let target_block = vx.world.get(target_voxel);
    let occupied = !block_is_replaceable(target_block)
        && (block_is_placement_blocked(target_block) || !target_block.is_air());
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
    // W142: clicking a rail face places into the adjacent cell,
    // exactly like MC (rails are not replaceable, so the click
    // offsets along the hit face; full blocks need no support in
    // vanilla, and a new RAIL in that cell is still gated by
    // rail_unsupported below). The W131 blanket "no building off
    // rails" rejection was stricter than vanilla - extending a
    // line by clicking the last rail's end face must work.
    //
    // W130: rails need a solid floor (MC support rule). This also
    // kills rail-on-rail vertical stacking - a rail below is not a
    // full solid block.
    let rail_unsupported = crate::block::rail::is_rail_block(block.0) && {
        let below = vx.world.get([
            target_voxel[0],
            target_voxel[1] - 1,
            target_voxel[2],
        ]);
        below.is_air() || !crate::block::block_fully_occludes(below)
    };
    // W367: a bed occupies TWO cells - the head goes one cell
    // along the player's look facing (MC BedBlock: head at
    // pos.relative(facing)). MC refuses the whole placement when
    // the head cell is taken, so reject up front (no partial foot).
    let bed_head_blocked = crate::block::is_bed_block(block.0) && {
        let f = crate::block::door::facing_from_look(
            vx.player_yaw.cos(),
            -vx.player_yaw.sin(),
        );
        let d: [i32; 2] = match f {
            0 => [1, 0],
            1 => [0, 1],
            2 => [-1, 0],
            _ => [0, -1],
        };
        !vx.world
            .get([
                target_voxel[0] + d[0],
                target_voxel[1],
                target_voxel[2] + d[1],
            ])
            .is_air()
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
    if overlaps || torch_unsupported || occupied || rail_unsupported || bed_head_blocked {
        // W127: reject reasons at debug level - "I clicked and
        // nothing happened" reports are undiagnosable without this.
        tracing::debug!(
            ?target_voxel,
            block = block.0,
            overlaps,
            occupied,
            torch_unsupported,
            rail_unsupported,
            bed_head_blocked,
            "block place rejected",
        );
        return;
    }
    tracing::debug!(?target_voxel, block = placed_block.0, "block placed");
    // W65.4: a planted crop starts at its immature stage and grows
    // via random tick. Swap before the edit is built / broadcast so
    // every peer sees the young sprite.
    let placed_block = match crate::world::crop::immature_for(placed_block.0) {
        Some(immature) => BlockId(immature),
        None => placed_block,
    };
    // W294: crops only plant on FARMLAND (CropBlock.mayPlaceOn) -
    // seeds bounce off bare dirt exactly like vanilla; the hoe makes
    // the soil first.
    if crate::world::crop::is_crop(placed_block.0) {
        let below = vx
            .world
            .get([target_voxel[0], target_voxel[1] - 1, target_voxel[2]])
            .0;
        if below != crate::block::crop_stages::FARMLAND_ID {
            return;
        }
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
    // W365: pillar blocks take their axis from the CLICKED face
    // (MC RotatedPillarBlock.getStateForPlacement) - click a wall
    // and the log lies toward you, click the ground and it stands
    // up. Only horizontal axes need a record.
    // W411: slabs carry their half in `stair_states` (MC SlabBlock
    // `type` prop; the world grid keeps one id per material). Only
    // the TOP half needs a record - bottom is the default.
    if crate::block::is_slab_block(placed_block.0) {
        let look = [
            vx.player_yaw.cos() * vx.player_pitch.cos(),
            vx.player_pitch.sin(),
            -vx.player_yaw.sin() * vx.player_pitch.cos(),
        ];
        let st = crate::block::door::slab_place_state(
            hit.normal,
            target_voxel,
            [vx.player_pos.x, vx.player_pos.y, vx.player_pos.z],
            look,
        );
        if st != 0 {
            vx.stair_states.insert(target_voxel, st);
        }
    }
    if crate::block::is_pillar_block(placed_block.0) {
        let ax = if hit.normal[0] != 0 {
            1u8
        } else if hit.normal[2] != 0 {
            2
        } else {
            0
        };
        if ax != 0 {
            vx.stair_states.insert(target_voxel, ax);
        }
    }
    if crate::block::door::is_door_block(placed_block.0)
        || crate::block::door::is_trapdoor_block(placed_block.0)
        // W167: fence gates store facing too (door-style, horizontal).
        || crate::block::door::is_fence_gate_block(placed_block.0)
    {
        // W125/W129: record the door OR trapdoor facing. Doors face
        // the way the player faces. W133: trapdoors follow the MC
        // placement rules instead - facing from the clicked face,
        // half (bit 2) from the click height / clicked face - so
        // side-placing high on a wall yields a ceiling trapdoor
        // that swings DOWN when opened. Keyed by the placed cell;
        // broadcast so every peer renders the same panel.
        let facing = if crate::block::door::is_trapdoor_block(placed_block.0)
        {
            let look = [
                vx.player_yaw.cos() * vx.player_pitch.cos(),
                vx.player_pitch.sin(),
                -vx.player_yaw.sin() * vx.player_pitch.cos(),
            ];
            crate::block::door::trapdoor_place_state(
                hit.normal,
                target_voxel,
                [vx.player_pos.x, vx.player_pos.y, vx.player_pos.z],
                look,
            )
        } else {
            crate::block::door::facing_from_look(
                vx.player_yaw.cos(),
                -vx.player_yaw.sin(),
            )
        };
        vx.door_facing.insert(target_voxel, facing);
        let door_state_payload: ([i32; 3], u8, bool) =
            (target_voxel, facing, false);
        match &mut vx.net {
            NetRole::Offline => {}
            NetRole::Server { server, .. } => {
                let cfg = bincode::config::standard();
                if let Ok(buf) =
                    bincode::serde::encode_to_vec(door_state_payload, cfg)
                {
                    server.broadcast_bytes("vx.door_state", buf);
                }
            }
            NetRole::Client { client, .. } => {
                let cfg = bincode::config::standard();
                if let Ok(buf) =
                    bincode::serde::encode_to_vec(door_state_payload, cfg)
                {
                    let _ = client.send_bytes("vx.door_state", buf);
                }
            }
        }
        let upper_pos = [target_voxel[0], target_voxel[1] + 1, target_voxel[2]];
        if crate::block::door::is_door_block(placed_block.0)
            && vx.world.get(upper_pos).is_air()
        {
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
    // W367: place the bed's HEAD half + record both piece states
    // (the reject above guarantees the cell is free). The foot cell
    // re-meshes so its wool-cube fallback swaps to the entity render
    // now that a state exists.
    if crate::block::is_bed_block(placed_block.0) {
        let f = crate::block::door::facing_from_look(
            vx.player_yaw.cos(),
            -vx.player_yaw.sin(),
        );
        let d: [i32; 2] = match f {
            0 => [1, 0],
            1 => [0, 1],
            2 => [-1, 0],
            _ => [0, -1],
        };
        let head_pos = [
            target_voxel[0] + d[0],
            target_voxel[1],
            target_voxel[2] + d[1],
        ];
        vx.world_set_tracked(head_pos, placed_block);
        vx.bed_states.insert(target_voxel, f);
        vx.bed_states
            .insert(head_pos, f | crate::block::BED_HEAD_BIT);
        vx.rebuild_chunks(&Voxel::affected_chunks(head_pos));
        vx.rebuild_chunks(&Voxel::affected_chunks(target_voxel));
        let head_edit = EditMsg {
            x: head_pos[0],
            y: head_pos[1],
            z: head_pos[2],
            block: placed_block.0,
        };
        match &mut vx.net {
            NetRole::Offline => {}
            NetRole::Server { server, .. } => {
                let cfg = bincode::config::standard();
                if let Ok(buf) = bincode::serde::encode_to_vec(head_edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
                vx.edit_history.push(head_edit);
            }
            NetRole::Client { pending_edits, .. } => {
                pending_edits.push(head_edit);
            }
        }
    }
    // W143: a placed rail decides its shape ONCE (MC RailState port)
    // and reshapes at most the neighbours it connects to. Stored +
    // broadcast so established junctions never re-bend from later
    // placements nearby.
    if crate::block::rail::is_rail_block(placed_block.0) {
        super::rail_place::on_rail_placed(vx, target_voxel, placed_block.0);
    }
    // Chest place: empty 27-slot container.
    if block.0 == CHEST_ID {
        vx.chest_render_positions.insert(target_voxel);
        vx.chests
            .entry(target_voxel)
            .or_insert_with(block::chest::new_chest);
        // W126: the latch faces the placer (MC chest placement) -
        // the OPPOSITE of the player's look direction. Broadcast so
        // every peer renders the same yaw + double-chest pairing.
        let facing = crate::block::door::closed_edge(
            crate::block::door::facing_from_look(
                vx.player_yaw.cos(),
                -vx.player_yaw.sin(),
            ),
        );
        vx.chest_facing.insert(target_voxel, facing);
        let payload: ([i32; 3], u8) = (target_voxel, facing);
        match &mut vx.net {
            NetRole::Offline => {}
            NetRole::Server { server, .. } => {
                let cfg = bincode::config::standard();
                if let Ok(buf) = bincode::serde::encode_to_vec(payload, cfg) {
                    server.broadcast_bytes("vx.chest_facing", buf);
                }
            }
            NetRole::Client { client, .. } => {
                let cfg = bincode::config::standard();
                if let Ok(buf) = bincode::serde::encode_to_vec(payload, cfg) {
                    let _ = client.send_bytes("vx.chest_facing", buf);
                }
            }
        }
    }
    // Furnace place: empty 3-slot container + zeroed runtime.
    // W163: smoker + blast furnace share the furnace state.
    if block::furnace::is_furnace_like(block.0) {
        block::furnace::place_furnace(
            &mut vx.furnaces,
            &mut vx.furnace_runtimes,
            target_voxel,
        );
    }
    // W172: chests / barrels / ender chests / hoppers mint their
    // container on place so a hopper can pull/push even into a
    // never-opened one. or_insert keeps any loaded save state.
    if block.0 == CHEST_ID
        || block.0 == crate::block::BARREL_ID
        || block.0 == crate::block::ENDER_CHEST_ID
    {
        vx.chests.entry(target_voxel).or_insert_with(block::chest::new_chest);
    }
    if block.0 == crate::block::HOPPER_ID {
        vx.chests
            .entry(target_voxel)
            .or_insert_with(|| crate::container::Container::with_size(block::hopper::HOPPER_SLOTS));
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
    // W65.3: a player-placed leaf is persistent (exempt from
    // random-tick decay), mirroring MC's persistent leaf flag. Host
    // side only - the decay pass runs on the host.
    if !is_client && placed_block.0 == crate::block::LEAVES_ID {
        vx.persistent_leaves.insert(target_voxel);
    }
    // W67.1: placing a redstone block (wire / source / lamp) re-solves
    // the local network so lamps update immediately.
    if !is_client && crate::world::redstone::is_redstone_block(placed_block.0) {
        crate::world::redstone::resolve(vx, target_voxel);
    }
    vx.blocks_placed = vx.blocks_placed.saturating_add(1);
    vx.place_history.push(target_voxel);
    if vx.place_history.len() > 32 {
        let drop = vx.place_history.len() - 32;
        vx.place_history.drain(0..drop);
    }
}
