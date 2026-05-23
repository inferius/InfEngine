//! W47.25 - chunk worker pool + worker-thread chunk mesher.
//! Relocated from main.rs to keep the bin shell small.
//!
//! `WorkerPool` owns the request/result channels + spawned threads.
//! `mesh_chunk_in_worker` is the free function each thread runs to
//! build MC-atlas submeshes against a captured `HaloLighting` +
//! `Arc<ResourcePack>` (no shared mutable state).

#![allow(unused_imports)]

use crate::*;

/// Thread pool that runs `generate_chunk` + `chunk_mesh` (and W25:
/// remesh-only jobs) off the main thread. The main thread enqueues
/// `Job` requests on `req_tx`; workers produce `WorkerResult` on
/// `res_rx`. Dropping the pool closes `req_tx`; the workers see
/// `recv` return Err and exit.
pub(crate) struct WorkerPool {
    pub(crate) req_tx: Sender<Job>,
    pub(crate) res_rx: Receiver<WorkerResult>,
    pub(crate) threads: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    /// W28.2: Number of worker threads in the pool.
    pub(crate) fn worker_count(&self) -> usize {
        self.threads.len()
    }

    pub(crate) fn spawn(num_workers: usize) -> Self {
        // W28.10: crossbeam-channel MPMC - receiver is Cloneable, no Mutex needed.
        let (req_tx, req_rx) = cb_unbounded::<Job>();
        let (res_tx, res_rx) = cb_unbounded::<WorkerResult>();
        let mut threads = Vec::with_capacity(num_workers);
        for i in 0..num_workers {
            let req_rx = req_rx.clone();
            let res_tx = res_tx.clone();
            threads.push(
                thread::Builder::new()
                    .name(format!("voxel_worker_{i}"))
                    .spawn(move || {
                        loop {
                            // W28.10: crossbeam recv() blocks without Mutex.
                            let job = match req_rx.recv() {
                                Ok(j) => j,
                                Err(_) => return,
                            };
                            match job {
                                Job::Generate(gj) => {
                                    // W46.25: disk-first chunk source. If
                                    // the slot has a saved chunk file at
                                    // this coord, decode it - that file
                                    // carries every player edit. Otherwise
                                    // fall through to fresh worldgen.
                                    //
                                    // Without this branch every chunk that
                                    // scrolled past UNLOAD_DISTANCE got
                                    // re-generated on re-entry, silently
                                    // wiping any torch / obsidian / placed
                                    // block that lived in it.
                                    let mut chunk_opt: Option<infengine_voxel::Chunk> = None;
                                    if let Some(slug) = gj.slug.as_deref() {
                                        match world_save::chunks::read_chunk(slug, &gj.dim, gj.coord) {
                                            Ok(Some(payload)) => {
                                                if let Ok(ch) = payload.voxels.into_chunk() {
                                                    chunk_opt = Some(ch);
                                                }
                                            }
                                            Ok(None) => {}
                                            Err(err) => {
                                                tracing::warn!(
                                                    coord = ?gj.coord,
                                                    error = %err,
                                                    "W46.25: read_chunk failed; falling back to worldgen",
                                                );
                                            }
                                        }
                                    }
                                    let chunk = chunk_opt.unwrap_or_else(|| {
                                        worldgen::generate_chunk_at_ctx(
                                            gj.coord, &gj.ctx,
                                        )
                                    });
                                    let origin = World::chunk_world_origin(gj.coord);

                                    // W13.2: Pre-scan voxels for side-effects
                                    // (snow / torch / liquid) so the main
                                    // thread doesn't have to iterate 4096
                                    // cells per chunk on delivery.
                                    let cs = CHUNK_SIZE as i32;
                                    let ox = origin.x as i32;
                                    let oy = origin.y as i32;
                                    let oz = origin.z as i32;
                                    let mut side_effects: Vec<ChunkSideEffect> = Vec::new();
                                    for ly in 0..cs {
                                        for lz in 0..cs {
                                            for lx in 0..cs {
                                                let b = chunk.get(
                                                    lx as usize,
                                                    ly as usize,
                                                    lz as usize,
                                                );
                                                let wv = [ox + lx, oy + ly, oz + lz];
                                                // Snow cap (block id 15).
                                                if b.0 == 15 {
                                                    side_effects.push(
                                                        ChunkSideEffect::Snow(wv),
                                                    );
                                                // W38.2: any open-flame block
                                                // registers a particle emitter.
                                                } else if flame_emitter(b.0) {
                                                    side_effects.push(
                                                        ChunkSideEffect::Torch(wv),
                                                    );
                                                } else if let Some(lq) = liquid_for(b) {
                                                    side_effects.push(
                                                        ChunkSideEffect::Liquid {
                                                            pos: wv,
                                                            tick_offset: lq.tick_period.max(1)
                                                                as u64,
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    }

                                    // W24: Build MC-atlas submeshes in the
                                    // worker using the pre-fetched halo
                                    // lighting + pack snapshot. Main thread
                                    // only does GPU upload.
                                    let submeshes = mesh_chunk_in_worker(
                                        &chunk,
                                        gj.coord,
                                        &gj.pack,
                                        &gj.halo,
                                    );

                                    if res_tx
                                        .send(WorkerResult::Generated {
                                            coord: gj.coord,
                                            chunk,
                                            side_effects,
                                            submeshes,
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                                Job::Remesh(rj) => {
                                    // W25: mesh-only - no worldgen, no
                                    // side-effect scan. The voxel snapshot
                                    // was cloned on the main thread at
                                    // dispatch time.
                                    let submeshes = mesh_chunk_in_worker(
                                        &rj.chunk,
                                        rj.coord,
                                        &rj.pack,
                                        &rj.halo,
                                    );
                                    if res_tx
                                        .send(WorkerResult::Remeshed {
                                            coord: rj.coord,
                                            revision: rj.revision,
                                            submeshes,
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                                Job::Lighting(lj) => {
                                    // W27: BFS over the owned chunk snapshot.
                                    // Runs entirely in the worker - no borrows
                                    // into the main-thread World.
                                    // Warm-start from existing grids so the
                                    // BFS clears + re-floods from scratch.
                                    let chunk_count = lj.affected.len();
                                    let bfs_start = std::time::Instant::now();
                                    let mut local_state =
                                        lighting::LightingState::with_grids(lj.existing_grids);
                                    lighting::block_engine::propagate_block_light(
                                        &lj.snapshot,
                                        &mut local_state,
                                        &lj.affected,
                                    );
                                    let block_done_ms = bfs_start.elapsed().as_millis() as u64;
                                    lighting::sky_engine::propagate_sky_light(
                                        &lj.snapshot,
                                        &mut local_state,
                                        &lj.affected,
                                    );
                                    let total_ms = bfs_start.elapsed().as_millis() as u64;
                                    tracing::info!(
                                        chunks = chunk_count,
                                        block_ms = block_done_ms,
                                        sky_ms = total_ms - block_done_ms,
                                        total_ms,
                                        "W46.30: lighting BFS done",
                                    );
                                    // Collect the updated grids for the
                                    // affected chunks only. Arc::clone is
                                    // cheap here (just a refcount bump on
                                    // the freshly created arcs).
                                    let new_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>> =
                                        lj.affected
                                            .iter()
                                            .filter_map(|cc| {
                                                local_state
                                                    .get(*cc)
                                                    .map(|g| (*cc, Arc::clone(g)))
                                            })
                                            .collect();
                                    let affected_for_remesh = lj.affected;
                                    if res_tx
                                        .send(WorkerResult::Lit {
                                            new_grids,
                                            affected_for_remesh,
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                        }
                    })
                    .expect("spawn voxel worker"),
            );
        }
        drop(res_tx); // workers keep their own clones
        Self {
            req_tx,
            res_rx,
            threads,
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // W31.1: Signal workers to stop by closing the request channel
        // (replace req_tx with a disconnected dummy; workers see recv()
        // return Err and exit their loop). We do NOT join the threads here:
        // a worker mid-BFS (lighting job) won't notice the disconnect until
        // AFTER the BFS completes, so joining would block the main thread for
        // hundreds of milliseconds - exactly the menu-return freeze the user
        // reported. Dropping a JoinHandle without calling .join() detaches the
        // thread; it runs to natural completion in the background and its
        // resources (Arc<ResourcePack>, chunk snapshots, lighting grids) are
        // released when the thread exits. GPU resources are never held by
        // workers, so detaching is safe.
        let (dummy_tx, _) = cb_unbounded::<Job>();
        let _ = std::mem::replace(&mut self.req_tx, dummy_tx);
        // Drop all handles (detach). Workers exit on the next recv() Err or
        // when they finish their current job and see the closed channel.
        let n = self.threads.len();
        self.threads.clear();
        info!("worker pool: signalled {n} workers to stop (detached)");
    }
}

// ---------------------------------------------------------------------------
// W24: Free function called from worker threads to build MC-atlas submeshes
// without touching the main-thread Voxel state. Mirrors `build_lit_submeshes`
// but reads from `HaloLighting` + `Arc<ResourcePack>` instead of self.
// ---------------------------------------------------------------------------

/// Build MC-atlas submeshes for `chunk` in a worker thread.
///
/// Replicates the closure setup in `Voxel::build_lit_submeshes` but
/// captures `pack` + `halo` instead of `&self.resource_pack` /
/// `&self.lighting`. Safe to call from any thread.
pub(crate) fn mesh_chunk_in_worker(
    chunk: &Chunk,
    coord: ChunkCoord,
    pack: &resource_pack::ResourcePack,
    halo: &HaloLighting,
) -> Vec<infengine_voxel::ChunkSubmesh> {
    let origin = World::chunk_world_origin(coord);
    let cs = infengine_voxel::CHUNK_SIZE as i32;
    let world_origin = [coord[0] * cs, coord[1] * cs, coord[2] * cs];

    // UV resolver: prefer MC pack atlas entry, fall back to legacy 4x4.
    let uv_resolver = |block: BlockId, normal: [i8; 3]| -> [f32; 4] {
        let face = if normal[1] > 0 {
            resource_pack::BlockFace::Top
        } else if normal[1] < 0 {
            resource_pack::BlockFace::Bottom
        } else {
            resource_pack::BlockFace::Sides
        };
        if let Some(r) = pack.block_face_ref(block.0, face) {
            return r.atlas_uv;
        }
        atlas_uv_rect(block, normal)
    };

    // Light lookup: translate local coord to world, ask the halo.
    let light_lookup = |air_local: [i32; 3]| -> [f32; 2] {
        let world = [
            world_origin[0] + air_local[0],
            world_origin[1] + air_local[1],
            world_origin[2] + air_local[2],
        ];
        halo.lookup(coord, world)
    };

    let anim_lookup = |block: BlockId, normal: [i8; 3]| -> u32 {
        let face = if normal[1] > 0 {
            resource_pack::BlockFace::Top
        } else if normal[1] < 0 {
            resource_pack::BlockFace::Bottom
        } else {
            resource_pack::BlockFace::Sides
        };
        pack.block_face_ref(block.0, face)
            .map(|t| pack.anim_slot_for_ref(t) as u32)
            .unwrap_or(0)
    };

    // emit_special: special-model geometry for non-cube blocks.
    let voxel_origin_ref = origin;
    let emit_special = move |block: BlockId,
                             voxel_origin: glam::Vec3,
                             verts: &mut Vec<MeshVertex>,
                             idxs: &mut Vec<u32>| {
        let lx = (voxel_origin.x - voxel_origin_ref.x) as i32;
        let ly = (voxel_origin.y - voxel_origin_ref.y) as i32;
        let lz = (voxel_origin.z - voxel_origin_ref.z) as i32;
        // W40.1: compute neighbour connection flags for multipart.
        let same_id = |dx: i32, dy: i32, dz: i32| -> bool {
            let nx = lx + dx;
            let ny = ly + dy;
            let nz = lz + dz;
            if (0..CHUNK_SIZE as i32).contains(&nx)
                && (0..CHUNK_SIZE as i32).contains(&ny)
                && (0..CHUNK_SIZE as i32).contains(&nz)
            {
                chunk.get(nx as usize, ny as usize, nz as usize).0 == block.0
            } else {
                false
            }
        };
        let neighbors = resource_pack::element_mesh::NeighborStates {
            north: same_id(0, 0, -1),
            south: same_id(0, 0,  1),
            east:  same_id( 1, 0, 0),
            west:  same_id(-1, 0, 0),
            up:    same_id(0,  1, 0),
            down:  same_id(0, -1, 0),
        };
        resource_pack::element_mesh::emit_block_elements(
            block.0,
            voxel_origin,
            verts,
            idxs,
            pack,
            neighbors,
            |delta: [i8; 3]| -> [f32; 2] {
                let air = [
                    world_origin[0] + lx + delta[0] as i32,
                    world_origin[1] + ly + delta[1] as i32,
                    world_origin[2] + lz + delta[2] as i32,
                ];
                halo.lookup(coord, air)
            },
            |_tex: &str| -> u32 { 0 },
            |delta: [i8; 3]| -> bool {
                let nx = lx + delta[0] as i32;
                let ny = ly + delta[1] as i32;
                let nz = lz + delta[2] as i32;
                if (0..CHUNK_SIZE as i32).contains(&nx)
                    && (0..CHUNK_SIZE as i32).contains(&ny)
                    && (0..CHUNK_SIZE as i32).contains(&nz)
                {
                    let nb = chunk.get(nx as usize, ny as usize, nz as usize);
                    // W44.5: air (id=0) has no BlockType entry so
                    // block_is_opaque(AIR) returned true via
                    // unwrap_or(true). Check is_air() first.
                    !nb.is_air() && block_is_opaque(nb)
                } else {
                    false
                }
            },
        )
    };

    // W34.2: worker also uses greedy mesher, matching build_lit_submeshes.
    chunk_mesh_greedy_with_special(
        chunk,
        origin,
        uv_resolver,
        block_is_opaque,
        |air_local, _normal| light_lookup(air_local),
        anim_lookup,
        emit_special,
    )
}
