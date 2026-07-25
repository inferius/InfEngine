//! Player-flavoured slash commands: HP, movement, teleports,
//! pose readout, identity.
//!
//! Each fn takes `(voxel, arg, lines)` and returns whether the
//! command was handled. Pushing lines is enough for the caller
//! to treat the command as handled, so most fns can return
//! `false` and rely on the dispatcher's "non-empty lines =
//! handled" rule.

use glam::Vec3;

use crate::Voxel;
use crate::net::{DamageMsg, NetRole};
use crate::PLAYER_MAX_HEALTH;

/// `/heal` (no args) refills self; `/heal <name> [amount]`
/// (host only) tops up a peer via a negative-damage message.
pub(crate) fn cmd_heal(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Refill HP. Without args heals
    // self; with /heal <name> + host
    // role, sends a vx.damage with a
    // negative amount to that peer's
    // client so its HP refills.
    if arg.is_empty() {
        voxel.player_health = PLAYER_MAX_HEALTH;
        voxel.regen_accum = 0.0;
        voxel.hurt_flash = 0.0;
        voxel.heartbeat_accum = 0.0;
        lines.push("Healed to full.".to_string());
    } else if !matches!(voxel.net, NetRole::Server { .. }) {
        lines.push("/heal <name> is host-only.".to_string());
    } else {
        // Allow `/heal <name> <amount>`
        // so a host can top up a peer by
        // an exact value instead of full
        // refill.
        let mut parts = arg.splitn(2, char::is_whitespace);
        let target = parts.next().unwrap_or("").trim();
        let amount: f32 = parts
            .next()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(100.0);
        let target_id = voxel
            .player_names
            .iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(target))
            .map(|(id, _)| *id);
        match target_id {
            None => {
                lines.push(format!("No player named {target}."))
            }
            Some(0) => {
                voxel.player_health = (voxel.player_health
                    + amount)
                    .min(PLAYER_MAX_HEALTH);
                lines.push(format!("Healed self by {amount}."));
            }
            Some(id) => {
                let cfg = bincode::config::standard();
                if let NetRole::Server { server, .. } =
                    &mut voxel.net
                    && let Ok(buf) = bincode::serde::encode_to_vec(
                        DamageMsg { amount: -amount },
                        cfg,
                    )
                {
                    server.send_bytes_to_client(
                        id, "vx.damage", buf,
                    );
                }
                lines.push(format!(
                    "Healed {target} for {amount}."
                ));
            }
        }
    }
    true
}

/// `/kill`. Force self to zero HP via the existing death path.
pub(crate) fn cmd_kill(
    voxel: &mut Voxel,
    _arg: &str,
    _lines: &mut Vec<String>,
) -> bool {
    // Force the player's HP to zero so
    // the existing apply_player_damage
    // death path runs (drops, respawn,
    // chat broadcast). Useful for testing
    // + when stuck in geometry.
    let dmg = voxel.player_health + 1.0;
    voxel.apply_player_damage(dmg);
    true
}

/// `/stats`. Per-session counters.
pub(crate) fn cmd_stats(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Per-session stats. Pairs naturally
    // with /uptime + /pos for a quick
    // status dump.
    lines.push(format!(
        "Kills: {}  broke: {}  placed: {}  walked: {:.0} m",
        voxel.kill_count,
        voxel.blocks_broken,
        voxel.blocks_placed,
        voxel.distance_walked,
    ));
    true
}

/// `/uptime`. Session timer formatted hh:mm:ss.
pub(crate) fn cmd_uptime(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Session timer formatted as hh:mm:ss.
    let secs = voxel
        .session_started
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    lines.push(format!("Uptime: {h:02}:{m:02}:{s:02}"));
    true
}

/// `/setname <name>`. Change local player_name + persist.
pub(crate) fn cmd_setname(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Change local player_name. In MP
    // the server's Hello-registered name
    // stays the original (UDP layer
    // doesn't expose rename), but the
    // chat tag + nameplate use the new
    // name and the next vx.names roster
    // sync may pick it up.
    let trimmed: String = arg
        .chars()
        .filter(|c| !c.is_control())
        .take(24)
        .collect();
    if trimmed.is_empty() {
        lines.push("Usage: /setname <name>".to_string());
    } else {
        voxel.player_name = trimmed.clone();
        if let NetRole::Server { .. } = &voxel.net {
            voxel.player_names.insert(0, trimmed.clone());
        }
        voxel.save_config();
        lines.push(format!("Name set to {trimmed}."));
    }
    true
}

