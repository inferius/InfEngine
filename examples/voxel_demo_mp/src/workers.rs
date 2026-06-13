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
                                                    let below_air = ly > 0
                                                        && chunk
                                                            .get(
                                                                lx as usize,
                                                                (ly - 1) as usize,
                                                                lz as usize,
                                                            )
                                                            .is_air();
                                                    side_effects.push(
                                                        ChunkSideEffect::Liquid {
                                                            pos: wv,
                                                            tick_offset: if below_air {
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

                                    // W24: Build MC-atlas submeshes in the
                                    // worker using the pre-fetched halo
                                    // lighting + pack snapshot. Main thread
                                    // only does GPU upload.
                                    // W125/W127: freshly-generated chunks
                                    // carry no doors or rails (worldgen
                                    // doesn't place them) - empty snapshots.
                                    let no_doors = std::collections::HashMap::new();
                                    let no_rails = std::collections::HashMap::new();
                                    let submeshes = mesh_chunk_in_worker(
                                        &chunk,
                                        gj.coord,
                                        &gj.pack,
                                        &gj.halo,
                                        &no_doors,
                                        &no_rails,
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
                                        &rj.doors,
                                        &rj.rails,
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
    doors: &std::collections::HashMap<[i32; 3], (u8, bool)>,
    rails: &std::collections::HashMap<[i32; 3], u8>,
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
    chunk_mesh_greedy_with_special(
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
    )
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
    },
    /// W25: remesh of an already-loaded chunk (block edit / neighbour
    /// update). No worldgen, no side-effect scan.
    Remeshed {
        coord: ChunkCoord,
        /// Revision at snapshot time. Main thread drops this result if
        /// `chunk_revisions[coord]` has advanced past it.
        revision: u64,
        submeshes: Vec<infengine_voxel::ChunkSubmesh>,
    },
    /// W27: lighting BFS result. The worker computed new LightGrids for
    /// `affected` chunks; the main thread swaps them in and triggers a
    /// remesh of every affected chunk so the GPU picks up new light values.
    Lit {
        /// Updated light grids, one per chunk in `affected`.
        new_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>>,
        /// Chunks that should be remeshed now that their lighting changed.
        affected_for_remesh: Vec<ChunkCoord>,
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
    pub(crate) coord: ChunkCoord,
    pub(crate) ctx: worldgen::WorldgenCtx,
    /// W24: resource pack snapshot for UV / anim lookup.
    pub(crate) pack: Arc<resource_pack::ResourcePack>,
    /// W24: pre-fetched light grids for own chunk + 6 neighbours.
    pub(crate) halo: HaloLighting,
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
}

