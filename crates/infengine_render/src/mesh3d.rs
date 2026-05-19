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

/// Vertex format used by `Mesh3DPass`. Includes UV coordinates so the
/// mesh shader can sample an albedo texture, plus a `light` channel
/// (Phase L): two normalised values (block, sky) in `[0, 1]` that
/// drive the per-voxel lighting modulation, plus an `anim_id` slot
/// (Wave 9.y): `0` = static tile, `1..=32` indexes into the shader's
/// `tile_anim` uniform array for animated UV scroll. Non-voxel
/// meshes (gltf loads, cube_demo) leave `light = [0.0, 1.0]` and
/// `anim_id = 0` so they default to "no block light, full sky,
/// static UV" which mirrors today's unlit-textured behaviour when
/// the shader picks `max(block, sky * day_factor)`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// `(block_light_norm, sky_light_norm)` in `[0, 1]`. Wave 3
    /// L.4 plumbs this from per-chunk `LightGrid`s; everything
    /// else keeps the default `[0.0, 1.0]` ("no block light, full
    /// sky") so day-time lighting is unchanged on legacy meshes.
    pub light: [f32; 2],
    /// Wave 9.y animation slot. `0` = static tile (no UV scroll),
    /// `1..=32` indexes into the shader's `tile_anim` uniform
    /// array so the fragment shader can offset `uv.v` by the
    /// active frame each draw. Voxel meshes set this from the
    /// resource pack's `AnimRegistry`; legacy callers leave it at
    /// 0 so static UV sampling is unchanged.
    pub anim_id: u32,
}

impl MeshVertex {
    pub fn new(position: [f32; 3], normal: [f32; 3]) -> Self {
        Self {
            position,
            normal,
            uv: [0.0, 0.0],
            light: [0.0, 1.0],
            anim_id: 0,
        }
    }

    pub fn with_uv(mut self, uv: [f32; 2]) -> Self {
        self.uv = uv;
        self
    }

    /// Override the per-vertex `(block, sky)` light values. Inputs
    /// are clamped to `[0, 1]`. The shader takes `max(block, sky *
    /// day_factor)` and folds it into the final color, so a vertex
    /// with `light = [0.0, 0.0]` renders pitch black (subject to
    /// the shader's small minimum floor) while `[1.0, 1.0]` renders
    /// at full brightness regardless of the day/night ramp.
    pub fn with_light(mut self, block: f32, sky: f32) -> Self {
        self.light = [block.clamp(0.0, 1.0), sky.clamp(0.0, 1.0)];
        self
    }

    /// Wave 9.y: tag this vertex with an animation slot id. `0`
    /// means static (no UV scroll); `1..=32` references an entry
    /// in the shader's `tile_anim` uniform array so the fragment
    /// shader walks through the animated tile's frame strip. Slots
    /// outside `0..=32` are clamped because the uniform array is
    /// statically sized.
    pub fn with_anim_id(mut self, slot: u32) -> Self {
        self.anim_id = slot.min(32);
        self
    }
}

/// Opaque numeric handle for a registered mesh.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct MeshHandle(u32);

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
        let fallback_v: [MeshVertex; 1] = [MeshVertex {
            position: [0.0; 3],
            normal: [0.0; 3],
            uv: [0.0; 2],
            light: [0.0, 1.0],
            anim_id: 0,
        }];
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
            vertices.push(MeshVertex {
                position: *c,
                normal,
                uv: FACE_UVS[i],
                light: [0.0, 1.0],
                anim_id: 0,
            });
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

    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_layout: &wgpu::BindGroupLayout,
        shadow_sample_layout: &wgpu::BindGroupLayout,
        lights_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::mesh_instance_buf"),
            size: INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<MeshInstance>() as u64,
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
            ],
            push_constant_ranges: &[],
        });

        let vertex_attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: (std::mem::size_of::<[f32; 3]>() * 2) as wgpu::BufferAddress,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            // Phase L.4: per-vertex (block_light, sky_light) in [0,1].
            // Sits right after the UV pair; the instance attrs at
            // location 3..=8 are in a separate buffer slot so this
            // is free to live at location 9.
            wgpu::VertexAttribute {
                offset: (std::mem::size_of::<[f32; 3]>() * 2
                    + std::mem::size_of::<[f32; 2]>())
                    as wgpu::BufferAddress,
                shader_location: 9,
                format: wgpu::VertexFormat::Float32x2,
            },
            // Wave 9.y: anim_id (u32). 0 = static, 1..=32 indexes
            // into the shader's `tile_anim` uniform array. Sits
            // right after the per-vertex light pair at offset 40.
            wgpu::VertexAttribute {
                offset: (std::mem::size_of::<[f32; 3]>() * 2
                    + std::mem::size_of::<[f32; 2]>() * 2)
                    as wgpu::BufferAddress,
                shader_location: 10,
                format: wgpu::VertexFormat::Uint32,
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

    pub fn draw<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        registry: &'a Mesh3DRegistry,
        batches: &[MeshBatch],
        shadow_bg: &'a wgpu::BindGroup,
        lights_bg: &'a wgpu::BindGroup,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_vertex_size_matches_attribute_layout() {
        // position (12) + normal (12) + uv (8) + light (8) + anim_id (4)
        // = 44 bytes. The wgpu vertex layout's last attribute lives
        // at offset 40 + 4 = 44, matching size_of::<MeshVertex>().
        assert_eq!(std::mem::size_of::<MeshVertex>(), 44);
    }

    #[test]
    fn mesh_vertex_new_defaults_static_anim_id() {
        let v = MeshVertex::new([1.0, 2.0, 3.0], [0.0, 1.0, 0.0]);
        assert_eq!(v.position, [1.0, 2.0, 3.0]);
        assert_eq!(v.normal, [0.0, 1.0, 0.0]);
        assert_eq!(v.uv, [0.0, 0.0]);
        assert_eq!(v.light, [0.0, 1.0]);
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
    fn mesh_vertex_anim_id_offset_is_40() {
        // Critical for the wgpu vertex attribute at shader_location
        // 10 - if any field shifts this offset, the GPU reads the
        // wrong bytes and animated tiles get random slot ids.
        let v = MeshVertex {
            position: [0.0; 3],
            normal: [0.0; 3],
            uv: [0.0; 2],
            light: [0.0; 2],
            anim_id: 0xDEADBEEFu32,
        };
        let bytes: &[u8] = bytemuck::bytes_of(&v);
        let offset = 40usize;
        let slice = &bytes[offset..offset + 4];
        let value = u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]);
        assert_eq!(value, 0xDEADBEEFu32);
    }
}
