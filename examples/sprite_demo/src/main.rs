//! Sprite demo - Phase 2.6 of InfEngine.
//!
//! Adds a HUD: bitmap-font text rendered in screen-space (anchored to
//! the camera) on top of the tilemap. Controls: WASD / arrows to move,
//! Q / E to zoom out / in, R to reset, Escape to exit.

use infengine_render::{Renderer, SpriteInstance, TileId, Tilemap, demo_atlas};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const PLAYER_SIZE: f32 = 64.0;
const TILE_SIZE: f32 = 64.0;
const TILEMAP_W: u32 = 40;
const TILEMAP_H: u32 = 30;
const MOVE_SPEED: f32 = 400.0;
const ZOOM_RATE: f32 = 1.6;
const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 4.0;
const HUD_PIXEL_SIZE: f32 = 24.0;
const HUD_MARGIN: f32 = 12.0;

struct Demo {
    renderer: Option<Renderer>,
    tilemap: Tilemap,
    pos: [f32; 2],
    zoom: f32,
    viewport: (u32, u32),
    fps_ema: f32,
    frame: u64,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            renderer: None,
            tilemap: build_tilemap(),
            pos: [0.0, 0.0],
            zoom: 1.0,
            viewport: (1, 1),
            fps_ema: 60.0,
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
        let dt = ctx.delta_time.max(1e-4);
        let instant_fps = 1.0 / dt;
        self.fps_ema = self.fps_ema * 0.9 + instant_fps * 0.1;

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
            self.pos[0] += dx * MOVE_SPEED * dt;
            self.pos[1] += dy * MOVE_SPEED * dt;
        }

        let mut zoom_step = 0.0_f32;
        if ctx.input.is_pressed(Key::Q) {
            zoom_step -= 1.0;
        }
        if ctx.input.is_pressed(Key::E) {
            zoom_step += 1.0;
        }
        if zoom_step != 0.0 {
            let factor = (zoom_step * ZOOM_RATE * dt).exp();
            self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        }

        if ctx.input.is_pressed(Key::R) {
            self.pos = [0.0, 0.0];
            self.zoom = 1.0;
        }

        let (half_w, half_h) = map_half_extent();
        let pad = PLAYER_SIZE * 0.5;
        self.pos[0] = self.pos[0].clamp(-(half_w - pad), half_w - pad);
        self.pos[1] = self.pos[1].clamp(-(half_h - pad), half_h - pad);

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

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

        // HUD: render text in screen-space relative to the camera. Convert
        // pixel coordinates to world coordinates using the current zoom.
        let half_w_world = self.viewport.0 as f32 * 0.5 / self.zoom;
        let half_h_world = self.viewport.1 as f32 * 0.5 / self.zoom;
        let text_world_size = HUD_PIXEL_SIZE / self.zoom;
        let margin = HUD_MARGIN / self.zoom;
        let top_left_x = self.pos[0] - half_w_world + margin;
        let top_left_y = self.pos[1] + half_h_world - margin;
        let line_step = text_world_size * 1.25;

        let line1 = format!("INFENGINE PHASE 2.6  FPS {:.0}", self.fps_ema);
        let line2 = format!(
            "X {:>5.0}  Y {:>5.0}  ZOOM {:.2}",
            self.pos[0], self.pos[1], self.zoom
        );
        renderer.draw_text(
            &line1,
            [top_left_x, top_left_y],
            text_world_size,
            [1.0, 1.0, 1.0, 1.0],
        );
        renderer.draw_text(
            &line2,
            [top_left_x, top_left_y - line_step],
            text_world_size,
            [0.8, 0.9, 1.0, 1.0],
        );

        let queued = renderer.queued_sprite_count();
        if let Err(err) = renderer.render() {
            warn!(error = ?err, "frame render failed");
        }

        self.frame = self.frame.wrapping_add(1);
        if self.frame.is_multiple_of(120) {
            info!(
                frame = self.frame,
                fps = self.fps_ema,
                x = self.pos[0],
                y = self.pos[1],
                zoom = self.zoom,
                sprites = queued,
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
        title: "InfEngine - sprite_demo (Phase 2.6)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
