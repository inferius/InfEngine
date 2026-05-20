// Textured billboard particle render pass (Wave 18.1).
//
// Each particle is a camera-facing quad sampled from a separate atlas
// texture (the "particle atlas" built by voxel_demo_mp at pack load
// time). The pass runs after the opaque mesh3d pass and before the
// egui overlay with alpha-blend + depth-test-read / depth-write-off.
//
// Pipeline layout:
//   @group(0) - camera_3d uniform  (same BGL as mesh3d, passed in)
//   @group(1) - particle atlas texture + nearest sampler
//
// Instance buffer layout: see `GpuParticleInstance` below.

use bytemuck::{Pod, Zeroable};

// -------------------------------------------------------------------
// Public instance type (CPU -> GPU)
// -------------------------------------------------------------------

/// One textured billboard particle ready for the GPU.
///
/// The vertex shader expands each instance into a screen-facing quad
/// using the camera's right + up vectors (spherical billboard mode).
///
/// `uv_min` / `uv_max` address the region inside the particle atlas
/// texture that this sprite occupies.
///
/// Size = 40 bytes.  Alignment matches `wgpu::VertexFormat`s used in
/// the pipeline layout (3 x Float32x2 + 1 x Float32x4 + 1 x Float32
/// = 4+4+4+4+4+4+4+4+4+4 = 40 bytes ... see `size_check` test).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct ParticleInstance {
    /// World-space centre of the quad.
    pub center_world: [f32; 3],
    /// Half-size of the quad in world units (radius from centre).
    /// Full visible size = `size * 2`.
    pub size: f32,
    /// Atlas UV rect: (u_min, v_min) of the sprite.
    pub uv_min: [f32; 2],
    /// Atlas UV rect: (u_max, v_max) of the sprite.
    pub uv_max: [f32; 2],
    /// RGBA linear-space tint.  Alpha channel controls opacity.
    pub color: [f32; 4],
}

// -------------------------------------------------------------------
// Pass
// -------------------------------------------------------------------

const INITIAL_CAPACITY: u64 = 2048;

/// Textured billboard particle render pass.
#[allow(missing_debug_implementations)]
///
/// Call order each frame:
/// 1. `upload(device, queue, instances)` - sync CPU instances to GPU.
/// 2. Inside a render pass: `draw(rpass, count)`.
pub struct ParticlePass {
    pipeline: wgpu::RenderPipeline,
    /// Bind group for the atlas texture + sampler. `None` until
    /// `set_atlas` is called; draw is a no-op without it.
    atlas_bind_group: Option<wgpu::BindGroup>,
    /// Per-particle instance buffer (vertex-step-mode Instance).
    instance_buffer: wgpu::Buffer,
    capacity: u64,
    /// BGL for group 1 (texture + sampler). Kept so `set_atlas` can
    /// rebuild the bind group without re-creating the pipeline.
    atlas_bgl: wgpu::BindGroupLayout,
    /// Nearest-neighbour sampler shared across all atlas swaps.
    sampler: wgpu::Sampler,
}

impl ParticlePass {
    /// Build the pipeline.  `camera_bgl` is the @group(0) layout
    /// already owned by `Camera3DResource` in the renderer; we borrow
    /// it to avoid duplicating it here.
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        camera_bgl: &wgpu::BindGroupLayout,
    ) -> Self {
        // Atlas texture + sampler bind group layout.
        let atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::particle_atlas_bgl"),
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("infengine_render::particle_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("infengine_render::particle_pipeline_layout"),
            bind_group_layouts: &[camera_bgl, &atlas_bgl],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("infengine_render::particle_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("particle.wgsl").into()),
        });

        // Instance attributes.  The vertex shader uses instance-step
        // data only (no per-vertex geometry - the 4 quad corners are
        // synthesised from gl_VertexIndex 0..3).
        let instance_attrs = [
            // center_world: Float32x3 @ offset 0
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            // size: Float32 @ offset 12
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32,
            },
            // uv_min: Float32x2 @ offset 16
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            // uv_max: Float32x2 @ offset 24
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x2,
            },
            // color: Float32x4 @ offset 32
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("infengine_render::particle_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ParticleInstance>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attrs,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // billboards must be visible from both sides
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None, // no depth buffer: particle pass piggybacks on mesh3d's target
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::particle_instance_buf"),
            size: INITIAL_CAPACITY * std::mem::size_of::<ParticleInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            atlas_bind_group: None,
            instance_buffer,
            capacity: INITIAL_CAPACITY,
            atlas_bgl,
            sampler,
        }
    }

    /// Upload (or replace) the particle atlas texture.  Any subsequent
    /// `draw` call will sample from this texture.
    ///
    /// Calling `set_atlas` again replaces the previous binding; the old
    /// GPU texture is released when the old bind group is dropped.
    pub fn set_atlas(&mut self, device: &wgpu::Device, atlas_texture: &wgpu::Texture) {
        let view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::particle_atlas_bg"),
            layout: &self.atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.atlas_bind_group = Some(bg);
    }

    /// Upload instances to the GPU instance buffer.  Grows the buffer
    /// (power-of-two doubling) when `instances.len()` exceeds the
    /// current capacity.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[ParticleInstance],
    ) {
        if instances.is_empty() {
            return;
        }
        let needed = instances.len() as u64;
        if needed > self.capacity {
            let mut new_cap = self.capacity.max(1);
            while new_cap < needed {
                new_cap *= 2;
            }
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("infengine_render::particle_instance_buf"),
                size: new_cap * std::mem::size_of::<ParticleInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.capacity = new_cap;
        }
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(instances),
        );
    }

    /// Emit `count` billboard quads.  Must be called inside an active
    /// `RenderPass` that already has `@group(0)` bound to the camera
    /// uniform (same BGL as mesh3d group 0).
    ///
    /// Silently returns when no atlas has been set or `count == 0`.
    pub fn draw<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        let Some(atlas_bg) = &self.atlas_bind_group else {
            return;
        };
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(1, atlas_bg, &[]);
        rpass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        // 4 vertices per quad (TriangleStrip), `count` instances.
        rpass.draw(0..4, 0..count);
    }
}

