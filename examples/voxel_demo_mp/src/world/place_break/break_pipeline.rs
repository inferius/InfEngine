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
        BlockId, CHEST_ID, EditMsg, REDSTONE_TORCH_ID,
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
    // W167: a broken fence gate / trapdoor drops its open + facing
    // state so a fresh block at the same cell doesn't inherit it.
    if crate::block::door::is_fence_gate_block(broken.0)
        || crate::block::door::is_trapdoor_block(broken.0)
    {
        vx.door_open.remove(&voxel);
        vx.door_facing.remove(&voxel);
    }
    // W143: a broken rail drops its stored shape; neighbours keep
    // theirs (MC: removal never re-bends the rest of the line).
    if crate::block::rail::is_rail_block(broken.0) {
        super::rail_place::on_rail_broken(vx, voxel);
    }
    // W156: breaking any part of a lit portal (a curtain block or a
    // frame obsidian) extinguishes the WHOLE connected curtain, like
    // MC. The curtain cells clear to air + broadcast; no drops.
    if broken.0 == block::PORTAL_ID || broken.0 == block::OBSIDIAN_ID {
        let mut curtain: Vec<[i32; 3]> = Vec::new();
        for d in [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ] {
            let seed = [voxel[0] + d[0], voxel[1] + d[1], voxel[2] + d[2]];
            for c in crate::block::portal::connected_portal_cells(&vx.world, seed) {
                if !curtain.contains(&c) {
                    curtain.push(c);
                }
            }
        }
        for c in curtain {
            vx.world_set_tracked(c, BlockId::AIR);
            let coords = Voxel::affected_chunks(c);
            vx.rebuild_chunks(&coords);
            let edit = EditMsg {
                x: c[0],
                y: c[1],
                z: c[2],
                block: 0,
            };
            match &mut vx.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    let cfg = bincode::config::standard();
                    if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                    vx.edit_history.push(edit);
                }
                NetRole::Client { pending_edits, .. } => {
                    pending_edits.push(edit);
                }
            }
        }
    }
    // Tint the debris with the broken block's albedo.
    let break_color = vx
        .registry
        .get(broken)
        .map(|d| d.color)
        .unwrap_or([0.75, 0.55, 0.30]);
    // W205: breaking a waterlogged plant (kelp / seagrass) restores
    // WATER, not air - in MC the cell contained water all along. The
    // ocean's worldgen sources never tick (equilibrium cells are
    // deliberately unscheduled), so an air hole would persist forever;
    // world_set_tracked(WATER) re-registers the cell as a source and
    // re-meshes the body.
    let restore = if crate::block::block_is_waterlogged(broken.0) {
        BlockId(crate::block::WATER_ID)
    } else {
        BlockId::AIR
    };
    let edit = EditMsg {
        x: voxel[0],
        y: voxel[1],
        z: voxel[2],
        block: restore.0,
    };
    if is_client {
        vx.world_set_tracked(voxel, restore);
        let coords = Voxel::affected_chunks(voxel);
        vx.rebuild_chunks(&coords);
        vx.spawn_break_particles_tinted(voxel, break_color);
        if let NetRole::Client { pending_edits, .. } = &mut vx.net {
            pending_edits.push(edit);
        }
    } else {
        vx.world_set_tracked(voxel, restore);
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
        if !suppress_drops && tier >= needed {
            // W35.2: wall-torch variants drop the floor-torch item.
            let drop_id = match broken.0 {
                x if x == REDSTONE_WALL_TORCH_ID => REDSTONE_TORCH_ID,
                x if x == SOUL_WALL_TORCH_ID => SOUL_TORCH_ID,
                _ => broken.0,
            };
            // W144: MC plant loot. Short grass / fern drop their
            // item only when cut with SHEARS; by hand they roll the
            // vanilla 1-in-8 wheat-seeds chance. A dead bush drops
            // 0-2 sticks without shears. Everything else keeps the
            // drop-itself path.
            const WHEAT_SEEDS_ITEM: u16 = 338;
            const STICK_ITEM: u16 = 302;
            const STRING_ITEM: u16 = 301;
            const SHEARS_ITEM: u16 = 640;
            let held_is_shears = held.0 == SHEARS_ITEM;
            // Same inline xorshift idiom as the other loot rolls
            // (net_tick + position seeded, host-side only).
            let mut rng: u32 = (vx.net_tick as u32)
                .wrapping_mul(0x9E37_79B9)
                ^ (voxel[0] as u32).wrapping_mul(0x85EB_CA6B)
                ^ (voxel[1] as u32).wrapping_mul(0xC2B2_AE35)
                ^ (voxel[2] as u32).wrapping_mul(0x27D4_EB2F);
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            match broken.0 {
                x if (x == crate::block::SHORT_GRASS_ID
                    || x == crate::block::FERN_ID)
                    && !held_is_shears =>
                {
                    if rng.is_multiple_of(8) {
                        vx.spawn_drop(BlockId(WHEAT_SEEDS_ITEM), voxel);
                    }
                }
                x if x == crate::block::DEAD_BUSH_ID && !held_is_shears => {
                    for _ in 0..(rng % 3) {
                        vx.spawn_drop(BlockId(STICK_ITEM), voxel);
                    }
                }
                // W242: spawners drop nothing in MC (only XP, which
                // we don't have yet).
                x if x == crate::block::SPAWNER_ID => {}
                // W243: cobwebs drop string without shears (the MC
                // silk-touch/shears self-drop is a tools gap).
                x if x == crate::block::COBWEB_ID && !held_is_shears => {
                    vx.spawn_drop(BlockId(STRING_ITEM), voxel);
                }
                // W294: MC crop loot tables - wheat drops the ITEM
                // + seeds (never the block); immature stages drop a
                // single seed / root. Roll shapes live in
                // block::crop_stages (unit-tested).
                x if crate::world::crop::is_crop(x) => {
                    for (item, count) in
                        crate::block::crop_stages::harvest_drops(x, rng)
                    {
                        for _ in 0..count {
                            vx.spawn_drop(BlockId(item), voxel);
                        }
                    }
                }
                _ => vx.spawn_drop(BlockId(drop_id), voxel),
            }
        }
        // Chest break: drain contents as drops, drop the container.
        // W169/W170/W172: barrel + ender chest + hopper share the
        // chest store.
        if (broken.0 == CHEST_ID
            || broken.0 == crate::block::BARREL_ID
            || broken.0 == crate::block::ENDER_CHEST_ID
            || broken.0 == crate::block::HOPPER_ID)
            && let Some(chest_inv) = vx.chests.remove(&voxel)
        {
            for id in block::chest::drain_drops(&chest_inv) {
                vx.spawn_drop(BlockId(id), voxel);
            }
            // W126: drop the render state with the chest.
            vx.chest_facing.remove(&voxel);
            vx.chest_lid_anim.remove(&voxel);
        }
        // Furnace break: drain 3 slots + drop the runtime entry.
        // W163: smoker + blast furnace share the furnace state.
        if block::furnace::is_furnace_like(broken.0) {
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
/// W294: hoe-RMB on dirt / grass tills FARMLAND (`HoeItem`
/// TILLABLES). Host/offline applies + broadcasts like the shovel
/// path; a pure client routes through the generic edit channel.
pub(crate) fn apply_hoe_till(vx: &mut crate::Voxel, voxel: [i32; 3]) {
    use crate::net::NetRole;
    use crate::{BlockId, EditMsg, Voxel};
    let cfg = bincode::config::standard();
    let farmland = crate::block::crop_stages::FARMLAND_ID;
    vx.world_set_tracked(voxel, BlockId(farmland));
    let coords = Voxel::affected_chunks(voxel);
    vx.rebuild_chunks(&coords);
    let edit = EditMsg {
        x: voxel[0],
        y: voxel[1],
        z: voxel[2],
        block: farmland,
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
            if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                let _ = client.send_bytes("vx.edit", buf);
            }
        }
    }
    vx.sfx_at(
        "dig/gravel",
        [
            voxel[0] as f32 + 0.5,
            voxel[1] as f32 + 0.5,
            voxel[2] as f32 + 0.5,
        ],
        |_, _| {},
    );
}

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
