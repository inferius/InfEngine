//! MC particle texture sampling helpers.
//!
//! Wave 9.2 already packs every PNG under
//! `assets/minecraft/textures/particle/` into the runtime atlas
//! and keys them as `TextureKey::Particle(stem)`. This module
//! exposes the engine-side sampling vocabulary the ambient
//! particle system uses:
//!
//! - **Smoke** - MC ships `particle/smoke_0.png` ..
//!   `particle/smoke_7.png` (the 8-frame smoke wisp animation).
//!   The W9.5 ambient torch-smoke emitter cycles these over a
//!   particle's lifetime so the smoke wisps animate as they rise.
//! - **Flame** - `particle/flame.png` is a 16x128 strip (8 frames
//!   stacked vertically). Cycled over the flame particle's
//!   lifetime.
//! - **Lava** - `particle/lava.png` is a single 16x16 droplet.
//!
//! Each `SpriteSet` exposes a stable index range (0..N) the
//! emitter writes into the particle struct's `sprite_idx` field.
//! The renderer maps sprite_idx -> atlas UV at draw time.
//!
//! Reference: decompiled MC `26.1.2-sources` ships the particle
//! catalog at
//! `net/minecraft/client/particle/SmokeParticle.java` (8-frame
//! cycle in `pickSprite`), `FlameParticle.java` (single sprite
//! tinted by lifetime), and `LavaParticle.java` (one-frame).

use super::{ResourcePack, TextureKey, TextureRef};

/// One sprite resolved into atlas-relative UVs. The renderer
/// consumes this directly - same shape as `TextureRef` minus the
/// animation metadata (particles drive their own per-particle
/// lifetime, not a clock-based UV scroll).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ParticleSprite {
    pub(crate) atlas_uv: [f32; 4],
}

/// A named bag of particle sprites the emitter cycles through.
/// The index range is `0..frames.len()` - emitters can pick a
/// random sprite, pick by lifetime fraction, or step through
/// sequentially.
#[derive(Debug, Clone, Default)]
pub(crate) struct SpriteSet {
    pub(crate) frames: Vec<ParticleSprite>,
}

impl SpriteSet {
    pub(crate) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.frames.len()
    }

    /// Pick the sprite for `lifetime_frac` in `[0.0, 1.0]`. Frames
    /// are evenly spread across the lifetime so a frac of 0.0
    /// returns frame 0 and a frac just below 1.0 returns the
    /// last frame. `frac >= 1.0` clamps to the last frame.
    /// Returns `None` if the set is empty.
    pub(crate) fn pick_by_lifetime(&self, lifetime_frac: f32) -> Option<ParticleSprite> {
        if self.frames.is_empty() {
            return None;
        }
        let n = self.frames.len();
        let idx = (lifetime_frac.clamp(0.0, 1.0) * n as f32) as usize;
        let idx = idx.min(n - 1);
        Some(self.frames[idx])
    }

    /// Direct sprite-index lookup. Out-of-range `idx` returns the
    /// last frame (a cap, not a wrap, so the renderer doesn't
    /// flicker when an animator overshoots).
    pub(crate) fn pick(&self, idx: usize) -> Option<ParticleSprite> {
        if self.frames.is_empty() {
            return None;
        }
        let last = self.frames.len() - 1;
        Some(self.frames[idx.min(last)])
    }
}

/// Smoke wisp set: `smoke_0.png` .. `smoke_7.png`. When a
/// resource pack only ships a subset the set degrades gracefully
/// (the picker uses whatever frames are present).
pub(crate) fn smoke_set(pack: &ResourcePack) -> SpriteSet {
    sprite_set_named(
        pack,
        &[
            "smoke_0", "smoke_1", "smoke_2", "smoke_3",
            "smoke_4", "smoke_5", "smoke_6", "smoke_7",
        ],
    )
}

