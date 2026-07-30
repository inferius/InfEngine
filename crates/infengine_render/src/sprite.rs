//! Batched 2D sprite pipeline.
//!
//! A single draw call renders many sprites that share a texture. Per
//! instance, the caller supplies world position, size, color tint, and a
//! UV sub-rectangle (so a texture atlas can be used in later steps).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::font;

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

/// Width of the procedurally generated sprite atlas, in pixels.
const ATLAS_W: u32 = 256;
/// Height of the atlas. The top `SPRITE_REGION_H` rows hold the 2x2
/// sprite quadrants; the remaining rows are reserved for the bitmap
/// font region used by text rendering.
const ATLAS_H: u32 = 304;
const SPRITE_REGION_H: u32 = 256;
const QUADRANT_SIZE: u32 = SPRITE_REGION_H / 2;
const FONT_BASE_Y: u32 = SPRITE_REGION_H;

/// Generate the demo atlas:
/// - top-left:     checkerboard
/// - top-right:    dot grid
/// - bottom-left:  horizontal stripes
/// - bottom-right: diagonal cross
/// - bottom strip: bitmap font glyphs (16 cols x 6 rows, 8x8 each).
fn generate_atlas_rgba() -> Vec<u8> {
    let w = ATLAS_W as usize;
    let h = ATLAS_H as usize;
    let mut bytes = vec![0u8; w * h * 4];

    let put = |bytes: &mut [u8], x: u32, y: u32, rgba: [u8; 4]| {
        let i = ((y as usize) * w + x as usize) * 4;
        bytes[i..i + 4].copy_from_slice(&rgba);
    };

    let warm = [235, 200, 90, 255];
    let dark = [40, 50, 70, 255];
    let mid = [120, 130, 150, 255];

    // Sprite region: 2x2 quadrants in the top SPRITE_REGION_H rows.
    for y in 0..SPRITE_REGION_H {
        for x in 0..ATLAS_W {
            let qx = x % QUADRANT_SIZE;
            let qy = y % QUADRANT_SIZE;
            let quadrant_x = x / QUADRANT_SIZE;
            let quadrant_y = y / QUADRANT_SIZE;

            let rgba = match (quadrant_x, quadrant_y) {
                (0, 0) => {
                    let on = ((qx / 16) + (qy / 16)).is_multiple_of(2);
                    if on { warm } else { dark }
                }
                (1, 0) => {
                    let cx = (qx % 16) as i32 - 8;
                    let cy = (qy % 16) as i32 - 8;
                    if cx * cx + cy * cy <= 5 * 5 {
                        warm
                    } else {
                        dark
                    }
                }
                (0, 1) => {
                    if (qy / 8).is_multiple_of(2) {
                        warm
                    } else {
                        dark
                    }
                }
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

    // Font region: glyphs are white on transparent so the tint color
    // controls the final ink color.
    let white = [255, 255, 255, 255];
    let transparent = [0, 0, 0, 0];
    for ascii in 32u8..128 {
        let bm = font::glyph_bitmap(ascii);
        let (gx, gy) = font::glyph_cell(ascii);
        let atlas_x = gx;
        let atlas_y = FONT_BASE_Y + gy;
        for py in 0..font::GLYPH_PIXELS {
            let row = bm[py as usize];
            for px in 0..font::GLYPH_PIXELS {
                let on = (row >> (7 - px)) & 1 != 0;
                let color = if on { white } else { transparent };
                if atlas_x + px < ATLAS_W && atlas_y + py < ATLAS_H {
                    put(&mut bytes, atlas_x + px, atlas_y + py, color);
                }
            }
        }
    }

    bytes
}

/// Atlas-space UV rectangle for the bitmap-font glyph at `ascii`.
pub fn glyph_uv(ascii: u8) -> ([f32; 2], [f32; 2]) {
    let (cx, cy) = font::glyph_cell(ascii);
    let x0 = cx as f32 / ATLAS_W as f32;
    let y0 = (FONT_BASE_Y + cy) as f32 / ATLAS_H as f32;
    let x1 = (cx + font::GLYPH_PIXELS) as f32 / ATLAS_W as f32;
    let y1 = (FONT_BASE_Y + cy + font::GLYPH_PIXELS) as f32 / ATLAS_H as f32;
    ([x0, y0], [x1, y1])
}

/// Pre-defined UV sub-rectangles for the four quadrants of the demo
/// atlas. Exposed so `sprite_demo` (and similar tests) can pick a
/// region without hard-coding magic numbers. These are temporary - they
/// disappear once the real asset pipeline arrives in Phase 3.
pub mod demo_atlas {
    use super::{ATLAS_H, SPRITE_REGION_H};
    const HALF_V: f32 = (SPRITE_REGION_H as f32) * 0.5 / (ATLAS_H as f32);
    const FULL_V: f32 = (SPRITE_REGION_H as f32) / (ATLAS_H as f32);

    /// Checkerboard quadrant.
    pub const CHECKER: ([f32; 2], [f32; 2]) = ([0.0, 0.0], [0.5, HALF_V]);
    /// Dot-grid quadrant.
    pub const DOTS: ([f32; 2], [f32; 2]) = ([0.5, 0.0], [1.0, HALF_V]);
    /// Horizontal-stripes quadrant.
    pub const STRIPES: ([f32; 2], [f32; 2]) = ([0.0, HALF_V], [0.5, FULL_V]);
    /// Diagonal cross quadrant.
    pub const CROSS: ([f32; 2], [f32; 2]) = ([0.5, HALF_V], [1.0, FULL_V]);
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
    /// `texture_bgl` must be the layout produced by
    /// [`crate::texture2d::sprite_texture_bgl`] and shared with the
    /// texture registry, so runtime textures can be bound at
    /// `@group(1)` against this same pipeline.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        camera_layout: &wgpu::BindGroupLayout,
        texture_bgl: &wgpu::BindGroupLayout,
    ) -> Self {
        // Procedural demo atlas with a font region in the bottom strip.
        let texture_size = wgpu::Extent3d {
            width: ATLAS_W,
            height: ATLAS_H,
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
                bytes_per_row: Some(4 * ATLAS_W),
                rows_per_image: Some(ATLAS_H),
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

        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::sprite_texture_bg"),
            layout: texture_bgl,
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
            bind_group_layouts: &[camera_layout, texture_bgl],
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

    /// Record draw commands for instances `[first, first + count)` of the
    /// uploaded buffer, sampling `texture_bind_group`.
    ///
    /// Assumes the caller has already bound the camera at `@group(0)`.
    /// A1.1: this is the batching primitive - the renderer walks runs of
    /// same-texture instances and issues one call per run.
    pub fn draw_range<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        texture_bind_group: &'a wgpu::BindGroup,
        first: u32,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(1, texture_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        rpass.draw(0..SPRITE_VERTICES.len() as u32, first..first + count);
    }

    /// Record draw commands for `count` instances starting at `first`,
    /// sampling the built-in procedural atlas (debug quads + font).
    pub fn draw_atlas<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, first: u32, count: u32) {
        self.draw_range(rpass, &self.texture_bind_group, first, count);
    }
}
