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
        }
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
pub(crate) const MOB_SPEED: f32 = 1.2;
pub(crate) const MOB_MAX_HEALTH: f32 = 8.0;
pub(crate) const MOB_HIT_DAMAGE: f32 = 2.0;

// ---------------------------------------------------------------
// Zombie / hostile knobs.
// ---------------------------------------------------------------

pub(crate) const ZOMBIE_SPEED: f32 = 1.8;
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
