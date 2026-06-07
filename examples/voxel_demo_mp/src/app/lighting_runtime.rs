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
        // W62.2: door open-state lookup. Cloned into the closure
        // so the meshing path can render open doors as invisible
        // (matches the "door swung aside" MC behavior; full
        // hinge rotation lands in W62.2b).
        let door_open_snapshot: std::collections::HashSet<[i32; 3]> =
            self.door_open.clone();
        let emit_special = move |block: BlockId,
                                 voxel_origin: glam::Vec3,
                                 verts: &mut Vec<MeshVertex>,
                                 idxs: &mut Vec<u32>| {
            // W62.2: door at open state -> emit nothing but
            // return `true` so the greedy cube path skips this
            // cell (door "disappears" visually).
            if crate::block::door::is_door_block(block.0) {
                let wp = [
                    voxel_origin.x.floor() as i32,
                    voxel_origin.y.floor() as i32,
                    voxel_origin.z.floor() as i32,
                ];
                // Lower-half resolution: the upper half also
                // disappears when its sibling lower is open.
                let lower = if (1..=78).contains(&block.0)
                    && door_open_snapshot.contains(&[wp[0], wp[1] - 1, wp[2]])
                {
                    [wp[0], wp[1] - 1, wp[2]]
                } else {
                    wp
                };
                if door_open_snapshot.contains(&lower) {
                    return true;
                }
            }
            // Compute voxel local coords from world position.
            let lx = (voxel_origin.x - voxel_origin_ref.x) as i32;
            let ly = (voxel_origin.y - voxel_origin_ref.y) as i32;
            let lz = (voxel_origin.z - voxel_origin_ref.z) as i32;
            // W40.1: compute horizontal neighbour connection flags so
            // multipart blockstates (fences, walls, panes) show arms.
            // A neighbour "connects" when it carries the same block id.
            let same_id = |dx: i32, dy: i32, dz: i32| -> bool {
                let nx = lx + dx;
                let ny = ly + dy;
                let nz = lz + dz;
                if (0..CHUNK_SIZE as i32).contains(&nx)
                    && (0..CHUNK_SIZE as i32).contains(&ny)
                    && (0..CHUNK_SIZE as i32).contains(&nz)
                {
                    chunk_ref.get(nx as usize, ny as usize, nz as usize).0 == block.0
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

        if let Some(pool) = self.workers.as_ref() {
            // W27.4: build the owned snapshot for the worker.
            // Clone every loaded chunk (~8 KB each). For 1296 chunks
            // that is ~10 MB - a one-time allocation acceptable at
            // load time.
            let snap_entries: HashMap<ChunkCoord, Chunk> = coords
                .iter()
                .filter_map(|cc| self.world.chunk(*cc).map(|c| (*cc, c.clone())))
                .collect();
            // Warm-start from any existing grids (usually empty on
            // first load; populated on dim switch).
            let existing_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>> = coords
                .iter()
                .filter_map(|cc| {
                    self.lighting.get(*cc).map(|g| (*cc, Arc::clone(g)))
                })
                .collect();
            let job = Job::Lighting(LightingJob {
                affected: coords,
                snapshot: lighting::ChunkSnapshot::from_iter(snap_entries),
                existing_grids,
            });
            let _ = pool.req_tx.send(job);
            self.pending_lighting_jobs += 1;
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
        const MAX_INFLIGHT_LIGHTING_JOBS: u32 = 6;
        const MAX_DIRTY_PER_TICK: usize = 8;
        if self.workers.is_some()
            && self.pending_lighting_jobs >= MAX_INFLIGHT_LIGHTING_JOBS
        {
            return Vec::new();
        }
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
        if let Some(pool) = self.workers.as_ref() {
            // Build the voxel snapshot: affected chunks + their 6
            // face-neighbours so the BFS frontier is correct at
            // chunk boundaries. ~27 chunks x ~8 KB = ~216 KB.
            let mut snap_entries: HashMap<ChunkCoord, Chunk> = HashMap::new();
            for cc in &affected {
                if let Some(c) = self.world.chunk(*cc) {
                    snap_entries.insert(*cc, c.clone());
                }
                // Include face-neighbours so the BFS doesn't lose
                // data at chunk edges.
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
            // Warm-start grids for affected chunks (cheap Arc clone).
            let existing_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>> = affected
                .iter()
                .filter_map(|cc| {
                    self.lighting.get(*cc).map(|g| (*cc, Arc::clone(g)))
                })
                .collect();
            let job = Job::Lighting(LightingJob {
                affected,
                snapshot: lighting::ChunkSnapshot::from_iter(snap_entries),
                existing_grids,
            });
            let _ = pool.req_tx.send(job);
            // Track the in-flight job so the loading phase can wait.
            self.pending_lighting_jobs += 1;
            // Remesh will be triggered by WorkerResult::Lit delivery.
            return Vec::new();
        }

        // Fallback (no worker pool): synchronous BFS on main thread.
        // Used by headless tests where WorkerPool is never spawned.
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
