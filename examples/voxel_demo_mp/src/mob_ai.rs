//! Mob entity state + AI tick + damage application.
//!
//! Owns the server-side mob list machinery: per-mob struct,
//! kind discriminator constants, balance knobs, the wander +
//! chase + zombie-touch-damage AI tick, the audio companion
//! tick (step swishes + species "say" lines), the mob-aimed
//! raycast for the player's melee swing, and the damage
//! application path that subtracts HP, broadcasts hurt/death
//! events, and drops loot.
//!
//! State stays on `Voxel`: `mobs: Vec<MobState>`,
//! `remote_mobs` (client mirror), `mob_hurt_flash`,
//! `mob_dying`, `mob_lunge`, `mob_step_timers`,
//! `mob_say_timers`, `mob_last_xz`, `mob_no_hit_time`,
//! `next_mob_id`, `last_mob_spawn_time`, `mob_cap`. This
//! module exposes free fns over `&mut Voxel`; the Voxel
//! methods that previously owned the logic stay as thin
//! wrappers so existing call sites (`self.tick_mobs(dt)`,
//! `self.damage_mob(id, who)`, etc.) keep working unchanged.

use std::collections::HashSet;

use glam::Vec3;
use infengine_voxel::{BlockId, CHUNK_SIZE};

use crate::inventory::inv_pickup;
use crate::net::{MobEventMsg, MobMsg, NetRole};
use crate::{DAY_LENGTH_S, VERTICAL_CHUNKS, Voxel, day_night, ray_aabb};

/// Server-side mob state. Not serialised; only `MobMsg` goes
/// over the wire. The server picks a random `target` for the
/// mob to wander toward + retargets when it gets there or 5 s
/// pass.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MobState {
    pub(crate) id: u32,
    pub(crate) kind: u8,
    pub(crate) pos: [f32; 3],
    pub(crate) target: [f32; 3],
    pub(crate) yaw: f32,
    /// Time-of-day clock value at the next forced retarget. Set
    /// to `day_time + RETARGET_AFTER` whenever the mob picks a
    /// new goal; the wander loop re-picks once we cross this.
    pub(crate) retarget_at: f32,
    /// Current hit points. Reaches zero = mob despawns + drops
    /// loot. Reset on spawn to `MOB_MAX_HEALTH`.
    pub(crate) health: f32,
    /// Seconds remaining in the death-fade animation. > 0
    /// means the mob is dying: rendered with falling tint +
    /// tilted orientation, no longer wanders or collides. On 0
    /// the server emits `vx.mob_remove` + evicts.
    pub(crate) dying: f32,
    /// Per-mob attack cooldown timer. Currently used by
    /// zombies: when the value reaches zero the mob deals
    /// contact damage to whichever player it's standing next
    /// to and resets to `ZOMBIE_ATTACK_COOLDOWN`.
    pub(crate) attack_cd: f32,
}

/// Length of the death-fade in seconds. Both server and clients
/// run the same countdown so the animation reads identically on
/// every peer.
pub(crate) const MOB_DEATH_FADE: f32 = 0.55;

pub(crate) const MOB_CAP_DEFAULT: usize = 12;
pub(crate) const MOB_SPEED: f32 = 1.2;
pub(crate) const MOB_MAX_HEALTH: f32 = 8.0;
pub(crate) const MOB_HIT_DAMAGE: f32 = 2.0;
/// Mob kind discriminator carried in `MobMsg.kind` + tagged on
/// every `MobState`. Sheep are passive wanderers, zombies chase
/// the nearest player and deal contact damage at night.
pub(crate) const KIND_SHEEP: u8 = 0;
pub(crate) const KIND_ZOMBIE: u8 = 1;
pub(crate) const KIND_CHICKEN: u8 = 2;
pub(crate) const KIND_PIG: u8 = 3;
/// Per-kind balance knobs.
pub(crate) const ZOMBIE_SPEED: f32 = 1.8;
pub(crate) const ZOMBIE_MAX_HEALTH: f32 = 14.0;
pub(crate) const ZOMBIE_TOUCH_DAMAGE: f32 = 2.5;
pub(crate) const ZOMBIE_ATTACK_COOLDOWN: f32 = 0.9;
pub(crate) const ZOMBIE_ATTACK_RANGE: f32 = 1.4;
pub(crate) const ZOMBIE_SIGHT_RANGE: f32 = 22.0;
/// Squared distance from the nearest player past which a mob
/// is despawned. Stops sheep + zombies from piling up in
/// unloaded chunks the player will never visit again.
pub(crate) const MOB_DESPAWN_DIST_SQ: f32 = 80.0 * 80.0;
/// Seconds the zombie body offsets forward after striking.
/// Drives the lunge animation timer.
pub(crate) const MOB_LUNGE_TIME: f32 = 0.30;
/// Maximum range (in blocks) for the player's melee attack on
/// a mob. Same number as voxel break reach so the two
/// interactions feel symmetric.
pub(crate) const MOB_ATTACK_REACH: f32 = 5.5;

