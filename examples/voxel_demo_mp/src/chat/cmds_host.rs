//! Host-flavoured slash commands: peer admin, server diagnostics,
//! moderator tools, mob ops, world save.
//!
//! Most commands here are gated on `NetRole::Server { .. }` and
//! print a "host-only" line otherwise. A handful (save, dmg,
//! killmobs, horde, spawnmob) also work in `NetRole::Offline`
//! since the local player effectively *is* the host there.

use glam::Vec3;
use infengine_voxel::BlockId;

use crate::Voxel;
use crate::block::{BLOCK_TYPES, is_tool_block};
use crate::net::{DamageMsg, MobMsg, NetRole, TeleportMsg};
use crate::{
    DAY_LENGTH_S, KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_SHEEP, KIND_SKELETON,
    KIND_SPIDER, KIND_ZOMBIE, MOB_HIT_DAMAGE, MOB_MAX_HEALTH, MobState,
    PLAYER_MAX_HEALTH, ZOMBIE_MAX_HEALTH,
};

/// `/motd`. Print the MOTD file next to the binary (if any).
pub(crate) fn cmd_motd(
    _voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    let mlines = Voxel::motd_lines();
    if mlines.is_empty() {
        lines.push("(no motd.txt next to exe)".to_string());
    } else {
        for l in mlines {
            lines.push(format!("[MOTD] {l}"));
        }
    }
    true
}

/// `/serverinfo`. Diagnostic dump: host name, client count,
/// role, seed, weather.
pub(crate) fn cmd_serverinfo(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Diagnostics dump: host name +
    // connected client count + active
    // role. Works in any role.
    let role = match &voxel.net {
        NetRole::Offline => "offline".to_string(),
        NetRole::Server { server, .. } => {
            format!("host ({} clients)", server.client_count())
        }
        NetRole::Client { client, .. } => {
            format!("client id={:?}", client.client_id)
        }
    };
    let host = match &voxel.net {
        NetRole::Server { .. } => voxel.host_name_buf.clone(),
        _ => "n/a".to_string(),
    };
    let weather_str = match voxel.weather {
        1 => "rain",
        2 => "snow",
        _ => "clear",
    };
    // W5.1: ctx-driven seed; reports the active dim's effective
    // seed instead of the (now-deleted) global atomic.
    let seed = voxel.current_worldgen_ctx().effective_seed();
    lines.push(format!(
        "Server: {host}  Role: {role}  Seed: {seed}  Weather: {weather_str}"
    ));
    true
}

/// `/save`. Flush the world state to disk now (host / offline
/// only).
pub(crate) fn cmd_save(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
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
        lines.push("/save is host / offline only.".to_string());
    } else {
        voxel.save_server_state();
        lines.push("World saved.".to_string());
    }
    true
}

/// `/dmg <name> [amount]` (alias `/damage`). Host-only:
/// damage a peer (or self).
pub(crate) fn cmd_dmg(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: deal damage to a named
    // peer or voxel. Usage: /dmg <name>
    // <amount>. Routes through the same
    // vx.damage path PvP uses.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/dmg is host-only.".to_string());
        return true;
    }
    let mut parts = arg.split_whitespace();
    let target = parts.next().unwrap_or("");
    let amount: f32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    if target.is_empty() {
        lines.push("Usage: /dmg <name> [amount]".to_string());
        return true;
    }
    let target_id = voxel
        .player_names
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(target))
        .map(|(id, _)| *id);
    match target_id {
        None => {
            lines.push(format!("No player named {target}."));
        }
        Some(0) => {
            voxel.apply_player_damage(amount);
            lines.push(format!("Self-damaged {amount}."));
        }
        Some(id) => {
            let cfg = bincode::config::standard();
            if let NetRole::Server { server, .. } = &mut voxel.net
                && let Ok(buf) = bincode::serde::encode_to_vec(
                    DamageMsg { amount },
                    cfg,
                )
            {
                server.send_bytes_to_client(
                    id, "vx.damage", buf,
                );
            }
            lines.push(format!(
                "Damaged {target} for {amount}."
            ));
        }
    }
    true
}

