//! Mob AI tick: wander + chase + zombie touch-damage + despawn +
//! broadcast.
//!
//! Owns the per-tick step of every server-side mob. H.1 (this
//! commit) is a pure refactor of the original `mob_ai::tick_mobs`:
//! the wander step, axis-resolved collision against world voxels,
//! surface-snap, zombie touch-damage path, despawn pass and the
//! `vx.mob` broadcast all move here unchanged. H.5 plugs in the
//! `GoalSelector` pattern + replaces this monolithic tick.
//!
//! MC mirror:
//! - `net/minecraft/world/entity/Mob.java::serverAiStep` is the
//!   per-tick AI entry point. We collapse the goal-selector +
//!   pathfinder hooks for the demo and inline a hand-rolled walk
//!   here; the Goal trait re-introduces the indirection in H.5.

use crate::net::{MobEventMsg, MobMsg, NetRole};
use crate::{DAY_LENGTH_S, Voxel};

use super::kinds::KIND_ZOMBIE;
use super::spawn;
use super::state::{
    MOB_DESPAWN_DIST_SQ, MOB_LUNGE_TIME, MOB_MAX_HEALTH, MOB_SPEED,
    MobState, ZOMBIE_ATTACK_COOLDOWN, ZOMBIE_ATTACK_RANGE,
    ZOMBIE_MAX_HEALTH, ZOMBIE_SIGHT_RANGE, ZOMBIE_SPEED,
    ZOMBIE_TOUCH_DAMAGE,
};

/// Authoritative mob AI tick: step every server-side mob
/// toward its target, retarget on arrival / timeout, spawn
/// new mobs when the cap allows. Builds the broadcast list as
/// a side effect.
pub(crate) fn tick_mobs(voxel: &mut Voxel, dt: f32) {
    // Only the authoritative side ticks mobs. Clients render
    // from `remote_mobs` which is updated by `vx.mob`.
    if matches!(voxel.net, NetRole::Client { .. }) {
        return;
    }
    hp_regen_pass(voxel, dt);
    let day_time = voxel.day_time;
    let mut rand_seed: u32 =
        (voxel.net_tick.wrapping_mul(2654435761) ^ 0xC0FFEE) as u32;
    let mut to_remove: Vec<u32> = Vec::new();
    let mobs_count = voxel.mobs.len();
    for i in 0..mobs_count {
        let m = voxel.mobs[i];
        let m = step_one(voxel, m, day_time, dt, &mut rand_seed, &mut to_remove);
        voxel.mobs[i] = m;
        voxel.remote_mobs.insert(
            m.id,
            MobMsg {
                id: m.id,
                kind: m.kind,
                pos: m.pos,
                yaw: m.yaw,
                health: m.health.max(0.0).round() as u8,
            },
        );
    }
    despawn_pass(voxel, &mut to_remove);
    evict(voxel, &to_remove);
    spawn::spawn_pass(voxel, day_time, &mut rand_seed);
    broadcast(voxel);
}

/// HP regen: any mob that hasn't been hit in 6 s slowly
/// refills toward its kind's max. Heal 1 HP/s.
fn hp_regen_pass(voxel: &mut Voxel, dt: f32) {
    for m in voxel.mobs.iter_mut() {
        if m.dying > 0.0 {
            continue;
        }
        let t = voxel
            .mob_no_hit_time
            .entry(m.id)
            .and_modify(|v| *v += dt)
            .or_insert(dt);
        let max = if m.kind == KIND_ZOMBIE {
            ZOMBIE_MAX_HEALTH
        } else {
            MOB_MAX_HEALTH
        };
        if *t > 6.0 && m.health < max {
            m.health = (m.health + 1.0 * dt).min(max);
        }
    }
}

