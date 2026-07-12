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
            torch_orient: std::mem::take(&mut self.torch_orient),
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
        self.torch_orient = target_world.torch_orient;
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
    pub(crate) fn tick_player_portal(&mut self) {
        if self.portal_cooldown_ticks > 0 {
            self.portal_cooldown_ticks -= 1;
            self.portal_in_ticks = 0;
            return;
        }
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
        // W155: generate REAL terrain at the arrival column before
        // any portal hunt. The old path inserted EMPTY chunks
        // (void), clamped the frame to y<=100 mid-air and the
        // player fell out of the sky; the empty shells also
        // permanently shadowed worldgen for those coords.
        let arrival_region = self.ensure_generated_chunks_around(dest);
        // Ground-snap the portal: first open spot with a solid
        // floor, scanning down from the dest height band.
        let ground = Self::portal_ground_y(&self.world, dest);
        let target = [dest[0], ground, dest[2]];
        // Find or build a return portal at the scaled coord.
        let cfg = bincode::config::standard();
        let mut edits: Vec<[i32; 3]> = Vec::new();
        let spawn = block::portal::ensure_return_portal(
            &mut self.world,
            target,
            Some(&mut edits),
        );
        // W159: GUARANTEE a solid platform + clear pocket at the
        // arrival, then re-mesh it. The surface scan can miss in
        // lava-sea / all-air columns, which is how the portal ended
        // up floating and the player fell. A dim-appropriate floor
        // under the frame footprint + a cleared 3-tall pocket makes
        // every arrival stand on solid ground like MC's platform.
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
            // W179: OVERWORLD return - NO stone platform. The old 4x4
            // stone floor + carve walled the player in a stone box (the
            // user's screenshot) because the overworld portal sits on
            // real terrain, not over void. Just clear the player's own
            // 1x1x3 standing column if it's buried so they don't
            // suffocate; the W173 embedded-safety-net lifts them out if
            // the spot is still solid.
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
        // W178: re-mesh the WHOLE arrival region, not just the portal
        // frame's chunks. switch_dim dropped the meshes of restored
        // chunks and the streamer only meshes chunks MISSING from the
        // world - so the present-but-unmeshed arrival chunks rendered
        // as a void (the floating portal + the chunk hole below/above
        // it, and the "inactive" portal whose purple cells just weren't
        // meshed). Rebuild every region chunk + the ground-snapped
        // frame's chunks (W156: they can sit chunks apart in Y).
        if self.renderer.is_some() {
            let mut coords = arrival_region;
            for c in Self::affected_chunks(spawn)
                .into_iter()
                .chain(Self::affected_chunks(target))
            {
                if !coords.contains(&c) {
                    coords.push(c);
                }
            }
            self.rebuild_chunks(&coords);
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
                    self.world.insert_chunk(c, chunk);
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
    pub(crate) fn portal_ground_y(
        world: &infengine_voxel::World,
        dest: [i32; 3],
    ) -> i32 {
        let (x, z) = (dest[0], dest[2]);
        let start = dest[1].clamp(40, 100);
        // Prefer a spot near the arrival height, then sweep down.
        for y in (6..=start).rev() {
            let below = world.get([x, y - 1, z]);
            let open = world.get([x, y, z]).is_air()
                && world.get([x, y + 1, z]).is_air()
                && world.get([x, y + 2, z]).is_air();
            if open && crate::block::block_fully_occludes(below) {
                return y;
            }
        }
        // All-solid column: carve nothing, just use a mid shelf -
        // build_minimal_portal overwrites the cells it needs.
        48
    }
}
