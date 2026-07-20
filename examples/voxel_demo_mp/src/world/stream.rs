//! W47.18 - chunk streamer, relocated from `main.rs`.
//!
//! `stream_chunks` runs once per frame and drives the entire
//! per-frame chunk lifecycle:
//! - queue missing chunks inside the view radius (sorted by XZ
//!   distance from the player)
//! - unload chunks outside the keep radius, persisting their edits
//!   to disk first (host / offline only) + pruning side maps
//!   (emitter / torch / fire / snow / liquid)
//! - dispatch pending coord jobs to the worker pool
//! - drain finished WorkerResults (Generated / Remeshed / Lit) into
//!   live world + chunk_meshes + lighting state
//! - drain deferred neighbour re-meshes
//!
//! Free function with `&mut Voxel`; same shape as `fire_runtime` +
//! `world::place_break` helpers. Body unchanged from the original
//! method; only the receiver moved.

use std::sync::Arc;

use infengine_voxel::{CHUNK_SIZE, World};

use crate::{
    CHUNK_PHASE_WARN_US, ChunkCoord, ChunkJob, ChunkSideEffect, Job,
    PendingChunk, REMESH_DEFERRED_PER_FRAME,
    VIEW_DISTANCE_MAX, Voxel, WorkerResult, emitter_scan,
    net::NetRole, world_save,
};
use crate::lighting::HaloLighting;