/// Authoritative damage application. Caller must own the
/// world simulation (offline or host server). Subtracts
/// `MOB_HIT_DAMAGE` from the mob's health, broadcasts a hurt
/// or death event, and on death drops loot. `attacker_client_id`
/// is `None` for "no inventory credit" (e.g. fall damage path
/// later).
pub(crate) fn damage_mob(voxel: &mut Voxel, mob_id: u32, attacker_client_id: Option<u32>) {
    damage_mob_scaled(voxel, mob_id, attacker_client_id, 1.0);
}

/// Same as `damage_mob` but applies a multiplier to
/// `MOB_HIT_DAMAGE` so sword-tier weapons can swing harder.
pub(crate) fn damage_mob_scaled(
    voxel: &mut Voxel,
    mob_id: u32,
    attacker_client_id: Option<u32>,
    mult: f32,
) {
    // Find the mob (server-side).
    let Some(idx) = voxel.mobs.iter().position(|m| m.id == mob_id) else {
        return;
    };
    let mob_pos;
    let killed;
    let already_dying;
    {
        let m = &mut voxel.mobs[idx];
        already_dying = m.dying > 0.0;
        if !already_dying {
            m.health -= MOB_HIT_DAMAGE * mult.max(0.1);
        }
        mob_pos = m.pos;
        killed = m.health <= 0.0 && !already_dying;
    }
    // No re-damage during the death fade.
    if already_dying {
        return;
    }
    // Local visual flash on every peer that has the mob.
    voxel.mob_hurt_flash.insert(mob_id, 0.30);
    // Reset the no-hit timer so the regen path waits anew.
    voxel.mob_no_hit_time.insert(mob_id, 0.0);
    // Broadcast the event so remote clients flash + play
    // sound. Hurt = 0, Death = 1.
    if let NetRole::Server { server, .. } = &voxel.net {
        let cfg = bincode::config::standard();
        let evt = MobEventMsg {
            id: mob_id,
            kind: if killed { 1 } else { 0 },
        };
        if let Ok(buf) = bincode::serde::encode_to_vec(evt, cfg) {
            server.broadcast_bytes("vx.mob_event", buf);
        }
    }
    // Hurt SFX locally too.
    voxel.sfx_at("mob/sheep/say", mob_pos, |a, g| {
        let _ = a.play_beep(280.0, 0.12, 0.20 * g);
    });
    if !killed {
        return;
    }
    // Death: start the fade timer. Mob stays in `voxel.mobs`
    // (skipping wander logic) until the fade completes, then
    // `tick_mobs` evicts it + broadcasts `vx.mob_remove`. Loot
    // drops immediately so the attacker doesn't have to wait
    // for the animation.
    voxel.mobs[idx].dying = MOB_DEATH_FADE;
    voxel.mob_dying.insert(mob_id, MOB_DEATH_FADE);
    // Per-kind loot table. Sheep drops 1-3 wool + 1-2 meat;
    // chicken drops 1-2 feathers + 1-2 meat. Quantities are
    // seeded from the mob id so save/replay matches.
    let kind = voxel.mobs[idx].kind;
    let seed = mob_id.wrapping_mul(2654435761);
    let meat_qty = (1 + ((seed >> 8) & 0x1) as u8).min(2);
    let (loot_id, loot_qty) = if kind == KIND_CHICKEN {
        (BlockId(19), (1 + ((seed >> 4) & 0x1) as u8).min(2))
    } else if kind == KIND_PIG {
        // Extra meat, no fibre drop.
        (BlockId(12), (2 + (seed & 0x1) as u8).min(3))
    } else {
        (BlockId(11), (1 + (seed & 0x3) as u8).min(3))
    };
    match attacker_client_id {
        None | Some(0) => {
            for _ in 0..loot_qty {
                voxel.pickup(loot_id);
            }
            for _ in 0..meat_qty {
                voxel.pickup(BlockId(12));
            }
            voxel.kill_count = voxel.kill_count.saturating_add(1);
        }
        Some(cid) => {
            if let NetRole::Server {
                inventories,
                pending_inv_sends,
                ..
            } = &mut voxel.net
                && let Some(inv) = inventories.get_mut(&cid)
            {
                for _ in 0..loot_qty {
                    inv_pickup(&mut inv.inventory, loot_id);
                }
                for _ in 0..meat_qty {
                    inv_pickup(&mut inv.inventory, BlockId(12));
                }
                pending_inv_sends.insert(cid);
            }
        }
    }
    // Particle burst at the death position so the kill reads
    // visually even when the SFX is muted.
    let target_voxel = [
        mob_pos[0].floor() as i32,
        mob_pos[1].floor() as i32,
        mob_pos[2].floor() as i32,
    ];
    voxel.spawn_break_particles(target_voxel);
}

