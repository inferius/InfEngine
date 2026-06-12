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
        // W5.1b: no global atomic to fall back on. The active dim
        // is expected to have an entry in `dim_worldgens` by the
        // time anyone reaches for a ctx; missing entries default
        // to Classic.
        let profile = self
            .dim_worldgens
            .get(&self.current_dim)
            .copied()
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
                for (_id, h) in kinds {
                    renderer.remove_mesh(h);
                }
            }
            self.liquid_chunk_sig.clear();
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
        self.ensure_generated_chunks_around(dest);
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
        // Broadcast portal edits so remote peers see the new
        // frame appear. Guarded on the renderer being live so
        // headless / test callers don't hit the rebuild path.
        if !edits.is_empty() {
            if self.renderer.is_some() {
                let coords = Self::affected_chunks(dest);
                self.rebuild_chunks(&coords);
            }
            let portal_id = block::PORTAL_ID;
            let obsidian_id = block::OBSIDIAN_ID;
            let edits_to_send: Vec<EditMsg> = edits
                .iter()
                .map(|p| EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: {
                        let v = self.world.get(*p).0;
                        if v == portal_id || v == obsidian_id { v } else { 0 }
                    },
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
        // Park the player adjacent to the return portal.
        self.player_pos = glam::Vec3::new(
            spawn[0] as f32 + 0.5,
            spawn[1] as f32,
            spawn[2] as f32 + 0.5,
        );
        self.player_vel = glam::Vec3::ZERO;
        self.portal_in_ticks = 0;
        self.portal_cooldown_ticks = block::portal::PORTAL_COOLDOWN_TICKS;
        self.chat_log.push((
            std::time::Instant::now(),
            format!("-> dim '{}'", target_dim),
        ));
    }

    /// W155: synchronously generate the chunk column stack around
    /// `pos` (3x3 columns, the full vertical range) with the ACTIVE
    /// dim's worldgen so a cross-dim arrival has real terrain.
    /// Already-present chunks (visited before) stay untouched.
    pub(crate) fn ensure_generated_chunks_around(&mut self, pos: [i32; 3]) {
        let ctx = self.current_worldgen_ctx();
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let ccx = pos[0].div_euclid(cs);
        let ccz = pos[2].div_euclid(cs);
        // Full vertical stack: the nether spans 0..=127 (chunks
        // 0..=7); generating the whole column keeps the ground
        // scan + ceiling consistent whatever Y the player arrives.
        let vertical = crate::VERTICAL_CHUNKS;
        for dx in -1..=1 {
            for dz in -1..=1 {
                for cy in 0..vertical.min(8) {
                    let c = [ccx + dx, cy, ccz + dz];
                    if self.world.chunk(c).is_none() {
                        let chunk =
                            crate::worldgen::generate_chunk_at_ctx(c, &ctx);
                        self.world.insert_chunk(c, chunk);
                    }
                }
            }
        }
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
