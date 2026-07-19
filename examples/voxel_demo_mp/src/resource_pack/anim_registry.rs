//! Animation slot registry + per-frame UV math.
//!
//! Wave 9.4 added `frames` + `frametime_ms` on every `TextureRef`,
//! but the chunk mesher writes static frame-0 UVs into the vertex
//! buffer + the per-vertex animation data path is not yet wired
//! through the WGSL shader. This module ships the CPU-side data
//! plumbing the future shader integration will need:
//!
//! 1. A registry that assigns each animated tile a numeric slot
//!    (1..=32). Static tiles report slot 0. The shader's anim
//!    uniform array is keyed by this slot.
//! 2. Per-frame UV scroll math identical to the shader's planned
//!    branch:
//!    `frame = (time_ms / frametime_ms) % frame_count;`
//!    `v0 += frame * v_step; v1 += frame * v_step;`
//!    where `v_step = (atlas_uv[3] - atlas_uv[1])` (the V extent
//!    of frame 0 in the atlas).
//! 3. A bounded slot pool (cap = 32, configurable) so the shader's
//!    uniform array stays statically sized.
//!
//! The future GPU path uses this module by:
//! - At pack-load time: walk the texture map + call
//!   `register_animated` for every `TextureRef` with `frames > 1`,
//!   storing the returned slot.
//! - In the mesher: the per-quad emit path queries
//!   `slot_for_texture(texture_ref)` and writes the slot id into
//!   the new `anim_id: u32` vertex attribute.
//! - In the shader: a uniform `tile_anim[32]` array reads the slot
//!   id, computes the v offset (using the math here), and shifts
//!   the sampled UV before reading the atlas.
//!
//! For Wave 9.x the WGSL shader does NOT yet read this data - the
//! mesher continues to emit static frame-0 UVs. But the registry
//! is callable from the existing render path so the W9.4 ambient
//! flame/particle animations + a CPU-side per-tick UV-update
//! fallback can land before the GPU integration completes.

use std::collections::HashMap;

use super::TextureRef;

/// Hard upper bound on simultaneously-animated tiles. Matches the
/// shader's fixed-size `tile_anim` uniform array
/// (`infengine_render::MAX_ANIM_SLOTS` + the WGSL array size) -
/// bump all three together if you raise it. W320: 32 -> 128. The
/// real MC pack ships ~102 animated block strips (fire, portal,
/// water, lava, campfire, kelp, sea lantern, stems, ...); the
/// dedup key below collapses them to ~22 live slots today, but
/// that count is a function of the atlas shelf layout (same-row
/// strips share v0), so 128 keeps a future repack from silently
/// overflowing into static tiles.
pub(crate) const MAX_ANIM_SLOTS: usize = 128;

/// Sentinel slot meaning "static texture, no animation". The
/// shader's `tile_anim` lookup branches on `slot > 0` so this
/// value MUST stay at 0.
pub(crate) const STATIC_SLOT: u8 = 0;

/// One entry in the registry. Matches the shape the future
/// shader uniform expects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AnimSlot {
    /// Number of frames in the strip. Always `>= 2` for animated
    /// tiles (static tiles never enter the registry).
    pub(crate) frame_count: u32,
    /// Milliseconds per frame. Always `> 0`.
    pub(crate) frametime_ms: u32,
    /// V extent of frame 0 in the atlas: `atlas_uv[3] - atlas_uv[1]`.
    /// The shader adds `frame * v_step` to `v0` + `v1` of every
    /// sampled UV to walk through the strip.
    pub(crate) v_step: f32,
    /// V offset of frame 0 inside the atlas. The shader uses
    /// `(v0_base, v_step)` to compute the active frame's V rect.
    pub(crate) v0_base: f32,
}

impl AnimSlot {
    /// Compute the active frame's V offset from the static-frame
    /// V0. Mirrors the shader's planned `(time_ms / frametime_ms)
    /// % frame_count` branch.
    pub(crate) fn active_v_offset(&self, time_ms: u32) -> f32 {
        if self.frame_count == 0 || self.frametime_ms == 0 {
            return 0.0;
        }
        let frame = (time_ms / self.frametime_ms) % self.frame_count;
        frame as f32 * self.v_step
    }

