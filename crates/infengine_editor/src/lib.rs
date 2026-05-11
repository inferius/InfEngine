//! egui-based editor / debug UI integration for InfEngine.
//!
//! Phase 4.1 introduces a minimal wrapper around `egui`, `egui-wgpu`,
//! and `egui-winit` so engine code can build dev-tool panels without
//! reimplementing the event plumbing.
//!
//! Usage from an application:
//!
//! ```ignore
//! let mut editor = EditorUi::new(&device, format, &window, scale_factor);
//! // Each window event:
//! editor.handle_window_event(&window, &event);
//! // Each frame:
//! editor.run(&window, |ctx| { /* egui UI here */ });
//! editor.render(&device, &queue, &mut encoder, &surface_view, surface_size);
//! ```

#![forbid(unsafe_code)]

use egui::ViewportId;
use egui_wgpu::ScreenDescriptor;
use infengine_window::EngineWindow;
use infengine_window::raw_winit::event::WindowEvent;
use infengine_window::raw_winit::window::Window as WinitWindow;
use tracing::info;

/// All state the engine needs to drive an egui frame and render it
/// through `wgpu`.
pub struct EditorUi {
    context: egui::Context,
    winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    last_full_output: Option<egui::FullOutput>,
    last_logical_size: [u32; 2],
}

impl std::fmt::Debug for EditorUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorUi").finish()
    }
}

impl EditorUi {
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        window: &WinitWindow,
        msaa_samples: u32,
    ) -> Self {
        let context = egui::Context::default();
        let viewport_id = ViewportId::ROOT;
        let winit_state = egui_winit::State::new(
            context.clone(),
            viewport_id,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );
        let renderer = egui_wgpu::Renderer::new(
            device,
            target_format,
            egui_wgpu::RendererOptions {
                msaa_samples,
                ..Default::default()
            },
        );
        let size = window.inner_size();
        info!(
            width = size.width,
            height = size.height,
            "editor UI initialized"
        );
        Self {
            context,
            winit_state,
            renderer,
            last_full_output: None,
            last_logical_size: [size.width.max(1), size.height.max(1)],
        }
    }

    /// Forward a winit `WindowEvent` to egui-winit. Call this from the
    /// `AppHandler::on_window_event` hook.
    pub fn handle_window_event(&mut self, window: &WinitWindow, event: &WindowEvent) {
        let _ = self.winit_state.on_window_event(window, event);
    }

    /// Build a new egui frame using `ui_fn` and store the result for the
    /// next call to [`Self::render`].
    pub fn run(&mut self, window: &WinitWindow, ui_fn: impl FnMut(&egui::Context)) {
        let raw_input = self.winit_state.take_egui_input(window);
        let mut ui_fn = ui_fn;
        let full_output = self.context.run(raw_input, |ctx| ui_fn(ctx));
        self.winit_state
            .handle_platform_output(window, full_output.platform_output.clone());
        let size = window.inner_size();
        self.last_logical_size = [size.width.max(1), size.height.max(1)];
        self.last_full_output = Some(full_output);
    }

    /// Read-only egui context, for code that wants to query state outside
    /// of the `run` closure (e.g. theme/style).
    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    /// Render the most recently built egui frame into the supplied
    /// surface view.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        physical_size: [u32; 2],
    ) {
        let Some(output) = self.last_full_output.take() else {
            return;
        };
        let pixels_per_point = output.pixels_per_point;
        let clipped_primitives = self.context.tessellate(output.shapes, pixels_per_point);
        for (id, image_delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }
        let screen = ScreenDescriptor {
            size_in_pixels: physical_size,
            pixels_per_point,
        };
        let _ = self
            .renderer
            .update_buffers(device, queue, encoder, &clipped_primitives, &screen);
        {
            let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("infengine_editor::egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer
                .render(&mut rpass.forget_lifetime(), &clipped_primitives, &screen);
        }
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }

    /// Convenience: extract the underlying `winit::Window` reference from
    /// an `EngineWindow` and forward an event.
    pub fn handle_engine_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        self.handle_window_event(window.raw(), event);
    }
}
