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
mod cmds_creative;
mod cmds_host;
mod cmds_player;
mod cmds_world;

use std::collections::HashSet;

use glam::Vec3;
use infengine_voxel::{BlockId, CHUNK_SIZE};

use crate::block::{BLOCK_TYPES, block_type};
use crate::net::NetRole;
use crate::{
    Biome, KIND_ZOMBIE, PLAYER_HEIGHT, RECIPES, Recipe,
    VERTICAL_CHUNKS, Voxel, biome_at,
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
