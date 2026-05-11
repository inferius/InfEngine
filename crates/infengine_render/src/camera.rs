//! 2D orthographic camera and its GPU resource.
//!
//! The camera maps a world-space coordinate system (1 unit = 1 logical
//! pixel at `zoom == 1.0`) onto wgpu's clip space. `position` is the
//! world coordinate at the center of the screen; `zoom > 1.0` makes the
//! world appear larger.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, vec3};
use wgpu::util::DeviceExt;

/// CPU-side 2D camera state.
#[derive(Debug, Clone)]
pub struct Camera2D {
    pub position: [f32; 2],
    pub zoom: f32,
    viewport: [u32; 2],
}

impl Camera2D {
    pub fn new(viewport_w: u32, viewport_h: u32) -> Self {
        Self {
            position: [0.0, 0.0],
            zoom: 1.0,
            viewport: [viewport_w.max(1), viewport_h.max(1)],
        }
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.viewport = [width.max(1), height.max(1)];
    }

    pub fn viewport(&self) -> (u32, u32) {
        (self.viewport[0], self.viewport[1])
    }

    /// Combined view-projection matrix. `proj` is a right-handed
    /// orthographic frustum spanning `viewport / zoom` in world units
    /// centered on the origin; `view` translates the world so that the
    /// camera position sits at the origin.
    pub fn view_projection(&self) -> Mat4 {
        let half_w = self.viewport[0] as f32 * 0.5 / self.zoom;
        let half_h = self.viewport[1] as f32 * 0.5 / self.zoom;
        let proj = Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, -1.0, 1.0);
        let view = Mat4::from_translation(vec3(-self.position[0], -self.position[1], 0.0));
        proj * view
    }
}

/// 16-byte-aligned GPU layout of a 4x4 matrix.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    fn from_matrix(m: Mat4) -> Self {
        Self {
            view_proj: m.to_cols_array_2d(),
        }
    }
}

/// GPU resources backing the camera: a uniform buffer, its bind group
/// layout, and a bind group ready to be bound at `@group(0)`.
pub(crate) struct CameraResource {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) layout: wgpu::BindGroupLayout,
    pub(crate) bind_group: wgpu::BindGroup,
}

impl CameraResource {
    pub fn new(device: &wgpu::Device, camera: &Camera2D) -> Self {
        let uniform = CameraUniform::from_matrix(camera.view_projection());
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("infengine_render::camera_uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::camera_bgl"),
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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::camera_bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self {
            buffer,
            layout,
            bind_group,
        }
    }

    pub fn upload(&self, queue: &wgpu::Queue, camera: &Camera2D) {
        let uniform = CameraUniform::from_matrix(camera.view_projection());
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uniform));
    }
}