/// Flame frame strip: `particle/flame.png` is one vertical strip
/// in MC; the W9.2 atlas packer flattens it into N frames sharing
/// one entry but with `frames > 1`. We expose every frame as a
/// distinct sprite so the per-particle emitter can cycle them.
pub(crate) fn flame_set(pack: &ResourcePack) -> SpriteSet {
    if let Some(tex) = pack.textures.get(&TextureKey::Particle("flame".into())) {
        return sprite_set_from_strip(tex);
    }
    SpriteSet::default()
}

/// Lava droplet: single 16x16 sprite. Returned as a one-frame set
/// so callers can use the same `SpriteSet` shape.
pub(crate) fn lava_set(pack: &ResourcePack) -> SpriteSet {
    sprite_set_named(pack, &["lava"])
}

/// Big smoke (MC's `big_smoke_0..big_smoke_11.png`) - 12 frames.
/// Used by furnace + chimney emitters.
pub(crate) fn big_smoke_set(pack: &ResourcePack) -> SpriteSet {
    sprite_set_named(
        pack,
        &[
            "big_smoke_0", "big_smoke_1", "big_smoke_2", "big_smoke_3",
            "big_smoke_4", "big_smoke_5", "big_smoke_6", "big_smoke_7",
            "big_smoke_8", "big_smoke_9", "big_smoke_10", "big_smoke_11",
        ],
    )
}

/// Build a sprite set from a list of named particle stems. The
/// resulting set contains only the stems that resolved against
/// the pack - missing ones are skipped silently so a partial pack
/// degrades gracefully.
fn sprite_set_named(pack: &ResourcePack, names: &[&str]) -> SpriteSet {
    let mut frames = Vec::with_capacity(names.len());
    for name in names {
        if let Some(tex) = pack.textures.get(&TextureKey::Particle((*name).into())) {
            frames.push(ParticleSprite {
                atlas_uv: tex.atlas_uv,
            });
        }
    }
    SpriteSet { frames }
}

