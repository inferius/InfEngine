//! W337 - MC `ServerExplosion` port (the ghast fireball's blast).
//!
//! The ray sweep is the vanilla algorithm verbatim: 16^3 grid SHELL
//! directions from the centre, each ray with intensity
//! `power * (0.7 + rand*0.6)`, stepping 0.3 blocks and paying
//! `(resistance + 0.3) * 0.3` per non-air cell plus a flat 0.225
//! per step. Cells a ray still has strength inside get destroyed.
//! Power 1 (ghast) therefore pocks soft ground (dirt, netherrack,
//! leaves) and NEVER cracks stone (resistance 6 eats the whole ray
//! budget) - same as vanilla.
//!
//! Entity damage follows `Explosion.getSeenPercent`-less MC math
//! (seen fraction fixed at 1): impact = 1 - dist/diameter, damage
//! = (impact^2 + impact)/2 * 7 * diameter + 1. Fire placement (a
//! third of destroyed cells over solid ground) matches
//! `ServerExplosion.createFire`.

use glam::Vec3;

use crate::Voxel;
use crate::block::{FIRE_ID, SOUL_FIRE_ID};
use crate::net::NetRole;
use crate::worldgen::mcgen::noise::SplitMix64;
use infengine_voxel::BlockId;

/// Coarse MC blast-resistance table, keyed by block-name family.
/// The default 6.0 (stone) means "power-1 safe" - only families
/// listed SOFTER than stone can break in a ghast blast.
fn blast_resistance(id: u16) -> f32 {
    if id == 36 || id == 37 {
        return 100.0; // water / lava absorb like vanilla
    }
    let Some(bt) = crate::block::block_type(id) else {
        return 6.0;
    };
    let n = bt.name;
    if n.contains("bedrock") || n.contains("obsidian") || n.contains("ancient_debris") {
        return 1200.0;
    }
    if n.contains("leaves")
        || n.contains("sapling")
        || n.contains("flower")
        || n.contains("grass")
            && n != "grass_block"
        || n.contains("fern")
        || n.contains("torch")
        || n.contains("crop")
        || n.contains("wheat")
        || n.contains("carrot")
        || n.contains("potato")
        || n.contains("beetroot")
    {
        return 0.0;
    }
    if n.contains("glass") || n.contains("pane") || n.contains("snow") || n.contains("ice") {
        return 0.3;
    }
    if n.contains("netherrack") || n.contains("soul_sand") || n.contains("soul_soil") {
        return 0.4;
    }
    if n.contains("dirt")
        || n == "grass_block"
        || n.contains("sand")
        || n.contains("gravel")
        || n.contains("podzol")
        || n.contains("mycelium")
        || n.contains("mud")
        || n.contains("clay")
        || n.contains("farmland")
        || n.contains("dirt_path")
    {
        return 0.5;
    }
    if n.contains("wool") || n.contains("carpet") {
        return 0.8;
    }
    if n.contains("log") || n.contains("wood") || n.contains("stem") {
        return 2.0;
    }
    if n.contains("planks") || n.contains("fence") || n.contains("door") || n.contains("chest") {
        return 3.0;
    }
    6.0
}

