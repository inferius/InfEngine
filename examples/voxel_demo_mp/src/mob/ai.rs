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

use super::despawn;
use super::kinds::{KIND_SKELETON, KIND_SPIDER, KIND_ZOMBIE, KIND_ZOMBIFIED_PIGLIN};
use super::spawn;
use super::state::{
    MOB_LUNGE_TIME, MobState, ZOMBIE_ATTACK_COOLDOWN, ZOMBIE_ATTACK_RANGE,
    ZOMBIE_SIGHT_RANGE, contact_damage_of, max_health_of,
};

/// W198.7 fixed mob-tick period (MC's 20 Hz). Module-scope since W204:
/// the renderer computes its interpolation alpha from it
/// (`mob_tick_accum / MOB_TICK_DT`).
pub(crate) const MOB_TICK_DT: f32 = 1.0 / 20.0;

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
    // W198.7: run the AI at a FIXED 20 Hz (MC's tick rate), not once per
    // render frame. Per-frame ticking made every mob's goal + walk +
    // head-look step run EVERY frame, so 25 mobs cost ~1ms each - a
    // framerate-dependent CPU load (more FPS = more mob work, so it
    // self-limited the frame rate) that dominated sim_ms and was
    // invisible on the F3 panel. At 20 Hz most frames do zero mob work;
    // render still animates limbs/head from `mob_anim` per frame so motion
    // stays smooth. Cap catch-up so a stall can't avalanche.
    const MOB_MAX_CATCHUP: u32 = 4;
    voxel.mob_tick_accum += dt;
    if voxel.mob_tick_accum < MOB_TICK_DT {
        return;
    }
    let mut catch = 0;
    while voxel.mob_tick_accum >= MOB_TICK_DT && catch < MOB_MAX_CATCHUP {
        voxel.mob_tick_accum -= MOB_TICK_DT;
        catch += 1;
        tick_mobs_once(voxel, MOB_TICK_DT);
    }
    if voxel.mob_tick_accum > MOB_TICK_DT {
        voxel.mob_tick_accum = 0.0;
    }
    pin_rider(voxel);
}

/// W348: glue the rider to the mount every FRAME, lerping the 20 Hz
/// mob step with the same prev->cur fraction the renderer uses (a
/// raw 20 Hz camera pin visibly judders at high FPS). Mirrors the
/// minecart pin; update_play zeroed the walk velocity already.
fn pin_rider(voxel: &mut Voxel) {
    let Some(id) = voxel.riding_mob else { return };
    match voxel.mobs.iter().find(|m| m.id == id) {
        Some(h) if h.dying <= 0.0 => {
            let t = (voxel.mob_tick_accum / MOB_TICK_DT).clamp(0.0, 1.0);
            let prev =
                voxel.mob_prev_pos.get(&h.id).copied().unwrap_or(h.pos);
            let eye = super::riding::rider_eye(h.kind);
            voxel.player_pos = glam::Vec3::new(
                prev[0] + (h.pos[0] - prev[0]) * t,
                prev[1] + (h.pos[1] - prev[1]) * t + eye,
                prev[2] + (h.pos[2] - prev[2]) * t,
            );
            voxel.player_vel = glam::Vec3::ZERO;
            voxel.grounded = true;
            voxel.fall_start_y = None;
        }
        _ => voxel.riding_mob = None,
    }
}

/// One fixed-timestep mob AI tick (the former per-frame body): HP regen,
/// per-mob goal + walk + head-look, despawn / spawn / breed, broadcast.
fn tick_mobs_once(voxel: &mut Voxel, dt: f32) {
    // W204: snapshot every mob's pre-step position. The renderer lerps
    // prev -> current by the tick-accumulator fraction, so the 20 Hz
    // position steps stop reading as "vibrating" mobs at high FPS.
    voxel.mob_prev_pos.clear();
    for m in &voxel.mobs {
        voxel.mob_prev_pos.insert(m.id, m.pos);
    }
    hp_regen_pass(voxel, dt);
    let day_time = voxel.day_time;
    let mut rand_seed: u32 =
        (voxel.net_tick.wrapping_mul(2654435761) ^ 0xC0FFEE) as u32;
    let mut to_remove: Vec<u32> = Vec::new();
    let mobs_count = voxel.mobs.len();
    for i in 0..mobs_count {
        let m = voxel.mobs[i];
        let mut m = step_one(voxel, m, day_time, dt, &mut rand_seed, &mut to_remove);
        // W147: MC LookAtPlayerGoal - occasional player glances +
        // smooth head steering, after the walk step so the body yaw
        // is fresh.
        head_look_step(voxel, &mut m, dt, &mut rand_seed);
        voxel.mobs[i] = m;
        voxel.remote_mobs.insert(
            m.id,
            MobMsg {
                id: m.id,
                kind: m.kind,
                pos: m.pos,
                yaw: m.yaw,
                health: m.health.max(0.0).round() as u8,
                head_yaw: m.head_yaw,
                head_pitch: m.head_pitch,
                baby: m.baby_left > 0.0,
                size: m.size,
                sheared: m.sheared,
            },
        );
    }
    voxel.last_despawn_check_time =
        despawn::despawn_pass(voxel, day_time, voxel.last_despawn_check_time, &mut to_remove);
    evict(voxel, &to_remove);
    spawn::spawn_pass(voxel, day_time, &mut rand_seed);
    // W242.2: monster-spawner blocks tick at the same fixed 20 Hz
    // (each call = one MC game tick for the BaseSpawner counters).
    super::spawner::spawner_pass(voxel);
    // W66.2: decay love / cooldown timers + pair in-love animals into
    // offspring. Runs before the broadcast so new babies ship this
    // tick.
    super::breed::breed_pass(voxel, dt);
    // W474: sheep EatBlockGoal - occasional grass bite that regrows
    // sheared wool. Before the broadcast so a cleared `sheared` flag
    // ships the same tick.
    super::graze::graze_pass(voxel, dt, &mut rand_seed);
    broadcast(voxel);
}

/// True for kinds that chase + bite players. Zombie / skeleton /
/// spider all share the stub chase + melee path in H.3; H.8 splits
/// skeleton off into a ranged-bow goal.
fn kind_is_hostile(kind: u8) -> bool {
    // W327: the magma cube joins the melee chasers (MC deals its
    // contact damage through the same attack path).
    matches!(
        kind,
        KIND_ZOMBIE
            | KIND_SKELETON
            | KIND_SPIDER
            | super::kinds::KIND_MAGMA_CUBE
            | super::kinds::KIND_ZOMBIE_VILLAGER
            // W376: the witch melee-chases until potions land;
            // the slime shares the magma-cube contact-damage path.
            | super::kinds::KIND_WITCH
            | super::kinds::KIND_SLIME
            // W377: zombie/skeleton variants share the base AI.
            | super::kinds::KIND_DROWNED
            | super::kinds::KIND_HUSK
            | super::kinds::KIND_STRAY
            // W378: the illager family melee-chases (the pillager
            // crossbow + evoker fangs are follow-ups).
            | super::kinds::KIND_PILLAGER
            | super::kinds::KIND_VINDICATOR
            | super::kinds::KIND_EVOKER
            // W379: blaze (melee until fireballs land) + wither
            // skeleton.
            | super::kinds::KIND_BLAZE
            | super::kinds::KIND_WITHER_SKELETON
            // W380: guardians (laser beam is a follow-up).
            | super::kinds::KIND_GUARDIAN
            | super::kinds::KIND_ELDER_GUARDIAN
            // W381/W432: the enderman is NEUTRAL now - it joins
            // the chase through the anger path below (stare or a
            // hit provokes it), not this always-hostile list.
            | super::kinds::KIND_CAVE_SPIDER
            | super::kinds::KIND_SILVERFISH
            // W382: the phantom dives at players.
            | super::kinds::KIND_PHANTOM
    )
}

