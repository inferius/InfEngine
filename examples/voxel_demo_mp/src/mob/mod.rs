//! Mob subsystem: kind taxonomy + state record + AI tick + damage +
//! audio + spawn + pathfinding.
//!
//! Phase H of the voxel-demo polish: replaces the monolithic
//! `mob_ai.rs` (>500 LOC) with a folder split. The submodules
//! reflect the MC `net.minecraft.world.entity` package's coarse
//! shape:
//!
//! - `kinds`     - `MobKind` enum + legacy `KIND_*` aliases.
//! - `state`     - `MobState` record + balance constants.
//! - `ai`        - per-tick walk + chase + zombie touch-damage.
//! - `spawn`     - new-mob spawn loop (light-aware in H.2,
//!   biome-aware in H.3).
//! - `damage`    - HP subtract + death event + loot drop.
//! - `raycast`   - melee mob-targeting raycast.
//! - `audio`     - footstep + species "say" timers.
//! - `pathfind`  - A* on the voxel grid (filled in H.4).
//!
//! External call sites (chat commands, save layer, dim_switch tests)
//! talk through the re-exports at the bottom of this file so the
//! split is invisible to them.

pub(crate) mod ai;
pub(crate) mod audio;
pub(crate) mod breed;
pub(crate) mod damage;
pub(crate) mod despawn;
pub(crate) mod goal;
pub(crate) mod hud;
pub(crate) mod kinds;
pub(crate) mod light;
pub(crate) mod pathfind;
pub(crate) mod pool;
pub(crate) mod raycast;
pub(crate) mod spawn;
pub(crate) mod state;

// ----------------------------------------------------------------
// Re-exports.
// ----------------------------------------------------------------
//
// Every constant + function the rest of the crate used to import
// from `mob_ai` lives here under the same name. Adding a new public
// item to a submodule means re-exporting it from this list.

pub(crate) use ai::tick_mobs;
pub(crate) use audio::tick_mob_audio;
pub(crate) use damage::{damage_mob, damage_mob_scaled};
#[allow(unused_imports)] // Skeleton + Spider land in H.3 spawn pools.
pub(crate) use kinds::{
    KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_SHEEP, KIND_SKELETON, KIND_SPIDER,
    KIND_ZOMBIE, MobKind,
};
pub(crate) use raycast::raycast_mob;
pub(crate) use state::{
    MOB_ATTACK_REACH, MOB_CAP_DEFAULT, MOB_DEATH_FADE, MOB_HIT_DAMAGE,
    MOB_LUNGE_TIME, MOB_MAX_HEALTH, MobState, ZOMBIE_MAX_HEALTH,
};

#[cfg(test)]
mod tests;
