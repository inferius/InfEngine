//! Per-mob SFX companion tick: footstep swishes + species "say"
//! lines, hurt-flash decay, death-fade decay, lunge-timer decay.
//!
//! Runs on every peer (the server's audio comes through too if the
//! host is playing). The decay passes for the visual timers also
//! live here so the same tick boundary clears both audio + visual
//! state.
//!
//! MC mirror: `net/minecraft/world/entity/LivingEntity::tickEffects`
//! interleaves potion / status timers with the entity's per-tick
//! sound + animation hooks. We split the timer decay into this
//! audio tick to avoid borrowing the mutable mob list while playing
//! audio (the audio path calls back into `&mut Voxel`).

use std::collections::HashSet;

use crate::Voxel;
use crate::net::MobMsg;

use super::kinds::{
    KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_POLAR_BEAR, KIND_RABBIT, KIND_SHEEP,
    KIND_SKELETON, KIND_SPIDER, KIND_WOLF, KIND_ZOMBIE,
};

/// W55.4 - per-mob sound key prefix. Returns the engine key stem
/// (`mob/<name>`) used by `sfx_at` to look up the matching MC
/// event via `ENGINE_TO_MC`. Unknown kinds collapse to sheep so
/// the existing fallback baa/step sound still fires.
/// W123: pub(crate) - the damage + net paths key hurt/death off it.
pub(crate) fn mob_sound_stem(kind: u8) -> &'static str {
    match kind {
        KIND_SHEEP => "mob/sheep",
        KIND_ZOMBIE => "mob/zombie",
        KIND_CHICKEN => "mob/chicken",
        KIND_PIG => "mob/pig",
        KIND_COW => "mob/cow",
        KIND_SKELETON => "mob/skeleton",
        KIND_SPIDER => "mob/spider",
        KIND_WOLF => "mob/wolf",
        KIND_POLAR_BEAR => "mob/polar_bear",
        KIND_RABBIT => "mob/rabbit",
        x if x == crate::mob::kinds::KIND_ZOMBIFIED_PIGLIN => {
            "mob/zombified_piglin"
        }
        x if x == crate::mob::kinds::KIND_VILLAGER => "mob/villager",
        x if x == crate::mob::kinds::KIND_CAT => "mob/cat",
        x if x == crate::mob::kinds::KIND_IRON_GOLEM => "mob/iron_golem",
        x if x == crate::mob::kinds::KIND_HORSE => "mob/horse",
        x if x == crate::mob::kinds::KIND_GHAST => "mob/ghast",
        x if x == crate::mob::kinds::KIND_MAGMA_CUBE => "mob/magma_cube",
        _ => "mob/sheep",
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
                let key = format!("{}/step", mob_sound_stem(m.kind));
                // W56.1: no synth fallback for mob steps. The
                // play_swish beep was firing for EVERY mob (since
                // the cached pack has no `sounds/` subtree),
                // producing a constant chorus of beeps. Silent
                // when the OGG isn't loaded; the real audio
                // arrives when a proper MC pack ships sound
                // files.
                voxel.sfx_at(&key, pos, |_a, _g| {});
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
            let key = format!("{}/say", mob_sound_stem(m.kind));
            // W56.1: no synth fallback for mob ambient. See step
            // handler above; silent until the pack ships sounds.
            voxel.sfx_at(&key, pos, |_a, _g| {});
        }
    }
}
