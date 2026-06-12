//! W143 - runtime glue for the stateful rail system.
//!
//! `block::rail_state` is the pure MC `RailState` port; this module
//! applies its results to the live app: the `rail_shapes` map, chunk
//! remeshes for every touched cell, and the `vx.rail_shape` net
//! message (same placer-computes / server-rebroadcasts flow as
//! `vx.door_state`). Payload: `([i32; 3], u8)` - shape `0xFF` means
//! the rail was removed.

use crate::Voxel;
use crate::net::NetRole;

/// Wire value meaning "rail removed" in a `vx.rail_shape` payload.
pub(crate) const RAIL_SHAPE_REMOVED: u8 = 0xFF;

/// Called by the place pipeline right after a rail block lands in
/// the world. Decides the new rail's shape (MC `RailState.place`),
/// reshapes at most the two neighbours it connects to, stores +
/// remeshes + broadcasts.
pub(crate) fn on_rail_placed(vx: &mut Voxel, pos: [i32; 3], id: u16) {
    // MC: the no-neighbour fallback shape follows the player's
    // horizontal facing axis (look east/west -> east_west).
    let player_axis_x = vx.player_yaw.cos().abs() >= vx.player_yaw.sin().abs();
    let result = {
        let acc = crate::block::rail_state::LiveRailAccess {
            world: &vx.world,
            shapes: &vx.rail_shapes,
        };
        crate::block::rail_state::place_rail(&acc, pos, id, player_axis_x)
    };
    let mut changed: Vec<([i32; 3], u8)> = Vec::with_capacity(1 + result.neighbor_updates.len());
    changed.push((pos, result.shape));
    changed.extend(result.neighbor_updates.iter().map(|u| (u.pos, u.shape)));
    apply_shape_changes(vx, &changed);
    broadcast_shape_changes(vx, &changed);
}

/// Called by the break pipeline when a rail block leaves the world.
/// MC keeps the neighbours' shapes on removal (a junction only
/// resets when IT is rebuilt), so this just drops the stored state.
pub(crate) fn on_rail_broken(vx: &mut Voxel, pos: [i32; 3]) {
    if vx.rail_shapes.remove(&pos).is_some() {
        broadcast_shape_changes(vx, &[(pos, RAIL_SHAPE_REMOVED)]);
    }
}

/// Store the new shapes and remesh every chunk a touched cell sits
/// in (neighbour reshapes can cross chunk borders).
pub(crate) fn apply_shape_changes(vx: &mut Voxel, changed: &[([i32; 3], u8)]) {
    let mut coords: Vec<infengine_voxel::ChunkCoord> = Vec::new();
    for &(p, shape) in changed {
        if shape == RAIL_SHAPE_REMOVED {
            vx.rail_shapes.remove(&p);
        } else {
            vx.rail_shapes.insert(p, shape);
        }
        for c in Voxel::affected_chunks(p) {
            if !coords.contains(&c) {
                coords.push(c);
            }
        }
    }
    vx.rebuild_chunks(&coords);
}

/// Placer-side fan-out, mirroring `vx.door_state`: hosts broadcast,
/// clients send to the server which rebroadcasts to everyone else.
fn broadcast_shape_changes(vx: &mut Voxel, changed: &[([i32; 3], u8)]) {
    let cfg = bincode::config::standard();
    match &mut vx.net {
        NetRole::Offline => {}
        NetRole::Server { server, .. } => {
            for payload in changed {
                if let Ok(buf) = bincode::serde::encode_to_vec(payload, cfg) {
                    server.broadcast_bytes("vx.rail_shape", buf);
                }
            }
        }
        NetRole::Client { client, .. } => {
            for payload in changed {
                if let Ok(buf) = bincode::serde::encode_to_vec(payload, cfg) {
                    let _ = client.send_bytes("vx.rail_shape", buf);
                }
            }
        }
    }
}
