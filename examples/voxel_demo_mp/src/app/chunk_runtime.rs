//! W47.28 - chunk_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {

    /// Drop every loaded chunk + the per-chunk mesh handles +
    /// tracker sets that the regenerated world will repopulate.
    /// Used by `Play offline` when the menu picks a different
    /// `WorldGen` variant, so the new world isn't a mix of the
    /// old + new chunk layouts.
    pub(crate) fn reset_world(&mut self) {
        let coords: Vec<ChunkCoord> = self
            .world
            .iter_chunks()
            .map(|(c, _)| *c)
            .collect();
        if let Some(renderer) = self.renderer.as_mut() {
            for (_cc, subs) in self.chunk_meshes.drain() {
                for (_block, handle) in subs {
                    renderer.remove_chunk_mesh(handle);
                }
            }
            for (_cc, kinds) in self.liquid_mesh_handles.drain() {
                for (_id, h, _tint) in kinds {
                    renderer.remove_mesh(h);
                }
            }
            // W321: drop the far-shell ring with the world.
            for (_k, t) in self.lod_meshes.drain() {
                if let Some(h) = t.opaque {
                    renderer.remove_chunk_mesh(h);
                }
                if let Some(h) = t.water {
                    renderer.remove_chunk_mesh(h);
                }
            }
            for (_k, t) in self.lod2_meshes.drain() {
                if let Some(h) = t.opaque {
                    renderer.remove_chunk_mesh(h);
                }
                if let Some(h) = t.water {
                    renderer.remove_chunk_mesh(h);
                }
            }
            for (_k, t) in self.lod3_meshes.drain() {
                if let Some(h) = t.opaque {
                    renderer.remove_chunk_mesh(h);
                }
                if let Some(h) = t.water {
                    renderer.remove_chunk_mesh(h);
                }
            }
        }
        for cc in coords {
            self.world.remove_chunk(cc);
        }
        self.pending_chunks.clear();
        self.pending_set.clear();
        self.pending_unload.clear();
        self.pending_unload_side = Default::default();
        self.unload_retain_due = false;
        self.bed_states.clear();
        self.in_flight_chunks.clear();
        self.lod_pending.clear();
        self.lod_pending_set.clear();
        self.lod2_pending.clear();
        self.lod3_pending.clear();
        self.lod2_pending_set.clear();
        self.lod3_pending_set.clear();
        self.pending_remesh.clear();
        self.pending_remesh_set.clear();
        self.pending_seam_remesh.clear();
        self.disk_backed.clear();
        self.chest_render_positions.clear();
        self.pending_seam_set.clear();
        // W198.13: force the streamer to re-run its queue + unload scan
        // on the next frame (player_chunk may be numerically unchanged
        // but the loaded set is now empty / freshly built).
        self.last_stream_origin = None;
        // W106: stream_delivered now carries unprocessed worker results
        // across frames (time-budgeted processing), so drop them on reset -
        // stale results from the old world must not apply to the new one.
        self.stream_delivered.clear();
        // W72: drop the in-flight worker lighting-job count. A world
        // reset (return-to-menu teardown / new-world entry) sets
        // `workers = None`, so any lighting jobs dispatched for the old
        // world never send their `WorkerResult::Lit` back to decrement
        // this. Left non-zero, the NEXT world's load hangs forever in
        // the BuildingLighting phase, which waits for
        // `pending_lighting_jobs == 0` (the "infinite loading when I
        // load a second world" bug).
        self.pending_lighting_jobs = 0;
        self.lighting_capped_frames = 0;
        // W237: queued-but-undispatched lighting segments belong to
        // the old world's coords - drop them with the job counter.
        self.pending_lighting_segments.clear();
        self.pending_lighting_chunks.clear();
        self.snow_positions.clear();
        self.torch_positions.clear();
        self.emitter_positions.clear();
        // W5.2a: portal tracker rebuilds via world_set_tracked
        // when chunks reload, so clear here too.
        self.portal_positions.clear();
        self.structure_mob_anchors.clear();
        // W242.2: spawner index + tick state are per-world.
        self.spawner_positions.clear();
        self.spawner_states.clear();
        self.spawner_kinds.clear();
        self.spawner_spin.clear();
        self.torch_orient.clear();
        self.stair_states.clear();
        self.liquid_schedule.clear();
        self.liquid_clock = 0;
        self.liquid_tick_accum = 0.0;
        self.liquid_chunk_sig.clear();
        self.liquid_remesh_backlog.clear();
        self.liquid_mesh_inflight.clear();
        self.bell_positions.clear();
        self.water_tint_cache.clear();
        self.liquid_level.clear();
        self.liquid_level_share = None;
        self.liquid_positions.clear();
        self.lava_columns.clear();
        self.falling_pending.clear();
        self.persistent_leaves.clear();
        self.lever_on.clear();
        self.liquid_mesh_dirty = true;
        // W125: door state is per-world; stale entries from the
        // previous world must not leak open/facing flags in.
        self.door_open.clear();
        self.door_facing.clear();
        // W164: note-block pitches are per-world too.
        self.note_pitches.clear();
        // W143: stored rail shapes are per-world too.
        self.rail_shapes.clear();
        self.chests.clear();
        self.chest_open = None;
        self.chest_held = None;
        // W126: chest entity-render state is per-world too.
        self.chest_facing.clear();
        self.chest_lid_anim.clear();
        self.furnaces.clear();
        self.furnace_runtimes.clear();
        self.furnace_open = None;
        self.furnace_held = None;
        // W50.3: crafting_table is client-side only; drop its
        // grid contents on world reset too.
        self.crafting_table_open = None;
        self.craft_grid_3x3 = [None; 9];
        self.crafting_table_held = None;
        // W171: stonecutter is client-side; drop its input on reset.
        self.stonecutter_open = None;
        self.smithing_open = None;
        self.smithing_slots = [None; 3];
        self.trade_open = None;
        self.save_fixups_pending = false;
        self.villager_trade_xp.clear();
        self.mob_professions.clear();
        self.stonecutter_input = None;
        // W50.5: brewing stand maps cleared on world reset.
        self.brewing_stands.clear();
        self.brewing_runtimes.clear();
        self.brewing_open = None;
        self.brewing_held = None;
        // W134: carts are per-world (persisted since W137).
        self.carts.clear();
        self.remote_carts.clear();
        self.riding_cart = None;
        self.riding_mob = None;
        self.riding_jump = false;
        self.cart_input = 0.0;
        self.cart_dismount_pending = false;
        self.cart_net_accum = 0.0;
        // W140: arrows are ephemeral - drop them with the world.
        self.arrows.clear();
        self.remote_arrows.clear();
        self.fireballs.clear();
        self.remote_fireballs.clear();
        self.thrown_potions.clear();
        self.primed_tnt.clear();
        self.remote_tnt.clear();
    }

    /// Close the chest UI. Drops the cursor-held stack back into
    /// the chest at the first free slot (or onto the floor as a
    /// drop if the chest is gone / full). Called by ESC, the
    /// window's close button, and the periodic out-of-range check.
    pub(crate) fn close_chest_ui(&mut self) {
        let pos = self.chest_open.take();
        let held = self.chest_held.take();
        if let Some(p) = pos {
            // W126: MC chest-close thunk.
            let spos = [p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5];
            self.sfx_at("container/chest/close", spos, |_, _| {});
            let leftovers = block::chest::recover_held_on_close(
                self.chests.get_mut(&p),
                held,
            );
            for id in leftovers {
                self.spawn_drop(BlockId(id), p);
            }
            // W5.3a: notify the host we're no longer viewing this
            // chest. Host clears the viewer registration so further
            // mutations stop fanning out a state snapshot to us.
            if let NetRole::Client { client, .. } = &mut self.net {
                let cfg = bincode::config::standard();
                let msg = crate::net::ChestCloseMsg { pos: p };
                if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                    let _ = client.send_bytes("vx.chest_close", buf);
                }
            }
        }
    }

    /// W50.3: close the 3x3 crafting-table UI. Drops the grid +
    /// held stack back into the inventory; on overflow the rest
    /// falls as item entities at the table position. No network
    /// notify because crafting_table is client-side state.
    pub(crate) fn close_crafting_table_ui(&mut self) {
        let pos = self.crafting_table_open.take();
        // Collect grid + held leftovers first so the iterator
        // borrow ends before we hit `spawn_drop` (which needs a
        // fresh `&mut self`).
        let mut overflow: Vec<(BlockId, u8)> = Vec::new();
        for cell in self.craft_grid_3x3.iter_mut() {
            if let Some((b, c)) = cell.take()
                && !crate::inventory::inv_pickup_n(&mut self.inventory, b, c)
            {
                overflow.push((b, c));
            }
        }
        if let Some((id, c)) = self.crafting_table_held.take()
            && !crate::inventory::inv_pickup_n(
                &mut self.inventory,
                BlockId(id),
                c,
            )
        {
            overflow.push((BlockId(id), c));
        }
        if let Some(p) = pos {
            for (b, _c) in overflow {
                // spawn_drop emits a single drop entity per call;
                // dropping multiple of the same item is fine because
                // each drop accumulates on pickup.
                self.spawn_drop(b, p);
            }
        }
    }

    /// W171: close the stonecutter UI. Returns the input stack to
    /// the inventory (or drops it at the block on overflow).
    /// Client-side; used by the ESC path (the X-button close is
    /// reconciled in `ui::render`).
    pub(crate) fn close_stonecutter_ui(&mut self) {
        let pos = self.stonecutter_open.take();
        if let Some((id, c)) = self.stonecutter_input.take()
            && !crate::inventory::inv_pickup_n(&mut self.inventory, BlockId(id), c)
            && let Some(p) = pos
        {
            self.spawn_drop(BlockId(id), p);
        }
    }

    /// W410: close the smithing UI. Returns every slot stack to the
    /// inventory (floor drop at the table on overflow).
    pub(crate) fn close_smithing_ui(&mut self) {
        let pos = self.smithing_open.take();
        let slots = std::mem::take(&mut self.smithing_slots);
        for (id, c) in slots.into_iter().flatten() {
            if !crate::inventory::inv_pickup_n(
                &mut self.inventory,
                BlockId(id),
                c,
            ) && let Some(p) = pos
            {
                self.spawn_drop(BlockId(id), p);
            }
        }
    }

    /// W50.5: close the brewing stand UI. Returns the held stack
    /// into the stand (filtered by accept) + falls back to floor
    /// drops at the stand position on overflow.
    pub(crate) fn close_brewing_ui(&mut self) {
        let pos = self.brewing_open.take();
        let held = self.brewing_held.take();
        if let Some(p) = pos {
            let leftovers = block::brewing::recover_held_on_close(
                self.brewing_stands.get_mut(&p),
                held,
            );
            for id in leftovers {
                self.spawn_drop(BlockId(id), p);
            }
        }
    }

    /// Close the furnace UI. Mirrors `close_chest_ui` but routes
    /// the held-stack recovery through `block::furnace`; the
    /// furnace's accept filters mean a held stack of the wrong
    /// type for input / fuel falls as floor drops.
    pub(crate) fn close_furnace_ui(&mut self) {
        let pos = self.furnace_open.take();
        let held = self.furnace_held.take();
        if let Some(p) = pos {
            let leftovers = block::furnace::recover_held_on_close(
                self.furnaces.get_mut(&p),
                held,
            );
            // W5.3b: notify the host so the viewer registration
            // clears + state broadcasts stop.
            if let NetRole::Client { client, .. } = &mut self.net {
                let cfg = bincode::config::standard();
                let msg = crate::net::FurnaceCloseMsg { pos: p };
                if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                    let _ = client.send_bytes("vx.furnace_close", buf);
                }
            }
            for id in leftovers {
                self.spawn_drop(BlockId(id), p);
            }
        }
    }

    /// L.2: enqueue every initial chunk coord into `pending_chunks` +
    /// `pending_set` so the worker pool can generate them
    /// asynchronously. Returns the total number of coords enqueued
    /// (= the `target_chunks` value the Loading state needs).
    ///
    /// Previously this function generated all chunks synchronously on
    /// the main thread (10-30 s for the Realistic worldgen). Now it
    /// just pushes coords and returns immediately; the loading-state
    /// tick calls `stream_chunks` each frame to drain the queue via
    /// the worker pool.
    pub(crate) fn build_world(&mut self) -> u32 {
        // Clear any stale queue from a previous session.
        self.pending_chunks.clear();
        self.pending_set.clear();
        self.pending_unload.clear();
        self.pending_unload_side = Default::default();
        self.unload_retain_due = false;
        self.bed_states.clear();
        self.in_flight_chunks.clear();
        self.pending_remesh.clear();
        self.pending_remesh_set.clear();
        self.pending_seam_remesh.clear();
        self.disk_backed.clear();
        self.chest_render_positions.clear();
        self.pending_seam_set.clear();
        // W236: fresh world entry - the Finalising step must apply
        // persisted state (load_server_state) exactly once again.
        self.load_state_applied = false;
        // W198.13: force the streamer to re-run its queue + unload scan
        // on the next frame (player_chunk may be numerically unchanged
        // but the loaded set is now empty / freshly built).
        self.last_stream_origin = None;
        self.stream_delivered.clear(); // W106: see clear_world_state note.
        // W27.1: use the runtime view_distance so target_chunks matches the
        // live count that stream_chunks will also use. Previously INITIAL_CHUNKS
        // (=3) was used here but stream_chunks uses view_distance (default 4),
        // causing the counter to show e.g. 1296/784 (100%) because the target
        // was 7x7x16=784 but the live count rose to 9x9x16=1296.
        // W250: the LOADING screen only waits for a core ring - the
        // streamer extends it to the full view distance during play,
        // outward from the player (no near-player holes since W249).
        // Waiting for the whole VD-32 ring (67k chunks) kept the
        // loading screen up for minutes. (W267: the streamer now also
        // clamps to this radius while Loading - it used to widen the
        // ring underneath the loading gate, recreating the wait.)
        let radius = (self.settings.view_distance.clamp(2, VIEW_DISTANCE_MAX as u8)
            as i32)
            .min(Self::LOADING_CORE_RADIUS);
        // W249: centre the initial ring on the PLAYER's chunk. A
        // fresh world still starts at the origin (the old W28.1
        // assumption); a saved world's pose was pre-applied by
        // enter_world, so the build fills the terrain the player
        // actually stands in instead of a view-distance ring around
        // 0,0 that load_world used to throw away.
        let cs = CHUNK_SIZE as i32;
        let pcx = (self.player_pos.x.floor() as i32).div_euclid(cs);
        let pcz = (self.player_pos.z.floor() as i32).div_euclid(cs);
        // W28.3: only enqueue Y slices visible from the player's current Y.
        let (cy_min, cy_max) = self.vertical_chunk_range();
        let mut enqueued: u32 = 0;
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let dist_sq = (dx * dx + dz * dz) as i64;
                for cy in cy_min..=cy_max {
                    let cc: ChunkCoord = [pcx + dx, cy, pcz + dz];
                    if self.pending_set.insert(cc) {
                        self.pending_chunks.push(PendingChunk {
                            neg_dist_sq: -dist_sq,
                            coord: cc,
                        });
                        enqueued += 1;
                    }
                }
            }
        }
        // Pin the worldgen profile snapshot so the loading tick can
        // later record it in `last_built_worldgen` once done.
        let active_wg = self.current_worldgen_ctx().profile;
        self.last_built_worldgen = Some(active_wg);
        info!(
            target_chunks = enqueued,
            worldgen = ?active_wg,
            "voxel: initial chunk coords enqueued (async build)"
        );
        enqueued
    }

    /// W267: the loading-screen core ring radius (see build_world +
    /// the streamer's Loading clamp).
    pub(crate) const LOADING_CORE_RADIUS: i32 = 12;

    /// W231: FULL vertical columns, like MC; W264: PER-DIM spans from
    /// the dimension registry (overworld -4..=19, nether/end 0..=15).
    ///
    /// This used to be a `player_cy +/- 3` band (W28.3, a ~40% chunk
    /// saving tuned for view distance 4 + short fog). With VD 12 and
    /// the MC fog model the band became user-visible brokenness: fly
    /// above y~112 over fresh terrain and NOTHING generates below
    /// (ocean floor lives in cy 2-3), leaving a fog void plus floating
    /// band-edge mountain tops in the distance. MC generates every
    /// section of a column; we now do the same and columns are only
    /// bounded horizontally (view distance + unload ring).
    #[inline]
    pub fn vertical_chunk_range(&self) -> (i32, i32) {
        let def = crate::worldgen::dim_registry::dim_def_or_overworld(&self.current_dim);
        (def.min_chunk_y, def.min_chunk_y + def.vertical_chunks - 1)
    }

    /// W28.7: Return (max_results, budget_ms) tuned for the current app
    /// state. During Loading use higher values so the initial chunk
    /// pipeline drains as fast as GPU uploads allow. During Playing use
    /// conservative values to keep frame time bounded.
    #[inline]
    pub(crate) fn current_results_budget(&self) -> (usize, u64) {
        if matches!(self.state, AppState::Loading { .. }) {
            // W345: behind a loading screen there is no frame to
            // protect - spend big and get the player in fast.
            return (256, 24);
        }
        // W198.17: streamed chunks arrive mesh-complete (the worker does
        // gen+mesh+light in one job), so how fast a freshly-entered area
        // "fills in" is bounded by how many of those meshes we GPU-upload
        // per frame = RESULTS_BUDGET_MS. While the player is essentially
        // stationary there is no movement-hitch risk, so spend a larger
        // budget to drain the backlog fast (the "docitani" pop-in the user
        // asked to tune). A ~12 ms frame standing still is imperceptible
        // (~80 FPS, one-time) and snaps the view in. The instant the player
        // moves we drop back to the conservative 6 ms so uploads never
        // cause a travel stutter.
        let v = self.player_vel;
        let speed_sq = v.x * v.x + v.y * v.y + v.z * v.z;
        // W345: scale the budget with the carried backlog. A deep
        // stream_delivered queue means the workers outran the
        // uploads - the old flat 6 ms cap made the WORLD the
        // laggard ("I outfly the loading"); spending up to 14 ms
        // while a burst is queued clears it in a few frames and
        // still holds ~70 FPS during the burst.
        let carry = self.stream_delivered.len() as u64;
        let boost = if carry > 1024 {
            8
        } else if carry > 512 {
            6
        } else if carry > 128 {
            4
        } else {
            0
        };
        if speed_sq < 4.0 {
            // W353: 12+6 -> 10+4. Standing at the frontier the three
            // catch-up budgets (results, liquid mesh, lighting) STACK
            // in one frame - the R17 "20-30 FPS while generating"
            // was exactly that pile-up.
            (RESULTS_PER_FRAME * 2, 10 + boost.min(4))
        } else {
            (RESULTS_PER_FRAME, RESULTS_BUDGET_MS + boost)
        }
    }

    /// Streaming step: queue missing chunks within `VIEW_DISTANCE`
    /// of the player, unload chunks beyond `UNLOAD_DISTANCE`, then
    /// process up to `RESULTS_PER_FRAME` queued chunks this frame.
    ///
    /// W13 changes:
    /// - W13.1: phase timing via `Instant` + `tracing::warn!` for any
    ///   phase > CHUNK_PHASE_WARN_US microseconds.
    /// - W13.2: neighbour re-mesh is DEFERRED into `pending_remesh`
    ///   instead of executing synchronously; drained REMESH_DEFERRED_PER_FRAME
    ///   per frame so the cost is spread over multiple frames.
    /// - W13.3: side-effects (snow / torch / liquid scans) come from
    ///   `WorkerResult::side_effects` pre-computed off the main thread.
    /// - W13.4: view_distance clamped to VIEW_DISTANCE_MAX (24).
    pub(crate) fn stream_chunks(&mut self) {
        crate::world::stream::stream_chunks(self);
    }


    pub(crate) fn affected_chunks(world_voxel: [i32; 3]) -> Vec<ChunkCoord> {
        // The edited voxel always belongs to one chunk. If it sits on
        // a chunk boundary (local 0 or CHUNK_SIZE-1 on any axis) the
        // neighbour chunk's faces facing it become visible/hidden too,
        // so we rebuild that chunk as well.
        let cs = CHUNK_SIZE as i32;
        let cc = [
            world_voxel[0].div_euclid(cs),
            world_voxel[1].div_euclid(cs),
            world_voxel[2].div_euclid(cs),
        ];
        let lc = [
            world_voxel[0].rem_euclid(cs),
            world_voxel[1].rem_euclid(cs),
            world_voxel[2].rem_euclid(cs),
        ];
        let mut out = vec![cc];
        if lc[0] == 0 {
            out.push([cc[0] - 1, cc[1], cc[2]]);
        }
        if lc[0] == cs - 1 {
            out.push([cc[0] + 1, cc[1], cc[2]]);
        }
        if lc[1] == 0 {
            out.push([cc[0], cc[1] - 1, cc[2]]);
        }
        if lc[1] == cs - 1 {
            out.push([cc[0], cc[1] + 1, cc[2]]);
        }
        if lc[2] == 0 {
            out.push([cc[0], cc[1], cc[2] - 1]);
        }
        if lc[2] == cs - 1 {
            out.push([cc[0], cc[1], cc[2] + 1]);
        }
        out
    }
}

