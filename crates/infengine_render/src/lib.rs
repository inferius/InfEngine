//! GPU renderer for InfEngine.
//!
//! Owns a wgpu surface, device, queue, a 2D orthographic camera, and a
//! sprite pipeline. Phase 2.1 introduces world-space coordinates (1 unit
//! = 1 logical pixel at zoom 1.0); higher steps will add sprite batching,
//! texture atlases, tilemaps, and text.

#![forbid(unsafe_code)]

mod camera;
mod camera_3d;
mod debug;
mod font;
mod mesh3d;
mod shadow;
mod sprite;
mod tilemap;

use std::sync::Arc;

use infengine_window::EngineWindow;
use tracing::{debug, info, warn};
use wgpu::{
    Backends, Color, CommandEncoderDescriptor, CompositeAlphaMode, DeviceDescriptor, Instance,
    InstanceDescriptor, LoadOp, MemoryHints, Operations, PowerPreference, PresentMode, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RequestAdapterOptions, StoreOp, Surface,
    SurfaceConfiguration, SurfaceError, TextureUsages, TextureViewDescriptor, Trace,
};

pub use camera::Camera2D;
use camera::CameraResource;
pub use camera_3d::Camera3D;
use camera_3d::Camera3DResource;
use debug::{DebugPass, DebugVertex};
use mesh3d::{Mesh3DPass, Mesh3DRegistry};
pub use mesh3d::{MeshHandle, MeshInstance, MeshVertex, cube_geometry_public};
use shadow::ShadowPass;
use sprite::SpritePass;
pub use sprite::{SpriteInstance, demo_atlas};
pub use tilemap::{TileId, Tilemap};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Errors that can occur while creating or running the renderer.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("failed to create wgpu surface: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("no compatible GPU adapter was found")]
    NoAdapter,
    #[error("failed to request wgpu device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("surface has no compatible configuration on this adapter")]
    NoSurfaceConfig,
}

/// Minimal wgpu-backed renderer.
///
/// Owns the GPU resources required to present frames to a single window.
/// Constructed via [`Renderer::new`] after the engine window has been
/// created (typically inside `AppHandler::on_start`).
pub struct Renderer {
    surface: Surface<'static>,
    device: wgpu::Device,
    queue: Queue,
    config: SurfaceConfiguration,
    clear_color: Color,
    camera: Camera2D,
    camera_resource: CameraResource,
    camera_3d: Camera3D,
    camera_3d_resource: Camera3DResource,
    sprite: SpritePass,
    sprite_queue: Vec<SpriteInstance>,
    debug: DebugPass,
    debug_vertices: Vec<DebugVertex>,
    mesh3d: Mesh3DPass,
    mesh3d_queue: Vec<(MeshHandle, MeshInstance)>,
    meshes: Mesh3DRegistry,
    cube_handle: MeshHandle,
    depth_view: wgpu::TextureView,
    shadow: ShadowPass,
}

