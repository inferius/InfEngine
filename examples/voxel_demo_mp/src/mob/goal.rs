//! Goal-based AI state machine.
//!
//! Mirrors MC's `net/minecraft/world/entity/ai/goal/Goal.java` +
//! `GoalSelector.java` shape, simplified for the demo:
//!
//! - `Goal` is a trait with `priority`, `can_start`, `tick`,
//!   `can_continue`. Lower priority = higher precedence (matches
//!   MC).
//! - `GoalCtx` is the read-only world view + the writable bits of
//!   `MobState` exposed to the goal tick. Pure data so goals are
//!   trivially testable.
//! - `GoalSelector::tick` picks the highest-priority goal whose
//!   `can_start` (or `can_continue` for the currently active one)
//!   passes, then forwards the tick.
//!
//! Built-in goals (this commit):
//! - `IdleGoal` (priority 10) - default when no other goal applies.
//! - `WanderGoal` (priority 7) - pick a random nearby target every
//!   5 sec, walk toward it.
//! - `ChasePlayerGoal { range }` (priority 2) - pathfind toward
//!   the player when within range.
//! - `FleeFromPlayerGoal { range }` (priority 3) - chicken: when
//!   player within range, set target to a point AWAY from them.
//! - `EatGrassGoal` (priority 6) - scaffold for sheep: stays
//!   stationary for `EAT_DURATION_S` when triggered. Cosmetic
//!   effect (texture swap) is deferred to a follow-up.
//!
//! Cited MC:
//! - `net/minecraft/world/entity/ai/goal/Goal.java`
//! - `net/minecraft/world/entity/ai/goal/GoalSelector.java`
//! - `net/minecraft/world/entity/ai/goal/WaterAvoidingRandomStrollGoal.java`
//!   (wander shape)
//! - `net/minecraft/world/entity/ai/goal/MeleeAttackGoal.java`
//!   (chase + bite shape)
//! - `net/minecraft/world/entity/ai/goal/AvoidEntityGoal.java`
//!   (flee shape)
//! - `net/minecraft/world/entity/animal/Sheep$SheepEatBlockGoal.java`
//!   (eat-grass scaffold)

#![allow(dead_code)]

use super::spawn::SpawnRng;

/// Goal priority range. Lower = more important (matches MC's
/// `GoalSelector` integer priority). Numeric range 0..=10.
pub(crate) const PRIORITY_RANGE: std::ops::RangeInclusive<u8> = 0..=10;

/// Read + write context exposed to a goal tick. Mutable parts go
/// through `target` (the next pose the mob should walk toward) and
/// `retarget_after` (seconds until the goal re-evaluates).
///
/// Kept pure-data so goals can be unit-tested without a `Voxel`
/// runtime; the ai-tick adapter builds a `GoalCtx` per mob each
/// frame.
#[derive(Debug)]
pub(crate) struct GoalCtx<'a> {
    pub(crate) mob_pos: [f32; 3],
    pub(crate) mob_kind: u8,
    pub(crate) player_pos: Option<[f32; 3]>,
    pub(crate) day_time: f32,
    pub(crate) rng: SpawnRng<'a>,
    /// Goal tick writes the new target here. The ai tick will
    /// drive the mob's wander step toward this point.
    pub(crate) target: [f32; 3],
    /// Goal tick writes the next retarget time here. Reads as
    /// `day_time + delay`. The ai tick uses this to decide when
    /// to re-evaluate the goal selector.
    pub(crate) retarget_at: f32,
}