/// `/version` (alias `/ver`). Print build version + active role.
pub(crate) fn cmd_version(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Print the build's CARGO_PKG_VERSION
    // plus the active net role so quick
    // diagnostic chat doesn't need F3.
    let role = match &voxel.net {
        NetRole::Offline => "offline",
        NetRole::Server { .. } => "host",
        NetRole::Client { .. } => "client",
    };
    lines.push(format!(
        "voxel_demo_mp v{}  [{role}]",
        env!("CARGO_PKG_VERSION")
    ));
    true
}

/// `/sethome`. Stash current pose as home spot.
pub(crate) fn cmd_sethome(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Stash the player's current pose for
    // /home recall. Saved to disk along
    // with the rest of the config on the
    // next Settings exit.
    voxel.home_pos = Some([
        voxel.player_pos.x,
        voxel.player_pos.y,
        voxel.player_pos.z,
    ]);
    voxel.save_config();
    lines.push("Home set.".to_string());
    true
}

/// `/home`. Teleport back to the saved home.
pub(crate) fn cmd_home(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Teleport back to the saved home, if
    // any. Velocity zeroed so no fall
    // damage on landing.
    if let Some(p) = voxel.home_pos {
        voxel.player_pos = Vec3::new(p[0], p[1], p[2]);
        voxel.player_vel = Vec3::ZERO;
        voxel.fall_start_y = None;
        lines.push("Teleported home.".to_string());
    } else {
        lines.push(
            "No home set. Use /sethome first.".to_string(),
        );
    }
    true
}

/// `/fov [deg]`. Read or set base camera FOV (30..=110).
pub(crate) fn cmd_fov(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /fov <deg>: clamp 30..=110, set
    // base camera FOV. The sprint widen
    // adds on top so /fov 100 still
    // bumps to ~110 when sprinting.
    if arg.is_empty() {
        let cur = voxel.fov_y_rad.to_degrees();
        lines.push(format!("FOV: {cur:.0} deg"));
    } else if let Ok(v) = arg.parse::<f32>() {
        let deg = v.clamp(30.0, 110.0);
        voxel.fov_y_rad = deg.to_radians();
        lines.push(format!("FOV set to {deg:.0} deg."));
    } else {
        lines.push(format!("Cannot parse FOV: {arg}"));
    }
    true
}

/// `/godmode` (alias `/god`). Toggle invincibility.
pub(crate) fn cmd_godmode(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    voxel.god_mode = !voxel.god_mode;
    voxel.save_config();
    lines.push(format!(
        "Godmode {}",
        if voxel.god_mode { "ON" } else { "OFF" }
    ));
    true
}

/// `/fly`. Toggle fly mode.
pub(crate) fn cmd_fly(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Toggle the local fly mode flag. The
    // movement integrator reads it; off
    // by default so a vanilla player
    // experience is unchanged.
    voxel.fly_mode = !voxel.fly_mode;
    voxel.save_config();
    if voxel.fly_mode {
        voxel.player_vel = Vec3::ZERO;
        voxel.fall_start_y = None;
        lines.push(
            "Fly mode ON. Space = up, Shift = down.".to_string(),
        );
    } else {
        lines.push("Fly mode OFF.".to_string());
    }
    true
}

/// `/tppos <x> <y> <z>` (alias `/goto`). Teleport to literal
/// coords.
pub(crate) fn cmd_tppos(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Teleport to literal coords. Usage:
    // /tppos <x> <y> <z>. Velocity zeroed
    // + fall tracker reset so no fall
    // damage on landing.
    let coords: Vec<&str> = arg.split_whitespace().collect();
    if coords.len() != 3 {
        lines.push("Usage: /tppos <x> <y> <z>".to_string());
    } else {
        match (
            coords[0].parse::<f32>(),
            coords[1].parse::<f32>(),
            coords[2].parse::<f32>(),
        ) {
            (Ok(x), Ok(y), Ok(z)) => {
                voxel.player_pos = Vec3::new(x, y, z);
                voxel.player_vel = Vec3::ZERO;
                voxel.fall_start_y = None;
                lines.push(format!(
                    "Teleported to ({x:.1}, {y:.1}, {z:.1})."
                ));
            }
            _ => {
                lines.push("Coords must be numbers.".to_string());
            }
        }
    }
    true
}

