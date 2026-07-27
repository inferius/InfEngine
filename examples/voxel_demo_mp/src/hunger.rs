//! W415 - the MC hunger system (`FoodData`).
//!
//! Food level 0..=20, saturation 0..=food, exhaustion 0..=4.
//! Exhaustion past 4 eats 1 saturation point (or 1 food when the
//! tank is dry); food >= 18 regenerates 1 HP every 4 s (costing 6
//! exhaustion, MC natural regeneration); food == 0 starves 1 HP
//! every 4 s down to the normal-difficulty floor of 1 HP. Pure
//! state + tick so it unit-tests without the app.

/// MC `FoodData` (player hunger state).
#[derive(Debug, Clone, Copy)]
pub(crate) struct FoodData {
    /// 0..=20 drumstick points.
    pub(crate) food: f32,
    /// 0..=food hidden buffer; drains before food does.
    pub(crate) saturation: f32,
    /// 0..=4 accumulator; overflow converts to saturation/food loss.
    pub(crate) exhaustion: f32,
    /// Seconds toward the next regen / starve event.
    timer: f32,
}

/// What the 4-second timer produced this tick.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HungerEvent {
    None,
    /// Heal 1 HP (natural regeneration).
    Heal,
    /// Take 1 HP of starvation damage.
    Starve,
}

/// MC: both regen and starvation fire on an 80-tick (4 s) cadence.
const EVENT_PERIOD_S: f32 = 4.0;

impl Default for FoodData {
    fn default() -> Self {
        // MC spawn state: 20 food, 5 saturation.
        Self { food: 20.0, saturation: 5.0, exhaustion: 0.0, timer: 0.0 }
    }
}

impl FoodData {
    /// `FoodData.eat` - nutrition points + the item's saturation
    /// modifier (saturation gained = nutrition * modifier * 2,
    /// capped at the new food level).
    pub(crate) fn eat(&mut self, nutrition: u32, sat_modifier: f32) {
        self.food = (self.food + nutrition as f32).min(20.0);
        self.saturation = (self.saturation
            + nutrition as f32 * sat_modifier * 2.0)
            .min(self.food);
    }

    /// `causeFoodExhaustion` - capped at 40 like MC (the tick
    /// consumes it 4 points at a time).
    pub(crate) fn add_exhaustion(&mut self, e: f32) {
        self.exhaustion = (self.exhaustion + e).min(40.0);
    }

    /// Per-frame tick. `health` / `max_health` gate regen; the
    /// caller applies the returned event to the actual HP pool
    /// (and adds the 6.0 regen exhaustion via this fn already).
    pub(crate) fn tick(
        &mut self,
        dt: f32,
        health: f32,
        max_health: f32,
    ) -> HungerEvent {
        // Exhaustion overflow -> saturation, then food.
        while self.exhaustion >= 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.food = (self.food - 1.0).max(0.0);
            }
        }
        // Natural regeneration.
        if self.food >= 18.0 && health < max_health {
            self.timer += dt;
            if self.timer >= EVENT_PERIOD_S {
                self.timer = 0.0;
                self.add_exhaustion(6.0);
                return HungerEvent::Heal;
            }
            return HungerEvent::None;
        }
        // Starvation (normal difficulty: down to 1 HP).
        if self.food <= 0.0 {
            self.timer += dt;
            if self.timer >= EVENT_PERIOD_S {
                self.timer = 0.0;
                if health > 1.0 {
                    return HungerEvent::Starve;
                }
            }
            return HungerEvent::None;
        }
        self.timer = 0.0;
        HungerEvent::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eat_caps_and_saturation() {
        let mut f = FoodData { food: 10.0, saturation: 0.0, exhaustion: 0.0, timer: 0.0 };
        // Bread: 5 nutrition, 0.6 modifier -> 15 food, 6 saturation.
        f.eat(5, 0.6);
        assert_eq!(f.food, 15.0);
        assert_eq!(f.saturation, 6.0);
        // Overeat caps at 20; saturation caps at the food level.
        f.eat(8, 1.2);
        assert_eq!(f.food, 20.0);
        assert!(f.saturation <= f.food);
    }

    #[test]
    fn exhaustion_drains_saturation_then_food() {
        let mut f = FoodData { food: 20.0, saturation: 1.0, exhaustion: 0.0, timer: 0.0 };
        f.add_exhaustion(4.0);
        f.tick(0.016, 20.0, 20.0);
        assert_eq!(f.saturation, 0.0);
        assert_eq!(f.food, 20.0);
        f.add_exhaustion(4.0);
        f.tick(0.016, 20.0, 20.0);
        assert_eq!(f.food, 19.0);
    }

    #[test]
    fn regen_after_four_seconds_costs_exhaustion() {
        let mut f = FoodData::default();
        let mut healed = 0;
        for _ in 0..300 {
            if f.tick(0.05, 10.0, 20.0) == HungerEvent::Heal {
                healed += 1;
            }
        }
        // 15 s at food >= 18 -> 3 heal events, each charging 6.0
        // exhaustion (visible as saturation/food loss over time).
        assert_eq!(healed, 3);
        assert!(f.saturation < 5.0);
    }

    #[test]
    fn starvation_stops_at_one_hp() {
        let mut f = FoodData { food: 0.0, saturation: 0.0, exhaustion: 0.0, timer: 0.0 };
        let mut hits = 0;
        for _ in 0..300 {
            if f.tick(0.05, 20.0, 20.0) == HungerEvent::Starve {
                hits += 1;
            }
        }
        assert_eq!(hits, 3);
        // At the 1 HP floor the timer still runs but never fires.
        let mut f2 = FoodData { food: 0.0, saturation: 0.0, exhaustion: 0.0, timer: 0.0 };
        for _ in 0..300 {
            assert_eq!(f2.tick(0.05, 1.0, 20.0), HungerEvent::None);
        }
    }
}
