//! Chat input state helpers + slash command dispatch.
//!
//! Owns the chat log fade rules + history dedup/cap. The
//! `dispatch` fn is the heart of the module: a giant `match` on
//! the verb after `/`, routing 70+ commands across player /
//! creative / world / host paths. Each arm forwards to a free
//! `cmd_X` fn in one of six sub-modules; the cmd fn pushes
//! feedback lines into `local_lines` (drained into
//! `voxel.chat_log` at the end) and returns `true` when the
//! command consumed the input. A `false` return means "no
//! slash command handled this, forward to peers".
//!
//! Every command has a permission level in [`CMD_META`]. The
//! dispatch gate refuses commands whose required level exceeds
//! the calling [`Sender`]. Offline + host code paths use
//! [`Sender::host`] (level 4); remote clients reach dispatch
//! through [`dispatch_as`] with their own sender record so the
//! gate can distinguish ops from regular players when the
//! dedicated server lands (Phase B).
//!
//! State stays on `Voxel`: `chat_input`, `chat_log`,
//! `chat_history`, `chat_open`, `chat_history_idx`. This module
//! is pure logic on top of that state.

mod cmds_chat;
mod cmds_creative;
mod cmds_host;
mod cmds_misc;
mod cmds_player;
mod cmds_world;

mod perms;
pub(crate) use perms::{Sender, cmd_level};

use std::collections::HashSet;

use crate::Voxel;

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

/// Dispatch the slash command in `text` as the local host /
/// offline player (full permission). Convenience wrapper
/// around [`dispatch_as`] for the common case. Returns the
/// same `handled_locally` bool.
pub(crate) fn dispatch(voxel: &mut Voxel, text: &str) -> bool {
    dispatch_as(voxel, &Sender::host(), text)
}

