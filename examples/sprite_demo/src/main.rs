//! Sprite demo - Phase 1 proof of concept for InfEngine.
//!
//! Step 1.1: open a window, log key events. Press `Escape` (or close the
//! window via its title bar) to exit.

use infengine_window::{AppHandler, EngineWindow, Key, WindowConfig, run};
use tracing::info;

#[derive(Default)]
struct Demo {
    frame: u64,
}

impl AppHandler for Demo {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        info!(width = w, height = h, "sprite_demo started");
    }

    fn on_update(&mut self, _window: &EngineWindow) {
        self.frame = self.frame.wrapping_add(1);
        if self.frame.is_multiple_of(600) {
            info!(frame = self.frame, "still alive");
        }
    }

    fn on_key(&mut self, key: Key, pressed: bool) {
        if pressed {
            info!(?key, "key pressed");
        }
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        info!(width, height, "window resized");
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
        title: "InfEngine - sprite_demo (Phase 1.1)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