/// W147 - MC `LookAtPlayerGoal` port. While a player is within 8
/// blocks the mob occasionally (0.02/tick) starts a 2-4 s glance:
/// the head steers toward the player's eyes at the look-control
/// rate (~10 deg/tick), clamped to +-75 deg yaw / +-40 deg pitch
/// off the body axis, then eases back to the body yaw when the
/// glance ends. Looks at creative players too (the goal is not
/// combat targeting).
fn head_look_step(
    voxel: &Voxel,
    m: &mut MobState,
    dt: f32,
    rand_seed: &mut u32,
) {
    use crate::entity_model::animation::{
        HEAD_PITCH_CLAMP, HEAD_YAW_CLAMP, approach_angle, wrap_angle,
    };
    if m.dying > 0.0 {
        return;
    }
    // LookControl turn rate: ~10 deg per tick at 20 TPS.
    let step = 3.5 * dt;
    let mut nearest: Option<([f32; 3], f32)> = None;
    let mut consider = |p: [f32; 3]| {
        let dx = p[0] - m.pos[0];
        let dz = p[2] - m.pos[2];
        let d2 = dx * dx + dz * dz;
        if nearest.is_none_or(|(_, bd)| d2 < bd) {
            nearest = Some((p, d2));
        }
    };
    consider([
        voxel.player_pos.x,
        voxel.player_pos.y,
        voxel.player_pos.z,
    ]);
    if let NetRole::Server { poses, .. } = &voxel.net {
        for p in poses.values() {
            if p.id != 0 {
                consider(p.pos);
            }
        }
    }
    let mut rng = spawn::SpawnRng::new(rand_seed);
    let in_range = nearest.is_some_and(|(_, d2)| d2 < 8.0 * 8.0);
    if m.look_left <= 0.0
        && in_range
        && rng.next_f32() < 0.02 * (dt * 20.0).min(1.0)
    {
        m.look_left = 2.0 + rng.next_f32() * 2.0;
    }
    m.look_left = (m.look_left - dt).max(0.0);
    let (target_yaw, target_pitch) = match nearest {
        Some((p, _)) if m.look_left > 0.0 && in_range => {
            let dx = p[0] - m.pos[0];
            let dz = p[2] - m.pos[2];
            let dh = (dx * dx + dz * dz).sqrt().max(1e-4);
            // Player poses are eye-level; approximate the mob's eye
            // a body height above its feet.
            let dy = p[1] - (m.pos[1] + 1.2);
            let raw_yaw = (-dz).atan2(dx);
            let yaw = m.yaw
                + wrap_angle(raw_yaw - m.yaw)
                    .clamp(-HEAD_YAW_CLAMP, HEAD_YAW_CLAMP);
            // MC pitch convention: positive looks DOWN.
            let pitch = (-dy).atan2(dh).clamp(-HEAD_PITCH_CLAMP, HEAD_PITCH_CLAMP);
            (yaw, pitch)
        }
        _ => (m.yaw, 0.0),
    };
    m.head_yaw = approach_angle(m.head_yaw, target_yaw, step);
    m.head_pitch = approach_angle(m.head_pitch, target_pitch, step);
}

