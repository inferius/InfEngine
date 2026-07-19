//! W47.28 - liquid_tick impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// Height of a single corner of voxel (vx,vy,vz). `cx`/`cz`
    /// in `{0,1}` pick which corner. Two-phase computation:
    ///
    ///   Phase 1: if any of the four touching cells is a falling
    ///   column (same liquid directly above), the corner snaps
    ///   to 1.0 - submerged / column voxels render as full
    ///   blocks, no slope.
    ///
    ///   Phase 2 (top-layer surface): average the surface
    ///   heights of touching cells, counting AIR as 0 height
    ///   and SKIPPING solid blocks entirely. This gives the
    ///   top of a pool a sloped lip toward the bank instead of
    ///   the boxy vanilla MC look. Solid neighbours are
    ///   ignored so a pond against a stone wall keeps the
    ///   wall-side corner at the source height.
    /// Horizontal flow direction at a liquid voxel, returned as
    /// a unit-ish 2D vector in (x, z). Computed by summing the
    /// "flow toward lower level" contribution of each NSEW
    /// neighbour - the cell pushes downhill toward lower-level
    /// flow cells, off cliffs (cell with air below), and out of
    /// itself. Source cells with no lower-level neighbour return
    /// the zero vector (no current).
    pub(crate) fn liquid_flow_dir(&self, pos: [i32; 3], liquid_id: u16) -> [f32; 2] {
        let level = self.liquid_level.get(&pos).copied().unwrap_or(0);
        let mut dx = 0.0_f32;
        let mut dz = 0.0_f32;
        let dirs: [([i32; 3], [f32; 2]); 4] = [
            ([1, 0, 0], [1.0, 0.0]),
            ([-1, 0, 0], [-1.0, 0.0]),
            ([0, 0, 1], [0.0, 1.0]),
            ([0, 0, -1], [0.0, -1.0]),
        ];
        for (d, axis) in &dirs {
            let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
            let nb = self.world.get(n);
            if nb.0 == liquid_id {
                let nl = self.liquid_level.get(&n).copied().unwrap_or(0);
                if nl > level {
                    // Neighbour lower in the surface ramp -> our
                    // flow vectors toward it.
                    let weight = (nl - level) as f32;
                    dx += axis[0] * weight;
                    dz += axis[1] * weight;
                }
            } else if nb.is_air() {
                // Air neighbour with air below = cliff edge.
                // Flow points outward.
                let below_n = [n[0], n[1] - 1, n[2]];
                if self.world.get(below_n).is_air() {
                    dx += axis[0];
                    dz += axis[1];
                }
            }
        }
        let mag = (dx * dx + dz * dz).sqrt();
        if mag < 1e-3 {
            [0.0, 0.0]
        } else {
            [dx / mag, dz / mag]
        }
    }

    pub(crate) fn liquid_corner_height(
        &self,
        vx: i32,
        vy: i32,
        vz: i32,
        cx: i32,
        cz: i32,
        liquid_id: u16,
    ) -> f32 {
        liquid_corner_height_at(
            &self.world,
            &self.liquid_level,
            vx,
            vy,
            vz,
            cx,
            cz,
            liquid_id,
        )
    }

    /// Rebuild the combined water mesh when the dirty flag is
    /// set. One handle covers every visible water voxel: top
    /// face uses per-corner heights computed from the depth map,
    /// side faces drop to y=0 only against air / decorations, and
    /// the bottom face emits only when there's something the
    /// player could actually see through the water from below.
    /// W111: emit the slope-aware quads for ONE liquid cell at `p` into the
    /// per-kind vertex/index bucket. Shared by the per-chunk rebuild below;
    /// extracted verbatim from the old whole-world loop.
    fn emit_liquid_cell(
        &self,
        p: [i32; 3],
        buckets: &mut HashMap<u16, (Vec<MeshVertex>, Vec<u32>)>,
    ) {
        let block = self.world.get(p);
        // W203: waterlogged plant cells ARE water for the liquid mesh -
        // they emit water faces exactly like a water cell (so the sea
        // surface stays continuous over a kelp tip and the body has no
        // holes), while the plant cross renders via the chunk mesh.
        let liquid_id = if crate::block::block_is_waterlogged(block.0) {
            crate::block::WATER_ID
        } else {
            block.0
        };
        if liquid_for(BlockId(liquid_id)).is_none() {
            return;
        }
        let (vertices, indices) = buckets.entry(liquid_id).or_default();
        let vx = p[0];
        let vy = p[1];
        let vz = p[2];
        let h00 = self.liquid_corner_height(vx, vy, vz, 0, 0, liquid_id);
        let h10 = self.liquid_corner_height(vx, vy, vz, 1, 0, liquid_id);
        let h11 = self.liquid_corner_height(vx, vy, vz, 1, 1, liquid_id);
        let h01 = self.liquid_corner_height(vx, vy, vz, 0, 1, liquid_id);
        let fx = vx as f32;
        let fy = vy as f32;
        let fz = vz as f32;
        // W200: resolve the MC animated liquid texture (water_still /
        // lava_still) once per cell - frame-0 atlas rect + tile-anim slot.
        // The quads sample it exactly like animated chunk-mesh tiles, so
        // liquids finally LOOK like MC liquids instead of a flat tinted
        // plane with a procedural shimmer. Falls back to the pack's
        // missing-texture sentinel when the pack has no entry (pack-less
        // dev runs), same policy as the chunk mesher.
        let (tile, anim) = {
            let pack = &self.resource_pack;
            match pack.block_face_ref(liquid_id, crate::resource_pack::BlockFace::Top) {
                Some(r) => {
                    (r.atlas_uv, pack.anim_slot_for_ref(r) as u32)
                }
                None => (
                    pack.icon_uv_for(block, [0, 1, 0]),
                    0,
                ),
            }
        };
        // W199: water-column depth below this cell, normalized for the
        // shader's depth-based surface opacity (deep ocean reads opaque so
        // it isn't see-through to the void; shallow ponds stay translucent).
        // Walk down counting same-liquid cells, capped at DEPTH_FULL blocks.
        let depth = {
            const DEPTH_FULL: i32 = 12;
            let mut d = 1i32;
            // W203: the walk continues through waterlogged plant cells -
            // stopping at a kelp stalk made the water above kelp forests
            // read "shallow" = translucent while neighbouring columns
            // were opaque (the patchy surface from above).
            while d <= DEPTH_FULL
                && crate::liquid::counts_as_liquid(
                    self.world.get([vx, vy - d, vz]).0,
                    liquid_id,
                )
            {
                d += 1;
            }
            d as f32 / DEPTH_FULL as f32
        };
        // Top face (CCW from above so the Back-cull keeps it) - only when
        // the cell above isn't the same liquid (waterlogged counts).
        // W276: ALSO cull against a full opaque block above (MC's
        // shouldRenderFace(UP)). Without it every underwater cell under
        // the seabed/beach emitted a phantom surface quad ~0.11 under
        // the solid's bottom face - visible as floating water sheets
        // from below, z-fight shimmer while swimming (the user's
        // screenshot 1).
        let above = self.world.get([vx, vy + 1, vz]);
        let above_same = crate::liquid::counts_as_liquid(above.0, liquid_id);
        let above_occludes = !above.is_air() && crate::block::block_is_opaque(above);
        if !above_same && !above_occludes {
            let t0 = [fx, fy + h00, fz];
            let t1 = [fx, fy + h01, fz + 1.0];
            let t2 = [fx + 1.0, fy + h11, fz + 1.0];
            let t3 = [fx + 1.0, fy + h10, fz];
            let normal = water_face_normal(t0, t1, t2, t3);
            push_quad_flow(vertices, indices, t0, t1, t2, t3, normal, tile, anim, depth);
        }
        // Side faces: only when the in-plane neighbour is open (air /
        // passable). Same-liquid cancels (shared face is interior).
        if side_open(&self.world, vx - 1, vy, vz, liquid_id) {
            push_quad_flow(
                vertices,
                indices,
                [fx, fy, fz + 1.0],
                [fx, fy + h01, fz + 1.0],
                [fx, fy + h00, fz],
                [fx, fy, fz],
                [-1.0, 0.0, 0.0],
                tile,
                anim,
                depth,
            );
        }
        if side_open(&self.world, vx + 1, vy, vz, liquid_id) {
            push_quad_flow(
                vertices,
                indices,
                [fx + 1.0, fy, fz],
                [fx + 1.0, fy + h10, fz],
                [fx + 1.0, fy + h11, fz + 1.0],
                [fx + 1.0, fy, fz + 1.0],
                [1.0, 0.0, 0.0],
                tile,
                anim,
                depth,
            );
        }
        if side_open(&self.world, vx, vy, vz - 1, liquid_id) {
            push_quad_flow(
                vertices,
                indices,
                [fx, fy, fz],
                [fx, fy + h00, fz],
                [fx + 1.0, fy + h10, fz],
                [fx + 1.0, fy, fz],
                [0.0, 0.0, -1.0],
                tile,
                anim,
                depth,
            );
        }
        if side_open(&self.world, vx, vy, vz + 1, liquid_id) {
            push_quad_flow(
                vertices,
                indices,
                [fx + 1.0, fy, fz + 1.0],
                [fx + 1.0, fy + h11, fz + 1.0],
                [fx, fy + h01, fz + 1.0],
                [fx, fy, fz + 1.0],
                [0.0, 0.0, 1.0],
                tile,
                anim,
                depth,
            );
        }
        // Bottom face: only when the cell below is open (air / passable) -
        // MC's shouldRenderFace(DOWN) gate, so a cliff-edge water cell
        // shows its underside rather than sky.
        if bottom_open(&self.world, vx, vy - 1, vz, liquid_id) {
            push_quad_flow(
                vertices,
                indices,
                [fx, fy, fz],
                [fx + 1.0, fy, fz],
                [fx + 1.0, fy, fz + 1.0],
                [fx, fy, fz + 1.0],
                [0.0, -1.0, 0.0],
                tile,
                anim,
                depth,
            );
        }
    }

    /// W111: PER-CHUNK liquid mesh - replaces the whole-world rebuild and
    /// the W107 throttle workaround. Each rebuild folds an order-independent
    /// per-chunk signature of the liquid cells (position + level) and
    /// re-meshes ONLY the chunks whose signature changed. So an oscillating
    /// edge cell near a big ocean re-meshes just its own 16^3 chunk
    /// (cheap), never the whole 30k+ cell world mesh every tick - no judder,
    /// no throttle, and it scales to oceans. GPU work is budgeted to a few
    /// chunks per frame so a fresh chunk-load wave can't spike a frame.
    /// W198.12: queue a chunk + its 6 face neighbours for an INCREMENTAL
    /// liquid remesh (neighbours re-cull their shared faces). The liquid
    /// tick / merge use this so a settled-ocean liquid change re-meshes only
    /// the touched chunks, instead of setting `liquid_mesh_dirty` and
    /// tripping the global O(all-liquids) signature scan - that periodic
    /// scan (fired ~20Hz by the handful of oscillating shore cells) was the
    /// ~15ms spike that juddered the view while just turning in place.
    pub(crate) fn enqueue_liquid_remesh(&mut self, cc: ChunkCoord) {
        self.liquid_remesh_backlog.insert(cc);
        self.liquid_remesh_backlog.insert([cc[0] - 1, cc[1], cc[2]]);
        self.liquid_remesh_backlog.insert([cc[0] + 1, cc[1], cc[2]]);
        self.liquid_remesh_backlog.insert([cc[0], cc[1] - 1, cc[2]]);
        self.liquid_remesh_backlog.insert([cc[0], cc[1] + 1, cc[2]]);
        self.liquid_remesh_backlog.insert([cc[0], cc[1], cc[2] - 1]);
        self.liquid_remesh_backlog.insert([cc[0], cc[1], cc[2] + 1]);
    }

    pub(crate) fn maybe_rebuild_liquid_mesh(&mut self) {
        if self.renderer.is_none() {
            return;
        }
        let cs = CHUNK_SIZE as i32;
        // W195: the per-chunk signature scan over ALL liquid_positions is
        // O(total liquid) - ~16 ms for a big ocean (400k+ cells). The old
        // code re-ran it EVERY frame while the mesh budget was backlogged
        // (>24 changed chunks), pinning sim_ms at ~18 ms near water (the
        // user's "FPS zalostny u vody"). Now the scan runs ONCE per
        // dirty-set: it queues every changed chunk into the remesh backlog
        // and updates the stored signatures immediately, so the backlog
        // drains a few chunks per frame WITHOUT rescanning. A settled
        // ocean then costs nothing per frame.
        if self.liquid_mesh_dirty {
            // W295: this rescan walks EVERY tracked liquid cell
            // (800k+ on an explored ocean world) - name every run so
            // a stray dirty-flag setter can't hide behind the coarse
            // pre_render bucket again.
            let _scan_t = std::time::Instant::now();
            let chunk_of = |p: [i32; 3]| -> ChunkCoord {
                [p[0].div_euclid(cs), p[1].div_euclid(cs), p[2].div_euclid(cs)]
            };
            let mut cur_sig: HashMap<ChunkCoord, u64> = HashMap::new();
            // W198.10: rebuild the lava subset here - we already walk every
            // liquid on a dirty rescan, so it's free, and it stops
            // tick_lava_fire_emit from scanning all ~300k liquids every frame
            // (that O(all-liquids) world.get sweep was the ~20ms sim stall).
            // W267: keyed by chunk column so consumers only touch their
            // reach (the deep lava sea made the flat set ~1M cells).
            self.lava_columns.clear();
            for p in self.liquid_positions.iter().copied() {
                let blk = self.world.get(p);
                if liquid_for(blk).is_none() {
                    continue;
                }
                if blk.0 == crate::block::LAVA_ID {
                    self.lava_columns
                        .entry([p[0].div_euclid(cs), p[2].div_euclid(cs)])
                        .or_default()
                        .push(p);
                }
                let lvl = self.liquid_level.get(&p).copied().unwrap_or(0) as u64;
                let mut hsh = 0xcbf2_9ce4_8422_2325u64;
                for v in [p[0] as u64, p[1] as u64, p[2] as u64, lvl] {
                    hsh ^= v;
                    hsh = hsh.wrapping_mul(0x0000_0100_0000_01b3);
                }
                // W113: fold a boundary cell into the FACE-adjacent chunk too,
                // so that neighbour re-meshes (re-culls the shared face) when
                // this chunk's edge water changes - else a "wall" lingers at
                // the seam.
                let cc = chunk_of(p);
                let lx = p[0].rem_euclid(cs);
                let ly = p[1].rem_euclid(cs);
                let lz = p[2].rem_euclid(cs);
                let mut fold: [Option<ChunkCoord>; 7] = [Some(cc), None, None, None, None, None, None];
                if lx == 0 { fold[1] = Some([cc[0] - 1, cc[1], cc[2]]); }
                if lx == cs - 1 { fold[2] = Some([cc[0] + 1, cc[1], cc[2]]); }
                if ly == 0 { fold[3] = Some([cc[0], cc[1] - 1, cc[2]]); }
                if ly == cs - 1 { fold[4] = Some([cc[0], cc[1] + 1, cc[2]]); }
                if lz == 0 { fold[5] = Some([cc[0], cc[1], cc[2] - 1]); }
                if lz == cs - 1 { fold[6] = Some([cc[0], cc[1], cc[2] + 1]); }
                for c in fold.into_iter().flatten() {
                    let e = cur_sig.entry(c).or_insert(0u64);
                    *e = e.wrapping_add(hsh);
                }
            }
            // Queue every changed chunk + update its stored signature NOW, so
            // a later rescan won't re-queue it (the queue drains over frames).
            for (cc, sig) in &cur_sig {
                if self.liquid_chunk_sig.get(cc) != Some(sig) {
                    self.liquid_remesh_backlog.insert(*cc);
                    self.liquid_chunk_sig.insert(*cc, *sig);
                }
            }
            let gone: Vec<ChunkCoord> = self
                .liquid_chunk_sig
                .keys()
                .filter(|cc| !cur_sig.contains_key(*cc))
                .copied()
                .collect();
            for cc in gone {
                self.liquid_remesh_backlog.insert(cc);
                self.liquid_chunk_sig.remove(&cc);
            }
            self.liquid_mesh_dirty = false;
            let _ms = _scan_t.elapsed().as_secs_f32() * 1000.0;
            if _ms > 15.0 {
                tracing::warn!(
                    ms = _ms,
                    cells = self.liquid_positions.len(),
                    queued = self.liquid_remesh_backlog.len(),
                    "W295: liquid dirty rescan (O(all-liquids) fallback)"
                );
            }
        }

        if self.liquid_remesh_backlog.is_empty() {
            return;
        }
        // Drain a budget of chunks per frame from the backlog - no rescan.
        // W198.11: lowered 24 -> 8. Meshing a water chunk costs ~1.9ms
        // (emit does ~10 world.get per exposed cell), so 24/frame was ~45ms
        // over open ocean - a per-frame stall while flying. 8/frame caps it
        // to ~15ms and spreads the rest over the next frames (water meshes
        // trail a touch behind the terrain while moving fast, which the
        // distance-opaque far water largely hides). The real fix is
        // chunk-local neighbour access in the liquid mesher (the recurring
        // HashMap-per-cell pattern); this is the immediate relief.
        const LIQUID_CHUNKS_PER_FRAME: usize = 8;
        // W205: adaptive drain. While flying, chunk streaming enqueues up
        // to ~7 backlog entries per streamed chunk (self + face
        // neighbours) x ~32 dispatches/frame, so a fixed 8/frame drain
        // falls ever further behind - the water surface visibly trailed
        // the terrain at the streaming frontier (kelp standing "dry" on
        // the edge, the user's cut-off water boundary). Scale the drain
        // with the backlog (an eighth per frame, capped) so a burst
        // catches up in a few frames while the steady-state cost stays
        // at the old 8.
        // W205.1: cap 64 -> 16. The user's load log showed the 64-chunk
        // drain costing 17-75 ms/frame in a water-heavy world (each ocean
        // chunk emit is ~1 ms) - the catch-up must never dominate the
        // frame. 16/frame still clears a 1200-entry load burst in ~5 s.
        // W278: while essentially stationary spend a bigger budget -
        // the user watches the ocean surface fill in, and a 4 ms
        // trickle took minutes over a VD-32 ocean backlog (~30k+
        // entries after the initial fill). Standing still a ~12 ms
        // frame is imperceptible (same policy as the GPU-upload
        // budget in current_results_budget); the instant the player
        // moves we drop back so travel never stutters.
        let v = self.player_vel;
        // W334: "stationary" also requires an idle stream. W324
        // zeroes player_vel whenever the player's own chunk is not
        // loaded - exactly the flying-ahead-of-the-frontier case - so
        // velocity alone promoted the big 12 ms drain budget DURING
        // flight (the liqmesh 10-15 ms log spikes). While generation
        // is busy the small budget applies regardless of speed.
        let stationary = (v.x * v.x + v.y * v.y + v.z * v.z) < 4.0
            && self.pending_chunks.is_empty();
        let (drain_cap, budget_ms) = if stationary { (48, 12) } else { (16, 4) };
        let drain = LIQUID_CHUNKS_PER_FRAME
            .max(self.liquid_remesh_backlog.len() / 8)
            .min(drain_cap);
        // W278: NEAREST-first. The backlog is an unordered HashSet, so
        // the old `.take(drain)` meshed arbitrary (often distant)
        // chunks while the water around the player stayed missing.
        // One O(n) pass keeps the `drain` closest entries.
        let pcx = (self.player_pos.x.floor() as i32).div_euclid(cs);
        let pcz = (self.player_pos.z.floor() as i32).div_euclid(cs);
        let mut to_mesh: Vec<(i32, ChunkCoord)> = self
            .liquid_remesh_backlog
            .iter()
            .map(|cc| {
                let (dx, dz) = (cc[0] - pcx, cc[2] - pcz);
                (dx * dx + dz * dz, *cc)
            })
            .collect();
        if to_mesh.len() > drain {
            to_mesh.select_nth_unstable_by_key(drain, |(d, _)| *d);
            to_mesh.truncate(drain);
        }
        to_mesh.sort_unstable_by_key(|(d, _)| *d);
        let to_mesh: Vec<ChunkCoord> = to_mesh.into_iter().map(|(_, cc)| cc).collect();
        // W237: TIME budget on top of the count cap. Ocean chunks
        // cost ~1.4 ms each to mesh, so a full 16-chunk drain was a
        // ~22 ms frame while streaming over water (the user's log).
        // Chunks past the budget stay in the backlog (each is only
        // removed as it's processed).
        let drain_budget = std::time::Duration::from_millis(budget_ms);
        let drain_start = std::time::Instant::now();
        for cc in to_mesh {
            if drain_start.elapsed() >= drain_budget {
                break;
            }
            self.liquid_remesh_backlog.remove(&cc);
            // Per-kind vertex/index pool for THIS chunk only.
            let mut buckets: HashMap<u16, (Vec<MeshVertex>, Vec<u32>)> = HashMap::new();
            let bx = cc[0] * cs;
            let by = cc[1] * cs;
            let bz = cc[2] * cs;
            // W198.2: skip non-liquid cells via ONE chunk borrow + local
            // array reads instead of a `world.get` (HashMap) lookup per
            // cell. Only actual liquid cells pay emit_liquid_cell's
            // neighbour scan, so an all-air neighbour chunk (enqueued for
            // seam re-cull on stream-in) now costs 4096 cheap array reads
            // rather than 4096 hashmap lookups + emit. The old code's
            // per-cell world.get over 24 chunks/frame was the bulk of the
            // "flying over ocean" sim_ms (per-frame main-thread liquid
            // meshing); this is its dominant term.
            if let Some(chunk) = self.world.chunk(cc) {
                let cu = cs as usize;
                for lx in 0..cs {
                    for ly in 0..cs {
                        for lz in 0..cs {
                            let (ux, uy, uz) = (lx as usize, ly as usize, lz as usize);
                            let raw = chunk.get(ux, uy, uz);
                            // W203: waterlogged plant cells emit water
                            // faces too (the cell IS water in MC terms).
                            let watery = crate::block::block_is_waterlogged(raw.0);
                            let id = if watery {
                                BlockId(crate::block::WATER_ID)
                            } else {
                                raw
                            };
                            if liquid_for(id).is_none() {
                                continue;
                            }
                            // W198.6: skip cells fully enclosed by the SAME
                            // liquid - they emit ZERO faces, but emit still
                            // pays ~10 world.get (HashMap) lookups per cell
                            // to discover that. For a deep-ocean chunk that's
                            // nearly every cell (the bulk of liqM over water).
                            // Only decidable locally for interior cells; border
                            // cells have cross-chunk neighbours, so emit them.
                            let interior =
                                ux > 0 && uy > 0 && uz > 0 && ux < cu - 1 && uy < cu - 1 && uz < cu - 1;
                            if interior {
                                // W203: waterlogged neighbours count as the
                                // same body, so a water cell ringed by kelp
                                // still skips (and vice versa).
                                let same = |dx: usize, dy: usize, dz: usize| {
                                    crate::liquid::counts_as_liquid(
                                        chunk.get(dx, dy, dz).0,
                                        id.0,
                                    )
                                };
                                if same(ux + 1, uy, uz)
                                    && same(ux - 1, uy, uz)
                                    && same(ux, uy + 1, uz)
                                    && same(ux, uy - 1, uz)
                                    && same(ux, uy, uz + 1)
                                    && same(ux, uy, uz - 1)
                                {
                                    continue;
                                }
                            }
                            self.emit_liquid_cell([bx + lx, by + ly, bz + lz], &mut buckets);
                        }
                    }
                }
            }
            // W212: the water tint is the chunk's MC biome water colour
            // (sampled once per rebuild at the chunk centre - warm ocean
            // turquoise, swamp murk, frozen ocean steel-blue). Non-Mc
            // profiles keep the BLOCK_TYPES water blue. Lava's texture
            // is fully coloured, so it always tints white (W200).
            //
            // W284: cached per chunk COLUMN. The climate-noise biome
            // sample is not free, and a busy backlog re-meshes up to 48
            // chunks per frame - re-deriving the same column colour on
            // every rebuild was pure waste. Cleared on dim switch /
            // world reset alongside the other liquid state.
            let water_tint = match self.water_tint_cache.get(&[cc[0], cc[2]]) {
                Some(t) => *t,
                None => {
                    let ctx = self.current_worldgen_ctx();
                    let t = if matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
                        let mc = crate::worldgen::mcgen::terrain::mc_full_biome_at(
                            &ctx,
                            bx + 8,
                            bz + 8,
                        );
                        crate::worldgen::mcgen::colors::water_color(mc)
                    } else {
                        crate::block::block_type(crate::block::WATER_ID)
                            .map(|bt| bt.color)
                            .unwrap_or([1.0, 1.0, 1.0])
                    };
                    self.water_tint_cache.insert([cc[0], cc[2]], t);
                    t
                }
            };
            let renderer = self.renderer.as_mut().expect("renderer ready");
            if let Some(old) = self.liquid_mesh_handles.remove(&cc) {
                for (_, h, _) in old {
                    renderer.remove_mesh(h);
                }
            }
            let mut handles = Vec::new();
            for (liquid_id, (vertices, indices)) in buckets {
                if vertices.is_empty() || indices.is_empty() {
                    continue;
                }
                let label = format!("voxel_liquid_{}_{}_{}_{liquid_id}", cc[0], cc[1], cc[2]);
                let h = renderer.create_mesh(&label, &vertices, &indices);
                // W200: liquids sample the MC animated texture from the
                // block atlas now - bind it (unbound meshes fall back to
                // the 1x1 white default = the old flat look).
                if let Some(a) = self.voxel_atlas.clone() {
                    renderer.bind_mesh_albedo_arc(h, a);
                }
                // W108: water translucent (alpha pipeline, cull_mode=None);
                // lava opaque. Draw order handled in infengine_render::render().
                if liquid_id == crate::block::WATER_ID {
                    renderer.set_mesh_transparent(h, true);
                }
                let tint = match liquid_id {
                    crate::block::WATER_ID => water_tint,
                    crate::block::LAVA_ID => [1.0, 1.0, 1.0],
                    other => crate::block::block_type(other)
                        .map(|bt| bt.color)
                        .unwrap_or([1.0, 1.0, 1.0]),
                };
                handles.push((liquid_id, h, tint));
            }
            if handles.is_empty() {
                self.liquid_mesh_handles.remove(&cc);
            } else {
                self.liquid_mesh_handles.insert(cc, handles);
            }
        }
        let _ = block_is_transparent_render;
    }

    /// Spawn a fresh drop entity at the voxel centre. Host-only
    /// path: assigns a new id, appends to `dropped_items`, and
    /// broadcasts a `vx.drop_spawn` so every connected peer
    /// renders it. Caller passes the source `block` (whatever
    /// was just broken or washed) - air drops are skipped.
    pub(crate) fn spawn_drop(&mut self, block: BlockId, voxel: [i32; 3]) {
        drops::spawn(self, block, voxel);
    }

    /// Step every drop's physics + pickup logic for one frame.
    /// Host runs the full simulation. Clients only advance the
    /// age + apply a simple gravity preview so the bob looks
    /// right between server snapshots; pickup is host-side.
    pub(crate) fn tick_drops(&mut self, dt: f32) {
        drops::tick_physics(self, dt);
    }

    /// Per-frame scheduler. Walks the `LIQUIDS` registry and
    /// dispatches each liquid's tick passes when the cadence
    /// (`Liquid::tick_period`) matches. Host-only - clients
    /// receive the resulting `vx.edit` traffic.
    pub(crate) fn tick_liquids(&mut self, dt: f32) {
        if matches!(self.net, NetRole::Client { .. }) {
            return;
        }
        // W104: advance the liquid clock at a FIXED 20 ticks/sec (MC's
        // game-tick rate) instead of once per render frame. Per-frame
        // advancement made flow speed scale with FPS (~12x too fast at
        // 240 FPS, and framerate-dependent) and forced the schedule
        // drain + merge scan + every never-settling cell's chunk
        // remesh to run on EVERY frame. A fixed cadence is MC-faithful
        // and means a high-FPS frame usually does zero liquid work, so
        // the residual liquid cost no longer tracks the frame rate.
        const LIQUID_TICK_DT: f32 = 1.0 / 20.0;
        // Cap catch-up so a hitch (or a long alt-tab) can't avalanche
        // into a multi-second liquid burst on the resume frame.
        const MAX_CATCHUP_TICKS: u32 = 4;
        self.liquid_tick_accum += dt;
        if self.liquid_tick_accum < LIQUID_TICK_DT {
            // Not a game-tick boundary yet - skip all liquid work.
            return;
        }
        // Per-game-tick CPU budget for liquid cell processing. Cells
        // that don't fit keep their slot in the schedule and tick on
        // the next game tick.
        const MAX_LIQUID_PER_FRAME: usize = 4096;
        // W272: a TIME budget on top of the cell cap. A fresh ocean
        // world's settle storm schedules tens of thousands of cells;
        // 4096 cells x ~10 world.gets each x 2+ catch-up ticks was a
        // ~90 ms `liquids` phase at 10 FPS - a death spiral (slower
        // frames -> more catch-up ticks -> slower frames). Unprocessed
        // cells stay due in the schedule (pop_due only removes what we
        // handle), so the settle just spreads over more frames.
        const LIQUID_TIME_BUDGET: std::time::Duration =
            std::time::Duration::from_millis(4);
        let budget_start = std::time::Instant::now();
        // W103: chunks touched by liquid edits this frame, deduplicated
        // across every catch-up tick. Rebuilt ONCE at the end instead
        // of per-cell (per-cell cloned the same ~8 KB chunk thousands
        // of times a frame - the open-area 10 fps collapse).
        let mut dirty: HashSet<ChunkCoord> = HashSet::new();
        let mut catch = 0u32;
        while self.liquid_tick_accum >= LIQUID_TICK_DT && catch < MAX_CATCHUP_TICKS {
            self.liquid_tick_accum -= LIQUID_TICK_DT;
            catch += 1;
            self.liquid_clock = self.liquid_clock.wrapping_add(1);
            let now = self.liquid_clock;
            let mut processed = 0usize;
            while processed < MAX_LIQUID_PER_FRAME {
                // Check the time budget every 64 cells (Instant::now
                // per cell would itself cost).
                if processed.is_multiple_of(64)
                    && budget_start.elapsed() > LIQUID_TIME_BUDGET
                {
                    break;
                }
                let Some(pos) = self.liquid_schedule.pop_due(now) else {
                    break;
                };
                // Identify which liquid owns this cell, if any. The
                // cell may have been broken since it was scheduled -
                // skip silently.
                let b = self.world.get(pos);
                let Some(liquid) = liquid_for(b) else {
                    continue;
                };
                // Snapshot the liquid by value (no &self borrow into
                // the inner method).
                let lq = *liquid;
                self.tick_liquid_cell(pos, &lq, now, &mut dirty);
                processed += 1;
            }
            // Source-merge pass: each `infinite_source` liquid gets a
            // chance to promote flow cells. Runs per game tick (MC
            // merges on tick), 256-cell cap inside `liquid_merge_step`.
            for liquid in LIQUIDS {
                if liquid.infinite_source {
                    self.tick_liquid_merge(liquid);
                }
            }
            // W272: over budget - skip further catch-up ticks this
            // frame (the accumulator carries over; the schedule keeps
            // the unprocessed cells due).
            if budget_start.elapsed() > LIQUID_TIME_BUDGET {
                break;
            }
        }
        // Drop any backlog beyond the catch-up cap so the accumulator
        // can't grow without bound after a long stall.
        if catch >= MAX_CATCHUP_TICKS {
            self.liquid_tick_accum = 0.0;
        }
        // W103: one batched remesh dispatch for every chunk this frame's
        // liquid ticks touched - one job per distinct chunk.
        if !dirty.is_empty() {
            // W198.12: enqueue the touched chunks for an INCREMENTAL liquid
            // remesh (no global O(all-liquids) scan - that periodic scan was
            // the turning judder).
            for &cc in &dirty {
                self.enqueue_liquid_remesh(cc);
            }
            // W326: the BLOCK remesh goes through the deferred queue
            // instead of a synchronous rebuild_chunks. The sync call
            // cloned chunk + halo snapshots for every touched chunk
            // right here on the main thread, OUTSIDE the 4 ms liquid
            // budget - a frontier settle storm made the `liquids`
            // phase spike to 18 ms per tick (the walking-stutter
            // log). The liquid surface itself updates immediately via
            // the incremental liquid remesh above; the chunk-grid
            // remesh (face culling at air<->water boundaries) can
            // trail by a few frames unnoticed.
            for cc in dirty {
                if self.world.chunk(cc).is_some()
                    && self.pending_remesh_set.insert(cc)
                {
                    self.pending_remesh.push_back(cc);
                }
            }
        }
    }

    /// MC `FlowingFluid.tick(pos)`: pulls `pos` through
    /// `getNewLiquid`, applies + spreads, then re-schedules every
    /// neighbour the apply touched (and `pos` itself if still
    /// active) at `now + tick_period`.
    pub(crate) fn tick_liquid_cell(
        &mut self,
        pos: [i32; 3],
        liquid: &Liquid,
        now: u64,
        dirty: &mut HashSet<ChunkCoord>,
    ) {
        let liquid_id = liquid.block_id;
        let delay = liquid.tick_period.max(1) as u64;
        // Build the per-cell plan against the current world snapshot.
        let mut plan = LiquidPlanStep::default();
        let mut updates: std::collections::HashMap<[i32; 3], (u8, bool)> =
            std::collections::HashMap::new();
        liquid_plan_cell(
            &self.world,
            &self.liquid_level,
            pos,
            liquid,
            self.liquid_bias_drops,
            &mut plan,
            &mut updates,
        );
        plan.place = updates.into_iter().map(|(p, (l, f))| (p, l, f)).collect();
        if plan.evaporate.is_empty() && plan.decay.is_empty() && plan.place.is_empty() {
            // Cell is settled. Don't reschedule - it'll only fire
            // again when a neighbouring edit/spread queues it.
            return;
        }
        // The schedule replaces the legacy `liquid_active` set;
        // pass a throwaway HashSet so `liquid_apply_plan` can
        // still maintain its internal wake-up shape. The
        // schedule itself catches the same wake via the explicit
        // re-schedule loop below.
        let mut scratch_active: HashSet<[i32; 3]> = HashSet::new();
        let tape = liquid_apply_plan(
            &mut self.world,
            &mut self.liquid_level,
            &mut scratch_active,
            &mut self.liquid_positions,
            liquid,
            plan,
        );
        // Drops + registry cleanup for washable replacements.
        for (block, p) in &tape.washed {
            if block.0 == 15 {
                self.snow_positions.remove(p);
            }
            // Q.4e: water spreading INTO a fire cell extinguishes
            // the fire. No drop spawn (burned blocks + fire don't
            // drop in MC). Clear the parallel age entry so the
            // fire_tick loop stops visiting the slot.
            if block.0 == block::FIRE_ID {
                self.fires.remove(p);
                continue;
            }
            self.spawn_drop(*block, *p);
        }
        // W103: accumulate the chunks this cell touched into the
        // frame-level dirty set; tick_liquids rebuilds them ONCE after
        // the per-frame budget drains. Previously every active liquid
        // cell called rebuild_chunks here directly, re-cloning the SAME
        // chunk once per cell — thousands of times a frame in a pool.
        for p in &tape.evaporated {
            for c in Self::affected_chunks(*p) {
                dirty.insert(c);
            }
        }
        for (p, _, _) in &tape.placed {
            for c in Self::affected_chunks(*p) {
                dirty.insert(c);
            }
        }
        // Q.4a: solidified cells are former lava cells now holding
        // obsidian / cobble. Treat them like placed entries for
        // chunk rebuild + mesh dirty so the renderer drops the
        // liquid slab and emits a full solid cube.
        for (p, _) in &tape.solidified {
            for c in Self::affected_chunks(*p) {
                dirty.insert(c);
            }
        }
        // W198.12: the touched chunks are already in `dirty` (collected
        // above) and the caller (tick_liquids) enqueues them for an
        // incremental liquid remesh, so we no longer set liquid_mesh_dirty
        // here - that tripped the global O(all-liquids) signature scan on
        // every liquid tick (~15ms spike = the turning judder).
        // Broadcast edits + level updates over the wire.
        let cfg = bincode::config::standard();
        if let NetRole::Server { server, .. } = &mut self.net {
            for p in &tape.evaporated {
                let edit = EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: 0,
                };
                self.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
            for (p, block_id, level) in &tape.placed {
                let edit = EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: *block_id,
                };
                self.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
                if let Ok(buf) = bincode::serde::encode_to_vec((*p, *level), cfg)
                {
                    server.broadcast_bytes("vx.liquid_lvl", buf);
                }
            }
            for (p, lvl) in &tape.decayed {
                if let Ok(buf) = bincode::serde::encode_to_vec((*p, *lvl), cfg) {
                    server.broadcast_bytes("vx.liquid_lvl", buf);
                }
            }
            // Q.4a: lava-water boundary -> broadcast the solid
            // block id as a regular edit so remote peers see the
            // obsidian / cobble swap. No liquid_lvl needed - the
            // cell is no longer a liquid.
            for (p, solid_id) in &tape.solidified {
                let edit = EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: *solid_id,
                };
                self.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
        }
        // MC: spread schedules the NEXT tick for each cell it
        // actually wrote into. A cell whose state stayed exactly
        // the same (apply collapsed the proposal into a no-op
        // because target was already at that level) doesn't get
        // re-scheduled - that's how settled pools quiesce.
        //
        // We mirror by only scheduling cells the tape ACTUALLY
        // touched + the 6 neighbours of those, so an outward wave
        // keeps marching and a settled pool stops ticking.
        for p in &tape.evaporated {
            for d in &SIX_NSEW_UD {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if self.world.get(n).0 == liquid_id {
                    self.liquid_schedule.schedule(n, now + delay);
                }
            }
        }
        for (p, _, _) in &tape.placed {
            self.liquid_schedule.schedule(*p, now + delay);
            for d in &SIX_NSEW_UD {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if self.world.get(n).0 == liquid_id {
                    self.liquid_schedule.schedule(n, now + delay);
                }
            }
        }
        for (p, _) in &tape.decayed {
            self.liquid_schedule.schedule(*p, now + delay);
            for d in &SIX_NSEW_UD {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if self.world.get(n).0 == liquid_id {
                    self.liquid_schedule.schedule(n, now + delay);
                }
            }
        }
        // Q.4a: solidified cells leave the liquid pool entirely;
        // wake every adjacent liquid neighbour (water OR lava) so
        // the flow re-evaluates against the new solid block. MC
        // does this implicitly via `setBlockAndUpdate` which fires
        // neighborChanged.
        for (p, _) in &tape.solidified {
            for d in &SIX_NSEW_UD {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                let nb = self.world.get(n);
                if crate::block::block_is_liquid(nb) {
                    self.liquid_schedule.schedule(n, now + 1);
                }
            }
        }
    }


    /// Source-merge pass: a flow voxel of `liquid` adjacent to
    /// two or more same-kind source neighbours gets promoted to
    /// a source itself (MC's infinite-water trick). Only called
    /// for liquids whose `infinite_source` flag is set.
    ///
    /// Mirrors the reference `shouldBecomeSource +
    /// isStableForSourceConversion` pair: the promotion only
    /// fires when every NSEW neighbour is either a same-kind
    /// source or a solid wall - no drop and no open flow path
    /// out of the pocket. Prevents stray source spawns in
    /// chaotic configurations.
    pub(crate) fn tick_liquid_merge(&mut self, liquid: &Liquid) {
        if self.liquid_level.is_empty() {
            return;
        }
        let promoted = liquid_merge_step(&self.world, &self.liquid_level, liquid);
        if promoted.is_empty() {
            return;
        }
        for pos in &promoted {
            self.liquid_level.insert(*pos, 0);
        }
        // W198.12: incremental remesh of the promoted cells' chunks instead
        // of the global O(all-liquids) scan.
        let cs = CHUNK_SIZE as i32;
        for pos in &promoted {
            // W250: the level change persists as a chunk-payload
            // override - mark the chunk without a block write.
            self.world.mark_dirty([
                pos[0].div_euclid(cs),
                pos[1].div_euclid(cs),
                pos[2].div_euclid(cs),
            ]);
            self.enqueue_liquid_remesh([
                pos[0].div_euclid(cs),
                pos[1].div_euclid(cs),
                pos[2].div_euclid(cs),
            ]);
        }
        let cfg = bincode::config::standard();
        if let NetRole::Server { server, .. } = &mut self.net {
            for pos in &promoted {
                if let Ok(buf) = bincode::serde::encode_to_vec((*pos, 0u8), cfg)
                {
                    server.broadcast_bytes("vx.liquid_lvl", buf);
                }
            }
        }
    }
}
