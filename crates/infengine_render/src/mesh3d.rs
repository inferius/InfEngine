//! 3D mesh pipeline.
//!
//! Phase 6.2 generalizes the cube-only pipeline into a registry of
//! user-defined meshes. A `Mesh3DRegistry` owns vertex / index buffers
//! per mesh and a numeric `MeshHandle` keys back to them. Draws are
//! still batched: callers submit `(MeshHandle, MeshInstance)` pairs
//! and the renderer groups them by handle to issue one instanced draw
//! per mesh per frame.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Vertex format used by `Mesh3DPass`. W30.1: extended to 40 bytes for
/// shader-side tile fract support enabling greedy meshing.
///
/// Layout (40 bytes, all 4-byte aligned for wgpu):
///
/// - `position`   `[f32; 3]`  offset  0  (12 B)  Float32x3 @ loc 0
/// - `normal_pad` `[i8; 4]`   offset 12  ( 4 B)  Sint8x4   @ loc 1 (first 3 = normal*127, 4th = 0)
/// - `uv_packed`  `[u16; 2]`  offset 16  ( 4 B)  Uint16x2  @ loc 2 (component / 65535.0) legacy
/// - `light_pad`  `[u8; 4]`   offset 20  ( 4 B)  Unorm8x4  @ loc 9 ([0]=block [1]=sky)
/// - `anim_id`    `u32`       offset 24  ( 4 B)  Uint32    @ loc 10
/// - `tile_uv0`   `[u16; 2]`  offset 28  ( 4 B)  Uint16x2  @ loc 11 (atlas u0/v0 / 65535.0)
/// - `tile_size`  `[u16; 2]`  offset 32  ( 4 B)  Uint16x2  @ loc 12 (atlas tile w/h / 65535.0)
/// - `quad_uv`    `[u16; 2]`  offset 36  ( 4 B)  Uint16x2  @ loc 13 (per-voxel UV in [0,16] packed as val/16*65535)
/// - `_pad`       `[u16; 1]`  alias inside quad_uv (struct is [u16;2] = 4 B total, see above)
///
/// Wait - quad_uv is 4 bytes ([u16;2]) so offsets are:
/// 28: tile_uv0 [u16;2] = 4 B
/// 32: tile_size [u16;2] = 4 B
/// 36: quad_uv [u16;2] = 4 B
/// Total: 40 bytes.
///
/// Greedy-meshing quads set `tile_uv0`/`tile_size` to the atlas tile bounds
/// and `quad_uv` to per-corner coords in [0, w] x [0, h]. The shader
/// computes `tile_uv0 + fract(quad_uv) * tile_size` which tiles the texture
/// correctly without sampling neighbouring tiles.
///
/// Per-voxel (non-greedy) quads set `quad_uv` to [0, 1] range UVs and
/// `tile_uv0 = uv_packed` position, `tile_size` = tile extents, so
/// `fract(quad_uv) == quad_uv` and the shader produces the same result.
///
/// Public API (`.new` / `.with_uv` / `.with_light` / `.with_anim_id` /
/// `.with_tile_uv`) unchanged for non-greedy paths.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct MeshVertex {
    /// World-space position.
    pub position: [f32; 3],
    /// Normal components packed as i8 * 127; 4th byte = padding 0.
    pub normal_pad: [i8; 4],
    /// UV coordinates packed as u16 = component * 65535. Legacy field;
    /// the shader now uses tile_uv0/tile_size/quad_uv for final UV.
    pub uv_packed: [u16; 2],
    /// Light values packed as unorm8: [block_light, sky_light, ao, 0].
    /// W122: byte 2 carries vertex ambient-occlusion (1.0 = no AO /
    /// full bright, lower = darker contact corner). Defaults to 255
    /// so meshes that never set it render unchanged.
    pub light_pad: [u8; 4],
    /// Animation slot id (0 = static, 1..=32 = animated tile).
    pub anim_id: u32,
    /// W30.1: Atlas tile origin packed as u16 = component * 65535.
    pub tile_uv0: [u16; 2],
    /// W30.1: Atlas tile size (u1-u0, v1-v0) packed as u16 = component * 65535.
    pub tile_size: [u16; 2],
    /// W30.1: Per-corner UV in [0, w] x [0, h] voxel coords.
    /// Packed as u16 = (component / 16.0) * 65535.0 to support range [0, 16].
    pub quad_uv: [u16; 2],
}

impl MeshVertex {
    /// Pack a float normal component to i8 range.
    #[inline]
    fn pack_normal_comp(n: f32) -> i8 {
        (n.clamp(-1.0, 1.0) * 127.0) as i8
    }

    pub fn new(position: [f32; 3], normal: [f32; 3]) -> Self {
        Self {
            position,
            normal_pad: [
                Self::pack_normal_comp(normal[0]),
                Self::pack_normal_comp(normal[1]),
                Self::pack_normal_comp(normal[2]),
                0,
            ],
            uv_packed: [0, 0],
            // Default: no block light (0), full sky (255 = 1.0),
            // no AO (255 = 1.0 = full bright). W122: ao defaults to
            // 255 so meshes that never call `.with_ao` are unchanged.
            light_pad: [0, 255, 255, 0],
            anim_id: 0,
            // W30.1: default tile attrs - identity tiling (single voxel quad).
            // tile_uv0=0, tile_size=65535 (= 1.0 in [0,1]), quad_uv=(0,0).
            // The shader produces uv = 0 + fract(0) * 1 = 0 which is
            // safe but invisible - callers must set with_uv or with_tile_uv.
            tile_uv0: [0, 0],
            tile_size: [65535, 65535],
            quad_uv: [0, 0],
        }
    }