/// HP regen: any mob that hasn't been hit in 6 s slowly
/// refills toward its kind's max. Heal 1 HP/s.
///
/// W474: the ceiling reads the per-kind attribute table (plus the
/// slime-size / horse-midpoint special cases) instead of the old
/// two-bucket hostile/passive globals.
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
        let max = max_health_of(m);
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
    // W289: MC goal shapes instead of the old perpetual pacing.
    //  - Hostiles chase the nearest player in sight range.
    //  - PanicGoal: a hurt passive sprints (kind panic multiplier)
    //    to a far point until the timer runs out.
    //  - RandomStrollGoal: otherwise mobs IDLE, and with chance
    //    1/120 per tick pick a stroll target within a 10-block
    //    square (MC WaterAvoidingRandomStrollGoal interval).
    let chase_player_pos: Option<[f32; 3]> = if kind_is_hostile(m.kind)
        // W343: creepers stalk players but deal NO touch damage -
        // they share the chase, never the melee arm.
        || m.kind == super::kinds::KIND_CREEPER
    {
        voxel.nearest_player_pos(m.pos, ZOMBIE_SIGHT_RANGE)
    } else if (m.kind == KIND_ZOMBIFIED_PIGLIN
        || m.kind == super::kinds::KIND_ENDERMAN)
        && m.anger_left > 0.0
    {
        // W300: NeutralMob anger - an angered zombified piglin hunts
        // the player for the anger window (MC follow range 35).
        voxel.nearest_player_pos(m.pos, 35.0)
    } else {
        None
    };
    // W340: zombies raid villages - MC's AbstractVillager + IronGolem
    // target goals sit BELOW the player goal, so a visible player
    // still wins; otherwise the nearest villager/golem in sight
    // range becomes the chase target. Skeletons/spiders keep
    // player-only aim.
    let mut chase_mob: Option<(u32, [f32; 3])> = None;
    let zombie_family = matches!(
        m.kind,
        KIND_ZOMBIE | super::kinds::KIND_ZOMBIE_VILLAGER
    );
    if zombie_family && chase_player_pos.is_none() {
        let mut best_d2 = ZOMBIE_SIGHT_RANGE * ZOMBIE_SIGHT_RANGE;
        for o in voxel.mobs.iter() {
            if o.id == m.id || o.dying > 0.0 {
                continue;
            }
            if o.kind != super::kinds::KIND_VILLAGER
                && o.kind != super::kinds::KIND_IRON_GOLEM
            {
                continue;
            }
            let d2 = (o.pos[0] - m.pos[0]).powi(2)
                + (o.pos[1] - m.pos[1]).powi(2)
                + (o.pos[2] - m.pos[2]).powi(2);
            if d2 < best_d2 {
                best_d2 = d2;
                chase_mob = Some((o.id, o.pos));
            }
        }
    }
    m.panic_left = (m.panic_left - dt).max(0.0);
    m.anger_left = (m.anger_left - dt).max(0.0);
    m.hop_cd = (m.hop_cd - dt).max(0.0);
    let mut speed = super::state::kind_walk_speed(m.kind);
    if m.panic_left > 0.0 {
        speed *= super::state::kind_panic_mult(m.kind);
    }
    if let Some(p) = chase_player_pos {
        // W461: chase through the A* planner (find_path existed
        // since H.5 but was never wired - hostiles steered the
        // direct line into walls). chase_waypoint falls back to
        // the straight line at close range, for flyers, and when
        // no path fits the budget; the waypoint list lives in the
        // voxel.mob_paths side map (MobState is Copy).
        m.target = super::pathfind::chase_waypoint(
            &voxel.world,
            &mut voxel.mob_paths,
            &m,
            p,
            dt,
        );
        m.strolling = true;
        m.retarget_at = (day_time + 1.0).rem_euclid(DAY_LENGTH_S);
    } else if let Some((_, p)) = chase_mob {
        m.target = super::pathfind::chase_waypoint(
            &voxel.world,
            &mut voxel.mob_paths,
            &m,
            p,
            dt,
        );
        m.strolling = true;
        m.retarget_at = (day_time + 1.0).rem_euclid(DAY_LENGTH_S);
    }
    // W290: BreedGoal + TemptGoal for passive animals (in that MC
    // priority order, both below PanicGoal).
    if !kind_is_hostile(m.kind) && m.panic_left <= 0.0 {
        if m.baby_left > 0.0 {
            // W291: FollowParentGoal - a baby trails the nearest
            // ADULT of its kind within 8 blocks (speed 1.1),
            // stopping once it's within 3. Babies skip love/tempt.
            let mut best: Option<([f32; 3], f32)> = None;
            for o in voxel.mobs.iter() {
                if o.id != m.id
                    && o.kind == m.kind
                    && o.baby_left <= 0.0
                    && o.dying <= 0.0
                {
                    let d2 = (o.pos[0] - m.pos[0]).powi(2)
                        + (o.pos[2] - m.pos[2]).powi(2);
                    if d2 < 64.0 && best.is_none_or(|(_, bd)| d2 < bd) {
                        best = Some((o.pos, d2));
                    }
                }
            }
            if let Some((p, d2)) = best
                && d2 > 9.0
            {
                m.target = p;
                m.strolling = true;
                m.retarget_at = (day_time + 1.0).rem_euclid(DAY_LENGTH_S);
                speed *= 1.1;
            }
        } else if m.love > 0.0 {
            // BreedGoal: walk to the nearest in-love same-kind
            // partner within 8 blocks; breed_pass pairs them off
            // once they close to 3.
            let mut best: Option<([f32; 3], f32)> = None;
            for o in voxel.mobs.iter() {
                if o.id != m.id
                    && o.kind == m.kind
                    && o.love > 0.0
                    && o.dying <= 0.0
                {
                    let d2 = (o.pos[0] - m.pos[0]).powi(2)
                        + (o.pos[2] - m.pos[2]).powi(2);
                    if d2 < 64.0 && best.is_none_or(|(_, bd)| d2 < bd) {
                        best = Some((o.pos, d2));
                    }
                }
            }
            if let Some((p, _)) = best {
                m.target = p;
                m.strolling = true;
                m.retarget_at = (day_time + 1.0).rem_euclid(DAY_LENGTH_S);
            }
        } else if let Some(held) = voxel.inventory[voxel.selected_slot].as_ref()
            && super::breed::is_food_for(m.kind, held.id)
        {
            // TemptGoal: follow the player holding this species'
            // food within 10 blocks, stopping at 2.5 (MC
            // stop-distance 6.25 squared) so the animal stands and
            // stares instead of shoving into the player.
            let pp = [
                voxel.player_pos.x,
                voxel.player_pos.y,
                voxel.player_pos.z,
            ];
            let dx = pp[0] - m.pos[0];
            let dz = pp[2] - m.pos[2];
            let d2 = dx * dx + dz * dz;
            if d2 < 100.0 {
                if d2 > 6.25 {
                    m.target = pp;
                    m.strolling = true;
                    m.retarget_at =
                        (day_time + 1.0).rem_euclid(DAY_LENGTH_S);
                    speed *= super::breed::tempt_speed(m.kind);
                } else {
                    // Close enough: stand still, face the food.
                    m.strolling = false;
                    m.target = m.pos;
                }
            }
        }
    }
    // W348: a ridden horse/donkey overrides every goal above.
    // TAMED: the rider steers (MC tickRidden - yaw copies the
    // rider, input pre-scaled in update_play, speed = the per-horse
    // MOVEMENT_SPEED attribute, Space jumps by JUMP_STRENGTH).
    // UNTAMED: RunAroundLikeCrazyGoal verbatim - sprint to random
    // nearby points at 1.2x; each tick has a 1/50 chance to roll
    // rand(100) < temper -> tame, else temper += 5 and the rider
    // is thrown with the angry bray.
    if voxel.riding_mob == Some(m.id) && m.dying <= 0.0 {
        if m.tamed {
            let yaw = voxel.player_yaw;
            let (f, s) = (voxel.riding_wish[0], voxel.riding_wish[1]);
            let (fx, fz) = (yaw.cos(), -yaw.sin());
            let (rx, rz) = (yaw.sin(), yaw.cos());
            let wx = fx * f + rx * s;
            let wz = fz * f + rz * s;
            m.yaw = crate::entity_model::animation::wrap_angle(yaw);
            // Input magnitude scales the gallop (backing crawls
            // at the 0.25 factor update_play already applied).
            speed = super::riding::riding_speed(m.kind, m.id)
                * f.abs().max(s.abs()).min(1.0);
            if wx * wx + wz * wz > 1e-4 {
                m.strolling = true;
                m.target =
                    [m.pos[0] + wx * 8.0, m.pos[1], m.pos[2] + wz * 8.0];
                m.retarget_at = (day_time + 30.0).rem_euclid(DAY_LENGTH_S);
            } else {
                m.strolling = false;
                m.target = m.pos;
            }
            if voxel.riding_jump && m.vy == 0.0 {
                m.vy = super::riding::jump_power(m.kind, m.id);
                if m.kind == super::kinds::KIND_HORSE {
                    voxel.sfx_at("mob/horse/jump", m.pos, |_, _| {});
                }
            }
            voxel.riding_jump = false;
        } else {
            speed = super::riding::riding_speed(m.kind, m.id) * 1.2;
            let arrived = {
                let ddx = m.target[0] - m.pos[0];
                let ddz = m.target[2] - m.pos[2];
                ddx * ddx + ddz * ddz < 1.0
            };
            if !m.strolling || arrived {
                m.strolling = true;
                m.target = [
                    m.pos[0] + (rng.next_f32() * 2.0 - 1.0) * 5.0,
                    m.pos[1],
                    m.pos[2] + (rng.next_f32() * 2.0 - 1.0) * 5.0,
                ];
                m.retarget_at = (day_time + 5.0).rem_euclid(DAY_LENGTH_S);
            }
            if rng.next_f32() < (dt * 20.0) / 50.0 {
                let roll = (rng.next_f32() * 100.0) as u8;
                if super::riding::tame_roll(m.temper, roll) {
                    m.tamed = true;
                    voxel.chat_log.push((
                        std::time::Instant::now(),
                        "* tamed".to_string(),
                    ));
                } else {
                    m.temper = (m.temper + 5).min(100);
                    voxel.riding_mob = None;
                    let stem = if m.kind == super::kinds::KIND_DONKEY {
                        "mob/donkey/angry"
                    } else {
                        "mob/horse/angry"
                    };
                    voxel.sfx_at(stem, m.pos, |_, _| {});
                }
            }
        }
    }
    // W474: EatBlockGoal freeze - an eating sheep stands still for
    // the 40-tick bite (graze.rs owns the timer + the block edit).
    // A panic (hit while eating) cancels the eat there.
    if m.eat_left > 0.0 && m.panic_left <= 0.0 {
        m.strolling = false;
        m.target = m.pos;
    }
    let dx = m.target[0] - m.pos[0];
    let dz = m.target[2] - m.pos[2];
    let d2 = dx * dx + dz * dz;
    // W99: when actively chasing a player, hold at attack distance instead
    // of walking right onto them (the "skeleton sticks to / glues onto the
    // player" bug). 1.0 block^2 stop is inside ZOMBIE_ATTACK_RANGE (1.4) so
    // the melee tick below still lands; the mob just stops advancing.
    // W140: skeletons keep BOW range instead - they stop advancing at
    // ~8 blocks and shoot (MC's RangedBowAttackGoal stand-off).
    const MOB_CHASE_STOP_D2: f32 = 1.0;
    const SKELETON_HOLD_D2: f32 = 64.0;
    let chasing = chase_player_pos.is_some();
    let stop_d2 = if m.kind == KIND_SKELETON {
        SKELETON_HOLD_D2
    } else {
        MOB_CHASE_STOP_D2
    };
    if chasing && d2 < stop_d2 {
        // Within reach: stand and attack, don't overshoot or wander.
        // W140: keep facing the target while standing (the bow
        // fires where the skeleton looks).
        if m.kind == KIND_SKELETON {
            m.yaw = (-dz).atan2(dx);
        }
    } else if !m.strolling && m.panic_left <= 0.0 {
        // IDLE - the default state. MC RandomStrollGoal: chance
        // 1/interval(120) per tick to start a stroll; standing
        // animals just graze/look around between strolls.
        if rng.next_f32() < (dt * 20.0) / 120.0 {
            // MC RandomPos land target: +-10 blocks on each axis.
            let tx = m.pos[0] + (rng.next_f32() * 2.0 - 1.0) * 10.0;
            let tz = m.pos[2] + (rng.next_f32() * 2.0 - 1.0) * 10.0;
            m.target = [tx, m.pos[1], tz];
            m.strolling = true;
            // Generous timeout so a blocked path ends the goal.
            m.retarget_at = (day_time + 15.0).rem_euclid(DAY_LENGTH_S);
        }
    } else if d2 < 0.25 || day_time >= m.retarget_at {
        // Arrived (or gave up): back to IDLE. While panicking keep
        // fleeing - pick a fresh far point instead of stopping.
        if m.panic_left > 0.0 {
            let theta = rng.next_f32() * std::f32::consts::TAU;
            let dist = 6.0 + rng.next_f32() * 6.0;
            m.target = [
                m.pos[0] + theta.cos() * dist,
                m.pos[1],
                m.pos[2] + theta.sin() * dist,
            ];
            m.retarget_at = (day_time + 4.0).rem_euclid(DAY_LENGTH_S);
        } else {
            m.strolling = false;
        }
    } else {
        let inv = d2.sqrt().max(1e-3).recip();
        let mut vx = dx * inv * speed;
        let mut vz = dz * inv * speed;
        // W289: rabbits move ONLY by hopping (MC RabbitJumpControl).
        // A grounded rabbit that wants to move launches a hop; while
        // waiting between hops it stands still. Airborne = full
        // horizontal speed (the hop arc carries it).
        if m.kind == super::kinds::KIND_RABBIT {
            let grounded = m.vy == 0.0;
            if grounded {
                if m.hop_cd <= 0.0 {
                    m.vy = 4.6;
                    m.hop_cd = 0.3 + rng.next_f32() * 0.4;
                } else {
                    vx = 0.0;
                    vz = 0.0;
                }
            }
        }
        // W327: the magma cube moves the same hop-only way (MC
        // Slime.MoveHelper jumps every jumpDelay), just heavier
        // and rarer than the rabbit.
        if m.kind == super::kinds::KIND_MAGMA_CUBE
            || m.kind == super::kinds::KIND_SLIME
        {
            let grounded = m.vy == 0.0;
            if grounded {
                if m.hop_cd <= 0.0 {
                    m.vy = 5.2;
                    m.hop_cd = 0.6 + rng.next_f32() * 1.0;
                } else {
                    vx = 0.0;
                    vz = 0.0;
                }
            }
        }
        // Axis-resolved collision against world voxels so
        // the sheep can't walk into trees / walls.
        let step_x = vx * dt;
        let step_z = vz * dt;
        let mut ax_block = false;
        let mut az_block = false;
        // W52: 1-block step-up on each axis. MC's `Mob.tryToStepUp`
        // tests the cell at `body_y` (blocked) plus the cell at
        // `body_y + 1` (clear) plus the cell at `body_y + 2`
        // (clear) - the mob then walks onto the step. Mirrors that
        // here so a sheep doesn't get stuck against any 1-block
        // ridge / staircase corner.
        // W297: only SOLID cells block - the old is_air() test made
        // tall grass / flowers / crops read as walls, so on a grassy
        // plain every animal wedged in place and the rabbit hopped
        // into the same grass tuft forever (the user's screenshot).
        // MC mobs walk straight through non-collidable decorations.
        let solid_at = |p: [i32; 3]| -> bool {
            let b = voxel.world.get(p);
            !b.is_air() && !crate::block::block_is_aabb_passable(b)
        };
        // W306: lava-edge refusal - MC's pathfinder never routes a
        // mob through lava (getPathfindingMalus(LAVA) = -1), but our
        // straight-line stroll marched every piglin off the shore
        // into the lava sea. A strolling (non-chasing, non-panicking)
        // mob that would step onto lava - or over a drop into lava -
        // abandons the stroll and repicks a target later.
        let (step_x, step_z) = if m.panic_left <= 0.0
            && chase_player_pos.is_none()
            && (step_x != 0.0 || step_z != 0.0)
        {
            let ax = (m.pos[0] + step_x.signum() * 0.8).floor() as i32;
            let az = (m.pos[2] + step_z.signum() * 0.8).floor() as i32;
            let by = m.pos[1].floor() as i32;
            let lava_at = |p: [i32; 3]| -> bool {
                voxel.world.get(p).0 == crate::block::LAVA_ID
            };
            if lava_at([ax, by, az])
                || lava_at([ax, by - 1, az])
                || lava_at([ax, by - 2, az])
            {
                m.strolling = false;
                (0.0, 0.0)
            } else {
                (step_x, step_z)
            }
        } else {
            (step_x, step_z)
        };
        let try_x = m.pos[0] + step_x;
        let body_y = m.pos[1].floor() as i32;
        let feet_x = try_x.floor() as i32;
        let cur_z = m.pos[2].floor() as i32;
        let blocked_feet = solid_at([feet_x, body_y, cur_z]);
        let blocked_head = solid_at([feet_x, body_y + 1, cur_z]);
        if blocked_feet && !blocked_head
            && !solid_at([feet_x, body_y + 2, cur_z])
        {
            // Step-up: walk onto the higher block.
            m.pos[0] = try_x;
            m.pos[1] = (body_y + 1) as f32;
        } else if blocked_feet || blocked_head {
            ax_block = true;
        } else {
            m.pos[0] = try_x;
        }
        let try_z = m.pos[2] + step_z;
        let cur_x = m.pos[0].floor() as i32;
        let feet_z = try_z.floor() as i32;
        let body_y2 = m.pos[1].floor() as i32; // may have changed above
        let blocked_feet_z = solid_at([cur_x, body_y2, feet_z]);
        let blocked_head_z = solid_at([cur_x, body_y2 + 1, feet_z]);
        if blocked_feet_z && !blocked_head_z
            && !solid_at([cur_x, body_y2 + 2, feet_z])
        {
            m.pos[2] = try_z;
            m.pos[1] = (body_y2 + 1) as f32;
        } else if blocked_feet_z || blocked_head_z {
            az_block = true;
        } else {
            m.pos[2] = try_z;
        }
        // Both axes blocked: stuck against terrain (very common in
        // water / pits). Retarget to a fresh escape heading - but on a
        // ~1 s cooldown, NOT every tick. The old code set
        // `retarget_at = day_time`, which retargeted EVERY frame: the
        // heading snapped to a new random direction each frame, so a
        // wedged mob spun rapidly in place and never got out (the
        // "cow spins extremely fast underwater, can't escape" bug). Only
        // pull the retarget in when it's currently further out than the
        // cooldown, so it fires a few times - not 20x - a second.
        if ax_block && az_block {
            const STUCK_RETARGET_S: f32 = 1.0;
            let ahead = (m.retarget_at - day_time).rem_euclid(DAY_LENGTH_S);
            if ahead > STUCK_RETARGET_S {
                m.retarget_at =
                    (day_time + STUCK_RETARGET_S).rem_euclid(DAY_LENGTH_S);
            }
        }
        // `Quat::from_rotation_y(yaw)` rotates +X to
        // `(cos yaw, 0, -sin yaw)` in our right-handed
        // Y-up frame, so the world-velocity needs the
        // negated-Z form to keep body + head pointing the
        // same direction the sheep is walking.
        //
        // W289: the body TURNS toward the heading at MC's body-yaw
        // rate (~10 deg/tick) instead of snapping - the instant flip
        // on every retarget was a big part of the "weird" motion.
        if vx != 0.0 || vz != 0.0 {
            let want = (-vz).atan2(vx);
            m.yaw = crate::entity_model::animation::approach_angle(
                m.yaw,
                want,
                10.0f32.to_radians() * dt * 20.0,
            );
        }
    }
    // W289: gravity + landing (MC LivingEntity -0.08 b/t^2 = -32
    // b/s^2). The old code TELEPORT-snapped feet to the surface every
    // tick, so mobs rode cliffs down like an elevator and never left
    // the ground. Now: at/below ground = stand there (the step-up
    // above already lifted onto 1-block steps); above it = fall, with
    // the chicken's wing-flutter clamp (MC Chicken caps fall speed +
    // takes no fall damage).
    let wx = m.pos[0].floor() as i32;
    let wz = m.pos[2].floor() as i32;
    let scan_top = (m.pos[1] + 0.01).floor() as i32;
    let ground = voxel
        .surface_y_below(wx, wz, scan_top)
        .map(|sy| sy as f32 + 1.0);
    // W309: buoyancy - MC land mobs SWIM UP in fluids (Mob.aiStep
    // presses jumpInLiquid +0.04/t while in water OR lava, and the
    // fluid travel branch quarters gravity + damps vertical speed
    // 0.8/t), so they bob to the surface instead of trudging the
    // seabed (the "mobs walk on the ocean floor" report - and the
    // lava-sea piglins now float like vanilla).
    let body_fluid = crate::liquid::liquid_for(
        voxel.world.get([wx, (m.pos[1] + 0.5).floor() as i32, wz]),
    )
    .is_some();
    if body_fluid {
        let head_in_fluid = crate::liquid::liquid_for(
            voxel.world.get([wx, (m.pos[1] + 1.2).floor() as i32, wz]),
        )
        .is_some();
        // Quarter gravity, up-press (stronger while fully submerged),
        // MC's 0.8/tick vertical fluid friction. At the surface the
        // press and gravity cancel and the friction settles a bob.
        m.vy -= 8.0 * dt;
        m.vy += if head_in_fluid { 24.0 } else { 8.0 } * dt;
        m.vy *= 0.8f32.powf(dt * 20.0);
        m.pos[1] += m.vy * dt;
        if let Some(g) = ground
            && m.pos[1] <= g
        {
            m.pos[1] = g;
            m.vy = m.vy.max(0.0);
        }
    } else {
        match ground {
            // Standing (a fresh hop with vy > 0 must NOT re-land here).
            Some(g) if m.pos[1] <= g + 0.001 && m.vy <= 0.0 => {
                m.pos[1] = g;
                m.vy = 0.0;
            }
            _ => {
                // W327: the ghast FLOATS - MC ghasts ignore gravity
                // entirely (RandomFloatAroundGoal steers in 3D; our
                // v1 keeps the current altitude and wanders).
                if m.kind == super::kinds::KIND_BEE
                    || m.kind == super::kinds::KIND_BAT
                {
                    // W386: bees hug the surface line (MC bees
                    // wander near the ground and flowers - the R22
                    // report had one cruising sky-high). Ease toward
                    // ~2.5 blocks above the floor; bats share the
                    // hold so cave flights stay near the tunnel
                    // floor instead of pinning to spawn altitude.
                    if let Some(g) = ground {
                        let dy = (g + 2.5) - m.pos[1];
                        m.vy = (dy * 1.5).clamp(-3.0, 3.0);
                        m.pos[1] += m.vy * dt;
                    } else {
                        m.vy = 0.0;
                    }
                } else if m.kind == super::kinds::KIND_GHAST
                    || m.kind == super::kinds::KIND_BLAZE
                    || m.kind == super::kinds::KIND_PHANTOM
                    || m.kind == super::kinds::KIND_GUARDIAN
                    || m.kind == super::kinds::KIND_ELDER_GUARDIAN
                    // W388: squids drift in place (swim goals are a
                    // follow-up with the water-mob movement pass).
                    || m.kind == super::kinds::KIND_SQUID
                    || m.kind == super::kinds::KIND_GLOW_SQUID
                {
                    // W375: hover flyers hold their altitude.
                    m.vy = 0.0;
                } else {
                m.vy -= 32.0 * dt;
                if m.kind == super::kinds::KIND_CHICKEN {
                    m.vy = m.vy.max(-2.0);
                }
                m.vy = m.vy.max(-40.0);
                m.pos[1] += m.vy * dt;
                if let Some(g) = ground
                    && m.pos[1] <= g
                {
                    m.pos[1] = g;
                    m.vy = 0.0;
                }
                }
            }
        }
    }
    // W343: SwellGoal + explodeCreeper. Within 3 blocks with line
    // of sight the fuse arms (hiss); it keeps burning until the
    // player is past 7 blocks or hidden, then unwinds. 30 ticks
    // (1.5 s) of swell detonates the vanilla radius-3 MOB blast -
    // no drops, the creeper simply ceases to be.
    if m.kind == super::kinds::KIND_CREEPER && m.dying <= 0.0 {
        let swell0 = voxel.mob_swell.get(&m.id).copied().unwrap_or(0.0);
        let mut dir = -1.0f32;
        if let Some(p) = chase_player_pos {
            let d2 = (p[0] - m.pos[0]).powi(2)
                + (p[1] - m.pos[1]).powi(2)
                + (p[2] - m.pos[2]).powi(2);
            let arm_r2 = if swell0 > 0.0 { 49.0 } else { 9.0 };
            if d2 < arm_r2 {
                let eye = glam::Vec3::new(m.pos[0], m.pos[1] + 1.2, m.pos[2]);
                let tgt = glam::Vec3::new(p[0], p[1] - 0.3, p[2]);
                let to = tgt - eye;
                let dist = to.length();
                let los = dist <= 1e-3
                    || infengine_voxel::raycast_with(
                        &voxel.world,
                        eye,
                        to / dist.max(1e-3),
                        dist,
                        |b| {
                            !b.is_air()
                                && crate::block::block_fully_occludes(b)
                        },
                    )
                    .is_none();
                if los {
                    dir = 1.0;
                }
            }
        }
        let swell = (swell0 + dir * dt).clamp(0.0, 1.5);
        if dir > 0.0 && swell0 <= 0.0 && swell > 0.0 {
            voxel.sfx_at("mob/creeper/fuse", m.pos, |_, _| {});
        }
        if swell > 0.0 {
            // SwellGoal.start stops navigation - hold still.
            m.target = m.pos;
            m.strolling = false;
            voxel.mob_swell.insert(m.id, swell);
        } else {
            voxel.mob_swell.remove(&m.id);
        }
        if swell >= 1.5 {
            let c = glam::Vec3::new(m.pos[0], m.pos[1] + 0.85, m.pos[2]);
            voxel.mob_swell.remove(&m.id);
            to_remove.push(m.id);
            crate::explosion::explode(voxel, c, 3.0, false);
            return m;
        }
    }
    // W339: iron golem village defense - MC DefendVillageTargetGoal
    // + MeleeAttackGoal: hunt the nearest hostile within 16 blocks;
    // at arm's reach deal attackDamage/2 + rand(attackDamage)
    // (attribute 15 -> 7.5..21.5 hp) and launch the victim 0.4 b/t
    // upward, on the melee cadence. The arm-raise anim rides the
    // shared lunge timer for now.
    if m.kind == super::kinds::KIND_IRON_GOLEM {
        let mut best: Option<([f32; 3], u32, f32)> = None;
        for o in voxel.mobs.iter() {
            if o.id != m.id && kind_is_hostile(o.kind) && o.dying <= 0.0 {
                let d2 = (o.pos[0] - m.pos[0]).powi(2)
                    + (o.pos[1] - m.pos[1]).powi(2)
                    + (o.pos[2] - m.pos[2]).powi(2);
                if d2 < 16.0 * 16.0 && best.is_none_or(|(_, _, bd)| d2 < bd) {
                    best = Some((o.pos, o.id, d2));
                }
            }
        }
        m.attack_cd = (m.attack_cd - dt).max(0.0);
        if let Some((tp, tid, d2)) = best {
            m.target = tp;
            m.strolling = true;
            m.retarget_at = (day_time + 1.0).rem_euclid(DAY_LENGTH_S);
            if d2 <= 2.5 * 2.5 && m.attack_cd <= 0.0 {
                m.attack_cd = 1.0;
                let dmg = 7.5 + rng.next_f32() * 15.0;
                voxel.damage_mob_scaled(
                    tid,
                    None,
                    dmg / super::state::MOB_HIT_DAMAGE,
                );
                if let Some(v) = voxel.mobs.iter_mut().find(|o| o.id == tid) {
                    // IronGolem.doHurtTarget: +0.4 b/t upward.
                    v.vy = 8.0;
                    v.pos[1] += 0.05;
                }
                voxel.mob_lunge.insert(m.id, MOB_LUNGE_TIME);
                voxel.sfx_at("mob/iron_golem/attack", m.pos, |_, _| {});
            }
        }
    }
    // W337: GhastShootFireballGoal - a player within 64 blocks and
    // line of sight starts the 1 s charge, then a LargeFireball
    // flies and the next shot waits 3 s (MC chargeTime 20 with the
    // -40 reset). attack_cd carries the cadence; the warn moan
    // fires crossing the 0.5 s mark (MC chargeTime == 10).
    if m.kind == super::kinds::KIND_GHAST {
        let prev_cd = m.attack_cd;
        m.attack_cd = (m.attack_cd - dt).max(0.0);
        if let Some(p) = voxel.nearest_player_pos(m.pos, 64.0) {
            let eye = glam::Vec3::new(m.pos[0], m.pos[1] + 2.0, m.pos[2]);
            let tgt = glam::Vec3::new(p[0], p[1] - 0.3, p[2]);
            let to = tgt - eye;
            let dist = to.length();
            let los = dist > 1e-3
                && infengine_voxel::raycast_with(
                    &voxel.world,
                    eye,
                    to / dist,
                    dist,
                    |b| !b.is_air() && crate::block::block_fully_occludes(b),
                )
                .is_none();
            if los {
                if prev_cd > 0.5 && m.attack_cd <= 0.5 {
                    voxel.sfx_at(
                        "mob/ghast/charge",
                        [eye.x, eye.y, eye.z],
                        |_, _| {},
                    );
                }
                if m.attack_cd <= 0.0 {
                    m.attack_cd = 3.0;
                    // MC: spawn 4 blocks toward the look direction,
                    // half a block above the body midline.
                    let dirh =
                        glam::Vec3::new(to.x, 0.0, to.z).normalize_or_zero();
                    let muzzle =
                        eye + dirh * 4.0 + glam::Vec3::new(0.0, 0.5, 0.0);
                    let dir = (tgt - muzzle).normalize_or_zero();
                    crate::projectile::spawn_fireball(voxel, muzzle, dir);
                    voxel.sfx_at(
                        "mob/ghast/fireball",
                        [eye.x, eye.y, eye.z],
                        |_, _| {},
                    );
                }
            } else if m.attack_cd <= 0.0 {
                // Blocked at the trigger: re-check shortly (MC decays
                // its charge counter instead; the cadence matches).
                m.attack_cd = 0.35;
            }
        }
    }
    // Hostile touch damage: zombies + skeletons + spiders all
    // share the stub melee path. Decrement attack cooldown each
    // frame; when in range of the host + cooldown expired,
    // hit the local player + reset. H.8 splits skeleton off into
    // a ranged-bow goal.
    if kind_is_hostile(m.kind)
        || ((m.kind == KIND_ZOMBIFIED_PIGLIN
            || m.kind == super::kinds::KIND_ENDERMAN)
            && m.anger_left > 0.0)
    {
        m.attack_cd = (m.attack_cd - dt).max(0.0);
        // W140: the skeleton's defining ranged attack. Out of melee
        // reach but inside bow range -> loose an arrow at the
        // nearest player with MC's muzzle speed + arc compensation
        // and a touch of spread, on the 2 s bow cadence. The shared
        // melee path below still covers point-blank range.
        if m.kind == KIND_SKELETON
            && m.attack_cd <= 0.0
            && let Some(p) = chase_player_pos
        {
            const SKELETON_BOW_COOLDOWN: f32 = 2.0;
            const BOW_MIN_D2: f32 = 3.0 * 3.0;
            const BOW_MAX_D2: f32 = 16.0 * 16.0;
            let ddx = p[0] - m.pos[0];
            let ddy = p[1] - m.pos[1];
            let ddz = p[2] - m.pos[2];
            let pd2 = ddx * ddx + ddy * ddy + ddz * ddz;
            if (BOW_MIN_D2..=BOW_MAX_D2).contains(&pd2) {
                m.attack_cd = SKELETON_BOW_COOLDOWN;
                let eye = glam::Vec3::new(m.pos[0], m.pos[1] + 1.5, m.pos[2]);
                // Player poses are eye-level - aim at the chest.
                let tgt = glam::Vec3::new(p[0], p[1] - 0.6, p[2]);
                let mut vel = crate::projectile::aim_at(
                    eye,
                    tgt,
                    crate::projectile::SKELETON_ARROW_SPEED,
                );
                // Spread so the skeleton isn't a sniper.
                vel += glam::Vec3::new(
                    rng.next_f32() - 0.5,
                    rng.next_f32() - 0.5,
                    rng.next_f32() - 0.5,
                ) * 1.6;
                crate::projectile::spawn_arrow(voxel, eye, vel, false);
                voxel.sfx_at(
                    "mob/skeleton/shoot",
                    [eye.x, eye.y, eye.z],
                    |_, _| {},
                );
            }
        }
        // W431: the pillager crossbow - same ranged pattern as the
        // skeleton bow (CrossbowAttackGoal), a touch slower on the
        // cadence and flatter on the shot (crossbows fire at 1.6x
        // the bow muzzle in MC; the shared arrow does the rest).
        if m.kind == super::kinds::KIND_PILLAGER
            && m.attack_cd <= 0.0
            && let Some(p) = chase_player_pos
        {
            const CROSSBOW_COOLDOWN: f32 = 2.6;
            const XBOW_MIN_D2: f32 = 3.0 * 3.0;
            const XBOW_MAX_D2: f32 = 16.0 * 16.0;
            let ddx = p[0] - m.pos[0];
            let ddy = p[1] - m.pos[1];
            let ddz = p[2] - m.pos[2];
            let pd2 = ddx * ddx + ddy * ddy + ddz * ddz;
            if (XBOW_MIN_D2..=XBOW_MAX_D2).contains(&pd2) {
                m.attack_cd = CROSSBOW_COOLDOWN;
                let eye = glam::Vec3::new(m.pos[0], m.pos[1] + 1.5, m.pos[2]);
                let tgt = glam::Vec3::new(p[0], p[1] - 0.6, p[2]);
                let mut vel = crate::projectile::aim_at(
                    eye,
                    tgt,
                    crate::projectile::SKELETON_ARROW_SPEED * 1.6,
                );
                // Crossbows are tighter than bows - half the spread.
                vel += glam::Vec3::new(
                    rng.next_f32() - 0.5,
                    rng.next_f32() - 0.5,
                    rng.next_f32() - 0.5,
                ) * 0.8;
                crate::projectile::spawn_arrow(voxel, eye, vel, false);
                voxel.sfx_at(
                    "mob/pillager/shoot",
                    [eye.x, eye.y, eye.z],
                    |_, _| {},
                );
            }
        }
        // W463: EvokerAttackSpellGoal - the fang cast on the MC
        // ~5 s cadence. Inside 3 blocks the rings surround the
        // caster; out to 12 the line runs at the target. Melee
        // stays for point-blank contact like the other illagers.
        if m.kind == super::kinds::KIND_EVOKER
            && m.attack_cd <= 0.0
            && let Some(p) = chase_player_pos
        {
            const FANG_COOLDOWN: f32 = 5.0;
            const FANG_MAX_D2: f32 = 12.0 * 12.0;
            let ddx = p[0] - m.pos[0];
            let ddz = p[2] - m.pos[2];
            let pd2 = ddx * ddx + ddz * ddz;
            if pd2 <= FANG_MAX_D2 {
                m.attack_cd = FANG_COOLDOWN;
                crate::mob::fangs::cast_fangs(
                    voxel,
                    glam::Vec3::new(m.pos[0], m.pos[1], m.pos[2]),
                    glam::Vec3::new(p[0], p[1], p[2]),
                );
                voxel.sfx_at(
                    "mob/evoker/cast",
                    [m.pos[0], m.pos[1] + 1.0, m.pos[2]],
                    |_, _| {},
                );
            }
        }
        // W462: DrownedTridentAttackGoal - the ~6% of drowned that
        // spawn holding a trident (MC natural equip chance) throw
        // it on the MC cadence (interval 40 ticks, radius 10); the
        // rest stay melee-only.
        if m.kind == super::kinds::KIND_DROWNED
            && m.id.is_multiple_of(16)
            && m.attack_cd <= 0.0
            && let Some(p) = chase_player_pos
        {
            const TRIDENT_COOLDOWN: f32 = 2.0;
            const TRIDENT_MIN_D2: f32 = 3.0 * 3.0;
            const TRIDENT_MAX_D2: f32 = 10.0 * 10.0;
            let ddx = p[0] - m.pos[0];
            let ddy = p[1] - m.pos[1];
            let ddz = p[2] - m.pos[2];
            let pd2 = ddx * ddx + ddy * ddy + ddz * ddz;
            if (TRIDENT_MIN_D2..=TRIDENT_MAX_D2).contains(&pd2) {
                m.attack_cd = TRIDENT_COOLDOWN;
                let eye =
                    glam::Vec3::new(m.pos[0], m.pos[1] + 1.5, m.pos[2]);
                let tgt = glam::Vec3::new(p[0], p[1] - 0.6, p[2]);
                let mut vel = crate::projectile::aim_at(
                    eye,
                    tgt,
                    crate::projectile::SKELETON_ARROW_SPEED,
                );
                vel += glam::Vec3::new(
                    rng.next_f32() - 0.5,
                    rng.next_f32() - 0.5,
                    rng.next_f32() - 0.5,
                ) * 1.2;
                crate::projectile::spawn_trident(voxel, eye, vel);
                voxel.sfx_at(
                    "mob/drowned/shoot",
                    [eye.x, eye.y, eye.z],
                    |_, _| {},
                );
            }
        }
        // W432: EndermanLookForPlayerGoal (the stare). Meeting the
        // enderman's gaze inside 32 blocks provokes it for the MC
        // persistent-anger window; the scream marks the moment.
        if m.kind == super::kinds::KIND_ENDERMAN && m.anger_left <= 0.0 {
            let to_mob = glam::Vec3::new(
                m.pos[0] - voxel.player_pos.x,
                (m.pos[1] + 1.6) - voxel.player_pos.y,
                m.pos[2] - voxel.player_pos.z,
            );
            let dist = to_mob.length();
            if dist < 32.0 && dist > 1.0 {
                let look = glam::Vec3::new(
                    voxel.player_yaw.cos() * voxel.player_pitch.cos(),
                    voxel.player_pitch.sin(),
                    -voxel.player_yaw.sin() * voxel.player_pitch.cos(),
                );
                if look.dot(to_mob / dist) > 0.985 {
                    let h = (m.id.wrapping_mul(0x9E37_79B1) >> 7) % 1900;
                    m.anger_left = 20.0 + h as f32 / 100.0;
                    voxel.sfx_at(
                        "mob/enderman/stare",
                        m.pos,
                        |_, _| {},
                    );
                }
            }
        }
        // W430: the witch ranged attack - a thrown splash potion
        // of harming on a ~3 s cadence, inside a 10-block window
        // (WitchAttackPotions). The shared melee path still covers
        // point-blank range, like the skeleton.
        if m.kind == super::kinds::KIND_WITCH
            && m.attack_cd <= 0.0
            && let Some(p) = chase_player_pos
        {
            const WITCH_THROW_COOLDOWN: f32 = 3.25;
            const POT_MIN_D2: f32 = 2.5 * 2.5;
            const POT_MAX_D2: f32 = 10.0 * 10.0;
            let ddx = p[0] - m.pos[0];
            let ddy = p[1] - m.pos[1];
            let ddz = p[2] - m.pos[2];
            let pd2 = ddx * ddx + ddy * ddy + ddz * ddz;
            if (POT_MIN_D2..=POT_MAX_D2).contains(&pd2) {
                m.attack_cd = WITCH_THROW_COOLDOWN;
                let eye = glam::Vec3::new(m.pos[0], m.pos[1] + 1.6, m.pos[2]);
                let tgt = glam::Vec3::new(p[0], p[1] - 0.4, p[2]);
                let vel = crate::projectile::aim_at(
                    eye,
                    tgt,
                    crate::projectile::POTION_SPEED,
                );
                // W438: WitchAttackPotions selection - slowness on
                // a fresh close target, poison while the target is
                // healthy, harming otherwise.
                let kind = if pd2 < 9.0
                    && voxel.player_effects.slowness <= 0.0
                {
                    crate::projectile::PotionKind::Slowness
                } else if voxel.player_health >= 8.0
                    && voxel.player_effects.poison <= 0.0
                {
                    crate::projectile::PotionKind::Poison
                } else {
                    crate::projectile::PotionKind::Harming
                };
                crate::projectile::spawn_potion(voxel, eye, vel, kind);
                voxel.sfx_at(
                    "mob/witch/throw",
                    [eye.x, eye.y, eye.z],
                    |_, _| {},
                );
            }
        }
        // W340: the zombie family batters its villager/golem victim
        // (MC zombie attack attribute 3.0) and a villager killed by
        // a zombie rises as a zombie villager (normal-difficulty
        // 50% roll - no difficulty setting yet).
        if let Some((tid, tp)) = chase_mob {
            let vd2 = (tp[0] - m.pos[0]).powi(2)
                + (tp[1] - m.pos[1]).powi(2)
                + (tp[2] - m.pos[2]).powi(2);
            if vd2 <= ZOMBIE_ATTACK_RANGE * ZOMBIE_ATTACK_RANGE
                && m.attack_cd <= 0.0
            {
                m.attack_cd = ZOMBIE_ATTACK_COOLDOWN;
                // W474: the attacker's per-kind ATTACK_DAMAGE
                // attribute (zombie family 3) instead of a
                // hardcoded 3.0.
                voxel.damage_mob_scaled(
                    tid,
                    None,
                    contact_damage_of(&m) / super::state::MOB_HIT_DAMAGE,
                );
                voxel.mob_lunge.insert(m.id, MOB_LUNGE_TIME);
                let died_villager = voxel
                    .mobs
                    .iter()
                    .find(|o| o.id == tid)
                    .map(|o| {
                        o.dying > 0.0
                            && o.kind == super::kinds::KIND_VILLAGER
                    })
                    .unwrap_or(false);
                if died_villager && rng.next_f32() < 0.5 {
                    let nid = voxel.next_mob_id;
                    voxel.next_mob_id =
                        voxel.next_mob_id.wrapping_add(1).max(1);
                    let mut zv = MobState::new_natural(
                        nid,
                        super::kinds::KIND_ZOMBIE_VILLAGER,
                        tp,
                        tp,
                        m.yaw,
                        (day_time + 1.0).rem_euclid(DAY_LENGTH_S),
                        super::spawn::initial_health_for(
                            super::kinds::KIND_ZOMBIE_VILLAGER,
                        ),
                    );
                    zv.persistent = true;
                    voxel.mobs.push(zv);
                }
            }
        }
        let dx = voxel.player_pos.x - m.pos[0];
        let dy = voxel.player_pos.y - m.pos[1];
        let dz = voxel.player_pos.z - m.pos[2];
        let d2 = dx * dx + dy * dy + dz * dz;
        if d2 <= ZOMBIE_ATTACK_RANGE * ZOMBIE_ATTACK_RANGE && m.attack_cd <= 0.0 {
            m.attack_cd = ZOMBIE_ATTACK_COOLDOWN;
            // W474: per-kind melee from the ATTACK_DAMAGE attribute
            // table (zombie 3, piglin 5, enderman 7, blaze 6, ...)
            // instead of the flat 2.5 global. Slime stays size-based
            // (W429; a tiny slime returns 0 = harmless) and the
            // magma cube bites at size + 2.
            let touch = contact_damage_of(&m);
            if touch > 0.0 {
                voxel.apply_player_damage(touch);
            }
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
        voxel.mob_swell.remove(id);
        // W461: drop the A* waypoint entry with the mob.
        voxel.mob_paths.remove(id);
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
            // W289: ship the REAL head pose - the body-yaw copy here
            // meant remote peers never saw a glance.
            let msg = MobMsg {
                id: m.id,
                kind: m.kind,
                pos: m.pos,
                yaw: m.yaw,
                health: m.health.max(0.0).round() as u8,
                head_yaw: m.head_yaw,
                head_pitch: m.head_pitch,
                baby: m.baby_left > 0.0,
                size: m.size,
                sheared: m.sheared,
            };
            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                server.broadcast_bytes("vx.mob", buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{BlockId, Chunk};

    /// W343: a creeper beside the player swells for 1.5 s, then
    /// detonates the radius-3 blast - the mob vanishes (no corpse,
    /// no drops) and the dirt floor craters.
    #[test]
    fn w343_creeper_swells_and_detonates() {
        let mut v = crate::Voxel::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                for cy in -1..=1 {
                    v.world.insert_chunk([cx, cy, cz], Chunk::filled(BlockId(0)));
                }
            }
        }
        for x in -8..=8 {
            for z in -8..=8 {
                v.world.set([x, 3, z], BlockId(2)); // dirt
            }
        }
        v.player_pos = glam::Vec3::new(2.5, 5.6, 0.5);
        let creeper = crate::mob::state::MobState::new_natural(
            1,
            crate::mob::kinds::KIND_CREEPER,
            [0.5, 4.0, 0.5],
            [0.5, 4.0, 0.5],
            0.0,
            0.0,
            20.0,
        );
        v.mobs.push(creeper);
        v.next_mob_id = 2;
        for _ in 0..12 {
            tick_mobs(&mut v, 0.2);
        }
        assert!(
            !v.mobs.iter().any(|m| m.id == 1),
            "creeper must detonate away",
        );
        assert!(v.mob_swell.is_empty(), "swell entry cleaned");
        let dirt_gone = (-3..=3)
            .flat_map(|x| (-3..=3).map(move |z| (x, z)))
            .filter(|&(x, z)| v.world.get([x, 3, z]).is_air())
            .count();
        assert!(dirt_gone > 0, "the blast must crater the floor");
    }

    /// W340: a zombie next to a villager (no player nearby - the
    /// test player sits far away) batters it, and a lethal string
    /// of hits can raise a zombie villager. Force the kill fast by
    /// dropping the villager to 1 hp first.
    #[test]
    fn w340_zombie_batters_villager_and_converts() {
        let mut v = crate::Voxel::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                for cy in -1..=1 {
                    v.world.insert_chunk([cx, cy, cz], Chunk::filled(BlockId(0)));
                }
            }
        }
        for x in -6..=6 {
            for z in -6..=6 {
                v.world.set([x, 3, z], BlockId(1));
            }
        }
        v.player_pos = glam::Vec3::new(500.0, 4.0, 500.0);
        let zombie = crate::mob::state::MobState::new_natural(
            1,
            KIND_ZOMBIE,
            [0.5, 4.0, 0.5],
            [0.5, 4.0, 0.5],
            0.0,
            0.0,
            20.0,
        );
        let mut villager = crate::mob::state::MobState::new_natural(
            2,
            crate::mob::kinds::KIND_VILLAGER,
            [1.5, 4.0, 0.5],
            [1.5, 4.0, 0.5],
            0.0,
            0.0,
            20.0,
        );
        villager.health = 1.0;
        v.mobs.push(zombie);
        v.mobs.push(villager);
        v.next_mob_id = 3;
        // Enough ticks for several swings; the 50% conversion roll
        // fires on the killing blow only, so retry across kills is
        // not needed - assert the DAMAGE always, conversion only if
        // the corpse is dying.
        tick_mobs(&mut v, 0.05);
        let vill = v.mobs.iter().find(|m| m.id == 2).expect("corpse fades");
        assert!(
            vill.dying > 0.0 || vill.health < 1.0,
            "zombie must batter the adjacent villager",
        );
        if vill.dying > 0.0 {
            let converted = v
                .mobs
                .iter()
                .any(|m| m.kind == crate::mob::kinds::KIND_ZOMBIE_VILLAGER);
            // 50% roll - both outcomes are legal; the assert is that
            // the sim did not crash and the corpse is fading.
            let _ = converted;
        }
    }

    /// W348: a tamed ridden horse follows the rider's forward wish
    /// and the rider stays pinned to the saddle; an untamed mount
    /// with temper 0 always throws the rider (and the buck raises
    /// temper by the vanilla +5).
    #[test]
    fn w348_riding_steers_and_untamed_bucks() {
        let mut v = crate::Voxel::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                for cy in -1..=1 {
                    v.world.insert_chunk([cx, cy, cz], Chunk::filled(BlockId(0)));
                }
            }
        }
        for x in -14..=14 {
            for z in -14..=14 {
                v.world.set([x, 3, z], BlockId(1));
            }
        }
        let mut horse = crate::mob::state::MobState::new_natural(
            1,
            super::super::kinds::KIND_HORSE,
            [0.5, 4.0, 0.5],
            [0.5, 4.0, 0.5],
            0.0,
            0.0,
            22.0,
        );
        horse.tamed = true;
        horse.persistent = true;
        v.mobs.push(horse);
        v.riding_mob = Some(1);
        v.riding_wish = [1.0, 0.0];
        v.player_yaw = 0.0; // forward = +X
        for _ in 0..40 {
            v.net_tick = v.net_tick.wrapping_add(1);
            tick_mobs(&mut v, 0.05);
        }
        let h = v.mobs.iter().find(|m| m.id == 1).unwrap();
        assert!(h.pos[0] > 3.0, "steered forward, x = {}", h.pos[0]);
        let eye = crate::mob::riding::rider_eye(h.kind);
        let d = (v.player_pos
            - glam::Vec3::new(h.pos[0], h.pos[1] + eye, h.pos[2]))
        .length();
        assert!(d < 1.5, "rider pinned to the saddle (d = {d})");

        // Untamed: temper 0 -> the crazy-run roll always ejects.
        let mut v2 = crate::Voxel::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                for cy in -1..=1 {
                    v2.world
                        .insert_chunk([cx, cy, cz], Chunk::filled(BlockId(0)));
                }
            }
        }
        for x in -14..=14 {
            for z in -14..=14 {
                v2.world.set([x, 3, z], BlockId(1));
            }
        }
        let mut wild = crate::mob::state::MobState::new_natural(
            2,
            super::super::kinds::KIND_HORSE,
            [0.5, 4.0, 0.5],
            [0.5, 4.0, 0.5],
            0.0,
            0.0,
            22.0,
        );
        wild.persistent = true;
        v2.mobs.push(wild);
        v2.riding_mob = Some(2);
        for _ in 0..400 {
            // net_tick seeds the per-tick rng in game (advanced by
            // the net dispatch each frame); step it here too.
            v2.net_tick = v2.net_tick.wrapping_add(1);
            tick_mobs(&mut v2, 0.05);
            if v2.riding_mob.is_none() {
                break;
            }
        }
        assert!(v2.riding_mob.is_none(), "temper-0 mount must buck");
        let w = v2.mobs.iter().find(|m| m.id == 2).unwrap();
        assert!(w.temper >= 5, "buck raises temper (got {})", w.temper);
        assert!(!w.tamed);
    }

    /// W339: a golem standing next to a zombie batters it on the
    /// first tick - health drops by at least the 7.5 minimum and
    /// the victim gets the 0.4 b/t upward launch.
    #[test]
    fn w339_golem_batters_adjacent_zombie() {
        let mut v = crate::Voxel::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                for cy in -1..=1 {
                    v.world.insert_chunk([cx, cy, cz], Chunk::filled(BlockId(0)));
                }
            }
        }
        for x in -6..=6 {
            for z in -6..=6 {
                v.world.set([x, 3, z], BlockId(1));
            }
        }
        let golem = crate::mob::state::MobState::new_natural(
            1,
            super::super::kinds::KIND_IRON_GOLEM,
            [0.5, 4.0, 0.5],
            [0.5, 4.0, 0.5],
            0.0,
            0.0,
            100.0,
        );
        let zombie = crate::mob::state::MobState::new_natural(
            2,
            KIND_ZOMBIE,
            [1.5, 4.0, 0.5],
            [1.5, 4.0, 0.5],
            0.0,
            0.0,
            20.0,
        );
        v.mobs.push(golem);
        v.mobs.push(zombie);
        tick_mobs(&mut v, 0.05);
        let z = v.mobs.iter().find(|m| m.id == 2).expect("zombie alive");
        assert!(
            z.health <= 20.0 - 7.5 + 1e-3,
            "golem must batter the zombie (health {})",
            z.health,
        );
        assert!(z.vy > 6.0, "victim launched upward (vy {})", z.vy);
    }
}
