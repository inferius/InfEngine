//! Batched 2D sprite pipeline.
//!
//! A single draw call renders many sprites that share a texture. Per
//! instance, the caller supplies world position, size, color tint, and a
//! UV sub-rectangle (so a texture atlas can be used in later steps).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Vertex format used by the unit-quad mesh: position in `[-0.5, 0.5]`
/// and UV in `[0, 1]`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct SpriteVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

const SPRITE_VERTICES: [SpriteVertex; 4] = [
    SpriteVertex {
        position: [-0.5, -0.5],
        uv: [0.0, 1.0],
    },
    SpriteVertex {
        position: [0.5, -0.5],
        uv: [1.0, 1.0],
    },
    SpriteVertex {
        position: [-0.5, 0.5],
        uv: [0.0, 0.0],
    },
    SpriteVertex {
        position: [0.5, 0.5],
        uv: [1.0, 0.0],
    },
];

/// One sprite submitted to the batcher.
///
/// `position` is the world-space center of the quad. `size` is the quad
/// extent in world units. `color` multiplies the sampled texel (use
/// `[1, 1, 1, 1]` for an untinted sprite). `uv_min` / `uv_max` define
/// the sampled sub-rectangle of the sprite texture; for a full-texture
/// sprite use `[0, 0]` and `[1, 1]`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct SpriteInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

impl SpriteInstance {
    /// Convenience constructor: untinted full-texture sprite.
    pub fn new(position: [f32; 2], size: [f32; 2]) -> Self {
        Self {
            position,
            size,
            color: [1.0, 1.0, 1.0, 1.0],
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
        }
    }

    /// Convenience constructor that samples a sub-rectangle of the
    /// sprite texture. Use with the constants in `demo_atlas` or with
    /// any other `(uv_min, uv_max)` pair from an atlas region.
    pub fn from_region(position: [f32; 2], size: [f32; 2], region: ([f32; 2], [f32; 2])) -> Self {
        Self {
            position,
            size,
            color: [1.0, 1.0, 1.0, 1.0],
            uv_min: region.0,
            uv_max: region.1,
        }
    }

    /// Builder-style tint mutator.
    pub fn with_color(mut self, color: [f32; 4]) -> Self {
        self.color = color;
        self
    }
}

/// Side length, in pixels, of the procedurally generated sprite atlas.
/// The atlas is laid out as a 2x2 grid of 128x128 quadrants.
const ATLAS_SIZE: u32 = 256;
const QUADRANT_SIZE: u32 = ATLAS_SIZE / 2;

/// Generate the four-quadrant demo atlas:
/// - top-left:     checkerboard
/// - top-right:    dot grid
/// - bottom-left:  horizontal stripes
/// - bottom-right: diagonal cross
fn generate_atlas_rgba() -> Vec<u8> {
    let size = ATLAS_SIZE as usize;
    let mut bytes = vec![0u8; size * size * 4];

    let put = |bytes: &mut [u8], x: u32, y: u32, rgba: [u8; 4]| {
        let i = ((y as usize) * size + x as usize) * 4;
        bytes[i..i + 4].copy_from_slice(&rgba);
    };

    let warm = [235, 200, 90, 255];
    let dark = [40, 50, 70, 255];
    let mid = [120, 130, 150, 255];

    for y in 0..ATLAS_SIZE {
        for x in 0..ATLAS_SIZE {
            let qx = x % QUADRANT_SIZE;
            let qy = y % QUADRANT_SIZE;
            let quadrant_x = x / QUADRANT_SIZE;
            let quadrant_y = y / QUADRANT_SIZE;

            let rgba = match (quadrant_x, quadrant_y) {
                // top-left: checker (note: top-left in UV space is y == 0)
                (0, 0) => {
                    let on = ((qx / 16) + (qy / 16)).is_multiple_of(2);
                    if on { warm } else { dark }
                }
                // top-right: dot grid (16-pixel cells, dot of radius 5)
                (1, 0) => {
                    let cx = (qx % 16) as i32 - 8;
                    let cy = (qy % 16) as i32 - 8;
                    if cx * cx + cy * cy <= 5 * 5 {
                        warm
                    } else {
                        dark
                    }
                }
                // bottom-left: horizontal stripes (every 8 pixels)
                (0, 1) => {
                    if (qy / 8).is_multiple_of(2) {
                        warm
                    } else {
                        dark
                    }
                }
                // bottom-right: diagonal cross (X) on a mid background
                (1, 1) => {
                    let d1 = (qx as i32 - qy as i32).abs();
                    let d2 = (qx as i32 + qy as i32 - QUADRANT_SIZE as i32 + 1).abs();
                    if d1 < 6 || d2 < 6 { warm } else { mid }
                }
                _ => dark,
            };
            put(&mut bytes, x, y, rgba);
        }
    }
    bytes
}