impl Renderer {
    /// Create a renderer bound to the given engine window.
    ///
    /// Performs blocking GPU initialization via `pollster`. Should be
    /// called once at startup; subsequent resizes happen through
    /// [`Renderer::resize`].
    pub fn new(window: &EngineWindow) -> Result<Self, RendererError> {
        let (width, height) = window.size();
        let raw: Arc<winit::window::Window> = Arc::clone(window.raw());

        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance.create_surface(raw)?;

        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|_| RendererError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        info!(
            backend = ?adapter_info.backend,
            name = %adapter_info.name,
            "selected GPU adapter"
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("infengine_render::device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: MemoryHints::Performance,
            trace: Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::default(),
        }))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or(RendererError::NoSurfaceConfig)?;

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);

        info!(
            width = config.width,
            height = config.height,
            format = ?config.format,
            present_mode = ?config.present_mode,
            "renderer initialized"
        );

        // Default clear color: muted teal so the demo proves rendering works.
        let clear_color = Color {
            r: 0.05,
            g: 0.10,
            b: 0.18,
            a: 1.0,
        };

        let camera = Camera2D::new(config.width, config.height);
        let camera_resource = CameraResource::new(&device, &camera);
        let camera_3d = Camera3D::new(config.width, config.height);
        let camera_3d_resource = Camera3DResource::new(&device, &camera_3d);
        let sprite = SpritePass::new(&device, &queue, config.format, &camera_resource.layout);
        let debug = DebugPass::new(&device, config.format, &camera_resource.layout);
        let shadow = ShadowPass::new(&device);
        let mesh3d = Mesh3DPass::new(
            &device,
            &queue,
            config.format,
            DEPTH_FORMAT,
            &camera_3d_resource.layout,
            &shadow.sample_bgl,
        );
        let mut meshes = Mesh3DRegistry::default();
        let (cube_v, cube_i) = mesh3d::cube_geometry_public();
        let cube_handle = meshes.insert(&device, "infengine_render::cube", &cube_v, &cube_i);
        let depth_view = create_depth_view(&device, config.width, config.height);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            clear_color,
            camera,
            camera_resource,
            camera_3d,
            camera_3d_resource,
            sprite,
            sprite_queue: Vec::new(),
            debug,
            debug_vertices: Vec::new(),
            mesh3d,
            mesh3d_queue: Vec::new(),
            meshes,
            cube_handle,
            depth_view,
            shadow,
        })
    }

    /// Queue a debug line in world space. `color` is RGBA in linear range.
    pub fn debug_line(&mut self, a: [f32; 2], b: [f32; 2], color: [f32; 4]) {
        self.debug_vertices.push(DebugVertex { position: a, color });
        self.debug_vertices.push(DebugVertex { position: b, color });
    }

    /// Queue a debug axis-aligned rectangle outline.
    pub fn debug_rect(&mut self, min: [f32; 2], max: [f32; 2], color: [f32; 4]) {
        let tl = [min[0], max[1]];
        let tr = [max[0], max[1]];
        let br = [max[0], min[1]];
        let bl = [min[0], min[1]];
        self.debug_line(tl, tr, color);
        self.debug_line(tr, br, color);
        self.debug_line(br, bl, color);
        self.debug_line(bl, tl, color);
    }

    /// Queue a debug circle approximated by `segments` line segments.
    /// `segments` is clamped to at least 3.
    pub fn debug_circle(&mut self, center: [f32; 2], radius: f32, color: [f32; 4], segments: u32) {
        let segments = segments.max(3);
        let step = std::f32::consts::TAU / segments as f32;
        let mut prev = [center[0] + radius, center[1]];
        for i in 1..=segments {
            let a = step * i as f32;
            let cur = [center[0] + radius * a.cos(), center[1] + radius * a.sin()];
            self.debug_line(prev, cur, color);
            prev = cur;
        }
    }

    /// Queue a sprite to be drawn during the next `render()` call. The
    /// queue is cleared after each frame, so call this every frame for
    /// every sprite that should appear.
    pub fn draw_sprite(&mut self, instance: SpriteInstance) {
        self.sprite_queue.push(instance);
    }

    /// Queue a 3D cube draw for the next frame. `model` is a column-major
    /// 4x4 transform (use `glam::Mat4::to_cols_array`).
    pub fn draw_cube(&mut self, model: [f32; 16], color: [f32; 4]) {
        self.draw_mesh(self.cube_handle, model, color);
    }

    /// Queue an arbitrary registered mesh for the next frame.
    pub fn draw_mesh(&mut self, handle: MeshHandle, model: [f32; 16], color: [f32; 4]) {
        self.mesh3d_queue
            .push((handle, MeshInstance::new(model, color)));
    }

    /// Queue an arbitrary registered mesh with PBR material parameters
    /// (metallic in `[0, 1]`, roughness in `[0.04, 1]`).
    pub fn draw_mesh_pbr(
        &mut self,
        handle: MeshHandle,
        model: [f32; 16],
        color: [f32; 4],
        metallic: f32,
        roughness: f32,
    ) {
        let instance = MeshInstance::new(model, color).with_material(metallic, roughness);
        self.mesh3d_queue.push((handle, instance));
    }

    /// Register a new mesh and return its handle. The caller can then
    /// reference the mesh via [`Renderer::draw_mesh`].
    pub fn create_mesh(
        &mut self,
        name: &str,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> MeshHandle {
        self.meshes.insert(&self.device, name, vertices, indices)
    }

    /// Handle of the built-in unit cube.
    pub fn cube_handle(&self) -> MeshHandle {
        self.cube_handle
    }

    /// Replace the global albedo texture used by all 3D mesh draws.
    /// `rgba` must be `width * height * 4` bytes in `Rgba8UnormSrgb`
    /// order. Phase 10.2 keeps this engine-wide (one texture shared by
    /// every mesh); per-material slots come later.
    pub fn set_mesh_albedo(&mut self, width: u32, height: u32, rgba: &[u8]) {
        self.mesh3d
            .set_albedo(&self.device, &self.queue, width, height, rgba);
    }

    /// Mutable access to the 3D camera. Edits take effect on the next
    /// `render()` call.
    pub fn camera_3d_mut(&mut self) -> &mut Camera3D {
        &mut self.camera_3d
    }

    /// Queue every non-empty tile of `map` as a sprite. Convenience
    /// wrapper around `Tilemap::build_instances`.
    pub fn draw_tilemap(&mut self, map: &Tilemap) {
        map.build_instances(&mut self.sprite_queue);
    }

    /// Queue ASCII text for rendering. `position` is the world-space
    /// top-left of the first glyph; `pixel_size` is the world-unit
    /// height of one glyph cell (8 pixels in the font); `color` tints
    /// the glyphs. Characters outside ASCII 32..127 render as blanks.
    pub fn draw_text(&mut self, text: &str, position: [f32; 2], pixel_size: f32, color: [f32; 4]) {
        let glyph_w = pixel_size;
        let glyph_h = pixel_size;
        // Glyphs are 5px wide inside an 8px cell. Advance by 6px so
        // letters do not crowd each other.
        let advance = pixel_size * 6.0 / 8.0;
        let mut cursor_x = position[0];
        let cursor_y_center = position[1] - glyph_h * 0.5;
        for ch in text.chars() {
            let ascii = if ch.is_ascii() { ch as u8 } else { b' ' };
            let region = sprite::glyph_uv(ascii);
            let inst = SpriteInstance {
                position: [cursor_x + glyph_w * 0.5, cursor_y_center],
                size: [glyph_w, glyph_h],
                color,
                uv_min: region.0,
                uv_max: region.1,
            };
            self.sprite_queue.push(inst);
            cursor_x += advance;
        }
    }

    /// Number of sprites currently queued for the next frame (including
    /// tilemap tiles).
    pub fn queued_sprite_count(&self) -> usize {
        self.sprite_queue.len()
    }

    /// Mutable access to the 2D camera. Changes take effect on the next
    /// `render()` call (the matrix is re-uploaded then).
    pub fn camera_mut(&mut self) -> &mut Camera2D {
        &mut self.camera
    }

    /// Access to the underlying wgpu device. Reserved for engine-internal
    /// crates (e.g. `infengine_editor`) that need to create their own
    /// GPU resources sharing the renderer's logical device.
    pub fn wgpu_device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Access to the underlying wgpu queue.
    pub fn wgpu_queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Current swapchain format. Use this when building auxiliary
    /// pipelines that render into the same surface.
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Resize the swapchain to match a new window size in physical pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            debug!("ignoring resize to zero dimensions");
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.camera.set_viewport(width, height);
        self.camera_3d.set_viewport(width, height);
        self.depth_view = create_depth_view(&self.device, width, height);
        debug!(width, height, "surface reconfigured");
    }

    /// Override the clear color used by [`Renderer::render`].
    pub fn set_clear_color(&mut self, r: f64, g: f64, b: f64, a: f64) {
        self.clear_color = Color { r, g, b, a };
    }

    /// Render one frame, with an optional overlay callback.
    ///
    /// The closure runs after the engine's main pass with its own
    /// encoder; it receives the wgpu device, queue, a mutable encoder
    /// borrowed from this frame, the swapchain `TextureView`, and the
    /// physical surface size. Use this to layer UI (e.g. egui) on top
    /// of the world.
    ///
    /// Recoverable surface errors (`Lost`, `Outdated`) trigger a
    /// reconfigure and return `Ok(())`.
    pub fn render<F>(&mut self, mut overlay: F) -> Result<(), SurfaceError>
    where
        F: FnMut(
            &wgpu::Device,
            &wgpu::Queue,
            &mut wgpu::CommandEncoder,
            &wgpu::TextureView,
            [u32; 2],
        ),
    {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                warn!("surface lost or outdated, reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(err) => return Err(err),
        };

        // Upload camera matrices and queued instance data.
        self.camera_resource.upload(&self.queue, &self.camera);
        self.camera_3d_resource.upload(&self.queue, &self.camera_3d);
        // Anchor the sun frustum on the 3D camera's look-at target so
        // the shadow map follows what the user is looking at.
        let target = self.camera_3d.target;
        let light_dir = glam::Vec3::new(0.4, 0.9, 0.5);
        self.shadow
            .upload_light(&self.queue, target, light_dir, 20.0, 40.0);
        self.sprite
            .upload(&self.device, &self.queue, &self.sprite_queue);
        self.debug
            .upload(&self.device, &self.queue, &self.debug_vertices);
        let mesh_batches = self
            .mesh3d
            .upload(&self.device, &self.queue, &self.mesh3d_queue);
        let sprite_count = self.sprite_queue.len() as u32;
        let debug_vertex_count = self.debug_vertices.len() as u32;
        let mesh3d_count = self.mesh3d_queue.len() as u32;

        let view = frame.texture.create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("infengine_render::frame_encoder"),
            });

        // Shadow depth pass first - writes the sun-perspective depth
        // map that the mesh fragment shader samples for occlusion.
        if mesh3d_count > 0 {
            let mut spass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("infengine_render::shadow_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow.depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.shadow.draw(
                &mut spass,
                &self.meshes,
                &mesh_batches,
                self.mesh3d.instance_buffer(),
            );
        }

        // 3D pass next (with depth), then 2D pass on top (no depth).
        if mesh3d_count > 0 {
            let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("infengine_render::mesh3d_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_bind_group(0, &self.camera_3d_resource.bind_group, &[]);
            self.mesh3d.draw(
                &mut rpass,
                &self.meshes,
                &mesh_batches,
                &self.shadow.sample_bg,
            );
        }

        {
            // Clear-or-load the color attachment: if the 3D pass ran it
            // already cleared and populated the surface, otherwise this
            // pass owns the clear.
            let load = if mesh3d_count > 0 {
                LoadOp::Load
            } else {
                LoadOp::Clear(self.clear_color)
            };
            let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("infengine_render::main_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rpass.set_bind_group(0, &self.camera_resource.bind_group, &[]);
            self.sprite.draw(&mut rpass, sprite_count);
            self.debug.draw(&mut rpass, debug_vertex_count);
        }

        let physical_size = [self.config.width, self.config.height];
        overlay(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            physical_size,
        );

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.sprite_queue.clear();
        self.debug_vertices.clear();
        self.mesh3d_queue.clear();
        Ok(())
    }

    /// Current swapchain dimensions in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("size", &(self.config.width, self.config.height))
            .field("format", &self.config.format)
            .field("present_mode", &self.config.present_mode)
            .finish()
    }
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("infengine_render::depth_texture"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