    pub fn with_uv(mut self, uv: [f32; 2]) -> Self {
        // W229.1: ROUND, don't truncate. Truncation biased every
        // packed UV up to 1/65535 BELOW the intended coordinate, which
        // on tile edges sampled the neighbouring atlas tile - the
        // bright pinstripe seams on block boundaries.
        self.uv_packed = [
            (uv[0].clamp(0.0, 1.0) * 65535.0).round() as u16,
            (uv[1].clamp(0.0, 1.0) * 65535.0).round() as u16,
        ];
        // W30.1: keep tile attrs consistent with a per-voxel quad that
        // samples exactly `uv`. tile_uv0 = uv, tile_size = (0,0) means
        // the shader always outputs tile_uv0 + fract(quad_uv)*0 = tile_uv0 = uv.
        // quad_uv stays (0,0).
        self.tile_uv0 = self.uv_packed;
        self.tile_size = [0, 0];
        self.quad_uv = [0, 0];
        self
    }

    /// W30.1: Set tile atlas bounds + per-corner UV for greedy-merged quads.
    ///
    /// `tile_uv0` - atlas origin `[u0, v0]` in `[0, 1]` space.
    /// `tile_size` - atlas tile extents `[u1-u0, v1-v0]` in `[0, 1]` space.
    /// `quad_uv`  - per-corner UV in `[0, w] x [0, h]` voxel coords
    ///              (w/h can exceed 1.0 for merged quads, max = CHUNK_SIZE = 16).
    ///
    /// The shader computes `final_uv = tile_uv0 + fract(quad_uv) * tile_size`
    /// which keeps sampling within `[tile_uv0, tile_uv0 + tile_size]` regardless
    /// of how large the merged quad is.
    pub fn with_tile_uv(mut self, tile_uv0: [f32; 2], tile_size: [f32; 2], quad_uv: [f32; 2]) -> Self {
        // W229.1: ROUND, don't truncate (see with_uv) - a truncated
        // tile_uv0 landed below the tile's edge and bled the
        // neighbouring atlas tile along quad borders.
        self.tile_uv0 = [
            (tile_uv0[0].clamp(0.0, 1.0) * 65535.0).round() as u16,
            (tile_uv0[1].clamp(0.0, 1.0) * 65535.0).round() as u16,
        ];
        self.tile_size = [
            (tile_size[0].clamp(0.0, 1.0) * 65535.0).round() as u16,
            (tile_size[1].clamp(0.0, 1.0) * 65535.0).round() as u16,
        ];
        // quad_uv range is [0, 16] (max CHUNK_SIZE voxels). Pack into u16
        // by dividing by 16 first so [0,16] maps to [0, 65535].
        const QUAD_UV_SCALE: f32 = 16.0;
        self.quad_uv = [
            (quad_uv[0].clamp(0.0, QUAD_UV_SCALE) / QUAD_UV_SCALE * 65535.0).round() as u16,
            (quad_uv[1].clamp(0.0, QUAD_UV_SCALE) / QUAD_UV_SCALE * 65535.0).round() as u16,
        ];
        self
    }

    /// Override the per-vertex `(block, sky)` light values. Inputs
    /// are clamped to `[0, 1]`. The shader takes `max(block, sky *
    /// day_factor)` and folds it into the final color.
    pub fn with_light(mut self, block: f32, sky: f32) -> Self {
        self.light_pad[0] = (block.clamp(0.0, 1.0) * 255.0) as u8;
        self.light_pad[1] = (sky.clamp(0.0, 1.0) * 255.0) as u8;
        self
    }

    /// W122: Override the per-vertex ambient-occlusion factor. `1.0`
    /// means no occlusion (full bright); lower values darken the
    /// corner (MC's contact shading in concave corners). The shader
    /// multiplies the final light by this. Clamped to `[0, 1]`.
    pub fn with_ao(mut self, ao: f32) -> Self {
        self.light_pad[2] = (ao.clamp(0.0, 1.0) * 255.0) as u8;
        self
    }

    /// W199: Override the per-vertex water-column depth (light byte 3,
    /// otherwise unused). Normalized to `[0, 1]` over the depth that
    /// reads fully opaque (~12 blocks). The water shader uses it to
    /// fade the surface from translucent (shallow) to opaque (deep) so
    /// deep ocean isn't see-through to the void. No effect on non-water
    /// meshes (the shader only reads it when `material.w >= 0.5`).
    pub fn with_water_depth(mut self, depth: f32) -> Self {
        self.light_pad[3] = (depth.clamp(0.0, 1.0) * 255.0) as u8;
        self
    }

    /// Wave 9.y: tag this vertex with an animation slot id. `0`
    /// means static (no UV scroll); `1..=32` references an entry
    /// in the shader's `tile_anim` uniform array. Slots outside
    /// `0..=32` are clamped because the uniform array is statically
    /// sized.
    pub fn with_anim_id(mut self, slot: u32) -> Self {
        self.anim_id = slot.min(32);
        self
    }

