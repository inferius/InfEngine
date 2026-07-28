//! W140 - arrow projectiles (the skeleton's ranged attack).
//!
//! MC mirror: `AbstractArrow` ballistics, MVP cut. Arrows fly with
//! MC's numbers (speed set by the shooter, gravity 0.05 blocks per
//! tick^2 = 20 m/s^2, air drag 0.99/tick), stop in the first solid
//! block they pierce (stick for a few seconds, then despawn) and
//! damage the first player their flight segment crosses. Host-
//! authoritative like mobs: the host simulates + streams `vx.arrow`
//! snapshots; clients render `remote_arrows`.
//!
//! Follow-ups: player-fired bows, arrow pickup of stuck arrows,
//! mob-vs-arrow friendly fire, crit particles, bow-in-hand render
//! on the skeleton model.

use glam::Vec3;
use infengine_voxel::raycast_with;

use crate::Voxel;
use crate::net::{ArrowMsg, DamageMsg, FireballMsg, NetRole};

/// MC projectile gravity: 0.05 blocks/tick^2 at 20 TPS.
pub(crate) const ARROW_GRAVITY: f32 = 20.0;
/// MC per-tick air drag on arrows.
const ARROW_DRAG_PER_TICK: f32 = 0.99;
/// Skeleton arrow damage (MC: 1-4 by difficulty/velocity).
const ARROW_DAMAGE: f32 = 3.0;
/// Flight lifetime cap; a lost arrow despawns quietly.
const ARROW_LIFETIME_S: f32 = 60.0;
/// How long a lodged arrow stays visible in the wall.
const ARROW_STUCK_S: f32 = 10.0;
/// MC skeleton bow muzzle speed: 1.6 blocks/tick.
pub(crate) const SKELETON_ARROW_SPEED: f32 = 32.0;

/// W337: MC `AbstractHurtingProjectile` acceleration - 0.1 b/t^2
/// along the fixed power vector (40 b/s^2), no gravity.
const FIREBALL_ACCEL: f32 = 40.0;
/// Per-tick inertia (terminal speed 1.9 b/t = 38 b/s).
const FIREBALL_INERTIA_PER_TICK: f32 = 0.95;
/// `LargeFireball.onHitEntity` direct damage.
const FIREBALL_DIRECT_DAMAGE: f32 = 6.0;
/// Safety cap - vanilla fireballs fly forever; a lost one despawns.
const FIREBALL_LIFETIME_S: f32 = 15.0;

/// W337: one live ghast fireball (host side).
#[derive(Debug, Clone)]
pub(crate) struct Fireball {
    pub(crate) id: u32,
    pub(crate) pos: Vec3,
    pub(crate) vel: Vec3,
    /// Constant acceleration (the MC power vector, b/s^2).
    pub(crate) accel: Vec3,
    pub(crate) age: f32,
}

/// Spawn a large fireball on the host + let the tick stream it.
/// `dir` must be normalized (the ghast's aim vector).
pub(crate) fn spawn_fireball(vx: &mut Voxel, origin: Vec3, dir: Vec3) {
    let id = vx.next_fireball_id;
    vx.next_fireball_id = vx.next_fireball_id.wrapping_add(1).max(1);
    vx.fireballs.push(Fireball {
        id,
        pos: origin,
        vel: dir * 2.0,
        accel: dir * FIREBALL_ACCEL,
        age: 0.0,
    });
}

