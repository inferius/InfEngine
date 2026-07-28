//! W438 - player status effects (`MobEffect` MVP).
//!
//! Three effects carry the witch fight + future potions: Slowness
//! (walk speed x0.85 per level, one level here), Poison (1 HP per
//! 1.25 s down to the MC 1 HP floor, bypasses armor) and
//! Regeneration (1 HP per 2.5 s). Timers tick in seconds; the HUD
//! prints the remaining time per active effect.

/// Active player effects - each field is seconds remaining.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PlayerEffects {
    pub(crate) slowness: f32,
    pub(crate) poison: f32,
    pub(crate) regen: f32,
    poison_timer: f32,
    regen_timer: f32,
}

/// What one tick asks the caller to apply to the HP pool.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EffectEvent {
    None,
    /// 1 HP of armor-bypassing poison damage (never lethal).
    PoisonTick,
    /// 1 HP of regeneration.
    RegenTick,
}

/// MC Poison amplifier 0: damage every 25 ticks = 1.25 s.
const POISON_PERIOD_S: f32 = 1.25;
/// MC Regeneration amplifier 0: heal every 50 ticks = 2.5 s.
const REGEN_PERIOD_S: f32 = 2.5;
/// MC Slowness I: -15% movement speed.
pub(crate) const SLOWNESS_FACTOR: f32 = 0.85;

impl PlayerEffects {
    pub(crate) fn any_active(&self) -> bool {
        self.slowness > 0.0 || self.poison > 0.0 || self.regen > 0.0
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Advance the timers; the caller applies the returned event
    /// (poison respects its own 1 HP floor there).
    pub(crate) fn tick(&mut self, dt: f32) -> EffectEvent {
        if self.slowness > 0.0 {
            self.slowness = (self.slowness - dt).max(0.0);
        }
        if self.regen > 0.0 {
            self.regen = (self.regen - dt).max(0.0);
            self.regen_timer += dt;
            if self.regen_timer >= REGEN_PERIOD_S {
                self.regen_timer = 0.0;
                return EffectEvent::RegenTick;
            }
        } else {
            self.regen_timer = 0.0;
        }
        if self.poison > 0.0 {
            self.poison = (self.poison - dt).max(0.0);
            self.poison_timer += dt;
            if self.poison_timer >= POISON_PERIOD_S {
                self.poison_timer = 0.0;
                return EffectEvent::PoisonTick;
            }
        } else {
            self.poison_timer = 0.0;
        }
        EffectEvent::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poison_ticks_on_mc_cadence() {
        let mut e = PlayerEffects { poison: 5.5, ..Default::default() };
        let mut hits = 0;
        // 5.5 s of poison across 120 x 0.05 s steps: cadence
        // points at 1.25/2.5/3.75/5.0 -> exactly 4 ticks, with
        // float headroom on the last one.
        for _ in 0..120 {
            if e.tick(0.05) == EffectEvent::PoisonTick {
                hits += 1;
            }
        }
        assert_eq!(hits, 4);
        assert_eq!(e.poison, 0.0);
    }

    #[test]
    fn regen_heals_slower_than_poison_hurts() {
        let mut e = PlayerEffects {
            poison: 10.0,
            regen: 10.0,
            ..Default::default()
        };
        let (mut p, mut r) = (0, 0);
        for _ in 0..200 {
            match e.tick(0.05) {
                EffectEvent::PoisonTick => p += 1,
                EffectEvent::RegenTick => r += 1,
                EffectEvent::None => {}
            }
        }
        assert!(p > r);
        assert!(r >= 3);
    }
}
