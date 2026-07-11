//! Per-tile animation uniform for `mesh3d.wgsl`.
//!
//! Wave 9.y wires animated MC tiles (water_still, lava_still,
//! fire_0, ...) through to the fragment shader so they walk
//! through their frame strip without re-uploading vertex buffers.
//! The data layer landed in W9.x as `resource_pack::anim_registry`;
//! this module exposes the GPU side - a 32-entry uniform array
//! the shader reads when a vertex's `anim_id != 0`.
//!
//! Layout: 32 entries, each 16 bytes. Total uniform = 512 bytes,
//! comfortably under the 64KiB minimum guarantee + small enough
//! to upload from a `bytemuck` slice every frame.
//!
//! Per-entry fields mirror the CPU-side `AnimSlot`:
//! - `frame_count` - frames in the strip (>= 2 for animated tiles).
//! - `frametime_ms` - ms per frame (> 0).
//! - `v_step` - V extent of one frame in atlas units.
//! - `_pad` - filler to round the entry to 16 bytes (WGSL
//!   requires 16-byte alignment inside uniform arrays of small
//!   structs).
//!
//! The shader reads:
//! ```wgsl
//! let anim = tile_anims.entries[anim_id - 1u];
//! let frame = u32(floor(time_ms / max(f32(anim.frametime_ms), 1.0)))
//!             % anim.frame_count;
//! uv.v += f32(frame) * anim.v_step;
//! ```
//! `anim_id` is 1-based (slot 0 means "static, do not scroll").

use bytemuck::{Pod, Zeroable};

/// Hard cap on simultaneously-animated tiles in flight. Matches
/// `resource_pack::anim_registry::MAX_ANIM_SLOTS` - bump both
/// values together if you raise it.
pub const MAX_ANIM_SLOTS: usize = 32;

/// One animated-tile descriptor as the shader sees it. 16 bytes,
/// `repr(C)` so the binary layout matches WGSL's struct rules.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug, PartialEq)]
pub struct TileAnimEntry {
    pub frame_count: u32,
    pub frametime_ms: u32,
    pub v_step: f32,
    /// Filler to round the entry to 16 bytes. WGSL packs
    /// `array<TileAnim, N>` with each element padded up to 16
    /// bytes, so this keeps the Rust + WGSL layouts in sync.
    pub _pad: f32,
}

impl Default for TileAnimEntry {
    fn default() -> Self {
        Self {
            frame_count: 0,
            frametime_ms: 0,
            v_step: 0.0,
            _pad: 0.0,
        }
    }
}

/// Full 32-entry uniform body. Always 512 bytes. The renderer
/// uploads one of these per frame; unused slots stay zeroed so
/// the shader's `frame_count <= 1` branch returns the static UV
/// unchanged.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct TileAnimsUniform {
    pub entries: [TileAnimEntry; MAX_ANIM_SLOTS],
}

impl Default for TileAnimsUniform {
    fn default() -> Self {
        Self {
            entries: [TileAnimEntry::default(); MAX_ANIM_SLOTS],
        }
    }
}

impl TileAnimsUniform {
    /// Stamp slot `slot_id` (1-based) with the given entry. Slot
    /// ids outside `1..=MAX_ANIM_SLOTS` are dropped silently so
    /// caller-side overflow doesn't panic. Returns `true` if the
    /// stamp landed.
    pub fn set(&mut self, slot_id: u32, entry: TileAnimEntry) -> bool {
        if slot_id == 0 || slot_id as usize > MAX_ANIM_SLOTS {
            return false;
        }
        self.entries[slot_id as usize - 1] = entry;
        true
    }

    /// Build a uniform body from a sequential list of slot
    /// entries. The first entry lands in slot 1, the second in
    /// slot 2, and so on - matches how `AnimRegistry::entries()`
    /// hands slots out. Extra entries beyond `MAX_ANIM_SLOTS`
    /// are dropped (the registry already caps itself, this is a
    /// defensive guard).
    pub fn from_sequential(entries: &[TileAnimEntry]) -> Self {
        let mut out = Self::default();
        for (i, e) in entries.iter().take(MAX_ANIM_SLOTS).enumerate() {
            out.entries[i] = *e;
        }
        out
    }
}

/// W240: per-BLOCK draw parameters, indexed by the block id packed
/// into a chunk vertex's `anim_id` high bits. Lets a whole chunk
/// draw as ONE command per render layer instead of one per block
/// type (the VD-32 wall was the draw-command count). Entry layout
/// mirrors WGSL: color (tint * texture, alpha included) + material
/// (metallic, roughness, emissive, unused).
pub const MAX_BLOCK_PARAMS: usize = 1024;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug, PartialEq)]
pub struct BlockParamEntry {
    pub color: [f32; 4],
    /// (metallic, roughness, emissive, _unused)
    pub material: [f32; 4],
}

impl Default for BlockParamEntry {
    fn default() -> Self {
        // Identity: white tint, dielectric mid-roughness, no glow.
        Self {
            color: [1.0, 1.0, 1.0, 1.0],
            material: [0.0, 0.5, 0.0, 0.0],
        }
    }
}