/// W337: fireball flight + impact (host only). Mirrors the arrow
/// tick's segment tests; any hit detonates a power-1 MOB explosion
/// with fire (`LargeFireball.onHit`), and a direct body hit adds
/// the 6.0 contact damage first.
pub(crate) fn tick_fireballs(vx: &mut Voxel, dt: f32) {
    if matches!(vx.net, NetRole::Client { .. }) {
        return;
    }
    if vx.fireballs.is_empty() {
        return;
    }
    let mut removed: Vec<u32> = Vec::new();
    let mut host_hits = 0u32;
    let mut remote_hits: Vec<u32> = Vec::new();
    let mut blasts: Vec<Vec3> = Vec::new();
    {
        let world = &vx.world;
        let mut targets: Vec<(Option<u32>, Vec3)> = Vec::new();
        if !vx.game_mode.is_creative() {
            targets.push((None, vx.player_pos));
        }
        if let NetRole::Server { poses, .. } = &vx.net {
            for p in poses.values() {
                if p.id != 0 {
                    targets.push((Some(p.id), Vec3::from(p.pos)));
                }
            }
        }
        for f in vx.fireballs.iter_mut() {
            f.age += dt;
            if f.age > FIREBALL_LIFETIME_S {
                removed.push(f.id);
                continue;
            }
            f.vel += f.accel * dt;
            f.vel *= FIREBALL_INERTIA_PER_TICK.powf(dt * 20.0);
            let step = f.vel * dt;
            let len = step.length();
            if len <= 1e-6 {
                continue;
            }
            let dir = step / len;
            let mut hit_player: Option<(Option<u32>, f32)> = None;
            for (pid, eye) in &targets {
                let bmin = *eye - Vec3::new(0.4, 1.8, 0.4);
                let bmax = *eye + Vec3::new(0.4, 0.1, 0.4);
                if let Some((t_in, t_out)) =
                    crate::world::targeting::ray_aabb(f.pos, dir, bmin, bmax)
                    && t_out >= 0.0
                    && t_in <= len
                    && hit_player.is_none_or(|(_, bt)| t_in.max(0.0) < bt)
                {
                    hit_player = Some((*pid, t_in.max(0.0)));
                }
            }
            let solid = |b: infengine_voxel::BlockId| -> bool {
                !b.is_air() && crate::block::block_fully_occludes(b)
            };
            let block_hit = raycast_with(world, f.pos, dir, len, solid);
            let block_t = block_hit.map(|h| {
                let c = Vec3::new(
                    h.voxel[0] as f32 + 0.5,
                    h.voxel[1] as f32 + 0.5,
                    h.voxel[2] as f32 + 0.5,
                );
                (c - f.pos).length()
            });
            match (hit_player, block_t) {
                (Some((pid, pt)), bt) if bt.is_none_or(|b| pt <= b) => {
                    match pid {
                        None => host_hits += 1,
                        Some(id) => remote_hits.push(id),
                    }
                    blasts.push(f.pos + dir * pt);
                    removed.push(f.id);
                }
                (_, Some(bt)) => {
                    // Detonate just shy of the face it struck.
                    blasts.push(f.pos + dir * (bt - 0.5).max(0.0));
                    removed.push(f.id);
                }
                _ => {
                    f.pos += step;
                }
            }
        }
    }
    for _ in 0..host_hits {
        vx.apply_player_damage(FIREBALL_DIRECT_DAMAGE);
    }
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        for id in remote_hits {
            if let Ok(buf) = bincode::serde::encode_to_vec(
                DamageMsg {
                    amount: FIREBALL_DIRECT_DAMAGE,
                },
                cfg,
            ) {
                server.send_bytes_to_client(id, "vx.damage", buf);
            }
        }
    }
    if !removed.is_empty() {
        vx.fireballs.retain(|f| !removed.contains(&f.id));
        if let NetRole::Server { server, .. } = &vx.net {
            let cfg = bincode::config::standard();
            for id in &removed {
                if let Ok(buf) = bincode::serde::encode_to_vec(*id, cfg) {
                    server.broadcast_bytes("vx.fireball_remove", buf);
                }
            }
        }
    }
    // The explosions LAST - they re-borrow the world mutably.
    for c in blasts {
        crate::explosion::explode(vx, c, 1.0, true);
    }
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        for f in &vx.fireballs {
            let msg = FireballMsg {
                id: f.id,
                pos: [f.pos.x, f.pos.y, f.pos.z],
                vel: [f.vel.x, f.vel.y, f.vel.z],
            };
            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                server.broadcast_bytes("vx.fireball", buf);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Arrow {
    pub(crate) id: u32,
    pub(crate) pos: Vec3,
    pub(crate) vel: Vec3,
    pub(crate) age: f32,
    /// `Some(t)` once lodged in a block: countdown to despawn.
    pub(crate) stuck: Option<f32>,
    /// W417: player-fired arrows hit MOBS and skip player bodies;
    /// mob-fired (skeleton) arrows keep hitting players only.
    pub(crate) from_player: bool,
}

/// Spawn an arrow on the host + announce it. `origin` is the
/// shooter's muzzle (eye-ish), `vel` the full velocity vector.
pub(crate) fn spawn_arrow(
    vx: &mut Voxel,
    origin: Vec3,
    vel: Vec3,
    from_player: bool,
) {
    let id = vx.next_arrow_id;
    vx.next_arrow_id = vx.next_arrow_id.wrapping_add(1).max(1);
    vx.arrows.push(Arrow {
        id,
        pos: origin,
        vel,
        age: 0.0,
        stuck: None,
        from_player,
    });
}

/// MC `shootFromRotation` aim helper: point at the target with the
/// vanilla `+0.2 * horizontal_distance` upward arc compensation,
/// then scale to `speed`.
pub(crate) fn aim_at(origin: Vec3, target: Vec3, speed: f32) -> Vec3 {
    let d = target - origin;
    let dh = (d.x * d.x + d.z * d.z).sqrt();
    Vec3::new(d.x, d.y + dh * 0.2, d.z).normalize_or_zero() * speed
}

/// One simulation step for every arrow (host only). Streams
/// per-tick snapshots like the mob broadcast.
pub(crate) fn tick_arrows(vx: &mut Voxel, dt: f32) {
    if matches!(vx.net, NetRole::Client { .. }) {
        return; // clients render `remote_arrows` from vx.arrow
    }
    if vx.arrows.is_empty() {
        return;
    }
    let mut removed: Vec<u32> = Vec::new();
    let mut host_hits = 0u32;
    let mut remote_hits: Vec<u32> = Vec::new();
    let mut impacts: Vec<Vec3> = Vec::new();
    // W417: (mob_id, damage) hits from player arrows, applied after
    // the world borrow drops.
    let mut mob_hits: Vec<(u32, f32)> = Vec::new();
    let mut picked_up = 0u32;
    let mob_boxes: Vec<(u32, Vec3)> = vx
        .mobs
        .iter()
        .filter(|m| m.health > 0.0)
        .map(|m| (m.id, Vec3::new(m.pos[0], m.pos[1], m.pos[2])))
        .collect();
    {
        let world = &vx.world;
        // Player body boxes the segment can hit: the host (unless
        // creative) + every server-tracked remote pose.
        let mut targets: Vec<(Option<u32>, Vec3)> = Vec::new();
        if !vx.game_mode.is_creative() {
            targets.push((None, vx.player_pos));
        }
        if let NetRole::Server { poses, .. } = &vx.net {
            for p in poses.values() {
                if p.id != 0 {
                    targets.push((Some(p.id), Vec3::from(p.pos)));
                }
            }
        }
        let ppos = vx.player_pos;
        for a in vx.arrows.iter_mut() {
            if let Some(t) = a.stuck.as_mut() {
                // W436: a stuck PLAYER arrow within reach picks
                // back up (AbstractArrow.playerTouch; mob arrows
                // stay pickup-less like MC's CREATIVE_ONLY).
                if a.from_player
                    && (ppos - Vec3::new(0.0, 0.9, 0.0) - a.pos).length()
                        < 1.2
                {
                    picked_up += 1;
                    removed.push(a.id);
                    continue;
                }
                *t -= dt;
                if *t <= 0.0 {
                    removed.push(a.id);
                }
                continue;
            }
            a.age += dt;
            if a.age > ARROW_LIFETIME_S {
                removed.push(a.id);
                continue;
            }
            a.vel.y -= ARROW_GRAVITY * dt;
            a.vel *= ARROW_DRAG_PER_TICK.powf(dt * 20.0);
            let step = a.vel * dt;
            let len = step.length();
            if len <= 1e-6 {
                continue;
            }
            let dir = step / len;
            // Player hit: intersect the flight segment with each
            // body box (poses are eye-level; the body hangs 1.8
            // below and pokes 0.1 above).
            let mut hit_player: Option<(Option<u32>, f32)> = None;
            if !a.from_player {
                for (pid, eye) in &targets {
                    let bmin = *eye - Vec3::new(0.4, 1.8, 0.4);
                    let bmax = *eye + Vec3::new(0.4, 0.1, 0.4);
                    if let Some((t_in, t_out)) =
                        crate::world::targeting::ray_aabb(
                            a.pos, dir, bmin, bmax,
                        )
                        && t_out >= 0.0
                        && t_in <= len
                        && hit_player.is_none_or(|(_, bt)| t_in.max(0.0) < bt)
                    {
                        hit_player = Some((*pid, t_in.max(0.0)));
                    }
                }
            }
            // W417: player arrows test the mob boxes instead
            // (feet-anchored ~1x1.6 body).
            let mut hit_mob: Option<(u32, f32)> = None;
            if a.from_player {
                for (mid, feet) in &mob_boxes {
                    let bmin = *feet - Vec3::new(0.5, 0.1, 0.5);
                    let bmax = *feet + Vec3::new(0.5, 1.6, 0.5);
                    if let Some((t_in, t_out)) =
                        crate::world::targeting::ray_aabb(
                            a.pos, dir, bmin, bmax,
                        )
                        && t_out >= 0.0
                        && t_in <= len
                        && hit_mob.is_none_or(|(_, bt)| t_in.max(0.0) < bt)
                    {
                        hit_mob = Some((*mid, t_in.max(0.0)));
                    }
                }
            }
            // Block hit along the same segment.
            let solid = |b: infengine_voxel::BlockId| -> bool {
                !b.is_air() && crate::block::block_fully_occludes(b)
            };
            let block_hit = raycast_with(world, a.pos, dir, len, solid);
            let block_t = block_hit.map(|h| {
                // Approximate: distance to the hit voxel's center
                // is good enough to order vs the player hit.
                let c = Vec3::new(
                    h.voxel[0] as f32 + 0.5,
                    h.voxel[1] as f32 + 0.5,
                    h.voxel[2] as f32 + 0.5,
                );
                (c - a.pos).length()
            });
            // W417: velocity-scaled damage like AbstractArrow
            // (full bow charge ~9 HP, tapering with drag).
            let arrow_dmg =
                (a.vel.length() / 60.0 * 9.0).clamp(1.0, 12.0);
            if let Some((mid, mt)) = hit_mob
                && block_t.is_none_or(|b| mt <= b)
            {
                mob_hits.push((mid, arrow_dmg));
                impacts.push(a.pos + dir * mt);
                removed.push(a.id);
                continue;
            }
            match (hit_player, block_t) {
                (Some((pid, pt)), bt) if bt.is_none_or(|b| pt <= b) => {
                    // Player first.
                    match pid {
                        None => host_hits += 1,
                        Some(id) => remote_hits.push(id),
                    }
                    impacts.push(a.pos + dir * pt);
                    removed.push(a.id);
                }
                (_, Some(bt)) => {
                    // Lodge into the wall just shy of the surface.
                    a.pos += dir * (bt - 0.55).max(0.0);
                    a.stuck = Some(ARROW_STUCK_S);
                    impacts.push(a.pos);
                }
                _ => {
                    a.pos += step;
                }
            }
        }
    }
    // Side effects after the borrow on arrows/world drops.
    for _ in 0..picked_up {
        vx.pickup(infengine_voxel::BlockId(crate::item::ARROW_ID));
        let p = [vx.player_pos.x, vx.player_pos.y, vx.player_pos.z];
        vx.sfx_at("item/pickup", p, |a, g| {
            let _ = a.play_click(1200.0, 0.10 * g);
        });
    }
    for _ in 0..host_hits {
        vx.apply_player_damage(ARROW_DAMAGE);
    }
    // W417: player-arrow mob damage rides the shared scaled path
    // (attacker Some(0) = the host player, like melee).
    for (mid, dmg) in mob_hits {
        vx.damage_mob_scaled(
            mid,
            Some(0),
            dmg / crate::mob::MOB_HIT_DAMAGE,
        );
    }
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        for id in remote_hits {
            if let Ok(buf) = bincode::serde::encode_to_vec(
                DamageMsg {
                    amount: ARROW_DAMAGE,
                },
                cfg,
            ) {
                server.send_bytes_to_client(id, "vx.damage", buf);
            }
        }
    }
    for p in impacts {
        vx.sfx_at("arrow/hit", [p.x, p.y, p.z], |_, _| {});
    }
    if !removed.is_empty() {
        vx.arrows.retain(|a| !removed.contains(&a.id));
        if let NetRole::Server { server, .. } = &vx.net {
            let cfg = bincode::config::standard();
            for id in &removed {
                if let Ok(buf) = bincode::serde::encode_to_vec(*id, cfg) {
                    server.broadcast_bytes("vx.arrow_remove", buf);
                }
            }
        }
    }
    // Stream live arrows to clients (per tick, like the mob pose
    // broadcast - arrows are few and short-lived).
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        for a in &vx.arrows {
            let msg = ArrowMsg {
                id: a.id,
                pos: [a.pos.x, a.pos.y, a.pos.z],
                vel: [a.vel.x, a.vel.y, a.vel.z],
            };
            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                server.broadcast_bytes("vx.arrow", buf);
            }
        }
    }
}


