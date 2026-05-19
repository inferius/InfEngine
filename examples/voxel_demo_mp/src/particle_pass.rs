//! Wave 12.3 - Textured-particle render-pass plumbing.
//!
//! The CPU side of the textured particle pipeline:
//! - `ParticleInstance` carries the per-particle data the future
//!   GPU pass uploads each frame (pos, scale, color, sprite_idx,
//!   frame_count, age).
//! - `ParticleAtlas` indexes a list of `SpriteSet`s by `EmitterKind`
//!   so the renderer can look up the right cycle for a given
//!   particle (smoke / flame / lava / big smoke).
//! - `resolve_uv` walks the atlas + advances the frame index by
//!   the particle's age, returning the atlas-relative UV rect the
//!   GPU shader samples.
//!
//! The actual wgpu pipeline (a per-frame instance buffer, textured
//! billboard shader, alpha-blend draw) is a follow-up. This module
//! ships the data shape so the GPU integration is a thin layer on
//! top - no code path needs to be re-plumbed when the GPU side
//! lands.
//!
//! Cite MC `26.1.2-sources`:
//! - `net/minecraft/client/particle/SmokeParticle.java::pickSprite`
//!   (8-frame cycle keyed by lifetime).
//! - `net/minecraft/client/particle/FlameParticle.java` (single
//!   tinted sprite over lifetime).
//! - `net/minecraft/client/particle/LavaParticle.java` (one-frame).

use crate::resource_pack::{
    particles::{
        big_smoke_set, flame_set, lava_set, smoke_set, ParticleSprite, SpriteSet,
    },
    ResourcePack,
};

/// Which atlas slot a particle samples from. The renderer dispatches
/// the matching `SpriteSet` per particle. Adding a new emitter kind
/// = one variant + one `SpriteSet` build in `ParticleAtlas::build`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EmitterKind {
    /// MC `smoke_0..7` 8-frame cycle. Torches, fire emitters.
    Smoke,
    /// MC `big_smoke_0..11` 12-frame cycle. Furnace plume.
    BigSmoke,
    /// MC `flame.png` 8-frame strip. Fire emitters.
    Flame,
    /// MC `lava.png` single sprite. Lava sources.
    Lava,
}

impl EmitterKind {
    /// Stable index for instance-buffer encoding. Picked once at
    /// build time + recorded on the particle so the GPU shader can
    /// index the atlas binding directly.
    pub(crate) const fn idx(self) -> u8 {
        match self {
            EmitterKind::Smoke => 0,
            EmitterKind::BigSmoke => 1,
            EmitterKind::Flame => 2,
            EmitterKind::Lava => 3,
        }
    }
}

/// Per-particle data the GPU instance buffer uploads. Mirrors the
/// existing `infengine_particles::Particle` fields the engine cares
/// about + the new sprite-routing data. Built by `instance_from`
/// once per live particle each frame.
///
/// 16-byte alignment isn't enforced here - the GPU side will pack
/// to whatever wgpu needs. This struct is the CPU intermediate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ParticleInstance {
    pub(crate) position: [f32; 3],
    /// Quad scale in world units. Matches the existing `Particle::size`.
    pub(crate) scale: f32,
    /// Particle tint (alpha = opacity). Existing tinted-cube path
    /// already populates this; the textured path multiplies by the
    /// sprite's sampled colour.
    pub(crate) color: [f32; 4],
    /// Which atlas slot to sample. Encoded as `EmitterKind::idx()`.
    pub(crate) sprite_idx: u8,
    /// Number of animation frames in the source set.
    pub(crate) frame_count: u8,
    /// Particle age in seconds. The GPU shader derives the current
    /// frame from `(age / lifetime) * frame_count`.
    pub(crate) age: f32,
    /// Particle total lifetime, used to compute the frame fraction.
    pub(crate) lifetime: f32,
}

impl ParticleInstance {
    /// Lifetime fraction in `[0, 1]` matching the simulation's
    /// `Particle::fraction()`. Saturates at 1.0 for past-end-of-life.
    pub(crate) fn lifetime_frac(&self) -> f32 {
        if self.lifetime <= 0.0 {
            1.0
        } else {
            (self.age / self.lifetime).clamp(0.0, 1.0)
        }
    }

    /// Compute the current animation frame index for this particle.
    /// Out-of-range frames clamp to the last frame (matches the
    /// `SpriteSet::pick` behaviour so the shader never flickers).
    pub(crate) fn current_frame(&self) -> u8 {
        if self.frame_count <= 1 {
            return 0;
        }
        let frac = self.lifetime_frac();
        let f = (frac * self.frame_count as f32) as u32;
        f.min(self.frame_count as u32 - 1) as u8
    }
}

/// Indexed pool of `SpriteSet`s, one per `EmitterKind`. Built from
/// the loaded `ResourcePack` once at startup and again whenever the
/// pack reloads.
pub(crate) struct ParticleAtlas {
    smoke: SpriteSet,
    big_smoke: SpriteSet,
    flame: SpriteSet,
    lava: SpriteSet,
}

