//! W47.14 - held-item RMB use submodule (split from W47.13 monolith).
//!
//! Contains the two "use the held item" branches:
//! - `apply_flint_and_steel_ignite` (W47.13g) - portal frame ignite
//!   or fire-drop fallback.
//! - `apply_bucket_use` (W47.13h) - empty bucket scoop / filled
//!   bucket pour, including fire extinguish + liquid level reset.

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
    if let block::portal::IgniteResult::Ok { cells, .. } = &res {
        // W273: the shape hands over the exact interior (up to
        // 21x21 - the old +-3 rescan missed big frames).
        let mut emitted: Vec<EditMsg> = Vec::with_capacity(cells.len());
        let mut coords: Vec<crate::ChunkCoord> = Vec::new();
        for &p in cells {
            vx.portal_positions.insert(p);
            emitted.push(EditMsg { x: p[0], y: p[1], z: p[2], block: block::PORTAL_ID });
            for c in Voxel::affected_chunks(p) {
                if !coords.contains(&c) {
                    coords.push(c);
                }
            }
        }
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
    } else if matches!(res, block::portal::IgniteResult::NotRectangular) {
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
                vx.inventory[vx.selected_slot] = Some(crate::item::ItemStack::new(filled, 1));
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
                    Some(crate::item::ItemStack::new(BUCKET_EMPTY_ID, 1));
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
