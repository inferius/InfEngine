//! W47.28 - world_init impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// generates the initial chunk grid, pins the player on the
    /// surface, and resumes any persisted edits. Idempotent: a
    /// second call returns immediately once `world_loaded` is set,
    /// so the per-entry-point wiring (PlayOffline / NewWorld+enter
    /// / DirectConnect / JoinServer) can call it unconditionally.
    pub(crate) fn enter_world(&mut self) {
        if self.world_loaded {
            return;
        }
        // W128: a dead audio system was previously a silent mystery
        // ("no sounds and no idea why"). Retry the init once on
        // world entry (the output device may have been busy at
        // launch - another instance, exclusive-mode app, ...) and
        // surface the failure in chat if it still won't open.
        if self.audio.is_none() {
            match infengine_audio::AudioSystem::new() {
                Ok(a) => {
                    tracing::info!("audio system recovered on world entry");
                    self.audio = Some(a);
                    // The bank skipped loading while audio was dead.
                    crate::bootstrap::load_mc_sound_bank(self);
                }
                Err(err) => {
                    tracing::warn!(error = %err, "audio init retry failed");
                    self.chat_log.push((
                        std::time::Instant::now(),
                        "[audio] zvuk se nepodarilo otevrit - SFX vypnuto \
                         (zarizeni drzi jina aplikace?)"
                            .to_string(),
                    ));
                }
            }
        }
        // W249: a saved world's meta + host pose must apply BEFORE
        // the initial async build. The build used to run around the
        // ORIGIN with the DEFAULT (Classic) profile: it generated the
        // full view distance of wrong chunks, leaked them into the
        // save through unload-writes, and threw the rest away when
        // load_world finally teleported the player - the "holes
        // around me for minutes after loading" report. With the meta
        // + pose applied here, the build targets the right chunks
        // with the right profile and the disk-first workers stream
        // the SAVED chunks straight in; load_world then overlays
        // containers/entities without resetting.
        if matches!(self.net, NetRole::Server { .. } | NetRole::Offline)
            && let Some(slug) = self.current_world_slug.clone()
        {
            match world_save::apply_world_meta(self, &slug) {
                Ok(pos) => {
                    self.player_pos = Vec3::new(pos[0], pos[1], pos[2]);
                    self.player_vel = Vec3::ZERO;
                    self.fall_start_y = None;
                    info!(
                        slug = %slug,
                        dim = %self.current_dim,
                        seed = self.base_seed,
                        pos = ?pos,
                        "W249: saved-world meta pre-applied before the initial build"
                    );
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    // Fresh slot - no save on disk yet; the defaults
                    // (origin + the menu-picked worldgen) are right.
                }
                Err(err) => {
                    warn!(error = %err, "W249: world meta pre-apply failed");
                }
            }
        }
        // L.2: spin up workers first so `build_world` can push into
        // pending_chunks immediately.
        self.workers = Some(WorkerPool::spawn(pick_worker_count()));
        // L.2: enqueue the initial chunks (returns immediately - no
        // generation happens on the main thread). Actual generation
        // runs via the worker pool as the loading-state tick drains
        // `pending_chunks` through `stream_chunks`.
        let target_chunks = self.build_world();
        // L.2: enter Loading state. The loading tick (on_update)
        // advances loaded_chunks + transitions to Playing once all
        // chunks are in + lighting/mesh passes finish.
        self.state = AppState::Loading {
            target_chunks,
            loaded_chunks: 0,
            phase: LoadingPhase::GeneratingChunks,
            started_at: std::time::Instant::now(),
        };
        // Ensure the menu is out of the way while loading.
        self.menu.apply_switch(menu::MenuScreen::InGame);
        info!(
            slug = ?self.current_world_slug,
            net = %self.net.label(),
            target_chunks,
            "voxel: entering world (async)"
        );
    }

    /// W8.1: explicit world-teardown step. Auto-saves, drops the
    /// worker pool, clears every per-world index, and flips
    /// `world_loaded` back to `false` so the next launch into the
    /// menu shows a cold runtime. Called from `action_main_menu`
    /// and `action_mp_disconnect`. Idempotent (no-op when
    /// `world_loaded` is already false).
    pub(crate) fn teardown_world(&mut self) {
        if !self.world_loaded {
            return;
        }
        // Best-effort save before tearing the world down. Offline +
        // server hosts persist; clients have no authoritative state
        // to save here.
        if matches!(self.net, NetRole::Server { .. } | NetRole::Offline) {
            self.save_server_state();
        }
        // Drop chunk geometry / liquid meshes / mob entities so
        // the menu draws over a clean black screen.
        self.reset_world();
        self.mobs.clear();
        self.remote_mobs.clear();
        self.dropped_items.clear();
        self.lighting = lighting::LightingState::new();
        self.fires.clear();
        self.fire_clock = 0;
        // Stop the worker pool so it doesn't keep streaming chunks
        // in the background. Dropping the handle closes the
        // request channel; the workers exit on the next recv.
        self.workers = None;
        self.world_loaded = false;
        // W249: never carry the pre-applied-meta marker into the
        // next world entry.
        self.preapplied_slug = None;
        info!("voxel: world torn down (returned to menu)");
    }

    pub(crate) fn new_with_net(net: NetRole) -> Self {
        let mut v = Self::new();
        v.net = net;
        v
    }

    /// After a block is broken at `removed`, scan the 5 attaching
    /// faces (above + NSEW) and pop off any `F_NO_PLACE`
    /// decoration that no longer has support. Each dislodged
    /// decoration spawns as a drop entity (pickup), broadcasts a
    /// `vx.edit` packet, and re-enters the queue so cascades
    /// (torch on torch on dirt) all unwind in one call.
    pub(crate) fn dislodge_unsupported_decorations(&mut self, removed: [i32; 3]) {
        decorations::dislodge_unsupported(self, removed);
    }

    /// Set a single voxel in the world + maintain the torch index
    /// so dynamic point lights track placed/broken torches. Every
    /// edit-apply path funnels through here so we only have one
    /// place that knows about torch bookkeeping.
    pub(crate) fn world_set_tracked(&mut self, voxel: [i32; 3], block: BlockId) {
        // W28.5: record old emission before the world is mutated.
        let old_block = self.world.get(voxel);
        let old_emission = block::light_emission::light_emission_for_id(old_block.0);
        let new_emission = block::light_emission::light_emission_for_id(block.0);
        self.world.set(voxel, block);
        // W32 fix: force lighting tick to fire on the next frame instead
        // of waiting up to 100 ms for the 10 Hz throttle. Without this,
        // breaking a block emits the first Remesh job with the OLD halo
        // lighting (chunk still dark from when it was enclosed). The
        // newly meshed surfaces then look dark/black for a few frames
        // until the lighting BFS catches up and triggers a second remesh.
        // Resetting the accumulator over the period means the very next
        // on_update tick dispatches Job::Lighting alongside the Remesh.
        self.last_lighting_tick_s = LIGHTING_TICK_PERIOD_S;
        // W38.2: any open-flame block gets a particle emitter entry.
        // Mirrors MC's flame/smoke spawn list. Glowstone emits light
        // but is a solid block with no open flame, so it stays in
        // emitter_positions only (point light, no particles).
        if flame_emitter(block.0) {
            self.torch_positions.insert(voxel);
        } else {
            self.torch_positions.remove(&voxel);
        }
        // W12.5: generic MC-emitter index for the halo picker. Covers
        // every block id with `light_emission_for_id > 0` (torches,
        // soul torches, sea_lantern, jack_o_lantern, end_rod, lava,
        // fire, glowstone, shroomlight, lit_furnace, ...).
        if block::light_emission::light_emission_for_id(block.0) > 0 {
            self.emitter_positions.insert(voxel, block.0);
        } else {
            self.emitter_positions.remove(&voxel);
        }
        // Snow layer voxels render as a thin slab in the world
        // pass; tracked separately so the renderer can skip
        // the full-cube path. Inserts default to 1 layer; the
        // accumulator path bumps the value when more snow lands
        // on the same voxel.
        if block.0 == 15 {
            self.snow_positions.entry(voxel).or_insert(1);
        } else {
            self.snow_positions.remove(&voxel);
        }
        // W5.2a: portal-block index. Tracked so the animated swirl
        // render pass doesn't have to scan every chunk for
        // `PORTAL_ID` each frame.
        if block.0 == block::PORTAL_ID {
            self.portal_positions.insert(voxel);
        } else {
            self.portal_positions.remove(&voxel);
        }
        // W242.2: monster-spawner index for the BaseSpawner tick.
        // Breaking the cage also drops its runtime delay state.
        if block.0 == block::SPAWNER_ID {
            self.spawner_positions.insert(voxel);
        } else {
            self.spawner_positions.remove(&voxel);
            self.spawner_states.remove(&voxel);
        }
        // Torch removal also clears the sticky orientation
        // override so a future placement starts fresh.
        if block.0 != TORCH_ID {
            self.torch_orient.remove(&voxel);
        }
        // W65.3: a cell that stops being a leaf can no longer be a
        // persistent (player-placed) leaf - drop its decay exemption
        // so a future tree-grown leaf in the same spot can decay.
        if block.0 != block::LEAVES_ID {
            self.persistent_leaves.remove(&voxel);
        }
        // W67.1: a cell that stops being a lever can't stay a powered
        // lever - drop its on-state so a future lever there starts off.
        if block.0 != block::LEVER_ID {
            self.lever_on.remove(&voxel);
        }
        // Liquid bookkeeping: any registered liquid (water,
        // lava, ...) re-enters the active queue + position set
        // when it's written. Level defaults to 0 (source) for
        // direct writes; tick passes overwrite when they
        // propagate flow from a source. Non-liquid writes purge
        // the entry so a broken liquid cell doesn't leak into
        // the tracker.
        if let Some(liquid) = liquid_for(block) {
            // MC-style: the freshly placed cell ticks after
            // `tick_period` frames - matches the legacy cadence
            // the planner enforced via `nt % tick_period`.
            let when = self.liquid_clock + liquid.tick_period.max(1) as u64;
            self.liquid_schedule.schedule(voxel, when);
            self.liquid_level.entry(voxel).or_insert(0);
            if self.liquid_positions.insert(voxel) {
                self.liquid_mesh_dirty = true;
            }
        } else if self.liquid_positions.remove(&voxel) {
            self.liquid_level.remove(&voxel);
            // The cell is no longer liquid - drop its slot in
            // the schedule. The matching heap entry becomes a
            // tombstone, popped + skipped on the next sweep.
            self.liquid_schedule.cancel(voxel);
            self.liquid_mesh_dirty = true;
        } else {
            self.liquid_level.remove(&voxel);
        }
        // Any world edit can change a neighbour's air / solid /
        // water status, which feeds into both the corner-height
        // blend and the liquid simulation - a settled pool cell
        // needs to wake up the moment the player digs out a hole
        // anywhere within flow range.
        //
        // Scan a 3x3x3 cube around the touched voxel and pull
        // adjacent liquid cells back into the schedule so
        // the next tick re-evaluates their flow. The wider 5x5x5
        // sweep was overhead-heavy on big pools; the tick's
        // own NSEW propagation handles cells past the 3x3 ring.
        let mut neighbour_liquid = false;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let n = [
                        voxel[0] + dx,
                        voxel[1] + dy,
                        voxel[2] + dz,
                    ];
                    if self.liquid_positions.contains(&n) {
                        neighbour_liquid = true;
                        // MC-style: re-schedule each adjacent
                        // liquid cell at a short delay so the
                        // spread can react to the new topology
                        // (block placed in front of a stream,
                        // pool floor dug out). MC uses
                        // 1..tickDelay; we use a 1-tick wake so
                        // the response shows up next frame.
                        self.liquid_schedule
                            .schedule(n, self.liquid_clock + 1);
                    }
                }
            }
        }
        if neighbour_liquid {
            self.liquid_mesh_dirty = true;
            // The 1-tick neighbour wake above already gets us
            // the next-frame response - no separate force flag
            // needed in the per-cell schedule model.
        }
        // Phase L.6: every block edit potentially changes the
        // local light field - whether the new block is an
        // emitter (torch/glowstone/lava/fire), an opaque blocker
        // (carving out / sealing up sky columns), or just a
        // transparent swap (water replacing leaves). W28.5: record
        // emission change so tick_lighting can use incremental path.
        self.lighting.mark_dirty_with_change(voxel, old_emission, new_emission);
        // Wave 7: fire on_block_place / on_block_break for
        // every loaded plugin. Air (block.0 == 0) is treated
        // as a break; everything else is a place. player_id
        // 0 means "engine-driven" - we don't have a clean
        // player attribution here (this fn is the common funnel
        // for player edits, liquid spread, worldgen settle...).
        // Future work: thread an Option<u32> player_id down
        // through the call sites that originate from a player
        // hit.
        if block.0 == 0 {
            plugin::events::dispatch_block_break(
                self,
                voxel[0],
                voxel[1],
                voxel[2],
                0,
                0,
            );
        } else {
            plugin::events::dispatch_block_place(
                self,
                voxel[0],
                voxel[1],
                voxel[2],
                block.0,
                0,
            );
        }
    }
}
