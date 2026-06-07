//! W47.18 - chunk streamer, relocated from `main.rs`.
//!
//! `stream_chunks` runs once per frame and drives the entire
//! per-frame chunk lifecycle:
//! - queue missing chunks inside the view radius (sorted by XZ
//!   distance from the player)
//! - unload chunks outside the keep radius, persisting their edits
//!   to disk first (host / offline only) + pruning side maps
//!   (emitter / torch / fire / snow / liquid)
//! - dispatch pending coord jobs to the worker pool
//! - drain finished WorkerResults (Generated / Remeshed / Lit) into
//!   live world + chunk_meshes + lighting state
//! - drain deferred neighbour re-meshes
//!
//! Free function with `&mut Voxel`; same shape as `fire_runtime` +
//! `world::place_break` helpers. Body unchanged from the original
//! method; only the receiver moved.

use std::sync::Arc;

use infengine_voxel::CHUNK_SIZE;

use crate::{
    CHUNK_PHASE_WARN_US, ChunkCoord, ChunkJob, ChunkSideEffect, Job,
    PendingChunk, REDSTONE_TORCH_ID, REDSTONE_WALL_TORCH_ID, REMESH_DEFERRED_PER_FRAME,
    VIEW_DISTANCE_MAX, Voxel, WorkerResult, block_skips_chunk_mesh, emitter_scan,
    net::NetRole, world_save,
};
use crate::lighting::HaloLighting;

