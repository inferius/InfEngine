//! GPU renderer for InfEngine.
//!
//! Phase 1.2: minimal `Renderer` that owns a `wgpu` instance, surface,
//! device, and queue, and can clear the framebuffer to a solid color.
//! Texture / mesh / pipeline support lands in later steps of Phase 1.

#![forbid(unsafe_code)]

mod sprite;

use std::sync::Arc;

use infengine_window::EngineWindow;
use tracing::{debug, info, warn};
use wgpu::{
    Backends, Color, CommandEncoderDescriptor, CompositeAlphaMode, DeviceDescriptor, Instance,
    InstanceDescriptor, LoadOp, MemoryHints, Operations, PowerPreference, PresentMode, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RequestAdapterOptions, StoreOp, Surface,
    SurfaceConfiguration, SurfaceError, TextureUsages, TextureViewDescriptor, Trace,
};

use sprite::SpritePass;

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
    sprite: SpritePass,
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

        let sprite = SpritePass::new(&device, &queue, config.format);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            clear_color,
            sprite,
        })
    }

    /// Update the sprite's transform (offset and scale in NDC).
    ///
    /// In NDC the visible region spans `[-1, 1]` on both axes; `scale` of
    /// `[1, 1]` covers the whole window.
    pub fn set_sprite_transform(&self, offset: [f32; 2], scale: [f32; 2]) {
        self.sprite.set_transform(&self.queue, offset, scale);
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
        debug!(width, height, "surface reconfigured");
    }

    /// Override the clear color used by [`Renderer::render`].
    pub fn set_clear_color(&mut self, r: f64, g: f64, b: f64, a: f64) {
        self.clear_color = Color { r, g, b, a };
    }

    /// Render one frame: acquire the next swapchain image, clear it, and
    /// submit. Returns `Ok(())` on success; surface errors that can be
    /// recovered from (lost, outdated) are handled by reconfiguring.
    pub fn render(&mut self) -> Result<(), SurfaceError> {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                warn!("surface lost or outdated, reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(err) => return Err(err),
        };

        let view = frame.texture.create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("infengine_render::frame_encoder"),
            });

        {
            let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("infengine_render::main_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.sprite.draw(&mut rpass);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
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
