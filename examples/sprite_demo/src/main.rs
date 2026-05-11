//! Sprite demo - Phase 1 proof of concept for InfEngine.
//!
//! Step 1.2: open a window and clear the framebuffer to a solid color
//! every frame via the wgpu-backed renderer. Press `Escape` to exit.

use infengine_render::Renderer;
use infengine_window::{AppHandler, EngineWindow, Key, WindowConfig, run};
use tracing::{error, info, warn};

#[derive(Default)]
struct Demo {
    renderer: Option<Renderer>,
    frame: u64,
}

impl AppHandler for Demo {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        info!(width = w, height = h, "sprite_demo started");
        match Renderer::new(window) {
            Ok(r) => self.renderer = Some(r),
            Err(err) => {
                error!(error = %err, "failed to initialize renderer");
                std::process::exit(2);
            }
        }
    }

    fn on_update(&mut self, _window: &EngineWindow) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        if let Err(err) = renderer.render() {
            warn!(error = ?err, "frame render failed");
        }
        self.frame = self.frame.wrapping_add(1);
        if self.frame.is_multiple_of(120) {
            info!(frame = self.frame, "rendering");
        }
    }

    fn on_key(&mut self, key: Key, pressed: bool) {
        if pressed {
            info!(?key, "key pressed");
        }
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        info!(width, height, "window resized");
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(width, height);
        }
    }

    fn on_exit(&mut self) {
        info!("sprite_demo exiting");
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = WindowConfig {
        title: "InfEngine - sprite_demo (Phase 1.3)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
