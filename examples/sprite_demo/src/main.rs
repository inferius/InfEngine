//! Sprite demo - Phase 2.7 of InfEngine.
//!
//! Combines every Phase-2 feature: tilemap background, batched sprite
//! player, screen-space HUD text, and a debug-line overlay (origin
//! cross, player bounding box and visibility circle, plus a leader
//! line from origin to player). WASD / arrows move, Q / E zoom out /
//! in, R resets, Escape exits.

use std::path::PathBuf;

use infengine_assets::{AssetEvent, AssetHandle, AssetServer, AssetWatcher, ImageAsset};
use infengine_audio::AudioSystem;
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
    assets: AssetServer,
    asset_watcher: Option<AssetWatcher>,
    sample_image: Option<AssetHandle<ImageAsset>>,
    audio: Option<AudioSystem>,
    reload_count: u32,
    beep_count: u32,
    pos: [f32; 2],
    zoom: f32,
    viewport: (u32, u32),
    fps_ema: f32,
    frame: u64,
}

impl Default for Demo {
    fn default() -> Self {
        let assets_root = PathBuf::from("assets");
        Self {
            renderer: None,
            tilemap: build_tilemap(),
            assets: AssetServer::new(assets_root),
            asset_watcher: None,
            sample_image: None,
            audio: None,
            reload_count: 0,
            beep_count: 0,
            pos: [0.0, 0.0],
            zoom: 1.0,
            viewport: (1, 1),
            fps_ema: 60.0,
            frame: 0,
        }
    }
}

/// Generate a small "InfEngine" gradient PNG into `assets/sample.png`
/// if it does not exist, so the demo always has something to load.
fn ensure_sample_image(asset_root: &std::path::Path) -> std::io::Result<()> {
    if !asset_root.exists() {
        std::fs::create_dir_all(asset_root)?;
    }
    let target = asset_root.join("sample.png");
    if target.exists() {
        return Ok(());
    }
    let (w, h) = (128u32, 128u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = ((x as f32 / w as f32) * 255.0) as u8;
            let g = ((y as f32 / h as f32) * 255.0) as u8;
            let b = 0x60;
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    image::save_buffer(&target, &rgba, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
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

        if let Err(err) = ensure_sample_image(self.assets.root()) {
            warn!(error = %err, "failed to write sample.png");
        }
        match self.assets.load_image("sample.png") {
            Ok(handle) => {
                if let Some(img) = self.assets.get_image(handle) {
                    info!(
                        width = img.width,
                        height = img.height,
                        bytes = img.rgba.len(),
                        "sample asset loaded"
                    );
                }
                self.sample_image = Some(handle);
            }
            Err(err) => warn!(error = %err, "failed to load sample.png"),
        }

        match self.assets.watch() {
            Ok(w) => self.asset_watcher = Some(w),
            Err(err) => warn!(error = %err, "filesystem watcher unavailable"),
        }

        match AudioSystem::new() {
            Ok(a) => self.audio = Some(a),
            Err(err) => warn!(error = %err, "audio system unavailable"),
        }
    }

    fn on_key(&mut self, key: Key, pressed: bool) {
        if pressed
            && key == Key::Space
            && let Some(audio) = self.audio.as_mut()
        {
            match audio.play_beep(440.0, 0.10, 0.3) {
                Ok(()) => self.beep_count += 1,
                Err(err) => warn!(error = %err, "play_beep failed"),
            }
        }
    }

    fn on_update(&mut self, _window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.max(1e-4);
        let instant_fps = 1.0 / dt;
        self.fps_ema = self.fps_ema * 0.9 + instant_fps * 0.1;

        // Drain hot-reload events; reload the sample image when it changes.
        if let Some(w) = self.asset_watcher.as_ref() {
            while let Ok(event) = w.receiver.try_recv() {
                match event {
                    AssetEvent::Modified(path) => {
                        info!(path = %path.display(), "asset modified");
                        if let Some(handle) = self.sample_image
                            && let Some(known) = self.assets.handle_path(handle)
                            && known == path
                        {
                            match self.assets.reload_image(handle) {
                                Ok(_) => self.reload_count += 1,
                                Err(err) => warn!(error = %err, "reload_image failed"),
                            }
                        }
                    }
                    AssetEvent::Removed(path) => {
                        warn!(path = %path.display(), "asset removed");
                    }
                }
            }
        }

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

        // Debug overlay: world origin marker, player bounding box and a
        // visibility circle, plus a leader line from origin to player.
        renderer.debug_line([-24.0, 0.0], [24.0, 0.0], [1.0, 0.4, 0.4, 1.0]);
        renderer.debug_line([0.0, -24.0], [0.0, 24.0], [0.4, 1.0, 0.4, 1.0]);
        let half = PLAYER_SIZE * 0.5;
        renderer.debug_rect(
            [self.pos[0] - half, self.pos[1] - half],
            [self.pos[0] + half, self.pos[1] + half],
            [1.0, 1.0, 0.0, 1.0],
        );
        renderer.debug_circle(self.pos, PLAYER_SIZE, [0.6, 0.9, 1.0, 0.9], 48);
        renderer.debug_line([0.0, 0.0], self.pos, [1.0, 1.0, 1.0, 0.4]);

        // HUD: render text in screen-space relative to the camera. Convert
        // pixel coordinates to world coordinates using the current zoom.
        let half_w_world = self.viewport.0 as f32 * 0.5 / self.zoom;
        let half_h_world = self.viewport.1 as f32 * 0.5 / self.zoom;
        let text_world_size = HUD_PIXEL_SIZE / self.zoom;
        let margin = HUD_MARGIN / self.zoom;
        let top_left_x = self.pos[0] - half_w_world + margin;
        let top_left_y = self.pos[1] + half_h_world - margin;
        let line_step = text_world_size * 1.25;

        let line1 = format!("INFENGINE PHASE 3.3  FPS {:.0}", self.fps_ema);
        let line2 = format!(
            "X {:>5.0}  Y {:>5.0}  ZOOM {:.2}  RELOADS {}  BEEPS {}",
            self.pos[0], self.pos[1], self.zoom, self.reload_count, self.beep_count
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
        title: "InfEngine - sprite_demo (Phase 3.3)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
