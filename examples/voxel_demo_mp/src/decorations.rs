//! Decoration placement + support rules.
//!
//! "Decorations" are blocks tagged with `F_NO_PLACE` - torches,
//! flowers, snow layers, anything that needs an adjacent solid
//! face to stay attached. This module owns the two pieces of
//! logic that govern them:
//!
//! - `is_supported`: does a candidate cell have a neighbour solid
//!   enough to mount on? Drives both the placement check and the
//!   cascade-dislodge pass so the two paths agree.
//! - `dislodge_unsupported`: after a block is broken, walk the
//!   five attaching neighbours (above + NSEW) and pop any
//!   `F_NO_PLACE` decoration that lost its support. Cascades:
//!   each dislodged voxel goes back on the queue so torches on
//!   torches on dirt all unwind in one call.
//!
//! State lives on `Voxel`: `torch_positions`, `torch_orient`,
//! and `snow_positions`. This module operates on it via
//! `&mut Voxel`. Each `Voxel` method that used to host the body
//! is now a one-line wrapper - keeps the call sites in `main.rs`
//! unchanged while the bodies live next to each other here.

use infengine_voxel::BlockId;

use crate::block::{
    F_NO_PLACE, block_has_flag, block_is_aabb_passable,
    block_is_placement_blocked,
};
use crate::net::{EditMsg, NetRole};
use crate::Voxel;

/// True when the given position has at least one solid
/// non-passable neighbour (below or any NSEW) - i.e. a torch or
/// flower placed there would have something to mount on. Used
/// both by the placement check and the cascade-dislodge pass so
/// the two paths agree on what counts as "supported".
pub(crate) fn is_supported(voxel: &Voxel, pos: [i32; 3]) -> bool {
    let supportive = |p: [i32; 3]| {
        let b = voxel.world.get(p);
        !b.is_air()
            && !block_is_placement_blocked(b)
            && !block_is_aabb_passable(b)
    };
    if supportive([pos[0], pos[1] - 1, pos[2]]) {
        return true;
    }
    let walls = [
        [pos[0] + 1, pos[1], pos[2]],
        [pos[0] - 1, pos[1], pos[2]],
        [pos[0], pos[1], pos[2] + 1],
        [pos[0], pos[1], pos[2] - 1],
    ];
    walls.iter().any(|p| supportive(*p))
}

/// After a block is broken at `removed`, scan the 5 attaching
/// faces (above + NSEW) and pop off any `F_NO_PLACE` decoration
/// that no longer has support. Each dislodged decoration spawns
/// as a drop entity (pickup), broadcasts a `vx.edit` packet, and
/// re-enters the queue so cascades (torch on torch on dirt) all
/// unwind in one call.
pub(crate) fn dislodge_unsupported(voxel: &mut Voxel, removed: [i32; 3]) {
    if matches!(voxel.net, NetRole::Client { .. }) {
        return;
    }
    let mut queue: std::collections::VecDeque<[i32; 3]> =
        std::collections::VecDeque::new();
    queue.push_back(removed);
    while let Some(v) = queue.pop_front() {
        let attaching = [
            [v[0], v[1] + 1, v[2]],
            [v[0] + 1, v[1], v[2]],
            [v[0] - 1, v[1], v[2]],
            [v[0], v[1], v[2] + 1],
            [v[0], v[1], v[2] - 1],
        ];
        for c in attaching {
            let b = voxel.world.get(c);
            if b.is_air() {
                continue;
            }
            if !block_has_flag(b, F_NO_PLACE) {
                continue;
            }
            if is_supported(voxel, c) {
                continue;
            }
            voxel.world_set_tracked(c, BlockId::AIR);
            let coords = Voxel::affected_chunks(c);
            voxel.rebuild_chunks(&coords);
            // W10.2: Creative `instabuild` skips the cascade
            // drops too. The torch / flower still pops off (so
            // the world stays consistent) but no item entity
            // spawns.
            if !voxel.abilities.instabuild {
                voxel.spawn_drop(b, c);
            }
            let edit = EditMsg {
                x: c[0],
                y: c[1],
                z: c[2],
                block: 0,
            };
            voxel.edit_history.push(edit);
            if let NetRole::Server { server, .. } = &mut voxel.net {
                let cfg = bincode::config::standard();
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg)
                {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
            queue.push_back(c);
        }
    }
}
