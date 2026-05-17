//! Directional light shadow map.
//!
//! Phase 10.3 adds a single-cascade shadow map: a 2048x2048 depth texture
//! rendered from the sun's POV through an orthographic projection. The
//! mesh fragment shader samples it with a 3x3 PCF kernel to attenuate
//! direct lighting on occluded surfaces. Cascades come later.
//!
//! Bind groups exposed:
//! - `vertex_bgl`/`vertex_bg`: just the light view-projection matrix at
//!   binding 0. Used by the depth-only shadow pipeline.
//! - `sample_bgl`/`sample_bg`: the same uniform at binding 0, the depth
//!   texture at binding 1, and a comparison sampler at binding 2. Bound
//!   at `@group(2)` of the main mesh pipeline.
//!
//! Both bind groups reference the same underlying buffer so updates via
//! `upload_light` reach both pipelines.
//!
//! Light direction is hard-coded to match the sun direction baked into
//! `mesh3d.wgsl`. The orthographic frustum is anchored on a caller-
//! supplied scene center and covers `ortho_half_size` units in X/Y and
//! `depth_range` in Z along the light direction.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use crate::mesh3d::{Mesh3DRegistry, MeshBatch, MeshInstance, MeshVertex};

pub(crate) const SHADOW_MAP_SIZE: u32 = 2048;
pub(crate) const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ShadowUniform {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
}

pub(crate) struct ShadowPass {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    pub(crate) depth_view: wgpu::TextureView,
    buffer: wgpu::Buffer,
    #[allow(dead_code)]
    pub(crate) vertex_bgl: wgpu::BindGroupLayout,
    pub(crate) vertex_bg: wgpu::BindGroup,
    pub(crate) sample_bgl: wgpu::BindGroupLayout,
    pub(crate) sample_bg: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl ShadowPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("infengine_render::shadow_map"),
            size: wgpu::Extent3d {
                width: SHADOW_MAP_SIZE,
                height: SHADOW_MAP_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::shadow_uniform"),
            size: std::mem::size_of::<ShadowUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let vertex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::shadow_vertex_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let vertex_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::shadow_vertex_bg"),
            layout: &vertex_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("infengine_render::shadow_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let sample_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::shadow_sample_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Depth,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let sample_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::shadow_sample_bg"),
            layout: &sample_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("infengine_render::shadow_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("infengine_render::shadow_pipeline_layout"),
            bind_group_layouts: &[&vertex_bgl],
            push_constant_ranges: &[],
        });

        // Mesh vertex layout: only position is consumed; normal / uv are
        // present in the buffer but ignored.
        let vertex_attrs = [wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        }];
        let instance_attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 48,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("infengine_render::shadow_pipeline"),
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
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // Render back faces only to mitigate self-shadow acne on
                // front-facing surfaces.
                cull_mode: Some(wgpu::Face::Front),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SHADOW_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            texture,
            depth_view,
            buffer,
            vertex_bgl,
            vertex_bg,
            sample_bgl,
            sample_bg,
            pipeline,
        }
    }

    /// Update the shadow uniform. `center` is the world-space point the
    /// shadow frustum is anchored on (typically the camera target);
    /// `light_dir` points TOWARDS the light (i.e. opposite of incoming
    /// rays); `half_size` is the orthographic half-extent in X/Y; `depth`
    /// is the half-extent in Z along the light direction.
    pub fn upload_light(
        &self,
        queue: &wgpu::Queue,
        center: Vec3,
        light_dir: Vec3,
        half_size: f32,
        depth: f32,
        intensity: f32,
    ) {
        let dir = light_dir.normalize_or_zero();
        let dir = if dir == Vec3::ZERO { Vec3::Y } else { dir };
        let eye = center + dir * depth;
        let up = if dir.abs().y > 0.9 { Vec3::Z } else { Vec3::Y };
        let view = Mat4::look_at_rh(eye, center, up);
        let proj = Mat4::orthographic_rh(
            -half_size,
            half_size,
            -half_size,
            half_size,
            0.0,
            depth * 2.0,
        );
        // Pack sun intensity into the w channel of light_dir so we
        // don't need a separate uniform binding. Drivers that
        // ignore the w component (older shaders) just behave as
        // if intensity == 0.
        let uniform = ShadowUniform {
            view_proj: (proj * view).to_cols_array_2d(),
            light_dir: [dir.x, dir.y, dir.z, intensity.max(0.0)],
        };
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn draw<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        registry: &'a Mesh3DRegistry,
        batches: &[MeshBatch],
        instance_buffer: &'a wgpu::Buffer,
    ) {
        if batches.is_empty() {
            return;
        }
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.vertex_bg, &[]);
        rpass.set_vertex_buffer(1, instance_buffer.slice(..));
        for batch in batches {
            // Transparent meshes (water etc.) skip the shadow pass:
            // their depth contribution would cast an opaque shadow
            // even though the lit surface beneath should still be
            // visible through the water.
            if registry.is_transparent(batch.handle) {
                continue;
            }
            let Some(entry) = registry.get(batch.handle) else {
                continue;
            };
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
