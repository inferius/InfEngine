//! W47.6 - net tick dispatch, relocated from main.rs.
//!
//! `tick` is the per-frame net-IO entry point. Pulls UDP packets,
//! routes server-side messages (chunk loads, mob updates, chest
//! ops, ...) into the appropriate Voxel side-maps, and pushes
//! pending edits + the player's own pose back upstream. The body
//! is large (~940 LOC) because it covers every protocol arm in
//! one place; per-arm helpers live in this module's sub-files
//! (none yet - waiting on the next refactor pass).

#![allow(clippy::too_many_lines)] // dispatch fn covers every arm in one body

// W47.6: dispatch lives inside the net module so `super::*` pulls in
// every Msg + the NetRole enum directly. The remaining crate-rooted
// imports cover items that live OUTSIDE net (block ids, drop / mob
// helpers, recipe table, time / damage consts, ...). The list looks
// long because tick_net touches almost every subsystem; the
// alternative is per-item `crate::X` prefixes scattered through 940
// lines of dispatch body, which the parent commit moved out of
// main.rs in one shot.
use super::*;
use crate::*;

pub(crate) fn tick(vx: &mut crate::Voxel) {
        vx.net_tick = vx.net_tick.wrapping_add(1);
        let net_tick = vx.net_tick;
        let player_pos = vx.player_pos;
        let player_yaw = vx.player_yaw;
        let player_pitch = vx.player_pitch;
        // Retransmit period (in net ticks) for pending unsynced
        // payloads. UDP losses are rare on LAN so resending every
        // tick is wasteful; every 4 ticks is roughly 67 ms at
        // 60 Hz which still feels snappy.
        const RETRY_PERIOD: u64 = 4;
        let do_retry = net_tick.is_multiple_of(RETRY_PERIOD);
        // Server-side heartbeat to the master server. Cheap so we
        // can do it inline.
        if let NetRole::Server { server, poses, .. } = &vx.net
            && let Some(token) = vx.master_token
            && vx
                .last_heartbeat
                .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(5))
        {
            use infengine_net::master::{MasterRequest, request_blocking};
            let players = poses.len().saturating_sub(1) as u32 + server.client_count() as u32;
            let req = MasterRequest::Heartbeat { token, players };
            let addr = vx.master_addr.clone();
            // Fire-and-forget so the server tick never stalls on a
            // network round trip. We don't care about the response;
            // a stale token just causes the next heartbeat to log
            // a warning.
            std::thread::spawn(move || {
                let _ = request_blocking(&addr, &req, std::time::Duration::from_millis(500));
            });
            vx.last_heartbeat = Some(std::time::Instant::now());
        }
        // Drain incoming bytes + edits to apply. Collect first so the
        // borrow on `vx.net` releases before we mutate the world.
        let mut to_apply: Vec<EditMsg> = Vec::new();
        // Validated grass voxels from inbound vx.path requests.
        // Filled in the Server arm + drained after vx.net's
        // borrow releases.
        let mut path_apply: Vec<[i32; 3]> = Vec::new();
        // Inventory snapshot the client received this tick. Applied
        // to `vx.inventory` / `vx.craft_grid` after the
        // `&mut vx.net` borrow releases.
        let mut client_snapshot: Option<InvSnapshotMsg> = None;
        // Mob pose updates received this tick (client). Folded into
        // `vx.remote_mobs` after the match exits.
        let mut mob_updates: Vec<MobMsg> = Vec::new();
        // W134: minecart snapshots / removals (client). Folded into
        // `vx.remote_carts` after the match exits.
        let mut cart_updates: Vec<crate::net::CartMsg> = Vec::new();
        let mut cart_removals: Vec<u32> = Vec::new();
        // W140: arrow snapshots / removals (client).
        let mut arrow_updates: Vec<crate::net::ArrowMsg> = Vec::new();
        let mut arrow_removals: Vec<u32> = Vec::new();
        let mut fireball_updates: Vec<crate::net::FireballMsg> = Vec::new();
        let mut fireball_removals: Vec<u32> = Vec::new();
        let mut tnt_updates: Vec<crate::net::TntMsg> = Vec::new();
        let mut tnt_removals: Vec<u32> = Vec::new();
        let mut player_removals: Vec<u32> = Vec::new();
        // Pose updates collected this tick. Folded into the
        // smoothing buffer (`remote_interp`) after the net match
        // arm releases its borrow on `vx.net`.
        let mut interp_updates: Vec<PlayerMsg> = Vec::new();
        // Local damage accumulator drained after the match arm
        // releases its borrow on `vx.net`. Used by:
        //  - vx.damage (client side): server-pushed PvP damage.
        //  - vx.pvp_hit (server side, host = target): a remote
        //    client whacked the host.
        let mut pvp_self_damage: f32 = 0.0;
        let mut pvp_self_heal: f32 = 0.0;
        // Chat lines received this tick. Appended to `chat_log`
        // after the net match arm releases its borrow on `vx.net`.
        let mut chat_inbox: Vec<String> = Vec::new();
        // Roster update from the server's `vx.names` broadcast.
        // Last-write-wins per tick: only the latest snapshot is
        // applied to `player_names` after the match arm closes.
        let mut names_update: Option<Vec<(u32, String)>> = None;
        // Server-pushed teleport. Drained after the net borrow
        // releases so we can write `vx.player_pos` directly.
        let mut teleport_request: Option<TeleportMsg> = None;
        // Latest authoritative day_time from the server. Last
        // wins so a mid-tick burst doesn't bounce the sky.
        let mut day_time_update: Option<f32> = None;
        let mut seed_update: Option<u32> = None;
        let mut weather_update: Option<u8> = None;
        let mut thunder_update: Option<bool> = None;
        let mut fang_spawns: Vec<([f32; 3], f32)> = Vec::new();
        let mut effect_updates: Vec<(u8, f32)> = Vec::new();
        let mut snow_updates: Vec<([i32; 3], u8)> = Vec::new();
        // Liquid level updates received this tick. Drained into
        // `vx.liquid_level` after the net match arm releases
        // its borrow on `vx.net`. Client-only path: the host
        // sets the level directly via the tick passes; the
        // broadcast is only consumed by clients so their slope
        // mesh matches the host's depth ramp.
        let mut liquid_level_updates: Vec<([i32; 3], u8)> = Vec::new();
        // Drop entity updates received this tick. Client-only -
        // the host is the source of truth and never receives
        // these. Drained into `vx.dropped_items` after the
        // net match arm releases its borrow on `vx.net`.
        let mut drop_spawns: Vec<DropSpawnMsg> = Vec::new();
        let mut drop_removes: Vec<u32> = Vec::new();
        // Torch lean orientation updates received this tick. Drained
        // into `vx.torch_orient` after the net match arm releases
        // its borrow on `vx.net`. Server arm also rebroadcasts so
        // every peer renders the matching wall mount.
        let mut torch_orient_updates: Vec<([i32; 3], u8)> = Vec::new();
        // W125: door place/toggle updates (lower pos, facing, open).
        let mut door_state_updates: Vec<([i32; 3], u8, bool)> = Vec::new();
        // W126: chest facing records (pos, facing).
        let mut chest_facing_updates: Vec<([i32; 3], u8)> = Vec::new();
        // W143: stored rail shape updates (pos, shape; 0xFF=removed).
        let mut rail_shape_updates: Vec<([i32; 3], u8)> = Vec::new();
        // W5.3a/b: incoming chest + furnace state snapshots from
        // the host. Applied below to refresh the local chest /
        // furnace maps so the open UI mirrors the authoritative
        // contents.
        let mut chest_state_updates: Vec<net::ChestStateMsg> = Vec::new();
        let mut furnace_state_updates: Vec<net::FurnaceStateMsg> = Vec::new();
        let mut kick_reason: Option<String> = None;
        // Mob damage requests + events to process after the match
        // arm closes (damage_mob borrows the whole `&mut vx`).
        let mut mob_hits_to_apply: Vec<(u32, u32)> = Vec::new();
        let mut mob_events: Vec<MobEventMsg> = Vec::new();
        let mut mob_removals: Vec<u32> = Vec::new();
        // W5.3a/b: chest + furnace messages are collected here so
        // the apply pass can re-borrow `&mut vx` after the
        // `match &mut vx.net` block closes.
        let mut chest_opens_outer: Vec<(u32, net::ChestOpenMsg)> = Vec::new();
        let mut chest_acts_outer: Vec<(u32, net::ChestActMsg)> = Vec::new();
        let mut chest_closes_outer: Vec<(u32, net::ChestCloseMsg)> = Vec::new();
        let mut furnace_opens_outer: Vec<(u32, net::FurnaceOpenMsg)> = Vec::new();
        let mut furnace_acts_outer: Vec<(u32, net::FurnaceActMsg)> = Vec::new();
        let mut furnace_closes_outer: Vec<(u32, net::FurnaceCloseMsg)> = Vec::new();
        // W468: container-cursor stacks of disconnected viewers
        // that didn't fit back into their inventory. Spawned as
        // floor drops after the `&mut vx.net` borrow releases.
        let mut disconnect_spills: Vec<([i32; 3], crate::item::ItemStack)> =
            Vec::new();
        let cfg = bincode::config::standard();
        match &mut vx.net {
            NetRole::Offline => return,
            NetRole::Server {
                server,
                poses,
                inventories,
                pending_inv_sends,
            } => {
                server.poll();
                // Track host pose under id 0.
                poses.insert(
                    0,
                    PlayerMsg {
                        id: 0,
                        pos: [player_pos.x, player_pos.y, player_pos.z],
                        yaw: player_yaw,
                        pitch: player_pitch,
                    },
                );
                // Raw inbound per client: (client_id, edit) +
                // (client_id, drag action). Resolved after the
                // drain loop so we can validate against the
                // authoritative inventory.
                let mut raw_edits: Vec<(u32, EditMsg)> = Vec::new();
                let mut path_requests: Vec<(u32, [i32; 3])> = Vec::new();
                let mut raw_drags: Vec<(u32, InvActionMsg)> = Vec::new();
                // W5.3a/b: borrow the outer-scope collectors so
                // the apply pass after the match has data to act on.
                {
                    let mut inbox = net::ServerInboxCtx {
                        day_time: &mut vx.day_time,
                        last_day_broadcast: &mut vx.last_day_broadcast,
                        world_locked: vx.world_locked,
                        host_player_pos: [
                            player_pos.x,
                            player_pos.y,
                            player_pos.z,
                        ],
                        raw_edits: &mut raw_edits,
                        path_requests: &mut path_requests,
                        raw_drags: &mut raw_drags,
                        interp_updates: &mut interp_updates,
                        mob_hits_to_apply: &mut mob_hits_to_apply,
                        torch_orient_updates: &mut torch_orient_updates,
                        door_state_updates: &mut door_state_updates,
                        chest_facing_updates: &mut chest_facing_updates,
                        rail_shape_updates: &mut rail_shape_updates,
                        chat_inbox: &mut chat_inbox,
                        pvp_self_damage: &mut pvp_self_damage,
                        chest_opens: &mut chest_opens_outer,
                        chest_acts: &mut chest_acts_outer,
                        chest_closes: &mut chest_closes_outer,
                        furnace_opens: &mut furnace_opens_outer,
                        furnace_acts: &mut furnace_acts_outer,
                        furnace_closes: &mut furnace_closes_outer,
                    };
                    net::process_server_inbox(
                        server,
                        poses,
                        inventories,
                        pending_inv_sends,
                        PVP_REACH,
                        PVP_DAMAGE,
                        DAY_LENGTH_S,
                        &|idx| RECIPES.get(idx).map(|r| r.inputs),
                        &mut inbox,
                    );
                }
                // W5.3a/b: chest + furnace messages are applied
                // AFTER the match arm closes so the apply path can
                // re-borrow `&mut vx.net` to send state snapshots.
                // The vars are visible outside the match because
                // they're declared in this scope's prelude (above).
                // Roster snapshot: use the authoritative connection
                // list (`server.client_ids`) rather than the pose
                // table, so dropped clients are detected as soon as
                // the reaper or an explicit `vx.leave` removes them
                // from the socket layer.
                let current_ids: HashSet<u32> =
                    server.client_ids().into_iter().collect();
                let new_ids: Vec<u32> = current_ids
                    .iter()
                    .filter(|id| !vx.known_client_ids.contains(*id))
                    .copied()
                    .collect();
                let departed_ids: Vec<u32> = vx
                    .known_client_ids
                    .iter()
                    .filter(|id| !current_ids.contains(*id))
                    .copied()
                    .collect();
                // Drop server-side state for clients that left so
                // their pose / inventory don't keep replicating.
                // Persist their inventory by name (if known) so the
                // next save flushes it, then evict.
                for id in &departed_ids {
                    // Grab the name BEFORE we lose track of the
                    // mapping below so the broadcast chat line
                    // reads sensibly.
                    let depart_name = vx
                        .player_names
                        .get(id)
                        .cloned()
                        .or_else(|| server.client_name(*id))
                        .unwrap_or_else(|| format!("p{id}"));
                    // W468: a viewer that disconnects mid-container
                    // gets its authoritative cursor back into the
                    // inventory BEFORE that inventory is persisted;
                    // what doesn't fit spills at the container (or
                    // the last known pose as a fallback).
                    let spill_pos: Option<[i32; 3]> = vx
                        .chest_viewers
                        .iter()
                        .find(|(_, v)| **v == *id)
                        .map(|(p, _)| *p)
                        .or_else(|| {
                            vx.furnace_viewers
                                .iter()
                                .find(|(_, v)| **v == *id)
                                .map(|(p, _)| *p)
                        });
                    vx.chest_viewers.retain(|_, v| *v != *id);
                    vx.furnace_viewers.retain(|_, v| *v != *id);
                    if let Some(held) =
                        vx.container_cursors.remove(id).flatten()
                    {
                        let mut leftover = held.count;
                        if let Some(inv) = inventories.get_mut(id) {
                            leftover =
                                crate::item::stack::drop_into_first_free_stacks(
                                    &mut inv.inventory,
                                    held.clone(),
                                );
                        }
                        if leftover > 0 {
                            let pos = spill_pos.or_else(|| {
                                poses.get(id).map(|p| {
                                    [
                                        p.pos[0].floor() as i32,
                                        p.pos[1].floor() as i32,
                                        p.pos[2].floor() as i32,
                                    ]
                                })
                            });
                            if let Some(pos) = pos {
                                disconnect_spills
                                    .push((pos, held.with_count(leftover)));
                            }
                        }
                    }
                    poses.remove(id);
                    if let Some(inv) = inventories.remove(id)
                        && let Some(name) = server.client_name(*id)
                    {
                        vx.saved_inventories.insert(name, inv);
                    }
                    pending_inv_sends.remove(id);
                    // Tell every still-connected peer to forget
                    // the departed player's cube.
                    let msg = PlayerRemoveMsg { id: *id };
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        server.broadcast_bytes("vx.player_remove", buf);
                    }
                    // Public departure notice. Hits the host's
                    // local log via chat_inbox + broadcasts to
                    // every still-connected peer.
                    let line = format!("* {depart_name} left.");
                    chat_inbox.push(line.clone());
                    if let Ok(buf) = bincode::serde::encode_to_vec(&line, cfg) {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
                if !new_ids.is_empty() && !vx.edit_history.is_empty() {
                    tracing::info!(
                        joined = ?new_ids,
                        edits = vx.edit_history.len(),
                        "rebroadcasting edit history for new client(s)"
                    );
                    for e in &vx.edit_history {
                        if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                }
                // Seed inventories for any newly-joined clients +
                // queue them an initial snapshot so their client UI
                // mirrors the authoritative server state. Restore
                // from the persisted side map when the client's
                // name matches a previous session.
                for id in &new_ids {
                    if !inventories.contains_key(id) {
                        let name = server.client_name(*id);
                        let inv = name
                            .as_ref()
                            .and_then(|n| vx.saved_inventories.remove(n))
                            .unwrap_or_else(ClientInv::starter);
                        inventories.insert(*id, inv);
                    }
                    pending_inv_sends.insert(*id);
                    // Public arrival notice. Same path as the
                    // depart line above; the server's broadcast
                    // pushes to every peer including the new
                    // joiner, so they immediately see who else
                    // is around.
                    let name = server
                        .client_name(*id)
                        .unwrap_or_else(|| format!("p{id}"));
                    let line = format!("* {name} joined.");
                    chat_inbox.push(line.clone());
                    if let Ok(buf) = bincode::serde::encode_to_vec(&line, cfg) {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
                // Roster broadcast: whenever the connected set
                // changes, ship the full id->name table to every
                // client so nameplates / Tab playerlist stay
                // current. Host owns id 0 with its own
                // `player_name`.
                // Send world seed to brand-new joiners so their
                // chunk workers generate matching terrain.
                // W5.1: ship the base seed; clients derive
                // per-dim values via dim_seed() the same way the
                // host does.
                if !new_ids.is_empty()
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(vx.base_seed, cfg)
                {
                    for id in &new_ids {
                        server.send_bytes_to_client(*id, "vx.seed", buf.clone());
                    }
                }
                // Ship current weather to new joiners too.
                if !new_ids.is_empty()
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(vx.weather, cfg)
                {
                    for id in &new_ids {
                        server.send_bytes_to_client(
                            *id,
                            "vx.weather",
                            buf.clone(),
                        );
                    }
                }
                // W460: and the thunder flag.
                if !new_ids.is_empty()
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(vx.thundering, cfg)
                {
                    for id in &new_ids {
                        server.send_bytes_to_client(
                            *id,
                            "vx.thunder",
                            buf.clone(),
                        );
                    }
                }
                if !new_ids.is_empty() || !departed_ids.is_empty() {
                    let mut entries: Vec<(u32, String)> =
                        vec![(0, vx.player_name.clone())];
                    for cid in server.client_ids() {
                        let name = server
                            .client_name(cid)
                            .unwrap_or_else(|| format!("p{cid}"));
                        entries.push((cid, name));
                    }
                    vx.player_names.clear();
                    for (id, n) in &entries {
                        vx.player_names.insert(*id, n.clone());
                    }
                    if let Ok(buf) = bincode::serde::encode_to_vec(&entries, cfg) {
                        server.broadcast_bytes("vx.names", buf);
                    }
                }
                vx.known_client_ids = current_ids;
                // Validate every incoming edit against the sender's
                // inventory + spatial range before accepting it.
                //
                // - BREAK: sender's pose must be within REACH of the
                //   target voxel. Passing the inventory check is
                //   trivial (breaks never consume); the pickup
                //   credit happens here too.
                // - PLACE: sender must have the block AND be within
                //   REACH of the target voxel. Silent reject on
                //   either failure (the next `vx.inv` snapshot
                //   reconciles the predicted client state).
                const REACH: f32 = 6.5;
                for (cid, e) in raw_edits {
                    // Look up the sender's last reported pose; if
                    // we've never seen one, fall back to a sentinel
                    // far away so the range check rejects the edit.
                    let pose = poses.get(&cid).copied().unwrap_or(PlayerMsg {
                        id: cid,
                        pos: [1e9, 1e9, 1e9],
                        yaw: 0.0,
                        pitch: 0.0,
                    });
                    let voxel_center = [
                        e.x as f32 + 0.5,
                        e.y as f32 + 0.5,
                        e.z as f32 + 0.5,
                    ];
                    let dx = pose.pos[0] - voxel_center[0];
                    let dy = pose.pos[1] - voxel_center[1];
                    let dz = pose.pos[2] - voxel_center[2];
                    let dist2 = dx * dx + dy * dy + dz * dz;
                    if dist2 > REACH * REACH {
                        tracing::debug!(
                            client_id = cid,
                            block = e.block,
                            "edit rejected: out of reach"
                        );
                        // Tell the client what the voxel actually
                        // looks like server-side so its prediction
                        // gets reverted on the next tick.
                        let truth = vx.world.get([e.x, e.y, e.z]);
                        let revert = EditMsg {
                            x: e.x,
                            y: e.y,
                            z: e.z,
                            block: truth.0,
                        };
                        if let Ok(buf) = bincode::serde::encode_to_vec(revert, cfg) {
                            server.send_bytes_to_client(cid, "vx.edit", buf);
                        }
                        pending_inv_sends.insert(cid);
                        continue;
                    }
                    if e.block == 0 {
                        // BREAK - capture old block to credit
                        // pickup, then apply.
                        let voxel = [e.x, e.y, e.z];
                        let broken = vx.world.get(voxel);
                        if broken != BlockId::AIR
                            && let Some(inv) = inventories.get_mut(&cid)
                            && crate::inventory::inv_pickup(&mut inv.inventory, broken)
                        {
                            pending_inv_sends.insert(cid);
                        }
                        to_apply.push(e);
                    } else {
                        // PLACE - require + consume from sender's
                        // inventory. Silently drop the placement if
                        // they don't have it (anti-cheat / desync).
                        let block = BlockId(e.block);
                        let ok = inventories
                            .get_mut(&cid)
                            .is_some_and(|inv| inv_consume(&mut inv.inventory, block));
                        if ok {
                            pending_inv_sends.insert(cid);
                            to_apply.push(e);
                        } else {
                            // Reverted: send the voxel's true state
                            // so the client's predicted block
                            // vanishes; also push the snapshot to
                            // restore the (unchanged) inventory.
                            let truth = vx.world.get([e.x, e.y, e.z]);
                            let revert = EditMsg {
                                x: e.x,
                                y: e.y,
                                z: e.z,
                                block: truth.0,
                            };
                            if let Ok(buf) = bincode::serde::encode_to_vec(revert, cfg) {
                                server.send_bytes_to_client(cid, "vx.edit", buf);
                            }
                            pending_inv_sends.insert(cid);
                        }
                    }
                }
                // Apply incoming drag&drop intents on the server's
                // authoritative inventory.
                for (cid, act) in raw_drags {
                    if let Some(inv) = inventories.get_mut(&cid) {
                        inv_apply_drag(inv, act.from.into(), act.to.into(), act.split);
                        pending_inv_sends.insert(cid);
                    }
                }
                // Server also records every received edit so future
                // joiners get a complete history.
                for e in &to_apply {
                    vx.edit_history.push(*e);
                }
                // Broadcast each player's pose (one packet each;
                // future: batch). Also rebroadcast any incoming
                // edits we'll apply below.
                for p in poses.values() {
                    if let Ok(buf) = bincode::serde::encode_to_vec(p, cfg) {
                        server.broadcast_bytes("vx.player", buf);
                    }
                }
                for e in &to_apply {
                    if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
                // Validate the deferred shovel-path requests
                // here while `poses` is still in scope; the
                // filtered voxel list is consumed by the
                // post-arm world-mutation pass.
                for (cid, v) in path_requests.iter() {
                    let pos = poses
                        .get(cid)
                        .copied()
                        .map(|p| p.pos)
                        .unwrap_or([1e9, 1e9, 1e9]);
                    let dx = pos[0] - v[0] as f32 - 0.5;
                    let dy = pos[1] - v[1] as f32 - 0.5;
                    let dz = pos[2] - v[2] as f32 - 0.5;
                    if dx * dx + dy * dy + dz * dz <= 49.0 {
                        path_apply.push(*v);
                    }
                }
                // Push authoritative inventory snapshots to every
                // client whose state changed this tick.
                for cid in pending_inv_sends.drain() {
                    if let Some(inv) = inventories.get(&cid) {
                        let snap = inv.to_snapshot();
                        if let Ok(buf) = bincode::serde::encode_to_vec(&snap, cfg) {
                            server.send_bytes_to_client(cid, "vx.inv", buf);
                        }
                    }
                }
                // Authoritative day_time broadcast on a slow
                // cadence (~once per second). Clients update
                // their local clock so the sun stays in sync
                // even if /day or /night was used to skip ahead.
                let now = std::time::Instant::now();
                let due = vx
                    .last_day_broadcast
                    .map(|t| now.duration_since(t).as_secs_f32() >= 1.0)
                    .unwrap_or(true);
                if due
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(vx.day_time, cfg)
                {
                    server.broadcast_bytes("vx.day", buf);
                    vx.last_day_broadcast = Some(now);
                }
            }
            NetRole::Client {
                client,
                remote,
                pending_edits,
                pending_inv_actions,
            } => {
                client.poll();
                let my_id = client.client_id;
                {
                    let mut inbox = net::ClientInboxCtx {
                        to_apply: &mut to_apply,
                        client_snapshot: &mut client_snapshot,
                        mob_updates: &mut mob_updates,
                        mob_events: &mut mob_events,
                        mob_removals: &mut mob_removals,
                        cart_updates: &mut cart_updates,
                        cart_removals: &mut cart_removals,
                        arrow_updates: &mut arrow_updates,
                        arrow_removals: &mut arrow_removals,
                        fireball_updates: &mut fireball_updates,
                        fireball_removals: &mut fireball_removals,
                        tnt_updates: &mut tnt_updates,
                        tnt_removals: &mut tnt_removals,
                        player_removals: &mut player_removals,
                        interp_updates: &mut interp_updates,
                        pvp_self_damage: &mut pvp_self_damage,
                        pvp_self_heal: &mut pvp_self_heal,
                        chat_inbox: &mut chat_inbox,
                        names_update: &mut names_update,
                        teleport_request: &mut teleport_request,
                        day_time_update: &mut day_time_update,
                        seed_update: &mut seed_update,
                        weather_update: &mut weather_update,
                        thunder_update: &mut thunder_update,
                        fang_spawns: &mut fang_spawns,
                        effect_updates: &mut effect_updates,
                        snow_updates: &mut snow_updates,
                        liquid_level_updates: &mut liquid_level_updates,
                        drop_spawns: &mut drop_spawns,
                        drop_removes: &mut drop_removes,
                        torch_orient_updates: &mut torch_orient_updates,
                        door_state_updates: &mut door_state_updates,
                        chest_facing_updates: &mut chest_facing_updates,
                        rail_shape_updates: &mut rail_shape_updates,
                        kick_reason: &mut kick_reason,
                        chest_state_updates: &mut chest_state_updates,
                        furnace_state_updates: &mut furnace_state_updates,
                    };
                    net::process_client_inbox(
                        client,
                        remote,
                        pending_edits,
                        pending_inv_actions,
                        &mut inbox,
                    );
                }
                // Re-send pending edits / inventory drags on a
                // throttled schedule. Server-applied edits are
                // idempotent (set+broadcast), so duplicate arrivals
                // are correctness-safe, but we don't need them every
                // single tick - `RETRY_PERIOD` gates the resend.
                if do_retry {
                    for e in pending_edits.iter() {
                        if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                            let _ = client.send_bytes("vx.edit", buf);
                        }
                    }
                    for a in pending_inv_actions.iter() {
                        if let Ok(buf) = bincode::serde::encode_to_vec(a, cfg) {
                            let _ = client.send_bytes("vx.inv_act", buf);
                        }
                    }
                }
                // Send own pose upstream.
                let me = PlayerMsg {
                    id: my_id.unwrap_or(0),
                    pos: [player_pos.x, player_pos.y, player_pos.z],
                    yaw: player_yaw,
                    pitch: player_pitch,
                };
                if let Ok(buf) = bincode::serde::encode_to_vec(me, cfg) {
                    let _ = client.send_bytes("vx.player", buf);
                }
            }
        }
        // Apply collected edits after the borrow on `vx.net`
        // releases - both paths funnel through here so the
        // chunk-rebuild logic only lives in one place.
        for e in to_apply {
            let voxel = [e.x, e.y, e.z];
            vx.world_set_tracked(voxel, BlockId(e.block));
            let coords = crate::Voxel::affected_chunks(voxel);
            vx.rebuild_chunks(&coords);
        }
        // Apply validated shovel-path requests (server side):
        // grass voxels become dirt + broadcast as vx.edit.
        if !path_apply.is_empty() {
            let cfg = bincode::config::standard();
            for v in path_apply.into_iter() {
                if vx.world.get(v).0 != 3 {
                    continue;
                }
                vx.world_set_tracked(v, BlockId(2));
                let coords = crate::Voxel::affected_chunks(v);
                vx.rebuild_chunks(&coords);
                let edit = EditMsg {
                    x: v[0],
                    y: v[1],
                    z: v[2],
                    block: 2,
                };
                vx.edit_history.push(edit);
                if let NetRole::Server { server, .. } = &mut vx.net
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(edit, cfg)
                {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
        }
        // Authoritative inventory snapshot from the server: replace
        // the local view wholesale. Anything the client mispredicted
        // (drag intent the server rejected, pickup the server
        // assigned to someone else) reconciles on this single
        // assignment.
        if let Some(snap) = client_snapshot {
            // Diff vs the pre-snapshot inventory so server-driven
            // pickups generate toasts the same way local pickups
            // do. Counts blocks by id; any positive delta becomes
            // a `+N` toast.
            let mut pre: HashMap<u16, u32> = HashMap::new();
            for st in vx.inventory.iter().flatten() {
                *pre.entry(st.id).or_insert(0) += st.count as u32;
            }
            // W468: the wire snapshot is stack-native - adopt the
            // authoritative slots wholesale, components included.
            for (i, s) in snap.inventory.into_iter().enumerate() {
                if i < vx.inventory.len() {
                    vx.inventory[i] = s;
                }
            }
            for (i, s) in snap.craft.iter().enumerate() {
                if i < vx.craft_grid.len() {
                    vx.craft_grid[i] = s.map(|(b, c)| (BlockId(b), c));
                }
            }
            let mut post: HashMap<u16, u32> = HashMap::new();
            for st in vx.inventory.iter().flatten() {
                *post.entry(st.id).or_insert(0) += st.count as u32;
            }
            for (id, after) in post {
                let before = pre.get(&id).copied().unwrap_or(0);
                if after > before {
                    let delta = (after - before).min(u8::MAX as u32) as u8;
                    let block = BlockId(id);
                    let now = std::time::Instant::now();
                    if let Some(last) = vx.pickup_toasts.last_mut()
                        && last.1 == block
                        && now.duration_since(last.0).as_secs_f32() < 1.5
                    {
                        last.0 = now;
                        last.2 = last.2.saturating_add(delta);
                    } else {
                        vx.pickup_toasts.push((now, block, delta));
                    }
                }
            }
            if vx.pickup_toasts.len() > 16 {
                let drop = vx.pickup_toasts.len() - 16;
                vx.pickup_toasts.drain(0..drop);
            }
        }
        for m in mob_updates {
            vx.remote_mobs.insert(m.id, m);
        }
        // W134: latest cart poses win; removals drop the entry.
        for m in cart_updates {
            vx.remote_carts.insert(m.id, m);
        }
        for id in cart_removals {
            vx.remote_carts.remove(&id);
        }
        // W140: same fold for arrows.
        for m in arrow_updates {
            vx.remote_arrows.insert(m.id, m);
        }
        for id in arrow_removals {
            vx.remote_arrows.remove(&id);
        }
        // W337: same fold for ghast fireballs.
        for m in fireball_updates {
            vx.remote_fireballs.insert(m.id, m);
        }
        for id in fireball_removals {
            vx.remote_fireballs.remove(&id);
        }
        // W338: primed TNT snapshots.
        for m in tnt_updates {
            vx.remote_tnt.insert(m.id, m);
        }
        for id in tnt_removals {
            vx.remote_tnt.remove(&id);
        }
        // Fold incoming pose updates into the interpolation buffer
        // so the render path can lerp toward each new snapshot
        // instead of teleporting.
        for p in interp_updates {
            vx.remote_interp
                .entry(p.id)
                .and_modify(|r| r.push(p))
                .or_insert_with(|| RemoteInterp::new(p));
        }
        // Drain chat lines collected while the `net` borrow was
        // held in the receive match. They render in the HUD chat
        // log with a 10s fade.
        if !chat_inbox.is_empty() {
            let now = std::time::Instant::now();
            for line in chat_inbox {
                // Mute filter: drop lines whose `<sender>` tag
                // or `* sender` prefix matches a muted name.
                if !vx.muted_players.is_empty()
                    && line_is_muted(&line, &vx.muted_players)
                {
                    continue;
                }
                vx.chat_log.push((now, line));
            }
            // Cap log length so very long sessions don't grow
            // unbounded.
            if vx.chat_log.len() > CHAT_LOG_MAX {
                let drop = vx.chat_log.len() - CHAT_LOG_MAX;
                vx.chat_log.drain(0..drop);
            }
        }
        // Apply the latest names roster (client-side; the server
        // already updates `player_names` inline when it builds the
        // broadcast).
        if let Some(entries) = names_update {
            vx.player_names.clear();
            for (id, n) in entries {
                vx.player_names.insert(id, n);
            }
        }
        if let Some(dt) = day_time_update {
            vx.day_time = dt.rem_euclid(DAY_LENGTH_S);
        }
        if let Some(w) = weather_update {
            vx.weather = w;
        }
        if let Some(t) = thunder_update {
            vx.thundering = t;
        }
        for (pos, warmup) in fang_spawns {
            crate::mob::fangs::spawn_remote(vx, pos, warmup);
        }
        // W465: host-sent status effects land on the local pool -
        // the same MC durations the host applies to itself.
        for (kind, secs) in effect_updates {
            match kind {
                1 => {
                    vx.player_effects.poison =
                        vx.player_effects.poison.max(secs);
                }
                2 => {
                    vx.player_effects.slowness =
                        vx.player_effects.slowness.max(secs);
                }
                _ => {}
            }
        }
        for (v, layers) in snow_updates {
            vx.snow_positions.insert(v, layers.clamp(1, 8));
            // W316: layers render from the chunk mesh now.
            let coords = crate::Voxel::affected_chunks(v);
            vx.rebuild_chunks(&coords);
        }
        for (v, orient) in torch_orient_updates {
            vx.torch_orient.insert(v, orient);
        }
        // W126: chest facing is render-side state; the entity pass
        // reads the live map (no remesh needed).
        for (v, facing) in chest_facing_updates {
            vx.chest_facing.insert(v, facing);
        }
        // W125: apply door state + remesh the affected chunk(s) so
        // the panel flips on every peer.
        for (v, facing, open) in door_state_updates {
            vx.door_facing.insert(v, facing);
            if open {
                vx.door_open.insert(v);
            } else {
                vx.door_open.remove(&v);
            }
            let cs = infengine_voxel::CHUNK_SIZE as i32;
            let mut ccs = vec![[
                v[0].div_euclid(cs),
                v[1].div_euclid(cs),
                v[2].div_euclid(cs),
            ]];
            let ucc = [
                v[0].div_euclid(cs),
                (v[1] + 1).div_euclid(cs),
                v[2].div_euclid(cs),
            ];
            if !ccs.contains(&ucc) {
                ccs.push(ucc);
            }
            vx.rebuild_chunks(&ccs);
        }
        // W143: apply stored rail shapes from peers (insert/remove +
        // remesh of every touched chunk).
        if !rail_shape_updates.is_empty() {
            crate::world::place_break::rail_place::apply_shape_changes(
                vx,
                &rail_shape_updates,
            );
        }
        if !liquid_level_updates.is_empty() {
            // W296: incremental per-chunk remesh (the global dirty
            // flag re-signatures every tracked liquid cell).
            let cs = infengine_voxel::CHUNK_SIZE as i32;
            for (v, level) in liquid_level_updates {
                vx.liquid_level.insert(v, level.min(7));
                vx.enqueue_liquid_remesh([
                    v[0].div_euclid(cs),
                    v[1].div_euclid(cs),
                    v[2].div_euclid(cs),
                ]);
            }
        }
        for m in drop_spawns {
            // Q.5: clients mirror the spawn but pickup is
            // host-authoritative, so the pickup-delay here is
            // decorative. W468: the message carries the full stack
            // so the mirror renders the real item.
            vx.dropped_items.push(DropItem {
                id: m.id,
                stack: m.stack,
                pos: m.pos,
                vel: [0.0, 2.0, 0.0],
                age: 0.0,
                age_ticks: 0,
                pickup_delay_ticks: drops::PickupConfig::DEFAULT.pickup_delay_ticks,
            });
        }
        for id in drop_removes {
            vx.dropped_items.retain(|d| d.id != id);
        }
        if let Some(reason) = kick_reason {
            // Server kicked us. Notify the player + bounce back
            // to the main menu cleanly. notify_server_leaving
            // would race the disconnect so we just drop the role.
            vx.chat_log.push((
                std::time::Instant::now(),
                format!("Kicked by host: {reason}"),
            ));
            vx.net = NetRole::Offline;
            // Kicked -> bounce to the top menu (Wave 4 decoupled
            // AppState from menu.screen so an explicit reset is
            // needed; otherwise the menu shows the last sub-screen).
            vx.menu.screen = menu::MenuScreen::Main;
            vx.state = AppState::MainMenu;
            vx.browser_error = Some(format!("kicked: {reason}"));
        }
        if let Some(seed) = seed_update
            && seed != vx.base_seed
        {
            // Server picked a new world seed: adopt it and drop
            // every generated chunk so the next streaming pass
            // re-runs the workers with the new noise. We can't
            // mid-flight remap existing chunks since heights /
            // ore positions / biomes all changed.
            // W5.1: writes voxel.base_seed; per-dim splitting
            // happens inside current_worldgen_ctx().
            vx.base_seed = seed;
            let to_unload: Vec<ChunkCoord> = vx
                .world
                .iter_chunks()
                .map(|(c, _)| *c)
                .collect();
            if !to_unload.is_empty() {
                let renderer = vx
                    .renderer
                    .as_mut()
                    .expect("renderer ready");
                for (_cc, subs) in vx.chunk_meshes.drain() {
                    for (_block, handle) in subs {
                        renderer.remove_chunk_mesh(handle);
                    }
                }
                for cc in &to_unload {
                    vx.world.remove_chunk(*cc);
                }
            }
            vx.pending_chunks.clear();
            vx.pending_set.clear();
            vx.in_flight_chunks.clear();
            vx.pending_remesh.clear();
            vx.pending_remesh_set.clear();
        }
        if let Some(t) = teleport_request {
            vx.player_pos = Vec3::new(t.pos[0], t.pos[1], t.pos[2]);
            vx.player_vel = Vec3::ZERO;
            vx.fall_start_y = None;
            vx.chat_log.push((
                std::time::Instant::now(),
                "You were summoned.".to_string(),
            ));
        }
        if pvp_self_heal > 0.0 {
            vx.player_health = (vx.player_health + pvp_self_heal)
                .min(PLAYER_MAX_HEALTH);
        }
        if pvp_self_damage > 0.0 {
            vx.apply_player_damage(pvp_self_damage);
        }
        // Drop departed players from the client-side remote table
        // so their cubes stop rendering as soon as the server
        // tells us they left.
        if !player_removals.is_empty()
            && let NetRole::Client { remote, .. } = &mut vx.net
        {
            for id in &player_removals {
                remote.remove(id);
            }
        }
        // Apply mob hits requested by clients. Server validates
        // reach against the sender's last known pose before
        // routing into the authoritative damage path.
        for (cid, mob_id) in mob_hits_to_apply {
            let sender_pose = match &vx.net {
                NetRole::Server { poses, .. } => poses.get(&cid).copied(),
                _ => None,
            };
            let mob_pos = vx.remote_mobs.get(&mob_id).map(|m| m.pos);
            let ok = match (sender_pose, mob_pos) {
                (Some(p), Some(mp)) => {
                    let dx = p.pos[0] - mp[0];
                    let dy = p.pos[1] - mp[1];
                    let dz = p.pos[2] - mp[2];
                    dx * dx + dy * dy + dz * dz
                        <= (MOB_ATTACK_REACH + 1.0).powi(2)
                }
                _ => false,
            };
            if ok {
                vx.damage_mob(mob_id, Some(cid));
            }
        }
        // Client-side: apply incoming mob events (visual flash +
        // death sound). The server doesn't run this branch since
        // it already played them when authoritative damage fired.
        for evt in mob_events {
            vx.mob_hurt_flash.insert(evt.id, 0.30);
            // Death event: start the local fade timer so the
            // body tilts + fades for `MOB_DEATH_FADE` seconds.
            // The server emits `vx.mob_remove` after the same
            // delay, which will evict the entry from
            // `remote_mobs` at the right moment.
            if evt.kind == 1 {
                vx.mob_dying.insert(evt.id, MOB_DEATH_FADE);
            }
            // Attack event: kick off the local lunge timer so the
            // attacker's body briefly pushes forward.
            if evt.kind == 2 {
                vx.mob_lunge.insert(evt.id, MOB_LUNGE_TIME);
            }
            // W123: per-kind hurt / death vocals (real MC OGGs).
            // Lunge events (kind 2) carry no universal vocal. No
            // synth fallback - silence beats the W56.1 beep.
            if evt.kind <= 1
                && let Some(mob) = vx.remote_mobs.get(&evt.id).copied()
            {
                let stem = crate::mob::audio::mob_sound_stem(mob.kind);
                let key = if evt.kind == 1 {
                    format!("{stem}/death")
                } else {
                    format!("{stem}/hurt")
                };
                vx.sfx_at(&key, mob.pos, |_, _| {});
            }
        }
        // W468: spill container-cursor stacks of viewers that
        // disconnected this tick (whatever didn't fit back into
        // their inventory).
        for (pos, stack) in disconnect_spills {
            crate::drops::spawn_stack(vx, stack, pos);
        }
        // W5.3a/b: apply queued chest + furnace messages. Each
        // open registers the sender as the viewer + ships an
        // immediate state snapshot. Each act mutates the live
        // container + re-broadcasts. Each close clears the viewer.
        // Called HERE - outside the `match &mut vx.net` arm above
        // - so the borrow checker is happy with `&mut vx`.
        vx.apply_chest_open_msgs(&chest_opens_outer);
        vx.apply_chest_act_msgs(&chest_acts_outer);
        vx.apply_chest_close_msgs(&chest_closes_outer);
        vx.apply_furnace_open_msgs(&furnace_opens_outer);
        vx.apply_furnace_act_msgs(&furnace_acts_outer);
        vx.apply_furnace_close_msgs(&furnace_closes_outer);
        // W5.3a/b: client-side - apply incoming state snapshots
        // from the host. Each snapshot overwrites the local
        // chest/furnace contents so the open UI mirrors the
        // authoritative state. W468: the messages are stack-native
        // - slots and the per-viewer cursor adopt wholesale,
        // components included.
        for state in chest_state_updates {
            let c = vx
                .chests
                .entry(state.pos)
                .or_insert_with(block::chest::new_chest);
            for i in 0..c.len() {
                c.set_slot(i, state.slots.get(i).cloned().flatten());
            }
            if vx.chest_open == Some(state.pos) {
                vx.chest_held = state.cursor;
            }
        }
        for state in furnace_state_updates {
            let f = vx
                .furnaces
                .entry(state.pos)
                .or_insert_with(block::furnace::new_furnace);
            for i in 0..f.len() {
                f.set_slot(i, state.slots.get(i).cloned().flatten());
            }
            vx.furnace_runtimes.insert(
                state.pos,
                block::furnace::FurnaceRuntime {
                    fuel_left_ms: state.fuel_left_ms,
                    fuel_full_ms: state.fuel_full_ms,
                    cook_progress_ms: state.cook_progress_ms,
                },
            );
            if vx.furnace_open == Some(state.pos) {
                vx.furnace_held = state.cursor;
            }
        }
        // Removals: clean up local mob bookkeeping mirroring what
        // damage_mob does server-side.
        for id in mob_removals {
            vx.remote_mobs.remove(&id);
            vx.mob_hurt_flash.remove(&id);
            vx.mob_dying.remove(&id);
            vx.mob_step_timers.remove(&id);
            vx.mob_say_timers.remove(&id);
            vx.mob_last_xz.remove(&id);
        }
        // Drop interpolation entries for any player who left.
        for id in &player_removals {
            vx.remote_interp.remove(id);
        }
        // Periodic state flush. Server hosts AND offline
        // singleplayer both own the world state, so both auto-
        // save - a fresh-install user who plays offline and
        // quits without `/save` previously lost every build on
        // restart. The cadence matches the design doc's 60 s
        // autosave window.
        // W8.1 guard: only autosave while a world is actually loaded.
        // Without this the loop fires on the empty main-menu lobby
        // state too (current_slug = None falls back to DEFAULT_SLUG +
        // creates an empty `default/` slot the user never asked for).
        if vx.world_loaded
            && matches!(vx.net, NetRole::Server { .. } | NetRole::Offline)
            && vx
                .last_state_save
                .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(60))
        {
            vx.save_server_state();
        }
    }