#[cfg(test)]
mod build_world_tests {
    use crate::Voxel;
    use infengine_voxel::CHUNK_SIZE;

    /// W345: the upload budget scales with the carried result
    /// backlog - a deep queue must buy a bigger frame slice so the
    /// world catches up instead of trickling at the flat 6 ms.
    #[test]
    fn w345_results_budget_scales_with_backlog() {
        let mut v = Voxel::new();
        v.state = crate::ui::AppState::Playing;
        v.player_vel = glam::Vec3::new(10.0, 0.0, 0.0);
        let (_, base) = v.current_results_budget();
        for _ in 0..600 {
            v.stream_delivered.push(crate::workers::WorkerResult::LodTile(
                crate::lod::LodTileResult {
                    key: [0, 0],
                    level: 1,
                    dim: String::new(),
                    y_surface: 0.0,
                    y_min: 0.0,
                    y_max: 0.0,
                    opaque: (Vec::new(), Vec::new()),
                    water: (Vec::new(), Vec::new()),
                    water_tint: [0.0; 3],
                },
            ));
        }
        let (_, boosted) = v.current_results_budget();
        assert!(
            boosted >= base + 6,
            "600-deep carry must boost the budget ({base} -> {boosted})",
        );
        // The loading screen spends big unconditionally.
        v.state = crate::ui::AppState::Loading {
            target_chunks: 1,
            loaded_chunks: 0,
            phase: crate::ui::LoadingPhase::GeneratingChunks,
            started_at: std::time::Instant::now(),
        };
        let (_, loading) = v.current_results_budget();
        assert!(loading >= 20, "loading budget {loading}");
    }

    /// W249: the initial ring centres on the PLAYER's chunk (a saved
    /// world's pose is pre-applied by enter_world), and the queue
    /// pops nearest-first. Regression for the "initial build around
    /// the origin gets thrown away after load" report.
    #[test]
    fn initial_ring_centres_on_the_player() {
        let cs = CHUNK_SIZE as f32;
        let mut v = Voxel::new();
        v.settings.view_distance = 3;
        v.player_pos = glam::Vec3::new(296.0 * cs + 8.0, 26.0, 295.0 * cs + 8.0);
        let target = v.build_world();
        assert!(target > 0);
        // The very first pop is the player's own column...
        let first = v.pending_chunks.pop().expect("queue non-empty");
        assert_eq!(
            [first.coord[0], first.coord[2]],
            [296, 295],
            "nearest-first must start at the player column"
        );
        // ...and every queued coord stays inside the ring.
        while let Some(p) = v.pending_chunks.pop() {
            assert!(
                (p.coord[0] - 296).abs() <= 3 && (p.coord[2] - 295).abs() <= 3,
                "coord outside the player ring: {:?}",
                p.coord
            );
        }
    }
}
