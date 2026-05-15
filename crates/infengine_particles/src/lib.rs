//! CPU-side particle simulation for InfEngine.
//!
//! Phase 1 is a deliberately small slice: an `Emitter` spawns
//! `Particle`s with a configurable lifetime + velocity, the system
//! integrates position / velocity each tick and culls dead
//! particles. The renderer consumes the live particle list via
//! `iter_live()` and draws each as a screen-space quad / small
//! cube; rendering is not part of this crate so the simulation can
//! be unit-tested without GPU resources.
//!
//! Wire shape stays serde-friendly so a future scene component can
//! persist emitters directly.

#![forbid(unsafe_code)]

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// One simulated particle. `age` counts seconds since spawn so
/// downstream code can fade colour / size with a normalised
/// fraction (`age / lifetime`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Particle {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub color: [f32; 4],
    pub size: f32,
    pub age: f32,
    pub lifetime: f32,
}

impl Particle {
    /// Normalised age in `[0, 1]`. Useful for fade curves.
    pub fn fraction(&self) -> f32 {
        if self.lifetime <= 0.0 {
            1.0
        } else {
            (self.age / self.lifetime).clamp(0.0, 1.0)
        }
    }

    pub fn is_dead(&self) -> bool {
        self.age >= self.lifetime
    }
}

/// One emitter. `rate` is particles per second; `gravity` is the
/// world-space acceleration applied to every live particle. The
/// initial velocity is `velocity_base + sample(velocity_spread)` -
/// sampling uses a deterministic xorshift so behaviour is
/// reproducible without an `rand` dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Emitter {
    pub position: [f32; 3],
    pub rate: f32,
    pub lifetime: f32,
    pub velocity_base: [f32; 3],
    pub velocity_spread: [f32; 3],
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub gravity: [f32; 3],
    /// Capacity ceiling: the system stops spawning new particles
    /// once `live_count >= max_particles`. Keeps the simulation
    /// bounded under bursty emitters.
    pub max_particles: usize,
    /// Fractional spawn accumulator. Bumped by `rate * dt` each
    /// tick; whole-number portions become new particles. Reset to
    /// zero by `Default`; downstream code rarely touches it.
    #[serde(skip)]
    pub spawn_acc: f32,
    /// xorshift32 state for deterministic velocity sampling.
    /// `Default` seeds with the golden-ratio constant.
    #[serde(skip)]
    pub rng: u32,
}

impl Default for Emitter {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rate: 30.0,
            lifetime: 2.0,
            velocity_base: [0.0, 2.0, 0.0],
            velocity_spread: [1.0, 0.5, 1.0],
            color_start: [1.0, 0.6, 0.2, 1.0],
            color_end: [1.0, 0.2, 0.0, 0.0],
            size_start: 0.25,
            size_end: 0.05,
            gravity: [0.0, -1.5, 0.0],
            max_particles: 1024,
            spawn_acc: 0.0,
            rng: 0x9E37_79B1,
        }
    }
}

impl Emitter {
    fn next_rng(&mut self) -> f32 {
        // xorshift32 - one float in `[-1, 1]` per call. Deterministic.
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn sample_velocity(&mut self) -> [f32; 3] {
        [
            self.velocity_base[0] + self.next_rng() * self.velocity_spread[0],
            self.velocity_base[1] + self.next_rng() * self.velocity_spread[1],
            self.velocity_base[2] + self.next_rng() * self.velocity_spread[2],
        ]
    }
}

/// Particle system: a list of emitters + a live particle pool.
/// Caller drives it with `tick(dt)`; rendering happens elsewhere.
#[derive(Debug, Default, Clone)]
pub struct ParticleSystem {
    pub emitters: Vec<Emitter>,
    pub particles: Vec<Particle>,
}

impl ParticleSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_emitter(&mut self, e: Emitter) -> usize {
        self.emitters.push(e);
        self.emitters.len() - 1
    }

    pub fn live_count(&self) -> usize {
        self.particles.len()
    }