/// Same as [`dispatch`] but executed on behalf of `sender`.
/// Used by the dedicated server / multiplayer path so a
/// remote client's command runs through the permission gate
/// with that client's resolved [`Sender::level`] instead of
/// the host's blanket level-4.
pub(crate) fn dispatch_as(
    voxel: &mut Voxel,
    sender: &Sender,
    text: &str,
) -> bool {
    let mut handled_locally = false;
    if let Some(rest) = text.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").to_lowercase();
        let arg = parts.next().unwrap_or("").trim();
        let mut local_lines: Vec<String> = Vec::new();
        // Permission gate. Unknown commands fall through to
        // the existing "Unknown command" arm so the user
        // sees a familiar message rather than "permission
        // denied" for a typo. Known commands whose required
        // level exceeds the sender's get a denial line and
        // the cmd body is never executed.
        if let Some(required) = cmd_level(&cmd)
            && sender.level < required
        {
            local_lines.push(format!(
                "/{cmd} requires permission level {required} (you have {})",
                sender.level
            ));
            let now = std::time::Instant::now();
            for l in local_lines {
                voxel.chat_log.push((now, l));
            }
            return true;
        }
        match cmd.as_str() {
            "help" | "?" => {
                cmds_chat::cmd_help(voxel, arg, &mut local_lines);
            }
            "day" => {
                handled_locally |=
                    cmds_world::cmd_day(voxel, arg, &mut local_lines);
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
                handled_locally |=
                    cmds_world::cmd_seed(voxel, arg, &mut local_lines);
            }
            "stats" => {
                handled_locally |= cmds_player::cmd_stats(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "listblocks" | "blocks" => {
                handled_locally |= cmds_misc::cmd_listblocks(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "recipe" => {
                handled_locally |=
                    cmds_misc::cmd_recipe(voxel, arg, &mut local_lines);
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
                handled_locally |=
                    cmds_host::cmd_motd(voxel, arg, &mut local_lines);
            }
            "serverinfo" => {
                handled_locally |= cmds_host::cmd_serverinfo(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "version" | "ver" => {
                handled_locally |= cmds_player::cmd_version(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "save" => {
                handled_locally |=
                    cmds_host::cmd_save(voxel, arg, &mut local_lines);
            }
            "dmg" | "damage" => {
                handled_locally |=
                    cmds_host::cmd_dmg(voxel, arg, &mut local_lines);
            }
            "killmobs" => {
                handled_locally |= cmds_host::cmd_killmobs(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "tree" => {
                handled_locally |=
                    cmds_world::cmd_tree(voxel, arg, &mut local_lines);
            }
            "horde" => {
                handled_locally |=
                    cmds_host::cmd_horde(voxel, arg, &mut local_lines);
            }
            "spawnmob" => {
                handled_locally |= cmds_host::cmd_spawnmob(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "clearcraft" => {
                handled_locally |= cmds_misc::cmd_clearcraft(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "resetinventory" | "resetinv" => {
                handled_locally |=
                    cmds_creative::cmd_resetinventory(
                        voxel,
                        arg,
                        &mut local_lines,
                    );
            }
            "clearinventory" | "clearinv" => {
                handled_locally |=
                    cmds_creative::cmd_clearinventory(
                        voxel,
                        arg,
                        &mut local_lines,
                    );
            }
            "sleep" => {
                handled_locally |=
                    cmds_world::cmd_sleep(voxel, arg, &mut local_lines);
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
                handled_locally |= cmds_world::cmd_setblock(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "blob" => {
                handled_locally |= cmds_creative::cmd_blob(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "fill" => {
                handled_locally |=
                    cmds_world::cmd_fill(voxel, arg, &mut local_lines);
            }
            "explode" => {
                handled_locally |= cmds_world::cmd_explode(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "fov" => {
                handled_locally |=
                    cmds_player::cmd_fov(voxel, arg, &mut local_lines);
            }
            "rd" | "renderdistance" => {
                handled_locally |=
                    cmds_world::cmd_rd(voxel, arg, &mut local_lines);
            }
            "mobcap" => {
                handled_locally |= cmds_host::cmd_mobcap(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "weather" => {
                handled_locally |= cmds_world::cmd_weather(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "lock" => {
                handled_locally |=
                    cmds_host::cmd_lock(voxel, arg, &mut local_lines);
            }
            "freezetime" | "freeze" => {
                handled_locally |= cmds_creative::cmd_freezetime(
                    voxel,
                    arg,
                    &mut local_lines,
                );
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
                handled_locally |=
                    cmds_world::cmd_night(voxel, arg, &mut local_lines);
            }
            "time" => {
                handled_locally |=
                    cmds_world::cmd_time(voxel, arg, &mut local_lines);
            }
            "giveall" => {
                handled_locally |= cmds_host::cmd_giveall(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "healall" => {
                handled_locally |= cmds_host::cmd_healall(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "restock" => {
                handled_locally |= cmds_creative::cmd_restock(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "tool" => {
                handled_locally |= cmds_creative::cmd_tool(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "give" => {
                handled_locally |= cmds_creative::cmd_give(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "summon" => {
                handled_locally |= cmds_host::cmd_summon(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "randomtp" | "rtp" => {
                handled_locally |= cmds_misc::cmd_randomtp(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "info" => {
                handled_locally |=
                    cmds_misc::cmd_info(voxel, arg, &mut local_lines);
            }
            "biome" => {
                handled_locally |=
                    cmds_misc::cmd_biome(voxel, arg, &mut local_lines);
            }
            "findbiome" => {
                handled_locally |= cmds_misc::cmd_findbiome(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "find" => {
                handled_locally |=
                    cmds_misc::cmd_find(voxel, arg, &mut local_lines);
            }
            "whois" => {
                handled_locally |=
                    cmds_misc::cmd_whois(voxel, arg, &mut local_lines);
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
                handled_locally |=
                    cmds_host::cmd_mute(voxel, arg, &mut local_lines);
            }
            "unmute" => {
                handled_locally |=
                    cmds_host::cmd_unmute(voxel, arg, &mut local_lines);
            }
            "wave" | "shrug" | "dance" => {
                handled_locally |= cmds_chat::cmd_emote(
                    voxel,
                    cmd.as_str(),
                    &mut local_lines,
                );
            }
            "fw" | "firework" => {
                handled_locally |=
                    cmds_misc::cmd_fw(voxel, arg, &mut local_lines);
            }
            "kick" => {
                handled_locally |=
                    cmds_host::cmd_kick(voxel, arg, &mut local_lines);
            }
            "announce" => {
                handled_locally |= cmds_host::cmd_announce(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "worldgen" | "gen" => {
                handled_locally |= cmds_world::cmd_worldgen(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "clearedits" | "wipe" => {
                handled_locally |= cmds_world::cmd_clearedits(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "grid" => {
                handled_locally |=
                    cmds_misc::cmd_grid(voxel, arg, &mut local_lines);
            }
            "listmobs" | "mobs" => {
                handled_locally |= cmds_misc::cmd_listmobs(
                    voxel,
                    arg,
                    &mut local_lines,
                );
            }
            "chunkinfo" => {
                handled_locally |= cmds_world::cmd_chunkinfo(
                    voxel,
                    arg,
                    &mut local_lines,
                );
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
