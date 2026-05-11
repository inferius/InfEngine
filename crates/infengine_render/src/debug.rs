//! Debug-line overlay.
//!
//! A separate pipeline that draws colored line segments in world space.
//! Intended for visualizing bounds, paths, raycasts, and similar
//! short-lived diagnostics. Lines are submitted via `Renderer::debug_*`
//! helpers each frame; the buffer is cleared after every `render()`.

use bytemuck::{Pod, Zeroable};

/// Vertex format for the debug-line pipeline.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct DebugVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

const INITIAL_VERTEX_CAPACITY: u64 = 2048;

pub(crate) struct DebugPass {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    capacity: u64,
}

impl DebugPass {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        camera_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("infengine_render::debug_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("debug.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("infengine_render::debug_pipeline_layout"),
            bind_group_layouts: &[camera_layout],
            push_constant_ranges: &[],
        });

        let attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("infengine_render::debug_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<DebugVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
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
                topology: wgpu::PrimitiveTopology::LineList,
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

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::debug_vbuf"),
            size: INITIAL_VERTEX_CAPACITY * std::mem::size_of::<DebugVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            buffer,
            capacity: INITIAL_VERTEX_CAPACITY,
        }
    }

    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vertices: &[DebugVertex]) {
        let needed = vertices.len() as u64;
        if needed > self.capacity {
            let mut new_cap = self.capacity.max(1);
            while new_cap < needed {
                new_cap *= 2;
            }
            self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("infengine_render::debug_vbuf"),
                size: new_cap * std::mem::size_of::<DebugVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.capacity = new_cap;
        }
        if !vertices.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(vertices));
        }
    }

    pub fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, vertex_count: u32) {
        if vertex_count == 0 {
            return;
        }
        rpass.set_pipeline(&self.pipeline);
        rpass.set_vertex_buffer(0, self.buffer.slice(..));
        rpass.draw(0..vertex_count, 0..1);
    }
}
