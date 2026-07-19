//! W47.28 - dim_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {

    /// F.2d: build a fresh `WorldgenCtx` snapshot from the live
    /// per-dim runtime state. Captured on the main thread, then
    /// shipped to the worker pool so the generator runs with the
    /// EXACT base seed + dim + profile the player saw at enqueue
    /// time. A later `switch_dim` between enqueue + pickup no
    /// longer poisons the gen.
    pub(crate) fn current_worldgen_ctx(&self) -> worldgen::WorldgenCtx {
        // W264: profile resolution order - the world's explicit
        // per-dim table, then the dimension registry's default
        // (nether/end rows), then Classic (unknown custom dims).
        let profile = self
            .dim_worldgens
            .get(&self.current_dim)
            .copied()
            .or_else(|| {
                worldgen::dim_registry::dim_def(&self.current_dim)
                    .and_then(|d| d.default_profile)
            })
            .unwrap_or(WorldGen::Classic);
        worldgen::WorldgenCtx {
            base_seed: self.base_seed,
            dim: self.current_dim.clone(),
            profile,
        }
    }

    /// Phase F: swap the active dim to `target`. Parks the
    /// current dim's voxel + entity state in the `_by_dim` maps
    /// and pulls the target dim's state back out (minting empty
    /// if missing).
    ///
    /// F.2e: NO LONGER writes the global `WORLD_SEED` /
    /// `WORLDGEN` atomics. Chunk gen now goes through
    /// `current_worldgen_ctx()` -> `generate_chunk_at_ctx()` so
    /// the per-dim seed + profile are passed by value into every
    /// worker job, race-free against any other thread or future
    /// dim swap.
    ///
    /// Idempotent if `target == current_dim`. The player's pose
    /// and inventory are untouched - they carry across dims.
    /// Mesh handles are NOT rebuilt here; the caller is
    /// responsible for `rebuild_all_meshes` (or letting the
    /// streaming code repopulate on the next frame).
    #[allow(dead_code)]
    pub(crate) fn switch_dim(&mut self, target: &str) {
        if target == self.current_dim {
            return;
        }
        // Park the current dim's state in the parking maps.
        // Move-semantics on `std::mem::take` zeroes the live
        // fields; we'll fill them back in from the target dim.
        let parked = dim::DimWorld {
            world: std::mem::replace(&mut self.world, World::new()),
            chests: std::mem::take(&mut self.chests),
            furnaces: std::mem::take(&mut self.furnaces),
            furnace_runtimes: std::mem::take(&mut self.furnace_runtimes),
            torch_positions: std::mem::take(&mut self.torch_positions),
            emitter_positions: std::mem::take(&mut self.emitter_positions),
            structure_mob_anchors: std::mem::take(&mut self.structure_mob_anchors),
            portal_positions: std::mem::take(&mut self.portal_positions),
            torch_orient: std::mem::take(&mut self.torch_orient),
            stair_states: std::mem::take(&mut self.stair_states),
            snow_positions: std::mem::take(&mut self.snow_positions),
            fires: std::mem::take(&mut self.fires),
            liquid_schedule: std::mem::replace(
                &mut self.liquid_schedule,
                LiquidSchedule::new(),
            ),
            liquid_clock: std::mem::take(&mut self.liquid_clock),
            liquid_level: std::mem::take(&mut self.liquid_level),
            liquid_positions: std::mem::take(&mut self.liquid_positions),
            // W138: block-state maps park with their dim (doors used
            // to reload facing-0 + closed after a switch).
            door_facing: std::mem::take(&mut self.door_facing),
            door_open: std::mem::take(&mut self.door_open),
            chest_facing: std::mem::take(&mut self.chest_facing),
            brewing_stands: std::mem::take(&mut self.brewing_stands),
            brewing_runtimes: std::mem::take(&mut self.brewing_runtimes),
            lever_on: std::mem::take(&mut self.lever_on),
            rail_shapes: std::mem::take(&mut self.rail_shapes),
            // W242.2: spawner index + tick state park with their dim.
            spawner_positions: std::mem::take(&mut self.spawner_positions),
            spawner_states: std::mem::take(&mut self.spawner_states),
            spawner_kinds: std::mem::take(&mut self.spawner_kinds),
        };
        self.worlds.insert(self.current_dim.clone(), parked);
        // W137: a rider can't follow their cart into the parking
        // map - hop off before the cart parks.
        if self.riding_cart.is_some() {
            crate::minecart::dismount(self);
        }
        let parked_ents = dim::DimEntities {
            mobs: std::mem::take(&mut self.mobs),
            drops: std::mem::take(&mut self.dropped_items),
            carts: std::mem::take(&mut self.carts),
            next_mob_id: self.next_mob_id,
            next_drop_id: self.next_drop_id,
            next_cart_id: self.next_cart_id,
        };
        self.entities_by_dim
            .insert(self.current_dim.clone(), parked_ents);
        // Remote snapshots are per-dim too.
        self.remote_carts.clear();
        // W155: EVERY per-dim render snapshot + visual cache must
        // reset with the world. `remote_mobs` is the render source
        // for all mobs (the host mirrors its own sim into it), so
        // leaving it populated kept painting the old dim's animals
        // into the nether sky (the user's ghost-mobs screenshot).
        self.remote_mobs.clear();
        self.mob_anim.clear();
        self.mob_hurt_flash.clear();
        self.mob_dying.clear();
        self.mob_lunge.clear();
        self.arrows.clear();
        self.remote_arrows.clear();
        self.bubbles.clear();

        // Pull the target dim's state out (mint empty if first
        // visit this session).
        let target_world = self
            .worlds
            .remove(target)
            .unwrap_or_default();
        self.world = target_world.world;
        self.chests = target_world.chests;
        self.furnaces = target_world.furnaces;
        self.furnace_runtimes = target_world.furnace_runtimes;
        self.torch_positions = target_world.torch_positions;
        self.emitter_positions = target_world.emitter_positions;
        self.structure_mob_anchors = target_world.structure_mob_anchors;
        self.portal_positions = target_world.portal_positions;
        self.torch_orient = target_world.torch_orient;
        self.stair_states = target_world.stair_states;
        self.snow_positions = target_world.snow_positions;
        self.fires = target_world.fires;
        self.liquid_schedule = target_world.liquid_schedule;
        self.liquid_clock = target_world.liquid_clock;
        self.liquid_level = target_world.liquid_level;
        self.liquid_positions = target_world.liquid_positions;
        self.door_facing = target_world.door_facing;
        self.door_open = target_world.door_open;
        self.chest_facing = target_world.chest_facing;
        self.brewing_stands = target_world.brewing_stands;
        self.brewing_runtimes = target_world.brewing_runtimes;
        self.lever_on = target_world.lever_on;
        self.rail_shapes = target_world.rail_shapes;
        self.spawner_positions = target_world.spawner_positions;
        self.spawner_states = target_world.spawner_states;
        self.spawner_kinds = target_world.spawner_kinds;
        let target_ents = self
            .entities_by_dim
            .remove(target)
            .unwrap_or_else(dim::DimEntities::empty);
        self.mobs = target_ents.mobs;
        self.dropped_items = target_ents.drops;
        self.carts = target_ents.carts;
        self.next_mob_id = target_ents.next_mob_id;
        self.next_drop_id = target_ents.next_drop_id;
        self.next_cart_id = target_ents.next_cart_id.max(1);

        // F.2e: switch_dim no longer touches the worldgen
        // atomics. Chunk gen reads the per-dim ctx via
        // `current_worldgen_ctx()`, which derives directly from
        // `current_dim` + `base_seed` + `dim_worldgens[target]`.
        // The atomics survive only as a "last-used" cache for UI
        // (HUD biome label, /biome chat command, the picker
        // dropdown); leaving them alone here means a worker job
        // already in-flight with a captured ctx finishes against
        // the dim it was enqueued against, not the one we just
        // swapped to.
        self.current_dim = target.to_string();
        // Drop renderer mesh handles for the chunks we just
        // parked - the active dim's chunk list completely
        // changed.
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
            self.liquid_chunk_sig.clear();
            self.liquid_remesh_backlog.clear();
            // W284: water tint is biome-derived = per-dim.
            self.water_tint_cache.clear();
        }
        // Lighting is per-dim + not persisted - drop every grid
        // on swap. We then re-derive from the target dim's
        // freshly-swapped block grid so the next frame's mesh
        // build already has correct light values.
        self.lighting.clear();
        self.rebuild_all_lighting();
        // Force the streaming pipeline to forget what was
        // pending for the previous dim - those coords mean
        // something different now.
        self.pending_chunks.clear();
        self.pending_set.clear();
        self.in_flight_chunks.clear();
        self.pending_remesh.clear();
        self.pending_remesh_set.clear();
        // W277: results already PULLED off the channel but not yet
        // processed belong to the old dim too (in-flight ones are
        // caught by the per-result dim tag on the drain).
        //
        // W284: a Lit result dropped HERE must still give its
        // in-flight slot back. The drain's stale-dim arm decrements
        // `pending_lighting_jobs`; this clear() didn't, so every
        // portal cycle could ratchet the counter up until it hit
        // MAX_INFLIGHT_LIGHTING_JOBS (24) and lighting dispatch
        // wedged for the rest of the session (the user's pinned
        // `light_jobs=24` + `light_ms=0.0` log signature).
        for r in &self.stream_delivered {
            if matches!(r, crate::workers::WorkerResult::Lit { .. }) {
                self.pending_lighting_jobs = self.pending_lighting_jobs.saturating_sub(1);
            }
        }
        self.stream_delivered.clear();
        // W284: queued-but-undispatched lighting segments hold OLD
        // dim chunk coords - snapshotting them against the swapped
        // world would light garbage. rebuild_all_lighting above
        // already re-seeds the new dim.
        self.pending_lighting_segments.clear();
        // Open UIs reference position-keyed containers that
        // belong to the parked dim. Close them on switch so the
        // player doesn't see a stale chest panel.
        self.chest_open = None;
        self.chest_held = None;
        self.furnace_open = None;
        self.furnace_held = None;
        self.liquid_mesh_dirty = true;
        // W159: THE "empty dim on return" FIX. The target dim's
        // chunks are restored INTO `self.world` above, but their
        // meshes were just dropped - and `stream_chunks` only ever
        // queues chunks MISSING from the world, so present-but-
        // unmeshed chunks render as void forever (the user's empty
        // overworld after a nether round-trip + the floating
        // inactive portal). Re-mesh every restored chunk once here.
        if self.renderer.is_some() {
            let restored: Vec<ChunkCoord> =
                self.world.iter_chunks().map(|(c, _)| *c).collect();
            if !restored.is_empty() {
                self.rebuild_chunks(&restored);
            }
        }
    }

    /// Phase F.6: increment / reset the in-portal counter +
    /// fire a cross-dim teleport when the threshold is reached.
    /// Called once per frame from the main tick loop.
    ///
    /// The check uses the player's foot voxel (rounded floor of
    /// player_pos.x/z; player_pos.y - PLAYER_HEIGHT/2). When
    /// the foot voxel is a `PORTAL_ID` and we're not on
    /// cool-down, increment; on miss, reset to 0.
    pub(crate) fn tick_player_portal(&mut self, dt: f32) {
        // W277: advance at the FIXED 20 Hz game-tick rate. This used
        // to run once per FRAME, so at 200 FPS the 80-tick warmup was
        // 0.4 s and the 60-tick cooldown 0.3 s - the player was still
        // standing in the arrival portal when the cooldown expired
        // and instantly bounced back (the ping-pong report).
        const PORTAL_TICK_DT: f32 = 1.0 / 20.0;
        self.portal_tick_accum += dt;
        if self.portal_tick_accum < PORTAL_TICK_DT {
            return;
        }
        // One logical tick per rollover; a stall doesn't burst.
        self.portal_tick_accum =
            (self.portal_tick_accum - PORTAL_TICK_DT).min(PORTAL_TICK_DT);
        // Foot voxel: the cell the player's feet sit in. Round
        // X/Z to floor; Y is the integer floor of the player's
        // base.
        let foot = [
            self.player_pos.x.floor() as i32,
            (self.player_pos.y - PLAYER_HEIGHT * 0.5).floor() as i32,
            self.player_pos.z.floor() as i32,
        ];
        let in_portal = self.world.get(foot).0 == block::PORTAL_ID
            || self.world.get([foot[0], foot[1] + 1, foot[2]]).0
                == block::PORTAL_ID;
        if self.portal_cooldown_ticks > 0 {
            // W277: MC Entity.setAsInsidePortal - standing inside a
            // portal WHILE on cooldown re-arms the FULL cooldown, so
            // the return trip can never fire until the player steps
            // out of the frame at least once.
            if in_portal {
                self.portal_cooldown_ticks = block::portal::PORTAL_COOLDOWN_TICKS;
            } else {
                self.portal_cooldown_ticks -= 1;
            }
            self.portal_in_ticks = 0;
            return;
        }
        if !in_portal {
            self.portal_in_ticks = 0;
            return;
        }
        // W156: the MC trigger hum starts the moment the player
        // steps in and loops while they stand there.
        if self.portal_in_ticks == 0 {
            let p = [
                self.player_pos.x,
                self.player_pos.y,
                self.player_pos.z,
            ];
            self.sfx_at("portal/trigger", p, |_, _| {});
        }
        self.portal_in_ticks = self.portal_in_ticks.saturating_add(1);
        if self.portal_in_ticks >= block::portal::PORTAL_TRIGGER_TICKS {
            self.do_cross_dim_teleport(foot);
        }
    }

    /// Phase F.6: fire a cross-dim teleport. Compute the
    /// destination coord (scaled by 8), switch_dim into the
    /// paired dim, find / build a return portal, place the
    /// player adjacent, and set the cool-down so they don't
    /// instantly bounce back.
    pub(crate) fn do_cross_dim_teleport(&mut self, from_pos: [i32; 3]) {
        let target_dim = block::portal::pair_dim(&self.current_dim);
        let dest = block::portal::pair_coord(&self.current_dim, from_pos);
        // W155: the nether dim must generate with the NETHER
        // profile - a missing registration silently fell back to
        // Classic (overworld terrain inside the nether).
        if target_dim == dim::NETHER {
            self.dim_worldgens
                .entry(dim::NETHER.to_string())
                .or_insert(crate::worldgen::WorldGen::Nether);
        }
        // Swap the active dim. switch_dim does the heavy lifting
        // (move chunks/entities to the parking maps + load the
        // target dim's state).
        self.switch_dim(target_dim);
        // W311: MC portal LINKING first - find the nearest existing
        // portal within the target dim's POI radius (128 overworld,
        // 16 nether) in the persistent swirl index. The old path only
        // probed a 16-block cube around the scaled dest, so a round
        // trip often built a SECOND portal far from the original
        // instead of linking back to it (the user's "came out
        // somewhere completely different" report).
        let poi_r: i64 = if target_dim == dim::NETHER { 16 } else { 128 };
        let linked: Option<[i32; 3]> = self
            .portal_positions
            .iter()
            .map(|p| {
                let dx = (p[0] - dest[0]) as i64;
                let dz = (p[2] - dest[2]) as i64;
                (*p, dx * dx + dz * dz)
            })
            .filter(|(_, d2)| *d2 <= poi_r * poi_r)
            .min_by_key(|(_, d2)| *d2)
            .map(|(p, _)| p);
        // W155: generate REAL terrain at the arrival column before
        // any portal hunt. The old path inserted EMPTY chunks
        // (void), clamped the frame to y<=100 mid-air and the
        // player fell out of the sky; the empty shells also
        // permanently shadowed worldgen for those coords.
        let arrival_region =
            self.ensure_generated_chunks_around(linked.unwrap_or(dest));
        // Ground-snap the portal: first open spot with a solid
        // floor, scanning down from the dest height band. W282:
        // searches a growing ring of columns (up to 8 blocks out)
        // like MC's candidate scan - a single-column miss used to
        // drop to the mid-shelf fallback and hang the frame in air.
        let cfg = bincode::config::standard();
        let mut edits: Vec<[i32; 3]> = Vec::new();
        let frame_hint;
        let spawn = if let Some(s) = linked {
            // W311: linked to an EXISTING portal - arrive inside it
            // like MC; nothing to build, no platform stamping.
            frame_hint = s;
            s
        } else {
        let mut target = Self::portal_ground_spot(&self.world, dest);
        // W298: sink the frame onto the REAL floor when the picked
        // spot hovers a few blocks above it - the netherrack platform
        // used to float over visible ground (MC only builds the
        // platform when there is truly nothing below).
        {
            let mut drop = 0;
            // W306: 12 -> 24 - tall nether caverns left the frame
            // hovering with 13+ blocks of air under it.
            while drop < 24
                && target[1] > crate::WORLD_MIN_Y + 2
                && self
                    .world
                    .get([target[0], target[1] - 1, target[2]])
                    .is_air()
            {
                target[1] -= 1;
                drop += 1;
            }
        }
        // Find or build a return portal at the scaled coord.
        frame_hint = target;
        block::portal::ensure_return_portal(
            &mut self.world,
            target,
            Some(&mut edits),
        )
        };
        // W159: GUARANTEE a solid platform + clear pocket at the
        // arrival, then re-mesh it. The surface scan can miss in
        // lava-sea / all-air columns, which is how the portal ended
        // up floating and the player fell. A dim-appropriate floor
        // under the frame footprint + a cleared 3-tall pocket makes
        // every arrival stand on solid ground like MC's platform.
        if linked.is_none() {
        let base_y = spawn[1];
        if target_dim == dim::NETHER {
            // MC builds a netherrack platform + cleared pocket in the
            // NETHER (the arrival is over lava-sea / void). Overworld
            // does NOT - see the else branch.
            for dx in -1..=2i32 {
                for dz in -1..=2i32 {
                    let fp = [spawn[0] + dx, base_y - 1, spawn[2] + dz];
                    if self.world.get(fp).is_air() {
                        self.world_set_tracked(fp, BlockId(block::NETHERRACK_ID));
                        edits.push(fp);
                    }
                    // Clear a 3-tall standing pocket above the floor
                    // (skip the frame's own obsidian + portal cells).
                    for dy in 0..3 {
                        let cp = [spawn[0] + dx, base_y + dy, spawn[2] + dz];
                        let here = self.world.get(cp).0;
                        let is_frame = here == block::OBSIDIAN_ID
                            || here == block::PORTAL_ID;
                        if !is_frame && !self.world.get(cp).is_air() {
                            self.world_set_tracked(cp, BlockId::AIR);
                            edits.push(cp);
                        }
                    }
                }
            }
        } else {
            // W306: forced-platform case - when the ground under the
            // frame is NOT solid (ocean surface / air), build a small
            // obsidian platform like MC's forced portal. Terrain
            // arrivals keep the W179 no-platform behaviour.
            let under = self.world.get([spawn[0], base_y - 1, spawn[2]]);
            if !crate::block::block_fully_occludes(under) {
                for dx in -1..=2i32 {
                    for dz in -1..=2i32 {
                        let fp = [spawn[0] + dx, base_y - 1, spawn[2] + dz];
                        if !crate::block::block_fully_occludes(self.world.get(fp)) {
                            self.world_set_tracked(
                                fp,
                                BlockId(block::OBSIDIAN_ID),
                            );
                            edits.push(fp);
                        }
                    }
                }
            }
            // W179: OVERWORLD return - NO stone platform on real
            // terrain. Just clear the player's own 1x1x3 standing
            // column if it's buried so they don't suffocate; the W173
            // embedded-safety-net lifts them out if the spot is still
            // solid.
            for dy in 0..3 {
                let cp = [spawn[0], base_y + dy, spawn[2]];
                let here = self.world.get(cp).0;
                let is_frame = here == block::OBSIDIAN_ID || here == block::PORTAL_ID;
                if !is_frame && !self.world.get(cp).is_air() {
                    self.world_set_tracked(cp, BlockId::AIR);
                    edits.push(cp);
                }
            }
        }
        }
        // W299: index the arrival curtain into the swirl render list.
        // `build_minimal_portal` writes via `world.set` (untracked) and
        // the sync gen path never emitter-scanned, so both a freshly
        // BUILT and a freshly FOUND return portal rendered dead - the
        // "portal arrived inactive" report, three times over.
        for p in block::portal::connected_portal_cells(&self.world, spawn) {
            self.portal_positions.insert(p);
        }
        // W178: re-mesh the WHOLE arrival region, not just the portal
        // frame's chunks. switch_dim dropped the meshes of restored
        // chunks and the streamer only meshes chunks MISSING from the
        // world - so the present-but-unmeshed arrival chunks rendered
        // as a void (the floating portal + the chunk hole below/above
        // it, and the "inactive" portal whose purple cells just weren't
        // meshed). Rebuild every region chunk + the ground-snapped
        // frame's chunks (W156: they can sit chunks apart in Y).
        if self.renderer.is_some() {
            // W301: only the chunks the player can immediately see -
            // the portal frame + spawn columns - mesh SYNCHRONOUSLY;
            // the rest of the 3x3-column arrival region drains through
            // the budgeted pending_remesh queue over the next frames.
            // The full sync rebuild of ~74 chunks was the 3.1 s
            // dim-switch stall in the user log (the purple cooldown
            // vignette covers the brief pop-in, like MC's own
            // progressive terrain load after a dim change).
            let mut hot: Vec<infengine_voxel::ChunkCoord> = Vec::new();
            for c in Self::affected_chunks(spawn)
                .into_iter()
                .chain(Self::affected_chunks(frame_hint))
            {
                if !hot.contains(&c) {
                    hot.push(c);
                }
            }
            self.rebuild_chunks(&hot);
            for c in arrival_region {
                if !hot.contains(&c)
                    && self.world.chunk(c).is_some()
                    && self.pending_remesh_set.insert(c)
                {
                    self.pending_remesh.push_back(c);
                }
            }
        }
        // Broadcast portal edits so remote peers see the new frame
        // appear.
        if !edits.is_empty() {
            // W159: broadcast the ACTUAL block at each edited cell -
            // the edit set now includes the platform floor + cleared
            // air pocket, not just the portal/obsidian frame, so the
            // old "portal-or-obsidian-else-0" filter would have wiped
            // the floor to air on remote peers.
            let edits_to_send: Vec<EditMsg> = edits
                .iter()
                .map(|p| EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: self.world.get(*p).0,
                })
                .collect();
            match &mut self.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    for e in &edits_to_send {
                        self.edit_history.push(*e);
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(*e, cfg)
                        {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                }
                NetRole::Client { pending_edits, .. } => {
                    for e in &edits_to_send {
                        pending_edits.push(*e);
                    }
                }
            }
        }
        // Park the player at the return portal.
        // W173 FIX: player_pos.y is the EYE, PLAYER_HEIGHT (1.7) above
        // the feet. The arrival pocket clears spawn[1]..spawn[1]+2 so
        // the feet stand at spawn[1] on the platform floor (spawn[1]-1).
        // The old code set the EYE to spawn[1], which buried the feet
        // ~1.7 blocks BELOW the floor - the player spawned inside the
        // ground and got stuck on BOTH portal directions (the bug the
        // user hit). The eye must sit PLAYER_HEIGHT above the pocket.
        self.player_pos = glam::Vec3::new(
            spawn[0] as f32 + 0.5,
            spawn[1] as f32 + PLAYER_HEIGHT,
            spawn[2] as f32 + 0.5,
        );
        // Safety net: if the capsule is still embedded (an odd column
        // the pocket build missed), lift it out instead of trapping
        // the player.
        let mut guard = 0;
        while self.capsule_embedded(self.player_pos) && guard < 6 {
            self.player_pos.y += 1.0;
            guard += 1;
        }
        self.player_vel = glam::Vec3::ZERO;
        self.portal_in_ticks = 0;
        self.portal_cooldown_ticks = block::portal::PORTAL_COOLDOWN_TICKS;
        // W156: MC plays the travel whoosh on arrival.
        let p = [
            self.player_pos.x,
            self.player_pos.y,
            self.player_pos.z,
        ];
        self.sfx_at("portal/travel", p, |_, _| {});
        self.chat_log.push((
            std::time::Instant::now(),
            format!("-> dim '{}'", target_dim),
        ));
    }

    /// W155: synchronously generate the chunk column stack around
    /// `pos` (3x3 columns, the full vertical range) with the ACTIVE
    /// dim's worldgen so a cross-dim arrival has real terrain.
    /// Already-present chunks (visited before) stay untouched.
    ///
    /// W156: the streamer's worker path runs a side-effect scan on
    /// every generated chunk (liquids / snow / flame emitters); this
    /// synchronous path MUST do the same or the arrival zone's lava
    /// sea is invisible (unregistered cells render nothing, while
    /// the streamed chunks around it do - the user's lava pillars).
    /// W178: returns EVERY chunk coord in the 3x3 x vertical arrival
    /// region (generated or pre-existing) so the caller can re-mesh
    /// them all - the cross-dim arrival otherwise left present-but-
    /// unmeshed chunks rendering as a void (the floating-portal +
    /// chunk-hole the user saw).
    pub(crate) fn ensure_generated_chunks_around(
        &mut self,
        pos: [i32; 3],
    ) -> Vec<infengine_voxel::ChunkCoord> {
        let ctx = self.current_worldgen_ctx();
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let ccx = pos[0].div_euclid(cs);
        let ccz = pos[2].div_euclid(cs);
        // Full vertical stack: the nether spans 0..=127 (chunks
        // 0..=7); generating the whole column keeps the ground
        // scan + ceiling consistent whatever Y the player arrives.
        let vertical = crate::VERTICAL_CHUNKS;
        let mut any_liquid = false;
        let mut region: Vec<infengine_voxel::ChunkCoord> = Vec::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                for cy in 0..vertical.min(8) {
                    let c = [ccx + dx, cy, ccz + dz];
                    region.push(c);
                    if self.world.chunk(c).is_some() {
                        continue;
                    }
                    let chunk = crate::worldgen::generate_chunk_at_ctx(c, &ctx);
                    // Same per-cell side effects the worker scan
                    // applies (W115 rule: schedule only cells with
                    // air directly below; the resting sea is
                    // render-only).
                    let origin = infengine_voxel::World::chunk_world_origin(c);
                    let (ox, oy, oz) =
                        (origin.x as i32, origin.y as i32, origin.z as i32);
                    for ly in 0..cs {
                        for lz in 0..cs {
                            for lx in 0..cs {
                                let b = chunk.get(lx as usize, ly as usize, lz as usize);
                                if crate::liquid::liquid_for(b).is_none() {
                                    continue;
                                }
                                let wv = [ox + lx, oy + ly, oz + lz];
                                self.liquid_positions.insert(wv);
                                any_liquid = true;
                                let below_air = ly > 0
                                    && chunk
                                        .get(lx as usize, (ly - 1) as usize, lz as usize)
                                        .is_air();
                                if below_air {
                                    let due = self.liquid_clock + 1;
                                    self.liquid_schedule.schedule(wv, due);
                                }
                            }
                        }
                    }
                    // W299: same emitter indexing the streamer runs
                    // (stream.rs) - without it the arrival region had
                    // no torch flames, no fire ticks, no portal swirl
                    // and no ambient-emitter entries at all.
                    crate::emitter_scan::scan_chunk(
                        c,
                        &chunk,
                        &mut self.emitter_positions,
                        &mut self.torch_positions,
                        &mut self.fires,
                        &mut self.portal_positions,
                    );
                    self.world.insert_chunk(c, chunk);
                    // W279: the synchronous gen path (portal arrival)
                    // runs the carve on THIS thread - fold its
                    // oriented template placements into the state
                    // maps directly (the worker path ships them as
                    // side effects instead).
                    // W302: structure-embedded mobs (sync path).
                    for m in crate::worldgen::mcgen::village::take_carved_mobs() {
                        if self.structure_mob_anchors.insert(m.anchor) {
                            crate::mob::spawn::mint_structure_mob(
                                self,
                                m.kind,
                                [
                                    m.anchor[0] as f32 + 0.5,
                                    m.anchor[1] as f32,
                                    m.anchor[2] as f32 + 0.5,
                                ],
                                0.0,
                                m.baby,
                            );
                        }
                    }
                    for o in crate::worldgen::mcgen::village::take_carved_oriented()
                    {
                        match o.kind {
                            0 => {
                                self.stair_states.insert(o.pos, o.state);
                            }
                            // W288: wall torch -> place-pipeline
                            // orient codes (see the stream drain arm).
                            4 => {
                                let orient = match o.state & 3 {
                                    0 => 1u8,
                                    1 => 3,
                                    2 => 2,
                                    _ => 4,
                                };
                                self.torch_orient.insert(o.pos, orient);
                            }
                            _ => {
                                self.door_facing.insert(o.pos, o.state);
                                if o.open {
                                    self.door_open.insert(o.pos);
                                }
                            }
                        }
                    }
                }
            }
        }
        if any_liquid {
            self.liquid_mesh_dirty = true;
        }
        region
    }

    /// W155: first Y (scanning down from the upper nether band)
    /// whose cell + the two above are open with a fully-occluding
    /// floor below - where a portal frame can stand. Falls back to
    /// a mid-height shelf when the column is all solid / all air.
    fn portal_ground_in_column(
        world: &infengine_voxel::World,
        x: i32,
        z: i32,
        start: i32,
    ) -> Option<i32> {
        for y in (6..=start).rev() {
            let below = world.get([x, y - 1, z]);
            let open = world.get([x, y, z]).is_air()
                && world.get([x, y + 1, z]).is_air()
                && world.get([x, y + 2, z]).is_air();
            if open && crate::block::block_fully_occludes(below) {
                return Some(y);
            }
        }
        None
    }

    /// W282: the ground snap searches a REGION like MC's
    /// createPortal candidate scan, not just the exact dest column.
    /// A single-column miss (cavern / lava-sea shaft) used to fall
    /// straight to the mid-shelf fallback and the return portal hung
    /// in the air. Rings of growing radius keep the found spot as
    /// close to the scaled dest as possible; only when EVERY column
    /// within 8 blocks misses does the forced mid-shelf remain (MC's
    /// forced roof portal equivalent).
    pub(crate) fn portal_ground_spot(
        world: &infengine_voxel::World,
        dest: [i32; 3],
    ) -> [i32; 3] {
        let start = dest[1].clamp(40, 100);
        if let Some(y) =
            Self::portal_ground_in_column(world, dest[0], dest[2], start)
        {
            return [dest[0], y, dest[2]];
        }
        // W306: 8 -> 16 - MC's createPortal scans 16 blocks out.
        for r in 1..=16i32 {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    if let Some(y) = Self::portal_ground_in_column(
                        world,
                        dest[0] + dx,
                        dest[2] + dz,
                        start,
                    ) {
                        return [dest[0] + dx, y, dest[2] + dz];
                    }
                }
            }
        }
        // W306: no valid dry column anywhere (mid-ocean / all-air).
        // The old fixed y=48 shelf put the frame UNDER the ocean (the
        // user's water portal). Use the column's SURFACE instead -
        // first non-air cell scanning down - so the forced platform
        // builds at sea level / ground level like MC's forced portal.
        let mut y = crate::WORLD_MAX_Y - 2;
        while y > crate::WORLD_MIN_Y + 2
            && world.get([dest[0], y, dest[2]]).is_air()
        {
            y -= 1;
        }
        [dest[0], (y + 1).clamp(crate::WORLD_MIN_Y + 2, crate::WORLD_MAX_Y - 4), dest[2]]
    }
}
