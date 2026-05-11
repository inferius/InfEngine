//! Single-sprite pipeline for Phase 1.3.
//!
//! Renders one textured quad whose position and scale are driven by a
//! tiny uniform. The texture is generated procedurally (a checkerboard)
//! so the renderer crate has no asset-loading dependency yet.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Vertex format used by the sprite pipeline: position in NDC and UV in
/// `[0, 1]`. Kept tightly packed so it can be uploaded directly.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct SpriteVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

const SPRITE_VERTICES: [SpriteVertex; 4] = [
    SpriteVertex {
        position: [-1.0, -1.0],
        uv: [0.0, 1.0],
    },
    SpriteVertex {
        position: [1.0, -1.0],
        uv: [1.0, 1.0],
    },
    SpriteVertex {
        position: [-1.0, 1.0],
        uv: [0.0, 0.0],
    },
    SpriteVertex {
        position: [1.0, 1.0],
        uv: [1.0, 0.0],
    },
];

/// Uniform pushed to the sprite shader each frame: (offset_xy, scale_xy).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct SpriteUniform {
    pub transform: [f32; 4],
}

impl SpriteUniform {
    pub fn new(offset: [f32; 2], scale: [f32; 2]) -> Self {
        Self {
            transform: [offset[0], offset[1], scale[0], scale[1]],
        }
    }
}

/// Side length, in pixels, of the procedural checker texture.
const CHECKER_SIZE: u32 = 128;
/// Side length of each individual checker cell.
const CHECKER_TILE: u32 = 16;

fn generate_checker_rgba(size: u32, tile: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let on = ((x / tile) + (y / tile)).is_multiple_of(2);
            if on {
                bytes.extend_from_slice(&[235, 200, 90, 255]);
            } else {
                bytes.extend_from_slice(&[40, 50, 70, 255]);
            }
        }
    }
    bytes
}

/// All GPU resources required to draw the single demo sprite.
pub(crate) struct SpritePass {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl SpritePass {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        // Texture: procedural checker, uploaded once at startup.
        let texture_size = wgpu::Extent3d {
            width: CHECKER_SIZE,
            height: CHECKER_SIZE,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("infengine_render::sprite_texture"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let pixels = generate_checker_rgba(CHECKER_SIZE, CHECKER_TILE);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * CHECKER_SIZE),
                rows_per_image: Some(CHECKER_SIZE),
            },
            texture_size,
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("infengine_render::sprite_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Default to a centered, half-size sprite.
        let initial_uniform = SpriteUniform::new([0.0, 0.0], [0.5, 0.5]);
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("infengine_render::sprite_uniform"),
            contents: bytemuck::bytes_of(&initial_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::sprite_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::sprite_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        // Vertex buffer for the unit quad. Static for the lifetime of the
        // pipeline; transforms come from the uniform.
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("infengine_render::sprite_vbuf"),
            contents: bytemuck::cast_slice(&SPRITE_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("infengine_render::sprite_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("infengine_render::sprite_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
        ];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("infengine_render::sprite_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SpriteVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attrs,
                }],
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
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            vertex_buffer,
            uniform_buffer,
            bind_group,
        }
    }

    /// Update the sprite's transform uniform (offset and scale in NDC).
    pub fn set_transform(&self, queue: &wgpu::Queue, offset: [f32; 2], scale: [f32; 2]) {
        let uniform = SpriteUniform::new(offset, scale);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// Record draw commands into an existing render pass.
    pub fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>) {
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.draw(0..SPRITE_VERTICES.len() as u32, 0..1);
    }
}
