//! W47.13 - place / break decision helpers extracted from `on_update`.
//!
//! The block-edit path inside `Voxel::on_update` is a ~1100 LOC
//! sequence with deep `&mut self` borrows + side effects (chunk
//! rebuilds, edit broadcasts, particle spawns, inventory writes).
//! Extracting the entire flow at once is high-risk; this module
//! instead grows piece-by-piece with pure free functions that the
//! main loop calls. Each helper takes only the fields it needs so
//! the caller can keep other borrows alive.
//!
//! W47.13a: `resolve_attack_target` - given the three candidate hits
//! (remote player, mob, voxel), return whichever wins by closest
//! distance. Pure data; no engine dependencies.
//!
//! W47.13c: `classify_rmb_action` - given the held block, target
//! block id, and ability flags, return which RMB branch should fire.
//! Replaces the inline `else if rmb && self.world.get(...) == X`
//! conditions in the main on_update cascade. Branch bodies (side
//! effects) stay in the caller.
//!
//! W47.13d: `apply_block_break` - the full break-edit pipeline
//!     (world set + chunk rebuild + particles + edit broadcast + drops
//!     + container drain + dislodge cascade + break SFX). Takes `&mut
//!     Voxel` because the body touches ~15 fields; the win is that the
//!     130-line block stops sitting inline in `on_update`.
//!
//! W47.13e: `apply_block_place` - the full place pipeline
//!     (target voxel resolve + replaceable / overlap / torch-support
//!     gates + torch wall-variant swap + edit broadcast + torch orient
//!     broadcast + container mint + held-stack decrement + place SFX +
//!     dust puff + counters). Same `&mut Voxel` shape as the break
//!     helper since the body fans out across ~15 fields.
//!
//! W47.13f: `apply_shovel_grass_path` - shovel-RMB on grass turns
//!     the cell into dirt (the MC "path" shortcut). Local edit +
//!     server broadcast `vx.edit` or client request `vx.path`.
//!
//! W47.13g: `apply_flint_and_steel_ignite` - flint+steel RMB tries
//!     to light a portal frame at the clicked face; on Ok scans the
//!     3x3x3 area for new portal cells + broadcasts each. On
//!     NotRectangular / PerimeterIncomplete it posts a chat warning;
//!     on Reject (no frame) it drops a fire cell at the clicked
//!     position. Always consumes one flint+steel durability use.
//!
//! W47.13h: `apply_bucket_use` - empty bucket scoops a source
//!     liquid; filled bucket pours liquid onto an air / replaceable
//!     cell (also extinguishes fire). Handles creative-mode (no
//!     bucket consume / no source remove) + liquid level broadcast.
//!
//! W47.13i: `open_furnace`, `open_chest`, `try_sleep_bed` - the
//!     small UI-opener branches. Each mints (or finds) the local
//!     container, sets the open marker on `Voxel`, and on
//!     `NetRole::Client` sends the matching open / sleep request
//!     to the host. The sleep branch also gates on the day phase
//!     + posts the right chat line.

/// Outcome of the per-frame combat-vs-block-edit arbitration.
///
/// At most one of `player` / `mob` is set; if `voxel_dist` was the
/// closest hit (or no candidate beat infinity) both are `None` and
/// the caller falls through to the block-break / place path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttackTarget {
    pub player: Option<u32>,
    pub mob: Option<u32>,
}

/// Pick whichever attack target is closest: player < mob < voxel.
///
/// The original main.rs sequence walked the three candidates in
/// (pvp, mob, voxel) order, each guarded by `d < best_dist`. The
/// voxel branch only resets the two combat outputs - it never sets
/// them - because a closer voxel means "let the block-edit path
/// handle it" rather than "attack the voxel".
pub fn resolve_attack_target(
    pvp_target: Option<(u32, f32)>,
    mob_target: Option<(u32, f32)>,
    voxel_dist: Option<f32>,
) -> AttackTarget {
    let mut best = f32::INFINITY;
    let mut player: Option<u32> = None;
    let mut mob: Option<u32> = None;
    if let Some((id, d)) = pvp_target
        && d < best
    {
        best = d;
        player = Some(id);
        mob = None;
    }
    if let Some((id, d)) = mob_target
        && d < best
    {
        best = d;
        mob = Some(id);
        player = None;
    }
    if let Some(d) = voxel_dist
        && d < best
    {
        mob = None;
        player = None;
    }
    AttackTarget { player, mob }
}

