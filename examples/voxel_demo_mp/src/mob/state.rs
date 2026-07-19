//! Server-side mob state struct + balance-knob constants.
//!
//! Holds the per-mob authoritative record kept in `Voxel.mobs` plus
//! the tuning constants that gate damage / speed / health / spawn
//! timings. The struct fields stay identical to the pre-split
//! `mob_ai::MobState` so external call sites (chat commands, save
//! layer, dim switch tests) keep working unchanged.
//!
//! MC mirror: `net/minecraft/world/entity/LivingEntity.java`
//! aggregates HP, AI goals, hurt timers + the entity position on
//! one base type. We split that into a thin `MobState` record (data)
//! plus free functions over `&mut Voxel` (behaviour).

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
    /// True when the mob is exempt from the despawn passes -
    /// named, leash-tagged, bred, or spawned via the /spawnmob
    /// chat command. Matches MC's `Mob.persistenceRequired` flag.
    /// Defaults to `false` for natural-spawn mobs.
    pub(crate) persistent: bool,
    /// Far-despawn timer (seconds remaining). Ticks down only
    /// while no player is within `FAR_DESPAWN_DIST`. Resets to
    /// `FAR_DESPAWN_TIMER` whenever a player comes back in
    /// range. Zero or below + persistent = false triggers the
    /// despawn pass. Mirrors MC's
    /// `Mob::checkDespawn` 30-second far-timer.
    pub(crate) far_despawn_timer: f32,
    /// W66.2: in-love timer (seconds). > 0 = fed + seeking a mate.
    /// Server-side only (not wired or saved); decays in `breed_pass`.
    pub(crate) love: f32,
    /// W66.2: seconds before this animal can be bred again. Set on a
    /// successful breed so a pair can't multiply every tick.
    pub(crate) breed_cooldown: f32,
    /// W147: absolute world yaw of the HEAD (MC `LookAtPlayerGoal`).
    /// Follows the body yaw when idle; turns toward a nearby player
    /// while `look_left` runs. Broadcast in `MobMsg` so every peer
    /// renders the same glance.
    pub(crate) head_yaw: f32,
    /// W147: head pitch, MC convention (positive = looking DOWN).
    pub(crate) head_pitch: f32,
    /// W147: seconds left in the current look-at-player glance.
    pub(crate) look_left: f32,
    /// W289: vertical velocity (blocks/s). MC LivingEntity gravity
    /// (-0.08 b/t^2) replaces the old teleport-snap to the surface,
    /// so mobs fall off ledges instead of riding terrain like an
    /// elevator. Chickens clamp their fall (wing flutter).
    pub(crate) vy: f32,
    /// W289: MC PanicGoal - seconds left sprinting away after a hit.
    pub(crate) panic_left: f32,
    /// W300: MC `NeutralMob` persistent-anger seconds left. While
    /// positive a zombified piglin chases + melees like a hostile;
    /// at zero it goes back to ambling. Set on player hit
    /// (`rangeOfSeconds(20, 39)`).
    pub(crate) anger_left: f32,
    /// W289: true while a stroll/chase target is active. MC passive
    /// mobs IDLE between strolls (RandomStrollGoal fires with chance
    /// 1/120 per tick); the old AI re-picked a target immediately on
    /// arrival, so every animal paced nonstop - the "weird behaviour".
    pub(crate) strolling: bool,
    /// W289: rabbit hop cooldown (s). Rabbits move ONLY while
    /// airborne (MC rabbits jump to move); grounded rabbits wait
    /// here between hops.
    pub(crate) hop_cd: f32,
    /// W291: seconds left growing up (MC baby age -24000 ticks =
    /// 20 min). 0 = adult. Babies render at half scale, follow the
    /// nearest adult of their kind, and can't breed or be tempted.
    pub(crate) baby_left: f32,
}

impl MobState {
    /// Build a fresh natural-spawn `MobState`. New fields land in
    /// here so existing call sites don't enumerate every field.
    /// Persistent + the despawn timer get their default values.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_natural(
        id: u32,
        kind: u8,
        pos: [f32; 3],
        target: [f32; 3],
        yaw: f32,
        retarget_at: f32,
        health: f32,
    ) -> Self {
        Self {
            id,
            kind,
            pos,
            target,
            yaw,
            retarget_at,
            health,
            dying: 0.0,
            attack_cd: 0.0,
            persistent: false,
            far_despawn_timer: FAR_DESPAWN_TIMER,
            love: 0.0,
            breed_cooldown: 0.0,
            head_yaw: yaw,
            head_pitch: 0.0,
            look_left: 0.0,
            vy: 0.0,
            panic_left: 0.0,
            anger_left: 0.0,
            strolling: false,
            hop_cd: 0.0,
            baby_left: 0.0,
        }
    }
}

