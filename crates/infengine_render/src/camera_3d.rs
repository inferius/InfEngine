//! 3D perspective camera and its GPU resource.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

/// Right-handed perspective camera.
#[derive(Debug, Clone)]
pub struct Camera3D {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y_rad: f32,
    pub aspect: f32,
    pub z_near: f32,
    pub z_far: f32,
}

impl Camera3D {
    pub fn new(viewport_w: u32, viewport_h: u32) -> Self {
        Self {
            position: Vec3::new(4.0, 3.0, 6.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_y_rad: 60_f32.to_radians(),
            aspect: viewport_w.max(1) as f32 / viewport_h.max(1) as f32,
            z_near: 0.1,
            z_far: 200.0,
        }
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.aspect = width.max(1) as f32 / height.max(1) as f32;
    }

    pub fn view_projection(&self) -> Mat4 {
        let proj = Mat4::perspective_rh(self.fov_y_rad, self.aspect, self.z_near, self.z_far);
        let view = Mat4::look_at_rh(self.position, self.target, self.up);
        proj * view
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct Camera3DUniform {
    view_proj: [[f32; 4]; 4],
    view_proj_inv: [[f32; 4]; 4],
    camera_pos: [f32; 4],
}

impl Camera3DUniform {
    fn from_camera(c: &Camera3D) -> Self {
        let vp = c.view_projection();
        Self {
            view_proj: vp.to_cols_array_2d(),
            view_proj_inv: vp.inverse().to_cols_array_2d(),
            camera_pos: [c.position.x, c.position.y, c.position.z, 1.0],
        }
    }
}

pub(crate) struct Camera3DResource {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) layout: wgpu::BindGroupLayout,
    pub(crate) bind_group: wgpu::BindGroup,
}

impl Camera3DResource {
    pub fn new(device: &wgpu::Device, camera: &Camera3D) -> Self {
        let uniform = Camera3DUniform::from_camera(camera);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("infengine_render::camera_3d_uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::camera_3d_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::camera_3d_bg"),
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

    pub fn upload(&self, queue: &wgpu::Queue, camera: &Camera3D) {
        let uniform = Camera3DUniform::from_camera(camera);
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uniform));
    }
}
