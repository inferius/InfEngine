//! 3D debug line list.
//!
//! Pair of `(Debug3DVertex, Debug3DVertex)` pushed via the renderer
//! `debug_line_3d` / `debug_box_3d` helpers and rendered after the
//! mesh pass with depth-test only (no depth write). Useful for tool
//! overlays - voxel outlines, gizmo arms, debug bounds.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct Debug3DVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

const INITIAL_CAPACITY: u64 = 1024;

pub(crate) struct Debug3DPass {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    capacity: u64,
}

impl Debug3DPass {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("infengine_render::debug3d_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("debug3d.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("infengine_render::debug3d_pipeline_layout"),
            bind_group_layouts: &[camera_layout],
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
                format: wgpu::VertexFormat::Float32x4,
            },
        ];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("infengine_render::debug3d_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Debug3DVertex>() as wgpu::BufferAddress,
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
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                // Read but don't write depth so the line strokes appear
                // on top of geometry at the same surface without
                // disturbing the depth buffer for later passes.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::debug3d_buffer"),
            size: INITIAL_CAPACITY * std::mem::size_of::<Debug3DVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            buffer,
            capacity: INITIAL_CAPACITY,
        }
    }

    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[Debug3DVertex],
    ) {
        if vertices.is_empty() {
            return;
        }
        let needed = vertices.len() as u64;
        if needed > self.capacity {
            let mut new_cap = self.capacity.max(1);
            while new_cap < needed {
                new_cap *= 2;
            }
            self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("infengine_render::debug3d_buffer"),
                size: new_cap * std::mem::size_of::<Debug3DVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.capacity = new_cap;
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(vertices));
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