/// Raycast a slab-tested ray against every known mob's AABB.
/// Returns `Some((mob_id, distance))` for the nearest hit
/// inside `max_dist`, or `None`. Uses the rendered cube
/// extents (body 1.2x0.8x0.7 + head bump) approximated as one
/// bigger 1.2x1.4x0.8 box so the player doesn't have to pixel-
/// hunt the small head.
pub(crate) fn raycast_mob(
    voxel: &Voxel,
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
) -> Option<(u32, f32)> {
    let mut best: Option<(u32, f32)> = None;
    for m in voxel.remote_mobs.values() {
        // Per-kind hitbox. Sheep is wide + low, zombie is
        // tall, chicken is a small cube near the feet.
        let (hx, hy, hz, cy_off) = match m.kind {
            KIND_ZOMBIE => (0.45, 1.0, 0.45, 1.0_f32),
            KIND_CHICKEN => (0.28, 0.45, 0.28, 0.35_f32),
            KIND_PIG => (0.6, 0.55, 0.45, 0.45_f32),
            _ => (0.7, 0.75, 0.5, 0.55_f32),
        };
        let cx = m.pos[0];
        let cy = m.pos[1] + cy_off;
        let cz = m.pos[2];
        let min = [cx - hx, cy - hy, cz - hz];
        let max = [cx + hx, cy + hy, cz + hz];
        if let Some(t) = ray_aabb(origin.into(), dir.into(), min, max)
            && t <= max_dist
            && best.is_none_or(|(_, bt)| t < bt)
        {
            best = Some((m.id, t));
        }
    }
    best
}

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
    // HP regen: any mob that hasn't been hit in 6 s slowly
    // refills toward its kind's max. Heal 1 HP/s. We modify
    // the live MobState before the rest of the AI tick uses
    // the values.
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
    // Step every mob toward its target.
    let day_time = voxel.day_time;
    let mut next_id = voxel.next_mob_id;
    let mut rand_seed: u32 =
        (voxel.net_tick.wrapping_mul(2654435761) ^ 0xC0FFEE) as u32;
    let mut rand = || -> u32 {
        rand_seed ^= rand_seed << 13;
        rand_seed ^= rand_seed >> 17;
        rand_seed ^= rand_seed << 5;
        rand_seed
    };
    let mut rand01 = || (rand() as f32 / u32::MAX as f32).clamp(0.0, 1.0);
    // Walk each mob. Collect a fresh broadcast set in
    // `remote_mobs` so the host's own render path stays in sync.
    let mobs_count = voxel.mobs.len();
    let mut to_remove: Vec<u32> = Vec::new();
    for i in 0..mobs_count {
        let mut m = voxel.mobs[i];
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
            continue;
        }
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
            let theta = rand01() * std::f32::consts::TAU;
            let dist = 4.0 + rand01() * 8.0;
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
            // the sheep can't walk into trees / walls. The body
            // is one cell tall; we test both the body level
            // (head height) and the level above the foot to
            // catch tree trunks reliably.
            let step_x = vx * dt;
            let step_z = vz * dt;
            let mut ax_block = false;
            let mut az_block = false;
            let try_x = m.pos[0] + step_x;
            let body_y = m.pos[1].floor() as i32;
            if !voxel.world.get([try_x.floor() as i32, body_y, m.pos[2].floor() as i32]).is_air()
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
        // Scan from one block above the mob's current feet
        // downwards so we ignore tree canopies and other
        // overhead voxels - the sheep should snap to the
        // floor under itself, not the tallest block in the
        // column.
        let wx = m.pos[0].floor() as i32;
        let wz = m.pos[2].floor() as i32;
        let scan_top = m.pos[1].floor() as i32;
        if let Some(sy) = voxel.surface_y_below(wx, wz, scan_top) {
            m.pos[1] = sy as f32 + 1.0;
        }
        // Zombie touch damage: decrement attack cooldown each
        // frame; when in range of the host + cooldown expired,
        // hit the local player + reset. Remote-player damage
        // is server-authoritative but would need a per-id
        // damage message - skipped for the demo; the host
        // gets the attack feedback.
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
    // Despawn pass: mobs far from every known player get
    // evicted (no point ticking + broadcasting an entity the
    // player will never see again). Dying mobs are exempt -
    // we want to finish their fade even when the player
    // walked away.
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
    // Evict mobs whose death fade just finished, plus anything
    // the despawn pass flagged. Broadcast `vx.mob_remove` so
    // clients drop them from the render table at the same
    // moment the local sim does.
    for id in &to_remove {
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
    // Spawn pass: if below cap + last spawn far enough behind,
    // try once. Pick a column near the origin; require its
    // surface to be grass (id 3).
    const MOB_SPAWN_PERIOD: f32 = 6.0;
    let due = ((day_time - voxel.last_mob_spawn_time).rem_euclid(DAY_LENGTH_S))
        > MOB_SPAWN_PERIOD;
    if voxel.mobs.len() < voxel.mob_cap && due {
        // Spawn in a ring 12..32 blocks around the host so
        // the world feels more populated as the player
        // explores. Pure origin-anchored spawns were
        // confusing once they wandered far from spawn.
        let angle = rand01() * std::f32::consts::TAU;
        let radius = 12.0 + rand01() * 20.0;
        let cx = voxel.player_pos.x;
        let cz = voxel.player_pos.z;
        let wx = (cx + angle.cos() * radius).round() as i32;
        let wz = (cz + angle.sin() * radius).round() as i32;
        // Spawn scan: probe the column ceiling so the sheep
        // lands on real ground (canopy is fine here too since
        // mobs spawn from above).
        let ceiling = (CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1;
        if let Some(sy) = voxel.surface_y_below(wx, wz, ceiling) {
            let top = voxel.world.get([wx, sy, wz]);
            if top.0 == 3 {
                // Pick kind: zombies only spawn at night
                // (sun_dir.y < 0) and crowd out sheep with a
                // 60% chance during the dark hours. Daytime is
                // sheep-only.
                let (sun_dir, _sky) = day_night(day_time);
                let is_night = sun_dir.y < 0.0;
                let roll = rand01();
                let kind = if is_night && roll < 0.6 {
                    KIND_ZOMBIE
                } else if !is_night && roll < 0.25 {
                    KIND_CHICKEN
                } else if !is_night && roll < 0.45 {
                    // Daytime mixes in pigs alongside
                    // chickens + sheep.
                    KIND_PIG
                } else {
                    KIND_SHEEP
                };
                let id = next_id;
                next_id = next_id.wrapping_add(1).max(1);
                let pos = [wx as f32 + 0.5, sy as f32 + 1.0, wz as f32 + 0.5];
                let mob = MobState {
                    id,
                    kind,
                    pos,
                    target: pos,
                    yaw: 0.0,
                    retarget_at: (day_time + 1.0).rem_euclid(DAY_LENGTH_S),
                    health: match kind {
                        KIND_ZOMBIE => ZOMBIE_MAX_HEALTH,
                        KIND_CHICKEN => 4.0,
                        KIND_PIG => 10.0,
                        _ => MOB_MAX_HEALTH,
                    },
                    dying: 0.0,
                    attack_cd: 0.0,
                };
                let initial_hp = match kind {
                    KIND_ZOMBIE => ZOMBIE_MAX_HEALTH,
                    KIND_CHICKEN => 4.0,
                    KIND_PIG => 10.0,
                    _ => MOB_MAX_HEALTH,
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
                voxel.last_mob_spawn_time = day_time;
            }
        }
    }
    voxel.next_mob_id = next_id;
    // Broadcast every mob's pose to all connected clients.
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

/// Tick the per-mob step + baa sound timers. Runs on every
/// peer (the server's audio comes through too if the host is
/// playing). Step sounds fire only when the mob actually
/// moved since the last tick; baa fires on a randomised
/// cooldown.
pub(crate) fn tick_mob_audio(voxel: &mut Voxel, dt: f32) {
    // Decay every hurt flash regardless of audio availability
    // so the visual tint clears even when SFX are disabled.
    voxel.mob_hurt_flash.retain(|_, t| {
        *t -= dt;
        *t > 0.0
    });
    // Client-side death fade: server's `tick_mobs` runs the
    // authoritative copy + broadcasts `vx.mob_remove` when the
    // timer hits zero, but every peer also runs the decay
    // locally so the visual fade stays smooth between net
    // ticks. The eventual `vx.mob_remove` (or pure-client
    // simulation timeout) clears the entry.
    voxel.mob_dying.retain(|_, t| {
        *t = (*t - dt).max(0.0);
        *t > 0.0
    });
    voxel.mob_lunge.retain(|_, t| {
        *t = (*t - dt).max(0.0);
        *t > 0.0
    });
    if voxel.audio.is_none() {
        return;
    }
    // Snapshot the mob list so we don't keep `voxel.remote_mobs`
    // borrowed while calling back into `&mut voxel` via sfx_at.
    let mobs: Vec<MobMsg> = voxel.remote_mobs.values().copied().collect();
    // Drop timers for mobs that no longer exist (despawned).
    let live: HashSet<u32> = mobs.iter().map(|m| m.id).collect();
    voxel.mob_step_timers.retain(|id, _| live.contains(id));
    voxel.mob_say_timers.retain(|id, _| live.contains(id));
    voxel.mob_last_xz.retain(|id, _| live.contains(id));
    for m in mobs {
        // Step detection: compare to last known XZ. If moved
        // by more than a small epsilon, count it as active
        // motion; decrement timer + fire when due.
        let last = voxel.mob_last_xz.get(&m.id).copied();
        voxel.mob_last_xz.insert(m.id, [m.pos[0], m.pos[2]]);
        let moved = match last {
            Some([lx, lz]) => {
                let dx = m.pos[0] - lx;
                let dz = m.pos[2] - lz;
                (dx * dx + dz * dz).sqrt() > 0.02
            }
            None => false,
        };
        let step_t = voxel.mob_step_timers.entry(m.id).or_insert(0.45);
        if moved {
            *step_t -= dt;
            if *step_t <= 0.0 {
                *step_t = 0.45;
                let pos = m.pos;
                voxel.sfx_at("mob/sheep/step", pos, |a, g| {
                    let _ = a.play_swish(0.18 * g);
                });
            }
        }
        // Baa: random initial timer, refresh on each fire.
        let say_t = voxel
            .mob_say_timers
            .entry(m.id)
            .or_insert_with(|| 5.0 + (m.id as f32 * 0.71).fract() * 12.0);
        *say_t -= dt;
        if *say_t <= 0.0 {
            // Pseudo-random new delay in [6, 20) s. Hash by
            // mob id + current net tick so we don't get a
            // chorus when many sheep retime together.
            let h = (m.id.wrapping_mul(2654435761) ^ voxel.net_tick as u32) as f32;
            *say_t = 6.0 + (h % 14.0).abs();
            let pos = m.pos;
            voxel.sfx_at("mob/sheep/say", pos, |a, g| {
                let _ = a.play_beep(300.0, 0.18, 0.18 * g);
            });
        }
    }
}