/// W47.13c: which RMB action wins for this click. The cascade in
/// `Voxel::on_update` walks these in priority order; the first
/// matching branch fires and the rest are suppressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RmbAction {
    /// No RMB this frame, or no condition matched.
    None,
    /// Shovel right-click on a grass voxel (id 3) - degrades to
    /// dirt (the MC "path" shortcut).
    PathGrass,
    /// Right-click on a furnace voxel - opens the furnace UI.
    OpenFurnace,
    /// Right-click on a chest voxel - opens the chest UI.
    OpenChest,
    /// Right-click on a bed voxel (id 35) - tries to skip the
    /// night.
    SleepBed,
    /// Right-click with flint+steel - tries to ignite a portal
    /// frame or drop a fire on the clicked surface.
    IgnitePortal,
    /// Right-click with an empty/water/lava bucket - scoops or
    /// pours a liquid voxel.
    BucketUse,
    /// Right-click with a placeable block selected - places a new
    /// voxel at the adjacent (or replaceable) cell.
    PlaceBlock,
}

/// Bed block id. There is no const in `block::ids` yet; magic
/// number documented here so the classifier stays self-contained.
const BED_ID: u16 = 35;

/// W47.13c: pick the winning RMB action.
///
/// `rmb` is the edge-triggered click; if false the result is
/// always `RmbAction::None`. The remaining inputs match what the
/// inline cascade reads from `self`:
/// - `target_id`: id of the voxel under the crosshair.
/// - `held_id`: id of the block in the selected hotbar slot,
///   `None` if the slot is empty.
/// - `held_shovel_tier`: `shovel_tier(held)` evaluated by the
///   caller. 0 if the held item is not a shovel.
/// - `held_is_tool_block`: `is_tool_block(held)` evaluated by the
///   caller. Tool / weapon blocks suppress the PlaceBlock branch.
/// - `may_build`: `self.abilities.may_build`. Adventure-mode
///   players have this off; PlaceBlock is then suppressed.
///
/// Priority order mirrors the cascade exactly: PathGrass beats
/// PlaceBlock when a shovel hits grass; the container/bed checks
/// only depend on the target voxel so they win whenever the held
/// item is irrelevant to them.
pub fn classify_rmb_action(
    rmb: bool,
    target_id: u16,
    held_id: Option<u16>,
    held_shovel_tier: u8,
    held_is_tool_block: bool,
    may_build: bool,
) -> RmbAction {
    use crate::block::{
        BUCKET_EMPTY_ID, BUCKET_LAVA_ID, BUCKET_WATER_ID, CHEST_ID,
        FLINT_AND_STEEL_ID, FURNACE_ID,
    };
    if !rmb {
        return RmbAction::None;
    }
    // 1. Shovel + grass -> path. Held must be a shovel (tier > 0)
    //    AND target must be the grass block.
    if target_id == 3 && held_shovel_tier > 0 {
        return RmbAction::PathGrass;
    }
    // 2. Containers + bed are target-only - the held block does
    //    not matter for these. A shovel on a furnace still opens
    //    the furnace.
    if target_id == FURNACE_ID {
        return RmbAction::OpenFurnace;
    }
    if target_id == CHEST_ID {
        return RmbAction::OpenChest;
    }
    if target_id == BED_ID {
        return RmbAction::SleepBed;
    }
    // 3. Held-item-driven branches: flint+steel and buckets.
    if let Some(h) = held_id {
        if h == FLINT_AND_STEEL_ID {
            return RmbAction::IgnitePortal;
        }
        if matches!(h, BUCKET_EMPTY_ID | BUCKET_WATER_ID | BUCKET_LAVA_ID) {
            return RmbAction::BucketUse;
        }
    }
    // 4. Generic block place. Suppressed in Adventure mode, with
    //    an empty hand, or while holding a tool/weapon.
    if may_build && held_id.is_some() && !held_is_tool_block {
        return RmbAction::PlaceBlock;
    }
    RmbAction::None
}

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
        if !suppress_drops && tier >= needed {
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
    vx.blocks_placed = vx.blocks_placed.saturating_add(1);
    vx.place_history.push(target_voxel);
    if vx.place_history.len() > 32 {
        let drop = vx.place_history.len() - 32;
        vx.place_history.drain(0..drop);
    }
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

/// W47.13g: flint+steel RMB.
///
/// `place_at` is the face cell adjacent to the clicked block, i.e.
/// the `hit.place_at` the cascade resolved. The helper drives one
/// of three outcomes:
/// - `try_ignite::Ok` -> writes portal cells across the 3x3x3 area
///   centered on `place_at`, indexes them in `portal_positions`,
///   broadcasts each edit + posts "Portal lit." to chat
/// - `NotRectangular | PerimeterIncomplete` -> posts "Portal frame
///   is invalid." to chat
/// - any other result -> tries to ignite a fire cell at `place_at`
///
/// Every outcome decrements the flint+steel durability + clears
/// the pending mouse flags. Returns nothing - all state changes
/// land on `vx`.
pub(crate) fn apply_flint_and_steel_ignite(
    vx: &mut crate::Voxel,
    place_at: [i32; 3],
) {
    use crate::net::NetRole;
    use crate::{EditMsg, Voxel, block};
    let cfg = bincode::config::standard();
    let res = block::portal::try_ignite(&mut vx.world, place_at);
    if let block::portal::IgniteResult::Ok { interior_count, .. } = res {
        let center = place_at;
        let mut emitted: Vec<EditMsg> = Vec::with_capacity(interior_count as usize);
        for dx in -3..=3 {
            for dy in -3..=3 {
                for dz in -3..=3 {
                    let p = [center[0] + dx, center[1] + dy, center[2] + dz];
                    if vx.world.get(p).0 == block::PORTAL_ID {
                        vx.portal_positions.insert(p);
                        emitted.push(EditMsg {
                            x: p[0],
                            y: p[1],
                            z: p[2],
                            block: block::PORTAL_ID,
                        });
                    }
                }
            }
        }
        let coords = Voxel::affected_chunks(place_at);
        vx.rebuild_chunks(&coords);
        match &mut vx.net {
            NetRole::Offline => {}
            NetRole::Server { server, .. } => {
                for edit in &emitted {
                    vx.edit_history.push(*edit);
                    if let Ok(buf) = bincode::serde::encode_to_vec(*edit, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
            }
            NetRole::Client { pending_edits, .. } => {
                for edit in &emitted {
                    pending_edits.push(*edit);
                }
            }
        }
        vx.chat_log.push((
            std::time::Instant::now(),
            "Portal lit.".to_string(),
        ));
    } else if matches!(
        res,
        block::portal::IgniteResult::NotRectangular
            | block::portal::IgniteResult::PerimeterIncomplete
    ) {
        vx.chat_log.push((
            std::time::Instant::now(),
            "Portal frame is invalid.".to_string(),
        ));
    } else {
        // No frame: drop a fire cell at the clicked position.
        let ignite_pos = place_at;
        if let Some(age) = block::fire::try_ignite_block(&vx.world, ignite_pos) {
            let step = block::fire::FireStep {
                ignite: vec![(ignite_pos, age)],
                ..Default::default()
            };
            vx.apply_fire_step(step);
        }
    }
    vx.consume_flint_and_steel_use();
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W47.13h: bucket RMB - scoop a liquid source or pour a filled
/// bucket.
///
/// `held` is the selected slot's block (already known to be one of
/// the bucket variants by `classify_rmb_action`). `forward` is the
/// player facing vector used for the scoop raycast (the standard
/// target ray skips liquids).
///
/// Scoop path (`BUCKET_EMPTY_ID`): runs a purpose-built raycast that
/// stops at the first liquid or solid voxel; if the hit is a source
/// (level 0) the source is replaced with air (skip in creative),
/// the bucket swaps to the matching filled id (skip in creative),
/// + a splash SFX fires.
///
/// Pour path (filled bucket): writes the liquid id at the hit voxel
/// (or `hit.place_at` if the hit cell isn't replaceable), sets the
/// level to 0 (source), extinguishes any fire that sat there, swaps
/// the bucket to empty (skip in creative), broadcasts both the edit
/// and the `vx.liquid_lvl` reset, + plays the splash SFX.
pub(crate) fn apply_bucket_use(
    vx: &mut crate::Voxel,
    hit: &infengine_voxel::RayHit,
    held: crate::BlockId,
    forward: glam::Vec3,
) {
    use crate::net::NetRole;
    use crate::{
        BUCKET_EMPTY_ID, BlockId, EditMsg, Voxel, block,
        block_is_liquid, block_is_ray_passable, block_is_replaceable,
        bucket_liquid_id, liquid_to_bucket_id, raycast_with,
    };
    let cfg = bincode::config::standard();
    if held.0 == BUCKET_EMPTY_ID {
        let liquid_hit = raycast_with(
            &vx.world,
            vx.player_pos,
            forward,
            6.0,
            |b| !b.is_air() && (block_is_liquid(b) || !block_is_ray_passable(b)),
        );
        let scoop_voxel = liquid_hit.map(|h| h.voxel);
        let scoop_block = scoop_voxel
            .map(|v| vx.world.get(v))
            .unwrap_or(BlockId::AIR);
        let scoop_level = scoop_voxel
            .and_then(|v| vx.liquid_level.get(&v).copied())
            .unwrap_or(0);
        let scoop_is_source = scoop_voxel.is_some() && scoop_level == 0;
        if scoop_is_source
            && let (Some(filled), Some(voxel)) = (
                liquid_to_bucket_id(scoop_block.0),
                scoop_voxel,
            )
        {
            if !vx.abilities.instabuild {
                vx.world_set_tracked(voxel, BlockId::AIR);
            }
            let coords = Voxel::affected_chunks(voxel);
            vx.rebuild_chunks(&coords);
            if !vx.abilities.instabuild {
                vx.inventory[vx.selected_slot] = Some((BlockId(filled), 1));
            }
            let edit = EditMsg {
                x: voxel[0],
                y: voxel[1],
                z: voxel[2],
                block: 0,
            };
            match &mut vx.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    vx.edit_history.push(edit);
                    if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
                NetRole::Client { pending_edits, .. } => {
                    pending_edits.push(edit);
                }
            }
            let center = [
                voxel[0] as f32 + 0.5,
                voxel[1] as f32 + 0.5,
                voxel[2] as f32 + 0.5,
            ];
            vx.sfx_at("splash", center, |a, g| {
                let _ = a.play_thud(140.0, 0.18 * g);
                let _ = a.play_swish(0.16 * g);
            });
        }
    } else if let Some(liquid_id) = bucket_liquid_id(held) {
        let target_block = vx.world.get(hit.voxel);
        let target_voxel = if block_is_replaceable(target_block) {
            hit.voxel
        } else {
            hit.place_at
        };
        let dest = vx.world.get(target_voxel);
        if dest.is_air() || block_is_replaceable(dest) {
            if dest.0 == block::FIRE_ID {
                vx.fires.remove(&target_voxel);
            }
            vx.world_set_tracked(target_voxel, BlockId(liquid_id));
            vx.liquid_level.insert(target_voxel, 0);
            let coords = Voxel::affected_chunks(target_voxel);
            vx.rebuild_chunks(&coords);
            if !vx.abilities.instabuild {
                vx.inventory[vx.selected_slot] =
                    Some((BlockId(BUCKET_EMPTY_ID), 1));
            }
            let edit = EditMsg {
                x: target_voxel[0],
                y: target_voxel[1],
                z: target_voxel[2],
                block: liquid_id,
            };
            match &mut vx.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    vx.edit_history.push(edit);
                    if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec((target_voxel, 0u8), cfg)
                    {
                        server.broadcast_bytes("vx.liquid_lvl", buf);
                    }
                }
                NetRole::Client { pending_edits, .. } => {
                    pending_edits.push(edit);
                }
            }
            let center = [
                target_voxel[0] as f32 + 0.5,
                target_voxel[1] as f32 + 0.5,
                target_voxel[2] as f32 + 0.5,
            ];
            vx.sfx_at("splash", center, |a, g| {
                let _ = a.play_thud(120.0, 0.18 * g);
                let _ = a.play_swish(0.14 * g);
            });
        }
    }
}

/// W47.13i: RMB on a furnace. Mints the container + runtime entry
/// if this is the first open, marks `furnace_open`, and tells the
/// host (when running as Client) so it can broadcast state
/// snapshots. Clears the pending mouse flags so the open click
/// doesn't replay as an edit.
pub(crate) fn open_furnace(vx: &mut crate::Voxel, pos: [i32; 3]) {
    use crate::block;
    use crate::net::NetRole;
    block::furnace::place_furnace(
        &mut vx.furnaces,
        &mut vx.furnace_runtimes,
        pos,
    );
    vx.furnace_open = Some(pos);
    if let NetRole::Client { client, .. } = &mut vx.net {
        let cfg = bincode::config::standard();
        let msg = crate::net::FurnaceOpenMsg { pos };
        if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
            let _ = client.send_bytes("vx.furnace_open", buf);
        }
    }
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W47.13i: RMB on a chest. Mints an empty container if this is
/// the first open, marks `chest_open`, and asks the host for the
/// authoritative state when running as Client. Pending mouse flags
/// cleared so the click doesn't replay against the world.
pub(crate) fn open_chest(vx: &mut crate::Voxel, pos: [i32; 3]) {
    use crate::block;
    use crate::net::NetRole;
    vx.chests
        .entry(pos)
        .or_insert_with(block::chest::new_chest);
    vx.chest_open = Some(pos);
    if let NetRole::Client { client, .. } = &mut vx.net {
        let cfg = bincode::config::standard();
        let msg = crate::net::ChestOpenMsg { pos };
        if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
            let _ = client.send_bytes("vx.chest_open", buf);
        }
    }
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W47.13i: RMB on a bed. Checks the day phase: outside night the
/// player gets a chat refusal; at night offline/host bumps the day
/// clock forward, client ships a `vx.sleep` request to the host.
pub(crate) fn try_sleep_bed(vx: &mut crate::Voxel) {
    use crate::DAY_LENGTH_S;
    use crate::net::NetRole;
    let phase = (vx.day_time / DAY_LENGTH_S).rem_euclid(1.0);
    let elev = (phase * std::f32::consts::TAU).sin();
    let is_night = elev < -0.05;
    if !is_night {
        vx.chat_log.push((
            std::time::Instant::now(),
            "You can only sleep at night.".to_string(),
        ));
        return;
    }
    match &mut vx.net {
        NetRole::Offline | NetRole::Server { .. } => {
            vx.day_time = DAY_LENGTH_S * 0.20;
            vx.last_day_broadcast = None;
            vx.chat_log.push((
                std::time::Instant::now(),
                "* sleeps through the night".to_string(),
            ));
        }
        NetRole::Client { client, .. } => {
            let _ = client.send_bytes("vx.sleep", Vec::new());
            vx.chat_log.push((
                std::time::Instant::now(),
                "Asking host to skip the night...".to_string(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_candidates_returns_empty() {
        let r = resolve_attack_target(None, None, None);
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn voxel_only_blocks_combat() {
        let r = resolve_attack_target(None, None, Some(3.0));
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn pvp_wins_when_closest() {
        let r = resolve_attack_target(
            Some((7, 1.0)),
            Some((9, 2.0)),
            Some(3.0),
        );
        assert_eq!(r, AttackTarget { player: Some(7), mob: None });
    }

    #[test]
    fn mob_wins_when_closer_than_player() {
        let r = resolve_attack_target(
            Some((7, 4.0)),
            Some((9, 2.0)),
            Some(3.0),
        );
        assert_eq!(r, AttackTarget { player: None, mob: Some(9) });
    }

    #[test]
    fn voxel_closest_suppresses_pvp_and_mob() {
        let r = resolve_attack_target(
            Some((7, 4.0)),
            Some((9, 5.0)),
            Some(1.0),
        );
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn mob_only_when_no_pvp_and_no_voxel() {
        let r = resolve_attack_target(None, Some((9, 2.0)), None);
        assert_eq!(r, AttackTarget { player: None, mob: Some(9) });
    }

    // --- classify_rmb_action ---

    use crate::block::{
        BUCKET_EMPTY_ID, BUCKET_LAVA_ID, BUCKET_WATER_ID, CHEST_ID,
        FLINT_AND_STEEL_ID, FURNACE_ID,
    };

    #[test]
    fn rmb_false_always_none() {
        assert_eq!(
            classify_rmb_action(false, FURNACE_ID, Some(1), 0, false, true),
            RmbAction::None,
        );
    }

    #[test]
    fn shovel_on_grass_wins_path() {
        assert_eq!(
            classify_rmb_action(true, 3, Some(50), 2, false, true),
            RmbAction::PathGrass,
        );
    }

    #[test]
    fn non_shovel_on_grass_falls_through_to_place() {
        // Held is some non-shovel non-tool block (e.g. dirt id 2)
        // - cascade reaches the generic place branch.
        assert_eq!(
            classify_rmb_action(true, 3, Some(2), 0, false, true),
            RmbAction::PlaceBlock,
        );
    }

    #[test]
    fn furnace_target_opens_regardless_of_held() {
        assert_eq!(
            classify_rmb_action(true, FURNACE_ID, Some(50), 3, false, true),
            RmbAction::OpenFurnace,
        );
    }

    #[test]
    fn chest_target_opens_regardless_of_held() {
        assert_eq!(
            classify_rmb_action(true, CHEST_ID, None, 0, false, true),
            RmbAction::OpenChest,
        );
    }

    #[test]
    fn bed_target_routes_to_sleep() {
        assert_eq!(
            classify_rmb_action(true, BED_ID, None, 0, false, true),
            RmbAction::SleepBed,
        );
    }

    #[test]
    fn flint_and_steel_routes_to_ignite() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(FLINT_AND_STEEL_ID), 0, false, true),
            RmbAction::IgnitePortal,
        );
    }

    #[test]
    fn empty_bucket_routes_to_bucket_use() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(BUCKET_EMPTY_ID), 0, false, true),
            RmbAction::BucketUse,
        );
    }

    #[test]
    fn water_bucket_routes_to_bucket_use() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(BUCKET_WATER_ID), 0, false, true),
            RmbAction::BucketUse,
        );
    }

    #[test]
    fn lava_bucket_routes_to_bucket_use() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(BUCKET_LAVA_ID), 0, false, true),
            RmbAction::BucketUse,
        );
    }

    #[test]
    fn empty_hand_blocks_place() {
        assert_eq!(
            classify_rmb_action(true, 1, None, 0, false, true),
            RmbAction::None,
        );
    }

    #[test]
    fn tool_block_blocks_place() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(99), 0, true, true),
            RmbAction::None,
        );
    }

    #[test]
    fn may_build_false_blocks_place() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(2), 0, false, false),
            RmbAction::None,
        );
    }

    #[test]
    fn place_block_with_regular_block() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(2), 0, false, true),
            RmbAction::PlaceBlock,
        );
    }
}