/// `/killmobs`. Nuke every live mob through the normal death
/// pipeline (host / offline only).
pub(crate) fn cmd_killmobs(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: nuke every live mob.
    // Pre-bleed HP to one hit so the
    // damage_mob call triggers the death
    // path the normal way (drops, remove
    // broadcast, event).
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/killmobs is host-only.".to_string());
        return true;
    }
    let ids: Vec<u32> = voxel.mobs.iter().map(|m| m.id).collect();
    let n = ids.len();
    for id in ids {
        if let Some(m) = voxel.mobs.iter_mut().find(|m| m.id == id)
        {
            m.health = MOB_HIT_DAMAGE;
        }
        voxel.damage_mob(id, None);
    }
    lines.push(format!("Killed {n} mobs."));
    true
}

/// `/horde [n]`. Spawn N zombies around the player in a ring
/// (host / offline only; clamped 1..=20).
pub(crate) fn cmd_horde(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: spawn N zombies around
    // the player in one go. Defaults to
    // 5; clamped 1..=20. Useful for
    // testing combat scenarios. Same
    // ring-spawn logic the spawner uses.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/horde is host-only.".to_string());
        return true;
    }
    let n: usize =
        arg.parse().ok().unwrap_or(5usize).clamp(1, 20);
    let mut spawned = 0;
    for i in 0..n {
        let angle = (i as f32 / n as f32) * std::f32::consts::TAU;
        let radius = 6.0;
        let cx = voxel.player_pos.x + angle.cos() * radius;
        let cz = voxel.player_pos.z + angle.sin() * radius;
        let pos = [cx, voxel.player_pos.y, cz];
        let id = voxel.next_mob_id;
        voxel.next_mob_id =
            voxel.next_mob_id.wrapping_add(1).max(1);
        // Horde mobs are command-spawned - mark them persistent
        // so they survive the despawn pass.
        let mut mob = MobState::new_natural(
            id,
            KIND_ZOMBIE,
            pos,
            pos,
            0.0,
            (voxel.day_time + 1.0).rem_euclid(DAY_LENGTH_S),
            ZOMBIE_MAX_HEALTH,
        );
        mob.persistent = true;
        voxel.mobs.push(mob);
        voxel.remote_mobs.insert(
            id,
            MobMsg {
                id,
                kind: KIND_ZOMBIE,
                pos,
                yaw: 0.0,
                health: ZOMBIE_MAX_HEALTH as u8,
                head_yaw: 0.0,
                head_pitch: 0.0,
            },
        );
        spawned += 1;
    }
    lines.push(format!("Spawned {spawned} zombies."));
    true
}

/// `/spawnmob [zombie|chicken|pig|sheep|cow|skeleton|spider]`. Spawn one
/// mob in front of the player (host / offline only).
pub(crate) fn cmd_spawnmob(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: spawn a sheep or zombie
    // a few blocks in front of the player.
    // Clients can't run this since the
    // server owns mob state.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/spawnmob is host-only.".to_string());
        return true;
    }
    let kind = if arg.eq_ignore_ascii_case("zombie") {
        KIND_ZOMBIE
    } else if arg.eq_ignore_ascii_case("chicken") {
        KIND_CHICKEN
    } else if arg.eq_ignore_ascii_case("pig") {
        KIND_PIG
    } else if arg.eq_ignore_ascii_case("cow") {
        KIND_COW
    } else if arg.eq_ignore_ascii_case("skeleton") {
        KIND_SKELETON
    } else if arg.eq_ignore_ascii_case("spider") {
        KIND_SPIDER
    } else if arg.eq_ignore_ascii_case("sheep") {
        KIND_SHEEP
    } else {
        // W101: an unknown kind used to silently spawn a sheep
        // (e.g. `/spawnmob creeper` -> sheep). Report it instead.
        lines.push(format!(
            "/spawnmob: unknown mob '{arg}'. Valid: zombie, skeleton, spider, cow, pig, sheep, chicken."
        ));
        return true;
    };
    let fwd_h = Vec3::new(
        voxel.player_yaw.cos(),
        0.0,
        -voxel.player_yaw.sin(),
    )
    .normalize_or_zero();
    let spawn_at = voxel.player_pos + fwd_h * 3.0;
    let id = voxel.next_mob_id;
    voxel.next_mob_id = voxel.next_mob_id.wrapping_add(1).max(1);
    let pos = [spawn_at.x, spawn_at.y, spawn_at.z];
    // /spawnmob is a command-spawn - mark persistent.
    let mut mob = MobState::new_natural(
        id,
        kind,
        pos,
        pos,
        0.0,
        (voxel.day_time + 1.0).rem_euclid(DAY_LENGTH_S),
        if kind == KIND_ZOMBIE {
            ZOMBIE_MAX_HEALTH
        } else {
            MOB_MAX_HEALTH
        },
    );
    mob.persistent = true;
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
            head_yaw: 0.0,
            head_pitch: 0.0,
        },
    );
    lines.push(format!(
        "Spawned {}.",
        if kind == KIND_ZOMBIE {
            "zombie"
        } else {
            "sheep"
        }
    ));
    true
}