/// Pre-defined UV sub-rectangles for the four quadrants of the demo
/// atlas. Exposed so `sprite_demo` (and similar tests) can pick a
/// region without hard-coding magic numbers. These are temporary - they
/// disappear once the real asset pipeline arrives in Phase 3.
pub mod demo_atlas {
    /// Checkerboard quadrant.
    pub const CHECKER: ([f32; 2], [f32; 2]) = ([0.0, 0.0], [0.5, 0.5]);
    /// Dot-grid quadrant.
    pub const DOTS: ([f32; 2], [f32; 2]) = ([0.5, 0.0], [1.0, 0.5]);
    /// Horizontal-stripes quadrant.
    pub const STRIPES: ([f32; 2], [f32; 2]) = ([0.0, 0.5], [0.5, 1.0]);
    /// Diagonal cross quadrant.
    pub const CROSS: ([f32; 2], [f32; 2]) = ([0.5, 0.5], [1.0, 1.0]);
}

/// Initial GPU instance buffer capacity (in sprite instances).
const INITIAL_INSTANCE_CAPACITY: u64 = 256;

/// Batched sprite pipeline + its texture, sampler, and instance buffer.
pub(crate) struct SpritePass {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    texture_bind_group: wgpu::BindGroup,
}

impl SpritePass {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        camera_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // Procedural 256x256 demo atlas (Phase 2 still has no asset loader).
        let texture_size = wgpu::Extent3d {
            width: ATLAS_SIZE,
            height: ATLAS_SIZE,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("infengine_render::sprite_atlas"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let pixels = generate_atlas_rgba();
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
                bytes_per_row: Some(4 * ATLAS_SIZE),
                rows_per_image: Some(ATLAS_SIZE),
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

        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::sprite_texture_bgl"),
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

        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::sprite_texture_bg"),
            layout: &texture_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("infengine_render::sprite_vbuf"),
            contents: bytemuck::cast_slice(&SPRITE_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::sprite_instance_buf"),
            size: INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<SpriteInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("infengine_render::sprite_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("infengine_render::sprite_pipeline_layout"),
            bind_group_layouts: &[camera_layout, &texture_bgl],
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

        let instance_attrs = [
            // position (vec2)
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            // size (vec2)
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x2,
            },
            // color (vec4)
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            // uv_min (vec2)
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x2,
            },
            // uv_max (vec2)
            wgpu::VertexAttribute {
                offset: 40,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x2,
            },
        ];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("infengine_render::sprite_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<SpriteVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vertex_attrs,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<SpriteInstance>() as wgpu::BufferAddress,
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
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            texture_bind_group,
        }
    }

    /// Upload `instances` into the GPU buffer, growing it if necessary.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[SpriteInstance],
    ) {
        let needed = instances.len() as u64;
        if needed > self.instance_capacity {
            let mut new_cap = self.instance_capacity.max(1);
            while new_cap < needed {
                new_cap *= 2;
            }
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("infengine_render::sprite_instance_buf"),
                size: new_cap * std::mem::size_of::<SpriteInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }
        if !instances.is_empty() {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
        }
    }

    /// Record draw commands for `count` previously-uploaded instances.
    /// Assumes the caller has already bound the camera at `@group(0)`.
    pub fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, count: u32) {
        if count == 0 {
            return;
        }
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(1, &self.texture_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        rpass.draw(0..SPRITE_VERTICES.len() as u32, 0..count);
    }
}