    /// Active frame's full UV rect when the static rect is
    /// `(u0, v0_base, u1, v0_base + v_step)`. Returns the shifted
    /// rect for time `time_ms`.
    pub(crate) fn active_uv(&self, u0: f32, u1: f32, time_ms: u32) -> [f32; 4] {
        let off = self.active_v_offset(time_ms);
        [u0, self.v0_base + off, u1, self.v0_base + off + self.v_step]
    }
}

/// Mutable registry. Built once per resource-pack load + reset on
/// pack reload. Each `register_animated` call returns a stable
/// slot id; subsequent calls with the same key (atlas v0/v1 +
/// frame count) deduplicate.
#[derive(Debug, Default)]
pub(crate) struct AnimRegistry {
    /// Slot 0 is the static sentinel - entries[0] is never read.
    /// Indices 1..=len are the live slots.
    entries: Vec<AnimSlot>,
    /// Reverse lookup: hash-friendly key -> slot id. The key is
    /// `(v0_base * 1e6, v_step * 1e6, frame_count, frametime_ms)`
    /// rounded to fixed-point so f32 comparisons are stable.
    by_key: HashMap<AnimKey, u8>,
    /// Capacity hint - bumped when registration would overflow
    /// `MAX_ANIM_SLOTS` so the warn log fires once.
    overflow_logged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AnimKey {
    v0_units: i64,
    vstep_units: i64,
    frame_count: u32,
    frametime_ms: u32,
}

impl AnimRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Clear every slot. Called when the resource pack reloads
    /// (the atlas changes -> every UV rebases).
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.by_key.clear();
        self.overflow_logged = false;
    }

    /// Register an animated texture + return its slot id. Returns
    /// `STATIC_SLOT` for static textures (`frames <= 1` or
    /// `frametime_ms == 0`) so callers can use the same code path
    /// for static + animated tiles. Returns `STATIC_SLOT` (with a
    /// one-time warning) if the registry has reached
    /// `MAX_ANIM_SLOTS`.
    pub(crate) fn register(&mut self, tex: &TextureRef) -> u8 {
        if tex.frames <= 1 || tex.frametime_ms == 0 {
            return STATIC_SLOT;
        }
        let v_step = tex.atlas_uv[3] - tex.atlas_uv[1];
        if v_step <= 0.0 {
            return STATIC_SLOT;
        }
        let v0_base = tex.atlas_uv[1];
        let key = AnimKey {
            v0_units: (v0_base as f64 * 1_000_000.0).round() as i64,
            vstep_units: (v_step as f64 * 1_000_000.0).round() as i64,
            frame_count: tex.frames,
            frametime_ms: tex.frametime_ms,
        };
        if let Some(slot) = self.by_key.get(&key) {
            return *slot;
        }
        // Slot ids are 1-based; entries[0] would be slot 1.
        let next_slot = self.entries.len() as u32 + 1;
        if next_slot as usize > MAX_ANIM_SLOTS {
            if !self.overflow_logged {
                tracing::warn!(
                    cap = MAX_ANIM_SLOTS,
                    "anim_registry cap reached - extra animated tiles will render static",
                );
                self.overflow_logged = true;
            }
            return STATIC_SLOT;
        }
        self.entries.push(AnimSlot {
            frame_count: tex.frames,
            frametime_ms: tex.frametime_ms,
            v_step,
            v0_base,
        });
        let slot = next_slot as u8;
        self.by_key.insert(key, slot);
        slot
    }

    /// Look up the slot data by id. Slot 0 returns `None`
    /// (the static sentinel has no associated entry).
    pub(crate) fn slot(&self, slot: u8) -> Option<&AnimSlot> {
        if slot == STATIC_SLOT {
            return None;
        }
        self.entries.get((slot - 1) as usize)
    }

    /// Number of live slots (excludes the static sentinel).
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no animated tiles have been registered.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// View into every registered slot, slot id 1 first. The future
    /// GPU uploader walks this to fill the shader uniform array.
    pub(crate) fn entries(&self) -> &[AnimSlot] {
        &self.entries
    }
}