impl ParticleAtlas {
    /// Build from a loaded pack. Missing sprite sets stay empty;
    /// `resolve_uv` returns `None` so callers fall through to the
    /// legacy tinted-cube path.
    pub(crate) fn build(pack: &ResourcePack) -> Self {
        Self {
            smoke: smoke_set(pack),
            big_smoke: big_smoke_set(pack),
            flame: flame_set(pack),
            lava: lava_set(pack),
        }
    }

    fn set_for(&self, kind: EmitterKind) -> &SpriteSet {
        match kind {
            EmitterKind::Smoke => &self.smoke,
            EmitterKind::BigSmoke => &self.big_smoke,
            EmitterKind::Flame => &self.flame,
            EmitterKind::Lava => &self.lava,
        }
    }

    /// How many animation frames the named kind has loaded. Used
    /// when packing an instance: emitters set `frame_count =
    /// atlas.frame_count(kind)` so the GPU shader knows the cycle
    /// length without an extra binding.
    pub(crate) fn frame_count(&self, kind: EmitterKind) -> u8 {
        let n = self.set_for(kind).len();
        n.min(255) as u8
    }

    /// Resolve the atlas-relative UV rect for an instance. The
    /// renderer feeds this into the per-particle quad. Returns
    /// `None` when the kind has no loaded sprites (placeholder
    /// pack) so the caller can route to the legacy tinted path.
    pub(crate) fn resolve_uv(
        &self,
        kind: EmitterKind,
        inst: &ParticleInstance,
    ) -> Option<ParticleSprite> {
        let set = self.set_for(kind);
        if set.is_empty() {
            return None;
        }
        let frame = inst.current_frame() as usize;
        set.pick(frame)
    }

    /// Returns true if every supported kind has at least one
    /// loaded sprite. Diagnostic used by the engine's startup
    /// banner + by the `pass_renders_with_no_particles_no_crash`
    /// test.
    pub(crate) fn is_fully_loaded(&self) -> bool {
        !self.smoke.is_empty()
            && !self.big_smoke.is_empty()
            && !self.flame.is_empty()
            && !self.lava.is_empty()
    }
}

