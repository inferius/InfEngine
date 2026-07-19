//! Chat-flavoured slash commands: help / listing / clear /
//! emotes / private message.
//!
//! Each fn takes `(voxel, arg, lines)` and pushes feedback into
//! `lines`. Returns whether the command was "handled" (i.e.
//! consumed and should not be broadcast as normal chat). The
//! caller treats a non-empty `lines` as implicitly handled too,
//! so most fns can just `false` if they only push lines.

use crate::Voxel;
use crate::net::NetRole;

/// `/help` (and `/?`). Dumps the command catalogue in themed
/// chunks so the chat log wraps cleanly.
pub(crate) fn cmd_help(
    _voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Split into themed sections so the
    // chat log shows them line by line
    // instead of one ugly wrap.
    // W346: the list drifted far behind the dispatcher - every
    // routed command shows up here now, still grouped in short
    // lines so the log stays readable.
    lines.push(
        "Chat: /help /players /clear /me /w /wave /shrug /dance".to_string(),
    );
    lines.push(
        "Player: /spawn /tp /tppos /home /sethome /setspawn /spawnpoint /pos /face /jump /stuck"
            .to_string(),
    );
    lines.push(
        "Player 2: /heal /kill /stats /uptime /version /fov /gamemode (/gm /gms /gmc) /fly /godmode"
            .to_string(),
    );
    lines.push(
        "Creative: /give /giveall /tool /restock /clearinv /resetinv /blob /freezetime"
            .to_string(),
    );
    lines.push(
        "World: /day /night /time /sleep /weather /seed /setblock /fill /explode /tree /worldgen /clearedits"
            .to_string(),
    );
    lines.push(
        "View: /rd /lod /dim /chunkinfo /grid /debug /biome /findbiome /find /randomtp"
            .to_string(),
    );
    lines.push(
        "Host MP: /summon /spawnmob /horde /killmobs /mobcap /dmg /healall /kick /announce /mute /unmute"
            .to_string(),
    );
    lines.push(
        "Host 2: /save /lock /motd /serverinfo /setname /whois /plugins /listblocks /mobs /recipe /fw"
            .to_string(),
    );
    false
}

/// `/players` (alias `/who`). Dumps the known peer list.
pub(crate) fn cmd_players(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    let mut names: Vec<&String> =
        voxel.player_names.values().collect();
    names.sort_by_key(|a| a.to_lowercase());
    let joined: Vec<String> = names.into_iter().cloned().collect();
    let summary = if joined.is_empty() {
        "(no peers known yet)".to_string()
    } else {
        joined.join(", ")
    };
    lines.push(format!(
        "Players ({}): {summary}",
        voxel.player_names.len()
    ));
    false
}

/// `/clear`. Wipes the local chat log.
pub(crate) fn cmd_clear(
    voxel: &mut Voxel,
    _arg: &str,
    _lines: &mut Vec<String>,
) -> bool {
    voxel.chat_log.clear();
    true
}

/// `/me <action>`. Broadcast as an emote ("* name action").
pub(crate) fn cmd_me(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    if !arg.is_empty() {
        // /me <action> -> broadcast as
        // "* name action" so other players
        // see an emote, not a command.
        let action = arg.to_string();
        match &voxel.net {
            NetRole::Offline => {
                lines.push(format!(
                    "* {} {action}",
                    voxel.player_name
                ));
            }
            NetRole::Server { server, .. } => {
                let line = format!(
                    "* {} {action}",
                    voxel.player_name
                );
                voxel
                    .chat_log
                    .push((std::time::Instant::now(), line.clone()));
                if let Ok(buf) = bincode::serde::encode_to_vec(
                    &line,
                    bincode::config::standard(),
                ) {
                    server.broadcast_bytes("vx.chat", buf);
                }
            }
            NetRole::Client { client, .. } => {
                // Smuggle the emote through
                // the normal chat path so
                // the server tags it; the
                // server doesn't know /me
                // specifically.
                let emote = format!("* {action}");
                if let Ok(buf) = bincode::serde::encode_to_vec(
                    &emote,
                    bincode::config::standard(),
                ) {
                    let _ = client.send_bytes("vx.chat", buf);
                }
            }
        }
        true
    } else {
        lines.push("Usage: /me <action>".to_string());
        false
    }
}

/// `/w` (aliases `/whisper`, `/tell`, `/msg`). Private message
/// routed via the server, with a local shortcut for self-whisper.
pub(crate) fn cmd_whisper(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Private message routed via the
    // server. Host short-circuits to a
    // local lookup when whispering itself
    // / a local peer.
    let mut parts = arg.splitn(2, char::is_whitespace);
    let target = parts.next().unwrap_or("").trim();
    let body = parts.next().unwrap_or("").trim();
    if target.is_empty() || body.is_empty() {
        lines.push("Usage: /w <name> <message>".to_string());
        return false;
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
        Some(target_id) => {
            let cfg = bincode::config::standard();
            let line_in = format!("[whisper to {target}] {body}");
            let line_out = format!(
                "[whisper from {}] {body}",
                voxel.player_name
            );
            voxel
                .chat_log
                .push((std::time::Instant::now(), line_in));
            match &mut voxel.net {
                NetRole::Offline => {
                    // No-op offline.
                }
                NetRole::Server { server, .. } => {
                    if target_id != 0
                        && let Ok(buf) = bincode::serde::encode_to_vec(
                            &line_out, cfg,
                        )
                    {
                        server.send_bytes_to_client(
                            target_id, "vx.chat", buf,
                        );
                    }
                }
                NetRole::Client { client, .. } => {
                    let payload = format!(
                        "\u{1}whisper\u{1}{target_id}\u{1}{body}"
                    );
                    if let Ok(buf) = bincode::serde::encode_to_vec(
                        &payload, cfg,
                    ) {
                        let _ = client.send_bytes("vx.chat", buf);
                    }
                }
            }
        }
    }
    true
}

/// `/wave`, `/shrug`, `/dance`. Built-in emote shortcuts; each
/// expands to a canned /me line. `cmd` selects which.
pub(crate) fn cmd_emote(
    voxel: &mut Voxel,
    cmd: &str,
    _lines: &mut Vec<String>,
) -> bool {
    // Built-in emote shortcuts: each one
    // is a canned /me line broadcast via
    // the existing chat path. Keeps the
    // chat log readable + lets remote
    // peers see the action.
    let line = match cmd {
        "wave" => "waves",
        "shrug" => "shrugs (\\_(\u{30C4})_/)",
        _ => "dances",
    };
    let cfg = bincode::config::standard();
    match &mut voxel.net {
        NetRole::Offline => {
            voxel.chat_log.push((
                std::time::Instant::now(),
                format!("* {} {line}", voxel.player_name),
            ));
        }
        NetRole::Server { server, .. } => {
            let line =
                format!("* {} {line}", voxel.player_name);
            voxel
                .chat_log
                .push((std::time::Instant::now(), line.clone()));
            if let Ok(buf) =
                bincode::serde::encode_to_vec(&line, cfg)
            {
                server.broadcast_bytes("vx.chat", buf);
            }
        }
        NetRole::Client { client, .. } => {
            let emote = format!("* {line}");
            if let Ok(buf) =
                bincode::serde::encode_to_vec(&emote, cfg)
            {
                let _ = client.send_bytes("vx.chat", buf);
            }
        }
    }
    true
}