    // W28.6 / W30.1: decode helpers for tests and debug inspection.

    /// Decode normal back to f32 components.
    #[inline]
    pub fn normal(&self) -> [f32; 3] {
        [
            self.normal_pad[0] as f32 / 127.0,
            self.normal_pad[1] as f32 / 127.0,
            self.normal_pad[2] as f32 / 127.0,
        ]
    }

    /// Decode UV back to f32 components.
    #[inline]
    pub fn uv(&self) -> [f32; 2] {
        [
            self.uv_packed[0] as f32 / 65535.0,
            self.uv_packed[1] as f32 / 65535.0,
        ]
    }

    /// Decode (block_light, sky_light) back to f32 components.
    #[inline]
    pub fn light(&self) -> [f32; 2] {
        [
            self.light_pad[0] as f32 / 255.0,
            self.light_pad[1] as f32 / 255.0,
        ]
    }

    /// W122: Decode the ambient-occlusion factor back to f32 `[0, 1]`.
    #[inline]
    pub fn ao(&self) -> f32 {
        self.light_pad[2] as f32 / 255.0
    }

    /// W30.1: Decode tile_uv0 back to f32 [u0, v0].
    #[inline]
    pub fn tile_uv0_f32(&self) -> [f32; 2] {
        [
            self.tile_uv0[0] as f32 / 65535.0,
            self.tile_uv0[1] as f32 / 65535.0,
        ]
    }

    /// W30.1: Decode tile_size back to f32 [dw, dh].
    #[inline]
    pub fn tile_size_f32(&self) -> [f32; 2] {
        [
            self.tile_size[0] as f32 / 65535.0,
            self.tile_size[1] as f32 / 65535.0,
        ]
    }

    /// W30.1: Decode quad_uv back to f32 [u, v] in [0, 16] range.
    #[inline]
    pub fn quad_uv_f32(&self) -> [f32; 2] {
        [
            self.quad_uv[0] as f32 / 65535.0 * 16.0,
            self.quad_uv[1] as f32 / 65535.0 * 16.0,
        ]
    }
}

/// Opaque numeric handle for a registered mesh.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct MeshHandle(pub(crate) u32);

pub(crate) struct MeshEntry {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
}

#[derive(Default)]
pub(crate) struct Mesh3DRegistry {
    next_id: u32,
    entries: HashMap<MeshHandle, MeshEntry>,
    /// Per-mesh transparency flag. Mesh handles in this set route
    /// to the alpha-blended pipeline + skip depth writes so they
    /// composite over the opaque scene without occluding each
    /// other arbitrarily. Default = opaque.
    transparent: std::collections::HashSet<MeshHandle>,
}

impl Mesh3DRegistry {
    pub fn insert(
        &mut self,
        device: &wgpu::Device,
        label: &str,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> MeshHandle {
        // Pad empty mesh data with a single degenerate vertex /
        // index so the resulting buffer is at least 1 byte.
        // wgpu's `slice(..)` panics on zero-size buffers, and
        // callers that submit an empty submesh would crash the
        // shadow pass. Padding lets the entry exist but the
        // `index_count = 0` draw call below stays a no-op.
        let fallback_v: [MeshVertex; 1] = [MeshVertex::new([0.0; 3], [0.0; 3])];
        let fallback_i: [u32; 1] = [0];
        let (vsrc, isrc) = if vertices.is_empty() || indices.is_empty() {
            (
                bytemuck::cast_slice(&fallback_v[..]),
                bytemuck::cast_slice(&fallback_i[..]),
            )
        } else {
            (
                bytemuck::cast_slice(vertices),
                bytemuck::cast_slice(indices),
            )
        };
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: vsrc,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: isrc,
            usage: wgpu::BufferUsages::INDEX,
        });
        let handle = MeshHandle(self.next_id);
        self.next_id += 1;
        self.entries.insert(
            handle,
            MeshEntry {
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
            },
        );
        handle
    }

    pub fn get(&self, handle: MeshHandle) -> Option<&MeshEntry> {
        self.entries.get(&handle)
    }

    /// Drop the GPU buffers backing `handle`. Returns true if the
    /// handle was registered. Caller must guarantee no draw call
    /// references the handle in the current frame.
    pub fn remove(&mut self, handle: MeshHandle) -> bool {
        self.transparent.remove(&handle);
        self.entries.remove(&handle).is_some()
    }

    /// Flag a registered mesh as transparent (or back to opaque).
    /// Transparent meshes draw via the alpha-blended pipeline in a
    /// second sub-pass so they composite over the opaque scene.
    pub fn set_transparent(&mut self, handle: MeshHandle, transparent: bool) {
        if transparent {
            self.transparent.insert(handle);
        } else {
            self.transparent.remove(&handle);
        }
    }

    pub fn is_transparent(&self, handle: MeshHandle) -> bool {
        self.transparent.contains(&handle)
    }
}

/// One mesh draw instance.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct MeshInstance {
    /// Column-major 4x4 model matrix (matches `glam::Mat4::to_cols_array`).
    pub model: [f32; 16],
    pub color: [f32; 4],
    /// PBR-ish material parameters: `(metallic, roughness, _pad, _pad)`.
    /// Both values are clamped to `[0, 1]` in the shader.
    pub material: [f32; 4],
}