/// Build a `ParticleInstance` from the engine's existing simulation
/// particle + the kind it belongs to. The kind is supplied at the
/// call site because the engine's `Particle` doesn't carry it
/// directly (it's implicit in the emitter that spawned it).
pub(crate) fn instance_from(
    p: &infengine_particles::Particle,
    kind: EmitterKind,
    atlas: &ParticleAtlas,
) -> ParticleInstance {
    ParticleInstance {
        position: p.position,
        scale: p.size,
        color: p.color,
        sprite_idx: kind.idx(),
        frame_count: atlas.frame_count(kind),
        age: p.age,
        lifetime: p.lifetime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_pack::{TextureKey, TextureRef};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Helper: build a synthetic pack with a controlled set of
    /// particle entries. Avoids any disk I/O.
    fn synth_pack(
        particles: &[(&str, [f32; 4], u32)],
        size: (u32, u32),
    ) -> ResourcePack {
        let mut textures = HashMap::new();
        for (name, uv, frames) in particles {
            textures.insert(
                TextureKey::Particle((*name).into()),
                TextureRef {
                    atlas_uv: *uv,
                    frames: *frames,
                    frametime_ms: if *frames > 1 { 100 } else { 0 },
                    interpolate: false,
                },
            );
        }
        let (w, h) = size;
        ResourcePack {
            textures,
            item_icons: HashMap::new(),
            sounds: HashMap::new(),
            source: PathBuf::from("<test>"),
            is_fallback: false,
            atlas_size: (w, h),
            atlas_rgba: vec![0u8; (w * h * 4) as usize],
            anim_registry: crate::resource_pack::anim_registry::AnimRegistry::new(),
            anim_slots: HashMap::new(),
        }
    }

    fn pack_with_smoke() -> ResourcePack {
        // Eight smoke frames spread across the atlas horizontally.
        let mut entries: Vec<(&str, [f32; 4], u32)> = Vec::new();
        for i in 0..8u32 {
            let u0 = i as f32 / 8.0;
            let u1 = (i + 1) as f32 / 8.0;
            let name = match i {
                0 => "smoke_0",
                1 => "smoke_1",
                2 => "smoke_2",
                3 => "smoke_3",
                4 => "smoke_4",
                5 => "smoke_5",
                6 => "smoke_6",
                7 => "smoke_7",
                _ => unreachable!(),
            };
            entries.push((name, [u0, 0.0, u1, 0.125], 1));
        }
        synth_pack(&entries, (256, 256))
    }

    #[test]
    fn smoke_instance_samples_correct_sprite() {
        // At frac = 0.5 (mid-life) the smoke cycle should pick the
        // 4th frame (idx 4) out of 8.
        let pack = pack_with_smoke();
        let atlas = ParticleAtlas::build(&pack);
        let inst = ParticleInstance {
            position: [0.0; 3],
            scale: 0.1,
            color: [1.0; 4],
            sprite_idx: EmitterKind::Smoke.idx(),
            frame_count: atlas.frame_count(EmitterKind::Smoke),
            age: 0.9,
            lifetime: 1.8,
        };
        assert_eq!(inst.current_frame(), 4);
        let sprite = atlas
            .resolve_uv(EmitterKind::Smoke, &inst)
            .expect("smoke uv resolved");
        // Frame 4 of 8 -> u0 = 4/8 = 0.5.
        assert!((sprite.atlas_uv[0] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn particle_age_advances_frame() {
        let pack = pack_with_smoke();
        let atlas = ParticleAtlas::build(&pack);
        let mut inst = ParticleInstance {
            position: [0.0; 3],
            scale: 0.1,
            color: [1.0; 4],
            sprite_idx: EmitterKind::Smoke.idx(),
            frame_count: atlas.frame_count(EmitterKind::Smoke),
            age: 0.0,
            lifetime: 1.0,
        };
        let f0 = inst.current_frame();
        inst.age = 0.25;
        let f1 = inst.current_frame();
        inst.age = 0.5;
        let f2 = inst.current_frame();
        inst.age = 0.99;
        let f3 = inst.current_frame();
        // Monotonically non-decreasing.
        assert!(f0 <= f1, "{f0} not <= {f1}");
        assert!(f1 <= f2, "{f1} not <= {f2}");
        assert!(f2 <= f3, "{f2} not <= {f3}");
        // At least one transition between full-life span.
        assert!(f0 < f3, "frame didn't advance across full lifetime");
    }

    #[test]
    fn pass_renders_with_no_particles_no_crash() {
        // Building an atlas from a pack with NO loaded particle
        // textures must succeed + `is_fully_loaded()` must report
        // false. The caller's "no particles to render" path is
        // exercised by calling resolve_uv on an empty kind.
        let pack = synth_pack(&[], (64, 64));
        let atlas = ParticleAtlas::build(&pack);
        assert!(!atlas.is_fully_loaded());
        let inst = ParticleInstance {
            position: [0.0; 3],
            scale: 0.1,
            color: [1.0; 4],
            sprite_idx: EmitterKind::Smoke.idx(),
            frame_count: 0,
            age: 0.0,
            lifetime: 1.0,
        };
        assert!(atlas.resolve_uv(EmitterKind::Smoke, &inst).is_none());
    }

    #[test]
    fn emitter_kind_indices_stable() {
        // Stable encoding across waves so save-format / instance
        // buffer layouts don't shift on a register reshuffle.
        assert_eq!(EmitterKind::Smoke.idx(), 0);
        assert_eq!(EmitterKind::BigSmoke.idx(), 1);
        assert_eq!(EmitterKind::Flame.idx(), 2);
        assert_eq!(EmitterKind::Lava.idx(), 3);
    }

    #[test]
    fn frame_count_matches_atlas() {
        let pack = pack_with_smoke();
        let atlas = ParticleAtlas::build(&pack);
        assert_eq!(atlas.frame_count(EmitterKind::Smoke), 8);
        assert_eq!(atlas.frame_count(EmitterKind::Flame), 0); // not in synth pack
    }

    #[test]
    fn frac_saturates_past_end_of_life() {
        let inst = ParticleInstance {
            position: [0.0; 3],
            scale: 0.1,
            color: [1.0; 4],
            sprite_idx: 0,
            frame_count: 8,
            age: 5.0,
            lifetime: 1.0,
        };
        assert!((inst.lifetime_frac() - 1.0).abs() < 1e-3);
        assert_eq!(inst.current_frame(), 7); // last frame, not wrapped.
    }

    #[test]
    fn zero_lifetime_returns_full_frac() {
        let inst = ParticleInstance {
            position: [0.0; 3],
            scale: 0.1,
            color: [1.0; 4],
            sprite_idx: 0,
            frame_count: 0,
            age: 0.0,
            lifetime: 0.0,
        };
        assert!((inst.lifetime_frac() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn instance_from_copies_simulation_fields() {
        let pack = pack_with_smoke();
        let atlas = ParticleAtlas::build(&pack);
        let p = infengine_particles::Particle {
            position: [1.0, 2.0, 3.0],
            velocity: [0.0; 3],
            color: [0.5, 0.5, 0.5, 0.7],
            size: 0.12,
            age: 0.6,
            lifetime: 1.2,
        };
        let inst = instance_from(&p, EmitterKind::Smoke, &atlas);
        assert_eq!(inst.position, p.position);
        assert_eq!(inst.scale, p.size);
        assert_eq!(inst.color, p.color);
        assert_eq!(inst.sprite_idx, EmitterKind::Smoke.idx());
        assert_eq!(inst.frame_count, 8);
        assert!((inst.lifetime_frac() - 0.5).abs() < 1e-3);
    }
}
