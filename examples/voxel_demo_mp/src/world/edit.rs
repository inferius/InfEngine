//! W65 - shared single-block edit funnel for runtime world
//! simulation (falling blocks, random ticks, ...).
//!
//! Writing a block at runtime always means the same four steps:
//! mutate via `world_set_tracked` (side-table bookkeeping), remesh
//! the affected chunks, broadcast `vx.edit` to clients, and record
//! the edit in `edit_history` for the save. Centralised here so the
//! falling-block tick, the random-tick behaviours, and any future
//! simulation share one code path. Host-only - callers gate on
//! `NetRole`.

use crate::net::NetRole;
use crate::{BlockId, EditMsg, Voxel};

/// Apply one authoritative block edit the same way a manual dig /
/// place does: write the cell with side-table bookkeeping via
/// `world_set_tracked`, remesh the affected chunks, broadcast
/// `vx.edit` on the server, and record it in `edit_history`.
pub(crate) fn emit(vx: &mut Voxel, pos: [i32; 3], block: BlockId) {
    vx.world_set_tracked(pos, block);
    let coords = Voxel::affected_chunks(pos);
    vx.rebuild_chunks(&coords);
    let edit = EditMsg {
        x: pos[0],
        y: pos[1],
        z: pos[2],
        block: block.0,
    };
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
            server.broadcast_bytes("vx.edit", buf);
        }
    }
    vx.edit_history.push(edit);
}
