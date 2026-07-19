//! W47.28 - lighting_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {

    /// Build per-block submeshes for `coord` with per-vertex
    /// lighting baked in. Reads `self.lighting` for the
    /// `(block_light, sky_light)` pair on each emitted face -
    /// missing chunks fall back to "no block light, full sky" so
    /// brand-new chunks render as if they're fully daylit until
    /// the next lighting tick catches up.
    pub(crate) fn build_lit_submeshes(
        &self,
        coord: ChunkCoord,
    ) -> Vec<infengine_voxel::ChunkSubmesh> {
        let Some(chunk) = self.world.chunk(coord) else {
            return Vec::new();
        };
        let origin = World::chunk_world_origin(coord);
        // The `light_at` closure receives the air-side cell's
        // world voxel coord + the face normal. We look the cell
        // up in `self.lighting` (which is keyed by world voxel
        // coord internally via `split`).
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let world_origin = [coord[0] * cs, coord[1] * cs, coord[2] * cs];
        let lighting = &self.lighting;
        let pack = &self.resource_pack;
        // Atlas UV resolver: prefer the live MC ResourcePack atlas
        // entry (per-block + per-face slot) so the dynamic atlas
        // layout works. Falls back to the legacy 4x4 grid only when
        // the pack has nothing for the (block, face) key.
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
            // W37: fall through to the magenta missing-texture sentinel
            // baked into the atlas at pack time so unmapped blocks read
            // as the obvious "no texture" pattern, not the legacy 4x4
            // grid which collides with random MC tiles.
            pack.icon_uv_for(block, normal)
        };
        // W16.2: light + anim helpers captured by both the cube path
        // and the emit_special closure.
        let light_lookup = |air_local: [i32; 3]| -> [f32; 2] {
            let world = [
                world_origin[0] + air_local[0],
                world_origin[1] + air_local[1],
                world_origin[2] + air_local[2],
            ];
            let other_cc = [
                world[0].div_euclid(cs),
                world[1].div_euclid(cs),
                world[2].div_euclid(cs),
            ];
            let other_lc = [
                world[0].rem_euclid(cs) as u32,
                world[1].rem_euclid(cs) as u32,
                world[2].rem_euclid(cs) as u32,
            ];
            match lighting.get(other_cc) {
                Some(g) => [
                    g.get_block(other_lc) as f32 / 15.0,
                    g.get_sky(other_lc) as f32 / 15.0,
                ],
                None => [0.0, 1.0],
            }
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
        // W16.2: `emit_special` closure - for blocks with a registered
        // special model, emit per-element geometry and return `true` so
        // the cube path is skipped. For all other blocks return `false`.
        let chunk_ref = chunk;
        let voxel_origin_ref = origin;
        // W125: door state snapshot (facing + open per lower half) -
        // the same bake the worker remesh path receives, so sync- and
        // worker-built chunks render doors identically.
        let door_states = self.door_state_snapshot(coord);
        // W127: derived rail shapes (same snapshot the worker gets).
        let rail_states = self.rail_state_snapshot(coord);
        let emit_special = move |block: BlockId,
                                 voxel_origin: glam::Vec3,
                                 verts: &mut Vec<MeshVertex>,
                                 idxs: &mut Vec<u32>| {
            // W126: chest = block entity; the chunk mesh emits
            // nothing (render::chests draws it per frame).
            if block.0 == crate::block::CHEST_ID {
                return true;
            }
            // W129: trapdoors render closed/open from the door snapshot.
            if crate::block::door::is_trapdoor_block(block.0) {
                let wp = [
                    voxel_origin.x.floor() as i32,
                    voxel_origin.y.floor() as i32,
                    voxel_origin.z.floor() as i32,
                ];
                let (facing, open) =
                    door_states.get(&wp).copied().unwrap_or((3, false));
                let local = [
                    wp[0] - world_origin[0],
                    wp[1] - world_origin[1],
                    wp[2] - world_origin[2],
                ];
                let light = light_lookup(local);
                resource_pack::element_mesh::emit_trapdoor(
                    verts, idxs, voxel_origin, block.0, facing, open,
                    pack, light,
                );
                return true;
            }
            // W127: rails render as a flat quad per the derived shape.
            if crate::block::rail::is_rail_block(block.0) {
                let wp = [
                    voxel_origin.x.floor() as i32,
                    voxel_origin.y.floor() as i32,
                    voxel_origin.z.floor() as i32,
                ];
                let shape = rail_states.get(&wp).copied().unwrap_or(0);
                let local = [
                    wp[0] - world_origin[0],
                    wp[1] - world_origin[1],
                    wp[2] - world_origin[2],
                ];
                let light = light_lookup(local);
                resource_pack::element_mesh::emit_rail_quad(
                    verts, idxs, voxel_origin, block.0, shape, pack, light,
                );
                return true;
            }
            // W125: doors render as a state-aware panel (closed =
            // edge opposite the facing, open = hinge-left edge).
            if crate::block::door::is_door_block(block.0) {
                let wp = [
                    voxel_origin.x.floor() as i32,
                    voxel_origin.y.floor() as i32,
                    voxel_origin.z.floor() as i32,
                ];
                let below = [wp[0], wp[1] - 1, wp[2]];
                let (lower, is_upper) = if door_states.contains_key(&wp) {
                    (wp, false)
                } else if door_states.contains_key(&below) {
                    (below, true)
                } else {
                    // Unmapped door (pre-W125 save): infer the half
                    // from the local chunk; boundary uppers degrade
                    // to closed-lower.
                    let lx = wp[0] - world_origin[0];
                    let ly = wp[1] - world_origin[1];
                    let lz = wp[2] - world_origin[2];
                    let upper_guess = ly > 0
                        && crate::block::door::is_door_block(
                            chunk_ref
                                .get(lx as usize, (ly - 1) as usize, lz as usize)
                                .0,
                        );
                    (if upper_guess { below } else { wp }, upper_guess)
                };
                let (facing, open) =
                    door_states.get(&lower).copied().unwrap_or((0, false));
                let local = [
                    wp[0] - world_origin[0],
                    wp[1] - world_origin[1],
                    wp[2] - world_origin[2],
                ];
                let light = light_lookup(local);
                resource_pack::element_mesh::emit_door_panel(
                    verts, idxs, voxel_origin, block.0, is_upper, facing,
                    open, pack, light,
                );
                return true;
            }
            // Compute voxel local coords from world position.
            let lx = (voxel_origin.x - voxel_origin_ref.x) as i32;
            let ly = (voxel_origin.y - voxel_origin_ref.y) as i32;
            let lz = (voxel_origin.z - voxel_origin_ref.z) as i32;
            // W40.1: compute horizontal neighbour connection flags so
            // multipart blockstates (fences, walls, panes) show arms.
            // W133: family-aware (MC fence/wall/pane rules) instead
            // of strict same-id, matching the worker path.
            let same_id = |dx: i32, dy: i32, dz: i32| -> bool {
                let nx = lx + dx;
                let ny = ly + dy;
                let nz = lz + dz;
                if (0..CHUNK_SIZE as i32).contains(&nx)
                    && (0..CHUNK_SIZE as i32).contains(&ny)
                    && (0..CHUNK_SIZE as i32).contains(&nz)
                {
                    let nb =
                        chunk_ref.get(nx as usize, ny as usize, nz as usize);
                    crate::block::multipart_connects(block.0, nb)
                } else {
                    false
                }
            };
            let neighbors = resource_pack::element_mesh::NeighborStates {
                // MC north = -Z, south = +Z, east = +X, west = -X.
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
                    // Air-side cell relative to the voxel.
                    let air = [
                        world_origin[0] + lx + delta[0] as i32,
                        world_origin[1] + ly + delta[1] as i32,
                        world_origin[2] + lz + delta[2] as i32,
                    ];
                    light_lookup(air)
                },
                |_tex: &str| -> u32 {
                    // Static anim id for element faces (anim is
                    // rare for special-model blocks).
                    0
                },
                |delta: [i8; 3]| -> bool {
                    let nx = lx + delta[0] as i32;
                    let ny = ly + delta[1] as i32;
                    let nz = lz + delta[2] as i32;
                    if (0..CHUNK_SIZE as i32).contains(&nx)
                        && (0..CHUNK_SIZE as i32).contains(&ny)
                        && (0..CHUNK_SIZE as i32).contains(&nz)
                    {
                        let nb = chunk_ref.get(nx as usize, ny as usize, nz as usize);
                        // W44.5: air (id=0) has no BlockType entry so
                        // block_is_opaque(AIR) returned true via
                        // unwrap_or(true). Check is_air() first so that
                        // faces toward air are never culled.
                        !nb.is_air() && block_is_opaque(nb)
                    } else {
                        false
                    }
                },
            )
        };
        // W122: AO occluder query (matches the worker path so sync- and
        // worker-built chunks are pixel-identical). In-range cells read
        // the local chunk; out-of-range neighbours return `false`.
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
        // W34.2: switch to greedy mesher with per-face UV tables.
        // The per-face quad_uv derivation was completed in W34.2 -
        // each of the 6 face directions now has an explicit
        // quad_uv table that matches the OLD per-voxel UV assignment.
        chunk_mesh_greedy_with_special(
            chunk,
            origin,
            uv_resolver,
            // W109: block_fully_occludes (not block_is_opaque) so the
            // cactus's inset model doesn't cull the sand top under it.
            crate::block::block_fully_occludes,
            |air_local, _normal| light_lookup(air_local),
            anim_lookup,
            occluder_at,
            emit_special,
        )
    }

    /// Phase L.7 / W27.4: derive both light layers from scratch over
    /// every loaded chunk. Used after `load_world` populates the
    /// block grid + after `switch_dim` (when the active dim's
    /// chunks completely change). Lighting is never persisted on
    /// disk - same convention as MC, which re-derives sky-light
    /// on chunk load via `LayerLightEngine` + block-light from
    /// emitter blocks.
    ///
    /// W27.4: When a worker pool is available, this dispatches ONE
    /// big `Job::Lighting` (owned snapshot of ALL loaded chunks) to
    /// the pool instead of running the BFS on the main thread. The
    /// 2-minute main-thread freeze for 1300+ chunks is replaced by a
    /// sub-second worker BFS. `pending_lighting_jobs` is incremented
    /// so the loading phase waits for `WorkerResult::Lit` before
    /// transitioning to `BuildingMeshes`.
    ///
    /// Without a worker pool (headless tests): runs synchronously as
    /// before.
    pub(crate) fn rebuild_all_lighting(&mut self) {
        // W46.32 / W46.34: cap the initial-load BFS to the N closest
        // chunks to the player. The W46.32 radius filter
        // (16xz x 8y) returned the full 1587-chunk set when the
        // save's chunks were all dense around the player and didn't
        // help. Sky-light BFS is O(columns * y_height); even on a
        // 24-worker pool a 1587-chunk batch is a single serial Job
        // taking 17 s.
        //
        // Sort by L1 chunk distance + truncate. Far chunks get their
        // sky-light derived lazily via tick_lighting + per-chunk
        // dispatch when the streamer adds them later.
        const LIGHTING_BFS_CAP: usize = 500;
        let cs = infengine_voxel::CHUNK_SIZE as f32;
        let pcx = (self.player_pos.x / cs).floor() as i32;
        let pcy = (self.player_pos.y / cs).floor() as i32;
        let pcz = (self.player_pos.z / cs).floor() as i32;
        let mut all: Vec<(i32, ChunkCoord)> = self
            .world
            .iter_chunks()
            .map(|(c, _)| {
                let d = (c[0] - pcx).abs()
                    + (c[1] - pcy).abs() * 2 // weight Y so vertical neighbours win over far horiz
                    + (c[2] - pcz).abs();
                (d, *c)
            })
            .collect();
        all.sort_by_key(|(d, _)| *d);
        let coords: Vec<ChunkCoord> = all
            .into_iter()
            .take(LIGHTING_BFS_CAP)
            .map(|(_, c)| c)
            .collect();
        if coords.is_empty() {
            return;
        }
        // Clear the dirty set: the full re-derive subsumes every
        // pending per-voxel mark so we don't re-flood on next tick.
        let _ = self.lighting.take_dirty();

        if self.workers.is_some() {
            // W207: split into per-region segments so the initial-load
            // BFS parallelises across the whole worker pool instead of
            // one worker grinding a 500-chunk job while the rest idle.
            self.dispatch_lighting_segments(coords);
            return;
        }

        // Fallback: synchronous BFS on main thread (tests / no workers).
        lighting::block_engine::propagate_block_light(
            &self.world,
            &mut self.lighting,
            &coords,
        );
        lighting::sky_engine::propagate_sky_light(
            &self.world,
            &mut self.lighting,
            &coords,
        );
    }

    /// W207: split an affected-chunk batch into per-region segments and
    /// dispatch one `Job::Lighting` per segment, so the BFS load spreads
    /// across the whole worker pool. Previously ONE job carried the
    /// entire batch (100-500 chunks): one worker ground it for
    /// 200-600 ms while the other 23 idled, and the generator outran
    /// the lighting frontier at high view distance.
    ///
    /// Segments are 4x4-chunk-column XZ buckets (full vertical range),
    /// each shipped with its own face-neighbour snapshot halo. Segment
    /// boundaries have the same conservative semantics the old batch
    /// boundaries already had (a job only writes its own affected set;
    /// cross-boundary spill comes from the halo snapshot).
    fn dispatch_lighting_segments(&mut self, affected: Vec<ChunkCoord>) {
        // Bucket by XZ region. BTreeMap for a deterministic dispatch
        // order. Vertical splitting is NOT an option here - sky light
        // propagates down whole columns, so a column's chunks must
        // light together.
        //
        // W234: region 4x4 -> 2x2 chunk columns. W231 full columns
        // made a 4x4 bucket up to 256 chunks (16 tall); the user's
        // log showed 84-95-chunk jobs at sky_ms 300-550 each, which
        // parked the flood on a few workers and delivered giant Lit
        // batches (= big remesh waves). 2x2 caps a job at <=64
        // chunks, quarters its latency, and spreads the same work
        // across the 24-worker pool.
        let mut segments: std::collections::BTreeMap<(i32, i32), Vec<ChunkCoord>> =
            std::collections::BTreeMap::new();
        for cc in affected {
            segments
                .entry((cc[0].div_euclid(2), cc[2].div_euclid(2)))
                .or_default()
                .push(cc);
        }
        // W237: queue the segments and drain under a TIME budget.
        // Snapshotting a segment clones its chunks (+ face halo) on
        // the MAIN thread; dispatching every segment of a big dirty
        // batch in one call was the recurring 26-45 ms pre_render
        // spike while streaming. Whatever doesn't fit this frame's
        // budget stays queued for the next tick.
        //
        // W334: coalesce into any segment already queued for the same
        // column key instead of queueing a duplicate flood.
        for (key, seg) in segments {
            let queued = self.pending_lighting_chunks.contains_key(&key);
            let e = self.pending_lighting_chunks.entry(key).or_default();
            for cc in seg {
                if !e.contains(&cc) {
                    e.push(cc);
                }
            }
            if !queued {
                self.pending_lighting_segments.push_back(key);
            }
        }
        self.drain_lighting_segments();
    }

    /// W237: dispatch queued lighting segments to the worker pool,
    /// bounded by BOTH the in-flight job cap and a per-call time
    /// budget (the snapshot clone is the main-thread cost).
    pub(crate) fn drain_lighting_segments(&mut self) {
        use std::collections::HashMap as Map;
        const MAX_INFLIGHT_LIGHTING_JOBS: u32 = 24;
        let Some(pool) = self.workers.as_ref() else {
            return;
        };
        let budget = std::time::Duration::from_millis(4);
        let start = std::time::Instant::now();
        while let Some(key) = {
            if self.pending_lighting_jobs >= MAX_INFLIGHT_LIGHTING_JOBS
                || start.elapsed() >= budget
            {
                None
            } else {
                self.pending_lighting_segments.pop_front()
            }
        } {
            let Some(seg) = self.pending_lighting_chunks.remove(&key)
            else {
                continue;
            };
            // Snapshot: segment chunks + face-neighbours (BFS frontier
            // correctness at chunk edges), cloned off the live world.
            let mut snap_entries: Map<ChunkCoord, Chunk> = Map::new();
            for cc in &seg {
                if let Some(c) = self.world.chunk(*cc) {
                    snap_entries.insert(*cc, c.clone());
                }
                for n in [
                    [cc[0] - 1, cc[1], cc[2]],
                    [cc[0] + 1, cc[1], cc[2]],
                    [cc[0], cc[1] - 1, cc[2]],
                    [cc[0], cc[1] + 1, cc[2]],
                    [cc[0], cc[1], cc[2] - 1],
                    [cc[0], cc[1], cc[2] + 1],
                ] {
                    if let Some(c) = self.world.chunk(n) {
                        snap_entries.entry(n).or_insert_with(|| c.clone());
                    }
                }
            }
            // Warm-start grids for the segment (cheap Arc clones).
            let existing_grids: Map<ChunkCoord, Arc<lighting::LightGrid>> = seg
                .iter()
                .filter_map(|cc| {
                    self.lighting.get(*cc).map(|g| (*cc, Arc::clone(g)))
                })
                .collect();
            let job = Job::Lighting(LightingJob {
                affected: seg,
                snapshot: lighting::ChunkSnapshot::from_iter(snap_entries),
                existing_grids,
                dim: self.current_dim.clone(),
            });
            // W304: the priority lane - see WorkerPool::light_tx.
            if pool.light_tx.send(job).is_ok() {
                self.pending_lighting_jobs += 1;
            }
        }
    }

    /// Drain the lighting dirty-set + dispatch a `Job::Lighting` to the
    /// worker pool. The worker runs the conservative re-flood of every
    /// dirty chunk + its 3x3x3 neighbour halo, then sends back
    /// `WorkerResult::Lit`; the main thread swaps in the new grids and
    /// enqueues the affected chunks for remesh on delivery.
    ///
    /// W27.3: previously this ran the BFS synchronously on the main
    /// thread and returned the affected chunk list. Now:
    /// - With a worker pool: dispatches `Job::Lighting`; returns empty
    ///   (remesh is triggered asynchronously by `WorkerResult::Lit`).
    /// - Without a worker pool (headless tests): falls back to the old
    ///   synchronous path and returns the affected list so the caller
    ///   can remesh immediately.
    pub(crate) fn tick_lighting(&mut self) -> Vec<ChunkCoord> {
        // W237: drain segments deferred by an earlier over-budget
        // dispatch even on ticks that add no new dirty voxels.
        self.drain_lighting_segments();
        // W94/W95: bound lighting work per tick on TWO axes so a fast,
        // sustained re-dirty source (chunk streaming while the player moves
        // through water-rich Natural terrain, where the BFS is slow and the
        // worker pool is shared with generate+remesh) can't tank the frame
        // rate or freeze the game:
        //
        // 1. In-flight cap: never have more than MAX_INFLIGHT lighting jobs
        //    outstanding, or the worker queue + result backlog grows without
        //    bound (the original 1-FPS freeze: pending_lighting_jobs 0->225).
        // 2. Per-dispatch cap: process at most MAX_DIRTY_PER_TICK dirty
        //    voxels per tick, re-marking the overflow. Without this, the
        //    dirty-set accumulated across in-flight-capped ticks and a
        //    single later dispatch cloned hundreds of chunks into the worker
        //    snapshot ON THE MAIN THREAD -> multi-100ms stalls -> ~10 FPS.
        //    Bounding the batch keeps every snapshot clone cheap.
        //
        // The dirty-set is a deduped HashSet, so retaining/re-marking across
        // ticks is bounded by the loaded world size (not the tick rate);
        // lighting just trails slightly under heavy load instead of running
        // away. Classic worldgen never hit any of this (no water -> fast BFS
        // -> the counter stayed at 0-1). The sync (no-worker, test) path is
        // untouched and still processes the whole set in one pass.
        // W207: raised 6 -> 24 and 8 -> 32. Jobs are now split into
        // per-region segments (dispatch_lighting_segments) and the BFS
        // block access is chunk-cached, so individual jobs are small and
        // parallel - the old caps starved the 24-worker pool and made
        // the lighting frontier trail the generator at high view
        // distance (the "slow chunk appearance").
        const MAX_INFLIGHT_LIGHTING_JOBS: u32 = 24;
        const MAX_DIRTY_PER_TICK: usize = 32;
        if self.workers.is_some()
            && self.pending_lighting_jobs >= MAX_INFLIGHT_LIGHTING_JOBS
        {
            // W284: self-heal a leaked in-flight counter. A real
            // lighting job finishes in well under a second, so
            // sitting AT the cap for thousands of consecutive frames
            // without a single Lit result means the decrements were
            // lost (e.g. results dropped by a buffer clear) - and a
            // wedged counter silently disables ALL lighting updates
            // for the rest of the session while the dirty set grows
            // without bound. Reset + warn; a rare false positive
            // just over-dispatches once (the arriving results
            // saturating_sub at zero).
            self.lighting_capped_frames += 1;
            if self.lighting_capped_frames > 3600 {
                tracing::warn!(
                    stuck = self.pending_lighting_jobs,
                    dirty = self.lighting.dirty.len(),
                    "W284: lighting in-flight counter wedged at cap - resetting"
                );
                self.pending_lighting_jobs = 0;
                self.lighting_capped_frames = 0;
            }
            return Vec::new();
        }
        self.lighting_capped_frames = 0;
        let mut dirty = self.lighting.take_dirty();
        // W28.5: also drain the per-change emission records.
        let dirty_changes = self.lighting.take_dirty_changes();
        if dirty.is_empty() {
            return Vec::new();
        }
        // Bound the batch for the worker-backed bulk path (gen-driven, no
        // emission changes). Edits carry `dirty_changes` and are few, so
        // they process whole to keep the incremental two-queue path exact.
        if self.workers.is_some()
            && dirty_changes.is_empty()
            && dirty.len() > MAX_DIRTY_PER_TICK
        {
            let mut iter = dirty.into_iter();
            let batch: HashSet<[i32; 3]> = iter.by_ref().take(MAX_DIRTY_PER_TICK).collect();
            for v in iter {
                self.lighting.mark_dirty(v);
            }
            dirty = batch;
        }
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let mut affected_set: HashSet<ChunkCoord> = HashSet::new();
        for v in &dirty {
            let cc = [
                v[0].div_euclid(cs),
                v[1].div_euclid(cs),
                v[2].div_euclid(cs),
            ];
            // 3x3x3 chunk halo around each dirty voxel. The bigger
            // halo lets a torch near a chunk boundary bleed light
            // into adjacent chunks (L.6 conservative strategy).
            for dx in -1..=1i32 {
                for dy in -1..=1i32 {
                    for dz in -1..=1i32 {
                        affected_set.insert([cc[0] + dx, cc[1] + dy, cc[2] + dz]);
                    }
                }
            }
        }
        // Keep only chunks that actually exist in the world.
        let affected: Vec<ChunkCoord> = affected_set
            .into_iter()
            .filter(|cc| self.world.chunk(*cc).is_some())
            .collect();
        if affected.is_empty() {
            return Vec::new();
        }

        // W27.3: if a worker pool is available, dispatch the BFS
        // off the main thread. The result arrives as
        // WorkerResult::Lit a few frames later.
        if self.workers.is_some() {
            // W207: per-region segments -> parallel workers.
            self.dispatch_lighting_segments(affected);
            // Remesh will be triggered by WorkerResult::Lit delivery.
            return Vec::new();
        }

        // Fallback (no worker pool): synchronous BFS on main thread.
        // Used by headless tests where WorkerPool is never spawned.
        //
        // (see dispatch_lighting_segments below for the worker path)
        //
        // W28.5 + W34.3: use two-queue incremental update when every
        // dirty voxel has a recorded emission change. W34.3 extends
        // this to also cover block-light blocker changes (opaque
        // block place/remove): propagate_block_light_incremental now
        // handles the unlight phase for added blockers.
        //
        // Sky-light still requires the full propagate_sky_light engine
        // when an opaque block changes (sky columns are not amenable to
        // simple two-queue BFS at chunk boundaries). Block-light-only
        // changes (emitter add/remove with no opaciy change) stay on
        // the fast incremental path.
        let all_changes_recorded = dirty_changes.len() == dirty.len() && dirty.len() == 1;
        let has_blocker_change = dirty_changes.iter().any(|(pos, old_e, new_e)| {
            let is_opaque_now = !self.world.get(*pos).is_air()
                && block_is_opaque(self.world.get(*pos));
            // A change is a blocker change if emission is 0 on both sides
            // and either the block is now opaque (placed solid) or
            // the block was an emitter that also happened to be opaque.
            old_e == &0 && new_e == &0 || is_opaque_now
        });

        if all_changes_recorded {
            for (pos, old_e, new_e) in &dirty_changes {
                lighting::block_engine::propagate_block_light_incremental(
                    &self.world,
                    &mut self.lighting,
                    &affected,
                    *pos,
                    *old_e,
                    *new_e,
                );
            }
            // Sky light needs the full engine if opacity changed.
            if has_blocker_change {
                lighting::sky_engine::propagate_sky_light(
                    &self.world,
                    &mut self.lighting,
                    &affected,
                );
            }
        } else {
            lighting::block_engine::propagate_block_light(
                &self.world,
                &mut self.lighting,
                &affected,
            );
            lighting::sky_engine::propagate_sky_light(
                &self.world,
                &mut self.lighting,
                &affected,
            );
        }
        affected
    }
}

#[cfg(test)]
mod tests {
    /// W334: repeat marks of the same 2x2 chunk column must merge
    /// into ONE queued segment (union of chunks, no duplicates).
    /// Streaming delivers a column across many ticks; before the
    /// coalescing every delivery queued a fresh flood of the same
    /// area and the lighting lane sat pinned at the in-flight cap.
    #[test]
    fn w334_lighting_segments_coalesce_per_column() {
        let mut v = crate::Voxel::new();
        v.dispatch_lighting_segments(vec![[0, 0, 0], [1, 0, 0]]);
        v.dispatch_lighting_segments(vec![[0, 1, 0], [0, 0, 0]]);
        assert_eq!(v.pending_lighting_segments.len(), 1);
        let seg = v.pending_lighting_chunks.get(&(0, 0)).unwrap();
        assert_eq!(seg.len(), 3, "union without duplicates: {seg:?}");
        // A different column queues separately.
        v.dispatch_lighting_segments(vec![[2, 0, 0]]);
        assert_eq!(v.pending_lighting_segments.len(), 2);
    }
}