/// W289: per-kind walk speed in blocks/s. MC's movement-speed
/// attribute scaled by the empirical mob-walk factor (~x10: zombie
/// attr 0.23 walks ~2.3 b/s in vanilla, pig 0.25 ~2.5). One shared
/// table instead of the old flat MOB_SPEED 1.2 for every passive.
pub(crate) fn kind_walk_speed(kind: u8) -> f32 {
    use super::kinds::*;
    match kind {
        k if k == KIND_COW => 2.0,          // attr 0.20
        k if k == KIND_SHEEP => 2.3,        // attr 0.23
        k if k == KIND_PIG => 2.5,          // attr 0.25
        k if k == KIND_CHICKEN => 2.5,      // attr 0.25
        k if k == KIND_RABBIT => 3.0,       // attr 0.30
        k if k == KIND_WOLF => 3.0,         // attr 0.30
        k if k == KIND_POLAR_BEAR => 2.5,   // attr 0.25
        k if k == KIND_ZOMBIE => 2.3,       // attr 0.23
        // W300: ZombifiedPiglin.createAttributes MOVEMENT_SPEED 0.23.
        k if k == KIND_ZOMBIFIED_PIGLIN => 2.3,
        // W302: villager attr 0.5 x the 0.4 stroll speedModifier
        // (VillagerGoalPackages) - vanilla villagers AMBLE at 0.2.
        k if k == KIND_VILLAGER => 2.0,
        // W315: Cat.createAttributes MOVEMENT_SPEED 0.3.
        k if k == KIND_CAT => 3.0,
        // W322: IronGolem.createAttributes MOVEMENT_SPEED 0.25.
        k if k == KIND_IRON_GOLEM => 2.5,
        // W322: AbstractHorse generateSpeed averages ~0.225.
        k if k == KIND_HORSE => 2.25,
        // W327: ghast drifts slowly (MC flies at ~0.06/tick bursts).
        k if k == KIND_GHAST => 1.2,
        // W343: Creeper.createAttributes MOVEMENT_SPEED 0.25.
        k if k == KIND_CREEPER => 2.5,
        // W344: donkey generateSpeed averages ~0.175.
        k if k == KIND_DONKEY => 1.75,
        // W327: magma cube ground speed attr 0.3 (slime family).
        k if k == KIND_MAGMA_CUBE => 3.0,
        // W330: zombie family attr 0.23.
        k if k == KIND_ZOMBIE_VILLAGER => 2.3,
        k if k == KIND_SKELETON => 2.5,     // attr 0.25
        k if k == KIND_SPIDER => 3.0,       // attr 0.30
        _ => 2.3,
    }
}

/// W289: MC PanicGoal speed modifier per kind (Cow 2.0, Sheep/Pig
/// 1.25, Chicken 1.4, Rabbit 2.2).
pub(crate) fn kind_panic_mult(kind: u8) -> f32 {
    use super::kinds::*;
    match kind {
        k if k == KIND_COW => 2.0,
        k if k == KIND_CHICKEN => 1.4,
        k if k == KIND_RABBIT => 2.2,
        _ => 1.25,
    }
}

// ---------------------------------------------------------------
// Global mob balance knobs.
// ---------------------------------------------------------------

/// Length of the death-fade in seconds. Both server and clients
/// run the same countdown so the animation reads identically on
/// every peer.
pub(crate) const MOB_DEATH_FADE: f32 = 0.55;

// W52: bumped from 12 to 25. MC's per-player mob density caps land
// at 70 hostile + 10 passive + 15 ambient + 5 water within a
// 128x128 column; the demo runs a single global cap, so 25 keeps
// the world feeling populated without overwhelming the renderer.
pub(crate) const MOB_CAP_DEFAULT: usize = 25;
// W289: superseded by kind_walk_speed (per-kind MC attributes).
pub(crate) const MOB_MAX_HEALTH: f32 = 8.0;
pub(crate) const MOB_HIT_DAMAGE: f32 = 2.0;

// ---------------------------------------------------------------
// Zombie / hostile knobs.
// ---------------------------------------------------------------


pub(crate) const ZOMBIE_MAX_HEALTH: f32 = 14.0;
pub(crate) const ZOMBIE_TOUCH_DAMAGE: f32 = 2.5;
pub(crate) const ZOMBIE_ATTACK_COOLDOWN: f32 = 0.9;
pub(crate) const ZOMBIE_ATTACK_RANGE: f32 = 1.4;
pub(crate) const ZOMBIE_SIGHT_RANGE: f32 = 22.0;

// ---------------------------------------------------------------
// Despawn + animation knobs.
// ---------------------------------------------------------------

/// Squared distance from the nearest player past which a mob
/// is despawned. Stops sheep + zombies from piling up in
/// unloaded chunks the player will never visit again. Wave 5
/// callers still reference this; H.6 layers a finer-grained
/// 3-tier despawn rule on top (see `despawn.rs`).
#[allow(dead_code)] // Retained as a public constant for callers that haven't moved to despawn.rs yet.
pub(crate) const MOB_DESPAWN_DIST_SQ: f32 = INSTANT_DESPAWN_DIST * INSTANT_DESPAWN_DIST;

/// Distance (blocks) past which a non-persistent mob despawns
/// immediately. Mirrors MC's
/// `Mob::removeWhenFarAway` 128-block threshold, dropped to
/// 80 for the demo's smaller render distance.
pub(crate) const INSTANT_DESPAWN_DIST: f32 = 80.0;
/// Distance (blocks) at which the far-despawn timer starts
/// ticking. Inside this range the timer is paused + reset on
/// every check. Matches MC's 32-block soft despawn band.
pub(crate) const FAR_DESPAWN_DIST: f32 = 48.0;
/// Seconds the far-despawn timer counts down before a mob is
/// evicted. Mirrors MC's 30-second timer in `Mob::checkDespawn`.
pub(crate) const FAR_DESPAWN_TIMER: f32 = 30.0;
/// Period (seconds) between despawn checks. Matches MC's
/// 20-tick (~1 second) cadence in `Mob::baseTick`; we throttle
/// to 5 seconds since the demo runs a smaller mob_cap.
pub(crate) const DESPAWN_CHECK_PERIOD: f32 = 5.0;
/// Seconds the zombie body offsets forward after striking.
/// Drives the lunge animation timer.
pub(crate) const MOB_LUNGE_TIME: f32 = 0.30;
/// Maximum range (in blocks) for the player's melee attack on
/// a mob. Same number as voxel break reach so the two
/// interactions feel symmetric.
pub(crate) const MOB_ATTACK_REACH: f32 = 5.5;
