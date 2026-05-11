//! Standalone runtime player for InfEngine projects.
//!
//! Loads `project.ron` (or the path passed as argv[1]), opens the
//! first listed scene, and renders it. No editor UI - this is what
//! the editor's Build/Export step ships next to a project's assets.
//!
//! Usage:
//!
//! ```sh
//! infengine_runtime              # ./project.ron
//! infengine_runtime path/to/project.ron
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use infengine_core::project::Project;
use infengine_core::scene::Scene;
use infengine_render::{Renderer, SpriteInstance};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

struct Runtime {
    scene: Scene,
    renderer: Option<Renderer>,
    zoom: f32,
    camera: [f32; 2],
}

impl AppHandler for Runtime {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        info!(width = w, height = h, scene = %self.scene.name, "runtime started");
        match Renderer::new(window) {
            Ok(r) => self.renderer = Some(r),
            Err(err) => {
                error!(error = %err, "renderer init failed");
                std::process::exit(2);
            }
        }
    }

    fn on_update(&mut self, _window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.max(1e-4);
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
            self.camera[0] += dx * 400.0 * dt;
            self.camera[1] += dy * 400.0 * dt;
        }
        if ctx.input.is_pressed(Key::Q) {
            self.zoom = (self.zoom + 1.2 * dt).min(4.0);
        }
        if ctx.input.is_pressed(Key::E) {
            self.zoom = (self.zoom - 1.2 * dt).max(0.25);
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        {
            let camera = renderer.camera_mut();
            camera.position = self.camera;
            camera.zoom = self.zoom;
        }
        renderer.set_clear_color(
            self.scene.clear_color[0] as f64,
            self.scene.clear_color[1] as f64,
            self.scene.clear_color[2] as f64,
            1.0,
        );
        for entity in &self.scene.entities {
            if let Some(sprite) = &entity.sprite {
                let size = [
                    sprite.size[0] * entity.transform.scale[0],
                    sprite.size[1] * entity.transform.scale[1],
                ];
                let (uv_min, uv_max) = sprite.region.uv_rect();
                renderer.draw_sprite(SpriteInstance {
                    position: entity.transform.position,
                    size,
                    color: sprite.color,
                    uv_min,
                    uv_max,
                });
            }
        }
        if let Err(err) = renderer.render(|_, _, _, _, _| {}) {
            warn!(error = ?err, "frame render failed");
        }
    }

    fn on_resize(&mut self, w: u32, h: u32) {
        if let Some(r) = self.renderer.as_mut() {
            r.resize(w, h);
        }
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let project_file = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "project.ron".to_string()),
    );
    let project_root = project_file
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let project = match Project::load(&project_file) {
        Ok(p) => p,
        Err(err) => {
            error!(error = %err, file = %project_file.display(), "failed to open project");
            return ExitCode::from(2);
        }
    };
    let Some(entry) = project.scenes.first().cloned() else {
        error!("project has no scenes");
        return ExitCode::from(3);
    };
    let scene_path = project.scene_full_path(&project_root, &entry);
    let scene = match Scene::load(&scene_path) {
        Ok(s) => s,
        Err(err) => {
            error!(error = %err, path = %scene_path.display(), "failed to load first scene");
            return ExitCode::from(4);
        }
    };

    let runtime = Runtime {
        scene,
        renderer: None,
        zoom: 1.0,
        camera: [0.0, 0.0],
    };
    let config = WindowConfig {
        title: format!("{} - InfEngine runtime", project.name),
        width: 1280,
        height: 720,
        resizable: true,
    };
    if let Err(err) = run(config, runtime) {
        error!(error = %err, "event loop failed");
        return ExitCode::from(5);
    }
    ExitCode::SUCCESS
}