/// Walk a single mob one tick. Pure function over the mob copy +
/// world state; caller writes the result back into `voxel.mobs`.
fn step_one(
    voxel: &mut Voxel,
    mut m: MobState,
    day_time: f32,
    dt: f32,
    rand_seed: &mut u32,
    to_remove: &mut Vec<u32>,
) -> MobState {
    // Dying mobs skip wander + collision: they just count
    // down the fade timer. When it hits zero the body
    // gets evicted from `mobs` + a `vx.mob_remove` goes
    // out so every client drops it from `remote_mobs`.
    if m.dying > 0.0 {
        m.dying = (m.dying - dt).max(0.0);
        voxel.mob_dying.insert(m.id, m.dying);
        if m.dying <= 0.0 {
            to_remove.push(m.id);
        }
        return m;
    }
    let mut rng = spawn::SpawnRng::new(rand_seed);
    // Per-kind targeting:
    //  - Sheep wander randomly + retarget on arrival or
    //    5 s timeout.
    //  - Zombies pick the nearest player within
    //    `ZOMBIE_SIGHT_RANGE` and lock onto their pose;
    //    when no player is in range they fall back to the
    //    sheep wander logic so they don't freeze in place.
    let chase_player_pos: Option<[f32; 3]> = if m.kind == KIND_ZOMBIE {
        voxel.nearest_player_pos(m.pos, ZOMBIE_SIGHT_RANGE)
    } else {
        None
    };
    let speed = if m.kind == KIND_ZOMBIE {
        ZOMBIE_SPEED
    } else {
        MOB_SPEED
    };
    if let Some(p) = chase_player_pos {
        m.target = p;
        m.retarget_at = (day_time + 1.0).rem_euclid(DAY_LENGTH_S);
    }
    let dx = m.target[0] - m.pos[0];
    let dz = m.target[2] - m.pos[2];
    let d2 = dx * dx + dz * dz;
    if d2 < 0.25 || day_time >= m.retarget_at {
        // Pick a new wander target nearby (within 12 blocks).
        let theta = rng.next_f32() * std::f32::consts::TAU;
        let dist = 4.0 + rng.next_f32() * 8.0;
        m.target = [
            m.pos[0] + theta.cos() * dist,
            m.pos[1],
            m.pos[2] + theta.sin() * dist,
        ];
        m.retarget_at = (day_time + 5.0).rem_euclid(DAY_LENGTH_S);
    } else {
        let inv = d2.sqrt().max(1e-3).recip();
        let vx = dx * inv * speed;
        let vz = dz * inv * speed;
        // Axis-resolved collision against world voxels so
        // the sheep can't walk into trees / walls.
        let step_x = vx * dt;
        let step_z = vz * dt;
        let mut ax_block = false;
        let mut az_block = false;
        let try_x = m.pos[0] + step_x;
        let body_y = m.pos[1].floor() as i32;
        if !voxel
            .world
            .get([try_x.floor() as i32, body_y, m.pos[2].floor() as i32])
            .is_air()
            || !voxel
                .world
                .get([try_x.floor() as i32, body_y + 1, m.pos[2].floor() as i32])
                .is_air()
        {
            ax_block = true;
        } else {
            m.pos[0] = try_x;
        }
        let try_z = m.pos[2] + step_z;
        if !voxel
            .world
            .get([m.pos[0].floor() as i32, body_y, try_z.floor() as i32])
            .is_air()
            || !voxel
                .world
                .get([m.pos[0].floor() as i32, body_y + 1, try_z.floor() as i32])
                .is_air()
        {
            az_block = true;
        } else {
            m.pos[2] = try_z;
        }
        // Both axes blocked: bail out by forcing a retarget
        // next tick. Stops the sheep from grinding into a
        // corner forever.
        if ax_block && az_block {
            m.retarget_at = day_time;
        }
        // `Quat::from_rotation_y(yaw)` rotates +X to
        // `(cos yaw, 0, -sin yaw)` in our right-handed
        // Y-up frame, so the world-velocity needs the
        // negated-Z form to keep body + head pointing the
        // same direction the sheep is walking.
        m.yaw = (-vz).atan2(vx);
    }
    // Surface snap: keep the sheep's feet on the ground.
    let wx = m.pos[0].floor() as i32;
    let wz = m.pos[2].floor() as i32;
    let scan_top = m.pos[1].floor() as i32;
    if let Some(sy) = voxel.surface_y_below(wx, wz, scan_top) {
        m.pos[1] = sy as f32 + 1.0;
    }
    // Zombie touch damage: decrement attack cooldown each
    // frame; when in range of the host + cooldown expired,
    // hit the local player + reset.
    if m.kind == KIND_ZOMBIE {
        m.attack_cd = (m.attack_cd - dt).max(0.0);
        let dx = voxel.player_pos.x - m.pos[0];
        let dy = voxel.player_pos.y - m.pos[1];
        let dz = voxel.player_pos.z - m.pos[2];
        let d2 = dx * dx + dy * dy + dz * dz;
        if d2 <= ZOMBIE_ATTACK_RANGE * ZOMBIE_ATTACK_RANGE && m.attack_cd <= 0.0 {
            m.attack_cd = ZOMBIE_ATTACK_COOLDOWN;
            voxel.apply_player_damage(ZOMBIE_TOUCH_DAMAGE);
            // Visual lunge: bump the local timer + tell
            // every other peer to do the same so the
            // animation reads identically on each client.
            voxel.mob_lunge.insert(m.id, MOB_LUNGE_TIME);
            if let NetRole::Server { server, .. } = &voxel.net {
                let cfg = bincode::config::standard();
                let evt = MobEventMsg { id: m.id, kind: 2 };
                if let Ok(buf) = bincode::serde::encode_to_vec(evt, cfg) {
                    server.broadcast_bytes("vx.mob_event", buf);
                }
            }
        }
    }
    m
}