/// `/face n|e|s|w`. Snap yaw to a cardinal bearing.
pub(crate) fn cmd_face(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Snap the player yaw to a cardinal
    // bearing. /face n|e|s|w. yaw=0 ->
    // +X (east) per the rest of the demo.
    let yaw = match arg.to_lowercase().as_str() {
        "e" | "east" => Some(0.0_f32),
        "n" | "north" => Some(std::f32::consts::FRAC_PI_2),
        "w" | "west" => Some(std::f32::consts::PI),
        "s" | "south" => {
            Some(3.0 * std::f32::consts::FRAC_PI_2)
        }
        _ => None,
    };
    match yaw {
        Some(y) => {
            voxel.player_yaw = y;
            voxel.player_pitch = 0.0;
            lines.push(format!("Facing {arg}."));
        }
        None => {
            lines.push("Usage: /face n|e|s|w".to_string());
        }
    }
    true
}

/// `/pos`. Print precise pose (position + yaw + pitch).
pub(crate) fn cmd_pos(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Print precise pose. Pairs with the
    // HUD-line XYZ readout (which is
    // rounded) for when the player needs
    // exact coordinates to /tppos back to.
    lines.push(format!(
        "Pos: ({:.2}, {:.2}, {:.2})  yaw: {:.2}  pitch: {:.2}",
        voxel.player_pos.x,
        voxel.player_pos.y,
        voxel.player_pos.z,
        voxel.player_yaw,
        voxel.player_pitch,
    ));
    true
}

/// `/jump [strength]`. Launch the player straight up (clamped
/// 1..=50 m/s).
pub(crate) fn cmd_jump(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Launches the local player straight
    // up. Default 12 m/s; arg sets the
    // vertical velocity directly (clamped
    // 1..=50). Fall damage still applies
    // on landing - this is a launch, not
    // a teleport. Stamina untouched.
    let strength: f32 = arg
        .parse::<f32>()
        .ok()
        .unwrap_or(12.0)
        .clamp(1.0, 50.0);
    voxel.player_vel.y = strength;
    voxel.grounded = false;
    voxel.fall_start_y = Some(voxel.player_pos.y);
    lines.push(format!("Launched at {strength:.0} m/s."));
    true
}

/// `/stuck` (alias `/unstuck`). Punch the player 3 blocks up.
pub(crate) fn cmd_stuck(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Quick rescue: punch the player 3
    // blocks straight up + zero velocity.
    // Useful when wedged inside geometry
    // or buried by /explode debris.
    voxel.player_pos.y += 3.0;
    voxel.player_vel = Vec3::ZERO;
    voxel.fall_start_y = None;
    lines.push("Lifted 3 blocks up.".to_string());
    true
}

/// `/spawnpoint`. Print the configured respawn point or fall
/// back to origin.
pub(crate) fn cmd_spawnpoint(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Read-only counterpart to /setspawn:
    // print the configured respawn point
    // or call out the origin fallback.
    match voxel.spawn_pos {
        Some(p) => lines.push(format!(
            "Spawn: ({:.1}, {:.1}, {:.1})",
            p[0], p[1], p[2]
        )),
        None => lines.push("Spawn: origin (auto)".to_string()),
    }
    true
}

/// `/setspawn`. Persist the current pose as the respawn point.
pub(crate) fn cmd_setspawn(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    voxel.spawn_pos = Some([
        voxel.player_pos.x,
        voxel.player_pos.y,
        voxel.player_pos.z,
    ]);
    voxel.save_config();
    lines.push("Respawn point updated.".to_string());
    true
}

/// `/spawn`. Snap to the configured spawn (or terrain-aware
/// fallback) without fall damage.
pub(crate) fn cmd_spawn(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Snap to the configured spawn or
    // the terrain-aware fallback. Avoids
    // dropping the player inside stone
    // when the seed put a mountain at
    // the origin column.
    voxel.player_pos = voxel.safe_spawn();
    voxel.player_vel = Vec3::ZERO;
    voxel.fall_start_y = None;
    lines.push("Teleported to spawn.".to_string());
    true
}

