//! Misc slash commands that don't fit the player / world /
//! creative / host buckets cleanly: info dumps (block/recipe
//! catalogue, biome/find scans, peer pose), randomtp, firework,
//! debug toggles (grid, listmobs), crafting helper.

use glam::Vec3;
use infengine_voxel::{BlockId, CHUNK_SIZE};

use crate::Voxel;
use crate::block::{BLOCK_TYPES, block_type};
use crate::net::NetRole;
use crate::{
    Biome, KIND_ZOMBIE, PLAYER_HEIGHT, RECIPES, Recipe,
    VERTICAL_CHUNKS, biome_at,
};

/// `/listblocks` (alias `/blocks`). Dump every known block
/// name in groups of ~8 per line.
pub(crate) fn cmd_listblocks(
    _voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
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
        lines.push(line);
    }
    true
}

/// `/recipe <block>`. Find a recipe that produces the named
/// block + print its ingredients.
pub(crate) fn cmd_recipe(
    _voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /recipe <block>: find a recipe that
    // produces the named block + print
    // its ingredients. Useful when the
    // recipe book is closed.
    if arg.is_empty() {
        lines.push("Usage: /recipe <block>".to_string());
        return true;
    }
    let target_id = BLOCK_TYPES
        .iter()
        .find(|bt| bt.name.eq_ignore_ascii_case(arg))
        .map(|bt| bt.id);
    match target_id {
        None => lines.push(format!("Unknown block: {arg}")),
        Some(id) => {
            let found: Vec<&Recipe> = RECIPES
                .iter()
                .filter(|r| r.output.0 == id)
                .collect();
            if found.is_empty() {
                lines.push(format!(
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
                    lines.push(format!(
                        "{}: {} -> {}x {arg}",
                        r.name,
                        inputs.join(" + "),
                        r.output.1
                    ));
                }
            }
        }
    }
    true
}

/// `/clearcraft`. Empty the 2x2 crafting grid + return its
/// contents to the inventory.
pub(crate) fn cmd_clearcraft(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
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
    lines.push(format!("Returned {count} craft slot(s)."));
    true
}

/// `/randomtp` (alias `/rtp`). Random nearby teleport (30..120
/// blocks) landing on the surface.
pub(crate) fn cmd_randomtp(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Random nearby teleport. Picks a
    // direction + 30..120 block hop,
    // lands on the surface above that
    // column so the player never spawns
    // inside terrain.
    let mut rng: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        .wrapping_mul(0x9E37_79B1);
    rng ^= rng << 13;
    rng ^= rng >> 17;
    rng ^= rng << 5;
    let r1 = (rng as f32 / u32::MAX as f32).abs();
    rng ^= rng << 7;
    let r2 = (rng as f32 / u32::MAX as f32).abs();
    let angle = r1 * std::f32::consts::TAU;
    let dist = 30.0 + r2 * 90.0;
    let nx = voxel.player_pos.x + angle.cos() * dist;
    let nz = voxel.player_pos.z + angle.sin() * dist;
    let cs = CHUNK_SIZE as i32;
    let ceiling = cs * VERTICAL_CHUNKS - 1;
    let sy = voxel
        .surface_y_below(
            nx.floor() as i32,
            nz.floor() as i32,
            ceiling,
        )
        .unwrap_or(32);
    voxel.player_pos = Vec3::new(nx, sy as f32 + 1.5, nz);
    voxel.player_vel = Vec3::ZERO;
    voxel.fall_start_y = None;
    lines.push(format!(
        "Random TP to ({:.0}, {:.0}, {:.0}).",
        nx,
        sy as f32 + 1.5,
        nz,
    ));
    true
}

/// `/info [x y z]`. Print the name of the block at the given
/// coords (or under the player's feet if no args).
pub(crate) fn cmd_info(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /info <x> <y> <z> -> name of the
    // block at those coords. Without
    // args, reads the voxel the player
    // currently stands on.
    let target_voxel: Option<[i32; 3]> = if arg.is_empty() {
        Some([
            voxel.player_pos.x.floor() as i32,
            (voxel.player_pos.y - PLAYER_HEIGHT - 0.05).floor()
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
                (Ok(x), Ok(y), Ok(z)) => Some([x, y, z]),
                _ => None,
            }
        } else {
            None
        }
    };
    match target_voxel {
        Some(v) => {
            let b = voxel.world.get(v);
            let name = block_type(b.0).map(|t| t.name).unwrap_or("?");
            lines.push(format!(
                "({},{},{}) = {name} (id {})",
                v[0], v[1], v[2], b.0
            ));
        }
        None => {
            lines.push("Usage: /info [x y z]".to_string());
        }
    }
    true
}

/// `/biome`. Print the current biome at the player's feet. Reads
/// `climate_at` if the active generator is `Realistic`, else
/// falls back to the legacy `biome_at`. Permission level 0.
pub(crate) fn cmd_biome(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    use crate::worldgen::WorldGen;
    let px = voxel.player_pos.x;
    let pz = voxel.player_pos.z;
    // W5.1: build a fresh WorldgenCtx from voxel state instead of
    // peeking at the (now-deleted) global atomics. The ctx already
    // knows the active dim's profile + effective seed.
    let ctx = voxel.current_worldgen_ctx();
    let seed = ctx.effective_seed();
    let label = match ctx.profile {
        // W230: Mc worlds answer with the REAL McBiome registry stem
        // (all 51 variants) instead of the legacy 3-biome collapse.
        WorldGen::Mc => crate::worldgen::mcgen::terrain::mc_full_biome_at(
            &ctx,
            px.floor() as i32,
            pz.floor() as i32,
        )
        .name(),
        WorldGen::Realistic => {
            // Pull the biome from the climate map for the
            // realistic generator. Re-uses the same lookup that
            // gen_realistic uses so the answer matches what the
            // player is standing on.
            let climate = crate::worldgen::climate::climate_at(px, pz, seed);
            crate::worldgen::biome_table::biome_from_climate(climate).label()
        }
        _ => biome_at(px, pz, seed).label(),
    };
    lines.push(format!("Biome: {label} at ({px:.0}, {pz:.0})"));
    true
}

/// `/findbiome plains|mountain|desert`. Locate the nearest tile
/// in the named biome via a spiral scan.
pub(crate) fn cmd_findbiome(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Locate the nearest tile in the
    // named biome via a spiral scan in
    // 8-block steps out to 320 blocks.
    let target_biome = match arg.to_lowercase().as_str() {
        "plains" => Some(Biome::Plains),
        "mountain" | "mountains" => Some(Biome::Mountain),
        "desert" => Some(Biome::Desert),
        _ => None,
    };
    match target_biome {
        None => lines.push(
            "Usage: /findbiome plains|mountain|desert"
                .to_string(),
        ),
        Some(b) => {
            let px = voxel.player_pos.x as i32;
            let pz = voxel.player_pos.z as i32;
            // W5.1: ctx-driven seed lookup; no atomic reads.
            let seed = voxel.current_worldgen_ctx().effective_seed();
            let mut best: Option<(i32, i32, f32)> = None;
            for r in (0i32..=40).rev() {
                for dx in (-r..=r).step_by(8usize) {
                    for dz in (-r..=r).step_by(8usize) {
                        let wx = (px + dx * 8) as f32;
                        let wz = (pz + dz * 8) as f32;
                        if biome_at(wx, wz, seed) == b {
                            let d2 =
                                (dx * dx + dz * dz) as f32;
                            if best.is_none_or(|(_, _, bd)| d2 < bd)
                            {
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
                None => lines.push(format!(
                    "No {arg} found nearby."
                )),
                Some((wx, wz, _)) => {
                    lines.push(format!(
                        "Nearest {arg}: ({wx},?,{wz})"
                    ));
                }
            }
        }
    }
    true
}

/// `/find <block>`. Locate the nearest occurrence of a named
/// block within a 48-block Chebyshev box.
pub(crate) fn cmd_find(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Locates the nearest occurrence of
    // a named block within a 48-block
    // Chebyshev box around the player.
    // Useful for tracking down ore veins
    // or rare blocks placed earlier.
    if arg.is_empty() {
        lines.push("Usage: /find <block>".to_string());
        return true;
    }
    let target_id = BLOCK_TYPES
        .iter()
        .find(|bt| bt.name.eq_ignore_ascii_case(arg))
        .map(|bt| bt.id);
    match target_id {
        None => lines.push(format!("Unknown block: {arg}")),
        Some(id) => {
            let cx = voxel.player_pos.x.floor() as i32;
            let cy = voxel.player_pos.y.floor() as i32;
            let cz = voxel.player_pos.z.floor() as i32;
            let r = 48i32;
            let mut best: Option<([i32; 3], f32)> = None;
            for dy in -r..=r {
                for dz in -r..=r {
                    for dx in -r..=r {
                        let v =
                            [cx + dx, cy + dy, cz + dz];
                        if voxel.world.get(v).0 == id {
                            let d = ((dx * dx)
                                + (dy * dy)
                                + (dz * dz))
                                as f32;
                            if best
                                .is_none_or(|(_, bd)| d < bd)
                            {
                                best = Some((v, d));
                            }
                        }
                    }
                }
            }
            match best {
                None => {
                    lines.push(format!(
                        "No {arg} within 48 blocks."
                    ));
                }
                Some((v, d2)) => {
                    let d = d2.sqrt();
                    lines.push(format!(
                        "Nearest {arg}: ({},{},{}) ~{d:.1}m",
                        v[0], v[1], v[2]
                    ));
                }
            }
        }
    }
    true
}

/// `/whois <name>`. Diagnostic dump for a named peer: id + last
/// known pose.
pub(crate) fn cmd_whois(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Diagnostics for a named peer: id +
    // last known pose. Useful when
    // chasing connection issues.
    if arg.is_empty() {
        lines.push("Usage: /whois <name>".to_string());
        return true;
    }
    let target_id = voxel
        .player_names
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(arg))
        .map(|(id, _)| *id);
    match target_id {
        None => {
            lines.push(format!("No player named {arg}."));
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
                    lines.push(format!(
                        "{arg}: id={id} pos=({:.1}, {:.1}, {:.1})",
                        p[0], p[1], p[2]
                    ));
                }
                None => {
                    lines.push(format!(
                        "{arg}: id={id} (no pose yet)"
                    ));
                }
            }
        }
    }
    true
}

/// `/fw` (alias `/firework`). Spawn a cosmetic vertical rocket
/// + multi-colour particle burst.
pub(crate) fn cmd_fw(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
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
                position: [base[0], base[1] + t * 9.0, base[2]],
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
                gravity_scale: 1.0,
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
        (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
    };
    let hue_base = rand01();
    for _ in 0..60 {
        let theta = rand01() * std::f32::consts::TAU;
        let phi = (rand01() - 0.5) * std::f32::consts::PI;
        let speed = 5.0 + rand01() * 3.0;
        let vx = theta.cos() * phi.cos() * speed;
        let vy = phi.sin() * speed;
        let vz = theta.sin() * phi.cos() * speed;
        let h = (hue_base + rand01() * 0.2).fract();
        let tau = std::f32::consts::TAU;
        let r =
            (0.5 + 0.5 * (h * tau).sin()).clamp(0.0, 1.0);
        let g = (0.5 + 0.5 * ((h + 0.33) * tau).sin())
            .clamp(0.0, 1.0);
        let b = (0.5 + 0.5 * ((h + 0.66) * tau).sin())
            .clamp(0.0, 1.0);
        voxel.particles.particles.push(
            infengine_particles::Particle {
                position: apex,
                velocity: [vx, vy, vz],
                color: [r, g, b, 1.0],
                size: 0.18,
                age: 0.0,
                lifetime: 1.6,
                gravity_scale: 1.0,
            },
        );
    }
    // W123: real MC firework blast (launch pop kept as synth flavour).
    voxel.sfx_at("firework/blast", base, |a, g| {
        let _ = a.play_thud(180.0, 0.3 * g);
    });
    lines.push("Boom.".to_string());
    true
}

/// `/grid`. Toggle the chunk grid overlay.
pub(crate) fn cmd_grid(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    voxel.show_chunk_grid = !voxel.show_chunk_grid;
    lines.push(format!(
        "Chunk grid {}",
        if voxel.show_chunk_grid { "ON" } else { "OFF" }
    ));
    true
}

/// `/debug <subsystem> [on|off]`. Toggle developer-only loops that
/// ship off-by-default to keep the live experience clean. Today
/// the only switch is `feathers`, which controls the periodic
/// chicken feather-laying loop fixed in Q.3.
///
/// Usage:
///   /debug feathers          - report current state
///   /debug feathers on       - enable
///   /debug feathers off      - disable
///
/// Returns `true` whenever the command is recognised, even when
/// the argument is malformed - the function prints usage so the
/// caller never has to reach for an `unknown command` arm.
pub(crate) fn cmd_debug(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    let mut parts = arg.split_whitespace();
    let subsystem = parts.next().unwrap_or("");
    let action = parts.next().unwrap_or("");
    match subsystem {
        "feathers" => {
            let new = match action {
                "" => voxel.debug_chicken_feathers,
                "on" | "true" | "1" => true,
                "off" | "false" | "0" => false,
                _ => {
                    lines.push(
                        "Usage: /debug feathers [on|off]".to_string(),
                    );
                    return true;
                }
            };
            voxel.debug_chicken_feathers = new;
            // Reset accumulator on toggle so flipping ON doesn't
            // immediately emit a stale-accumulated feather.
            voxel.chicken_lay_accum = 0.0;
            lines.push(format!(
                "Debug feathers {}",
                if new { "ON" } else { "OFF" }
            ));
        }
        "" => {
            lines.push(
                "Usage: /debug <subsystem> [on|off]; known: feathers"
                    .to_string(),
            );
        }
        other => {
            lines.push(format!(
                "Unknown debug subsystem: {other} (known: feathers)"
            ));
        }
    }
    true
}

/// `/listmobs` (alias `/mobs`). Print a summary of remote mobs:
/// total count + per-kind tally.
pub(crate) fn cmd_listmobs(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
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
    lines.push(format!(
        "Mobs: {} total ({sheep} sheep, {zomb} zombie)",
        voxel.remote_mobs.len()
    ));
    true
}
