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

/// Vertex format used by `Mesh3DPass`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

impl MeshVertex {
    pub fn new(position: [f32; 3], normal: [f32; 3]) -> Self {
        Self { position, normal }
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
}

impl Mesh3DRegistry {
    pub fn insert(
        &mut self,
        device: &wgpu::Device,
        label: &str,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> MeshHandle {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(indices),
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

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, corners) in faces {
        let base = vertices.len() as u16;
        for c in corners {
            vertices.push(MeshVertex {
                position: c,
                normal,
            });
        }
        // Two triangles per face: 0-1-2, 0-2-3.
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
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

/// Pipeline + shared instance buffer for batched mesh draws.
pub(crate) struct Mesh3DPass {
    pipeline: wgpu::RenderPipeline,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
}

impl Mesh3DPass {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_layout: &wgpu::BindGroupLayout,
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("infengine_render::mesh3d_pipeline_layout"),
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
                format: wgpu::VertexFormat::Float32x3,
            },
        ];

        let instance_attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 48,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 64,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 80,
                shader_location: 7,
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

        Self {
            pipeline,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
        }
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
    ) {
        if batches.is_empty() {
            return;
        }
        rpass.set_pipeline(&self.pipeline);
        rpass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        for batch in batches {
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
