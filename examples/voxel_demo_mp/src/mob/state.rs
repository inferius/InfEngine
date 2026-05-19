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
}

// ---------------------------------------------------------------
// Global mob balance knobs.
// ---------------------------------------------------------------

/// Length of the death-fade in seconds. Both server and clients
/// run the same countdown so the animation reads identically on
/// every peer.
pub(crate) const MOB_DEATH_FADE: f32 = 0.55;

pub(crate) const MOB_CAP_DEFAULT: usize = 12;
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
/// unloaded chunks the player will never visit again.
pub(crate) const MOB_DESPAWN_DIST_SQ: f32 = 80.0 * 80.0;
/// Seconds the zombie body offsets forward after striking.
/// Drives the lunge animation timer.
pub(crate) const MOB_LUNGE_TIME: f32 = 0.30;
/// Maximum range (in blocks) for the player's melee attack on
/// a mob. Same number as voxel break reach so the two
/// interactions feel symmetric.
pub(crate) const MOB_ATTACK_REACH: f32 = 5.5;