/// CPU-side helper: shift `static_uv` (the frame-0 UV rect the
/// mesher wrote into vertex buffers) by the active frame's V
/// offset at `time_ms`. Returns `static_uv` unchanged for
/// `STATIC_SLOT`. Used by the W9.x.4 fallback path until the
/// shader integration lands.
pub(crate) fn shifted_uv(
    registry: &AnimRegistry,
    slot: u8,
    static_uv: [f32; 4],
    time_ms: u32,
) -> [f32; 4] {
    if slot == STATIC_SLOT {
        return static_uv;
    }
    let Some(s) = registry.slot(slot) else {
        return static_uv;
    };
    let off = s.active_v_offset(time_ms);
    [
        static_uv[0],
        static_uv[1] + off,
        static_uv[2],
        static_uv[3] + off,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animated_ref(v0: f32, v_step: f32, frames: u32, ms: u32) -> TextureRef {
        TextureRef {
            atlas_uv: [0.0, v0, 1.0, v0 + v_step],
            frames,
            frametime_ms: ms,
            interpolate: false,
        }
    }

    #[test]
    fn static_block_has_anim_id_zero() {
        let mut reg = AnimRegistry::new();
        let tex = TextureRef::placeholder([0.0, 0.0, 1.0, 1.0]);
        assert_eq!(reg.register(&tex), STATIC_SLOT);
        assert!(reg.is_empty());
    }

    #[test]
    fn animated_tile_gets_slot_one_first() {
        let mut reg = AnimRegistry::new();
        let tex = animated_ref(0.0, 0.0625, 4, 100);
        let slot = reg.register(&tex);
        assert_eq!(slot, 1);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn duplicate_register_returns_same_slot() {
        let mut reg = AnimRegistry::new();
        let tex = animated_ref(0.0, 0.0625, 4, 100);
        let a = reg.register(&tex);
        let b = reg.register(&tex);
        assert_eq!(a, b);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn distinct_tiles_get_distinct_slots() {
        let mut reg = AnimRegistry::new();
        let water = animated_ref(0.0, 0.0625, 32, 100);
        let lava = animated_ref(0.5, 0.0625, 20, 60);
        let fire = animated_ref(0.25, 0.0625, 30, 70);
        let s1 = reg.register(&water);
        let s2 = reg.register(&lava);
        let s3 = reg.register(&fire);
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);
    }

    #[test]
    fn tile_anim_registry_caps_at_max() {
        let mut reg = AnimRegistry::new();
        // Register MAX_ANIM_SLOTS distinct animated tiles - all
        // succeed. W320: the cap is 128 so the full MC pack roster
        // (~102 strips) fits without overflow.
        for i in 0..MAX_ANIM_SLOTS as u32 {
            let tex = animated_ref(i as f32 / 1000.0, 0.0625, 4, 100);
            assert_ne!(reg.register(&tex), STATIC_SLOT, "slot {i}");
        }
        assert_eq!(reg.len(), MAX_ANIM_SLOTS);
        // One past the cap falls back to STATIC_SLOT.
        let tex = animated_ref(99.0, 0.0625, 4, 100);
        assert_eq!(reg.register(&tex), STATIC_SLOT);
        // The cap holds.
        assert_eq!(reg.len(), MAX_ANIM_SLOTS);
    }

    #[test]
    fn vshader_uv_advances_with_time() {
        // CPU-side simulation of the planned shader math:
        // frame = (t / frametime) % frames; v_off = frame * v_step.
        let slot = AnimSlot {
            frame_count: 4,
            frametime_ms: 100,
            v_step: 0.0625,
            v0_base: 0.0,
        };
        // t = 0 -> frame 0 -> v_off = 0.
        assert!((slot.active_v_offset(0) - 0.0).abs() < 1e-6);
        // t = 100 -> frame 1 -> v_off = 0.0625.
        assert!((slot.active_v_offset(100) - 0.0625).abs() < 1e-6);
        // t = 250 -> frame 2 -> v_off = 0.125.
        assert!((slot.active_v_offset(250) - 0.125).abs() < 1e-6);
        // t = 400 -> wraps: frame 0 again.
        assert!((slot.active_v_offset(400) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn active_uv_rect_walks_through_strip() {
        let slot = AnimSlot {
            frame_count: 4,
            frametime_ms: 100,
            v_step: 0.0625,
            v0_base: 0.0,
        };
        let uv0 = slot.active_uv(0.0, 1.0, 0);
        assert_eq!(uv0, [0.0, 0.0, 1.0, 0.0625]);
        let uv1 = slot.active_uv(0.0, 1.0, 100);
        assert_eq!(uv1, [0.0, 0.0625, 1.0, 0.125]);
        // Last frame.
        let uv3 = slot.active_uv(0.0, 1.0, 300);
        assert_eq!(uv3, [0.0, 0.1875, 1.0, 0.25]);
    }

    #[test]
    fn clear_resets_registry() {
        let mut reg = AnimRegistry::new();
        reg.register(&animated_ref(0.0, 0.0625, 4, 100));
        reg.register(&animated_ref(0.5, 0.0625, 4, 100));
        assert_eq!(reg.len(), 2);
        reg.clear();
        assert!(reg.is_empty());
        // After clear, the next register starts at slot 1 again.
        let s = reg.register(&animated_ref(0.0, 0.0625, 4, 100));
        assert_eq!(s, 1);
    }

    #[test]
    fn shifted_uv_returns_input_for_static_slot() {
        let reg = AnimRegistry::new();
        let r = shifted_uv(&reg, STATIC_SLOT, [0.0, 0.0, 1.0, 1.0], 1234);
        assert_eq!(r, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn shifted_uv_walks_active_frame() {
        let mut reg = AnimRegistry::new();
        let tex = animated_ref(0.0, 0.0625, 4, 100);
        let slot = reg.register(&tex);
        let r = shifted_uv(&reg, slot, [0.0, 0.0, 1.0, 0.0625], 100);
        assert_eq!(r, [0.0, 0.0625, 1.0, 0.125]);
    }

    #[test]
    fn shifted_uv_safe_for_unknown_slot() {
        let reg = AnimRegistry::new();
        // No slots registered; asking for slot 5 falls through.
        let r = shifted_uv(&reg, 5, [0.0, 0.0, 1.0, 1.0], 100);
        assert_eq!(r, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn slot_lookup_returns_none_for_static() {
        let reg = AnimRegistry::new();
        assert!(reg.slot(STATIC_SLOT).is_none());
    }

    #[test]
    fn zero_v_step_falls_back_to_static_slot() {
        let mut reg = AnimRegistry::new();
        // Animated but the atlas v range is zero (degenerate).
        let tex = TextureRef {
            atlas_uv: [0.0, 0.5, 1.0, 0.5],
            frames: 4,
            frametime_ms: 100,
            interpolate: false,
        };
        assert_eq!(reg.register(&tex), STATIC_SLOT);
    }

    #[test]
    fn entries_view_matches_registered_order() {
        let mut reg = AnimRegistry::new();
        let s1 = reg.register(&animated_ref(0.0, 0.0625, 4, 100));
        let s2 = reg.register(&animated_ref(0.5, 0.125, 8, 50));
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        let entries = reg.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].frame_count, 4);
        assert_eq!(entries[1].frame_count, 8);
        assert_eq!(entries[1].frametime_ms, 50);
    }

    #[test]
    fn frame_count_zero_safe() {
        // Defensive: a malformed slot with frame_count = 0 must
        // not divide-by-zero in active_v_offset.
        let s = AnimSlot {
            frame_count: 0,
            frametime_ms: 100,
            v_step: 0.0625,
            v0_base: 0.0,
        };
        assert_eq!(s.active_v_offset(1234), 0.0);
    }

    #[test]
    fn frametime_zero_safe() {
        let s = AnimSlot {
            frame_count: 4,
            frametime_ms: 0,
            v_step: 0.0625,
            v0_base: 0.0,
        };
        assert_eq!(s.active_v_offset(1234), 0.0);
    }
}