impl MeshInstance {
    /// Default material: fully dielectric (metallic = 0), middling
    /// roughness (0.5).
    pub fn new(model: [f32; 16], color: [f32; 4]) -> Self {
        Self {
            model,
            color,
            material: [0.0, 0.5, 0.0, 0.0],
        }
    }

    pub fn with_material(mut self, metallic: f32, roughness: f32) -> Self {
        self.material = [
            metallic.clamp(0.0, 1.0),
            roughness.clamp(0.04, 1.0),
            0.0,
            0.0,
        ];
        self
    }

    /// W46.19: set emissive boost. Adds `strength * albedo` to the
    /// lit result so the mesh is visible even in pitch dark. Used
    /// for self-illuminated blocks (torches, glowstone, lava, lit
    /// redstone, ...) so MC's "lit-tip" glow reads at all times.
    /// `strength` is clamped to `[0, 4]` (0 = no emission, 1 = full
    /// albedo added on top of lighting, 2+ = HDR boost).
    pub fn with_emissive(mut self, strength: f32) -> Self {
        self.material[2] = strength.clamp(0.0, 4.0);
        self
    }

    /// Flag this instance as the water-surface mesh. The fragment
    /// shader switches to the animated-ripple branch (driven by
    /// `camera.time`) and reads `MeshVertex.uv` as a per-vertex
    /// flow direction instead of sampling the albedo texture.
    pub fn with_water_marker(mut self) -> Self {
        self.material[3] = 1.0;
        self
    }
}

/// Unit cube vertices with per-face normals (24 vertices, 36 indices).
/// Each face has its own vertex copies so flat shading works without
/// any extra logic.
pub fn cube_geometry_public() -> (Vec<MeshVertex>, Vec<u32>) {
    let (verts, idx) = cube_geometry();
    (verts, idx.into_iter().map(u32::from).collect())
}