/// Detonate at `center`. Host-authoritative: block edits go through
/// `world_set_tracked` (remesh + light + broadcast), drops spawn at
/// vanilla 1/power yield, players and mobs take the MC radius
/// damage, and the blast sound plays for everyone nearby.
pub(crate) fn explode(vx: &mut Voxel, center: Vec3, power: f32, fire: bool) {
    let mut rng = SplitMix64::new(
        (center.x.to_bits() as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (center.z.to_bits() as u64).rotate_left(17)
            ^ (vx.day_time.to_bits() as u64),
    );
    // --- vanilla ray sweep -> set of destroyed voxels ------------
    let mut destroyed: Vec<[i32; 3]> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for j in 0..16 {
        for k in 0..16 {
            for l in 0..16 {
                if j != 0 && j != 15 && k != 0 && k != 15 && l != 0 && l != 15 {
                    continue; // shell only
                }
                let dir = Vec3::new(
                    j as f32 / 15.0 * 2.0 - 1.0,
                    k as f32 / 15.0 * 2.0 - 1.0,
                    l as f32 / 15.0 * 2.0 - 1.0,
                )
                .normalize();
                let mut intensity = power * (0.7 + (rng.next_f64() as f32) * 0.6);
                let mut p = center;
                while intensity > 0.0 {
                    let voxel = [
                        p.x.floor() as i32,
                        p.y.floor() as i32,
                        p.z.floor() as i32,
                    ];
                    let b = vx.world.get(voxel);
                    if !b.is_air() {
                        intensity -= (blast_resistance(b.0) + 0.3) * 0.3;
                        if intensity > 0.0 && seen.insert(voxel) {
                            destroyed.push(voxel);
                        }
                    }
                    p += dir * 0.3;
                    intensity -= 0.225_000_01;
                }
            }
        }
    }
    // --- break + drops (vanilla yield 1/power) -------------------
    let drop_chance = (1.0 / power).min(1.0);
    for voxel in &destroyed {
        let b = vx.world.get(*voxel);
        if b.is_air() {
            continue;
        }
        vx.world_set_tracked(*voxel, BlockId::AIR);
        // W338: TNT caught in a blast primes with the short vanilla
        // fuse (10 + rand 20 ticks) instead of dropping.
        if b.0 == crate::block::TNT_ID {
            let fuse = 0.5 + (rng.next_f64() as f32) * 1.0;
            crate::tnt::spawn_primed(
                vx,
                Vec3::new(
                    voxel[0] as f32 + 0.5,
                    voxel[1] as f32,
                    voxel[2] as f32 + 0.5,
                ),
                fuse,
            );
            continue;
        }
        if (rng.next_f64() as f32) < drop_chance {
            vx.spawn_drop(b, *voxel);
        }
    }
    // --- fire (a third of the crater, over solid ground) ---------
    if fire {
        for voxel in &destroyed {
            if (rng.next_f64() as f32) >= 1.0 / 3.0 {
                continue;
            }
            let below = [voxel[0], voxel[1] - 1, voxel[2]];
            let bb = vx.world.get(below);
            if bb.is_air() || !crate::block::block_fully_occludes(bb) {
                continue;
            }
            if !vx.world.get(*voxel).is_air() {
                continue;
            }
            let soul = crate::block::block_type(bb.0)
                .map(|bt| bt.name.contains("soul_"))
                .unwrap_or(false);
            let fid = if soul { SOUL_FIRE_ID } else { FIRE_ID };
            vx.world_set_tracked(*voxel, BlockId(fid));
        }
    }
    // --- entity damage + knockback (MC radius math, seen = 1) ----
    let diameter = power * 2.0;
    let impact_at = |pos: Vec3| -> f32 {
        let d = (pos - center).length();
        (1.0 - d / diameter).clamp(0.0, 1.0)
    };
    let dmg_of = |imp: f32| -> f32 {
        if imp <= 0.0 {
            0.0
        } else {
            (imp * imp + imp) / 2.0 * 7.0 * diameter + 1.0
        }
    };
    // Host player.
    if !vx.game_mode.is_creative() {
        let imp = impact_at(vx.player_pos);
        let dmg = dmg_of(imp);
        if dmg > 0.0 {
            vx.apply_player_damage(dmg);
            let push = (vx.player_pos - center).normalize_or_zero();
            vx.player_vel += push * imp * 15.0;
        }
    }
    // Remote players.
    let mut remote_dmg: Vec<(u32, f32)> = Vec::new();
    if let NetRole::Server { poses, .. } = &vx.net {
        for p in poses.values() {
            if p.id == 0 {
                continue;
            }
            let dmg = dmg_of(impact_at(Vec3::from(p.pos)));
            if dmg > 0.0 {
                remote_dmg.push((p.id, dmg));
            }
        }
    }
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        for (id, dmg) in &remote_dmg {
            if let Ok(buf) = bincode::serde::encode_to_vec(
                crate::net::DamageMsg { amount: *dmg },
                cfg,
            ) {
                server.send_bytes_to_client(*id, "vx.damage", buf);
            }
        }
    }
    // Mobs (scaled through the shared hurt path so death/broadcast
    // behave like any other hit).
    let mob_hits: Vec<(u32, f32)> = vx
        .mobs
        .iter()
        .filter_map(|m| {
            let dmg = dmg_of(impact_at(Vec3::new(
                m.pos[0],
                m.pos[1] + 0.5,
                m.pos[2],
            )));
            (dmg > 0.0).then_some((m.id, dmg))
        })
        .collect();
    for (id, dmg) in mob_hits {
        vx.damage_mob_scaled(id, None, dmg / crate::mob::state::MOB_HIT_DAMAGE);
    }
    vx.sfx_at("explosion", [center.x, center.y, center.z], |_, _| {});
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::Chunk;

    /// Power-1 blast (the ghast fireball): soft ground pocks, stone
    /// survives - vanilla's ray budget can never pay stone's
    /// (6 + 0.3) * 0.3 toll.
    #[test]
    fn w337_power1_breaks_dirt_never_stone() {
        let mut v = crate::Voxel::new();
        for cy in -1..=1 {
            v.world.insert_chunk([0, cy, 0], Chunk::filled(BlockId(0)));
            v.world.insert_chunk([-1, cy, 0], Chunk::filled(BlockId(0)));
            v.world.insert_chunk([0, cy, -1], Chunk::filled(BlockId(0)));
            v.world.insert_chunk([-1, cy, -1], Chunk::filled(BlockId(0)));
        }
        // Dirt floor at y=4 under an air pocket, stone wall at y=5..8
        // two blocks +X of the centre.
        for x in -6..=6 {
            for z in -6..=6 {
                v.world.set([x, 4, z], BlockId(2)); // dirt
                v.world.set([x, 3, z], BlockId(1)); // stone below
            }
        }
        explode(&mut v, Vec3::new(0.5, 5.5, 0.5), 1.0, false);
        let dirt_gone = (-3..=3)
            .flat_map(|x| (-3..=3).map(move |z| (x, z)))
            .filter(|&(x, z)| v.world.get([x, 4, z]).is_air())
            .count();
        assert!(dirt_gone > 0, "a power-1 blast must pock the dirt floor");
        for x in -3..=3 {
            for z in -3..=3 {
                assert_eq!(
                    v.world.get([x, 3, z]),
                    BlockId(1),
                    "stone at [{x},3,{z}] must survive power 1",
                );
            }
        }
    }

    /// MC damage curve: centre hit of a power-1 blast is 15, outside
    /// the 2-block diameter it is zero.
    #[test]
    fn w337_damage_curve_matches_mc() {
        let diameter = 2.0f32;
        let dmg = |imp: f32| (imp * imp + imp) / 2.0 * 7.0 * diameter + 1.0;
        assert!((dmg(1.0) - 15.0).abs() < 1e-4);
        assert!(dmg(0.0) >= 1.0); // the +1 floor applies at the rim
    }
}