/// W47.18: drive one frame of the chunk stream. See module docs.
pub(crate) fn stream_chunks(vx: &mut Voxel) {
    // W284: whole-phase breakdown. The user's 10-FPS log showed
    // stream_ms pinned at ~120/frame while EVERY existing sub-probe
    // (W232 rescan, W233 result + remesh-drain) stayed silent - so
    // time each section and name the split whenever the phase blows
    // past 25 ms. Cheap: five Instant reads per frame.
    let _sc_t = std::time::Instant::now();
    let mut _ms_dispatch = 0.0f32;
    let mut _ms_recv = 0.0f32;
    let mut _ms_process = 0.0f32;
    let mut _n_generated = 0usize;
    let mut _n_remeshed = 0usize;
    let mut _n_lit = 0usize;
    let cs = CHUNK_SIZE as i32;
    let player_chunk = [
        (vx.player_pos.x.floor() as i32).div_euclid(cs),
        0,
        (vx.player_pos.z.floor() as i32).div_euclid(cs),
    ];
    // W267: during the LOADING screen the stream must not widen the
    // ring past the initial-build core - the loading gate waits for
    // `pending_remesh` to drain, and every extra streamed chunk keeps
    // feeding it, so a VD-32 world sat on the progress bar for the
    // WHOLE 101k-chunk ring (~5 min) instead of the 15k core. Clamp
    // to the core radius while loading; the full ring streams in the
    // background once Playing.
    let cfg_dist = vx.settings.view_distance.clamp(2, VIEW_DISTANCE_MAX as u8) as i32;
    let view_dist = if matches!(vx.state, crate::AppState::Loading { .. }) {
        cfg_dist.min(Voxel::LOADING_CORE_RADIUS)
    } else {
        cfg_dist
    };
    let unload_dist = view_dist + 2;

    let (cy_min, cy_max) = vx.vertical_chunk_range();

    // W198.13: the queue-missing + unload scans are the only uncapped
    // O(N) work in the streamer (everything past them is count- or
    // time-budgeted). Their RESULT only changes when the player crosses
    // a chunk boundary (horizontal or vertical) or the view distance
    // changes - so run them only then. Standing still / turning in
    // place no longer pays ~30k HashMap lookups + a full iter_chunks()
    // pass every frame. Dispatch + delivery + remesh below still run
    // every frame to keep draining the existing queues.
    // W321: the LOD radius is part of the key so a slider change
    // re-runs the ring scan without needing a boundary crossing.
    let lod_key = vx.settings.lod_distance as i32
        + ((vx.settings.lod2_distance as i32) << 9);
    let origin =
        (player_chunk[0], player_chunk[2], cy_min, cy_max, view_dist, lod_key);
    if vx.last_stream_origin != Some(origin) {
        vx.last_stream_origin = Some(origin);
        rescan_chunk_radius(vx, player_chunk, view_dist, unload_dist, cy_min, cy_max);
    }

    // Dispatch pending chunks to the worker pool.
    //
    // W239: dispatch whole COLUMNS. All still-queued chunks of the
    // popped coord's column go into ONE job, so the expensive
    // per-column worldgen data (density lattice + carve mask, ~26 ms
    // cold vs ~3 ms per warm chunk) computes once on one worker's
    // thread-local cache instead of once per worker the column's 16
    // chunks used to scatter across (measured 465 ms -> 80 ms per
    // column). Batched-along coords keep their stale heap entries;
    // `in_flight_chunks` makes later pops skip them.
    let _t = std::time::Instant::now();
    if let Some(pool) = vx.workers.as_ref() {
        let horiz_speed = (vx.player_vel.x * vx.player_vel.x
            + vx.player_vel.z * vx.player_vel.z)
            .sqrt();
        let dispatch_cap = if horiz_speed > 6.0 {
            32
        } else {
            pool.worker_count() * 2
        };
        let ctx = vx.current_worldgen_ctx();
        let pack_arc = Arc::clone(&vx.resource_pack);
        let mut dispatched = 0usize;
        while dispatched < dispatch_cap {
            let Some(pending) = vx.pending_chunks.pop() else { break };
            let cc = pending.coord;
            // Batched into an earlier column job (stale heap entry).
            if vx.in_flight_chunks.contains(&cc) {
                continue;
            }
            // Collect every queued, not-yet-flying chunk of this
            // column (bottom-up so the worker's column cache warms
            // once and stays hot).
            let mut coords: Vec<ChunkCoord> = Vec::new();
            for cy in cy_min..=cy_max {
                let col_cc = [cc[0], cy, cc[2]];
                if vx.pending_set.contains(&col_cc)
                    && !vx.in_flight_chunks.contains(&col_cc)
                {
                    coords.push(col_cc);
                }
            }
            if coords.is_empty() {
                continue;
            }
            let halos: Vec<HaloLighting> = coords
                .iter()
                .map(|cc| HaloLighting {
                    own: vx.lighting.get(*cc).cloned(),
                    px: vx.lighting.get([cc[0] + 1, cc[1], cc[2]]).cloned(),
                    nx: vx.lighting.get([cc[0] - 1, cc[1], cc[2]]).cloned(),
                    pz: vx.lighting.get([cc[0], cc[1], cc[2] + 1]).cloned(),
                    nz: vx.lighting.get([cc[0], cc[1], cc[2] - 1]).cloned(),
                    py: vx.lighting.get([cc[0], cc[1] + 1, cc[2]]).cloned(),
                    ny: vx.lighting.get([cc[0], cc[1] - 1, cc[2]]).cloned(),
                })
                .collect();
            let batch = coords.len();
            for col_cc in &coords {
                vx.in_flight_chunks.insert(*col_cc);
            }
            let job = Job::Generate(ChunkJob {
                coords: coords.clone(),
                ctx: ctx.clone(),
                pack: Arc::clone(&pack_arc),
                halos,
                slug: vx.current_world_slug.clone(),
                dim: vx.current_dim.clone(),
            });
            if pool.req_tx.send(job).is_err() {
                // Pool shutting down: un-mark the batch so a future
                // dispatch can retry it, and requeue the popped entry.
                for col_cc in &coords {
                    vx.in_flight_chunks.remove(col_cc);
                }
                vx.pending_chunks.push(pending);
                break;
            }
            // The cap counts CHUNKS (not jobs) so the in-flight result
            // volume matches the old per-chunk dispatch behaviour.
            dispatched += batch;
        }
    }

    _ms_dispatch = _t.elapsed().as_secs_f32() * 1000.0;

    // W321: dispatch far LOD shells only when the full-res queue is
    // idle - real chunks always win the workers. Tiles are pure
    // column-data replays (mcgen only), so the ring is gated to the
    // MC overworld profile.
    if let Some(pool) = vx.workers.as_ref()
        && !vx.lod_pending.is_empty()
    {
        let ctx = vx.current_worldgen_ctx();
        if ctx.profile == crate::worldgen::WorldGen::Mc {
            let pack_arc = Arc::clone(&vx.resource_pack);
            // W333: trickle shells even while the gen queue is busy.
            // The old is_empty() gate meant a travelling player NEVER
            // built shells (the queue never drains in flight) - the
            // far scenery only filled in after stopping. A 2/frame
            // trickle keeps real chunks dominant; the full 16/frame
            // burst still applies once the queue is idle.
            // W335: tiles are preliminary-surface replays now
            // (microseconds each), so the idle drain can be generous
            // - and W345 raises the busy trickle too (a tile costs
            // the workers next to nothing vs a chunk column).
            let cap = if vx.pending_chunks.is_empty() { 64 } else { 8 };
            let mut sent = 0usize;
            while sent < cap {
                let Some(pending) = vx.lod_pending.pop() else { break };
                let key = pending.key;
                // Stale heap entries (tile arrived / ring moved).
                if !vx.lod_pending_set.contains(&key)
                    || vx.lod_meshes.contains_key(&key)
                {
                    continue;
                }
                let job = Job::LodTile(crate::lod::LodTileJob {
                    key,
                    level: 1,
                    ctx: ctx.clone(),
                    pack: Arc::clone(&pack_arc),
                    dim: vx.current_dim.clone(),
                });
                if pool.req_tx.send(job).is_err() {
                    vx.lod_pending.push(pending);
                    break;
                }
                sent += 1;
            }
            // W333: far tiles trickle after near ones (they are far
            // cheaper to build - preliminary surface only).
            let mut sent2 = 0usize;
            while sent2 < cap {
                let Some(pending) = vx.lod2_pending.pop() else { break };
                let key = pending.key;
                if !vx.lod2_pending_set.contains(&key)
                    || vx.lod2_meshes.contains_key(&key)
                {
                    continue;
                }
                let job = Job::LodTile(crate::lod::LodTileJob {
                    key,
                    level: 2,
                    ctx: ctx.clone(),
                    pack: Arc::clone(&pack_arc),
                    dim: vx.current_dim.clone(),
                });
                if pool.req_tx.send(job).is_err() {
                    vx.lod2_pending.push(pending);
                    break;
                }
                sent2 += 1;
            }
        }
    }

    // Drain finished results from the workers.
    let _t = std::time::Instant::now();
    let (_results_cap, budget_ms) = vx.current_results_budget();
    // W106: do NOT clear here - results that last frame's processing
    // time-budget left unprocessed carry over, so a burst of finished
    // chunks drains across several frames instead of one big spike.
    //
    // W306: drain the channel FULLY every frame (a try_recv is
    // pointer-cheap; PROCESSING is what the time budget bounds). The
    // old results_cap left results IN the channel, so during a gen
    // burst hundreds of Generated entries queued AHEAD of Lit results
    // - the in-flight lighting slots never freed and light_segq froze
    // at 1600+ for minutes (the underwater-portal 10 FPS log). The
    // 8192 guard only bounds a pathological worker flood.
    if let Some(pool) = vx.workers.as_ref() {
        while vx.stream_delivered.len() < 8192
            && let Ok(r) = pool.res_rx.try_recv()
        {
            vx.stream_delivered.push(r);
        }
    }
    _ms_recv = _t.elapsed().as_secs_f32() * 1000.0;
    let mut delivered = std::mem::take(&mut vx.stream_delivered);
    // W306: process priority - Lit first (frees the lighting lane,
    // cheap Arc swaps), then Remeshed (player edits must show NOW,
    // not after the gen backlog), then Generated bulk. Stable sort
    // keeps FIFO order within each class.
    delivered.sort_by_key(|r| match r {
        WorkerResult::Lit { .. } => 0u8,
        WorkerResult::Remeshed { .. } => 1,
        WorkerResult::Generated { .. } => 2,
        // W321: shells are pure decoration - always after real chunks.
        WorkerResult::LodTile(_) => 3,
    });
    // W106: apply the per-frame time budget to PROCESSING (the GPU mesh
    // uploads), which is what the budget always intended (see the
    // RESULTS_BUDGET_MS doc). Previously it gated only the cheap try_recv
    // drain, so all `results_cap` meshes still GPU-uploaded in ONE frame -
    // ballooning it to 15-20 ms and making the camera + nearby mobs/trees
    // visibly judder during streaming/lighting bursts. Whatever doesn't
    // fit the budget carries to the next frame via `stream_delivered`.
    let process_start = std::time::Instant::now();
    let process_budget = std::time::Duration::from_millis(budget_ms);
    let mut delivered_iter = delivered.into_iter();
    for result in delivered_iter.by_ref() {
        // W233: name any single result whose processing blows past the
        // whole frame budget - the user's log showed single-frame
        // stream_ms of 186/319 ms that no other probe attributed.
        let _result_t = std::time::Instant::now();
        let _result_kind = match &result {
            WorkerResult::Generated { .. } => {
                _n_generated += 1;
                "generated"
            }
            WorkerResult::Remeshed { .. } => {
                _n_remeshed += 1;
                "remeshed"
            }
            WorkerResult::Lit { .. } => {
                _n_lit += 1;
                "lit"
            }
            WorkerResult::LodTile(_) => "lod",
        };
        match result {
            WorkerResult::Generated {
                coord,
                chunk,
                side_effects,
                submeshes,
                dim,
            } => {
                vx.pending_set.remove(&coord);
                vx.in_flight_chunks.remove(&coord);
                // W277: a result enqueued against another dim (portal
                // switch mid-flight) must NOT land in this dim's world
                // - it would permanently shadow worldgen at that coord
                // (the post-ping-pong "no chunks load" breakage).
                if dim != vx.current_dim {
                    continue;
                }
                let dx = (coord[0] - player_chunk[0]).abs();
                let dz = (coord[2] - player_chunk[2]).abs();
                if dx > unload_dist || dz > unload_dist {
                    continue;
                }
                if vx.world.chunk(coord).is_some() {
                    continue;
                }
                let t0 = std::time::Instant::now();
                // W127: a loaded chunk containing rails re-meshes once
                // so the rail quads pick up their derived shapes (the
                // worker meshed it with an empty snapshot).
                let mut needs_rail_remesh = false;
                let mut chunk_got_liquid = false;
                for se in &side_effects {
                    match se {
                        ChunkSideEffect::Snow(wv) => {
                            vx.snow_positions.entry(*wv).or_insert(1);
                        }
                        ChunkSideEffect::Torch(wv) => {
                            vx.torch_positions.insert(*wv);
                        }
                        ChunkSideEffect::Rail => {
                            needs_rail_remesh = true;
                        }
                        ChunkSideEffect::DungeonChest { pos: wv, table } => {
                            // W353: the render set - see draw_chests.
                            vx.chest_render_positions.insert(*wv);
                            // W242: mint the structure chest ONCE with a
                            // seed+position-deterministic loot roll. A
                            // container that already exists (saved world,
                            // possibly looted by the player) always wins.
                            if !vx.chests.contains_key(wv) {
                                let seed = vx
                                    .current_worldgen_ctx()
                                    .effective_seed();
                                let mut rng =
                                    crate::worldgen::mcgen::noise::SplitMix64::new(
                                        crate::loot::chest_pos_seed(seed, *wv),
                                    );
                                // W243/W246/W249: the loot table follows
                                // the structure that owns the cell; the
                                // WORKER decided it (membership probes
                                // need the worldgen caches - on the main
                                // thread they built the whole MC noise
                                // stack: 100-200 ms delivery folds).
                                let table = match table {
                                    crate::workers::ChestLootKind::AbandonedMineshaft => {
                                        &crate::loot::ABANDONED_MINESHAFT
                                    }
                                    crate::workers::ChestLootKind::DesertPyramid => {
                                        &crate::loot::DESERT_PYRAMID
                                    }
                                    crate::workers::ChestLootKind::IglooChest => {
                                        &crate::loot::IGLOO_CHEST
                                    }
                                    crate::workers::ChestLootKind::PillagerOutpost => {
                                        &crate::loot::PILLAGER_OUTPOST
                                    }
                                    crate::workers::ChestLootKind::ShipwreckMap => {
                                        &crate::loot::SHIPWRECK_MAP
                                    }
                                    crate::workers::ChestLootKind::ShipwreckTreasure => {
                                        &crate::loot::SHIPWRECK_TREASURE
                                    }
                                    crate::workers::ChestLootKind::ShipwreckSupply => {
                                        &crate::loot::SHIPWRECK_SUPPLY
                                    }
                                    crate::workers::ChestLootKind::BuriedTreasure => {
                                        &crate::loot::BURIED_TREASURE
                                    }
                                    crate::workers::ChestLootKind::UnderwaterRuinSmall => {
                                        &crate::loot::UNDERWATER_RUIN_SMALL
                                    }
                                    crate::workers::ChestLootKind::UnderwaterRuinBig => {
                                        &crate::loot::UNDERWATER_RUIN_BIG
                                    }
                                    crate::workers::ChestLootKind::JungleTemple => {
                                        &crate::loot::JUNGLE_TEMPLE
                                    }
                                    crate::workers::ChestLootKind::StrongholdCorridor => {
                                        &crate::loot::STRONGHOLD_CORRIDOR
                                    }
                                    crate::workers::ChestLootKind::StrongholdCrossing => {
                                        &crate::loot::STRONGHOLD_CROSSING
                                    }
                                    crate::workers::ChestLootKind::StrongholdLibrary => {
                                        &crate::loot::STRONGHOLD_LIBRARY
                                    }
                                    crate::workers::ChestLootKind::RuinedPortal => {
                                        &crate::loot::RUINED_PORTAL
                                    }
                                    crate::workers::ChestLootKind::WoodlandMansion => {
                                        &crate::loot::WOODLAND_MANSION
                                    }
                                    crate::workers::ChestLootKind::AncientCity => {
                                        &crate::loot::ANCIENT_CITY
                                    }
                                    crate::workers::ChestLootKind::AncientCityIceBox => {
                                        &crate::loot::ANCIENT_CITY_ICE_BOX
                                    }
                                    crate::workers::ChestLootKind::TrialChambersCorridor => {
                                        &crate::loot::TRIAL_CHAMBERS_CORRIDOR
                                    }
                                    crate::workers::ChestLootKind::TrialChambersEntrance => {
                                        &crate::loot::TRIAL_CHAMBERS_ENTRANCE
                                    }
                                    crate::workers::ChestLootKind::TrialChambersIntersection => {
                                        &crate::loot::TRIAL_CHAMBERS_INTERSECTION
                                    }
                                    crate::workers::ChestLootKind::TrialChambersSupply => {
                                        &crate::loot::TRIAL_CHAMBERS_SUPPLY
                                    }
                                    crate::workers::ChestLootKind::BastionTreasure => {
                                        &crate::loot::BASTION_TREASURE
                                    }
                                    crate::workers::ChestLootKind::BastionOther => {
                                        &crate::loot::BASTION_OTHER
                                    }
                                    crate::workers::ChestLootKind::BastionBridge => {
                                        &crate::loot::BASTION_BRIDGE
                                    }
                                    crate::workers::ChestLootKind::BastionHoglinStable => {
                                        &crate::loot::BASTION_HOGLIN_STABLE
                                    }
                                    crate::workers::ChestLootKind::NetherBridge => {
                                        &crate::loot::NETHER_BRIDGE
                                    }
                                    crate::workers::ChestLootKind::SimpleDungeon => {
                                        &crate::loot::SIMPLE_DUNGEON
                                    }
                                };
                                let stacks = crate::loot::roll(table, &mut rng);
                                let mut c = crate::block::chest::new_chest();
                                crate::loot::fill_container(
                                    &mut c, &stacks, &mut rng,
                                );
                                vx.chests.insert(*wv, c);
                                // Latch faces the room: the dungeon rule
                                // places chests with exactly one solid
                                // horizontal neighbour, so face AWAY from
                                // it (MC StructurePiece::reorient).
                                // dir list index -> facing code (0=+X,
                                // 1=+Z, 2=-X, 3=-Z per facing_from_look).
                                // This fold runs BEFORE insert_chunk, so
                                // in-chunk neighbours read from the local
                                // chunk; border neighbours fall back to
                                // the world (may be unloaded = air).
                                let cs = CHUNK_SIZE as i32;
                                let ov = World::chunk_world_origin(coord);
                                let org =
                                    [ov.x as i32, ov.y as i32, ov.z as i32];
                                let nb_id = |dx: i32, dz: i32| -> u16 {
                                    let (x, y, z) =
                                        (wv[0] + dx, wv[1], wv[2] + dz);
                                    let (lx, ly, lz) =
                                        (x - org[0], y - org[1], z - org[2]);
                                    if (0..cs).contains(&lx)
                                        && (0..cs).contains(&ly)
                                        && (0..cs).contains(&lz)
                                    {
                                        chunk
                                            .get(
                                                lx as usize,
                                                ly as usize,
                                                lz as usize,
                                            )
                                            .0
                                    } else {
                                        vx.world.get([x, y, z]).0
                                    }
                                };
                                let dirs =
                                    [[1, 0], [-1, 0], [0, 1], [0, -1]];
                                let codes = [0u8, 2, 1, 3];
                                let solid_dir =
                                    dirs.iter().position(|[dx, dz]| {
                                        let nb = nb_id(*dx, *dz);
                                        nb != 0
                                            && crate::block::block_type(nb)
                                                .is_some_and(|bt| {
                                                    !bt.transparent
                                                })
                                    });
                                if let Some(d) = solid_dir {
                                    vx.chest_facing
                                        .entry(*wv)
                                        .or_insert((codes[d] + 2) % 4);
                                }
                            }
                        }
                        ChunkSideEffect::Spawner { pos: wv, kind } => {
                            // W242.2: index only - tick state mints
                            // lazily when a player walks into range.
                            // W250: the worker-resolved entity kind
                            // rides along (main-thread membership
                            // probes built the noise stack).
                            vx.spawner_positions.insert(*wv);
                            vx.spawner_kinds.insert(*wv, *kind);
                        }
                        ChunkSideEffect::StructureMob { anchor, kind, baby } => {
                            // W302: spawn once per anchor, ever - the
                            // persistent set survives saves, so chunk
                            // regen (stream-out + revisit) can't
                            // double-populate a village.
                            if vx.structure_mob_anchors.insert(*anchor) {
                                crate::mob::spawn::mint_structure_mob(
                                    vx,
                                    *kind,
                                    [
                                        anchor[0] as f32 + 0.5,
                                        anchor[1] as f32,
                                        anchor[2] as f32 + 0.5,
                                    ],
                                    0.0,
                                    *baby,
                                );
                            }
                        }
                        ChunkSideEffect::Oriented { pos, kind, state, open } => {
                            // W279: template-placed oriented blocks.
                            // Stairs key their own map; doors (lower
                            // half), trapdoors + fence gates share the
                            // door facing/open state the mesher +
                            // toggles already read.
                            match kind {
                                0 => {
                                    vx.stair_states.insert(*pos, *state);
                                }
                                // W288: wall torches land in the same
                                // orient map player placements use
                                // (facing 0..3 -> the place-pipeline
                                // 1..4 wall codes).
                                4 => {
                                    let orient = match *state & 3 {
                                        0 => 1u8,
                                        1 => 3,
                                        2 => 2,
                                        _ => 4,
                                    };
                                    vx.torch_orient.insert(*pos, orient);
                                }
                                _ => {
                                    vx.door_facing.insert(*pos, *state);
                                    if *open {
                                        vx.door_open.insert(*pos);
                                    }
                                }
                            }
                        }
                        ChunkSideEffect::Liquid { pos, tick_offset } => {
                            // W115: tick_offset 0 = a static worldgen liquid
                            // cell at equilibrium (sea resting on seabed) -
                            // render it, but DON'T schedule a tick, so the
                            // ocean never re-settles on stream-in (no surface
                            // "walls" from edge oscillation, no sky-relight
                            // spike). Non-zero = the cell can move (air below)
                            // -> schedule it. Placed/edited liquid still ticks
                            // through set_block, unaffected.
                            if *tick_offset > 0 {
                                vx.liquid_schedule.schedule(
                                    *pos,
                                    vx.liquid_clock + tick_offset,
                                );
                            }
                            if vx.liquid_positions.insert(*pos) {
                                chunk_got_liquid = true;
                            }
                        }
                    }
                }
                // W199: a streamed-in liquid chunk needs its OWN liquid mesh
                // built - but it must NOT trip the global `liquid_mesh_dirty`
                // rescan, which re-signatures ALL ~300k ocean cells on every
                // streamed chunk (W195 only de-duped the per-frame REPEAT, not
                // the per-chunk-load TRIGGER, so flying over ocean still paid
                // an O(all-liquid) ~14ms scan every frame). Instead enqueue
                // just this chunk + its 6 face neighbours (so the shared
                // faces re-cull) straight into the per-chunk remesh backlog,
                // which drains a budget of chunks per frame.
                if chunk_got_liquid {
                    vx.liquid_remesh_backlog.insert(coord);
                    vx.liquid_remesh_backlog.insert([coord[0] - 1, coord[1], coord[2]]);
                    vx.liquid_remesh_backlog.insert([coord[0] + 1, coord[1], coord[2]]);
                    vx.liquid_remesh_backlog.insert([coord[0], coord[1] - 1, coord[2]]);
                    vx.liquid_remesh_backlog.insert([coord[0], coord[1] + 1, coord[2]]);
                    vx.liquid_remesh_backlog.insert([coord[0], coord[1], coord[2] - 1]);
                    vx.liquid_remesh_backlog.insert([coord[0], coord[1], coord[2] + 1]);
                }
                let side_effect_us = t0.elapsed().as_micros();
                if side_effect_us > CHUNK_PHASE_WARN_US {
                    tracing::warn!(
                        coord = ?coord,
                        us = side_effect_us,
                        "W13.1: side_effects fold exceeded threshold"
                    );
                }
                emitter_scan::scan_chunk(
                    coord,
                    &chunk,
                    &mut vx.emitter_positions,
                    &mut vx.torch_positions,
                    &mut vx.fires,
                    &mut vx.portal_positions,
                );
                vx.world.insert_chunk(coord, chunk);
                // W127: rails meshed with an empty shape snapshot in the
                // worker; queue one rebuild now that the chunk is live.
                if needs_rail_remesh {
                    vx.rebuild_chunks(&[coord]);
                }
                let cc = coord;
                if !submeshes.is_empty() {
                    let atlas = vx.voxel_atlas.clone();
                    let renderer = vx.renderer.as_mut().expect("renderer ready");
                    // W240: submeshes arrive MERGED per render layer
                    // (sub.block = layer sentinel); the skip filter +
                    // cutout special-case moved into chunk_merge.
                    for sub in submeshes {
                        if sub.vertices.is_empty() || sub.indices.is_empty() {
                            continue;
                        }
                        let handle = renderer.create_chunk_mesh(
                            &format!(
                                "voxel_chunk_{}_{}_{}_l{}",
                                cc[0], cc[1], cc[2], sub.block.0
                            ),
                            &sub.vertices,
                            &sub.indices,
                        );
                        if let Some(a) = atlas.clone() {
                            renderer.bind_chunk_mesh_albedo(handle, a);
                        }
                        if sub.block.0 != 0 {
                            renderer.set_chunk_mesh_layer(handle, sub.block.0 as u8);
                        }
                        vx.chunk_meshes.entry(cc).or_default().push((sub.block, handle));
                    }
                }
                // W345: neighbour re-mesh is OVERDRAW CLEANUP, not
                // correctness - the mesher emits boundary faces
                // against missing neighbours (nothing is ever
                // revealed by a new chunk, and light seams flow
                // through the Lit pipeline + W275 diff). Streaming
                // used to triple the mesh+upload volume with these;
                // now they wait for an idle stream.
                for n in [
                    [cc[0] - 1, cc[1], cc[2]],
                    [cc[0] + 1, cc[1], cc[2]],
                    [cc[0], cc[1], cc[2] - 1],
                    [cc[0], cc[1], cc[2] + 1],
                ] {
                    if vx.world.chunk(n).is_some() && vx.pending_seam_set.insert(n) {
                        vx.pending_seam_remesh.push_back(n);
                    }
                }
                let cs = CHUNK_SIZE as i32;
                vx.lighting.mark_dirty([
                    cc[0] * cs + cs / 2,
                    cc[1] * cs + cs / 2,
                    cc[2] * cs + cs / 2,
                ]);
            }
            WorkerResult::Remeshed {
                coord,
                revision,
                submeshes,
                dim,
            } => {
                // W277: stale-dim remesh would mint ghost meshes for
                // chunks that aren't in this dim's world.
                if dim != vx.current_dim {
                    continue;
                }
                let current_rev = vx.chunk_revisions.get(&coord).copied().unwrap_or(0);
                if revision < current_rev {
                    tracing::debug!(
                        coord = ?coord,
                        arrived_rev = revision,
                        current_rev,
                        "W32 debug: stale Remeshed result dropped"
                    );
                    continue;
                }
                tracing::debug!(
                    coord = ?coord,
                    revision,
                    submeshes = submeshes.len(),
                    "W32 debug: Remeshed result kept"
                );
                {
                    let renderer = vx.renderer.as_mut().expect("renderer ready");
                    if let Some(old_subs) = vx.chunk_meshes.remove(&coord) {
                        for (_block, handle) in old_subs {
                            renderer.remove_chunk_mesh(handle);
                        }
                    }
                }
                if !submeshes.is_empty() {
                    let atlas = vx.voxel_atlas.clone();
                    let renderer = vx.renderer.as_mut().expect("renderer ready");
                    // W240: merged per-layer submeshes (see the
                    // Generated arm note).
                    for sub in submeshes {
                        if sub.vertices.is_empty() || sub.indices.is_empty() {
                            continue;
                        }
                        let handle = renderer.create_chunk_mesh(
                            &format!(
                                "voxel_chunk_{}_{}_{}_l{}",
                                coord[0], coord[1], coord[2], sub.block.0
                            ),
                            &sub.vertices,
                            &sub.indices,
                        );
                        if let Some(a) = atlas.clone() {
                            renderer.bind_chunk_mesh_albedo(handle, a);
                        }
                        if sub.block.0 != 0 {
                            renderer.set_chunk_mesh_layer(handle, sub.block.0 as u8);
                        }
                        vx.chunk_meshes
                            .entry(coord)
                            .or_default()
                            .push((sub.block, handle));
                    }
                }
            }
            WorkerResult::Lit {
                new_grids,
                affected_for_remesh,
                dim,
            } => {
                // W277: stale-dim grids are discarded, but the job
                // still finished - the in-flight counter MUST drop or
                // the dispatch cap wedges at MAX forever.
                if dim != vx.current_dim {
                    vx.pending_lighting_jobs =
                        vx.pending_lighting_jobs.saturating_sub(1);
                    continue;
                }
                let lit_apply_start = std::time::Instant::now();
                let new_grid_count = new_grids.len();
                for (cc, grid) in new_grids {
                    vx.lighting.set_grid(cc, grid);
                }
                let mut queued = 0usize;
                for cc in affected_for_remesh {
                    if vx.world.chunk(cc).is_some() && vx.pending_remesh_set.insert(cc) {
                        vx.pending_remesh.push_back(cc);
                        queued += 1;
                    }
                }
                // W234: debug, not info - this fired once per Lit result
                // during streaming and the stdout lock contention with the
                // 24 workers' own log lines stalled the main thread for
                // 25-105 ms per call on a live console (apply_ms was 0 the
                // whole time; the cost was the LOGGING, not the apply).
                tracing::debug!(
                    grids = new_grid_count,
                    remesh_queued = queued,
                    apply_ms = lit_apply_start.elapsed().as_millis() as u64,
                    "W46.30: WorkerResult::Lit applied + remesh queued",
                );
                vx.pending_lighting_jobs = vx.pending_lighting_jobs.saturating_sub(1);
            }
            WorkerResult::LodTile(res) => {
                // W321/W333: a surface shell finished. The pending
                // set doubles as the in-flight marker; level routes
                // the target map.
                let far = res.level == 2;
                if far {
                    vx.lod2_pending_set.remove(&res.key);
                } else {
                    vx.lod_pending_set.remove(&res.key);
                }
                if res.dim != vx.current_dim {
                    continue;
                }
                // Beyond the (possibly changed) ring: drop it. Far
                // keys are in 4x4-chunk tile space.
                let (ring, span) = if far {
                    (
                        (vx.settings.lod2_distance as i32)
                            .min(crate::lod::LOD2_DISTANCE_MAX),
                        crate::lod::LOD2_TILE_CHUNKS,
                    )
                } else {
                    (
                        (vx.settings.lod_distance as i32)
                            .min(crate::lod::LOD_DISTANCE_MAX),
                        1,
                    )
                };
                let dx = res.key[0] * span + span / 2 - player_chunk[0];
                let dz = res.key[1] * span + span / 2 - player_chunk[2];
                let keep = ring + 2 * span;
                if dx * dx + dz * dz > keep * keep {
                    continue;
                }
                let Some(renderer) = vx.renderer.as_mut() else {
                    continue;
                };
                let map = if far {
                    &mut vx.lod2_meshes
                } else {
                    &mut vx.lod_meshes
                };
                // Replacing an existing tile frees the old meshes.
                if let Some(old) = map.remove(&res.key) {
                    if let Some(h) = old.opaque {
                        renderer.remove_chunk_mesh(h);
                    }
                    if let Some(h) = old.water {
                        renderer.remove_chunk_mesh(h);
                    }
                }
                let [cx, cz] = res.key;
                let lvl = res.level;
                let opaque = (!res.opaque.0.is_empty()).then(|| {
                    renderer.create_chunk_mesh(
                        &format!("voxel_lod{lvl}_{cx}_{cz}"),
                        &res.opaque.0,
                        &res.opaque.1,
                    )
                });
                let water = (!res.water.0.is_empty()).then(|| {
                    let h = renderer.create_chunk_mesh(
                        &format!("voxel_lod{lvl}_{cx}_{cz}_w"),
                        &res.water.0,
                        &res.water.1,
                    );
                    renderer.set_chunk_mesh_layer(h, 2);
                    h
                });
                let map = if far {
                    &mut vx.lod2_meshes
                } else {
                    &mut vx.lod_meshes
                };
                map.insert(
                    res.key,
                    crate::lod::LodTile {
                        opaque,
                        water,
                        y_min: res.y_min,
                        y_max: res.y_max,
                        y_surface: res.y_surface,
                        water_tint: res.water_tint,
                    },
                );
            }
        }
        // W233: see _result_t above.
        {
            let _ms = _result_t.elapsed().as_secs_f32() * 1000.0;
            if _ms > 20.0 {
                tracing::warn!(
                    ms = _ms,
                    kind = _result_kind,
                    "W233: slow stream result"
                );
            }
        }
        // W106: stop once this frame's streaming budget is spent; the
        // remaining drained results stay queued for the next frame.
        if process_start.elapsed() >= process_budget {
            break;
        }
    }
    _ms_process = process_start.elapsed().as_secs_f32() * 1000.0;
    vx.stream_delivered = delivered_iter.collect();

    // Drain deferred neighbour re-meshes.
    // W326: scale the per-frame cap with the backlog. The fixed
    // 64/frame + 2 ms guard let the queue ratchet to 7000+ during a
    // walking lighting churn (every frontier Lit queues neighbour
    // remeshes) - visible as chunks holding stale light for many
    // seconds. A deep queue now drains up to 4x as many per frame
    // with a matching time guard; the small steady-state cost is
    // unchanged when the queue is short.
    let _remesh_t = std::time::Instant::now();
    let mut remesh_done = 0;
    let q_len = vx.pending_remesh.len();
    let (remesh_cap, remesh_budget_us) = if q_len > 2048 {
        (REMESH_DEFERRED_PER_FRAME * 4, 6_000)
    } else if q_len > 512 {
        (REMESH_DEFERRED_PER_FRAME * 2, 4_000)
    } else {
        (REMESH_DEFERRED_PER_FRAME, 2_000)
    };
    while remesh_done < remesh_cap
        && _remesh_t.elapsed().as_micros() < remesh_budget_us
    {
        let Some(coord) = vx.pending_remesh.pop_front() else {
            break;
        };
        vx.pending_remesh_set.remove(&coord);
        if vx.world.chunk(coord).is_none() {
            continue;
        }
        vx.rebuild_chunks(&[coord]);
        remesh_done += 1;
    }
    // W345: seam-cleanup drain - ONLY while the stream is idle
    // (no queued gen, no carried results, remesh queue quiet), a
    // few per frame. Culls the hidden boundary faces + fixes seam
    // AO that streaming deliberately skipped.
    if vx.pending_chunks.is_empty()
        && vx.stream_delivered.is_empty()
        && vx.pending_remesh.len() < 32
    {
        let mut seam_done = 0usize;
        while seam_done < 8 {
            let Some(coord) = vx.pending_seam_remesh.pop_front() else {
                break;
            };
            vx.pending_seam_set.remove(&coord);
            if vx.world.chunk(coord).is_none() {
                continue;
            }
            // Route through the normal queue so the budget +
            // dedup logic stays in one place.
            if vx.pending_remesh_set.insert(coord) {
                vx.pending_remesh.push_back(coord);
            }
            seam_done += 1;
        }
    }
    // W233: the remesh drain dispatches snapshots (chunk + halo
    // clones) to the workers - name it if it blows up.
    {
        let _ms = _remesh_t.elapsed().as_secs_f32() * 1000.0;
        if _ms > 20.0 {
            tracing::warn!(
                ms = _ms,
                drained = remesh_done,
                "W233: slow remesh-drain"
            );
        }
    }
    // W284: name the split whenever the whole phase runs long (the
    // existing narrower probes stayed silent through a sustained
    // stream_ms=120/frame incident - whatever section they don't
    // cover shows up here as total minus the parts).
    {
        let _ms_total = _sc_t.elapsed().as_secs_f32() * 1000.0;
        if _ms_total > 25.0 {
            tracing::warn!(
                total_ms = _ms_total,
                dispatch_ms = _ms_dispatch,
                recv_ms = _ms_recv,
                process_ms = _ms_process,
                remesh_ms = _remesh_t.elapsed().as_secs_f32() * 1000.0,
                generated = _n_generated,
                remeshed = _n_remeshed,
                lit = _n_lit,
                carry = vx.stream_delivered.len(),
                "W284: slow stream_chunks"
            );
        }
    }
}

