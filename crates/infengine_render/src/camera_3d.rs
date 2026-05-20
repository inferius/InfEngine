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
    /// Wall-clock seconds since app start. Uploaded into the
    /// camera uniform so shaders (animated water, eventually
    /// fire / grass sway, etc.) can drive procedural animation.
    /// The host is expected to refresh this every frame.
    pub time_seconds: f32,
    /// Phase L (Wave 3): sky-light multiplier in `[0, 1]` keyed
    /// by the host's day clock. The mesh3d shader reads it as
    /// `time.y` of the camera uniform and uses
    /// `effective_light = max(block_light, sky_light * day_factor)`
    /// to dim outdoor cells at night while keeping torch-lit cells
    /// bright. Defaults to 1.0 (full daylight); hosts without a
    /// day cycle can leave it alone.
    pub day_factor: f32,
    /// W30.5: Distance at which fog starts blending (world units).
    /// Fragments closer than this distance are not fogged.
    /// Default: 0.0 (no fog).
    pub fog_start: f32,
    /// W30.5: Distance at which fog reaches full opacity (world units).
    /// Default: 0.0 (no fog - same as fog_start means no visible fog).
    pub fog_end: f32,
    /// W30.5: Sky / fog color used for the fog blend. Linear RGB, matches
    /// the renderer's clear_color. Default: (0, 0, 0) (fades to black, set
    /// via Renderer::set_fog to match the actual sky color).
    pub sky_color: [f32; 3],
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
            time_seconds: 0.0,
            day_factor: 1.0,
            fog_start: 0.0,
            fog_end: 0.0,
            sky_color: [0.0, 0.0, 0.0],
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
    /// `(time_seconds, day_factor, 0, 0)`. Slot kept as a vec4 so
    /// the struct stays 16-byte aligned. `time.y` carries the
    /// Phase L sky-light multiplier; remaining lanes reserved
    /// for future "global" shader inputs (sky tint, wind
    /// strength, ...).
    time: [f32; 4],
    /// W30.5: `(fog_start, fog_end, 0, 0)` in world units.
    /// `fog_start` = distance where fog begins blending in.
    /// `fog_end` = distance where fog reaches full opacity.
    /// fog_start == fog_end == 0 means no fog (disabled).
    fog: [f32; 4],
    /// W30.5: `(r, g, b, 1)` sky/fog colour in linear RGB.
    sky_color: [f32; 4],
}

impl Camera3DUniform {
    fn from_camera(c: &Camera3D) -> Self {
        let vp = c.view_projection();
        Self {
            view_proj: vp.to_cols_array_2d(),
            view_proj_inv: vp.inverse().to_cols_array_2d(),
            camera_pos: [c.position.x, c.position.y, c.position.z, 1.0],
            time: [c.time_seconds, c.day_factor.clamp(0.0, 1.0), 0.0, 0.0],
            fog: [c.fog_start.max(0.0), c.fog_end.max(0.0), 0.0, 0.0],
            sky_color: [c.sky_color[0], c.sky_color[1], c.sky_color[2], 1.0],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_day_factor_is_one() {
        let c = Camera3D::new(1, 1);
        assert_eq!(c.day_factor, 1.0);
    }

    #[test]
    fn uniform_time_y_carries_day_factor() {
        let mut c = Camera3D::new(1, 1);
        c.day_factor = 0.42;
        let u = Camera3DUniform::from_camera(&c);
        assert!((u.time[1] - 0.42).abs() < 1e-5);
    }

    #[test]
    fn uniform_clamps_day_factor_into_unit_range() {
        let mut c = Camera3D::new(1, 1);
        c.day_factor = 2.5;
        let u = Camera3DUniform::from_camera(&c);
        assert!((u.time[1] - 1.0).abs() < 1e-5);
        c.day_factor = -0.5;
        let u = Camera3DUniform::from_camera(&c);
        assert!((u.time[1] - 0.0).abs() < 1e-5);
    }

    #[test]
    fn uniform_time_x_still_carries_seconds() {
        let mut c = Camera3D::new(1, 1);
        c.time_seconds = 7.5;
        let u = Camera3DUniform::from_camera(&c);
        assert!((u.time[0] - 7.5).abs() < 1e-5);
    }
}
