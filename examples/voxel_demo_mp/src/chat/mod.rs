//! Chat input state helpers + slash command dispatch.
//!
//! Owns the chat log fade rules + history dedup/cap. The
//! `dispatch` fn is the heart of the module: a giant `match` on
//! the verb after `/`, routing 70+ commands across player /
//! creative / world / host paths. Each arm pushes feedback
//! lines into `local_lines` (drained into `voxel.chat_log` at
//! the end); the caller checks the returned `bool` to decide
//! whether to broadcast the raw text as a normal chat message
//! (`false` means "no slash command handled this, forward to
//! peers").
//!
//! The match stays as a single block here rather than being
//! split into one fn per command because every arm reaches deep
//! into `Voxel` state - splitting would mostly multiply
//! signatures without improving readability.
//!
//! State stays on `Voxel`: `chat_input`, `chat_log`,
//! `chat_history`, `chat_open`, `chat_history_idx`. This module
//! is pure logic on top of that state.

mod cmds_chat;
mod cmds_player;

use std::collections::HashSet;

use glam::Vec3;
use infengine_voxel::{BlockId, CHUNK_SIZE, ChunkCoord};

use crate::block::{BLOCK_TYPES, block_type, is_tool_block};
use crate::inventory::{HOTBAR_SLOTS, inv_pickup};
use crate::net::{DamageMsg, EditMsg, MobMsg, NetRole, TeleportMsg};
use crate::{
    Biome, DAY_LENGTH_S, KIND_CHICKEN, KIND_PIG, KIND_SHEEP, KIND_ZOMBIE,
    MOB_HIT_DAMAGE, MOB_MAX_HEALTH, MobState, PLAYER_HEIGHT, PLAYER_MAX_HEALTH,
    RECIPES, Recipe, VERTICAL_CHUNKS, Voxel, WORLDGEN, WorldGen,
    ZOMBIE_MAX_HEALTH, biome_at, set_world_seed, world_seed,
};

/// Chat log retention: lines fade after this many seconds.
pub(crate) const CHAT_FADE_SECS: f32 = 10.0;

/// Chat log cap. Very long sessions trim the head.
pub(crate) const CHAT_LOG_MAX: usize = 64;

/// Chat input history cap. Up/Down in the chat box scrolls
/// through this many of the player's previous lines.
pub(crate) const CHAT_HISTORY_MAX: usize = 20;

/// Returns true when `line` is a chat message from a player
/// whose name appears in `muted`. Matches both the `<sender>`
/// tag the host prepends to normal chat and the `* sender`
/// prefix used by `/me` emotes.
pub(crate) fn line_is_muted(line: &str, muted: &HashSet<String>) -> bool {
    if let Some(rest) = line.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        let sender = &rest[..end];
        if muted.contains(&sender.to_lowercase()) {
            return true;
        }
    }
    if let Some(rest) = line.strip_prefix("* ")
        && let Some(end) = rest.find(' ')
    {
        let sender = &rest[..end];
        if muted.contains(&sender.to_lowercase()) {
            return true;
        }
    }
    false
}