fn cube_geometry() -> (Vec<MeshVertex>, Vec<u16>) {
    // (normal, four corners ccw when viewed from +normal direction)
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        // +X
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        // -X
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, -0.5, 0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        // +Y
        (
            [0.0, 1.0, 0.0],
            [
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        // -Y
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
            ],
        ),
        // +Z
        (
            [0.0, 0.0, 1.0],
            [
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
            ],
        ),
        // -Z
        (
            [0.0, 0.0, -1.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
    ];

    // Standard quad UVs: bl, br, tr, tl. Each face's corners are listed in
    // that order in the faces table.
    const FACE_UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, corners) in faces {
        let base = vertices.len() as u16;
        for (i, c) in corners.iter().enumerate() {
            vertices.push(
                MeshVertex::new(*c, normal)
                    .with_uv(FACE_UVS[i])
                    .with_light(0.0, 1.0),
            );
        }
        // Two triangles per face wound CCW when viewed from the
        // outside (the `normal` direction). With `FrontFace::Ccw` and
        // `cull_mode: Back` this keeps the outward faces visible while
        // hiding the interior - the previous winding had the cube's
        // outward faces culled, so the renderer was painting the back
        // of the opposite face instead and lighting flipped.
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    (vertices, indices)
}

const INITIAL_INSTANCE_CAPACITY: u64 = 256;

/// Sub-range of the instance buffer that belongs to one mesh handle.
pub(crate) struct MeshBatch {
    pub handle: MeshHandle,
    pub start: u32,
    pub count: u32,
}

/// Pipeline + shared instance buffer + albedo texture for batched mesh draws.
pub(crate) struct Mesh3DPass {
    pipeline: wgpu::RenderPipeline,
    /// Alpha-blended variant used by transparent meshes (e.g. water).
    /// Same shader + vertex layout but the colour target swaps to
    /// premultiplied alpha and depth-write is disabled so multiple
    /// translucent layers can stack without writing each other out
    /// of the depth buffer.
    pipeline_transparent: wgpu::RenderPipeline,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    /// W157: a SECOND instance buffer for the first-person viewmodel
    /// (held item). It lives in its own buffer so the held-item draw
    /// can run in a separate depth-cleared pass without clobbering the
    /// world-mesh instance data already uploaded this frame.
    vm_instance_buffer: wgpu::Buffer,
    vm_instance_capacity: u64,
    albedo_texture: wgpu::Texture,
    albedo_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    texture_bgl: wgpu::BindGroupLayout,
    texture_bind_group: wgpu::BindGroup,
}

fn create_albedo_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("infengine_render::mesh3d_albedo_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

impl Mesh3DPass {
    pub fn instance_buffer(&self) -> &wgpu::Buffer {
        &self.instance_buffer
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_layout: &wgpu::BindGroupLayout,
        shadow_sample_layout: &wgpu::BindGroupLayout,
        lights_layout: &wgpu::BindGroupLayout,
        tile_anims_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::mesh_instance_buf"),
            size: INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<MeshInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // W157: viewmodel instance buffer (held item; tiny - a couple
        // of instances at most). Starts at 8 and grows like the main.
        let vm_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::vm_instance_buf"),
            size: 8 * std::mem::size_of::<MeshInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("infengine_render::mesh3d_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh3d.wgsl").into()),
        });

        // Default 1x1 white albedo texture. Replaced on demand via
        // Renderer::set_albedo_texture.
        let albedo_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("infengine_render::mesh3d_default_albedo"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &albedo_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let albedo_view = albedo_texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Nearest sampling preserves the crisp pixel-art look of
        // voxel / Minecraft-style textures. Linear filtering would
        // blur 16x16 tiles into mush at any reasonable view
        // distance. Higher-res PBR meshes that need smooth filtering
        // can request a different sampler via a follow-up API.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("infengine_render::mesh3d_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::mesh3d_texture_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let texture_bind_group =
            create_albedo_bind_group(device, &texture_bgl, &albedo_view, &sampler);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("infengine_render::mesh3d_pipeline_layout"),
            bind_group_layouts: &[
                camera_layout,
                &texture_bgl,
                shadow_sample_layout,
                lights_layout,
                tile_anims_layout,
            ],
            push_constant_ranges: &[],
        });

        // W30.1: packed vertex layout (40 bytes, see MeshVertex).
        // loc 0: position  [f32;3] @ offset  0  Float32x3
        // loc 1: normal    [i8;4]  @ offset 12  Sint8x4
        // loc 2: uv        [u16;2] @ offset 16  Uint16x2
        // loc 9: light     [u8;4]  @ offset 20  Unorm8x4
        // loc10: anim_id   u32     @ offset 24  Uint32
        // loc11: tile_uv0  [u16;2] @ offset 28  Uint16x2  (atlas u0/v0)
        // loc12: tile_size [u16;2] @ offset 32  Uint16x2  (atlas dw/dh)
        // loc13: quad_uv   [u16;2] @ offset 36  Uint16x2  (per-corner [0,16])
        let vertex_attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Sint8x4,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Uint16x2,
            },
            wgpu::VertexAttribute {
                offset: 20,
                shader_location: 9,
                format: wgpu::VertexFormat::Unorm8x4,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 10,
                format: wgpu::VertexFormat::Uint32,
            },
            wgpu::VertexAttribute {
                offset: 28,
                shader_location: 11,
                format: wgpu::VertexFormat::Uint16x2,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 12,
                format: wgpu::VertexFormat::Uint16x2,
            },
            wgpu::VertexAttribute {
                offset: 36,
                shader_location: 13,
                format: wgpu::VertexFormat::Uint16x2,
            },
        ];

        let instance_attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 48,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 64,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 80,
                shader_location: 8,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("infengine_render::mesh3d_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vertex_attrs,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    },
                ],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Transparent pipeline: identical layout / shader, but the
        // colour target uses alpha blending and depth-write is off
        // so stacked translucent meshes don't punch each other out.
        // Cull is also disabled so a swimmer looking up sees the
        // water surface from below.
        let pipeline_transparent =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("infengine_render::mesh3d_pipeline_transparent"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<MeshVertex>()
                                as wgpu::BufferAddress,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &vertex_attrs,
                        },
                        wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<MeshInstance>()
                                as wgpu::BufferAddress,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &instance_attrs,
                        },
                    ],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    // Transparent pass: no backface cull. Water
                    // / liquid mesh quads in voxel_demo_mp use a
                    // mixed CCW/CW winding (push_quad_flow walks
                    // corners in source-position order, not
                    // consistently from the outward side), so
                    // enabling cull made the top face invisible
                    // when viewed from above + the outer side
                    // faces invisible from outside the pool. The
                    // "underwater grid of tiles" issue this
                    // briefly fixed is recurring; the real fix
                    // is to normalise the mesh winding in
                    // voxel_demo_mp, then re-enable cull here.
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        Self {
            pipeline,
            pipeline_transparent,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            vm_instance_buffer,
            vm_instance_capacity: 8,
            albedo_texture,
            albedo_view,
            sampler,
            texture_bgl,
            texture_bind_group,
        }
    }

    /// Build an albedo bind group from an in-memory RGBA buffer. The
    /// caller owns the returned bind group (typically stored in a
    /// `HashMap<MeshHandle, BindGroup>` by `Renderer`).
    pub fn make_albedo_bind_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> wgpu::BindGroup {
        assert!(
            rgba.len() as u32 >= width * height * 4,
            "albedo buffer too small for {width}x{height}"
        );
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("infengine_render::mesh3d_per_mesh_albedo"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Leak the texture into the bind group's resource handle by
        // keeping it alive inside the BG itself (wgpu retains the
        // texture as long as the bind group lives).
        create_albedo_bind_group(device, &self.texture_bgl, &view, &self.sampler)
    }

    /// Replace the albedo texture used by all 3D meshes. `rgba` must be
    /// `width * height * 4` bytes in `Rgba8UnormSrgb` order.
    pub fn set_albedo(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) {
        assert!(
            rgba.len() as u32 >= width * height * 4,
            "albedo buffer too small for {width}x{height}"
        );
        self.albedo_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("infengine_render::mesh3d_albedo"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.albedo_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.albedo_view = self
            .albedo_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.texture_bind_group =
            create_albedo_bind_group(device, &self.texture_bgl, &self.albedo_view, &self.sampler);
    }

    /// Group `pairs` by mesh handle, upload all instances back-to-back
    /// into the shared instance buffer, and return per-mesh batches
    /// (start offset + count) for the draw step.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pairs: &[(MeshHandle, MeshInstance)],
    ) -> Vec<MeshBatch> {
        if pairs.is_empty() {
            return Vec::new();
        }
        // Sort by handle so each mesh is contiguous.
        let mut sorted: Vec<(MeshHandle, MeshInstance)> = pairs.to_vec();
        sorted.sort_by_key(|(h, _)| h.0);

        let needed = sorted.len() as u64;
        if needed > self.instance_capacity {
            let mut new_cap = self.instance_capacity.max(1);
            while new_cap < needed {
                new_cap *= 2;
            }
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("infengine_render::mesh_instance_buf"),
                size: new_cap * std::mem::size_of::<MeshInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }

        let instances: Vec<MeshInstance> = sorted.iter().map(|(_, inst)| *inst).collect();
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));

        let mut batches = Vec::new();
        let mut start = 0u32;
        let mut current = sorted[0].0;
        for (i, (h, _)) in sorted.iter().enumerate() {
            if *h != current {
                batches.push(MeshBatch {
                    handle: current,
                    start,
                    count: i as u32 - start,
                });
                start = i as u32;
                current = *h;
            }
        }
        batches.push(MeshBatch {
            handle: current,
            start,
            count: sorted.len() as u32 - start,
        });
        batches
    }

    /// W157: upload the first-person viewmodel instances into the
    /// dedicated `vm_instance_buffer`. Same packing as [`upload`] but
    /// kept separate so the held-item draw runs in its own
    /// depth-cleared pass without overwriting the world instances.
    pub fn upload_viewmodel(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pairs: &[(MeshHandle, MeshInstance)],
    ) -> Vec<MeshBatch> {
        if pairs.is_empty() {
            return Vec::new();
        }
        let mut sorted: Vec<(MeshHandle, MeshInstance)> = pairs.to_vec();
        sorted.sort_by_key(|(h, _)| h.0);

        let needed = sorted.len() as u64;
        if needed > self.vm_instance_capacity {
            let mut new_cap = self.vm_instance_capacity.max(1);
            while new_cap < needed {
                new_cap *= 2;
            }
            self.vm_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("infengine_render::vm_instance_buf"),
                size: new_cap * std::mem::size_of::<MeshInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vm_instance_capacity = new_cap;
        }

        let instances: Vec<MeshInstance> = sorted.iter().map(|(_, inst)| *inst).collect();
        queue.write_buffer(&self.vm_instance_buffer, 0, bytemuck::cast_slice(&instances));

        let mut batches = Vec::new();
        let mut start = 0u32;
        let mut current = sorted[0].0;
        for (i, (h, _)) in sorted.iter().enumerate() {
            if *h != current {
                batches.push(MeshBatch { handle: current, start, count: i as u32 - start });
                start = i as u32;
                current = *h;
            }
        }
        batches.push(MeshBatch {
            handle: current,
            start,
            count: sorted.len() as u32 - start,
        });
        batches
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        registry: &'a Mesh3DRegistry,
        batches: &[MeshBatch],
        shadow_bg: &'a wgpu::BindGroup,
        lights_bg: &'a wgpu::BindGroup,
        tile_anims_bg: &'a wgpu::BindGroup,
        mesh_textures: &'a std::collections::HashMap<
            MeshHandle,
            std::sync::Arc<wgpu::BindGroup>,
        >,
    ) {
        if batches.is_empty() {
            return;
        }
        // Two sub-passes: opaque first (depth-write on), transparent
        // last (alpha blend, no depth write). Splitting by the
        // registry's transparent set keeps the two pipelines'
        // batches contiguous without re-sorting the instance buffer.
        rpass.set_bind_group(2, shadow_bg, &[]);
        rpass.set_bind_group(3, lights_bg, &[]);
        rpass.set_bind_group(4, tile_anims_bg, &[]);
        rpass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        rpass.set_pipeline(&self.pipeline);
        for batch in batches {
            if registry.is_transparent(batch.handle) {
                continue;
            }
            let Some(entry) = registry.get(batch.handle) else {
                continue;
            };
            let bg: &wgpu::BindGroup = match mesh_textures.get(&batch.handle) {
                Some(arc) => arc.as_ref(),
                None => &self.texture_bind_group,
            };
            rpass.set_bind_group(1, bg, &[]);
            rpass.set_vertex_buffer(0, entry.vertex_buffer.slice(..));
            rpass.set_index_buffer(entry.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            rpass.draw_indexed(
                0..entry.index_count,
                0,
                batch.start..batch.start + batch.count,
            );
        }
        rpass.set_pipeline(&self.pipeline_transparent);
        for batch in batches {
            if !registry.is_transparent(batch.handle) {
                continue;
            }
            let Some(entry) = registry.get(batch.handle) else {
                continue;
            };
            let bg: &wgpu::BindGroup = match mesh_textures.get(&batch.handle) {
                Some(arc) => arc.as_ref(),
                None => &self.texture_bind_group,
            };
            rpass.set_bind_group(1, bg, &[]);
            rpass.set_vertex_buffer(0, entry.vertex_buffer.slice(..));
            rpass.set_index_buffer(entry.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            rpass.draw_indexed(
                0..entry.index_count,
                0,
                batch.start..batch.start + batch.count,
            );
        }
    }

    /// W157: draw the first-person viewmodel batches from the
    /// `vm_instance_buffer`. The caller runs this inside a render pass
    /// that has just CLEARED depth (load color, clear depth) so the
    /// held item always paints on top of the world - MC clears the
    /// depth buffer before the hand for exactly this reason. Held
    /// items are opaque/cutout (never the translucent water branch),
    /// so only the opaque pipeline is used.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_viewmodel<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        registry: &'a Mesh3DRegistry,
        batches: &[MeshBatch],
        shadow_bg: &'a wgpu::BindGroup,
        lights_bg: &'a wgpu::BindGroup,
        tile_anims_bg: &'a wgpu::BindGroup,
        mesh_textures: &'a std::collections::HashMap<
            MeshHandle,
            std::sync::Arc<wgpu::BindGroup>,
        >,
    ) {
        if batches.is_empty() {
            return;
        }
        rpass.set_bind_group(2, shadow_bg, &[]);
        rpass.set_bind_group(3, lights_bg, &[]);
        rpass.set_bind_group(4, tile_anims_bg, &[]);
        rpass.set_vertex_buffer(1, self.vm_instance_buffer.slice(..));
        rpass.set_pipeline(&self.pipeline);
        for batch in batches {
            let Some(entry) = registry.get(batch.handle) else {
                continue;
            };
            let bg: &wgpu::BindGroup = match mesh_textures.get(&batch.handle) {
                Some(arc) => arc.as_ref(),
                None => &self.texture_bind_group,
            };
            rpass.set_bind_group(1, bg, &[]);
            rpass.set_vertex_buffer(0, entry.vertex_buffer.slice(..));
            rpass.set_index_buffer(entry.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            rpass.draw_indexed(
                0..entry.index_count,
                0,
                batch.start..batch.start + batch.count,
            );
        }
    }

    /// W32: draw all queued chunk meshes using the shared uber vertex/index
    /// buffers and a packed indirect command list.
    ///
    /// The opaque and transparent commands are stored consecutively in the
    /// indirect/instance buffers: opaque at offsets [0, opaque_count), then
    /// transparent at [opaque_count, opaque_count + transparent_count).
    ///
    /// When `has_indirect_first_instance` is true (DX12 / Vulkan / Metal)
    /// a single `multi_draw_indexed_indirect` call covers all commands in
    /// each layer. Otherwise a tight loop of `draw_indexed_indirect` calls
    /// is used (still avoids vertex/index rebinds, just slightly more CPU
    /// overhead than the single-call path).
    ///
    /// The caller must have already set @group(0) (camera) before calling
    /// this method; it sets groups 1-4 internally.
    /// W33.3: draw_uber_chunks with 3 render layers: opaque, cutout, translucent.
    ///
    /// Indirect buffer layout (layer order, each element is DrawIndexedIndirectArgs):
    ///   [0 .. opaque_count)                    - opaque
    ///   [opaque_count .. +cutout_count)        - cutout (alpha-discard, same pipeline as opaque)
    ///   [opaque_count+cutout_count .. +transl) - translucent (alpha-blend)
    ///
    /// Cutout uses the same `pipeline` as opaque (the fragment shader already
    /// discards fragments with alpha < 0.5). A dedicated pipeline_cutout can
    /// be added later for two-pass backface rendering of leaves.
    /// W238: draws are packed per (layer, page) segment; each segment
    /// binds its page's vertex/index buffers (only when they change)
    /// and issues one multi_draw over its contiguous indirect range.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_uber_chunks<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        pages: &'a [crate::uber_buffer::UberPage],
        indirect_buf: &'a wgpu::Buffer,
        instance_buf: &'a wgpu::Buffer,
        shadow_bg: &'a wgpu::BindGroup,
        lights_bg: &'a wgpu::BindGroup,
        tile_anims_bg: &'a wgpu::BindGroup,
        segments: &[crate::uber_buffer::ChunkDrawSegment],
        has_indirect_first_instance: bool,
    ) {
        if segments.is_empty() {
            return;
        }

        let indirect_stride = std::mem::size_of::<wgpu::util::DrawIndexedIndirectArgs>() as u64;

        rpass.set_bind_group(1, &self.texture_bind_group, &[]);
        rpass.set_bind_group(2, shadow_bg, &[]);
        rpass.set_bind_group(3, lights_bg, &[]);
        rpass.set_bind_group(4, tile_anims_bg, &[]);
        rpass.set_vertex_buffer(1, instance_buf.slice(..));

        // Segments arrive in layer order (opaque, cutout, translucent),
        // page-grouped within each layer. Opaque + cutout share the
        // depth-write pipeline (the shader alpha-discards); translucent
        // switches to the blend pipeline.
        let mut bound_page: Option<u32> = None;
        let mut bound_translucent = false;
        rpass.set_pipeline(&self.pipeline);
        for seg in segments {
            if seg.count == 0 {
                continue;
            }
            let translucent = seg.layer == crate::uber_buffer::LAYER_TRANSLUCENT;
            if translucent != bound_translucent {
                rpass.set_pipeline(if translucent {
                    &self.pipeline_transparent
                } else {
                    &self.pipeline
                });
                bound_translucent = translucent;
            }
            if bound_page != Some(seg.page) {
                let Some(page) = pages.get(seg.page as usize) else {
                    continue;
                };
                rpass.set_vertex_buffer(0, page.vertex_buf.slice(..));
                rpass.set_index_buffer(page.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                bound_page = Some(seg.page);
            }
            let offset = seg.first as u64 * indirect_stride;
            if has_indirect_first_instance {
                rpass.multi_draw_indexed_indirect(indirect_buf, offset, seg.count);
            } else {
                for i in 0..seg.count {
                    rpass.draw_indexed_indirect(
                        indirect_buf,
                        offset + i as u64 * indirect_stride,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_vertex_size_matches_attribute_layout() {
        // W30.1: position (12) + normal_pad (4) + uv_packed (4)
        // + light_pad (4) + anim_id (4) + tile_uv0 (4) + tile_size (4)
        // + quad_uv (4) = 40 bytes.
        assert_eq!(std::mem::size_of::<MeshVertex>(), 40);
    }

    #[test]
    fn mesh_vertex_new_defaults_static_anim_id() {
        let v = MeshVertex::new([1.0, 2.0, 3.0], [0.0, 1.0, 0.0]);
        assert_eq!(v.position, [1.0, 2.0, 3.0]);
        // Decode and check with tolerance (i8 packing loses ~0.8% precision).
        let n = v.normal();
        assert!((n[0]).abs() < 1e-3);
        assert!((n[1] - 1.0).abs() < 0.01);
        assert!((n[2]).abs() < 1e-3);
        let uv = v.uv();
        assert!(uv[0].abs() < 1e-5);
        assert!(uv[1].abs() < 1e-5);
        let light = v.light();
        assert!(light[0].abs() < 1e-4);
        assert!((light[1] - 1.0).abs() < 1e-4);
        assert_eq!(v.anim_id, 0);
    }

    #[test]
    fn with_anim_id_clamps_to_max_slot() {
        // The shader's tile_anim array has 32 entries. Slot ids
        // above 32 clamp to 32 so they never index out of bounds.
        let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_anim_id(5);
        assert_eq!(v.anim_id, 5);
        let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_anim_id(32);
        assert_eq!(v.anim_id, 32);
        let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_anim_id(99);
        assert_eq!(v.anim_id, 32);
    }

    #[test]
    fn with_anim_id_zero_remains_static() {
        let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_anim_id(0);
        assert_eq!(v.anim_id, 0);
    }

    #[test]
    fn mesh_vertex_anim_id_offset_is_24() {
        // W30.1: packed layout. anim_id is at offset 24.
        // Critical: if any field shifts this offset, the GPU reads the
        // wrong bytes and animated tiles get random slot ids.
        let mut v = MeshVertex::new([0.0; 3], [0.0; 3]);
        v.anim_id = 0xDEAD_BEEFu32;
        let bytes: &[u8] = bytemuck::bytes_of(&v);
        let offset = 24usize;
        let slice = &bytes[offset..offset + 4];
        let value = u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]);
        assert_eq!(value, 0xDEAD_BEEFu32);
    }

    #[test]
    fn with_tile_uv_round_trips() {
        // W30.1: tile_uv0/tile_size/quad_uv pack + decode round-trips within
        // expected quantisation error.
        let v = MeshVertex::new([0.0; 3], [0.0; 3])
            .with_tile_uv([0.25, 0.5], [0.0625, 0.0625], [2.0, 3.0]);
        let uv0 = v.tile_uv0_f32();
        let sz = v.tile_size_f32();
        let quv = v.quad_uv_f32();
        // tile_uv0 tolerance: 1/65535 ~ 1.5e-5
        assert!((uv0[0] - 0.25).abs() < 2e-4, "tile_uv0.x off: {}", uv0[0]);
        assert!((uv0[1] - 0.5).abs() < 2e-4, "tile_uv0.y off: {}", uv0[1]);
        assert!((sz[0] - 0.0625).abs() < 2e-4, "tile_size.x off: {}", sz[0]);
        assert!((sz[1] - 0.0625).abs() < 2e-4, "tile_size.y off: {}", sz[1]);
        // quad_uv tolerance: 16/65535 ~ 2.4e-4
        assert!((quv[0] - 2.0).abs() < 5e-3, "quad_uv.x off: {}", quv[0]);
        assert!((quv[1] - 3.0).abs() < 5e-3, "quad_uv.y off: {}", quv[1]);
    }

    #[test]
    fn with_uv_sets_tile_uv0_and_zero_size() {
        // W30.1: with_uv compat - tile_uv0 = uv, tile_size = 0 (shader
        // outputs tile_uv0 + fract(0) * 0 = tile_uv0 = uv).
        let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_uv([0.5, 0.75]);
        let uv = v.uv();
        let t0 = v.tile_uv0_f32();
        let ts = v.tile_size_f32();
        assert!((uv[0] - t0[0]).abs() < 2e-5, "tile_uv0.x != uv.x");
        assert!((uv[1] - t0[1]).abs() < 2e-5, "tile_uv0.y != uv.y");
        assert_eq!(ts, [0.0, 0.0], "tile_size should be 0 for with_uv path");
    }

    #[test]
    fn mesh_vertex_size_is_40() {
        // W30.1: extended layout must be exactly 40 bytes.
        assert_eq!(std::mem::size_of::<MeshVertex>(), 40);
    }
}