/// Despawn pass: mobs far from every known player get evicted.
fn despawn_pass(voxel: &mut Voxel, to_remove: &mut Vec<u32>) {
    for m in &voxel.mobs {
        if m.dying > 0.0 {
            continue;
        }
        let nearest = voxel.nearest_player_pos(m.pos, 1.0e6);
        if let Some(p) = nearest {
            let dx = p[0] - m.pos[0];
            let dy = p[1] - m.pos[1];
            let dz = p[2] - m.pos[2];
            if dx * dx + dy * dy + dz * dz > MOB_DESPAWN_DIST_SQ
                && !to_remove.contains(&m.id)
            {
                to_remove.push(m.id);
            }
        }
    }
}

/// Evict mobs whose death fade just finished, plus anything
/// the despawn pass flagged.
fn evict(voxel: &mut Voxel, to_remove: &[u32]) {
    for id in to_remove {
        if let Some(pos) = voxel.mobs.iter().position(|m| m.id == *id) {
            voxel.mobs.swap_remove(pos);
        }
        voxel.remote_mobs.remove(id);
        voxel.mob_hurt_flash.remove(id);
        voxel.mob_dying.remove(id);
        voxel.mob_step_timers.remove(id);
        voxel.mob_say_timers.remove(id);
        voxel.mob_last_xz.remove(id);
        if let NetRole::Server { server, .. } = &voxel.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(*id, cfg) {
                server.broadcast_bytes("vx.mob_remove", buf);
            }
        }
    }
}

/// Broadcast every mob's pose to all connected clients.
fn broadcast(voxel: &Voxel) {
    if let NetRole::Server { server, .. } = &voxel.net {
        let cfg = bincode::config::standard();
        for m in &voxel.mobs {
            let msg = MobMsg {
                id: m.id,
                kind: m.kind,
                pos: m.pos,
                yaw: m.yaw,
                health: m.health.max(0.0).round() as u8,
            };
            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                server.broadcast_bytes("vx.mob", buf);
            }
        }
    }
}