/// W198.13: queue missing chunks in the view radius + unload chunks
/// beyond the keep radius, then prune their side maps. Split out of
/// `stream_chunks` so the caller can gate it on player chunk-boundary
/// crossings (see `Voxel::last_stream_origin`) rather than paying its
/// O(view_dist^2 * height) queue scan + O(all-loaded) unload scan on
/// every single frame. Body is byte-for-byte the pre-W198.13 inline
/// logic; only the receiver moved.
fn rescan_chunk_radius(
    vx: &mut Voxel,
    player_chunk: [i32; 3],
    view_dist: i32,
    unload_dist: i32,
    cy_min: i32,
    cy_max: i32,
) {
    let cs = CHUNK_SIZE as i32;
    // W232: sub-phase probe. The W198.16 hitch line blamed
    // "stream 111ms" on chunk crossings while travelling without
    // saying which part - name it (queue scan / unload collect /
    // mesh removal / payload build / side-map retains).
    let _t_all = std::time::Instant::now();
    let mut _ms_queue = 0.0f32;
    let mut _ms_collect = 0.0f32;
    let mut _ms_mesh = 0.0f32;
    let mut _ms_payload = 0.0f32;
    let _t = std::time::Instant::now();

    // Queue missing chunks inside the view radius.
    for dz in -view_dist..=view_dist {
        for dx in -view_dist..=view_dist {
            let dist_sq = (dx * dx + dz * dz) as i64;
            for cy in cy_min..=cy_max {
                let cc = [player_chunk[0] + dx, cy, player_chunk[2] + dz];
                if vx.world.chunk(cc).is_none() && vx.pending_set.insert(cc) {
                    vx.pending_chunks.push(PendingChunk {
                        neg_dist_sq: -dist_sq,
                        coord: cc,
                    });
                }
            }
        }
    }
    _ms_queue = _t.elapsed().as_secs_f32() * 1000.0;
    // W321: maintain the far LOD ring on the same boundary-cross
    // cadence (must run BEFORE the to_unload early-return below).
    rescan_lod_ring(vx, player_chunk, view_dist, cy_min, cy_max);
    let _t = std::time::Instant::now();

    // Unload chunks beyond the keep radius.
    let to_unload: std::collections::HashSet<ChunkCoord> = vx
        .world
        .iter_chunks()
        .map(|(c, _)| *c)
        .filter(|c| {
            let dx = (c[0] - player_chunk[0]).abs();
            let dz = (c[2] - player_chunk[2]).abs();
            let horizontal_out = dx > unload_dist || dz > unload_dist;
            let vertical_out = c[1] < cy_min - 1 || c[1] > cy_max + 1;
            horizontal_out || vertical_out
        })
        .collect();
    _ms_collect = _t.elapsed().as_secs_f32() * 1000.0;
    if to_unload.is_empty() {
        return;
    }
    let _t = std::time::Instant::now();

    let renderer = vx.renderer.as_mut().expect("renderer ready");
    for cc in &to_unload {
        if let Some(subs) = vx.chunk_meshes.remove(cc) {
            for (_block, handle) in subs {
                renderer.remove_chunk_mesh(handle);
            }
        }
        // W111: drop this chunk's per-chunk liquid mesh + signature.
        if let Some(kinds) = vx.liquid_mesh_handles.remove(cc) {
            for (_id, h, _tint) in kinds {
                renderer.remove_mesh(h);
            }
        }
        vx.liquid_chunk_sig.remove(cc);
        vx.liquid_remesh_backlog.remove(cc);
    }
    _ms_mesh = _t.elapsed().as_secs_f32() * 1000.0;
    let _t = std::time::Instant::now();
    // Persist edits to disk on a background thread.
    if matches!(vx.net, NetRole::Server { .. } | NetRole::Offline)
        && let Some(slug) = vx.current_world_slug.clone()
    {
        let dim = vx.current_dim.clone();
        // W333: persist EVERY unloaded chunk, not just dirty ones.
        // The W250 dirty-only rule made a RETURN flight regenerate
        // the whole ring from scratch (~13 ms a chunk on a worker)
        // while MC just reads its region files back (~0.1 ms) - the
        // user's "I outfly the world loading" report. The workers'
        // W46.25 disk-first path already prefers the saved file, so
        // writing everything turns revisits into disk reads. Writes
        // compress + land on the detached unload_writer thread; only
        // the payload snapshot happens here.
        // W350: the known-on-disk SET replaces the per-chunk file
        // stat - ~700 stats per rescan cost 50-135 ms on the main
        // thread at the world edge (Windows stat + AV latency), the
        // R14 "extreme drops while generating" hitches.
        let dirty_unload: std::collections::HashSet<ChunkCoord> = to_unload
            .iter()
            .filter(|cc| {
                vx.world.dirty().contains(*cc)
                    || !vx.disk_backed.contains(*cc)
            })
            .copied()
            .collect();
        for cc in &dirty_unload {
            vx.disk_backed.insert(*cc);
        }
        for cc in &dirty_unload {
            vx.world.clear_dirty_chunk(*cc);
        }
        // W233: ONE pass per side map for the whole batch (the
        // per-chunk build_chunk_payload scanned every side map per
        // chunk = O(unloaded x map), ~18 ms per crossing at VD 12).
        let payloads = world_save::runtime::build_chunk_payloads(vx, &dirty_unload);
        if !payloads.is_empty() {
            std::thread::Builder::new()
                .name("voxel_demo_mp::unload_writer".to_string())
                .spawn(move || {
                    for payload in payloads {
                        let coord = payload.coord;
                        if let Err(err) =
                            world_save::write_chunk_payload(&slug, &dim, &payload)
                        {
                            tracing::warn!(
                                coord = ?coord,
                                error = %err,
                                "world_save: async unload-save failed",
                            );
                        }
                    }
                })
                .ok();
        }
    }
    // Prune side maps for unloaded chunk volumes.
    //
    // W198.15: ONE pass per map, not one per unloaded chunk. The old
    // loop ran a full `retain()` for EVERY chunk in `to_unload`, i.e.
    // O(unloaded * all_positions) - a multi-million-op main-thread
    // spike, the travel judder.
    _ms_payload = _t.elapsed().as_secs_f32() * 1000.0;
    let _t = std::time::Instant::now();
    // W232: prune side maps against the KEEP BOX, not the `to_unload`
    // set. A cell belongs to an unloaded volume iff its chunk sits
    // outside the keep ring (the maps only ever hold cells of loaded
    // chunks, and any stale outside-the-box leftovers SHOULD go too),
    // so the per-entry test is four integer compares in block space -
    // no div_euclid, no [i32;3] SipHash, no HashSet probe. The old
    // `to_unload.contains(&chunk_of(p))` closures cost 24-42 ms per
    // chunk-boundary crossing over ocean (8 maps x up to 1.3M liquid
    // entries) = the user's "streaming causes frame drops".
    let min_bx = (player_chunk[0] - unload_dist) * cs;
    let max_bx = (player_chunk[0] + unload_dist) * cs + (cs - 1);
    let min_bz = (player_chunk[2] - unload_dist) * cs;
    let max_bz = (player_chunk[2] + unload_dist) * cs + (cs - 1);
    let in_keep = move |p: &[i32; 3]| {
        p[0] >= min_bx
            && p[0] <= max_bx
            && p[2] >= min_bz
            && p[2] <= max_bz
    };
    vx.emitter_positions.retain(|p, _| in_keep(p));
    vx.torch_positions.retain(|p| in_keep(p));
    // W282: portal swirl cells follow their chunks out (the scan
    // re-adds them on stream-in).
    // W311: portals are NOT distance-pruned - the round-trip link
    // search (128-block MC POI radius) must see portals whose chunks
    // streamed out. The set stays tiny (a few cells per frame built).
    vx.torch_orient.retain(|p, _| in_keep(p));
    vx.fires.retain(|p, _| in_keep(p));
    vx.snow_positions.retain(|p, _| in_keep(p));
    vx.chest_render_positions.retain(|p| in_keep(p));
    vx.liquid_positions.retain(|p| in_keep(p));
    vx.liquid_level.retain(|p, _| in_keep(p));
    // W198.15: the lava subset was never pruned on unload, leaving
    // stale entries for unloaded chunks. W267: column-keyed - drop
    // whole columns outside the keep rect (the fire tick still
    // re-validates surviving entries).
    let min_cx = player_chunk[0] - unload_dist;
    let max_cx = player_chunk[0] + unload_dist;
    let min_cz = player_chunk[2] - unload_dist;
    let max_cz = player_chunk[2] + unload_dist;
    vx.lava_columns.retain(|c, _| {
        c[0] >= min_cx && c[0] <= max_cx && c[1] >= min_cz && c[1] <= max_cz
    });
    // W242.2: spawner index + tick state follow their chunks out; the
    // worker scan re-adds the positions when the chunk streams back
    // (the delay countdown restarts, like a freshly-loaded MC chunk
    // whose block entity re-inits from defaults when NBT is absent).
    vx.spawner_positions.retain(|p| in_keep(p));
    vx.spawner_states.retain(|p, _| in_keep(p));
    vx.spawner_kinds.retain(|p, _| in_keep(p));
    vx.spawner_spin.retain(|p, _| in_keep(p));
    for cc in &to_unload {
        vx.world.remove_chunk(*cc);
    }
    let _ms_retain = _t.elapsed().as_secs_f32() * 1000.0;
    let _ms_all = _t_all.elapsed().as_secs_f32() * 1000.0;
    if _ms_all > 5.0 {
        tracing::warn!(
            total_ms = _ms_all,
            queue_ms = _ms_queue,
            collect_ms = _ms_collect,
            mesh_ms = _ms_mesh,
            payload_ms = _ms_payload,
            retain_ms = _ms_retain,
            unloaded = to_unload.len(),
            "W232: slow chunk-radius rescan"
        );
    }
}