/// `/mobcap [n]`. Read or set the per-host mob cap (0..=64).
pub(crate) fn cmd_mobcap(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /mobcap reports the cap; /mobcap N
    // sets it. Hard ceiling at 64 so the
    // server doesn't blow up its tick
    // budget; floor at 0 to allow a
    // disabled spawner.
    if arg.is_empty() {
        lines.push(format!(
            "Mob cap: {} (live: {})",
            voxel.mob_cap,
            voxel.mobs.len()
        ));
    } else if let Ok(v) = arg.parse::<usize>() {
        let new_cap = v.min(64);
        voxel.mob_cap = new_cap;
        lines.push(format!("Mob cap set to {new_cap}."));
    } else {
        lines.push(format!("Cannot parse mob cap: {arg}"));
    }
    true
}

/// `/lock`. Host-only world lock - blocks client `vx.edit`
/// packets. Host can still edit.
pub(crate) fn cmd_lock(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only world lock. Blocks any
    // vx.edit packets arriving from
    // clients. Host can still edit.
    if !matches!(voxel.net, NetRole::Server { .. }) {
        lines.push("/lock is host-only.".to_string());
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
        voxel
            .chat_log
            .push((std::time::Instant::now(), line.clone()));
        if let NetRole::Server { server, .. } = &mut voxel.net
            && let Ok(buf) = bincode::serde::encode_to_vec(
                &line,
                bincode::config::standard(),
            )
        {
            server.broadcast_bytes("vx.chat", buf);
        }
    }
    true
}

/// `/giveall`. Drops one of every non-tool block into the local
/// inventory.
pub(crate) fn cmd_giveall(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
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
    lines.push("Granted one of each block.".to_string());
    true
}

/// `/healall`. Host-only: heal self + every connected client.
pub(crate) fn cmd_healall(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only. Sends a full-heal
    // DamageMsg (-100) to every connected
    // client and tops the host's HP
    // back to max.
    if !matches!(voxel.net, NetRole::Server { .. }) {
        lines.push("/healall is host-only.".to_string());
        return true;
    }
    voxel.player_health = PLAYER_MAX_HEALTH;
    voxel.regen_accum = 0.0;
    voxel.hurt_flash = 0.0;
    let cfg = bincode::config::standard();
    let mut count = 0u32;
    if let NetRole::Server { server, .. } = &mut voxel.net {
        for cid in server.client_ids() {
            if let Ok(buf) = bincode::serde::encode_to_vec(
                DamageMsg { amount: -100.0 },
                cfg,
            ) {
                server.send_bytes_to_client(
                    cid, "vx.damage", buf,
                );
                count += 1;
            }
        }
    }
    lines.push(format!("Healed self + {count} clients."));
    true
}

/// `/summon <name>`. Host-only: yank a peer to the host's pose.
pub(crate) fn cmd_summon(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: yank a remote peer to
    // the host's pose by sending them a
    // vx.teleport. Silently ignored on
    // any other role to keep clients from
    // grabbing each other.
    if !matches!(voxel.net, NetRole::Server { .. }) {
        lines.push("/summon is host-only.".to_string());
        return false;
    }
    if arg.is_empty() {
        lines.push("Usage: /summon <name>".to_string());
        return false;
    }
    let target_id = voxel
        .player_names
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(arg))
        .map(|(id, _)| *id);
    if let Some(id) = target_id {
        if id == 0 {
            lines.push("Can't summon yourvoxel.".to_string());
        } else {
            let msg = TeleportMsg {
                pos: [
                    voxel.player_pos.x,
                    voxel.player_pos.y + 0.1,
                    voxel.player_pos.z,
                ],
            };
            if let NetRole::Server { server, .. } = &mut voxel.net
                && let Ok(buf) = bincode::serde::encode_to_vec(
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
            lines.push(format!("Summoned {arg}."));
        }
    } else {
        lines.push(format!("No player named {arg}."));
    }
    true
}