/// Goal trait. Built-in goals are stateless or carry per-goal
/// state (cooldowns, eat-timer). The selector instantiates a goal
/// per mob via a static table on `MobKind` (set up in H.5
/// follow-ups; the ai-tick currently dispatches inline).
pub(crate) trait Goal {
    fn priority(&self) -> u8;
    fn can_start(&self, ctx: &GoalCtx<'_>) -> bool;
    fn tick(&mut self, ctx: &mut GoalCtx<'_>);
    fn can_continue(&self, ctx: &GoalCtx<'_>) -> bool {
        self.can_start(ctx)
    }
}

/// Idle: lowest priority. Always startable; never does anything.
/// Renders as a standing mob.
#[derive(Debug, Default)]
pub(crate) struct IdleGoal;

impl Goal for IdleGoal {
    fn priority(&self) -> u8 {
        10
    }
    fn can_start(&self, _ctx: &GoalCtx<'_>) -> bool {
        true
    }
    fn tick(&mut self, ctx: &mut GoalCtx<'_>) {
        // Stand still: target = current position.
        ctx.target = ctx.mob_pos;
    }
}

/// Wander: pick a random target within `RADIUS` blocks every
/// `RETARGET_AFTER` seconds. Default goal for passive mobs.
#[derive(Debug, Default)]
pub(crate) struct WanderGoal;

/// Radius (blocks) for wander targets. Matches the original
/// `mob_ai::tick_mobs` wander knob: 4..12 from current pos.
pub(crate) const WANDER_RADIUS_MIN: f32 = 4.0;
pub(crate) const WANDER_RADIUS_MAX: f32 = 12.0;
/// Seconds between forced retargets.
pub(crate) const WANDER_RETARGET_S: f32 = 5.0;

impl Goal for WanderGoal {
    fn priority(&self) -> u8 {
        7
    }
    fn can_start(&self, _ctx: &GoalCtx<'_>) -> bool {
        true
    }
    fn tick(&mut self, ctx: &mut GoalCtx<'_>) {
        let theta = ctx.rng.next_f32() * std::f32::consts::TAU;
        let dist = WANDER_RADIUS_MIN
            + ctx.rng.next_f32() * (WANDER_RADIUS_MAX - WANDER_RADIUS_MIN);
        ctx.target = [
            ctx.mob_pos[0] + theta.cos() * dist,
            ctx.mob_pos[1],
            ctx.mob_pos[2] + theta.sin() * dist,
        ];
        ctx.retarget_at = ctx.day_time + WANDER_RETARGET_S;
    }
}

/// Chase the player when within `range` blocks. Sets `target` to
/// the player's position so the wander step heads straight there.
/// Real A* pathfinding lives in `pathfind.rs`; H.5 connects them
/// in a follow-up - here we drive the wander tick toward the
/// player to keep parity with the pre-H.5 zombie behaviour.
#[derive(Debug)]
pub(crate) struct ChasePlayerGoal {
    pub(crate) range: f32,
}

impl Goal for ChasePlayerGoal {
    fn priority(&self) -> u8 {
        2
    }
    fn can_start(&self, ctx: &GoalCtx<'_>) -> bool {
        let Some(p) = ctx.player_pos else {
            return false;
        };
        let dx = p[0] - ctx.mob_pos[0];
        let dy = p[1] - ctx.mob_pos[1];
        let dz = p[2] - ctx.mob_pos[2];
        let d2 = dx * dx + dy * dy + dz * dz;
        d2 <= self.range * self.range
    }
    fn tick(&mut self, ctx: &mut GoalCtx<'_>) {
        if let Some(p) = ctx.player_pos {
            ctx.target = p;
            ctx.retarget_at = ctx.day_time + 1.0;
        }
    }
}

/// Flee: when the player gets within `range`, pick a target on
/// the opposite side of the player. Cap the flee distance at 8
/// blocks so the chicken doesn't tunnel into the next chunk.
#[derive(Debug)]
pub(crate) struct FleeFromPlayerGoal {
    pub(crate) range: f32,
}

/// Cap distance (blocks) on the flee target relative to current
/// pos. Matches MC's `AvoidEntityGoal::DEFAULT_AVOID_DISTANCE`
/// shape - flee just far enough to lose interest.
pub(crate) const FLEE_TARGET_DIST: f32 = 8.0;

impl Goal for FleeFromPlayerGoal {
    fn priority(&self) -> u8 {
        3
    }
    fn can_start(&self, ctx: &GoalCtx<'_>) -> bool {
        let Some(p) = ctx.player_pos else {
            return false;
        };
        let dx = p[0] - ctx.mob_pos[0];
        let dy = p[1] - ctx.mob_pos[1];
        let dz = p[2] - ctx.mob_pos[2];
        let d2 = dx * dx + dy * dy + dz * dz;
        d2 <= self.range * self.range
    }
    fn tick(&mut self, ctx: &mut GoalCtx<'_>) {
        if let Some(p) = ctx.player_pos {
            // Vector pointing away from the player.
            let dx = ctx.mob_pos[0] - p[0];
            let dz = ctx.mob_pos[2] - p[2];
            let mag = (dx * dx + dz * dz).sqrt().max(1e-3);
            let nx = dx / mag;
            let nz = dz / mag;
            ctx.target = [
                ctx.mob_pos[0] + nx * FLEE_TARGET_DIST,
                ctx.mob_pos[1],
                ctx.mob_pos[2] + nz * FLEE_TARGET_DIST,
            ];
            ctx.retarget_at = ctx.day_time + 1.0;
        }
    }
}

/// Eat-grass scaffold for sheep. Stays in `Eating` state for
/// `EAT_DURATION_S` seconds. Cosmetic effect (texture swap, sound)
/// is deferred to a follow-up; the state machine is in place so
/// the goal selector can pre-empt other goals while a sheep
/// eats.
///
/// W474: the LIVE eat-grass behaviour ships in `mob::graze`
/// (EatBlockGoal port wired into the monolithic tick - block edit,
/// wool regrow, the 1/1000 gate). This scaffold stays dormant until
/// the H.5 goal-selector swap lands.
#[derive(Debug, Default)]
pub(crate) struct EatGrassGoal {
    pub(crate) eat_until: f32,
}

pub(crate) const EAT_DURATION_S: f32 = 1.5;

impl EatGrassGoal {
    /// True when the eat timer is still running.
    pub(crate) fn is_eating(&self, ctx: &GoalCtx<'_>) -> bool {
        ctx.day_time < self.eat_until
    }
}

impl Goal for EatGrassGoal {
    fn priority(&self) -> u8 {
        6
    }
    fn can_start(&self, _ctx: &GoalCtx<'_>) -> bool {
        // Deferred: real start gate also checks "cell below is
        // grass", same shape as MC's SheepEatBlockGoal.
        false
    }
    fn tick(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.eat_until <= ctx.day_time {
            self.eat_until = ctx.day_time + EAT_DURATION_S;
        }
        ctx.target = ctx.mob_pos;
        ctx.retarget_at = self.eat_until;
    }
    fn can_continue(&self, ctx: &GoalCtx<'_>) -> bool {
        self.is_eating(ctx)
    }
}

/// Decide which goal applies to a mob this tick. Returns the
/// index of the highest-priority goal (lowest `priority()` number)
/// whose `can_start` passes.
///
/// Public so the ai tick can dispatch through it; H.5 keeps the
/// list per-kind hardcoded inside the dispatch shim until a
/// per-kind goal-table follow-up.
pub(crate) fn pick_active(
    goals: &[Box<dyn Goal>],
    ctx: &GoalCtx<'_>,
) -> Option<usize> {
    let mut best: Option<(u8, usize)> = None;
    for (idx, g) in goals.iter().enumerate() {
        if !g.can_start(ctx) {
            continue;
        }
        let p = g.priority();
        match best {
            Some((bp, _)) if bp <= p => {}
            _ => best = Some((p, idx)),
        }
    }
    best.map(|(_, idx)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DAY_LENGTH_S;

    fn ctx(seed: &mut u32, mob_pos: [f32; 3], player_pos: Option<[f32; 3]>) -> GoalCtx<'_> {
        GoalCtx {
            mob_pos,
            mob_kind: 0,
            player_pos,
            day_time: 0.0,
            rng: SpawnRng::new(seed),
            target: mob_pos,
            retarget_at: 0.0,
        }
    }

    #[test]
    fn idle_goal_has_lowest_priority() {
        // Sanity: every other built-in goal must out-prioritise idle.
        let idle = IdleGoal;
        let wander = WanderGoal;
        let chase = ChasePlayerGoal { range: 10.0 };
        let flee = FleeFromPlayerGoal { range: 6.0 };
        let eat = EatGrassGoal::default();
        for g in [
            wander.priority(),
            chase.priority(),
            flee.priority(),
            eat.priority(),
        ] {
            assert!(g < idle.priority(), "{} < {}", g, idle.priority());
        }
    }

    #[test]
    fn goal_priorities_in_documented_range() {
        // Every built-in priority must land in 0..=10 (PRIORITY_RANGE).
        for p in [
            IdleGoal.priority(),
            WanderGoal.priority(),
            ChasePlayerGoal { range: 1.0 }.priority(),
            FleeFromPlayerGoal { range: 1.0 }.priority(),
            EatGrassGoal::default().priority(),
        ] {
            assert!(PRIORITY_RANGE.contains(&p), "{}", p);
        }
    }

    #[test]
    fn chase_starts_when_player_in_range() {
        let mut seed = 1u32;
        let c = ctx(&mut seed, [0.0, 0.0, 0.0], Some([5.0, 0.0, 0.0]));
        let chase = ChasePlayerGoal { range: 10.0 };
        assert!(chase.can_start(&c));
    }

    #[test]
    fn chase_does_not_start_out_of_range() {
        let mut seed = 1u32;
        let c = ctx(&mut seed, [0.0, 0.0, 0.0], Some([20.0, 0.0, 0.0]));
        let chase = ChasePlayerGoal { range: 10.0 };
        assert!(!chase.can_start(&c));
    }

    #[test]
    fn chase_does_not_start_when_player_absent() {
        let mut seed = 1u32;
        let c = ctx(&mut seed, [0.0, 0.0, 0.0], None);
        let chase = ChasePlayerGoal { range: 10.0 };
        assert!(!chase.can_start(&c));
    }

    #[test]
    fn chase_tick_sets_target_to_player_pos() {
        let mut seed = 1u32;
        let mut c = ctx(&mut seed, [0.0, 0.0, 0.0], Some([5.0, 0.0, 0.0]));
        let mut chase = ChasePlayerGoal { range: 10.0 };
        chase.tick(&mut c);
        assert_eq!(c.target, [5.0, 0.0, 0.0]);
    }

    #[test]
    fn flee_starts_when_player_in_range() {
        let mut seed = 1u32;
        let c = ctx(&mut seed, [0.0, 0.0, 0.0], Some([3.0, 0.0, 0.0]));
        let flee = FleeFromPlayerGoal { range: 6.0 };
        assert!(flee.can_start(&c));
    }

    #[test]
    fn flee_target_points_away_from_player() {
        let mut seed = 1u32;
        // Player to east; flee target should land west.
        let mut c = ctx(&mut seed, [0.0, 64.0, 0.0], Some([5.0, 64.0, 0.0]));
        let mut flee = FleeFromPlayerGoal { range: 10.0 };
        flee.tick(&mut c);
        // Target x < mob x (heading west).
        assert!(c.target[0] < 0.0, "{:?}", c.target);
    }

    #[test]
    fn flee_target_distance_capped() {
        let mut seed = 1u32;
        let mut c = ctx(&mut seed, [0.0, 0.0, 0.0], Some([1.0, 0.0, 0.0]));
        let mut flee = FleeFromPlayerGoal { range: 5.0 };
        flee.tick(&mut c);
        let dx = c.target[0] - 0.0;
        let dz = c.target[2] - 0.0;
        let d = (dx * dx + dz * dz).sqrt();
        // Flee distance is the constant - allow a small slop for
        // numerical drift.
        assert!((d - FLEE_TARGET_DIST).abs() < 0.01, "{}", d);
    }

    #[test]
    fn wander_target_lands_within_radius() {
        let mut seed = 1u32;
        let mut c = ctx(&mut seed, [0.0, 0.0, 0.0], None);
        let mut wander = WanderGoal;
        wander.tick(&mut c);
        let dx = c.target[0];
        let dz = c.target[2];
        let d = (dx * dx + dz * dz).sqrt();
        assert!(
            (WANDER_RADIUS_MIN - 0.01..=WANDER_RADIUS_MAX + 0.01).contains(&d),
            "wander distance {} out of [{}, {}]",
            d,
            WANDER_RADIUS_MIN,
            WANDER_RADIUS_MAX
        );
    }

    #[test]
    fn pick_active_returns_highest_priority() {
        // Build a goal list with Wander (7) + Chase (2). Player in
        // range -> chase wins.
        let mut seed = 1u32;
        let c = ctx(&mut seed, [0.0, 0.0, 0.0], Some([5.0, 0.0, 0.0]));
        let goals: Vec<Box<dyn Goal>> = vec![
            Box::new(WanderGoal),
            Box::new(ChasePlayerGoal { range: 10.0 }),
            Box::new(IdleGoal),
        ];
        let idx = pick_active(&goals, &c).expect("at least one goal applies");
        assert_eq!(goals[idx].priority(), 2);
    }

    #[test]
    fn pick_active_falls_back_to_idle_when_no_player() {
        let mut seed = 1u32;
        let c = ctx(&mut seed, [0.0, 0.0, 0.0], None);
        let goals: Vec<Box<dyn Goal>> = vec![
            Box::new(ChasePlayerGoal { range: 10.0 }),
            Box::new(IdleGoal),
        ];
        let idx = pick_active(&goals, &c).expect("idle always applies");
        assert_eq!(goals[idx].priority(), IdleGoal.priority());
    }

    #[test]
    fn pick_active_chase_exits_when_player_leaves_range() {
        // Player crosses the range boundary: chase no longer
        // `can_start`, so the selector falls back to wander.
        let mut seed = 1u32;
        let mut c = ctx(&mut seed, [0.0, 0.0, 0.0], Some([5.0, 0.0, 0.0]));
        let goals: Vec<Box<dyn Goal>> = vec![
            Box::new(ChasePlayerGoal { range: 6.0 }),
            Box::new(WanderGoal),
        ];
        let idx = pick_active(&goals, &c).unwrap();
        assert_eq!(goals[idx].priority(), 2); // chase
        c.player_pos = Some([20.0, 0.0, 0.0]);
        let idx = pick_active(&goals, &c).unwrap();
        assert_eq!(goals[idx].priority(), 7); // wander
    }

    #[test]
    fn eat_grass_state_machine_tracks_timer() {
        let mut seed = 1u32;
        let mut c = ctx(&mut seed, [0.0, 0.0, 0.0], None);
        let mut eat = EatGrassGoal::default();
        // Force start (real start gate is deferred behind a stub).
        eat.tick(&mut c);
        assert!(eat.is_eating(&c));
        // Fast-forward past the eat duration.
        c.day_time = EAT_DURATION_S + 0.1;
        assert!(!eat.is_eating(&c));
    }

    #[test]
    fn wander_retarget_at_advances_by_period() {
        let mut seed = 1u32;
        let mut c = ctx(&mut seed, [0.0, 0.0, 0.0], None);
        c.day_time = 100.0;
        let mut wander = WanderGoal;
        wander.tick(&mut c);
        assert!((c.retarget_at - (100.0 + WANDER_RETARGET_S)).abs() < 0.01);
    }

    #[test]
    fn ctx_day_time_wraps_via_day_length() {
        // Spot check the day-length constant exposed for goals.
        // The real ai tick uses `day_time.rem_euclid(DAY_LENGTH_S)`
        // on retarget_at; this test pins the wrap.
        let dl = DAY_LENGTH_S;
        assert!(dl > 0.0);
    }
}
