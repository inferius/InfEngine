//! W47.28 - chunk_rebuild impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {

    /// W125: bake the door state (facing + open per LOWER half) for
    /// doors whose lower OR upper half voxel lands in `coord`. The
    /// worker meshes from snapshots and can't read `door_open` /
    /// `door_facing` live - without this the remesh always drew
    /// doors closed (the "door never visually opens" bug). Union
    /// over both maps so a toggled door without a facing row
    /// (pre-W125 save) still ships its open flag.
    pub(crate) fn door_state_snapshot(
        &self,
        coord: ChunkCoord,
    ) -> std::collections::HashMap<[i32; 3], (u8, bool)> {
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let in_chunk = |p: [i32; 3]| -> bool {
            [p[0].div_euclid(cs), p[1].div_euclid(cs), p[2].div_euclid(cs)]
                == coord
        };
        let mut out = std::collections::HashMap::new();
        for (&pos, &facing) in &self.door_facing {
            if in_chunk(pos) || in_chunk([pos[0], pos[1] + 1, pos[2]]) {
                out.insert(pos, (facing, self.door_open.contains(&pos)));
            }
        }
        for &pos in &self.door_open {
            if !out.contains_key(&pos)
                && (in_chunk(pos) || in_chunk([pos[0], pos[1] + 1, pos[2]]))
            {
                out.insert(pos, (0, true));
            }
        }
        out
    }

    /// W127: derive the rail shapes for every rail in `coord`.
    /// Pure function of the World (cross-chunk neighbours seen),
    /// computed on the main thread at dispatch time - rails carry
    /// no stored state anywhere.
    pub(crate) fn rail_state_snapshot(
        &self,
        coord: ChunkCoord,
    ) -> std::collections::HashMap<[i32; 3], u8> {
        let mut out = std::collections::HashMap::new();
        let Some(chunk) = self.world.chunk(coord) else {
            return out;
        };
        let cs = infengine_voxel::CHUNK_SIZE;
        let origin = [
            coord[0] * cs as i32,
            coord[1] * cs as i32,
            coord[2] * cs as i32,
        ];
        let rail_at = |p: [i32; 3]| -> bool {
            crate::block::rail::is_rail_block(self.world.get(p).0)
        };
        for z in 0..cs {
            for y in 0..cs {
                for x in 0..cs {
                    let id = chunk.get(x, y, z).0;
                    if !crate::block::rail::is_rail_block(id) {
                        continue;
                    }
                    let wp = [
                        origin[0] + x as i32,
                        origin[1] + y as i32,
                        origin[2] + z as i32,
                    ];
                    // W130: bit 7 = redstone powered (torch/lever
                    // adjacency + the 8-rail MC chain).
                    let mut shape =
                        crate::block::rail::compute_shape(id, wp, rail_at);
                    if crate::block::rail::is_powerable(id)
                        && self.rail_is_powered(wp, id)
                    {
                        shape |= 0x80;
                    }
                    out.insert(wp, shape);
                }
            }
        }
        out
    }

    /// W130: is the powerable rail at `wp` redstone-powered? Direct
    /// power = a redstone torch in the 6-neighbourhood or an ON
    /// lever adjacent. Indirect = a connected same-kind rail chain
    /// up to 8 hops away with direct power (MC powered-rail chain).
    pub(crate) fn rail_is_powered(&self, wp: [i32; 3], id: u16) -> bool {
        let direct = |p: [i32; 3]| -> bool {
            let neigh = [
                [p[0] + 1, p[1], p[2]],
                [p[0] - 1, p[1], p[2]],
                [p[0], p[1] + 1, p[2]],
                [p[0], p[1] - 1, p[2]],
                [p[0], p[1], p[2] + 1],
                [p[0], p[1], p[2] - 1],
            ];
            neigh.iter().any(|n| {
                let b = self.world.get(*n).0;
                b == crate::block::REDSTONE_TORCH_ID
                    || b == crate::block::REDSTONE_WALL_TORCH_ID
                    || (b == crate::block::LEVER_ID
                        && self.lever_on.contains(n))
            })
        };
        if direct(wp) {
            return true;
        }
        // BFS along connected same-kind rails (horizontal steps,
        // one level up/down), bounded at 8 hops like MC.
        let mut seen = std::collections::HashSet::new();
        let mut frontier = vec![wp];
        seen.insert(wp);
        for _ in 0..8 {
            let mut next = Vec::new();
            for p in frontier {
                for d in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]] {
                    for dy in [0i32, 1, -1] {
                        let n = [p[0] + d[0], p[1] + dy, p[2] + d[2]];
                        if seen.contains(&n) {
                            continue;
                        }
                        if self.world.get(n).0 != id {
                            continue;
                        }
                        if direct(n) {
                            return true;
                        }
                        seen.insert(n);
                        next.push(n);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        false
    }

    /// W25: Dispatch remesh jobs for `coords` to the worker pool.
    /// For each coord a `Job::Remesh` is enqueued carrying a voxel
    /// snapshot + halo lighting. The actual mesh build runs off the
    /// main thread; the result arrives as `WorkerResult::Remeshed` and
    /// is GPU-uploaded in the next `stream_chunks` delivery pass.
    ///
    /// Falls back to `rebuild_chunks_sync` when no worker pool is
    /// present (headless tests, early init) so all call sites remain
    /// identical regardless of runtime mode.
    pub(crate) fn rebuild_chunks(&mut self, coords: &[ChunkCoord]) {
        if self.workers.is_none() {
            return self.rebuild_chunks_sync(coords);
        }
        let pack_arc = Arc::clone(&self.resource_pack);
        // W32 NOTE: proactive mesh-removal-on-dispatch was tried but caused
        // visible whole-chunk flicker on every edit (entire chunk disappeared
        // for the worker round-trip, then re-appeared). Reverted - the
        // WorkerResult::Remeshed handler removes + uploads atomically when
        // the result arrives. If voxels "stick" visually after an edit, the
        // bug is in result delivery (worker panic, channel disconnect, or
        // revision-guard mis-drop), NOT in dispatch timing.
        for &coord in coords {
            let Some(chunk) = self.world.chunk(coord) else {
                continue;
            };
            // Snapshot voxels. Chunk is ~8 KB; clone is fast.
            let chunk_snap = chunk.clone();

            // Snapshot halo lighting (same pre-fetch used for Generate
            // jobs in the dispatch path above).
            let halo = HaloLighting {
                own: self.lighting.get(coord).cloned(),
                px: self.lighting.get([coord[0] + 1, coord[1], coord[2]]).cloned(),
                nx: self.lighting.get([coord[0] - 1, coord[1], coord[2]]).cloned(),
                pz: self.lighting.get([coord[0], coord[1], coord[2] + 1]).cloned(),
                nz: self.lighting.get([coord[0], coord[1], coord[2] - 1]).cloned(),
                py: self.lighting.get([coord[0], coord[1] + 1, coord[2]]).cloned(),
                ny: self.lighting.get([coord[0], coord[1] - 1, coord[2]]).cloned(),
            };

            // Bump revision so any in-flight result for this coord that
            // was snapshotted before this edit is discarded on delivery.
            let rev = self.chunk_revisions.entry(coord).or_insert(0);
            *rev += 1;
            let revision = *rev;

            let job = Job::Remesh(RemeshJob {
                coord,
                chunk: chunk_snap,
                pack: Arc::clone(&pack_arc),
                halo,
                revision,
                doors: self.door_state_snapshot(coord),
                rails: self.rail_state_snapshot(coord),
            });
            // If the send fails the pool is shutting down; ignore.
            if let Some(pool) = self.workers.as_ref() {
                let _ = pool.req_tx.send(job);
            }
        }
    }

    /// Synchronous fallback for `rebuild_chunks`. Used when no worker
    /// pool is present (headless tests, pre-init). Builds submeshes on
    /// the main thread and uploads them immediately.
    pub(crate) fn rebuild_chunks_sync(&mut self, coords: &[ChunkCoord]) {
        // Release old mesh handles for the affected chunks so the
        // mesh registry doesn't accumulate stale buffers across edits.
        // W26.4: HashMap remove is O(1) per coord.
        {
            let renderer = self.renderer.as_mut().expect("renderer ready");
            for coord in coords {
                if let Some(old_subs) = self.chunk_meshes.remove(coord) {
                    for (_block, handle) in old_subs {
                        renderer.remove_chunk_mesh(handle);
                    }
                }
            }
        }
        let atlas = self.voxel_atlas.clone();
        for coord in coords {
            let subs = self.build_lit_submeshes(*coord);
            if subs.is_empty() {
                continue;
            }
            let renderer = self.renderer.as_mut().expect("renderer ready");
            for sub in subs {
                if sub.vertices.is_empty() || sub.indices.is_empty() {
                    continue;
                }
                // Skip torch + snow submeshes: both render through
                // dedicated world-pass meshes (thin torch + thin snow
                // slab) instead of as full cube voxels.
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
                self.chunk_meshes.entry(*coord).or_default().push((sub.block, handle));
            }
        }
    }

    /// Walk every chunk in the world and register its mesh handles.
    /// Used at startup. Edits use `rebuild_chunks` for the affected
    /// chunk + its solid neighbours so the cost stays local.
    pub(crate) fn rebuild_all_meshes(&mut self) {
        self.chunk_meshes.clear();
        let atlas = self.voxel_atlas.clone();
        let coords: Vec<ChunkCoord> = self.world.iter_chunks().map(|(c, _)| *c).collect();
        for coord in coords {
            let subs = self.build_lit_submeshes(coord);
            if subs.is_empty() {
                continue;
            }
            let renderer = self.renderer.as_mut().expect("renderer ready");
            for sub in subs {
                if sub.vertices.is_empty() || sub.indices.is_empty() {
                    continue;
                }
                // Skip torch + snow submeshes: both render
                // through dedicated world-pass meshes (thin
                // torch + thin snow slab) instead of as full
                // cube voxels.
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
                self.chunk_meshes.entry(coord).or_default().push((sub.block, handle));
            }
        }
    }
}