/// W47.18: drive one frame of the chunk stream. See module docs.
pub(crate) fn stream_chunks(vx: &mut Voxel) {
    let cs = CHUNK_SIZE as i32;
    let player_chunk = [
        (vx.player_pos.x.floor() as i32).div_euclid(cs),
        0,
        (vx.player_pos.z.floor() as i32).div_euclid(cs),
    ];
    let view_dist = vx.settings.view_distance.clamp(2, VIEW_DISTANCE_MAX as u8) as i32;
    let unload_dist = view_dist + 2;

    let (cy_min, cy_max) = Voxel::vertical_chunk_range(vx.player_pos.y);

    // Queue missing chunks inside the view radius.
    for dz in -view_dist..=view_dist {
        for dx in -view_dist..=view_dist {
            let dist_sq = (dx * dx + dz * dz) as i64;
            for cy in cy_min..=cy_max {
                let cc = [player_chunk[0] + dx, cy, player_chunk[2] + dz];
                if vx.world.chunk(cc).is_none() && vx.pending_set.insert(cc) {
                    vx.pending_chunks.push(PendingChunk {
                        neg_dist_sq: -dist_sq,
                        coord: cc,
                    });
                }
            }
        }
    }

    // Unload chunks beyond the keep radius.
    let to_unload: std::collections::HashSet<ChunkCoord> = vx
        .world
        .iter_chunks()
        .map(|(c, _)| *c)
        .filter(|c| {
            let dx = (c[0] - player_chunk[0]).abs();
            let dz = (c[2] - player_chunk[2]).abs();
            let horizontal_out = dx > unload_dist || dz > unload_dist;
            let vertical_out = c[1] < cy_min - 1 || c[1] > cy_max + 1;
            horizontal_out || vertical_out
        })
        .collect();
    if !to_unload.is_empty() {
        let renderer = vx.renderer.as_mut().expect("renderer ready");
        for cc in &to_unload {
            if let Some(subs) = vx.chunk_meshes.remove(cc) {
                for (_block, handle) in subs {
                    renderer.remove_chunk_mesh(handle);
                }
            }
            // W111: drop this chunk's per-chunk liquid mesh + signature.
            if let Some(kinds) = vx.liquid_mesh_handles.remove(cc) {
                for (_id, h) in kinds {
                    renderer.remove_mesh(h);
                }
            }
            vx.liquid_chunk_sig.remove(cc);
        }
        // Persist edits to disk on a background thread.
        if matches!(vx.net, NetRole::Server { .. } | NetRole::Offline)
            && let Some(slug) = vx.current_world_slug.clone()
        {
            let dim = vx.current_dim.clone();
            let mut payloads: Vec<world_save::chunks::ChunkPayload> =
                Vec::with_capacity(to_unload.len());
            for cc in &to_unload {
                if let Some(p) = world_save::build_chunk_payload(vx, *cc) {
                    payloads.push(p);
                }
            }
            if !payloads.is_empty() {
                std::thread::Builder::new()
                    .name("voxel_demo_mp::unload_writer".to_string())
                    .spawn(move || {
                        for payload in payloads {
                            let coord = payload.coord;
                            if let Err(err) =
                                world_save::write_chunk_payload(&slug, &dim, &payload)
                            {
                                tracing::warn!(
                                    coord = ?coord,
                                    error = %err,
                                    "world_save: async unload-save failed",
                                );
                            }
                        }
                    })
                    .ok();
            }
        }
        // Prune side maps for unloaded chunk volumes.
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        for cc in &to_unload {
            let xmin = cc[0] * cs;
            let xmax = xmin + cs;
            let ymin = cc[1] * cs;
            let ymax = ymin + cs;
            let zmin = cc[2] * cs;
            let zmax = zmin + cs;
            let in_chunk = |p: &[i32; 3]| {
                p[0] >= xmin && p[0] < xmax
                    && p[1] >= ymin && p[1] < ymax
                    && p[2] >= zmin && p[2] < zmax
            };
            vx.emitter_positions.retain(|p, _| !in_chunk(p));
            vx.torch_positions.retain(|p| !in_chunk(p));
            vx.torch_orient.retain(|p, _| !in_chunk(p));
            vx.fires.retain(|p, _| !in_chunk(p));
            vx.snow_positions.retain(|p, _| !in_chunk(p));
            vx.liquid_positions.retain(|p| !in_chunk(p));
            vx.liquid_level.retain(|p, _| !in_chunk(p));
        }
        for cc in &to_unload {
            vx.world.remove_chunk(*cc);
        }
    }

    // Dispatch pending chunks to the worker pool.
    if let Some(pool) = vx.workers.as_ref() {
        let horiz_speed = (vx.player_vel.x * vx.player_vel.x
            + vx.player_vel.z * vx.player_vel.z)
            .sqrt();
        let dispatch_cap = if horiz_speed > 6.0 {
            32
        } else {
            pool.worker_count() * 2
        };
        let ctx = vx.current_worldgen_ctx();
        let pack_arc = Arc::clone(&vx.resource_pack);
        let mut dispatched = 0usize;
        while dispatched < dispatch_cap {
            let Some(pending) = vx.pending_chunks.pop() else { break };
            let cc = pending.coord;
            let halo = HaloLighting {
                own: vx.lighting.get(cc).cloned(),
                px: vx.lighting.get([cc[0] + 1, cc[1], cc[2]]).cloned(),
                nx: vx.lighting.get([cc[0] - 1, cc[1], cc[2]]).cloned(),
                pz: vx.lighting.get([cc[0], cc[1], cc[2] + 1]).cloned(),
                nz: vx.lighting.get([cc[0], cc[1], cc[2] - 1]).cloned(),
                py: vx.lighting.get([cc[0], cc[1] + 1, cc[2]]).cloned(),
                ny: vx.lighting.get([cc[0], cc[1] - 1, cc[2]]).cloned(),
            };
            let job = Job::Generate(ChunkJob {
                coord: cc,
                ctx: ctx.clone(),
                pack: Arc::clone(&pack_arc),
                halo,
                slug: vx.current_world_slug.clone(),
                dim: vx.current_dim.clone(),
            });
            if pool.req_tx.send(job).is_err() {
                vx.pending_chunks.push(pending);
                break;
            }
            dispatched += 1;
        }
    }

    // Drain finished results from the workers.
    let (results_cap, budget_ms) = vx.current_results_budget();
    // W106: do NOT clear here - results that last frame's processing
    // time-budget left unprocessed carry over, so a burst of finished
    // chunks drains across several frames instead of one big spike.
    if let Some(pool) = vx.workers.as_ref() {
        let drain_start = std::time::Instant::now();
        let drain_budget = std::time::Duration::from_millis(budget_ms);
        while vx.stream_delivered.len() < results_cap
            && drain_start.elapsed() < drain_budget
            && let Ok(r) = pool.res_rx.try_recv()
        {
            vx.stream_delivered.push(r);
        }
    }
    let delivered = std::mem::take(&mut vx.stream_delivered);
    // W106: apply the per-frame time budget to PROCESSING (the GPU mesh
    // uploads), which is what the budget always intended (see the
    // RESULTS_BUDGET_MS doc). Previously it gated only the cheap try_recv
    // drain, so all `results_cap` meshes still GPU-uploaded in ONE frame -
    // ballooning it to 15-20 ms and making the camera + nearby mobs/trees
    // visibly judder during streaming/lighting bursts. Whatever doesn't
    // fit the budget carries to the next frame via `stream_delivered`.
    let process_start = std::time::Instant::now();
    let process_budget = std::time::Duration::from_millis(budget_ms);
    let mut delivered_iter = delivered.into_iter();
    for result in delivered_iter.by_ref() {
        match result {
            WorkerResult::Generated {
                coord,
                chunk,
                side_effects,
                submeshes,
            } => {
                vx.pending_set.remove(&coord);
                let dx = (coord[0] - player_chunk[0]).abs();
                let dz = (coord[2] - player_chunk[2]).abs();
                if dx > unload_dist || dz > unload_dist {
                    continue;
                }
                if vx.world.chunk(coord).is_some() {
                    continue;
                }
                let t0 = std::time::Instant::now();
                for se in &side_effects {
                    match se {
                        ChunkSideEffect::Snow(wv) => {
                            vx.snow_positions.entry(*wv).or_insert(1);
                        }
                        ChunkSideEffect::Torch(wv) => {
                            vx.torch_positions.insert(*wv);
                        }
                        ChunkSideEffect::Liquid { pos, tick_offset } => {
                            vx.liquid_schedule.schedule(
                                *pos,
                                vx.liquid_clock + tick_offset,
                            );
                            if vx.liquid_positions.insert(*pos) {
                                vx.liquid_mesh_dirty = true;
                            }
                        }
                    }
                }
                let side_effect_us = t0.elapsed().as_micros();
                if side_effect_us > CHUNK_PHASE_WARN_US {
                    tracing::warn!(
                        coord = ?coord,
                        us = side_effect_us,
                        "W13.1: side_effects fold exceeded threshold"
                    );
                }
                emitter_scan::scan_chunk(
                    coord,
                    &chunk,
                    &mut vx.emitter_positions,
                    &mut vx.torch_positions,
                    &mut vx.fires,
                );
                vx.world.insert_chunk(coord, chunk);
                let cc = coord;
                if !submeshes.is_empty() {
                    let atlas = vx.voxel_atlas.clone();
                    let renderer = vx.renderer.as_mut().expect("renderer ready");
                    for sub in submeshes {
                        if sub.vertices.is_empty() || sub.indices.is_empty() {
                            continue;
                        }
                        if block_skips_chunk_mesh(sub.block) {
                            continue;
                        }
                        let handle = renderer.create_chunk_mesh(
                            &format!(
                                "voxel_chunk_{}_{}_{}_b{}",
                                cc[0], cc[1], cc[2], sub.block.0
                            ),
                            &sub.vertices,
                            &sub.indices,
                        );
                        if let Some(a) = atlas.clone() {
                            renderer.bind_chunk_mesh_albedo(handle, a);
                        }
                        if matches!(
                            sub.block.0,
                            REDSTONE_TORCH_ID | REDSTONE_WALL_TORCH_ID
                        ) {
                            renderer.set_chunk_mesh_layer(
                                handle,
                                infengine_render::CHUNK_LAYER_CUTOUT,
                            );
                        }
                        vx.chunk_meshes.entry(cc).or_default().push((sub.block, handle));
                    }
                }
                for n in [
                    [cc[0] - 1, cc[1], cc[2]],
                    [cc[0] + 1, cc[1], cc[2]],
                    [cc[0], cc[1], cc[2] - 1],
                    [cc[0], cc[1], cc[2] + 1],
                ] {
                    if vx.world.chunk(n).is_some() && vx.pending_remesh_set.insert(n) {
                        vx.pending_remesh.push_back(n);
                    }
                }
                let cs = CHUNK_SIZE as i32;
                vx.lighting.mark_dirty([
                    cc[0] * cs + cs / 2,
                    cc[1] * cs + cs / 2,
                    cc[2] * cs + cs / 2,
                ]);
            }
            WorkerResult::Remeshed {
                coord,
                revision,
                submeshes,
            } => {
                let current_rev = vx.chunk_revisions.get(&coord).copied().unwrap_or(0);
                if revision < current_rev {
                    tracing::debug!(
                        coord = ?coord,
                        arrived_rev = revision,
                        current_rev,
                        "W32 debug: stale Remeshed result dropped"
                    );
                    continue;
                }
                tracing::debug!(
                    coord = ?coord,
                    revision,
                    submeshes = submeshes.len(),
                    "W32 debug: Remeshed result kept"
                );
                {
                    let renderer = vx.renderer.as_mut().expect("renderer ready");
                    if let Some(old_subs) = vx.chunk_meshes.remove(&coord) {
                        for (_block, handle) in old_subs {
                            renderer.remove_chunk_mesh(handle);
                        }
                    }
                }
                if !submeshes.is_empty() {
                    let atlas = vx.voxel_atlas.clone();
                    let renderer = vx.renderer.as_mut().expect("renderer ready");
                    for sub in submeshes {
                        if sub.vertices.is_empty() || sub.indices.is_empty() {
                            continue;
                        }
                        if block_skips_chunk_mesh(sub.block) {
                            continue;
                        }
                        let handle = renderer.create_chunk_mesh(
                            &format!(
                                "voxel_chunk_{}_{}_{}_b{}",
                                coord[0], coord[1], coord[2], sub.block.0
                            ),
                            &sub.vertices,
                            &sub.indices,
                        );
                        if let Some(a) = atlas.clone() {
                            renderer.bind_chunk_mesh_albedo(handle, a);
                        }
                        if matches!(
                            sub.block.0,
                            REDSTONE_TORCH_ID | REDSTONE_WALL_TORCH_ID
                        ) {
                            renderer.set_chunk_mesh_layer(
                                handle,
                                infengine_render::CHUNK_LAYER_CUTOUT,
                            );
                        }
                        vx.chunk_meshes
                            .entry(coord)
                            .or_default()
                            .push((sub.block, handle));
                    }
                }
            }
            WorkerResult::Lit {
                new_grids,
                affected_for_remesh,
            } => {
                let lit_apply_start = std::time::Instant::now();
                let new_grid_count = new_grids.len();
                for (cc, grid) in new_grids {
                    vx.lighting.set_grid(cc, grid);
                }
                let mut queued = 0usize;
                for cc in affected_for_remesh {
                    if vx.world.chunk(cc).is_some() && vx.pending_remesh_set.insert(cc) {
                        vx.pending_remesh.push_back(cc);
                        queued += 1;
                    }
                }
                tracing::info!(
                    grids = new_grid_count,
                    remesh_queued = queued,
                    apply_ms = lit_apply_start.elapsed().as_millis() as u64,
                    "W46.30: WorkerResult::Lit applied + remesh queued",
                );
                vx.pending_lighting_jobs = vx.pending_lighting_jobs.saturating_sub(1);
            }
        }
        // W106: stop once this frame's streaming budget is spent; the
        // remaining drained results stay queued for the next frame.
        if process_start.elapsed() >= process_budget {
            break;
        }
    }
    vx.stream_delivered = delivered_iter.collect();

    // Drain deferred neighbour re-meshes.
    let mut remesh_done = 0;
    while remesh_done < REMESH_DEFERRED_PER_FRAME {
        let Some(coord) = vx.pending_remesh.pop_front() else {
            break;
        };
        vx.pending_remesh_set.remove(&coord);
        if vx.world.chunk(coord).is_none() {
            continue;
        }
        vx.rebuild_chunks(&[coord]);
        remesh_done += 1;
    }
}