/// W321: queue missing LOD shells in the `view_dist..=lod_dist`
/// ring, drop shells that scrolled out of it, and evict shells whose
/// column now has full-res meshes. Runs only on chunk-boundary
/// crossings (caller is `rescan_chunk_radius`). A disabled ring
/// (`lod_distance` <= view distance, or a non-MC worldgen profile)
/// clears any leftover state.
fn rescan_lod_ring(
    vx: &mut Voxel,
    player_chunk: [i32; 3],
    view_dist: i32,
    cy_min: i32,
    cy_max: i32,
) {
    let lod_dist = (vx.settings.lod_distance as i32)
        .min(crate::lod::LOD_DISTANCE_MAX);
    let mc = vx.current_worldgen_ctx().profile == crate::worldgen::WorldGen::Mc;
    if lod_dist <= view_dist || !mc {
        if !vx.lod_meshes.is_empty() || !vx.lod_pending.is_empty() {
            let Voxel {
                renderer,
                lod_meshes,
                lod_pending,
                lod_pending_set,
                ..
            } = vx;
            crate::lod::clear_lod_state(
                renderer.as_mut(),
                lod_meshes,
                lod_pending,
                lod_pending_set,
            );
        }
        return;
    }
    let view_sq = view_dist * view_dist;
    let lod_sq = lod_dist * lod_dist;
    // Queue missing tiles in the ring band.
    for dz in -lod_dist..=lod_dist {
        for dx in -lod_dist..=lod_dist {
            let dist_sq = dx * dx + dz * dz;
            if dist_sq <= view_sq || dist_sq > lod_sq {
                continue;
            }
            let key = [player_chunk[0] + dx, player_chunk[2] + dz];
            if !vx.lod_meshes.contains_key(&key)
                && vx.lod_pending_set.insert(key)
            {
                vx.lod_pending.push(crate::lod::PendingLodTile {
                    neg_dist_sq: -dist_sq,
                    key,
                });
            }
        }
    }
    // Drop pending entries that scrolled out of the ring (their
    // stale heap entries skip at dispatch via this set).
    vx.lod_pending_set.retain(|k| {
        let dx = k[0] - player_chunk[0];
        let dz = k[1] - player_chunk[2];
        let d = dx * dx + dz * dz;
        d > view_sq && d <= lod_sq
    });
    // W333: FAR ring (level 2) in 4x4-chunk tile space, spanning
    // lod_dist..lod2_dist. Preliminary-surface tiles are near-free
    // to build, so the whole 256-chunk horizon can queue at once.
    let lod2_dist = (vx.settings.lod2_distance as i32)
        .min(crate::lod::LOD2_DISTANCE_MAX);
    let span = crate::lod::LOD2_TILE_CHUNKS;
    if lod2_dist > lod_dist {
        let t_min_x = (player_chunk[0] - lod2_dist).div_euclid(span);
        let t_max_x = (player_chunk[0] + lod2_dist).div_euclid(span);
        let t_min_z = (player_chunk[2] - lod2_dist).div_euclid(span);
        let t_max_z = (player_chunk[2] + lod2_dist).div_euclid(span);
        let lod2_sq = lod2_dist * lod2_dist;
        let inner_sq = lod_dist * lod_dist;
        for tzk in t_min_z..=t_max_z {
            for txk in t_min_x..=t_max_x {
                let cdx = txk * span + span / 2 - player_chunk[0];
                let cdz = tzk * span + span / 2 - player_chunk[2];
                let d = cdx * cdx + cdz * cdz;
                if d <= inner_sq || d > lod2_sq {
                    continue;
                }
                let key = [txk, tzk];
                if !vx.lod2_meshes.contains_key(&key)
                    && vx.lod2_pending_set.insert(key)
                {
                    vx.lod2_pending.push(crate::lod::PendingLodTile {
                        neg_dist_sq: -d,
                        key,
                    });
                }
            }
        }
        vx.lod2_pending_set.retain(|k| {
            let cdx = k[0] * span + span / 2 - player_chunk[0];
            let cdz = k[1] * span + span / 2 - player_chunk[2];
            let d = cdx * cdx + cdz * cdz;
            d > inner_sq && d <= lod2_sq
        });
        let keep2 = lod2_dist + 2 * span;
        let keep2_sq = keep2 * keep2;
        let mut drop2: Vec<[i32; 2]> = Vec::new();
        for k in vx.lod2_meshes.keys() {
            let cdx = k[0] * span + span / 2 - player_chunk[0];
            let cdz = k[1] * span + span / 2 - player_chunk[2];
            let d = cdx * cdx + cdz * cdz;
            if d > keep2_sq || d <= inner_sq {
                drop2.push(*k);
            }
        }
        if !drop2.is_empty()
            && let Some(renderer) = vx.renderer.as_mut()
        {
            for k in drop2 {
                if let Some(t) = vx.lod2_meshes.remove(&k) {
                    if let Some(h) = t.opaque {
                        renderer.remove_chunk_mesh(h);
                    }
                    if let Some(h) = t.water {
                        renderer.remove_chunk_mesh(h);
                    }
                }
            }
        }
    } else if !vx.lod2_meshes.is_empty() || !vx.lod2_pending.is_empty() {
        let Voxel {
            renderer,
            lod2_meshes,
            lod2_pending,
            lod2_pending_set,
            ..
        } = vx;
        crate::lod::clear_lod_state(
            renderer.as_mut(),
            lod2_meshes,
            lod2_pending,
            lod2_pending_set,
        );
    }
    // Unload shells: out past the ring hysteresis, or superseded by
    // full-res meshes anywhere in their column.
    let keep_sq = (lod_dist + 2) * (lod_dist + 2);
    let mut drop_keys: Vec<[i32; 2]> = Vec::new();
    for (k, t) in vx.lod_meshes.iter() {
        let dx = k[0] - player_chunk[0];
        let dz = k[1] - player_chunk[2];
        let d = dx * dx + dz * dz;
        if d > keep_sq {
            drop_keys.push(*k);
            continue;
        }
        if d <= view_sq {
            // W353: release the shell only once the SURFACE chunk of
            // the column has a full-res mesh. "Any chunk meshed"
            // dropped the shell when a cave slice arrived first and
            // the visible terrain blinked out for a beat (the R17
            // "remove the LOD only after the real surface shows").
            let scy = (t.y_surface.floor() as i32)
                .div_euclid(infengine_voxel::CHUNK_SIZE as i32)
                .clamp(cy_min, cy_max);
            let covered =
                vx.chunk_meshes.contains_key(&[k[0], scy, k[1]])
                    || (cy_min..=cy_max).all(|cy| {
                        vx.chunk_meshes.contains_key(&[k[0], cy, k[1]])
                    });
            if covered {
                drop_keys.push(*k);
            }
        }
    }
    if !drop_keys.is_empty()
        && let Some(renderer) = vx.renderer.as_mut()
    {
        for k in drop_keys {
            if let Some(t) = vx.lod_meshes.remove(&k) {
                if let Some(h) = t.opaque {
                    renderer.remove_chunk_mesh(h);
                }
                if let Some(h) = t.water {
                    renderer.remove_chunk_mesh(h);
                }
            }
        }
    }
}
