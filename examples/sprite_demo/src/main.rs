//! Sprite demo - Phase 2.2 of InfEngine.
//!
//! Demonstrates instanced sprite rendering: the player sprite plus four
//! static "decoration" sprites at the screen corners, all drawn in a
//! single batched draw call. WASD / arrow keys move the player; `R`
//! recenters; `Escape` (or closing the window) exits.

use infengine_render::{Renderer, SpriteInstance};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const PLAYER_SIZE: f32 = 128.0;
const DECO_SIZE: f32 = 96.0;
const DECO_MARGIN: f32 = 80.0;
/// Sprite move speed in world units per second (1 unit = 1 logical pixel).
const MOVE_SPEED: f32 = 400.0;

struct Demo {
    renderer: Option<Renderer>,
    pos: [f32; 2],
    viewport: (u32, u32),
    frame: u64,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            renderer: None,
            pos: [0.0, 0.0],
            viewport: (1, 1),
            frame: 0,
        }
    }
}

impl Demo {
    fn corner_decorations(&self) -> [SpriteInstance; 4] {
        let half_w = self.viewport.0 as f32 * 0.5 - DECO_MARGIN;
        let half_h = self.viewport.1 as f32 * 0.5 - DECO_MARGIN;
        let mk = |x: f32, y: f32, tint: [f32; 4]| SpriteInstance {
            position: [x, y],
            size: [DECO_SIZE, DECO_SIZE],
            color: tint,
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
        };
        [
            mk(-half_w, half_h, [1.0, 0.5, 0.5, 1.0]), // top-left, red tint
            mk(half_w, half_h, [0.5, 1.0, 0.5, 1.0]),  // top-right, green tint
            mk(-half_w, -half_h, [0.5, 0.5, 1.0, 1.0]), // bottom-left, blue tint
            mk(half_w, -half_h, [1.0, 1.0, 0.5, 1.0]), // bottom-right, yellow tint
        ]
    }
}

impl AppHandler for Demo {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, "sprite_demo started");
        match Renderer::new(window) {
            Ok(r) => self.renderer = Some(r),
            Err(err) => {
                error!(error = %err, "failed to initialize renderer");
                std::process::exit(2);
            }
        }
    }

    fn on_update(&mut self, _window: &EngineWindow, ctx: &FrameContext<'_>) {
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
            self.pos[0] += dx * MOVE_SPEED * ctx.delta_time;
            self.pos[1] += dy * MOVE_SPEED * ctx.delta_time;
        }

        if ctx.input.is_pressed(Key::R) {
            self.pos = [0.0, 0.0];
        }

        let half_w = self.viewport.0 as f32 * 0.5 - PLAYER_SIZE * 0.5;
        let half_h = self.viewport.1 as f32 * 0.5 - PLAYER_SIZE * 0.5;
        self.pos[0] = self.pos[0].clamp(-half_w, half_w);
        self.pos[1] = self.pos[1].clamp(-half_h, half_h);

        let decorations = self.corner_decorations();

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        for deco in decorations {
            renderer.draw_sprite(deco);
        }
        renderer.draw_sprite(SpriteInstance::new(self.pos, [PLAYER_SIZE, PLAYER_SIZE]));

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
        self.viewport = (width, height);
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
        title: "InfEngine - sprite_demo (Phase 2.2)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
