//! Cube demo - Phase 6.1 of InfEngine.
//!
//! Demonstrates the 3D pipeline: a perspective camera orbits a small
//! field of rotating cubes plus a wide ground plate. WASD slides the
//! camera horizontally on the orbit plane; Q / E zoom the orbit radius;
//! Esc exits.

use glam::{Mat4, Quat, Vec3};
use infengine_render::{MeshHandle, MeshVertex, Renderer};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

struct Demo {
    renderer: Option<Renderer>,
    sphere: Option<MeshHandle>,
    orbit_radius: f32,
    orbit_angle: f32,
    orbit_height: f32,
    time: f32,
    frame: u64,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            renderer: None,
            sphere: None,
            orbit_radius: 8.0,
            orbit_angle: 0.0,
            orbit_height: 4.0,
            time: 0.0,
            frame: 0,
        }
    }
}

/// Build a UV sphere with smooth normals (radius 0.5).
fn uv_sphere(latitudes: u32, longitudes: u32) -> (Vec<MeshVertex>, Vec<u32>) {
    let mut verts = Vec::new();
    let mut indices = Vec::new();
    for lat in 0..=latitudes {
        let theta = lat as f32 * std::f32::consts::PI / latitudes as f32;
        let sin_t = theta.sin();
        let cos_t = theta.cos();
        for lon in 0..=longitudes {
            let phi = lon as f32 * std::f32::consts::TAU / longitudes as f32;
            let sin_p = phi.sin();
            let cos_p = phi.cos();
            let n = [sin_t * cos_p, cos_t, sin_t * sin_p];
            let p = [n[0] * 0.5, n[1] * 0.5, n[2] * 0.5];
            verts.push(MeshVertex::new(p, n));
        }
    }
    let row = longitudes + 1;
    for lat in 0..latitudes {
        for lon in 0..longitudes {
            let a = lat * row + lon;
            let b = a + row;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    (verts, indices)
}

fn cube_model(position: Vec3, rotation: Quat, scale: Vec3) -> [f32; 16] {
    Mat4::from_scale_rotation_translation(scale, rotation, position).to_cols_array()
}

impl AppHandler for Demo {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        info!(width = w, height = h, "cube_demo started");
        match Renderer::new(window) {
            Ok(mut r) => {
                let (verts, idx) = uv_sphere(24, 32);
                self.sphere = Some(r.create_mesh("sphere", &verts, &idx));
                self.renderer = Some(r);
            }
            Err(err) => {
                error!(error = %err, "failed to initialize renderer");
                std::process::exit(2);
            }
        }
    }

    fn on_update(&mut self, _window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.max(1e-4);
        self.time += dt;

        let mut orbit_step = 0.0_f32;
        if ctx.input.any_pressed(&[Key::A, Key::ArrowLeft]) {
            orbit_step -= 1.0;
        }
        if ctx.input.any_pressed(&[Key::D, Key::ArrowRight]) {
            orbit_step += 1.0;
        }
        self.orbit_angle += orbit_step * 1.2 * dt;

        if ctx.input.any_pressed(&[Key::W, Key::ArrowUp]) {
            self.orbit_height = (self.orbit_height + 4.0 * dt).min(15.0);
        }
        if ctx.input.any_pressed(&[Key::S, Key::ArrowDown]) {
            self.orbit_height = (self.orbit_height - 4.0 * dt).max(1.0);
        }
        if ctx.input.is_pressed(Key::Q) {
            self.orbit_radius = (self.orbit_radius + 6.0 * dt).min(30.0);
        }
        if ctx.input.is_pressed(Key::E) {
            self.orbit_radius = (self.orbit_radius - 6.0 * dt).max(3.0);
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        {
            let camera = renderer.camera_3d_mut();
            camera.position = Vec3::new(
                self.orbit_angle.cos() * self.orbit_radius,
                self.orbit_height,
                self.orbit_angle.sin() * self.orbit_radius,
            );
            camera.target = Vec3::ZERO;
            camera.up = Vec3::Y;
        }
        renderer.set_clear_color(0.04, 0.06, 0.10, 1.0);

        // Ground plate.
        renderer.draw_cube(
            cube_model(
                Vec3::new(0.0, -0.5, 0.0),
                Quat::IDENTITY,
                Vec3::new(20.0, 0.2, 20.0),
            ),
            [0.25, 0.32, 0.38, 1.0],
        );

        // Five rotating cubes arranged on a ring.
        for i in 0..5 {
            let theta = i as f32 * std::f32::consts::TAU / 5.0;
            let pos = Vec3::new(theta.cos() * 3.0, 1.0, theta.sin() * 3.0);
            let rot = Quat::from_rotation_y(self.time + i as f32 * 0.7)
                * Quat::from_rotation_x(0.3 * (self.time * 0.6 + i as f32));
            let color = [
                0.5 + 0.5 * (i as f32 * 1.3).sin(),
                0.5 + 0.5 * (i as f32 * 1.7 + 1.0).sin(),
                0.5 + 0.5 * (i as f32 * 2.1 + 2.0).sin(),
                1.0,
            ];
            renderer.draw_cube(cube_model(pos, rot, Vec3::splat(1.0)), color);
        }

        // Tall pillar at the center.
        renderer.draw_cube(
            cube_model(
                Vec3::new(0.0, 1.5, 0.0),
                Quat::from_rotation_y(self.time * 0.4),
                Vec3::new(0.8, 3.0, 0.8),
            ),
            [0.95, 0.85, 0.55, 1.0],
        );

        // Bouncing sphere above the pillar.
        if let Some(sphere) = self.sphere {
            let bounce = 4.0 + (self.time * 2.0).sin() * 0.6;
            renderer.draw_mesh(
                sphere,
                cube_model(
                    Vec3::new(0.0, bounce, 0.0),
                    Quat::from_rotation_y(self.time * 1.5),
                    Vec3::splat(1.4),
                ),
                [0.95, 0.55, 0.85, 1.0],
            );
        }

        // Three sphere ornaments orbiting the column.
        if let Some(sphere) = self.sphere {
            for i in 0..3 {
                let theta = self.time * 0.6 + i as f32 * std::f32::consts::TAU / 3.0;
                let pos = Vec3::new(
                    theta.cos() * 1.6,
                    3.0 + theta.sin() * 0.3,
                    theta.sin() * 1.6,
                );
                let color = [
                    0.6 + 0.4 * ((i as f32) * 1.7).sin(),
                    0.6 + 0.4 * ((i as f32) * 2.3 + 1.0).sin(),
                    0.6 + 0.4 * ((i as f32) * 3.1 + 2.0).sin(),
                    1.0,
                ];
                renderer.draw_mesh(
                    sphere,
                    cube_model(pos, Quat::IDENTITY, Vec3::splat(0.5)),
                    color,
                );
            }
        }

        if let Err(err) = renderer.render(|_, _, _, _, _| {}) {
            warn!(error = ?err, "frame render failed");
        }

        self.frame = self.frame.wrapping_add(1);
        if self.frame.is_multiple_of(120) {
            info!(
                frame = self.frame,
                dt_ms = dt * 1000.0,
                orbit_r = self.orbit_radius,
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
        info!("cube_demo exiting");
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = WindowConfig {
        title: "InfEngine - cube_demo (Phase 6.2)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