/// Dispatch the slash command in `text`. Returns true if a
/// command was matched + handled locally (caller should NOT
/// broadcast `text` as normal chat). A `false` return means
/// either the text wasn't a slash command at all, or the
/// command's handler explicitly fell through to the broadcast
/// path.
pub(crate) fn dispatch(voxel: &mut Voxel, text: &str) -> bool {
    let mut handled_locally = false;
    if let Some(rest) = text.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").to_lowercase();
        let arg = parts.next().unwrap_or("").trim();
        let mut local_lines: Vec<String> = Vec::new();
        match cmd.as_str() {
            "help" | "?" => {
                cmds_chat::cmd_help(voxel, arg, &mut local_lines);
            }
            "day" => {
                // Snap day_time to mid-morning.
                voxel.day_time = DAY_LENGTH_S * 0.25;
                local_lines.push("Time set to morning.".to_string());
                handled_locally = true;
            }
            "heal" => {
                handled_locally |=
                    cmds_player::cmd_heal(voxel, arg, &mut local_lines);
            }
            "kill" => {
                handled_locally |=
                    cmds_player::cmd_kill(voxel, arg, &mut local_lines);
            }
            "seed" => {
                // /seed prints the active value;
                // /seed <n> (host-only) re-rolls the
                // world. Adopting a new seed drops
                // every generated chunk + broadcasts
                // vx.seed so connected clients follow
                // suit on the next net tick.
                if arg.is_empty() {
                    local_lines.push(format!(
                        "World seed: {}",
                        world_seed()
                    ));
                } else if !matches!(
                    voxel.net,
                    NetRole::Server { .. } | NetRole::Offline
                ) {
                    local_lines.push(
                        "/seed <n> is host-only.".to_string(),
                    );
                } else if let Ok(new_seed) = arg.parse::<u32>() {
                    set_world_seed(new_seed);
                    let to_unload: Vec<ChunkCoord> = voxel
                        .world
                        .iter_chunks()
                        .map(|(c, _)| *c)
                        .collect();
                    if !to_unload.is_empty() {
                        let renderer = voxel
                            .renderer
                            .as_mut()
                            .expect("renderer ready");
                        for entry in voxel.chunk_meshes.drain(..) {
                            renderer.remove_mesh(entry.2);
                        }
                        for cc in &to_unload {
                            voxel.world.remove_chunk(*cc);
                        }
                    }
                    voxel.pending_chunks.clear();
                    voxel.pending_set.clear();
                    if let NetRole::Server { server, .. } =
                        &mut voxel.net
                        && let Ok(buf) =
                            bincode::serde::encode_to_vec(
                                new_seed,
                                bincode::config::standard(),
                            )
                    {
                        server.broadcast_bytes("vx.seed", buf);
                    }
                    local_lines.push(format!(
                        "World seed set to {new_seed}; chunks regenerating."
                    ));
                } else {
                    local_lines.push(format!(
                        "Cannot parse seed: {arg}"
                    ));
                }
                handled_locally = true;
            }
            "stats" => {
                handled_locally |= cmds_player::cmd_stats(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "listblocks" | "blocks" => {
                // Dump every known block name in
                // groups of ~8 per line.
                let mut chunks: Vec<String> = Vec::new();
                let mut buf: Vec<&str> = Vec::new();
                for bt in BLOCK_TYPES.iter() {
                    buf.push(bt.name);
                    if buf.len() >= 8 {
                        chunks.push(buf.join(", "));
                        buf.clear();
                    }
                }
                if !buf.is_empty() {
                    chunks.push(buf.join(", "));
                }
                for line in chunks {
                    local_lines.push(line);
                }
                handled_locally = true;
            }
            "recipe" => {
                // /recipe <block>: find a recipe that
                // produces the named block + print
                // its ingredients. Useful when the
                // recipe book is closed.
                if arg.is_empty() {
                    local_lines.push(
                        "Usage: /recipe <block>".to_string(),
                    );
                } else {
                    let target_id = BLOCK_TYPES
                        .iter()
                        .find(|bt| {
                            bt.name.eq_ignore_ascii_case(arg)
                        })
                        .map(|bt| bt.id);
                    match target_id {
                        None => local_lines.push(format!(
                            "Unknown block: {arg}"
                        )),
                        Some(id) => {
                            let found: Vec<&Recipe> = RECIPES
                                .iter()
                                .filter(|r| r.output.0 == id)
                                .collect();
                            if found.is_empty() {
                                local_lines.push(format!(
                                    "No recipe produces {arg}."
                                ));
                            } else {
                                for r in found {
                                    let inputs: Vec<String> = r
                                        .inputs
                                        .iter()
                                        .map(|(bid, n)| {
                                            let name = BLOCK_TYPES
                                                .iter()
                                                .find(|bt| bt.id == *bid)
                                                .map(|bt| bt.name)
                                                .unwrap_or("?");
                                            format!("{n}x{name}")
                                        })
                                        .collect();
                                    local_lines.push(format!(
                                        "{}: {} -> {}x {arg}",
                                        r.name,
                                        inputs.join(" + "),
                                        r.output.1
                                    ));
                                }
                            }
                        }
                    }
                }
                handled_locally = true;
            }
            "uptime" => {
                handled_locally |= cmds_player::cmd_uptime(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "setname" => {
                handled_locally |= cmds_player::cmd_setname(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "motd" => {
                let lines = Voxel::motd_lines();
                if lines.is_empty() {
                    local_lines.push(
                        "(no motd.txt next to exe)"
                            .to_string(),
                    );
                } else {
                    for l in lines {
                        local_lines.push(format!("[MOTD] {l}"));
                    }
                }
                handled_locally = true;
            }
            "serverinfo" => {
                // Diagnostics dump: host name +
                // connected client count + active
                // role. Works in any role.
                let role = match &voxel.net {
                    NetRole::Offline => "offline".to_string(),
                    NetRole::Server { server, .. } => {
                        format!(
                            "host ({} clients)",
                            server.client_count()
                        )
                    }
                    NetRole::Client { client, .. } => {
                        format!(
                            "client id={:?}",
                            client.client_id
                        )
                    }
                };
                let host = match &voxel.net {
                    NetRole::Server { .. } => {
                        voxel.host_name_buf.clone()
                    }
                    _ => "n/a".to_string(),
                };
                let weather_str = match voxel.weather {
                    1 => "rain",
                    2 => "snow",
                    _ => "clear",
                };
                local_lines.push(format!(
                    "Server: {host}  Role: {role}  Seed: {}  Weather: {weather_str}",
                    world_seed()
                ));
                handled_locally = true;
            }
            "version" | "ver" => {
                handled_locally |= cmds_player::cmd_version(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "save" => {
                // Flush the world state (edit
                // history + inventories + day clock
                // + HP + kill count + snow + torch
                // orientation + liquid levels) to
                // disk right now instead of waiting
                // for the autosave cadence.
                // Available to whoever owns the
                // world: host in MP, the player
                // themselves in offline. Clients
                // can't save (they don't have the
                // authoritative state).
                if !matches!(
                    voxel.net,
                    NetRole::Server { .. } | NetRole::Offline
                ) {
                    local_lines.push(
                        "/save is host / offline only.".to_string(),
                    );
                } else {
                    voxel.save_server_state();
                    local_lines
                        .push("World saved.".to_string());
                }
                handled_locally = true;
            }
            "dmg" | "damage" => {
                // Host-only: deal damage to a named
                // peer or voxel. Usage: /dmg <name>
                // <amount>. Routes through the same
                // vx.damage path PvP uses.
                if !matches!(voxel.net, NetRole::Server { .. } | NetRole::Offline) {
                    local_lines.push(
                        "/dmg is host-only.".to_string(),
                    );
                } else {
                    let mut parts = arg.split_whitespace();
                    let target = parts.next().unwrap_or("");
                    let amount: f32 = parts
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(2.0);
                    if target.is_empty() {
                        local_lines.push(
                            "Usage: /dmg <name> [amount]"
                                .to_string(),
                        );
                    } else {
                        let target_id = voxel
                            .player_names
                            .iter()
                            .find(|(_, n)| {
                                n.eq_ignore_ascii_case(target)
                            })
                            .map(|(id, _)| *id);
                        match target_id {
                            None => {
                                local_lines.push(format!(
                                    "No player named {target}."
                                ));
                            }
                            Some(0) => {
                                voxel.apply_player_damage(amount);
                                local_lines.push(format!(
                                    "Self-damaged {amount}."
                                ));
                            }
                            Some(id) => {
                                let cfg =
                                    bincode::config::standard();
                                if let NetRole::Server {
                                    server,
                                    ..
                                } = &mut voxel.net
                                    && let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            DamageMsg { amount },
                                            cfg,
                                        )
                                {
                                    server.send_bytes_to_client(
                                        id,
                                        "vx.damage",
                                        buf,
                                    );
                                }
                                local_lines.push(format!(
                                    "Damaged {target} for {amount}."
                                ));
                            }
                        }
                    }
                    handled_locally = true;
                }
            }
            "killmobs" => {
                // Host-only: nuke every live mob.
                // Pre-bleed HP to one hit so the
                // damage_mob call triggers the death
                // path the normal way (drops, remove
                // broadcast, event).
                if !matches!(voxel.net, NetRole::Server { .. } | NetRole::Offline) {
                    local_lines.push(
                        "/killmobs is host-only.".to_string(),
                    );
                } else {
                    let ids: Vec<u32> =
                        voxel.mobs.iter().map(|m| m.id).collect();
                    let n = ids.len();
                    for id in ids {
                        if let Some(m) = voxel
                            .mobs
                            .iter_mut()
                            .find(|m| m.id == id)
                        {
                            m.health = MOB_HIT_DAMAGE;
                        }
                        voxel.damage_mob(id, None);
                    }
                    local_lines.push(format!("Killed {n} mobs."));
                }
                handled_locally = true;
            }
            "tree" => {
                // Host-only: drop a small oak tree
                // a few blocks in front of the
                // player. Routes every voxel through
                // the edit pipeline so the change
                // persists + replicates.
                if !matches!(voxel.net, NetRole::Server { .. } | NetRole::Offline) {
                    local_lines.push(
                        "/tree is host-only.".to_string(),
                    );
                } else {
                    let fwd = Vec3::new(
                        voxel.player_yaw.cos(),
                        0.0,
                        -voxel.player_yaw.sin(),
                    )
                    .normalize_or_zero();
                    let cx = (voxel.player_pos.x
                        + fwd.x * 4.0)
                        .floor()
                        as i32;
                    let cz = (voxel.player_pos.z
                        + fwd.z * 4.0)
                        .floor()
                        as i32;
                    let ceiling = (CHUNK_SIZE as i32)
                        * VERTICAL_CHUNKS
                        - 1;
                    let sy = voxel
                        .surface_y_below(cx, cz, ceiling)
                        .unwrap_or(voxel.player_pos.y.floor() as i32);
                    let cfg = bincode::config::standard();
                    let mut place = |x: i32,
                                     y: i32,
                                     z: i32,
                                     bid: u16| {
                        let v = [x, y, z];
                        voxel.world_set_tracked(
                            v,
                            BlockId(bid),
                        );
                        let edit = EditMsg {
                            x,
                            y,
                            z,
                            block: bid,
                        };
                        if let NetRole::Server {
                            server, ..
                        } = &mut voxel.net
                        {
                            voxel.edit_history.push(edit);
                            if let Ok(buf) =
                                bincode::serde::encode_to_vec(
                                    edit, cfg,
                                )
                            {
                                server.broadcast_bytes(
                                    "vx.edit",
                                    buf,
                                );
                            }
                        }
                    };
                    // 4-block trunk.
                    for ty in 1..=4 {
                        place(cx, sy + ty, cz, 5);
                    }
                    // 2 canopy layers + cap + top.
                    for layer in 0i32..2 {
                        let wy = sy + 3 + layer;
                        for dx in -2i32..=2 {
                            for dz in -2i32..=2 {
                                if dx.abs() == 2
                                    && dz.abs() == 2
                                {
                                    continue;
                                }
                                if dx == 0 && dz == 0 && layer == 0 {
                                    continue;
                                }
                                place(
                                    cx + dx,
                                    wy,
                                    cz + dz,
                                    6,
                                );
                            }
                        }
                    }
                    for dx in -1i32..=1 {
                        for dz in -1i32..=1 {
                            place(cx + dx, sy + 5, cz + dz, 6);
                        }
                    }
                    place(cx, sy + 6, cz, 6);
                    let coords = Voxel::affected_chunks(
                        [cx, sy + 3, cz],
                    );
                    voxel.rebuild_chunks(&coords);
                    local_lines
                        .push("Planted a tree.".to_string());
                }
                handled_locally = true;
            }
            "horde" => {
                // Host-only: spawn N zombies around
                // the player in one go. Defaults to
                // 5; clamped 1..=20. Useful for
                // testing combat scenarios. Same
                // ring-spawn logic the spawner uses.
                if !matches!(
                    voxel.net,
                    NetRole::Server { .. } | NetRole::Offline
                ) {
                    local_lines.push(
                        "/horde is host-only.".to_string(),
                    );
                } else {
                    let n: usize = arg
                        .parse()
                        .ok()
                        .unwrap_or(5usize)
                        .clamp(1, 20);
                    let mut spawned = 0;
                    for i in 0..n {
                        let angle = (i as f32 / n as f32)
                            * std::f32::consts::TAU;
                        let radius = 6.0;
                        let cx = voxel.player_pos.x
                            + angle.cos() * radius;
                        let cz = voxel.player_pos.z
                            + angle.sin() * radius;
                        let pos = [cx, voxel.player_pos.y, cz];
                        let id = voxel.next_mob_id;
                        voxel.next_mob_id = voxel
                            .next_mob_id
                            .wrapping_add(1)
                            .max(1);
                        let mob = MobState {
                            id,
                            kind: KIND_ZOMBIE,
                            pos,
                            target: pos,
                            yaw: 0.0,
                            retarget_at: (voxel.day_time + 1.0)
                                .rem_euclid(DAY_LENGTH_S),
                            health: ZOMBIE_MAX_HEALTH,
                            dying: 0.0,
                            attack_cd: 0.0,
                        };
                        voxel.mobs.push(mob);
                        voxel.remote_mobs.insert(
                            id,
                            MobMsg {
                                id,
                                kind: KIND_ZOMBIE,
                                pos,
                                yaw: 0.0,
                                health: ZOMBIE_MAX_HEALTH as u8,
                            },
                        );
                        spawned += 1;
                    }
                    local_lines.push(format!(
                        "Spawned {spawned} zombies."
                    ));
                }
                handled_locally = true;
            }
            "spawnmob" => {
                // Host-only: spawn a sheep or zombie
                // a few blocks in front of the player.
                // Clients can't run this since the
                // server owns mob state.
                if !matches!(voxel.net, NetRole::Server { .. } | NetRole::Offline) {
                    local_lines.push(
                        "/spawnmob is host-only.".to_string(),
                    );
                } else {
                    let kind = if arg.eq_ignore_ascii_case("zombie") {
                        KIND_ZOMBIE
                    } else if arg.eq_ignore_ascii_case("chicken") {
                        KIND_CHICKEN
                    } else if arg.eq_ignore_ascii_case("pig") {
                        KIND_PIG
                    } else {
                        KIND_SHEEP
                    };
                    let fwd_h = Vec3::new(
                        voxel.player_yaw.cos(),
                        0.0,
                        -voxel.player_yaw.sin(),
                    )
                    .normalize_or_zero();
                    let spawn_at = voxel.player_pos + fwd_h * 3.0;
                    let id = voxel.next_mob_id;
                    voxel.next_mob_id =
                        voxel.next_mob_id.wrapping_add(1).max(1);
                    let pos = [
                        spawn_at.x,
                        spawn_at.y,
                        spawn_at.z,
                    ];
                    let mob = MobState {
                        id,
                        kind,
                        pos,
                        target: pos,
                        yaw: 0.0,
                        retarget_at: (voxel.day_time + 1.0)
                            .rem_euclid(DAY_LENGTH_S),
                        health: if kind == KIND_ZOMBIE {
                            ZOMBIE_MAX_HEALTH
                        } else {
                            MOB_MAX_HEALTH
                        },
                        dying: 0.0,
                        attack_cd: 0.0,
                    };
                    let initial_hp = if kind == KIND_ZOMBIE {
                        ZOMBIE_MAX_HEALTH
                    } else {
                        MOB_MAX_HEALTH
                    } as u8;
                    voxel.mobs.push(mob);
                    voxel.remote_mobs.insert(
                        id,
                        MobMsg {
                            id,
                            kind,
                            pos,
                            yaw: 0.0,
                            health: initial_hp,
                        },
                    );
                    local_lines.push(format!(
                        "Spawned {}.",
                        if kind == KIND_ZOMBIE {
                            "zombie"
                        } else {
                            "sheep"
                        }
                    ));
                }
                handled_locally = true;
            }
            "clearcraft" => {
                // Empty the 2x2 crafting grid. Frees
                // up materials trapped in the grid
                // without forcing the player to drag
                // each slot back out manually.
                let mut taken: Vec<(BlockId, u8)> = Vec::new();
                for s in voxel.craft_grid.iter_mut() {
                    if let Some(stack) = s.take() {
                        taken.push(stack);
                    }
                }
                let count = taken.len() as u32;
                for (b, c) in taken {
                    for _ in 0..c {
                        voxel.pickup(b);
                    }
                }
                local_lines.push(format!(
                    "Returned {count} craft slot(s)."
                ));
                handled_locally = true;
            }
            "resetinventory" | "resetinv" => {
                // Restore the curated starter bundle
                // (wood tools + basic blocks). Slot
                // 0 + storage stay empty.
                for s in voxel.inventory.iter_mut() {
                    *s = None;
                }
                let seed: &[(u16, u8)] = &[
                    (16, 1),
                    (23, 1),
                    (29, 1),
                    (32, 1),
                    (2, 16),
                    (7, 16),
                    (8, 16),
                    (10, 4),
                ];
                for (i, (bid, count)) in seed.iter().enumerate()
                {
                    let slot = i + 1;
                    if slot < HOTBAR_SLOTS {
                        voxel.inventory[slot] =
                            Some((BlockId(*bid), *count));
                    }
                }
                for s in voxel.craft_grid.iter_mut() {
                    *s = None;
                }
                local_lines.push(
                    "Inventory reset to starter.".to_string(),
                );
                handled_locally = true;
            }
            "clearinventory" | "clearinv" => {
                // Empty every slot including hotbar
                // + craft grid. Useful for testing.
                // In MP this only zeroes the local
                // predicted view; the next vx.inv
                // snapshot from the host wipes it.
                for s in voxel.inventory.iter_mut() {
                    *s = None;
                }
                for s in voxel.craft_grid.iter_mut() {
                    *s = None;
                }
                local_lines.push(
                    "Inventory cleared.".to_string(),
                );
                handled_locally = true;
            }
            "sleep" => {
                // Skip the night: if the sun is below
                // the horizon, fast-forward day_time
                // to morning. Daytime use is a no-op
                // (matches the MC bed rule). Host-only
                // in MP so the night gets skipped for
                // every connected player via the
                // existing day_time broadcast.
                let phase = (voxel.day_time / DAY_LENGTH_S)
                    .rem_euclid(1.0);
                let raw_elev =
                    (phase * std::f32::consts::TAU).sin();
                let is_night = raw_elev < -0.05;
                if !is_night {
                    local_lines.push(
                        "You can only sleep at night.".to_string(),
                    );
                } else if matches!(voxel.net, NetRole::Client { .. }) {
                    local_lines.push(
                        "Only the host can advance time.".to_string(),
                    );
                } else {
                    voxel.day_time = DAY_LENGTH_S * 0.20;
                    voxel.last_day_broadcast = None; // force resend
                    local_lines.push(
                        "You slept until morning.".to_string(),
                    );
                }
                handled_locally = true;
            }
            "sethome" => {
                handled_locally |= cmds_player::cmd_sethome(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "home" => {
                handled_locally |=
                    cmds_player::cmd_home(voxel, arg, &mut local_lines);
            }
            "setblock" => {
                // /setblock x y z block: place a
                // block at exact world coords. Routes
                // through the normal edit path so the
                // change persists in the edit history
                // + replicates to peers. "air"
                // clears the cell.
                let parts: Vec<&str> = arg.split_whitespace().collect();
                if parts.len() != 4 {
                    local_lines.push(
                        "Usage: /setblock <x> <y> <z> <block>"
                            .to_string(),
                    );
                } else {
                    let x = parts[0].parse::<i32>();
                    let y = parts[1].parse::<i32>();
                    let z = parts[2].parse::<i32>();
                    let block_id: Option<u16> = if parts[3]
                        .eq_ignore_ascii_case("air")
                    {
                        Some(0)
                    } else {
                        BLOCK_TYPES
                            .iter()
                            .find(|bt| {
                                bt.name.eq_ignore_ascii_case(
                                    parts[3],
                                )
                            })
                            .map(|bt| bt.id)
                    };
                    match (x, y, z, block_id) {
                        (Ok(x), Ok(y), Ok(z), Some(bid)) => {
                            let coord = [x, y, z];
                            let block = BlockId(bid);
                            let edit = EditMsg {
                                x,
                                y,
                                z,
                                block: bid,
                            };
                            voxel.world_set_tracked(coord, block);
                            let coords = Voxel::affected_chunks(coord);
                            voxel.rebuild_chunks(&coords);
                            let cfg = bincode::config::standard();
                            match &mut voxel.net {
                                NetRole::Offline => {}
                                NetRole::Server { server, .. } => {
                                    voxel.edit_history.push(edit);
                                    if let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            edit, cfg,
                                        )
                                    {
                                        server.broadcast_bytes(
                                            "vx.edit",
                                            buf,
                                        );
                                    }
                                }
                                NetRole::Client {
                                    pending_edits, ..
                                } => {
                                    pending_edits.push(edit);
                                }
                            }
                            local_lines.push(format!(
                                "Set ({x},{y},{z}) = {}",
                                parts[3]
                            ));
                        }
                        _ => {
                            local_lines.push(
                                "Bad args. Coords must be ints + block must exist."
                                    .to_string(),
                            );
                        }
                    }
                    handled_locally = true;
                }
            }
            "blob" => {
                // Host-only: fill a sphere of voxels
                // around the player with the named
                // block. Usage: /blob <radius> <block>.
                // Hard radius cap at 12 so a typo
                // doesn't lock the host.
                if !matches!(
                    voxel.net,
                    NetRole::Server { .. } | NetRole::Offline
                ) {
                    local_lines.push(
                        "/blob is host-only.".to_string(),
                    );
                } else {
                    let mut parts = arg.split_whitespace();
                    let r_str = parts.next().unwrap_or("");
                    let block_name = parts.next().unwrap_or("");
                    let r_opt: Option<i32> =
                        r_str.parse().ok();
                    let bid_opt: Option<u16> = if block_name
                        .eq_ignore_ascii_case("air")
                    {
                        Some(0)
                    } else {
                        BLOCK_TYPES
                            .iter()
                            .find(|bt| {
                                bt.name
                                    .eq_ignore_ascii_case(
                                        block_name,
                                    )
                            })
                            .map(|bt| bt.id)
                    };
                    match (r_opt, bid_opt) {
                        (None, _) | (_, None) => {
                            local_lines.push(
                                "Usage: /blob <radius> <block>"
                                    .to_string(),
                            );
                        }
                        (Some(r), Some(bid)) => {
                            let radius = r.clamp(1, 12);
                            let block = BlockId(bid);
                            let center = [
                                voxel.player_pos.x.floor() as i32,
                                voxel.player_pos.y.floor() as i32,
                                voxel.player_pos.z.floor() as i32,
                            ];
                            let r2 = radius * radius;
                            let mut count = 0u32;
                            let cfg =
                                bincode::config::standard();
                            for dy in -radius..=radius {
                                for dz in -radius..=radius {
                                    for dx in -radius..=radius
                                    {
                                        if dx * dx + dy * dy
                                            + dz * dz
                                            > r2
                                        {
                                            continue;
                                        }
                                        let v = [
                                            center[0] + dx,
                                            center[1] + dy,
                                            center[2] + dz,
                                        ];
                                        if v[1] < 1 {
                                            continue;
                                        }
                                        voxel.world_set_tracked(
                                            v, block,
                                        );
                                        count += 1;
                                        let edit = EditMsg {
                                            x: v[0],
                                            y: v[1],
                                            z: v[2],
                                            block: bid,
                                        };
                                        if let NetRole::Server {
                                            server,
                                            ..
                                        } = &mut voxel.net
                                        {
                                            voxel
                                                .edit_history
                                                .push(edit);
                                            if let Ok(buf) =
                                                bincode::serde::encode_to_vec(
                                                    edit, cfg,
                                                )
                                            {
                                                server
                                                    .broadcast_bytes(
                                                        "vx.edit",
                                                        buf,
                                                    );
                                            }
                                        }
                                    }
                                }
                            }
                            let cs = CHUNK_SIZE as i32;
                            let mn = [
                                (center[0] - radius - 1)
                                    .div_euclid(cs),
                                (center[1] - radius - 1)
                                    .div_euclid(cs),
                                (center[2] - radius - 1)
                                    .div_euclid(cs),
                            ];
                            let mx = [
                                (center[0] + radius + 1)
                                    .div_euclid(cs),
                                (center[1] + radius + 1)
                                    .div_euclid(cs),
                                (center[2] + radius + 1)
                                    .div_euclid(cs),
                            ];
                            let mut coords: Vec<ChunkCoord> =
                                Vec::new();
                            for cz in mn[2]..=mx[2] {
                                for cy in mn[1]..=mx[1] {
                                    for cx in mn[0]..=mx[0] {
                                        coords
                                            .push([cx, cy, cz]);
                                    }
                                }
                            }
                            voxel.rebuild_chunks(&coords);
                            local_lines.push(format!(
                                "Placed {count} {block_name} (r={radius})."
                            ));
                        }
                    }
                    handled_locally = true;
                }
            }
            "fill" => {
                // Host-only: fill a box defined by
                // (x1 y1 z1 x2 y2 z2 block) with the
                // named block. "air" clears. Routes
                // every cell through the edit
                // pipeline so the change persists +
                // replicates. Hard cap at 64^3
                // voxels per call.
                if !matches!(
                    voxel.net,
                    NetRole::Server { .. } | NetRole::Offline
                ) {
                    local_lines.push(
                        "/fill is host-only.".to_string(),
                    );
                } else {
                    let parts: Vec<&str> =
                        arg.split_whitespace().collect();
                    if parts.len() != 7 {
                        local_lines.push(
                            "Usage: /fill x1 y1 z1 x2 y2 z2 block"
                                .to_string(),
                        );
                    } else if let (
                        Ok(x1),
                        Ok(y1),
                        Ok(z1),
                        Ok(x2),
                        Ok(y2),
                        Ok(z2),
                    ) = (
                        parts[0].parse::<i32>(),
                        parts[1].parse::<i32>(),
                        parts[2].parse::<i32>(),
                        parts[3].parse::<i32>(),
                        parts[4].parse::<i32>(),
                        parts[5].parse::<i32>(),
                    ) {
                        let block_id: Option<u16> = if parts[6]
                            .eq_ignore_ascii_case("air")
                        {
                            Some(0)
                        } else {
                            BLOCK_TYPES
                                .iter()
                                .find(|bt| {
                                    bt.name
                                        .eq_ignore_ascii_case(
                                            parts[6],
                                        )
                                })
                                .map(|bt| bt.id)
                        };
                        let mn = [
                            x1.min(x2),
                            y1.min(y2),
                            z1.min(z2),
                        ];
                        let mx = [
                            x1.max(x2),
                            y1.max(y2),
                            z1.max(z2),
                        ];
                        let span = [
                            (mx[0] - mn[0]).unsigned_abs() as u64
                                + 1,
                            (mx[1] - mn[1]).unsigned_abs() as u64
                                + 1,
                            (mx[2] - mn[2]).unsigned_abs() as u64
                                + 1,
                        ];
                        let total =
                            span[0] * span[1] * span[2];
                        let bid_opt = block_id;
                        if bid_opt.is_none() {
                            local_lines.push(format!(
                                "Unknown block: {}",
                                parts[6]
                            ));
                        } else if total > 64 * 64 * 64 {
                            local_lines.push(
                                "Too large: cap is 64^3 voxels.".to_string(),
                            );
                        } else if let Some(bid) = bid_opt {
                            let block = BlockId(bid);
                            let cfg =
                                bincode::config::standard();
                            let mut count = 0u64;
                            for y in mn[1]..=mx[1] {
                                for z in mn[2]..=mx[2] {
                                    for x in mn[0]..=mx[0] {
                                        let v = [x, y, z];
                                        voxel.world_set_tracked(
                                            v, block,
                                        );
                                        count += 1;
                                        let edit = EditMsg {
                                            x,
                                            y,
                                            z,
                                            block: bid,
                                        };
                                        if let NetRole::Server {
                                            server,
                                            ..
                                        } = &mut voxel.net
                                        {
                                            voxel
                                                .edit_history
                                                .push(edit);
                                            if let Ok(buf) =
                                                bincode::serde::encode_to_vec(
                                                    edit, cfg,
                                                )
                                            {
                                                server
                                                    .broadcast_bytes(
                                                        "vx.edit",
                                                        buf,
                                                    );
                                            }
                                        }
                                    }
                                }
                            }
                            // Remesh all touched
                            // chunks at once.
                            let cs = CHUNK_SIZE as i32;
                            let min_cc = [
                                mn[0].div_euclid(cs),
                                mn[1].div_euclid(cs),
                                mn[2].div_euclid(cs),
                            ];
                            let max_cc = [
                                mx[0].div_euclid(cs),
                                mx[1].div_euclid(cs),
                                mx[2].div_euclid(cs),
                            ];
                            let mut coords: Vec<ChunkCoord> =
                                Vec::new();
                            for cz in min_cc[2]..=max_cc[2] {
                                for cy in min_cc[1]..=max_cc[1] {
                                    for cx in
                                        min_cc[0]..=max_cc[0]
                                    {
                                        coords
                                            .push([cx, cy, cz]);
                                    }
                                }
                            }
                            voxel.rebuild_chunks(&coords);
                            local_lines.push(format!(
                                "Filled {count} blocks."
                            ));
                        }
                    } else {
                        local_lines.push(
                            "Coords must be ints.".to_string(),
                        );
                    }
                    handled_locally = true;
                }
            }
            "explode" => {
                // Host-only: blast a sphere of voxels
                // to air around the player. Default
                // radius 4. Routes every cleared cell
                // through the normal edit pipeline so
                // the change replicates + saves.
                if !matches!(
                    voxel.net,
                    NetRole::Server { .. } | NetRole::Offline
                ) {
                    local_lines.push(
                        "/explode is host-only.".to_string(),
                    );
                } else {
                    let radius: i32 = arg
                        .parse()
                        .ok()
                        .unwrap_or(4i32)
                        .clamp(1, 16);
                    let center = [
                        voxel.player_pos.x.floor() as i32,
                        voxel.player_pos.y.floor() as i32,
                        voxel.player_pos.z.floor() as i32,
                    ];
                    let mut cleared = 0u32;
                    let r2 = radius * radius;
                    let cfg = bincode::config::standard();
                    for dy in -radius..=radius {
                        for dz in -radius..=radius {
                            for dx in -radius..=radius {
                                if dx * dx + dy * dy + dz * dz
                                    > r2
                                {
                                    continue;
                                }
                                let v = [
                                    center[0] + dx,
                                    center[1] + dy,
                                    center[2] + dz,
                                ];
                                if v[1] < 1 {
                                    // Skip the bedrock
                                    // floor so the
                                    // crater doesn't
                                    // open into the
                                    // void.
                                    continue;
                                }
                                if voxel.world.get(v).0 == 0 {
                                    continue;
                                }
                                voxel.world_set_tracked(
                                    v,
                                    BlockId::AIR,
                                );
                                cleared += 1;
                                let edit = EditMsg {
                                    x: v[0],
                                    y: v[1],
                                    z: v[2],
                                    block: 0,
                                };
                                if let NetRole::Server {
                                    server,
                                    ..
                                } = &mut voxel.net
                                {
                                    voxel.edit_history.push(edit);
                                    if let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            edit, cfg,
                                        )
                                    {
                                        server.broadcast_bytes(
                                            "vx.edit",
                                            buf,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    // Remesh every chunk the
                    // explosion bounding box touches
                    // (plus its 1-cell halo).
                    let cs = CHUNK_SIZE as i32;
                    let min_cc = [
                        (center[0] - radius - 1).div_euclid(cs),
                        (center[1] - radius - 1).div_euclid(cs),
                        (center[2] - radius - 1).div_euclid(cs),
                    ];
                    let max_cc = [
                        (center[0] + radius + 1).div_euclid(cs),
                        (center[1] + radius + 1).div_euclid(cs),
                        (center[2] + radius + 1).div_euclid(cs),
                    ];
                    let mut coords: Vec<ChunkCoord> = Vec::new();
                    for cz in min_cc[2]..=max_cc[2] {
                        for cy in min_cc[1]..=max_cc[1] {
                            for cx in min_cc[0]..=max_cc[0] {
                                coords.push([cx, cy, cz]);
                            }
                        }
                    }
                    voxel.rebuild_chunks(&coords);
                    local_lines.push(format!(
                        "Exploded {cleared} blocks (r={radius})."
                    ));
                }
                handled_locally = true;
            }
            "fov" => {
                handled_locally |=
                    cmds_player::cmd_fov(voxel, arg, &mut local_lines);
            }
            "rd" | "renderdistance" => {
                // Without args: report current view
                // distance. With one arg (2..=12):
                // set it. Routes through the same
                // settings field the Settings panel
                // slider drives, so the change takes
                // effect on the next stream pass.
                if arg.is_empty() {
                    local_lines.push(format!(
                        "Render distance: {}",
                        voxel.settings.view_distance
                    ));
                } else if let Ok(v) = arg.parse::<u8>() {
                    voxel.settings.view_distance =
                        v.clamp(2, 12);
                    voxel.save_config();
                    local_lines.push(format!(
                        "Render distance set to {}.",
                        voxel.settings.view_distance
                    ));
                } else {
                    local_lines.push(format!(
                        "Cannot parse distance: {arg}"
                    ));
                }
                handled_locally = true;
            }
            "mobcap" => {
                // /mobcap reports the cap; /mobcap N
                // sets it. Hard ceiling at 64 so the
                // server doesn't blow up its tick
                // budget; floor at 0 to allow a
                // disabled spawner.
                if arg.is_empty() {
                    local_lines.push(format!(
                        "Mob cap: {} (live: {})",
                        voxel.mob_cap,
                        voxel.mobs.len()
                    ));
                } else if let Ok(v) = arg.parse::<usize>() {
                    let new_cap = v.min(64);
                    voxel.mob_cap = new_cap;
                    local_lines.push(format!(
                        "Mob cap set to {new_cap}."
                    ));
                } else {
                    local_lines.push(format!(
                        "Cannot parse mob cap: {arg}"
                    ));
                }
                handled_locally = true;
            }
            "weather" => {
                // /weather clear | rain. Sets the
                // local state immediately. If we're
                // hosting, also broadcast a vx.weather
                // packet so connected clients flip
                // their visuals to match.
                let kind = match arg.to_lowercase().as_str() {
                    "rain" => Some(1u8),
                    "snow" => Some(2u8),
                    "clear" | "" => Some(0u8),
                    _ => None,
                };
                match kind {
                    Some(k) => {
                        voxel.weather = k;
                        if let NetRole::Server { server, .. } =
                            &mut voxel.net
                            && let Ok(buf) =
                                bincode::serde::encode_to_vec(
                                    k,
                                    bincode::config::standard(),
                                )
                        {
                            server.broadcast_bytes("vx.weather", buf);
                        }
                        local_lines.push(format!(
                            "Weather: {}",
                            match k {
                                1 => "rain",
                                2 => "snow",
                                _ => "clear",
                            }
                        ));
                    }
                    None => {
                        local_lines.push(
                            "Usage: /weather clear|rain"
                                .to_string(),
                        );
                    }
                }
                handled_locally = true;
            }
            "lock" => {
                // Host-only world lock. Blocks any
                // vx.edit packets arriving from
                // clients. Host can still edit.
                if !matches!(voxel.net, NetRole::Server { .. }) {
                    local_lines.push(
                        "/lock is host-only.".to_string(),
                    );
                } else {
                    voxel.world_locked = !voxel.world_locked;
                    let line = format!(
                        "* world {}",
                        if voxel.world_locked {
                            "LOCKED"
                        } else {
                            "unlocked"
                        }
                    );
                    voxel.chat_log.push((
                        std::time::Instant::now(),
                        line.clone(),
                    ));
                    if let NetRole::Server { server, .. } =
                        &mut voxel.net
                        && let Ok(buf) =
                            bincode::serde::encode_to_vec(
                                &line,
                                bincode::config::standard(),
                            )
                    {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
                handled_locally = true;
            }
            "freezetime" | "freeze" => {
                voxel.freeze_time = !voxel.freeze_time;
                local_lines.push(format!(
                    "Time clock {}",
                    if voxel.freeze_time {
                        "PAUSED"
                    } else {
                        "RUNNING"
                    }
                ));
                handled_locally = true;
            }
            "godmode" | "god" => {
                handled_locally |= cmds_player::cmd_godmode(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "fly" => {
                handled_locally |=
                    cmds_player::cmd_fly(voxel, arg, &mut local_lines);
            }
            "night" => {
                // Snap day_time to midnight.
                voxel.day_time = DAY_LENGTH_S * 0.75;
                local_lines.push("Time set to night.".to_string());
                handled_locally = true;
            }
            "time" => {
                // /time <hh>: jump to a specific
                // 24-hour mark. Accepts 0..24, wraps
                // anything else.
                if arg.is_empty() {
                    let frac = (voxel.day_time / DAY_LENGTH_S)
                        .rem_euclid(1.0);
                    let hours = frac * 24.0;
                    let hh = hours.floor() as i32;
                    let mm = ((hours - hh as f32) * 60.0) as i32;
                    local_lines.push(format!(
                        "Current time: {hh:02}:{mm:02}"
                    ));
                } else if let Ok(hour) = arg.parse::<f32>() {
                    let h = hour.rem_euclid(24.0);
                    voxel.day_time = DAY_LENGTH_S * (h / 24.0);
                    local_lines.push(format!(
                        "Time set to {:.1}h.",
                        h
                    ));
                } else {
                    local_lines.push(format!(
                        "Cannot parse hour: {arg}"
                    ));
                }
                handled_locally = true;
            }
            "giveall" => {
                // Drops one of every block type into
                // the local inventory. Handy for UI
                // testing or quickly stocking a
                // builder slot.
                for bt in BLOCK_TYPES.iter() {
                    if is_tool_block(BlockId(bt.id)) {
                        continue;
                    }
                    voxel.pickup(BlockId(bt.id));
                }
                local_lines
                    .push("Granted one of each block.".to_string());
                handled_locally = true;
            }
            "healall" => {
                // Host-only. Sends a full-heal
                // DamageMsg (-100) to every connected
                // client and tops the host's HP
                // back to max.
                if !matches!(voxel.net, NetRole::Server { .. }) {
                    local_lines.push(
                        "/healall is host-only.".to_string(),
                    );
                } else {
                    voxel.player_health = PLAYER_MAX_HEALTH;
                    voxel.regen_accum = 0.0;
                    voxel.hurt_flash = 0.0;
                    let cfg = bincode::config::standard();
                    let mut count = 0u32;
                    if let NetRole::Server {
                        server, ..
                    } = &mut voxel.net
                    {
                        for cid in server.client_ids() {
                            if let Ok(buf) =
                                bincode::serde::encode_to_vec(
                                    DamageMsg {
                                        amount: -100.0,
                                    },
                                    cfg,
                                )
                            {
                                server.send_bytes_to_client(
                                    cid,
                                    "vx.damage",
                                    buf,
                                );
                                count += 1;
                            }
                        }
                    }
                    local_lines.push(format!(
                        "Healed self + {count} clients."
                    ));
                }
                handled_locally = true;
            }
            "restock" => {
                // Host-only. Drops a stack of every
                // non-tool block into every connected
                // client's inventory + the host's
                // own. Useful for co-op MP starts.
                if !matches!(voxel.net, NetRole::Server { .. }) {
                    local_lines.push(
                        "/restock is host-only.".to_string(),
                    );
                } else {
                    // Self first.
                    for bt in BLOCK_TYPES.iter() {
                        if is_tool_block(BlockId(bt.id)) {
                            continue;
                        }
                        for _ in 0..16 {
                            voxel.pickup(BlockId(bt.id));
                        }
                    }
                    // Each client.
                    let mut touched = 0u32;
                    if let NetRole::Server {
                        inventories,
                        pending_inv_sends,
                        ..
                    } = &mut voxel.net
                    {
                        for (cid, inv) in inventories.iter_mut() {
                            for bt in BLOCK_TYPES.iter() {
                                if is_tool_block(BlockId(bt.id)) {
                                    continue;
                                }
                                for _ in 0..16 {
                                    inv_pickup(
                                        &mut inv.inventory,
                                        BlockId(bt.id),
                                    );
                                }
                            }
                            pending_inv_sends.insert(*cid);
                            touched += 1;
                        }
                    }
                    local_lines.push(format!(
                        "Restocked self + {touched} clients."
                    ));
                }
                handled_locally = true;
            }
            "tool" => {
                // Quick pickaxe grant: /tool wood|stone|iron.
                // Equivalent to /give pickaxe_<tier> 1 but
                // shorter to type.
                let id_opt: Option<u16> = match arg
                    .to_lowercase()
                    .as_str()
                {
                    "wood" | "pickaxe_wood" => Some(16),
                    "stone" | "pickaxe_stone" => Some(17),
                    "iron" | "pickaxe_iron" => Some(18),
                    "sword_wood" => Some(23),
                    "sword_stone" => Some(24),
                    "sword_iron" => Some(25),
                    "shovel_wood" => Some(29),
                    "shovel_stone" => Some(30),
                    "shovel_iron" => Some(31),
                    "axe_wood" => Some(32),
                    "axe_stone" => Some(33),
                    "axe_iron" => Some(34),
                    _ => None,
                };
                match id_opt {
                    None => local_lines.push(
                        "Usage: /tool wood|stone|iron|sword_wood|sword_stone|sword_iron"
                            .to_string(),
                    ),
                    Some(id) => {
                        voxel.pickup(BlockId(id));
                        local_lines.push(format!(
                            "Granted {arg} pickaxe."
                        ));
                    }
                }
                handled_locally = true;
            }
            "give" => {
                // Add stacks of a named block to the
                // local inventory. In MP this only
                // affects the host's authoritative
                // copy; a connected client running
                // /give just edits their predicted
                // view (the next vx.inv snapshot
                // wipes it). Mostly useful for
                // testing + creative play.
                if arg.is_empty() {
                    local_lines.push(
                        "Usage: /give <block> [count]"
                            .to_string(),
                    );
                } else {
                    let mut parts = arg.split_whitespace();
                    let block_name = parts.next().unwrap_or("");
                    let count: u32 = parts
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(64);
                    let block_id = BLOCK_TYPES
                        .iter()
                        .find(|bt| {
                            bt.name.eq_ignore_ascii_case(block_name)
                        })
                        .map(|bt| bt.id);
                    if let Some(id) = block_id {
                        let bid = BlockId(id);
                        let mut added = 0u32;
                        while added < count {
                            let before = voxel
                                .inventory
                                .iter()
                                .filter_map(|s| s.map(|(_, c)| c as u32))
                                .sum::<u32>();
                            voxel.pickup(bid);
                            let after = voxel
                                .inventory
                                .iter()
                                .filter_map(|s| s.map(|(_, c)| c as u32))
                                .sum::<u32>();
                            if after <= before {
                                // Inventory was
                                // full; stop early so
                                // we don't infinite
                                // loop.
                                break;
                            }
                            added += 1;
                        }
                        local_lines.push(format!(
                            "Gave {added} x {block_name}."
                        ));
                    } else {
                        local_lines.push(format!(
                            "Unknown block: {block_name}"
                        ));
                    }
                    handled_locally = true;
                }
            }
            "summon" => {
                // Host-only: yank a remote peer to
                // the host's pose by sending them a
                // vx.teleport. Silently ignored on
                // any other role to keep clients from
                // grabbing each other.
                if !matches!(voxel.net, NetRole::Server { .. }) {
                    local_lines.push(
                        "/summon is host-only.".to_string(),
                    );
                } else if arg.is_empty() {
                    local_lines
                        .push("Usage: /summon <name>".to_string());
                } else {
                    let target_id = voxel
                        .player_names
                        .iter()
                        .find(|(_, n)| {
                            n.eq_ignore_ascii_case(arg)
                        })
                        .map(|(id, _)| *id);
                    if let Some(id) = target_id {
                        if id == 0 {
                            local_lines.push(
                                "Can't summon yourvoxel."
                                    .to_string(),
                            );
                        } else {
                            let msg = TeleportMsg {
                                pos: [
                                    voxel.player_pos.x,
                                    voxel.player_pos.y + 0.1,
                                    voxel.player_pos.z,
                                ],
                            };
                            if let NetRole::Server {
                                server, ..
                            } = &mut voxel.net
                                && let Ok(buf) =
                                    bincode::serde::encode_to_vec(
                                        msg,
                                        bincode::config::standard(),
                                    )
                            {
                                server.send_bytes_to_client(
                                    id,
                                    "vx.teleport",
                                    buf,
                                );
                            }
                            local_lines.push(format!(
                                "Summoned {arg}."
                            ));
                        }
                    } else {
                        local_lines
                            .push(format!("No player named {arg}."));
                    }
                    handled_locally = true;
                }
            }
            "randomtp" | "rtp" => {
                // Random nearby teleport. Picks a
                // direction + 30..120 block hop,
                // lands on the surface above that
                // column so the player never spawns
                // inside terrain.
                let mut rng: u32 = std::time::SystemTime::now()
                    .duration_since(
                        std::time::UNIX_EPOCH,
                    )
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
                    .wrapping_mul(0x9E37_79B1);
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let r1 =
                    (rng as f32 / u32::MAX as f32).abs();
                rng ^= rng << 7;
                let r2 =
                    (rng as f32 / u32::MAX as f32).abs();
                let angle = r1 * std::f32::consts::TAU;
                let dist = 30.0 + r2 * 90.0;
                let nx = voxel.player_pos.x
                    + angle.cos() * dist;
                let nz = voxel.player_pos.z
                    + angle.sin() * dist;
                let cs = CHUNK_SIZE as i32;
                let ceiling =
                    cs * VERTICAL_CHUNKS - 1;
                let sy = voxel
                    .surface_y_below(
                        nx.floor() as i32,
                        nz.floor() as i32,
                        ceiling,
                    )
                    .unwrap_or(32);
                voxel.player_pos = Vec3::new(
                    nx,
                    sy as f32 + 1.5,
                    nz,
                );
                voxel.player_vel = Vec3::ZERO;
                voxel.fall_start_y = None;
                local_lines.push(format!(
                    "Random TP to ({:.0}, {:.0}, {:.0}).",
                    nx,
                    sy as f32 + 1.5,
                    nz,
                ));
                handled_locally = true;
            }
            "info" => {
                // /info <x> <y> <z> -> name of the
                // block at those coords. Without
                // args, reads the voxel the player
                // currently stands on.
                let target_voxel: Option<[i32; 3]> = if arg.is_empty()
                {
                    Some([
                        voxel.player_pos.x.floor() as i32,
                        (voxel.player_pos.y - PLAYER_HEIGHT - 0.05)
                            .floor()
                            as i32,
                        voxel.player_pos.z.floor() as i32,
                    ])
                } else {
                    let parts: Vec<&str> =
                        arg.split_whitespace().collect();
                    if parts.len() == 3 {
                        match (
                            parts[0].parse::<i32>(),
                            parts[1].parse::<i32>(),
                            parts[2].parse::<i32>(),
                        ) {
                            (Ok(x), Ok(y), Ok(z)) => {
                                Some([x, y, z])
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                };
                match target_voxel {
                    Some(v) => {
                        let b = voxel.world.get(v);
                        let name = block_type(b.0)
                            .map(|t| t.name)
                            .unwrap_or("?");
                        local_lines.push(format!(
                            "({},{},{}) = {name} (id {})",
                            v[0], v[1], v[2], b.0
                        ));
                    }
                    None => {
                        local_lines.push(
                            "Usage: /info [x y z]"
                                .to_string(),
                        );
                    }
                }
                handled_locally = true;
            }
            "findbiome" => {
                // Locate the nearest tile in the
                // named biome via a spiral scan in
                // 8-block steps out to 320 blocks.
                let target_biome = match arg
                    .to_lowercase()
                    .as_str()
                {
                    "plains" => Some(Biome::Plains),
                    "mountain" | "mountains" => {
                        Some(Biome::Mountain)
                    }
                    "desert" => Some(Biome::Desert),
                    _ => None,
                };
                match target_biome {
                    None => local_lines.push(
                        "Usage: /findbiome plains|mountain|desert"
                            .to_string(),
                    ),
                    Some(b) => {
                        let px = voxel.player_pos.x as i32;
                        let pz = voxel.player_pos.z as i32;
                        let mut best: Option<(
                            i32,
                            i32,
                            f32,
                        )> = None;
                        for r in (0i32..=40).rev() {
                            for dx in
                                (-r..=r).step_by(8usize)
                            {
                                for dz in
                                    (-r..=r).step_by(8usize)
                                {
                                    let wx =
                                        (px + dx * 8) as f32;
                                    let wz =
                                        (pz + dz * 8) as f32;
                                    if biome_at(wx, wz) == b
                                    {
                                        let d2 = (dx * dx
                                            + dz * dz)
                                            as f32;
                                        if best.is_none_or(
                                            |(_, _, bd)| {
                                                d2 < bd
                                            },
                                        ) {
                                            best = Some((
                                                wx as i32,
                                                wz as i32,
                                                d2,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                        match best {
                            None => local_lines.push(format!(
                                "No {arg} found nearby."
                            )),
                            Some((wx, wz, _)) => {
                                local_lines.push(format!(
                                    "Nearest {arg}: ({wx},?,{wz})"
                                ));
                            }
                        }
                    }
                }
                handled_locally = true;
            }
            "find" => {
                // Locates the nearest occurrence of
                // a named block within a 48-block
                // Chebyshev box around the player.
                // Useful for tracking down ore veins
                // or rare blocks placed earlier.
                if arg.is_empty() {
                    local_lines.push(
                        "Usage: /find <block>".to_string(),
                    );
                } else {
                    let target_id = BLOCK_TYPES
                        .iter()
                        .find(|bt| {
                            bt.name.eq_ignore_ascii_case(arg)
                        })
                        .map(|bt| bt.id);
                    match target_id {
                        None => local_lines.push(format!(
                            "Unknown block: {arg}"
                        )),
                        Some(id) => {
                            let cx =
                                voxel.player_pos.x.floor() as i32;
                            let cy =
                                voxel.player_pos.y.floor() as i32;
                            let cz =
                                voxel.player_pos.z.floor() as i32;
                            let r = 48i32;
                            let mut best: Option<(
                                [i32; 3],
                                f32,
                            )> = None;
                            for dy in -r..=r {
                                for dz in -r..=r {
                                    for dx in -r..=r {
                                        let v = [
                                            cx + dx,
                                            cy + dy,
                                            cz + dz,
                                        ];
                                        if voxel
                                            .world
                                            .get(v)
                                            .0
                                            == id
                                        {
                                            let d = ((dx
                                                * dx)
                                                + (dy * dy)
                                                + (dz * dz))
                                                as f32;
                                            if best
                                                .is_none_or(
                                                    |(_, bd)| {
                                                        d < bd
                                                    },
                                                )
                                            {
                                                best = Some((
                                                    v,
                                                    d,
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                            match best {
                                None => {
                                    local_lines.push(format!(
                                        "No {arg} within 48 blocks."
                                    ));
                                }
                                Some((v, d2)) => {
                                    let d = d2.sqrt();
                                    local_lines.push(format!(
                                        "Nearest {arg}: ({},{},{}) ~{d:.1}m",
                                        v[0], v[1], v[2]
                                    ));
                                }
                            }
                        }
                    }
                }
                handled_locally = true;
            }
            "whois" => {
                // Diagnostics for a named peer: id +
                // last known pose. Useful when
                // chasing connection issues.
                if arg.is_empty() {
                    local_lines
                        .push("Usage: /whois <name>".to_string());
                } else {
                    let target_id = voxel
                        .player_names
                        .iter()
                        .find(|(_, n)| {
                            n.eq_ignore_ascii_case(arg)
                        })
                        .map(|(id, _)| *id);
                    match target_id {
                        None => {
                            local_lines.push(format!(
                                "No player named {arg}."
                            ));
                        }
                        Some(id) => {
                            let pose: Option<[f32; 3]> = match &voxel.net {
                                NetRole::Offline => None,
                                NetRole::Server { poses, .. } => {
                                    poses.get(&id).map(|p| p.pos)
                                }
                                NetRole::Client { remote, .. } => {
                                    remote.get(&id).map(|p| p.pos)
                                }
                            };
                            match pose {
                                Some(p) => {
                                    local_lines.push(format!(
                                        "{arg}: id={id} pos=({:.1}, {:.1}, {:.1})",
                                        p[0], p[1], p[2]
                                    ));
                                }
                                None => {
                                    local_lines.push(format!(
                                        "{arg}: id={id} (no pose yet)"
                                    ));
                                }
                            }
                        }
                    }
                }
                handled_locally = true;
            }
            "tppos" | "goto" => {
                handled_locally |= cmds_player::cmd_tppos(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "face" => {
                handled_locally |=
                    cmds_player::cmd_face(voxel, arg, &mut local_lines);
            }
            "pos" => {
                handled_locally |=
                    cmds_player::cmd_pos(voxel, arg, &mut local_lines);
            }
            "mute" => {
                if arg.is_empty() {
                    if voxel.muted_players.is_empty() {
                        local_lines.push(
                            "No one muted.".to_string(),
                        );
                    } else {
                        let mut names: Vec<String> = voxel
                            .muted_players
                            .iter()
                            .cloned()
                            .collect();
                        names.sort();
                        local_lines.push(format!(
                            "Muted: {}",
                            names.join(", ")
                        ));
                    }
                } else {
                    voxel.muted_players
                        .insert(arg.to_lowercase());
                    local_lines
                        .push(format!("Muted {arg}."));
                }
                handled_locally = true;
            }
            "unmute" => {
                if arg.is_empty() {
                    voxel.muted_players.clear();
                    local_lines.push(
                        "All mutes cleared.".to_string(),
                    );
                } else {
                    voxel.muted_players
                        .remove(&arg.to_lowercase());
                    local_lines.push(format!("Unmuted {arg}."));
                }
                handled_locally = true;
            }
            "wave" | "shrug" | "dance" => {
                handled_locally |= cmds_chat::cmd_emote(
                    voxel,
                    cmd.as_str(),
                    &mut local_lines,
                );
            }
            "fw" | "firework" => {
                // Spawn a vertical rocket trail of
                // particles from the player's feet
                // that "explodes" in a multi-colour
                // burst near apex. Cosmetic; nothing
                // is persisted.
                let base = [
                    voxel.player_pos.x,
                    voxel.player_pos.y - 0.6,
                    voxel.player_pos.z,
                ];
                // Streak: a column of fading orange.
                for i in 0..20 {
                    let t = i as f32 / 20.0;
                    voxel.particles.particles.push(
                        infengine_particles::Particle {
                            position: [
                                base[0],
                                base[1] + t * 9.0,
                                base[2],
                            ],
                            velocity: [0.0, 6.0, 0.0],
                            color: [
                                1.0,
                                0.7 - t * 0.4,
                                0.2,
                                0.9 - t * 0.6,
                            ],
                            size: 0.18,
                            age: 0.0,
                            lifetime: 0.7 - t * 0.4,
                        },
                    );
                }
                // Burst: scattered sparks at the
                // apex with hash-derived hue.
                let apex = [base[0], base[1] + 9.0, base[2]];
                let mut rng: u32 = ((voxel.net_tick as u32)
                    .wrapping_mul(0x9E37_79B1))
                    ^ 0xFAFAFA;
                let mut rand01 = || -> f32 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    (rng as f32 / u32::MAX as f32)
                        .clamp(0.0, 1.0)
                };
                let hue_base = rand01();
                for _ in 0..60 {
                    let theta = rand01() * std::f32::consts::TAU;
                    let phi = (rand01() - 0.5)
                        * std::f32::consts::PI;
                    let speed = 5.0 + rand01() * 3.0;
                    let vx = theta.cos() * phi.cos() * speed;
                    let vy = phi.sin() * speed;
                    let vz = theta.sin() * phi.cos() * speed;
                    let h =
                        (hue_base + rand01() * 0.2).fract();
                    let tau = std::f32::consts::TAU;
                    let r =
                        (0.5 + 0.5 * (h * tau).sin()).clamp(0.0, 1.0);
                    let g = (0.5
                        + 0.5 * ((h + 0.33) * tau).sin())
                        .clamp(0.0, 1.0);
                    let b = (0.5
                        + 0.5 * ((h + 0.66) * tau).sin())
                        .clamp(0.0, 1.0);
                    voxel.particles.particles.push(
                        infengine_particles::Particle {
                            position: apex,
                            velocity: [vx, vy, vz],
                            color: [r, g, b, 1.0],
                            size: 0.18,
                            age: 0.0,
                            lifetime: 1.6,
                        },
                    );
                }
                voxel.sfx_at("firework", base, |a, g| {
                    let _ = a.play_thud(180.0, 0.3 * g);
                });
                local_lines.push("Boom.".to_string());
                handled_locally = true;
            }
            "kick" => {
                // Host-only soft kick: sends a
                // vx.kick to the target with a
                // reason string. The client treats it
                // as a server-initiated disconnect
                // and bounces back to the main menu.
                if !matches!(voxel.net, NetRole::Server { .. }) {
                    local_lines.push(
                        "/kick is host-only.".to_string(),
                    );
                } else {
                    let mut parts =
                        arg.splitn(2, char::is_whitespace);
                    let target =
                        parts.next().unwrap_or("").trim();
                    let reason = parts
                        .next()
                        .unwrap_or("kicked by host")
                        .trim();
                    if target.is_empty() {
                        local_lines.push(
                            "Usage: /kick <name> [reason]"
                                .to_string(),
                        );
                    } else {
                        let target_id = voxel
                            .player_names
                            .iter()
                            .find(|(_, n)| {
                                n.eq_ignore_ascii_case(target)
                            })
                            .map(|(id, _)| *id);
                        match target_id {
                            None | Some(0) => {
                                local_lines.push(format!(
                                    "Cannot kick {target}."
                                ));
                            }
                            Some(id) => {
                                let cfg =
                                    bincode::config::standard();
                                if let NetRole::Server {
                                    server,
                                    ..
                                } = &mut voxel.net
                                    && let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            reason.to_string(),
                                            cfg,
                                        )
                                {
                                    server
                                        .send_bytes_to_client(
                                            id, "vx.kick", buf,
                                        );
                                }
                                // Announce in chat so
                                // other peers see who
                                // bounced.
                                let line = format!(
                                    "* {target} was kicked: {reason}"
                                );
                                voxel.chat_log.push((
                                    std::time::Instant::now(),
                                    line.clone(),
                                ));
                                if let NetRole::Server {
                                    server,
                                    ..
                                } = &mut voxel.net
                                    && let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            &line,
                                            bincode::config::standard(),
                                        )
                                {
                                    server.broadcast_bytes(
                                        "vx.chat", buf,
                                    );
                                }
                                local_lines.push(format!(
                                    "Kicked {target}."
                                ));
                            }
                        }
                    }
                }
                handled_locally = true;
            }
            "announce" => {
                // Host-only system broadcast.
                // Prefixed `[SERVER]` so peers can
                // distinguish operator messages from
                // regular chat. Body is sent through
                // vx.chat directly so the recipient
                // chat path treats it like any other
                // line.
                if !matches!(voxel.net, NetRole::Server { .. }) {
                    local_lines.push(
                        "/announce is host-only.".to_string(),
                    );
                } else if arg.is_empty() {
                    local_lines.push(
                        "Usage: /announce <message>"
                            .to_string(),
                    );
                } else {
                    let cfg = bincode::config::standard();
                    let line = format!("[SERVER] {arg}");
                    voxel.chat_log.push((
                        std::time::Instant::now(),
                        line.clone(),
                    ));
                    if let NetRole::Server { server, .. } =
                        &mut voxel.net
                        && let Ok(buf) =
                            bincode::serde::encode_to_vec(
                                &line, cfg,
                            )
                    {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
                handled_locally = true;
            }
            "worldgen" | "gen" => {
                // /worldgen with no arg prints the
                // current mode; with classic / flat /
                // islands / amplified it switches +
                // regenerates every cached chunk.
                // Host-only since the workers share
                // the WORLDGEN atomic.
                if !matches!(
                    voxel.net,
                    NetRole::Server { .. } | NetRole::Offline
                ) {
                    local_lines.push(
                        "/worldgen is host-only.".to_string(),
                    );
                } else if arg.is_empty() {
                    local_lines.push(format!(
                        "Worldgen: {:?}",
                        WorldGen::current()
                    ));
                } else if let Some(g) = WorldGen::from_str(arg) {
                    WORLDGEN.store(
                        g as u8,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    let to_unload: Vec<ChunkCoord> = voxel
                        .world
                        .iter_chunks()
                        .map(|(c, _)| *c)
                        .collect();
                    if !to_unload.is_empty() {
                        let renderer = voxel
                            .renderer
                            .as_mut()
                            .expect("renderer ready");
                        for entry in voxel.chunk_meshes.drain(..)
                        {
                            renderer.remove_mesh(entry.2);
                        }
                        for cc in &to_unload {
                            voxel.world.remove_chunk(*cc);
                        }
                    }
                    voxel.pending_chunks.clear();
                    voxel.pending_set.clear();
                    local_lines.push(format!(
                        "Worldgen set to {g:?}; chunks regenerating."
                    ));
                } else {
                    local_lines.push(
                        "Usage: /worldgen classic|flat|islands|amplified"
                            .to_string(),
                    );
                }
                handled_locally = true;
            }
            "clearedits" | "wipe" => {
                // Host-only: drop the entire edit
                // history + every cached chunk. Next
                // streaming pass regenerates from
                // scratch under the current seed.
                if !matches!(
                    voxel.net,
                    NetRole::Server { .. } | NetRole::Offline
                ) {
                    local_lines.push(
                        "/clearedits is host-only.".to_string(),
                    );
                } else {
                    let n_edits = voxel.edit_history.len();
                    voxel.edit_history.clear();
                    voxel.torch_positions.clear();
                    let to_unload: Vec<ChunkCoord> = voxel
                        .world
                        .iter_chunks()
                        .map(|(c, _)| *c)
                        .collect();
                    if !to_unload.is_empty() {
                        let renderer = voxel
                            .renderer
                            .as_mut()
                            .expect("renderer ready");
                        for entry in voxel.chunk_meshes.drain(..) {
                            renderer.remove_mesh(entry.2);
                        }
                        for cc in &to_unload {
                            voxel.world.remove_chunk(*cc);
                        }
                    }
                    voxel.pending_chunks.clear();
                    voxel.pending_set.clear();
                    local_lines.push(format!(
                        "Wiped {n_edits} edits + regenerating chunks."
                    ));
                }
                handled_locally = true;
            }
            "grid" => {
                voxel.show_chunk_grid = !voxel.show_chunk_grid;
                local_lines.push(format!(
                    "Chunk grid {}",
                    if voxel.show_chunk_grid {
                        "ON"
                    } else {
                        "OFF"
                    }
                ));
                handled_locally = true;
            }
            "listmobs" | "mobs" => {
                // Print a summary of remote mobs:
                // total count + per-kind tally.
                let mut sheep = 0u32;
                let mut zomb = 0u32;
                for m in voxel.remote_mobs.values() {
                    if m.kind == KIND_ZOMBIE {
                        zomb += 1;
                    } else {
                        sheep += 1;
                    }
                }
                local_lines.push(format!(
                    "Mobs: {} total ({sheep} sheep, {zomb} zombie)",
                    voxel.remote_mobs.len()
                ));
                handled_locally = true;
            }
            "chunkinfo" => {
                // Diagnostic: chunk count + size +
                // pending queue + edit history length.
                let cs = CHUNK_SIZE as i32;
                let voxels_per_chunk =
                    (cs as u64).pow(3);
                let total = voxel.chunk_meshes.len() as u64
                    * voxels_per_chunk;
                local_lines.push(format!(
                    "Chunks: {}  pending: {}  edits: {}  voxels: {total}",
                    voxel.chunk_meshes.len(),
                    voxel.pending_chunks.len(),
                    voxel.edit_history.len(),
                ));
                handled_locally = true;
            }
            "jump" => {
                handled_locally |=
                    cmds_player::cmd_jump(voxel, arg, &mut local_lines);
            }
            "stuck" | "unstuck" => {
                handled_locally |= cmds_player::cmd_stuck(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "spawnpoint" => {
                handled_locally |= cmds_player::cmd_spawnpoint(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "setspawn" => {
                handled_locally |= cmds_player::cmd_setspawn(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "spawn" => {
                handled_locally |= cmds_player::cmd_spawn(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "w" | "whisper" | "tell" | "msg" => {
                handled_locally |= cmds_chat::cmd_whisper(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "tp" => {
                handled_locally |=
                    cmds_player::cmd_tp(voxel, arg, &mut local_lines);
            }
            "players" | "who" => {
                cmds_chat::cmd_players(voxel, arg, &mut local_lines);
            }
            "clear" => {
                handled_locally |=
                    cmds_chat::cmd_clear(voxel, arg, &mut local_lines);
            }
            "me" => {
                handled_locally |=
                    cmds_chat::cmd_me(voxel, arg, &mut local_lines);
            }
            _ => {
                local_lines.push(format!("Unknown command: /{cmd}"));
            }
        }
        if !local_lines.is_empty() {
            let now = std::time::Instant::now();
            for l in local_lines {
                voxel.chat_log.push((now, l));
            }
            handled_locally = true;
        }
    }
    handled_locally
}
