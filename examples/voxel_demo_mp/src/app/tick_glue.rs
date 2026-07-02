//! W47.28 - tick_glue impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// Q.4c: fire random-tick. Walks `self.fires` once per
    /// net-tick frame; each cell rolls `1/RATE_DENOM` and on
    /// success runs `block::fire::plan_fire_cell`. Server-only
    /// (clients receive the resulting `vx.edit` traffic).
    pub(crate) fn fire_tick(&mut self) { crate::fire_runtime::fire_tick(self); }
    pub(crate) fn apply_fire_step(&mut self, step: block::fire::FireStep) { crate::fire_runtime::apply_fire_step(self, step); }
    pub(crate) fn tick_lava_fire_emit(&mut self) { crate::fire_runtime::tick_lava_fire_emit(self); }

    /// Server-side mob update: gravity-snap to surface, walk
    /// toward target, retarget on arrival / timeout, spawn new
    /// sheep when the cap allows. Builds the broadcast list as a
    /// side effect.
    /// Thin wrapper - see `mob::ai::tick_mobs`.
    pub(crate) fn tick_mobs(&mut self, dt: f32) {
        mob::tick_mobs(self, dt);
    }

    /// Thin wrapper - see `mob::audio::tick_mob_audio`.
    pub(crate) fn tick_mob_audio(&mut self, dt: f32) {
        mob::tick_mob_audio(self, dt);
    }

    pub(crate) fn sfx_at(
        &mut self,
        key: &str,
        pos: [f32; 3],
        synth: impl FnOnce(&mut infengine_audio::AudioSystem, f32),
    ) {
        const AUDIBLE_RANGE: f32 = 24.0;
        let Some(a) = self.audio.as_mut() else {
            return;
        };
        let listener = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
        // Per-sound mix level. MC plays footsteps quiet (Entity
        // .playStepSound uses volume * 0.15) - at full gain they drown
        // everything else (the user's "kroky moc hlasite"). Match that
        // for player + mob step keys; everything else stays at 1.0.
        let volume = key_volume(key);
        // Seed the variant shuffle from tick + spatial position so
        // back-to-back triggers don't reuse the same recording.
        let seed = self.net_tick.wrapping_mul(2654435761)
            ^ (pos[0].to_bits() as u64).wrapping_mul(7919)
            ^ (pos[2].to_bits() as u64);
        if self
            .audio_bank
            .play_at(a, key, listener, pos, AUDIBLE_RANGE, seed, volume)
            .is_some()
        {
            return;
        }
        // Bank miss / out of range. For out-of-range we still want
        // the synth path to gate on distance, so reuse the same
        // formula here.
        let dx = pos[0] - listener[0];
        let dy = pos[1] - listener[1];
        let dz = pos[2] - listener[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist >= AUDIBLE_RANGE {
            return;
        }
        // W200.1: log each bank-missed key ONCE so any audible synth
        // fallback (beeps/clicks the MC pack should have covered) is
        // attributable from the log instead of guessed at. The repeat
        // offenders so far: mob beeps (W56.1), footstep clicks (W200.1).
        {
            use std::collections::HashSet;
            use std::sync::{Mutex, OnceLock};
            static MISSED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
            let missed = MISSED.get_or_init(|| Mutex::new(HashSet::new()));
            if let Ok(mut set) = missed.lock()
                && set.insert(key.to_string())
            {
                tracing::warn!(key, "W200.1: sfx bank miss -> synth fallback");
            }
        }
        let gain = (1.0 - dist / AUDIBLE_RANGE).clamp(0.0, 1.0) * volume;
        synth(a, gain);
    }

}

/// Per-sound mix level applied on top of the distance falloff.
/// Footsteps (player `step:<material>` + mob `mob/<kind>/step`) play
/// at MC's 0.15 so they don't dominate the mix; everything else is
/// full volume. Mirrors `Entity.playStepSound`'s `volume * 0.15`.
fn key_volume(key: &str) -> f32 {
    if key.starts_with("step:") || key.ends_with("/step") {
        0.15
    } else {
        1.0
    }
}