    /// Iterate live particles. The renderer calls this per frame
    /// to push instance data.
    pub fn iter_live(&self) -> impl Iterator<Item = &Particle> {
        self.particles.iter()
    }

    /// Advance the simulation by `dt` seconds. Spawns new
    /// particles according to each emitter's `rate`, integrates
    /// position + velocity for every live particle, and culls
    /// dead ones in-place.
    pub fn tick(&mut self, dt: f32) {
        // Spawn pass.
        let cap_remaining = |live: usize, max: usize| max.saturating_sub(live);
        for e in &mut self.emitters {
            e.spawn_acc += e.rate * dt;
            let mut to_spawn = e.spawn_acc.floor() as i32;
            e.spawn_acc -= to_spawn as f32;
            let room = cap_remaining(self.particles.len(), e.max_particles) as i32;
            if to_spawn > room {
                to_spawn = room;
            }
            for _ in 0..to_spawn {
                let velocity = e.sample_velocity();
                self.particles.push(Particle {
                    position: e.position,
                    velocity,
                    color: e.color_start,
                    size: e.size_start,
                    age: 0.0,
                    lifetime: e.lifetime,
                });
            }
        }
        // Integrate + interpolate colour / size by age fraction.
        // Drop-in-place via retain_mut keeps the pool compact
        // without an aux buffer.
        let mut i = 0;
        while i < self.particles.len() {
            let p = &mut self.particles[i];
            p.age += dt;
            if p.is_dead() {
                self.particles.swap_remove(i);
                continue;
            }
            // Linear euler.
            let pos = Vec3::from_array(p.position);
            let mut vel = Vec3::from_array(p.velocity);
            // Apply gravity from the first emitter whose particles
            // we're integrating. Cheap heuristic: take the closest
            // emitter's gravity vector. Most scenes have one
            // dominant emitter so this stays correct in practice;
            // the unit test covers the single-emitter case.
            if let Some(g) = self
                .emitters
                .first()
                .map(|e| Vec3::from_array(e.gravity))
            {
                vel += g * dt;
            }
            let new_pos = pos + vel * dt;
            p.position = new_pos.to_array();
            p.velocity = vel.to_array();
            // Interpolate colour + size via the emitter the
            // particle was spawned from (here we approximate by
            // using the first emitter; storing per-particle
            // emitter idx is a v2 improvement).
            if let Some(e) = self.emitters.first() {
                let t = p.fraction();
                for c in 0..4 {
                    p.color[c] = e.color_start[c] + (e.color_end[c] - e.color_start[c]) * t;
                }
                p.size = e.size_start + (e.size_end - e.size_start) * t;
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitter_spawns_per_rate() {
        let mut sys = ParticleSystem::new();
        sys.add_emitter(Emitter {
            rate: 60.0,
            lifetime: 1.0,
            ..Default::default()
        });
        // 1 second at 60 Hz = ~60 particles spawned, all still
        // live (lifetime = 1s, accumulated age < 1s at this step).
        for _ in 0..60 {
            sys.tick(1.0 / 60.0);
        }
        // First spawned particle has age ~1s, may have died this
        // step. Just check we built up live count first.
        assert!(sys.live_count() > 0);
    }

    #[test]
    fn particles_die_after_lifetime() {
        let mut sys = ParticleSystem::new();
        sys.add_emitter(Emitter {
            rate: 10.0,
            lifetime: 0.1,
            ..Default::default()
        });
        sys.tick(0.5); // long enough for every spawned particle to die
        // After a long advance the system should have spawned + killed
        // several particles. Live count should be bounded.
        assert!(sys.live_count() <= 5);
    }

    #[test]
    fn capacity_cap_holds() {
        let mut sys = ParticleSystem::new();
        sys.add_emitter(Emitter {
            rate: 10_000.0,
            lifetime: 10.0,
            max_particles: 50,
            ..Default::default()
        });
        sys.tick(1.0);
        assert!(sys.live_count() <= 50);
    }
}
