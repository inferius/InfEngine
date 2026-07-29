//! W451: legacy-save data fixups, gated by `WorldMeta::data_version`.
//!
//! R34 mandate: current-format saves must not pay ANY perpetual
//! healing cost. Worlds stamped with `WORLD_DATA_VERSION` skip this
//! module entirely; older saves get exactly one scrub per chunk
//! when it enters RAM (the upfront-load pass at world load + the
//! disk-first stream path), because chunk files load lazily and can
//! stay old on disk for as long as the player never walks near
//! them.
//!
//! Fixups (all idempotent, all cheap 16^3 id scans):
//! - stale pillar-axis records on vertical trunks (pre-W421 saves;
//!   moved from the W426 stream scrub)
//! - missing door upper halves + facing rows (pre-W311 saves baked
//!   AIR above template doors; moved from the W450 stream heal)
//! - missing ladder facing rows (pre-W442 saves have no kind-7
//!   records; the wall is inferred from WORLD neighbours here, so
//!   old igloo shafts stop rendering ladders detached from their
//!   wall - the mesher's chunk-local fallback cannot see across
//!   chunk seams)

use infengine_voxel::{BlockId, ChunkCoord, CHUNK_SIZE};

use crate::Voxel;

/// Scrub one chunk. Queues remeshes for anything it changed and
/// returns whether it changed anything.
pub(crate) fn scrub_chunk(vx: &mut Voxel, cc: ChunkCoord) -> bool {
    let mut stale_pillar: Vec<[i32; 3]> = Vec::new();
    let mut heal_doors: Vec<([i32; 3], u16)> = Vec::new();
    let mut heal_facing: Vec<([i32; 3], u8)> = Vec::new();
    let cs_i = CHUNK_SIZE as i32;
    if let Some(ch) = vx.world.chunk(cc) {
        for z in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let id = ch.get(x, y, z).0;
                    if id == 0 {
                        continue;
                    }
                    let wp = [
                        cc[0] * cs_i + x as i32,
                        cc[1] * cs_i + y as i32,
                        cc[2] * cs_i + z as i32,
                    ];
                    if crate::block::door::is_door_block(id) {
                        let below =
                            vx.world.get([wp[0], wp[1] - 1, wp[2]]).0;
                        if crate::block::door::is_door_block(below) {
                            continue; // upper half
                        }
                        let above = [wp[0], wp[1] + 1, wp[2]];
                        if vx.world.get(above).0 == 0 {
                            heal_doors.push((
                                above,
                                if id == 77 { 78 } else { id },
                            ));
                        }
                        if !vx.door_facing.contains_key(&wp) {
                            // Wall along X -> the panel spans X ->
                            // faces south; otherwise it spans Z and
                            // faces east.
                            let wall_x = crate::block::block_is_opaque(
                                vx.world.get([wp[0] - 1, wp[1], wp[2]]),
                            ) || crate::block::block_is_opaque(
                                vx.world.get([wp[0] + 1, wp[1], wp[2]]),
                            );
                            heal_facing
                                .push((wp, if wall_x { 1 } else { 0 }));
                        }
                        continue;
                    }
                    if id == crate::block::LADDER_ID {
                        if !vx.door_facing.contains_key(&wp) {
                            let solid = |dx: i32, dz: i32| -> bool {
                                crate::block::block_is_opaque(
                                    vx.world.get([
                                        wp[0] + dx,
                                        wp[1],
                                        wp[2] + dz,
                                    ]),
                                )
                            };
                            // Ladder FACES AWAY from its wall (same
                            // codes as the W442 mesher arm).
                            let facing = if solid(1, 0) {
                                2
                            } else if solid(-1, 0) {
                                0
                            } else if solid(0, 1) {
                                3
                            } else {
                                1
                            };
                            heal_facing.push((wp, facing));
                        }
                        continue;
                    }
                    if !crate::block::is_pillar_block(id) {
                        continue;
                    }
                    let Some(&st) = vx.stair_states.get(&wp) else {
                        continue;
                    };
                    if st != 1 && st != 2 {
                        continue;
                    }
                    let above = vx.world.get([wp[0], wp[1] + 1, wp[2]]).0;
                    let below = vx.world.get([wp[0], wp[1] - 1, wp[2]]).0;
                    // A vertical pillar neighbour marks a trunk
                    // column - real fallen logs lie flat and village
                    // beams don't stack vertically.
                    if crate::block::is_pillar_block(above)
                        || crate::block::is_pillar_block(below)
                    {
                        stale_pillar.push(wp);
                    }
                }
            }
        }
    }
    let changed = !stale_pillar.is_empty()
        || !heal_doors.is_empty()
        || !heal_facing.is_empty();
    if !changed {
        return false;
    }
    let mut touched: Vec<ChunkCoord> = vec![cc];
    for wp in stale_pillar {
        vx.stair_states.remove(&wp);
    }
    for (pos, id) in heal_doors {
        vx.world_set_tracked(pos, BlockId(id));
        let acc = [
            pos[0].div_euclid(cs_i),
            pos[1].div_euclid(cs_i),
            pos[2].div_euclid(cs_i),
        ];
        if !touched.contains(&acc) {
            touched.push(acc);
        }
    }
    for (pos, facing) in heal_facing {
        vx.door_facing.insert(pos, facing);
    }
    for c in touched {
        if vx.pending_seam_set.insert(c) {
            vx.pending_seam_remesh.push_back(c);
        }
    }
    true
}

/// One-shot pass over every chunk currently in RAM (the upfront
/// load radius). Called from `load_world` when the save predates
/// `WORLD_DATA_VERSION`.
pub(crate) fn scrub_all_loaded(vx: &mut Voxel) {
    let coords: Vec<ChunkCoord> =
        vx.world.iter_chunks().map(|(c, _)| *c).collect();
    let mut changed = 0usize;
    for cc in &coords {
        if scrub_chunk(vx, *cc) {
            changed += 1;
        }
    }
    if changed > 0 {
        tracing::info!(
            chunks = coords.len(),
            changed,
            "W451: legacy-save fixups applied to loaded radius",
        );
    }
}
