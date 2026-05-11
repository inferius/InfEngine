//! Sprite demo - Phase 2.5 of InfEngine.
//!
//! A 40x30 tilemap forms an explorable world. The camera follows the
//! player (WASD or arrow keys) and Q / E zoom out / in. `R` recenters
//! the player and resets the zoom. `Escape` exits.

use infengine_render::{Renderer, SpriteInstance, TileId, Tilemap, demo_atlas};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const PLAYER_SIZE: f32 = 64.0;
const TILE_SIZE: f32 = 64.0;
const TILEMAP_W: u32 = 40;
const TILEMAP_H: u32 = 30;
/// Sprite move speed in world units per second.
const MOVE_SPEED: f32 = 400.0;
const ZOOM_RATE: f32 = 1.6; // per second exponent change
const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 4.0;

struct Demo {
    renderer: Option<Renderer>,
    tilemap: Tilemap,
    pos: [f32; 2],
    zoom: f32,
    frame: u64,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            renderer: None,
            tilemap: build_tilemap(),
            pos: [0.0, 0.0],
            zoom: 1.0,
            frame: 0,
        }
    }
}

fn build_tilemap() -> Tilemap {
    let origin = [
        -(TILEMAP_W as f32) * 0.5 * TILE_SIZE,
        (TILEMAP_H as f32) * 0.5 * TILE_SIZE,
    ];
    let mut map = Tilemap::new(TILEMAP_W, TILEMAP_H, TILE_SIZE, origin);

    for y in 0..TILEMAP_H {
        for x in 0..TILEMAP_W {
            let mut h = x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA77);
            h ^= h >> 13;
            h = h.wrapping_mul(0xC2B2_AE35);
            h ^= h >> 16;
            map.set(x, y, TileId((h % 4) as u8));
        }
    }
    map
}

fn map_half_extent() -> (f32, f32) {
    let half_w = TILEMAP_W as f32 * 0.5 * TILE_SIZE;
    let half_h = TILEMAP_H as f32 * 0.5 * TILE_SIZE;
    (half_w, half_h)
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

        // Zoom via exponential: Q out, E in.
        let mut zoom_step = 0.0_f32;
        if ctx.input.is_pressed(Key::Q) {
            zoom_step -= 1.0;
        }
        if ctx.input.is_pressed(Key::E) {
            zoom_step += 1.0;
        }
        if zoom_step != 0.0 {
            let factor = (zoom_step * ZOOM_RATE * ctx.delta_time).exp();
            self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        }

        if ctx.input.is_pressed(Key::R) {
            self.pos = [0.0, 0.0];
            self.zoom = 1.0;
        }

        // Clamp player to the playable map area, leaving room for the sprite.
        let (half_w, half_h) = map_half_extent();
        let pad = PLAYER_SIZE * 0.5;
        self.pos[0] = self.pos[0].clamp(-(half_w - pad), half_w - pad);
        self.pos[1] = self.pos[1].clamp(-(half_h - pad), half_h - pad);

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        // Drive the camera: follow the player and apply current zoom.
        {
            let camera = renderer.camera_mut();
            camera.position = self.pos;
            camera.zoom = self.zoom;
        }

        renderer.draw_tilemap(&self.tilemap);
        renderer.draw_sprite(
            SpriteInstance::from_region(self.pos, [PLAYER_SIZE, PLAYER_SIZE], demo_atlas::CHECKER)
                .with_color([1.0, 0.85, 0.4, 1.0]),
        );

        let queued = renderer.queued_sprite_count();
        if let Err(err) = renderer.render() {
            warn!(error = ?err, "frame render failed");
        }

        self.frame = self.frame.wrapping_add(1);
        if self.frame.is_multiple_of(120) {
            info!(
                frame = self.frame,
                x = self.pos[0],
                y = self.pos[1],
                zoom = self.zoom,
                dt_ms = ctx.delta_time * 1000.0,
                sprites = queued,
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
        title: "InfEngine - sprite_demo (Phase 2.5)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