/// `/mute [name]`. With no arg: list muted players. With a
/// name: mute them.
pub(crate) fn cmd_mute(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    if arg.is_empty() {
        if voxel.muted_players.is_empty() {
            lines.push("No one muted.".to_string());
        } else {
            let mut names: Vec<String> =
                voxel.muted_players.iter().cloned().collect();
            names.sort();
            lines.push(format!("Muted: {}", names.join(", ")));
        }
    } else {
        voxel.muted_players.insert(arg.to_lowercase());
        lines.push(format!("Muted {arg}."));
    }
    true
}

/// `/unmute [name]`. With no arg: clear all mutes. With a name:
/// unmute them.
pub(crate) fn cmd_unmute(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    if arg.is_empty() {
        voxel.muted_players.clear();
        lines.push("All mutes cleared.".to_string());
    } else {
        voxel.muted_players.remove(&arg.to_lowercase());
        lines.push(format!("Unmuted {arg}."));
    }
    true
}

/// `/kick <name> [reason]`. Host-only: send a vx.kick to the
/// target + announce in chat.
pub(crate) fn cmd_kick(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only soft kick: sends a
    // vx.kick to the target with a
    // reason string. The client treats it
    // as a server-initiated disconnect
    // and bounces back to the main menu.
    if !matches!(voxel.net, NetRole::Server { .. }) {
        lines.push("/kick is host-only.".to_string());
        return true;
    }
    let mut parts = arg.splitn(2, char::is_whitespace);
    let target = parts.next().unwrap_or("").trim();
    let reason =
        parts.next().unwrap_or("kicked by host").trim();
    if target.is_empty() {
        lines.push(
            "Usage: /kick <name> [reason]".to_string(),
        );
        return true;
    }
    let target_id = voxel
        .player_names
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(target))
        .map(|(id, _)| *id);
    match target_id {
        None | Some(0) => {
            lines.push(format!("Cannot kick {target}."));
        }
        Some(id) => {
            let cfg = bincode::config::standard();
            if let NetRole::Server { server, .. } = &mut voxel.net
                && let Ok(buf) = bincode::serde::encode_to_vec(
                    reason.to_string(),
                    cfg,
                )
            {
                server.send_bytes_to_client(id, "vx.kick", buf);
            }
            // Announce in chat so
            // other peers see who
            // bounced.
            let line = format!(
                "* {target} was kicked: {reason}"
            );
            voxel
                .chat_log
                .push((std::time::Instant::now(), line.clone()));
            if let NetRole::Server { server, .. } = &mut voxel.net
                && let Ok(buf) = bincode::serde::encode_to_vec(
                    &line,
                    bincode::config::standard(),
                )
            {
                server.broadcast_bytes("vx.chat", buf);
            }
            lines.push(format!("Kicked {target}."));
        }
    }
    true
}

/// `/announce <message>`. Host-only system broadcast, prefixed
/// `[SERVER]`.
pub(crate) fn cmd_announce(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only system broadcast.
    // Prefixed `[SERVER]` so peers can
    // distinguish operator messages from
    // regular chat. Body is sent through
    // vx.chat directly so the recipient
    // chat path treats it like any other
    // line.
    if !matches!(voxel.net, NetRole::Server { .. }) {
        lines.push("/announce is host-only.".to_string());
    } else if arg.is_empty() {
        lines.push("Usage: /announce <message>".to_string());
    } else {
        let cfg = bincode::config::standard();
        let line = format!("[SERVER] {arg}");
        voxel
            .chat_log
            .push((std::time::Instant::now(), line.clone()));
        if let NetRole::Server { server, .. } = &mut voxel.net
            && let Ok(buf) =
                bincode::serde::encode_to_vec(&line, cfg)
        {
            server.broadcast_bytes("vx.chat", buf);
        }
    }
    true
}