// ----------------------------------------------------------------
// W430 - the witch's thrown splash potion (`ThrownPotion` MVP).
// ----------------------------------------------------------------

/// MC potion throw speed: 0.75 blocks/tick.
pub(crate) const POTION_SPEED: f32 = 15.0;
/// Splash radius (MC affects entities within 4 blocks, scaled).
const POTION_SPLASH_R: f32 = 3.5;
/// Harming splash at the centre (tapers linearly to the edge).
const POTION_DAMAGE: f32 = 6.0;
const POTION_LIFETIME_S: f32 = 6.0;

pub(crate) struct Potion {
    pub(crate) pos: Vec3,
    pub(crate) vel: Vec3,
    pub(crate) age: f32,
}

pub(crate) fn spawn_potion(vx: &mut Voxel, origin: Vec3, vel: Vec3) {
    vx.thrown_potions.push(Potion { pos: origin, vel, age: 0.0 });
}

/// Host-side potion flight: arrow ballistics, splash on the first
/// solid block OR on closing within a body's reach. Damage tapers
/// with distance like MC's splash scaling. (Flight has no mesh yet -
/// the impact burst + sound carry the read; a trail is a follow-up.)
pub(crate) fn tick_potions(vx: &mut Voxel, dt: f32) {
    if matches!(vx.net, NetRole::Client { .. }) || vx.thrown_potions.is_empty()
    {
        return;
    }
    let mut bursts: Vec<Vec3> = Vec::new();
    {
        let world = &vx.world;
        let mut targets: Vec<Vec3> = Vec::new();
        if !vx.game_mode.is_creative() {
            targets.push(vx.player_pos);
        }
        if let NetRole::Server { poses, .. } = &vx.net {
            for p in poses.values() {
                if p.id != 0 {
                    targets.push(Vec3::from(p.pos));
                }
            }
        }
        for pot in vx.thrown_potions.iter_mut() {
            pot.age += dt;
            pot.vel.y -= ARROW_GRAVITY * dt;
            let step = pot.vel * dt;
            let len = step.length();
            let dir = if len > 1e-6 { step / len } else { Vec3::Y };
            let solid = |b: infengine_voxel::BlockId| -> bool {
                !b.is_air() && crate::block::block_fully_occludes(b)
            };
            let hit_block = raycast_with(world, pot.pos, dir, len, solid).is_some();
            let near_body = targets.iter().any(|t| {
                (*t - Vec3::new(0.0, 0.9, 0.0) - pot.pos).length() < 1.2
            });
            if hit_block || near_body || pot.age > POTION_LIFETIME_S {
                bursts.push(pot.pos);
                pot.age = f32::INFINITY; // mark for removal
            } else {
                pot.pos += step;
            }
        }
    }
    vx.thrown_potions.retain(|p| p.age.is_finite());
    for at in bursts {
        // Splash: host player damage tapers with distance; remote
        // players get the same through vx.damage.
        let d_host = (vx.player_pos - Vec3::new(0.0, 0.9, 0.0) - at).length();
        if d_host < POTION_SPLASH_R && !vx.game_mode.is_creative() {
            let dmg = POTION_DAMAGE * (1.0 - d_host / (POTION_SPLASH_R + 0.5));
            vx.apply_player_damage(dmg.max(1.0));
        }
        if let NetRole::Server { server, poses, .. } = &vx.net {
            let cfg = bincode::config::standard();
            let hits: Vec<(u32, f32)> = poses
                .values()
                .filter(|p| p.id != 0)
                .filter_map(|p| {
                    let d = (Vec3::from(p.pos) - Vec3::new(0.0, 0.9, 0.0) - at)
                        .length();
                    if d < POTION_SPLASH_R {
                        let dmg = (POTION_DAMAGE
                            * (1.0 - d / (POTION_SPLASH_R + 0.5)))
                            .max(1.0);
                        Some((p.id, dmg))
                    } else {
                        None
                    }
                })
                .collect();
            for (pid, dmg) in hits {
                if let Ok(buf) =
                    bincode::serde::encode_to_vec(DamageMsg { amount: dmg }, cfg)
                {
                    server.send_bytes_to_client(pid, "vx.damage", buf);
                }
            }
        }
        // Purple glass-shatter burst + sound at the impact cell.
        let cell = [
            at.x.floor() as i32,
            at.y.floor() as i32,
            at.z.floor() as i32,
        ];
        vx.spawn_place_particles(cell, [0.55, 0.25, 0.75]);
        vx.sfx_at("potion/splash", [at.x, at.y, at.z], |a, g| {
            let _ = a.play_click(900.0, 0.12 * g);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aim_at_adds_the_vanilla_arc_boost() {
        // Level shot at 10 blocks: the aim vector gains +2 y
        // (0.2 * horizontal distance) before normalization.
        let v = aim_at(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 32.0);
        assert!((v.length() - 32.0).abs() < 1e-3);
        assert!(v.y > 0.0, "arc compensation must lift the shot");
        let expect = Vec3::new(10.0, 2.0, 0.0).normalize() * 32.0;
        assert!((v - expect).length() < 1e-3);
    }

    #[test]
    fn aim_at_zero_distance_is_zero() {
        let v = aim_at(Vec3::ONE, Vec3::ONE, 32.0);
        assert_eq!(v, Vec3::ZERO);
    }
}