/// GPU-side resource: uniform buffers + bind group layout + bind
/// group. Bound at `@group(4)` in `mesh3d.wgsl`:
///   binding(0) = tile anims, binding(1) = block params (W240).
pub(crate) struct TileAnimsResource {
    pub buffer: wgpu::Buffer,
    pub block_params_buffer: wgpu::Buffer,
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

impl TileAnimsResource {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::util::DeviceExt;
        let initial = TileAnimsUniform::default();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("infengine_render::tile_anims_uniform"),
            contents: bytemuck::bytes_of(&initial),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let block_params_initial =
            [BlockParamEntry::default(); MAX_BLOCK_PARAMS];
        let block_params_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("infengine_render::block_params_uniform"),
                contents: bytemuck::cast_slice(&block_params_initial),
                // W240: STORAGE, not UNIFORM - the 32 KB table exceeds
                // the 16 KB max_uniform_buffer_binding_size default.
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::tile_anims_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::tile_anims_bg"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: block_params_buffer.as_entire_binding(),
                },
            ],
        });
        Self {
            buffer,
            block_params_buffer,
            layout,
            bind_group,
        }
    }

    /// Upload a fresh uniform body. Caller fills the entries
    /// table from its `AnimRegistry` snapshot then hands the
    /// uniform here on each frame (or only when slots change).
    pub fn upload(&self, queue: &wgpu::Queue, uniform: &TileAnimsUniform) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(uniform));
    }

    /// W240: upload the per-block-id draw parameter table (once at
    /// startup; ids beyond the slice keep the identity default).
    pub fn upload_block_params(&self, queue: &wgpu::Queue, entries: &[BlockParamEntry]) {
        let mut table = [BlockParamEntry::default(); MAX_BLOCK_PARAMS];
        for (i, e) in entries.iter().take(MAX_BLOCK_PARAMS).enumerate() {
            table[i] = *e;
        }
        queue.write_buffer(&self.block_params_buffer, 0, bytemuck::cast_slice(&table));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_layout_is_16_bytes() {
        // Critical: WGSL's `array<TileAnim, 32>` requires the
        // element stride to be 16 bytes. If a field shifts, the
        // GPU reads the wrong bytes for every slot.
        assert_eq!(std::mem::size_of::<TileAnimEntry>(), 16);
    }

    #[test]
    fn uniform_total_is_512_bytes() {
        // 32 slots * 16 bytes = 512 bytes. The renderer's bind
        // group expects this exact size.
        assert_eq!(std::mem::size_of::<TileAnimsUniform>(), 512);
    }

    #[test]
    fn default_uniform_has_zeroed_slots() {
        let u = TileAnimsUniform::default();
        for e in u.entries.iter() {
            assert_eq!(e.frame_count, 0);
            assert_eq!(e.frametime_ms, 0);
            assert_eq!(e.v_step, 0.0);
        }
    }

    #[test]
    fn set_stamps_correct_slot() {
        let mut u = TileAnimsUniform::default();
        let e = TileAnimEntry {
            frame_count: 8,
            frametime_ms: 100,
            v_step: 0.0625,
            _pad: 0.0,
        };
        assert!(u.set(1, e));
        assert_eq!(u.entries[0], e);
        // slot 2 lands in index 1.
        assert!(u.set(2, e));
        assert_eq!(u.entries[1], e);
    }

    #[test]
    fn set_drops_slot_zero_and_overflow() {
        let mut u = TileAnimsUniform::default();
        let e = TileAnimEntry {
            frame_count: 8,
            frametime_ms: 100,
            v_step: 0.0625,
            _pad: 0.0,
        };
        // Slot 0 is reserved for "static" and must never be
        // written by the renderer.
        assert!(!u.set(0, e));
        // Slots beyond MAX_ANIM_SLOTS are silently dropped.
        assert!(!u.set((MAX_ANIM_SLOTS as u32) + 1, e));
        assert!(!u.set(99, e));
        // Boundary: slot == MAX_ANIM_SLOTS lands.
        assert!(u.set(MAX_ANIM_SLOTS as u32, e));
        assert_eq!(u.entries[MAX_ANIM_SLOTS - 1], e);
    }

    #[test]
    fn entry_is_pod_and_zeroable() {
        // The Pod + Zeroable derives gate bytemuck::cast_slice
        // safety; the test just exercises the conversion to make
        // sure the derives stay applied.
        let entries = [TileAnimEntry::default(); MAX_ANIM_SLOTS];
        let bytes: &[u8] = bytemuck::cast_slice(&entries);
        assert_eq!(bytes.len(), 512);
    }

    #[test]
    fn from_sequential_aligns_with_one_based_slots() {
        // `AnimRegistry::entries()[0]` is slot 1; `entries[1]` is
        // slot 2. The uniform builder must preserve that mapping
        // so the shader's `entries[anim_id - 1u]` lookup picks
        // the right row.
        let in_a = TileAnimEntry {
            frame_count: 32,
            frametime_ms: 100,
            v_step: 0.03125,
            _pad: 0.0,
        };
        let in_b = TileAnimEntry {
            frame_count: 20,
            frametime_ms: 60,
            v_step: 0.0625,
            _pad: 0.0,
        };
        let u = TileAnimsUniform::from_sequential(&[in_a, in_b]);
        assert_eq!(u.entries[0], in_a);
        assert_eq!(u.entries[1], in_b);
        // Remaining slots stay zeroed.
        assert_eq!(u.entries[2], TileAnimEntry::default());
        assert_eq!(u.entries[MAX_ANIM_SLOTS - 1], TileAnimEntry::default());
    }

    #[test]
    fn from_sequential_caps_at_max_slots() {
        // Defensive: feeding more than 32 entries silently drops
        // the overflow instead of panicking.
        let entry = TileAnimEntry {
            frame_count: 4,
            frametime_ms: 100,
            v_step: 0.0625,
            _pad: 0.0,
        };
        let entries = vec![entry; MAX_ANIM_SLOTS + 5];
        let u = TileAnimsUniform::from_sequential(&entries);
        // All 32 slots stamped; the extra 5 are dropped.
        for i in 0..MAX_ANIM_SLOTS {
            assert_eq!(u.entries[i], entry);
        }
    }
}
