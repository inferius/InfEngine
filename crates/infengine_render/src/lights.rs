//! Point-light uniform buffer.
//!
//! The mesh3d shader samples a small uniform array of point lights.
//! Up to `MAX_LIGHTS` are uploaded per frame; `count` tells the
//! shader how many entries are live. Each light: `position` (xyz +
//! pad), `color` (rgb + intensity), `params` (range, _, _, _).

use bytemuck::{Pod, Zeroable};

pub(crate) const MAX_LIGHTS: usize = 8;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct PointLight {
    pub position: [f32; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            position: [0.0; 4],
            color: [0.0; 4],
            params: [1.0, 0.0, 0.0, 0.0],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct LightsUniform {
    pub count: [u32; 4],
    pub lights: [PointLight; MAX_LIGHTS],
}

pub(crate) struct LightsResource {
    pub buffer: wgpu::Buffer,
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

impl LightsResource {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::util::DeviceExt;
        let initial = LightsUniform {
            count: [0; 4],
            lights: [PointLight::default(); MAX_LIGHTS],
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("infengine_render::lights_uniform"),
            contents: bytemuck::bytes_of(&initial),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("infengine_render::lights_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("infengine_render::lights_bg"),
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

    pub fn upload(&self, queue: &wgpu::Queue, lights: &[PointLight], count: usize) {
        let mut uni = LightsUniform {
            count: [count.min(MAX_LIGHTS) as u32, 0, 0, 0],
            lights: [PointLight::default(); MAX_LIGHTS],
        };
        for (i, l) in lights.iter().take(MAX_LIGHTS).enumerate() {
            uni.lights[i] = *l;
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uni));
    }
}
