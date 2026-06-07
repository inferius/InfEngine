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
        let liquid_id = block.0;
        if liquid_for(block).is_none() {
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
        // Per-cell flow vector, baked into the faces so the water shader
        // scrolls its caustics in the right direction (zero -> isotropic).
        let flow = self.liquid_flow_dir([vx, vy, vz], liquid_id);
        // Top face (CCW from above so the Back-cull keeps it) - only when
        // the cell above isn't the same liquid.
        let above_same = self.world.get([vx, vy + 1, vz]).0 == liquid_id;
        if !above_same {
            let t0 = [fx, fy + h00, fz];
            let t1 = [fx, fy + h01, fz + 1.0];
            let t2 = [fx + 1.0, fy + h11, fz + 1.0];
            let t3 = [fx + 1.0, fy + h10, fz];
            let normal = water_face_normal(t0, t1, t2, t3);
            push_quad_flow(vertices, indices, t0, t1, t2, t3, normal, flow);
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
                flow,
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
                flow,
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
                flow,
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
                flow,
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
                flow,
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
    pub(crate) fn maybe_rebuild_liquid_mesh(&mut self) {
        if !self.liquid_mesh_dirty {
            return;
        }
        if self.renderer.is_none() {
            return;
        }
        let cs = CHUNK_SIZE as i32;
        let chunk_of = |p: [i32; 3]| -> ChunkCoord {
            [p[0].div_euclid(cs), p[1].div_euclid(cs), p[2].div_euclid(cs)]
        };
        // Per-chunk signature: a commutative fold (wrapping-add of a per-cell
        // hash over position + level) so HashSet iteration order is irrelevant.
        let mut cur_sig: HashMap<ChunkCoord, u64> = HashMap::new();
        for p in self.liquid_positions.iter().copied() {
            if liquid_for(self.world.get(p)).is_none() {
                continue;
            }
            let lvl = self.liquid_level.get(&p).copied().unwrap_or(0) as u64;
            let mut hsh = 0xcbf2_9ce4_8422_2325u64;
            for v in [p[0] as u64, p[1] as u64, p[2] as u64, lvl] {
                hsh ^= v;
                hsh = hsh.wrapping_mul(0x0000_0100_0000_01b3);
            }
            let e = cur_sig.entry(chunk_of(p)).or_insert(0u64);
            *e = e.wrapping_add(hsh);
        }
        // Chunks to (re)mesh: new / changed signature, or liquid now gone.
        let mut changed: Vec<ChunkCoord> = Vec::new();
        for (cc, sig) in &cur_sig {
            if self.liquid_chunk_sig.get(cc) != Some(sig) {
                changed.push(*cc);
            }
        }
        for cc in self.liquid_chunk_sig.keys() {
            if !cur_sig.contains_key(cc) {
                changed.push(*cc);
            }
        }
        const LIQUID_CHUNKS_PER_FRAME: usize = 24;
        let budget_hit = changed.len() > LIQUID_CHUNKS_PER_FRAME;
        changed.truncate(LIQUID_CHUNKS_PER_FRAME);
        for cc in changed {
            // Per-kind vertex/index pool for THIS chunk only.
            let mut buckets: HashMap<u16, (Vec<MeshVertex>, Vec<u32>)> =
                HashMap::new();
            let bx = cc[0] * cs;
            let by = cc[1] * cs;
            let bz = cc[2] * cs;
            let cells = (0..cs).flat_map(move |lx| {
                (0..cs).flat_map(move |ly| {
                    (0..cs).map(move |lz| [bx + lx, by + ly, bz + lz])
                })
            });
            for p in cells {
                self.emit_liquid_cell(p, &mut buckets);
            }
            // Swap THIS chunk's per-kind liquid meshes for the freshly
            // built ones, then record its new signature.
            let renderer = self.renderer.as_mut().expect("renderer ready");
            if let Some(old) = self.liquid_mesh_handles.remove(&cc) {
                for (_, h) in old {
                    renderer.remove_mesh(h);
                }
            }
            let mut handles = Vec::new();
            for (liquid_id, (vertices, indices)) in buckets {
                if vertices.is_empty() || indices.is_empty() {
                    continue;
                }
                let label =
                    format!("voxel_liquid_{}_{}_{}_{liquid_id}", cc[0], cc[1], cc[2]);
                let h = renderer.create_mesh(&label, &vertices, &indices);
                // W108: water translucent (alpha pipeline, cull_mode=None);
                // lava opaque. Draw order handled in infengine_render::render().
                if liquid_id == crate::block::WATER_ID {
                    renderer.set_mesh_transparent(h, true);
                }
                handles.push((liquid_id, h));
            }
            match cur_sig.get(&cc) {
                Some(sig) => {
                    self.liquid_chunk_sig.insert(cc, *sig);
                }
                None => {
                    self.liquid_chunk_sig.remove(&cc);
                }
            }
            if handles.is_empty() {
                self.liquid_mesh_handles.remove(&cc);
            } else {
                self.liquid_mesh_handles.insert(cc, handles);
            }
        }
        // Clear dirty only once every changed chunk is re-meshed; any
        // beyond the per-frame budget finish on following frames.
        if !budget_hit {
            self.liquid_mesh_dirty = false;
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
        }
        // Drop any backlog beyond the catch-up cap so the accumulator
        // can't grow without bound after a long stall.
        if catch >= MAX_CATCHUP_TICKS {
            self.liquid_tick_accum = 0.0;
        }
        // W103: one batched remesh dispatch for every chunk this frame's
        // liquid ticks touched - one job per distinct chunk.
        if !dirty.is_empty() {
            let coords: Vec<ChunkCoord> = dirty.into_iter().collect();
            self.rebuild_chunks(&coords);
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
        if !tape.evaporated.is_empty()
            || !tape.placed.is_empty()
            || !tape.decayed.is_empty()
            || !tape.solidified.is_empty()
        {
            self.liquid_mesh_dirty = true;
        }
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
        self.liquid_mesh_dirty = true;
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
