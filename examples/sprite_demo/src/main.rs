//! Sprite demo - Phase 1 proof of concept for InfEngine.
//!
//! Step 1.4: the checker sprite can be moved with `WASD` or the arrow
//! keys. `R` resets position. `Escape` (or closing the window) exits.

use infengine_render::Renderer;
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const SPRITE_HALF_SIZE: f32 = 0.25;
const MOVE_SPEED_NDC: f32 = 1.4;

struct Demo {
    renderer: Option<Renderer>,
    pos: [f32; 2],
    frame: u64,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            renderer: None,
            pos: [0.0, 0.0],
            frame: 0,
        }
    }
}

impl AppHandler for Demo {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        info!(width = w, height = h, "sprite_demo started");
        match Renderer::new(window) {
            Ok(r) => {
                r.set_sprite_transform(self.pos, [SPRITE_HALF_SIZE, SPRITE_HALF_SIZE]);
                self.renderer = Some(r);
            }
            Err(err) => {
                error!(error = %err, "failed to initialize renderer");
                std::process::exit(2);
            }
        }
    }

    fn on_update(&mut self, _window: &EngineWindow, ctx: &FrameContext<'_>) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        let mut dx = 0.0_f32;
        let mut dy = 0.0_f32;
        if ctx.input.any_pressed(&[Key::A, Key::ArrowLeft]) {
            dx -= 1.0;
        }
        if ctx.input.any_pressed(&[Key::D, Key::ArrowRight]) {
            dx += 1.0;
        }
        if ctx.input.any_pressed(&[Key::W, Key::ArrowUp]) {
            dy += 1.0;
        }
        if ctx.input.any_pressed(&[Key::S, Key::ArrowDown]) {
            dy -= 1.0;
        }

        if dx != 0.0 || dy != 0.0 {
            let len = (dx * dx + dy * dy).sqrt();
            dx /= len;
            dy /= len;
            self.pos[0] += dx * MOVE_SPEED_NDC * ctx.delta_time;
            self.pos[1] += dy * MOVE_SPEED_NDC * ctx.delta_time;
        }

        if ctx.input.is_pressed(Key::R) {
            self.pos = [0.0, 0.0];
        }

        let limit = 1.0 - SPRITE_HALF_SIZE;
        self.pos[0] = self.pos[0].clamp(-limit, limit);
        self.pos[1] = self.pos[1].clamp(-limit, limit);

        renderer.set_sprite_transform(self.pos, [SPRITE_HALF_SIZE, SPRITE_HALF_SIZE]);

        if let Err(err) = renderer.render() {
            warn!(error = ?err, "frame render failed");
        }

        self.frame = self.frame.wrapping_add(1);
        if self.frame.is_multiple_of(120) {
            info!(
                frame = self.frame,
                x = self.pos[0],
                y = self.pos[1],
                dt_ms = ctx.delta_time * 1000.0,
                "frame"
            );
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
        title: "InfEngine - sprite_demo (Phase 1.4)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