/// Split a `TextureRef` whose atlas occupies a multi-frame strip
/// into N `ParticleSprite` entries (one per frame). Used by the
/// flame loader since `flame.png` is shipped as a vertical strip.
fn sprite_set_from_strip(tex: &TextureRef) -> SpriteSet {
    if tex.frames <= 1 {
        return SpriteSet {
            frames: vec![ParticleSprite {
                atlas_uv: tex.atlas_uv,
            }],
        };
    }
    // The W9.2 atlas stores frame 0's `atlas_uv` + frames stacked
    // sequentially BELOW that rect in the atlas. v_step is the
    // V extent of one frame.
    let v_step = tex.atlas_uv[3] - tex.atlas_uv[1];
    let mut frames = Vec::with_capacity(tex.frames as usize);
    for i in 0..tex.frames {
        let off = i as f32 * v_step;
        frames.push(ParticleSprite {
            atlas_uv: [
                tex.atlas_uv[0],
                tex.atlas_uv[1] + off,
                tex.atlas_uv[2],
                tex.atlas_uv[3] + off,
            ],
        });
    }
    SpriteSet { frames }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Build a synthetic ResourcePack with a controlled set of
    /// particle entries. Avoids any disk I/O.
    fn synth_pack(particles: &[(&str, [f32; 4], u32)]) -> ResourcePack {
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
        ResourcePack {
            textures,
            item_icons: HashMap::new(),
            sounds: HashMap::new(),
            source: PathBuf::from("<test>"),
            is_fallback: false,
            atlas_size: (256, 256),
            atlas_rgba: vec![0u8; 256 * 256 * 4],
        }
    }

    #[test]
    fn particle_atlas_loads_smoke_variants() {
        // All 8 smoke variants present in the pack.
        let entries: Vec<(&str, [f32; 4], u32)> = (0..8u32)
            .map(|i| {
                let name = match i {
                    0 => "smoke_0",
                    1 => "smoke_1",
                    2 => "smoke_2",
                    3 => "smoke_3",
                    4 => "smoke_4",
                    5 => "smoke_5",
                    6 => "smoke_6",
                    _ => "smoke_7",
                };
                let v = i as f32 * 0.0625;
                (name, [0.0, v, 1.0, v + 0.0625], 1)
            })
            .collect();
        let pack = synth_pack(&entries);
        let set = smoke_set(&pack);
        assert_eq!(set.len(), 8);
        assert!(!set.is_empty());
    }

    #[test]
    fn particle_sprite_idx_maps_to_atlas_uv() {
        let pack = synth_pack(&[
            ("smoke_0", [0.0, 0.0, 0.5, 0.5], 1),
            ("smoke_1", [0.5, 0.0, 1.0, 0.5], 1),
        ]);
        let set = smoke_set(&pack);
        assert_eq!(set.len(), 2);
        let s0 = set.pick(0).unwrap();
        assert_eq!(s0.atlas_uv, [0.0, 0.0, 0.5, 0.5]);
        let s1 = set.pick(1).unwrap();
        assert_eq!(s1.atlas_uv, [0.5, 0.0, 1.0, 0.5]);
    }

    #[test]
    fn smoke_set_degrades_when_pack_is_partial() {
        // Only 3 of 8 smoke frames present - the set returns 3.
        let pack = synth_pack(&[
            ("smoke_0", [0.0, 0.0, 0.5, 0.5], 1),
            ("smoke_3", [0.5, 0.0, 1.0, 0.5], 1),
            ("smoke_7", [0.0, 0.5, 0.5, 1.0], 1),
        ]);
        let set = smoke_set(&pack);
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn flame_set_unpacks_vertical_strip() {
        // One entry, 4 frames, frame 0 at v = [0.0, 0.25].
        let pack = synth_pack(&[("flame", [0.0, 0.0, 1.0, 0.25], 4)]);
        let set = flame_set(&pack);
        assert_eq!(set.len(), 4);
        assert_eq!(set.frames[0].atlas_uv, [0.0, 0.0, 1.0, 0.25]);
        assert_eq!(set.frames[1].atlas_uv, [0.0, 0.25, 1.0, 0.5]);
        assert_eq!(set.frames[2].atlas_uv, [0.0, 0.5, 1.0, 0.75]);
        assert_eq!(set.frames[3].atlas_uv, [0.0, 0.75, 1.0, 1.0]);
    }

    #[test]
    fn flame_set_handles_missing_entry() {
        let pack = synth_pack(&[]);
        let set = flame_set(&pack);
        assert!(set.is_empty());
    }

    #[test]
    fn lava_set_is_one_frame() {
        let pack = synth_pack(&[("lava", [0.0, 0.0, 1.0, 1.0], 1)]);
        let set = lava_set(&pack);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn big_smoke_set_loads_twelve_frames() {
        let names = [
            "big_smoke_0",
            "big_smoke_1",
            "big_smoke_2",
            "big_smoke_3",
            "big_smoke_4",
            "big_smoke_5",
            "big_smoke_6",
            "big_smoke_7",
            "big_smoke_8",
            "big_smoke_9",
            "big_smoke_10",
            "big_smoke_11",
        ];
        let entries: Vec<(&str, [f32; 4], u32)> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (*n, [0.0, i as f32 * 0.05, 1.0, (i + 1) as f32 * 0.05], 1))
            .collect();
        let pack = synth_pack(&entries);
        let set = big_smoke_set(&pack);
        assert_eq!(set.len(), 12);
    }

    #[test]
    fn pick_by_lifetime_walks_evenly_across_frames() {
        let pack = synth_pack(&[
            ("smoke_0", [0.0, 0.0, 0.5, 0.25], 1),
            ("smoke_1", [0.0, 0.25, 0.5, 0.5], 1),
            ("smoke_2", [0.0, 0.5, 0.5, 0.75], 1),
            ("smoke_3", [0.0, 0.75, 0.5, 1.0], 1),
        ]);
        let set = smoke_set(&pack);
        assert_eq!(set.len(), 4);
        // 0.0 -> first frame.
        let f0 = set.pick_by_lifetime(0.0).unwrap();
        assert_eq!(f0.atlas_uv[1], 0.0);
        // 0.25 -> second frame.
        let f1 = set.pick_by_lifetime(0.25).unwrap();
        assert_eq!(f1.atlas_uv[1], 0.25);
        // 0.99 -> last frame.
        let f3 = set.pick_by_lifetime(0.99).unwrap();
        assert_eq!(f3.atlas_uv[1], 0.75);
    }

    #[test]
    fn pick_by_lifetime_clamps_above_one() {
        let pack = synth_pack(&[
            ("smoke_0", [0.0, 0.0, 0.5, 0.5], 1),
            ("smoke_1", [0.0, 0.5, 0.5, 1.0], 1),
        ]);
        let set = smoke_set(&pack);
        let f = set.pick_by_lifetime(2.0).unwrap();
        // Should clamp to the last frame.
        assert_eq!(f.atlas_uv[1], 0.5);
    }

    #[test]
    fn pick_clamps_at_last_frame() {
        let pack = synth_pack(&[
            ("smoke_0", [0.0, 0.0, 0.5, 0.5], 1),
            ("smoke_1", [0.0, 0.5, 0.5, 1.0], 1),
        ]);
        let set = smoke_set(&pack);
        let f = set.pick(99).unwrap();
        assert_eq!(f.atlas_uv[1], 0.5);
    }

    #[test]
    fn empty_set_returns_none_for_pickers() {
        let set = SpriteSet::default();
        assert!(set.pick(0).is_none());
        assert!(set.pick_by_lifetime(0.5).is_none());
    }

    #[test]
    fn flame_set_with_single_frame_treats_as_one_sprite() {
        let pack = synth_pack(&[("flame", [0.0, 0.0, 1.0, 1.0], 1)]);
        let set = flame_set(&pack);
        assert_eq!(set.len(), 1);
        assert_eq!(set.frames[0].atlas_uv, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn sprite_set_from_strip_walks_n_frames() {
        let tex = TextureRef {
            atlas_uv: [0.0, 0.0, 1.0, 0.125],
            frames: 8,
            frametime_ms: 100,
            interpolate: false,
        };
        let set = sprite_set_from_strip(&tex);
        assert_eq!(set.len(), 8);
        // Last frame.
        let last = set.frames[7].atlas_uv;
        assert_eq!(last, [0.0, 0.875, 1.0, 1.0]);
    }

    #[test]
    fn smoke_set_resolves_in_order() {
        // Verify the cycle order (0..7) is preserved.
        let pack = synth_pack(&[
            ("smoke_0", [0.0, 0.0, 0.1, 0.1], 1),
            ("smoke_1", [0.1, 0.0, 0.2, 0.1], 1),
            ("smoke_2", [0.2, 0.0, 0.3, 0.1], 1),
            ("smoke_3", [0.3, 0.0, 0.4, 0.1], 1),
            ("smoke_4", [0.4, 0.0, 0.5, 0.1], 1),
            ("smoke_5", [0.5, 0.0, 0.6, 0.1], 1),
            ("smoke_6", [0.6, 0.0, 0.7, 0.1], 1),
            ("smoke_7", [0.7, 0.0, 0.8, 0.1], 1),
        ]);
        let set = smoke_set(&pack);
        assert_eq!(set.len(), 8);
        // Frame 0 sits at u=0.0..0.1, frame 7 at u=0.7..0.8.
        assert_eq!(set.frames[0].atlas_uv[0], 0.0);
        assert_eq!(set.frames[7].atlas_uv[0], 0.7);
    }
}
