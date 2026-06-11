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
                for (_id, h) in kinds {
                    renderer.remove_mesh(h);
                }
            }
        }
        for cc in coords {
            self.world.remove_chunk(cc);
        }
        self.pending_chunks.clear();
        self.pending_set.clear();
        self.pending_remesh.clear();
        self.pending_remesh_set.clear();
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
        self.snow_positions.clear();
        self.torch_positions.clear();
        self.emitter_positions.clear();
        // W5.2a: portal tracker rebuilds via world_set_tracked
        // when chunks reload, so clear here too.
        self.portal_positions.clear();
        self.torch_orient.clear();
        self.liquid_schedule.clear();
        self.liquid_clock = 0;
        self.liquid_tick_accum = 0.0;
        self.liquid_chunk_sig.clear();
        self.liquid_level.clear();
        self.liquid_positions.clear();
        self.falling_pending.clear();
        self.persistent_leaves.clear();
        self.lever_on.clear();
        self.liquid_mesh_dirty = true;
        // W125: door state is per-world; stale entries from the
        // previous world must not leak open/facing flags in.
        self.door_open.clear();
        self.door_facing.clear();
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
        // W50.5: brewing stand maps cleared on world reset.
        self.brewing_stands.clear();
        self.brewing_runtimes.clear();
        self.brewing_open = None;
        self.brewing_held = None;
        // W134: carts are per-world (persisted since W137).
        self.carts.clear();
        self.remote_carts.clear();
        self.riding_cart = None;
        self.cart_input = 0.0;
        self.cart_dismount_pending = false;
        self.cart_net_accum = 0.0;
        // W140: arrows are ephemeral - drop them with the world.
        self.arrows.clear();
        self.remote_arrows.clear();
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
        self.pending_remesh.clear();
        self.pending_remesh_set.clear();
        self.stream_delivered.clear(); // W106: see clear_world_state note.
        // W27.1: use the runtime view_distance so target_chunks matches the
        // live count that stream_chunks will also use. Previously INITIAL_CHUNKS
        // (=3) was used here but stream_chunks uses view_distance (default 4),
        // causing the counter to show e.g. 1296/784 (100%) because the target
        // was 7x7x16=784 but the live count rose to 9x9x16=1296.
        let radius = self.settings.view_distance.clamp(2, VIEW_DISTANCE_MAX as u8) as i32;
        // W28.1: player spawns at origin so player_chunk = [0, 0, 0].
        // Compute XZ squared distance from origin for priority ordering.
        // W28.3: only enqueue Y slices visible from the player's current Y.
        let (cy_min, cy_max) = Self::vertical_chunk_range(self.player_pos.y);
        let mut enqueued: u32 = 0;
        for cz in -radius..=radius {
            for cx in -radius..=radius {
                let dist_sq = (cx * cx + cz * cz) as i64;
                for cy in cy_min..=cy_max {
                    let cc: ChunkCoord = [cx, cy, cz];
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

    /// W28.3: Compute the vertical chunk index range (inclusive) that
    /// overlaps the player's visible area. Returns `(cy_min, cy_max)`
    /// clamped to `[0, VERTICAL_CHUNKS - 1]`. The +/-3 heuristic covers
    /// ~96 blocks vertically - more than the fov cone at view_distance=4.
    /// Using this instead of `0..VERTICAL_CHUNKS` cuts ~40% of chunks for
    /// above-ground players.
    #[inline]
    pub fn vertical_chunk_range(player_y: f32) -> (i32, i32) {
        let cs = CHUNK_SIZE as i32;
        let player_cy = (player_y.floor() as i32).div_euclid(cs);
        let cy_min = (player_cy - 3).max(0);
        let cy_max = (player_cy + 3).min(VERTICAL_CHUNKS - 1);
        (cy_min, cy_max)
    }

    /// W28.7: Return (max_results, budget_ms) tuned for the current app
    /// state. During Loading use higher values so the initial chunk
    /// pipeline drains as fast as GPU uploads allow. During Playing use
    /// conservative values to keep frame time bounded.
    #[inline]
    pub(crate) fn current_results_budget(&self) -> (usize, u64) {
        if matches!(self.state, AppState::Loading { .. }) {
            (32, 8)
        } else {
            (RESULTS_PER_FRAME, RESULTS_BUDGET_MS)
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