/// `/tp <name>`. Teleport the local player to the pose of the
/// named peer.
pub(crate) fn cmd_tp(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /tp <name>: teleport the local
    // player to the pose of the named
    // peer. Works in any role - the
    // server uses `poses`, clients use
    // `remote_interp` since `remote`
    // mirrors the same data either way.
    if arg.is_empty() {
        lines.push("Usage: /tp <name>".to_string());
        return true;
    }
    let target_id = voxel
        .player_names
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(arg))
        .map(|(id, _)| *id);
    if let Some(id) = target_id {
        let pose: Option<[f32; 3]> = match &voxel.net {
            NetRole::Offline => None,
            NetRole::Server { poses, .. } => {
                poses.get(&id).map(|p| p.pos)
            }
            NetRole::Client { remote, .. } => {
                remote.get(&id).map(|p| p.pos)
            }
        };
        if let Some(p) = pose {
            voxel.player_pos =
                Vec3::new(p[0], p[1] + 0.1, p[2]);
            voxel.player_vel = Vec3::ZERO;
            voxel.fall_start_y = None;
            lines.push(format!("Teleported to {arg}."));
        } else {
            lines.push(format!(
                "No pose known for {arg} yet."
            ));
        }
    } else {
        lines.push(format!("No player named {arg}."));
    }
    true
}

/// W10.5: `/gamemode <survival|creative|0|1>` (alias `/gm`).
/// Switches the local player's mode + recomputes the cached
/// `Abilities` bundle so fly / invuln / instabuild flip in
/// lockstep. Mirrors MC's GameModeCommand.java lines 30-80.
///
/// Accepted spellings (case-insensitive):
/// - `survival`, `s`, `0` -> Survival
/// - `creative`, `c`, `1` -> Creative
///
/// Empty arg prints the current mode without changing it.
pub(crate) fn cmd_gamemode(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    use crate::game_mode::GameMode;
    let arg = arg.trim();
    if arg.is_empty() {
        lines.push(format!(
            "Current game mode: {}",
            voxel.game_mode.label_cs()
        ));
        return true;
    }
    // W394: MC syntax allows a trailing target selector
    // (`gamemode creative @a`) - accept and ignore it (single
    // player: every selector resolves to the host anyway).
    let arg = arg.split_whitespace().next().unwrap_or(arg);
    let target = match arg.to_lowercase().as_str() {
        "survival" | "s" | "0" => GameMode::Survival,
        "creative" | "c" | "1" => GameMode::Creative,
        other => {
            lines.push(format!(
                "Unknown game mode: {other}. \
                 Use survival|creative|0|1."
            ));
            return true;
        }
    };
    if voxel.game_mode == target {
        lines.push(format!(
            "Already in {} mode.",
            target.label_cs()
        ));
        return true;
    }
    voxel.game_mode = target;
    voxel.refresh_abilities();
    lines.push(format!(
        "Game mode set to {}.",
        target.label_cs()
    ));
    true
}

#[cfg(test)]
mod gamemode_tests {
    use super::*;
    use crate::chat::{Sender, cmd_level, dispatch_as};
    use crate::game_mode::GameMode;

    fn make_voxel() -> Voxel {
        Voxel::new()
    }