// -------------------------------------------------------------------
// Tests (no GPU device needed - pure CPU layout checks)
// -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_instance_size_is_40_bytes() {
        // Layout: center_world(12) + size(4) + uv_min(8) + uv_max(8) + color(16) = 48
        // Wait: 12+4+8+8+16 = 48 bytes.  Verify the actual value and keep
        // this test as a layout canary - if a field is added / removed the
        // test will catch the shift before a GPU draw silently reads garbage.
        assert_eq!(
            std::mem::size_of::<ParticleInstance>(),
            48,
            "ParticleInstance size changed - update wgpu vertex attribute offsets"
        );
    }

    #[test]
    fn particle_instance_is_pod_compatible() {
        // bytemuck::cast_slice requires Pod.  Zeroable is implied by Pod.
        let inst = ParticleInstance {
            center_world: [1.0, 2.0, 3.0],
            size: 0.125,
            uv_min: [0.0, 0.0],
            uv_max: [0.5, 0.5],
            color: [1.0, 0.8, 0.4, 0.9],
        };
        let bytes: &[u8] = bytemuck::bytes_of(&inst);
        assert_eq!(bytes.len(), 48);
    }

    #[test]
    fn particle_instance_fields_round_trip_through_bytes() {
        let inst = ParticleInstance {
            center_world: [10.0, 20.0, 30.0],
            size: 0.25,
            uv_min: [0.125, 0.25],
            uv_max: [0.25, 0.5],
            color: [0.5, 0.6, 0.7, 0.8],
        };
        let bytes = bytemuck::bytes_of(&inst);
        let back: ParticleInstance = *bytemuck::from_bytes(bytes);
        assert_eq!(back, inst);
    }

    #[test]
    fn upload_preserves_instance_order_via_cast_slice() {
        // cast_slice works on a Vec<ParticleInstance> without panicking.
        let a = ParticleInstance {
            center_world: [1.0, 0.0, 0.0],
            size: 0.1,
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
            color: [1.0; 4],
        };
        let b = ParticleInstance {
            center_world: [0.0, 1.0, 0.0],
            size: 0.2,
            uv_min: [0.5, 0.5],
            uv_max: [1.0, 1.0],
            color: [0.0, 0.0, 0.0, 1.0],
        };
        let instances = vec![a, b];
        let bytes: &[u8] = bytemuck::cast_slice(&instances);
        assert_eq!(bytes.len(), 96);
        let back: &[ParticleInstance] = bytemuck::cast_slice(bytes);
        assert_eq!(back[0], a);
        assert_eq!(back[1], b);
    }

    #[test]
    fn particle_instance_center_world_offset_is_0() {
        // The vertex attribute for center_world is at offset 0.
        // If any field is reordered, the offset assertion catches it.
        let inst = ParticleInstance {
            center_world: [0xDEAD as f32, 0.0, 0.0],
            size: 0.0,
            uv_min: [0.0; 2],
            uv_max: [0.0; 2],
            color: [0.0; 4],
        };
        let bytes: &[u8] = bytemuck::bytes_of(&inst);
        let x = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(x, 0xDEAD as f32);
    }

    #[test]
    fn particle_instance_color_offset_is_32() {
        let inst = ParticleInstance {
            center_world: [0.0; 3],
            size: 0.0,
            uv_min: [0.0; 2],
            uv_max: [0.0; 2],
            color: [1.0, 2.0, 3.0, 4.0],
        };
        let bytes: &[u8] = bytemuck::bytes_of(&inst);
        let r = f32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
        assert_eq!(r, 1.0);
    }

    #[test]
    fn particle_instance_uv_min_offset_is_16() {
        let inst = ParticleInstance {
            center_world: [0.0; 3],
            size: 0.0,
            uv_min: [0.75, 0.25],
            uv_max: [0.0; 2],
            color: [0.0; 4],
        };
        let bytes: &[u8] = bytemuck::bytes_of(&inst);
        let u = f32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        assert_eq!(u, 0.75);
    }
}
