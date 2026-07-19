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
    MOB_LUNGE_TIME, MOB_MAX_HEALTH, MobState, ZOMBIE_ATTACK_COOLDOWN,
    ZOMBIE_ATTACK_RANGE, ZOMBIE_MAX_HEALTH, ZOMBIE_SIGHT_RANGE,
    ZOMBIE_TOUCH_DAMAGE,
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
        let max = if kind_is_hostile(m.kind) {
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
    // W289: MC goal shapes instead of the old perpetual pacing.
    //  - Hostiles chase the nearest player in sight range.
    //  - PanicGoal: a hurt passive sprints (kind panic multiplier)
    //    to a far point until the timer runs out.
    //  - RandomStrollGoal: otherwise mobs IDLE, and with chance
    //    1/120 per tick pick a stroll target within a 10-block
    //    square (MC WaterAvoidingRandomStrollGoal interval).
    let chase_player_pos: Option<[f32; 3]> = if kind_is_hostile(m.kind) {
        voxel.nearest_player_pos(m.pos, ZOMBIE_SIGHT_RANGE)
    } else if m.kind == KIND_ZOMBIFIED_PIGLIN && m.anger_left > 0.0 {
        // W300: NeutralMob anger - an angered zombified piglin hunts
        // the player for the anger window (MC follow range 35).
        voxel.nearest_player_pos(m.pos, 35.0)
    } else {
        None
    };
    m.panic_left = (m.panic_left - dt).max(0.0);
    m.anger_left = (m.anger_left - dt).max(0.0);
    m.hop_cd = (m.hop_cd - dt).max(0.0);
    let mut speed = super::state::kind_walk_speed(m.kind);
    if m.panic_left > 0.0 {
        speed *= super::state::kind_panic_mult(m.kind);
    }
    if let Some(p) = chase_player_pos {
        m.target = p;
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
        } else if let Some((b, _)) = voxel.inventory[voxel.selected_slot]
            && super::breed::is_food_for(m.kind, b.0)
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
        if m.kind == super::kinds::KIND_MAGMA_CUBE {
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
                if m.kind == super::kinds::KIND_GHAST {
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
        || (m.kind == KIND_ZOMBIFIED_PIGLIN && m.anger_left > 0.0)
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
                crate::projectile::spawn_arrow(voxel, eye, vel);
                voxel.sfx_at(
                    "mob/skeleton/shoot",
                    [eye.x, eye.y, eye.z],
                    |_, _| {},
                );
            }
        }
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
            };
            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                server.broadcast_bytes("vx.mob", buf);
            }
        }
    }
}