    #[test]
    fn gamemode_command_creative_switches() {
        // W10.5: /gamemode creative flips the mode + refreshes
        // abilities (instabuild now true).
        let mut v = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_gamemode(&mut v, "creative", &mut lines));
        assert_eq!(v.game_mode, GameMode::Creative);
        assert!(v.abilities.instabuild);
        assert!(lines.iter().any(|l| l.contains("Creative")));
    }

    #[test]
    fn gamemode_command_survival_switches() {
        // Inverse: Creative -> Survival drops instabuild back.
        let mut v = make_voxel();
        v.game_mode = GameMode::Creative;
        v.refresh_abilities();
        let mut lines = Vec::new();
        assert!(cmd_gamemode(&mut v, "survival", &mut lines));
        assert_eq!(v.game_mode, GameMode::Survival);
        assert!(!v.abilities.instabuild);
        assert!(!v.abilities.invulnerable);
    }

    #[test]
    fn gamemode_command_accepts_numeric_aliases() {
        // /gamemode 0 == survival, /gamemode 1 == creative.
        // Matches MC's GameModeCommand integer overload.
        let mut v = make_voxel();
        let mut lines = Vec::new();
        cmd_gamemode(&mut v, "1", &mut lines);
        assert_eq!(v.game_mode, GameMode::Creative);
        cmd_gamemode(&mut v, "0", &mut lines);
        assert_eq!(v.game_mode, GameMode::Survival);
    }

    #[test]
    fn gamemode_command_accepts_short_aliases() {
        // s / c shorthand. The /gm s / /gm c dispatcher form
        // delegates here.
        let mut v = make_voxel();
        let mut lines = Vec::new();
        cmd_gamemode(&mut v, "c", &mut lines);
        assert_eq!(v.game_mode, GameMode::Creative);
        cmd_gamemode(&mut v, "s", &mut lines);
        assert_eq!(v.game_mode, GameMode::Survival);
    }

    #[test]
    fn gamemode_command_invalid_arg_errors() {
        // Unknown spelling stays in the current mode + emits a
        // helpful error line. Catches typos like /gm adventure.
        let mut v = make_voxel();
        let before = v.game_mode;
        let mut lines = Vec::new();
        assert!(cmd_gamemode(&mut v, "adventure", &mut lines));
        assert_eq!(v.game_mode, before);
        assert!(lines.iter().any(|l| l.contains("Unknown")));
    }

    #[test]
    fn gamemode_command_empty_arg_prints_current() {
        // Bare /gamemode reports the current mode without
        // changing it. Matches MC's `/gamemode` with no arg.
        let mut v = make_voxel();
        let before = v.game_mode;
        let mut lines = Vec::new();
        assert!(cmd_gamemode(&mut v, "", &mut lines));
        assert_eq!(v.game_mode, before);
        assert!(lines.iter().any(|l| l.contains("Current")));
    }

    #[test]
    fn gamemode_command_perm_level_is_two() {
        // W10.5 spec: perm 2 (creative/movement). A level-0
        // sender is blocked at the dispatcher gate.
        assert_eq!(cmd_level("gamemode"), Some(2));
        assert_eq!(cmd_level("gm"), Some(2));
    }

    #[test]
    fn gamemode_command_perm_blocked_for_level_0() {
        // Drive the full dispatcher with a guest-level sender
        // and confirm the mode never changes.
        let mut v = make_voxel();
        let guest = Sender { name: "guest".into(), level: 0 };
        let handled = dispatch_as(&mut v, &guest, "/gamemode creative");
        assert!(handled);
        // Mode untouched.
        assert_eq!(v.game_mode, GameMode::Survival);
        // Feedback line mentions the gate.
        let last = v.chat_log.iter().next_back().map(|(_, l)| l.clone());
        assert!(
            last.as_deref().unwrap_or("").contains("permission level 2"),
            "expected perm denial line, got: {last:?}"
        );
    }

    #[test]
    fn gamemode_command_already_in_mode_is_noop_line() {
        // /gamemode creative while already creative emits an
        // "Already in" line + leaves the mode untouched.
        let mut v = make_voxel();
        v.game_mode = GameMode::Creative;
        v.refresh_abilities();
        let mut lines = Vec::new();
        assert!(cmd_gamemode(&mut v, "creative", &mut lines));
        assert_eq!(v.game_mode, GameMode::Creative);
        assert!(lines.iter().any(|l| l.contains("Already")));
    }

    #[test]
    fn gms_alias_routes_to_survival() {
        // /gms is a shorthand for /gamemode survival. The
        // dispatcher rewrites the arg to "survival" before
        // calling cmd_gamemode.
        let mut v = make_voxel();
        v.game_mode = GameMode::Creative;
        v.refresh_abilities();
        dispatch_as(&mut v, &Sender::host(), "/gms");
        assert_eq!(v.game_mode, GameMode::Survival);
    }

    #[test]
    fn gmc_alias_routes_to_creative() {
        // /gmc is a shorthand for /gamemode creative.
        let mut v = make_voxel();
        assert_eq!(v.game_mode, GameMode::Survival);
        dispatch_as(&mut v, &Sender::host(), "/gmc");
        assert_eq!(v.game_mode, GameMode::Creative);
        assert!(v.abilities.instabuild);
    }
}
