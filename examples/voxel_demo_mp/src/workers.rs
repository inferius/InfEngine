//! W47.25 - chunk worker pool + worker-thread chunk mesher.
//! Relocated from main.rs to keep the bin shell small.
//!
//! `WorkerPool` owns the request/result channels + spawned threads.
//! `mesh_chunk_in_worker` is the free function each thread runs to
//! build MC-atlas submeshes against a captured `HaloLighting` +
//! `Arc<ResourcePack>` (no shared mutable state).

#![allow(unused_imports)]

use crate::*;
use crate::lighting::HaloLighting;

thread_local! {
    /// W305: per-worker gen profile - (gen_us, scan_us, mesh_us,
    /// chunks). One summary log per 512 generated chunks tells the
    /// next perf report WHERE worker time goes (worldgen vs the
    /// side-effect scan vs meshing) without per-chunk log spam.
    static GEN_PROF: std::cell::RefCell<(u64, u64, u64, u32)> =
        const { std::cell::RefCell::new((0, 0, 0, 0)) };
}

/// Thread pool that runs `generate_chunk` + `chunk_mesh` (and W25:
/// remesh-only jobs) off the main thread. The main thread enqueues
/// `Job` requests on `req_tx`; workers produce `WorkerResult` on
/// `res_rx`. Dropping the pool closes `req_tx`; the workers see
/// `recv` return Err and exit.
pub(crate) struct WorkerPool {
    pub(crate) req_tx: Sender<Job>,
    /// W304: the lighting PRIORITY lane. Gen/remesh floods `req_tx`
    /// by the hundred at high view distance; lighting segments queued
    /// behind them starved for minutes (light_dirty 10k-20k in the
    /// user log). Workers drain this channel first; the in-flight
    /// lighting cap (24) bounds the lane so gen can't starve either.
    pub(crate) light_tx: Sender<Job>,
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
        let (light_tx, light_rx) = cb_unbounded::<Job>();
        let (res_tx, res_rx) = cb_unbounded::<WorkerResult>();
        let mut threads = Vec::with_capacity(num_workers);
        for i in 0..num_workers {
            let req_rx = req_rx.clone();
            let light_rx = light_rx.clone();
            let res_tx = res_tx.clone();
            threads.push(
                thread::Builder::new()
                    .name(format!("voxel_worker_{i}"))
                    .spawn(move || {
                        loop {
                            // W28.10: crossbeam recv() blocks without
                            // Mutex. W304: lighting lane first - a
                            // waiting lighting segment always beats
                            // the gen/remesh backlog.
                            let job = match light_rx.try_recv() {
                                Ok(j) => j,
                                Err(_) => {
                                    crossbeam_channel::select! {
                                        recv(light_rx) -> j => match j {
                                            Ok(j) => j,
                                            Err(_) => return,
                                        },
                                        recv(req_rx) -> j => match j {
                                            Ok(j) => j,
                                            Err(_) => return,
                                        },
                                    }
                                }
                            };
                            match job {
                                Job::Generate(gj) => {
                                    // W239: one job = ONE COLUMN batch.
                                    // Iterating the column's chunks on this
                                    // worker keeps the thread-local
                                    // per-column worldgen cache hot for
                                    // chunks 2..N (the cold column data is
                                    // ~26 ms; a warm chunk ~3 ms).
                                    for (coord, halo) in
                                        gj.coords.iter().zip(gj.halos.iter())
                                    {
                                    let coord = *coord;
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
                                        match world_save::chunks::read_chunk(slug, &gj.dim, coord) {
                                            Ok(Some(payload)) => {
                                                if let Ok(ch) = payload.voxels.into_chunk() {
                                                    chunk_opt = Some(ch);
                                                }
                                            }
                                            Ok(None) => {}
                                            Err(err) => {
                                                tracing::warn!(
                                                    coord = ?coord,
                                                    error = %err,
                                                    "W46.25: read_chunk failed; falling back to worldgen",
                                                );
                                            }
                                        }
                                    }
                                    let _prof_t0 = std::time::Instant::now();
                                    let chunk = chunk_opt.unwrap_or_else(|| {
                                        worldgen::generate_chunk_at_ctx(
                                            coord, &gj.ctx,
                                        )
                                    });
                                    let _prof_gen_us =
                                        _prof_t0.elapsed().as_micros() as u64;
                                    let _prof_t1 = std::time::Instant::now();
                                    let origin = World::chunk_world_origin(coord);

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
                                                // W127: note rails so the
                                                // main thread re-meshes with
                                                // derived shapes (once per
                                                // chunk - dedup below).
                                                if crate::block::rail::is_rail_block(b.0)
                                                    && !side_effects.iter().any(|se| {
                                                        matches!(se, ChunkSideEffect::Rail)
                                                    })
                                                {
                                                    side_effects.push(ChunkSideEffect::Rail);
                                                }
                                                // Snow cap (block id 15).
                                                if b.0 == 15 {
                                                    side_effects.push(
                                                        ChunkSideEffect::Snow(wv),
                                                    );
                                                // W242: structure loot chest.
                                                // W249: the loot-table
                                                // pick happens HERE on
                                                // the worker (layout +
                                                // column caches warm) -
                                                // deciding it at delivery
                                                // built the whole MC
                                                // noise stack on the
                                                // MAIN thread (100-200 ms
                                                // folds near villages).
                                                } else if b.0 == crate::block::CHEST_ID {
                                                    side_effects.push(
                                                        ChunkSideEffect::DungeonChest {
                                                            pos: wv,
                                                            table: chest_loot_kind(
                                                                &gj.ctx, wv,
                                                            ),
                                                        },
                                                    );
                                                // W242.2: monster spawner.
                                                // W250: the entity kind
                                                // resolves HERE (the
                                                // mineshaft-corridor
                                                // stand-in probe needs
                                                // worldgen caches; on
                                                // the main thread it
                                                // built the noise
                                                // stack).
                                                } else if b.0 == crate::block::SPAWNER_ID {
                                                    side_effects.push(
                                                        ChunkSideEffect::Spawner {
                                                            pos: wv,
                                                            kind: crate::mob::spawner::spawner_entity_kind_at(
                                                                &gj.ctx, wv,
                                                            ),
                                                        },
                                                    );
                                                // W38.2: any open-flame block
                                                // registers a particle emitter.
                                                } else if flame_emitter(b.0) {
                                                    side_effects.push(
                                                        ChunkSideEffect::Torch(wv),
                                                    );
                                                } else if let Some(lq) = liquid_for(b) {
                                                    // W115: only schedule a tick when the cell
                                                    // can actually move - air DIRECTLY BELOW means
                                                    // it can fall / feed a flow (cliff edge, cave).
                                                    // A worldgen sea cell rests on seabed or more
                                                    // water, so it's already at equilibrium:
                                                    // rendering it is enough. Scheduling every ocean
                                                    // cell made the whole sea re-settle on load -
                                                    // the never-quiet edge oscillation that warped
                                                    // the surface into "walls" AND spiked the
                                                    // sky-light relight (the near-water stutter).
                                                    // tick_offset 0 = render only, don't schedule.
                                                    // ly==0's below cell is in the chunk underneath
                                                    // (unknown here) -> treat as stable; a real
                                                    // falling column still ticks via its higher
                                                    // cells. Placed/edited liquid is unaffected (it
                                                    // schedules through set_block, not here).
                                                    // W192: skip fully-interior liquid cells
                                                    // (every within-chunk neighbour the same
                                                    // liquid, not on a chunk border). Such a cell
                                                    // has no visible face, can't move, and never
                                                    // ticks - registering every one made an ocean
                                                    // ~900k liquid_positions, and the per-chunk mesh
                                                    // signature rescans that whole set each time a
                                                    // chunk streams in (the FPS drop approaching
                                                    // water). The mesh iterates chunk cells, not
                                                    // liquid_positions, so surface faces still
                                                    // render; an edit re-registers the now-exposed
                                                    // cells via set_block, so flow still works.
                                                    let on_border = lx == 0
                                                        || ly == 0
                                                        || lz == 0
                                                        || lx == cs - 1
                                                        || ly == cs - 1
                                                        || lz == cs - 1;
                                                    let same = |dx: i32, dy: i32, dz: i32| {
                                                        chunk
                                                            .get(
                                                                (lx + dx) as usize,
                                                                (ly + dy) as usize,
                                                                (lz + dz) as usize,
                                                            )
                                                            .0
                                                            == b.0
                                                    };
                                                    let exposed = on_border
                                                        || !same(-1, 0, 0)
                                                        || !same(1, 0, 0)
                                                        || !same(0, -1, 0)
                                                        || !same(0, 1, 0)
                                                        || !same(0, 0, -1)
                                                        || !same(0, 0, 1);
                                                    if !exposed {
                                                        continue;
                                                    }
                                                    let below_air = ly > 0
                                                        && chunk
                                                            .get(
                                                                lx as usize,
                                                                (ly - 1) as usize,
                                                                lz as usize,
                                                            )
                                                            .is_air();
                                                    // W221: SIDEWAYS air is an unstable
                                                    // boundary from generation - the
                                                    // aquifer's dry pockets and carver
                                                    // tunnels cut into water bodies. MC
                                                    // floods those via its scheduled
                                                    // post-processing fluid ticks
                                                    // (shouldScheduleFluidUpdate); we
                                                    // schedule the same cells here. Air
                                                    // ABOVE only stays a stable surface
                                                    // (never scheduled), so oceans and
                                                    // lakes stay quiet as before.
                                                    let side_air = !below_air
                                                        && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                                                            .into_iter()
                                                            .any(|(dx, dz)| {
                                                                let nx = lx + dx;
                                                                let nz = lz + dz;
                                                                (0..cs).contains(&nx)
                                                                    && (0..cs).contains(&nz)
                                                                    && chunk
                                                                        .get(
                                                                            nx as usize,
                                                                            ly as usize,
                                                                            nz as usize,
                                                                        )
                                                                        .is_air()
                                                            });
                                                    side_effects.push(
                                                        ChunkSideEffect::Liquid {
                                                            pos: wv,
                                                            tick_offset: if below_air || side_air {
                                                                lq.tick_period.max(1) as u64
                                                            } else {
                                                                0
                                                            },
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    }

                                    // W279: drain the oriented template
                                    // placements the carve recorded for
                                    // THIS chunk - they feed the mesher
                                    // (stairs/doors render their real
                                    // facing on first sight) and ride to
                                    // the main-thread state maps as side
                                    // effects.
                                    let oriented =
                                        crate::worldgen::mcgen::village::take_carved_oriented();
                                    let mut gen_doors: std::collections::HashMap<
                                        [i32; 3],
                                        (u8, bool),
                                    > = std::collections::HashMap::new();
                                    let mut gen_stairs: std::collections::HashMap<
                                        [i32; 3],
                                        u8,
                                    > = std::collections::HashMap::new();
                                    for o in &oriented {
                                        match o.kind {
                                            0 => {
                                                gen_stairs.insert(o.pos, o.state);
                                            }
                                            // Doors, trapdoors + fence
                                            // gates all read the door
                                            // snapshot in the mesher.
                                            _ => {
                                                gen_doors
                                                    .insert(o.pos, (o.state, o.open));
                                            }
                                        }
                                        side_effects.push(ChunkSideEffect::Oriented {
                                            pos: o.pos,
                                            kind: o.kind,
                                            state: o.state,
                                            open: o.open,
                                        });
                                    }
                                    // W302: structure-embedded mobs the
                                    // carve found in this chunk.
                                    for m in
                                        crate::worldgen::mcgen::village::take_carved_mobs()
                                    {
                                        side_effects.push(
                                            ChunkSideEffect::StructureMob {
                                                anchor: m.anchor,
                                                kind: m.kind,
                                                baby: m.baby,
                                            },
                                        );
                                    }
                                    let _prof_scan_us =
                                        _prof_t1.elapsed().as_micros() as u64;
                                    // W24: Build MC-atlas submeshes in the
                                    // worker using the pre-fetched halo
                                    // lighting + pack snapshot. Main thread
                                    // only does GPU upload.
                                    // W125/W127: player doors/rails can't
                                    // exist in a freshly-generated chunk;
                                    // the W279 template states stand in.
                                    let _prof_t2 = std::time::Instant::now();
                                    let no_rails = std::collections::HashMap::new();
                                    // W316: fresh worldgen snow is
                                    // always 1 layer - the mesher
                                    // defaults on a missing entry.
                                    let no_snow = std::collections::HashMap::new();
                                    let submeshes = mesh_chunk_in_worker(
                                        &chunk,
                                        coord,
                                        &gj.pack,
                                        halo,
                                        &gen_doors,
                                        &no_rails,
                                        &gen_stairs,
                                        &no_snow,
                                    );
                                    let _prof_mesh_us =
                                        _prof_t2.elapsed().as_micros() as u64;
                                    GEN_PROF.with(|p| {
                                        let mut p = p.borrow_mut();
                                        p.0 += _prof_gen_us;
                                        p.1 += _prof_scan_us;
                                        p.2 += _prof_mesh_us;
                                        p.3 += 1;
                                        if p.3 >= 512 {
                                            tracing::info!(
                                                avg_gen_ms =
                                                    p.0 as f32 / 1000.0 / p.3 as f32,
                                                avg_scan_ms =
                                                    p.1 as f32 / 1000.0 / p.3 as f32,
                                                avg_mesh_ms =
                                                    p.2 as f32 / 1000.0 / p.3 as f32,
                                                chunks = p.3,
                                                "W305: worker gen profile"
                                            );
                                            *p = (0, 0, 0, 0);
                                        }
                                    });

                                    if res_tx
                                        .send(WorkerResult::Generated {
                                            coord,
                                            chunk,
                                            side_effects,
                                            submeshes,
                                            dim: gj.dim.clone(),
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                    } // W239: end of column-batch loop
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
                                        &rj.doors,
                                        &rj.rails,
                                        &rj.stairs,
                                        &rj.snow,
                                    );
                                    if res_tx
                                        .send(WorkerResult::Remeshed {
                                            coord: rj.coord,
                                            revision: rj.revision,
                                            submeshes,
                                            dim: rj.dim,
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
                                    // W275: keep the warm-start Arcs for the
                                    // post-BFS diff (refcount bumps only; the
                                    // engines copy-on-write shared grids).
                                    let warm_start = lj.existing_grids.clone();
                                    let mut local_state =
                                        lighting::LightingState::with_grids(lj.existing_grids);
                                    // W207: MC's last-chunk cache in front of the
                                    // snapshot - the BFS's per-cell HashMap lookup
                                    // was the bulk of sky_ms/block_ms.
                                    let access =
                                        lighting::CachedSnapshotAccess::new(&lj.snapshot);
                                    lighting::block_engine::propagate_block_light(
                                        &access,
                                        &mut local_state,
                                        &lj.affected,
                                    );
                                    let block_done_ms = bfs_start.elapsed().as_millis() as u64;
                                    lighting::sky_engine::propagate_sky_light(
                                        &access,
                                        &mut local_state,
                                        &lj.affected,
                                    );
                                    let total_ms = bfs_start.elapsed().as_millis() as u64;
                                    // W234: demoted to debug + a WARN only for
                                    // outliers. At INFO this printed once per
                                    // lighting job from up to 24 worker threads
                                    // at once during streaming - the stdout
                                    // writer lock serialized them AND blocked
                                    // the main thread's own log calls for
                                    // 25-105 ms per line on a live console
                                    // (the user's "lit" stream hitches; a
                                    // file-redirected run never showed it).
                                    if total_ms >= 750 {
                                        tracing::warn!(
                                            chunks = chunk_count,
                                            block_ms = block_done_ms,
                                            sky_ms = total_ms - block_done_ms,
                                            total_ms,
                                            "W46.30: slow lighting BFS job",
                                        );
                                    } else {
                                        tracing::debug!(
                                            chunks = chunk_count,
                                            block_ms = block_done_ms,
                                            sky_ms = total_ms - block_done_ms,
                                            total_ms,
                                            "W46.30: lighting BFS done",
                                        );
                                    }
                                    // W275: DIFF against the warm-start
                                    // grids and ship only chunks whose
                                    // light actually changed. The old
                                    // path returned the WHOLE segment
                                    // unconditionally, so the post-load
                                    // relight of a 105k-chunk world
                                    // queued ~100 redundant remeshes per
                                    // Lit result - pending_remesh pinned
                                    // at ~26k, every mesh churned through
                                    // remove+create (falling to the
                                    // legacy render path once the uber
                                    // pool filled) and render_ms crawled
                                    // to 145-166. Identical grids don't
                                    // need a grid swap or a remesh.
                                    let mut new_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>> =
                                        HashMap::new();
                                    let mut affected_for_remesh: Vec<ChunkCoord> = Vec::new();
                                    for cc in &lj.affected {
                                        let Some(new_g) = local_state.get(*cc) else {
                                            continue;
                                        };
                                        let unchanged = warm_start
                                            .get(cc)
                                            .is_some_and(|old| **old == **new_g);
                                        if unchanged {
                                            continue;
                                        }
                                        new_grids.insert(*cc, Arc::clone(new_g));
                                        affected_for_remesh.push(*cc);
                                    }
                                    if res_tx
                                        .send(WorkerResult::Lit {
                                            new_grids,
                                            affected_for_remesh,
                                            dim: lj.dim,
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
            light_tx,
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
        let (dummy_light, _) = cb_unbounded::<Job>();
        let _ = std::mem::replace(&mut self.light_tx, dummy_light);
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn mesh_chunk_in_worker(
    chunk: &Chunk,
    coord: ChunkCoord,
    pack: &resource_pack::ResourcePack,
    halo: &HaloLighting,
    doors: &std::collections::HashMap<[i32; 3], (u8, bool)>,
    rails: &std::collections::HashMap<[i32; 3], u8>,
    stairs: &std::collections::HashMap<[i32; 3], u8>,
    // W316: snow-layer heights (id 15 cells; missing = 1 layer).
    snow: &std::collections::HashMap<[i32; 3], u8>,
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
        // W126: the chest is a block ENTITY - render::chests draws
        // it per frame with the real entity sheets + lid animation.
        // The chunk mesh emits nothing for its cell.
        if block.0 == crate::block::CHEST_ID {
            return true;
        }
        // W279: stairs with a recorded orientation (template
        // placements) emit the rotated two-box shape; stairs without
        // one fall through to the static default model below.
        if crate::block::door::is_stairs_block(block.0) {
            let wp = [
                world_origin[0] + lx,
                world_origin[1] + ly,
                world_origin[2] + lz,
            ];
            if let Some(state) = stairs.get(&wp).copied() {
                let light = halo.lookup(coord, wp);
                resource_pack::element_mesh::emit_stairs(
                    verts, idxs, voxel_origin, block.0, state, pack, light,
                );
                return true;
            }
            return false;
        }
        // W129: trapdoors render closed (floor slab) or open
        // (panel at the hinge edge) from the same door snapshot.
        if crate::block::door::is_trapdoor_block(block.0) {
            let wp = [
                world_origin[0] + lx,
                world_origin[1] + ly,
                world_origin[2] + lz,
            ];
            let (facing, open) =
                doors.get(&wp).copied().unwrap_or((3, false));
            let light = halo.lookup(coord, wp);
            resource_pack::element_mesh::emit_trapdoor(
                verts, idxs, voxel_origin, block.0, facing, open, pack,
                light,
            );
            return true;
        }
        // W316: snow layers mesh as a thin slab (the per-cell decor
        // draw was thousands of draw calls per frame in snow biomes).
        if block.0 == 15 {
            let wp = [
                world_origin[0] + lx,
                world_origin[1] + ly,
                world_origin[2] + lz,
            ];
            let layers = snow.get(&wp).copied().unwrap_or(1);
            let light = halo.lookup(coord, wp);
            resource_pack::element_mesh::emit_snow_layer(
                verts, idxs, voxel_origin, layers, pack, light,
            );
            return true;
        }
        // W288: wall torches lean off their wall; floor torches (no
        // orient row) keep the static standing model fallback.
        if block.0 == crate::block::TORCH_ID {
            let wp = [
                world_origin[0] + lx,
                world_origin[1] + ly,
                world_origin[2] + lz,
            ];
            if let Some(&(facing, _)) = doors.get(&wp) {
                let light = halo.lookup(coord, wp);
                resource_pack::element_mesh::emit_wall_torch(
                    verts, idxs, voxel_origin, block.0, facing, pack, light,
                );
                return true;
            }
            return false;
        }
        // W167: fence gate - closed = two posts + two rails across the
        // facing axis; open = posts only (a gap to walk through).
        if crate::block::door::is_fence_gate_block(block.0) {
            let wp = [
                world_origin[0] + lx,
                world_origin[1] + ly,
                world_origin[2] + lz,
            ];
            let (facing, open) =
                doors.get(&wp).copied().unwrap_or((0, false));
            let light = halo.lookup(coord, wp);
            resource_pack::element_mesh::emit_fence_gate(
                verts, idxs, voxel_origin, block.0, facing, open, pack,
                light,
            );
            return true;
        }
        // W127: rails render as a flat quad per the derived shape.
        if crate::block::rail::is_rail_block(block.0) {
            let wp = [
                world_origin[0] + lx,
                world_origin[1] + ly,
                world_origin[2] + lz,
            ];
            let shape = rails.get(&wp).copied().unwrap_or(0);
            let light = halo.lookup(coord, wp);
            resource_pack::element_mesh::emit_rail_quad(
                verts, idxs, voxel_origin, block.0, shape, pack, light,
            );
            return true;
        }
        // W125: doors render from the baked state snapshot (facing +
        // open per lower half). Handled here so the static west-edge
        // `door_lower` model never runs for doors.
        if crate::block::door::is_door_block(block.0) {
            // W311: an upper door cell has no doors-map row of its
            // own - inherit the LOWER cell's facing/open so both
            // halves swing together (template doors write the same
            // id into both cells now).
            let wp = [
                world_origin[0] + lx,
                world_origin[1] + ly,
                world_origin[2] + lz,
            ];
            let below = [wp[0], wp[1] - 1, wp[2]];
            // Snapshot rows are keyed by the LOWER half, so the map
            // itself answers which half this voxel is - works across
            // a vertical chunk boundary where the sibling voxel
            // isn't in this chunk snapshot.
            let (lower, is_upper) = if doors.contains_key(&wp) {
                (wp, false)
            } else if doors.contains_key(&below) {
                (below, true)
            } else {
                // Unmapped door (pre-W125 save): infer from the
                // local chunk; boundary uppers degrade to closed.
                let upper_guess = ly > 0
                    && crate::block::door::is_door_block(
                        chunk
                            .get(lx as usize, (ly - 1) as usize, lz as usize)
                            .0,
                    );
                (if upper_guess { below } else { wp }, upper_guess)
            };
            let (facing, open) =
                doors.get(&lower).copied().unwrap_or((0, false));
            let light = halo.lookup(coord, wp);
            resource_pack::element_mesh::emit_door_panel(
                verts, idxs, voxel_origin, block.0, is_upper, facing,
                open, pack, light,
            );
            return true;
        }
        // W40.1: compute neighbour connection flags for multipart.
        // W133: family-aware (MC fence/wall/pane rules) instead of
        // strict same-id - fences grab other wood fences + solids.
        let same_id = |dx: i32, dy: i32, dz: i32| -> bool {
            let nx = lx + dx;
            let ny = ly + dy;
            let nz = lz + dz;
            if (0..CHUNK_SIZE as i32).contains(&nx)
                && (0..CHUNK_SIZE as i32).contains(&ny)
                && (0..CHUNK_SIZE as i32).contains(&nz)
            {
                let nb = chunk.get(nx as usize, ny as usize, nz as usize);
                crate::block::multipart_connects(block.0, nb)
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

    // W122: AO occluder query. In-range cells read the chunk directly;
    // out-of-range neighbour cells return `false` (no contact shadow at
    // the chunk seam yet - a full cross-chunk occupancy halo is a
    // follow-up). `block_fully_occludes` so inset blocks (cactus, ...)
    // don't cast a hard AO corner.
    let occluder_at = |local: [i32; 3]| -> bool {
        if (0..CHUNK_SIZE as i32).contains(&local[0])
            && (0..CHUNK_SIZE as i32).contains(&local[1])
            && (0..CHUNK_SIZE as i32).contains(&local[2])
        {
            let b = chunk.get(local[0] as usize, local[1] as usize, local[2] as usize);
            !b.is_air() && crate::block::block_fully_occludes(b)
        } else {
            false
        }
    };

    // W34.2: worker also uses greedy mesher, matching build_lit_submeshes.
    let subs = chunk_mesh_greedy_with_special(
        chunk,
        origin,
        uv_resolver,
        // W109: block_fully_occludes (not block_is_opaque) so the cactus's
        // inset model doesn't cull the sand top under it (you'd see the void
        // through the 1 px inset gap). Lighting still uses block_is_opaque.
        crate::block::block_fully_occludes,
        |air_local, _normal| light_lookup(air_local),
        anim_lookup,
        occluder_at,
        emit_special,
    );
    // W240: collapse the per-block submeshes into one per render
    // layer (block ids move into the vertices; the shader reads the
    // per-block params from the uniform table).
    crate::render::chunk_merge::merge_submeshes(subs)
}

// W47.27 - chunk worker types relocated from main.rs.
/// W28.1: Entry in the priority queue for pending chunk generation.
/// BinaryHeap is max-heap; we negate squared distance so the chunk
/// closest to the player sorts highest (pops first).
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct PendingChunk {
    /// Negated squared XZ distance from player chunk (so closest = max).
    pub(crate) neg_dist_sq: i64,
    /// The chunk coord to generate.
    pub(crate) coord: ChunkCoord,
}

impl Ord for PendingChunk {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.neg_dist_sq
            .cmp(&other.neg_dist_sq)
            .then_with(|| self.coord.cmp(&other.coord))
    }
}

impl PartialOrd for PendingChunk {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}


/// W13.2: Side-effect detected in a freshly-generated chunk while
/// scanning its voxels on the worker thread. Folded into the
/// per-world index tables on the main thread after GPU upload.
pub(crate) enum ChunkSideEffect {
    /// Snow voxel (id 15) at this world position.
    Snow([i32; 3]),
    /// Torch or glowstone emitter at this world position.
    Torch([i32; 3]),
    /// Liquid voxel at this world position: `tick_offset` is
    /// `liquid.tick_period.max(1)` pre-computed on the worker so
    /// the main thread only does the addition.
    Liquid { pos: [i32; 3], tick_offset: u64 },
    /// W127: a rail voxel exists in this chunk. The Generate path
    /// meshes with an EMPTY rail-shape snapshot (the worker can't
    /// see neighbours), so the main thread queues one remesh for
    /// the chunk - that rebuild carries the derived shapes.
    Rail,
    /// W242: a worldgen chest (monster room / future structures) at
    /// this world position. Players can't have built in a chunk that
    /// is only now being generated, so every chest the scan finds is
    /// structure loot: the main thread mints its container with a
    /// seed-deterministic loot roll + derives the facing. W249: the
    /// loot-table kind is decided by the WORKER (structure-membership
    /// probes need the worldgen caches; running them at delivery cost
    /// the main thread the whole MC noise stack).
    DungeonChest { pos: [i32; 3], table: ChestLootKind },
    /// W242.2: a monster-spawner block at this world position. The
    /// main thread folds it into `Voxel.spawner_positions` so the
    /// spawner tick + cage particles walk a small index instead of
    /// scanning chunks. W250: the entity kind rides along - the
    /// mineshaft-corridor membership probe behind the cave-spider
    /// stand-in needs the worldgen caches only workers keep warm.
    Spawner { pos: [i32; 3], kind: u8 },
    /// W279: a template-placed oriented block (stairs / door lower /
    /// trapdoor / fence gate). The main thread folds it into the
    /// per-block state maps so remeshes keep the orientation. Kinds:
    /// 0 stairs, 1 door, 2 trapdoor, 3 fence gate; `state` = facing
    /// (bits 0-1) | top-half bit 2.
    Oriented { pos: [i32; 3], kind: u8, state: u8, open: bool },
    /// W302: a template-embedded entity (villager / pen animal). The
    /// main thread dedups on `anchor` via the persistent
    /// `structure_mob_anchors` set and mints a persistent mob.
    StructureMob { anchor: [i32; 3], kind: u8, baby: bool },
}

/// W249: which loot table a worldgen chest rolls at delivery. Decided
/// on the worker via the deterministic structure-membership probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChestLootKind {
    SimpleDungeon,
    AbandonedMineshaft,
    DesertPyramid,
    IglooChest,
    PillagerOutpost,
    ShipwreckMap,
    ShipwreckTreasure,
    ShipwreckSupply,
    BuriedTreasure,
    UnderwaterRuinSmall,
    UnderwaterRuinBig,
    JungleTemple,
    StrongholdCorridor,
    StrongholdCrossing,
    StrongholdLibrary,
    RuinedPortal,
    WoodlandMansion,
    AncientCity,
    AncientCityIceBox,
    TrialChambersCorridor,
    TrialChambersEntrance,
    TrialChambersIntersection,
    TrialChambersSupply,
    BastionTreasure,
    BastionOther,
    BastionBridge,
    BastionHoglinStable,
    NetherBridge,
}

/// The membership chain behind [`ChestLootKind`]. Worker-side only:
/// the probes replay structure layouts through the per-thread
/// worldgen caches, which are warm right after generating the chunk.
pub(crate) fn chest_loot_kind(
    ctx: &crate::worldgen::WorldgenCtx,
    pos: [i32; 3],
) -> ChestLootKind {
    use crate::worldgen::mcgen;
    if mcgen::mineshaft::pos_in_mineshaft(ctx, pos) {
        ChestLootKind::AbandonedMineshaft
    } else if mcgen::pyramid::pos_in_desert_pyramid(ctx, pos) {
        ChestLootKind::DesertPyramid
    } else if mcgen::igloo::pos_in_igloo(ctx, pos) {
        ChestLootKind::IglooChest
    } else if mcgen::outpost::pos_in_outpost(ctx, pos) {
        ChestLootKind::PillagerOutpost
    } else if let Some(k) = mcgen::shipwreck::chest_kind_at(ctx, pos) {
        match k {
            mcgen::shipwreck::ShipwreckChest::Map => ChestLootKind::ShipwreckMap,
            mcgen::shipwreck::ShipwreckChest::Treasure => {
                ChestLootKind::ShipwreckTreasure
            }
            mcgen::shipwreck::ShipwreckChest::Supply => ChestLootKind::ShipwreckSupply,
        }
    } else if mcgen::buried_treasure::is_treasure_chest(ctx, pos) {
        ChestLootKind::BuriedTreasure
    } else if let Some(big) = mcgen::ocean_ruin::chest_is_big_at(ctx, pos) {
        if big {
            ChestLootKind::UnderwaterRuinBig
        } else {
            ChestLootKind::UnderwaterRuinSmall
        }
    } else if mcgen::jungle_temple::pos_in_jungle_temple(ctx, pos) {
        ChestLootKind::JungleTemple
    } else if let Some(k) = mcgen::stronghold::chest_kind_at(ctx, pos) {
        match k {
            mcgen::stronghold::StrongholdChest::Corridor => {
                ChestLootKind::StrongholdCorridor
            }
            mcgen::stronghold::StrongholdChest::Crossing => {
                ChestLootKind::StrongholdCrossing
            }
            mcgen::stronghold::StrongholdChest::Library => {
                ChestLootKind::StrongholdLibrary
            }
        }
    } else if mcgen::ruined_portal::pos_in_ruined_portal(ctx, pos) {
        ChestLootKind::RuinedPortal
    } else if mcgen::mansion::pos_in_mansion(ctx, pos) {
        ChestLootKind::WoodlandMansion
    } else if let Some(k) = mcgen::ancient_city::city_chest_at(ctx, pos) {
        match k {
            mcgen::ancient_city::CityChest::IceBox => ChestLootKind::AncientCityIceBox,
            mcgen::ancient_city::CityChest::Regular => ChestLootKind::AncientCity,
        }
    } else if let Some(k) = mcgen::trial_chambers::chamber_chest_at(ctx, pos) {
        match k {
            mcgen::trial_chambers::ChamberChest::Corridor => {
                ChestLootKind::TrialChambersCorridor
            }
            mcgen::trial_chambers::ChamberChest::Entrance => {
                ChestLootKind::TrialChambersEntrance
            }
            mcgen::trial_chambers::ChamberChest::Intersection => {
                ChestLootKind::TrialChambersIntersection
            }
            mcgen::trial_chambers::ChamberChest::Supply => {
                ChestLootKind::TrialChambersSupply
            }
        }
    } else if let Some(k) = mcgen::bastion::bastion_chest_at(ctx, pos) {
        match k {
            mcgen::bastion::BastionChest::Treasure => ChestLootKind::BastionTreasure,
            mcgen::bastion::BastionChest::HoglinStable => {
                ChestLootKind::BastionHoglinStable
            }
            mcgen::bastion::BastionChest::Bridge => ChestLootKind::BastionBridge,
            mcgen::bastion::BastionChest::Other => ChestLootKind::BastionOther,
        }
    } else if mcgen::fortress::is_fortress_chest(ctx, pos) {
        ChestLootKind::NetherBridge
    } else {
        ChestLootKind::SimpleDungeon
    }
}

// W24: Snapshot of 6-neighbour light grids shipped with each ChunkJob.
// Defined in `lighting::HaloLighting`; re-exported here so the worker
// pool code and tests can refer to it as plain `HaloLighting`.

/// W25 / W27: output from one worker thread. Three variants:
/// - `Generated`: new chunk from worldgen + side-effects + mesh.
/// - `Remeshed`: remesh of an existing chunk (no worldgen, no side-effects).
/// - `Lit`: W27 lighting BFS result for a set of chunks.
///
/// The `revision` field on `Remeshed` carries the snapshot revision so
/// the main thread can discard stale results when the chunk was edited
/// again before the first remesh returned.
pub(crate) enum WorkerResult {
    /// W13 / W24: new chunk generated by `generate_chunk_at_ctx`.
    Generated {
        coord: ChunkCoord,
        chunk: Chunk,
        /// W13.2: voxel-scan results computed off-thread.
        side_effects: Vec<ChunkSideEffect>,
        /// W24: MC-atlas submeshes built by the worker.
        submeshes: Vec<infengine_voxel::ChunkSubmesh>,
        /// W277: the dim the job was enqueued against. The drain drops
        /// results whose dim no longer matches `current_dim` - a
        /// portal ping-pong used to let an old dim's in-flight chunks
        /// land in the NEW dim's world, permanently shadowing worldgen
        /// at those coords (the "no chunks load anywhere" breakage).
        dim: String,
    },
    /// W25: remesh of an already-loaded chunk (block edit / neighbour
    /// update). No worldgen, no side-effect scan.
    Remeshed {
        coord: ChunkCoord,
        /// Revision at snapshot time. Main thread drops this result if
        /// `chunk_revisions[coord]` has advanced past it.
        revision: u64,
        submeshes: Vec<infengine_voxel::ChunkSubmesh>,
        /// W277: see `Generated::dim`.
        dim: String,
    },
    /// W27: lighting BFS result. The worker computed new LightGrids for
    /// `affected` chunks; the main thread swaps them in and triggers a
    /// remesh of every affected chunk so the GPU picks up new light values.
    Lit {
        /// Updated light grids, one per chunk in `affected`.
        new_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>>,
        /// Chunks that should be remeshed now that their lighting changed.
        affected_for_remesh: Vec<ChunkCoord>,
        /// W277: see `Generated::dim` (stale Lit still decrements the
        /// in-flight counter, its grids are just discarded).
        dim: String,
    },
}

/// W25 / W27: a single unit of work sent to a worker thread.
/// - `Generate`: full worldgen + mesh for a new chunk.
/// - `Remesh`: mesh-only for an existing chunk snapshot.
/// - `Lighting`: W27 lighting BFS over an owned chunk snapshot.
pub(crate) enum Job {
    /// Full generation + mesh path (pre-W25 ChunkJob).
    Generate(ChunkJob),
    /// W25: remesh-only path; skips worldgen and side-effect scan.
    Remesh(RemeshJob),
    /// W27: lighting BFS over an owned snapshot of affected chunks +
    /// their face-neighbours. No worldgen, no mesh.
    Lighting(LightingJob),
}

/// W27: lighting BFS job. The main thread clones the affected chunks
/// (and their 6 face-neighbours for BFS frontier correctness) into an
/// owned snapshot so the worker can run BFS without any borrow into the
/// live `World`. Cloning ~27 chunks x ~8 KB = ~216 KB per tick_lighting
/// call; acceptable.
pub(crate) struct LightingJob {
    /// Chunks whose light grids must be recomputed. All entries must be
    /// present in `snapshot`.
    pub(crate) affected: Vec<ChunkCoord>,
    /// Owned voxel snapshot: affected chunks + their 6 face-neighbours.
    pub(crate) snapshot: lighting::ChunkSnapshot,
    /// Existing light grids for `affected` (warm-start). The BFS clears
    /// and re-floods so these are optional context; passing them avoids
    /// a cold-start cost on subsequent tick_lighting calls.
    pub(crate) existing_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>>,
    /// W277: dim at dispatch time (echoed in the result).
    pub(crate) dim: String,
}

/// One unit of work for the chunk worker pool. Carries the per-job
/// `WorldgenCtx` snapshot captured at enqueue time, so the worker
/// generates the chunk against the EXACT base seed + dim + profile
/// the player saw when the request was made. Without this, a
/// `switch_dim` between enqueue and pickup poisoned the result
/// (the worker would read fresh atomics from the wrong dim).
///
/// W24: also carries `Arc<ResourcePack>` + `HaloLighting` so the
/// worker can build the full MC-atlas mesh without shared mutable
/// state.
pub(crate) struct ChunkJob {
    /// W239: every queued chunk of ONE vertical column (same cx/cz),
    /// batched so the expensive per-column worldgen data (density
    /// lattice + carve mask, ~26 ms cold) computes ONCE on one
    /// worker's thread-local cache. The old one-coord-per-job
    /// dispatch scattered a column's 16 chunks across the pool and
    /// recomputed that data on nearly every worker it touched
    /// (measured 465 ms vs 80 ms per column).
    pub(crate) coords: Vec<ChunkCoord>,
    pub(crate) ctx: worldgen::WorldgenCtx,
    /// W24: resource pack snapshot for UV / anim lookup.
    pub(crate) pack: Arc<resource_pack::ResourcePack>,
    /// W24: pre-fetched light grids (own chunk + 6 neighbours),
    /// parallel to `coords`.
    pub(crate) halos: Vec<HaloLighting>,
    /// W46.25: current world slug + dim so the worker reads the
    /// chunk from disk before falling through to worldgen. Without
    /// this the streamer regenerated every reloaded chunk from
    /// scratch, wiping every player edit (placed torch / obsidian /
    /// anything) that landed in a chunk after the player walked
    /// past UNLOAD_DISTANCE.
    pub(crate) slug: Option<String>,
    pub(crate) dim: String,
}

/// W25: remesh-only job. Carries a clone of the chunk's voxel data
/// (Chunk is ~8 KB; clone is fast) so the worker can mesh without
/// shared mutable state. No worldgen, no side-effect scan.
pub(crate) struct RemeshJob {
    pub(crate) coord: ChunkCoord,
    /// Snapshot of the chunk voxels at dispatch time.
    pub(crate) chunk: Chunk,
    /// Resource pack snapshot for UV / anim lookup.
    pub(crate) pack: Arc<resource_pack::ResourcePack>,
    /// Pre-fetched light grids for own chunk + 6 neighbours.
    pub(crate) halo: HaloLighting,
    /// Revision counter at dispatch time. Used by the main thread to
    /// discard stale results (a second edit arrived before this one
    /// returned).
    pub(crate) revision: u64,
    /// W316: snow-layer heights for cells in this chunk (id 15).
    pub(crate) snow: std::collections::HashMap<[i32; 3], u8>,
    /// W125: door state snapshot for doors intersecting this chunk:
    /// LOWER-half world pos -> (facing, open). The worker can't see
    /// `Voxel::door_open` / `door_facing`, so the dispatcher bakes
    /// them in - without this the remesh always drew doors closed
    /// (the old "door never visually opens" bug).
    pub(crate) doors: std::collections::HashMap<[i32; 3], (u8, bool)>,
    /// W127: derived rail shapes for rails in this chunk (world pos
    /// -> RailShape code). Computed on the main thread where the
    /// full World is visible (cross-chunk neighbours included).
    pub(crate) rails: std::collections::HashMap<[i32; 3], u8>,
    /// W279: stair orientation states for this chunk (world pos ->
    /// facing | top bit), from `Voxel::stair_states`.
    pub(crate) stairs: std::collections::HashMap<[i32; 3], u8>,
    /// W277: dim at dispatch time (echoed in the result).
    pub(crate) dim: String,
}

