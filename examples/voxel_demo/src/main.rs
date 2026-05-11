//! Phase 14 voxel showcase.
//!
//! Generates a 3x3 grid of 16^3 chunks with a simple hill-height
//! function, builds per-block submeshes with `infengine_voxel`, and
//! lets the player walk around the world with WASD + mouse-look.
//! Demonstrates the full path: voxel data -> mesh builder -> engine
//! renderer -> PBR + shadow + grid.

use std::process::ExitCode;

use glam::{Mat4, Vec3};
use infengine_render::{MeshHandle, Renderer};
use infengine_voxel::{
    BlockDef, BlockId, BlockRegistry, CHUNK_SIZE, Chunk, chunk_mesh, submesh_instance,
};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const CHUNKS: i32 = 3;
const WORLD_HALF: f32 = (CHUNKS as f32 * CHUNK_SIZE as f32) * 0.5;
const PLAYER_SPEED: f32 = 6.0;
const JUMP_VELOCITY: f32 = 6.0;
const GRAVITY: f32 = -18.0;
const PLAYER_HEIGHT: f32 = 1.7;

struct Voxel {
    renderer: Option<Renderer>,
    registry: BlockRegistry,
    /// Per-chunk: list of (block id, mesh handle) - one entry per
    /// unique block kind present in the chunk.
    submeshes: Vec<(BlockId, MeshHandle)>,
    player_pos: Vec3,
    player_vel: Vec3,
    player_yaw: f32,
    player_pitch: f32,
    last_mouse: Option<[f32; 2]>,
    mouse_dx: f32,
    mouse_dy: f32,
    middle_button: bool,
    requested_quit: bool,
    viewport: (u32, u32),
    fps_ema: f32,
}

impl Voxel {
    fn new() -> Self {
        let mut registry = BlockRegistry::new();
        registry.insert(BlockId(1), BlockDef::solid("stone", [0.50, 0.50, 0.55]));
        registry.insert(BlockId(2), BlockDef::solid("dirt", [0.45, 0.32, 0.20]));
        registry.insert(BlockId(3), BlockDef::solid("grass", [0.35, 0.62, 0.28]));
        registry.insert(BlockId(4), BlockDef::solid("sand", [0.92, 0.86, 0.62]));
        registry.insert(BlockId(5), BlockDef::solid("wood", [0.55, 0.38, 0.22]));
        Self {
            renderer: None,
            registry,
            submeshes: Vec::new(),
            player_pos: Vec3::new(0.0, 32.0, 0.0),
            player_vel: Vec3::ZERO,
            player_yaw: 0.0,
            player_pitch: -0.4,
            last_mouse: None,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            middle_button: false,
            requested_quit: false,
            viewport: (1, 1),
            fps_ema: 60.0,
        }
    }

    fn build_world(&mut self) {
        // Procedural terrain: low-frequency hills via two sine waves.
        // Surface gets grass, three layers below dirt, the rest stone.
        // Sand patches at the world boundary so the demo shows multiple
        // materials without needing a noise generator.
        let height_at = |wx: f32, wz: f32| -> i32 {
            let h = 8.0
                + (wx * 0.13).sin() * 2.5
                + (wz * 0.17).cos() * 2.0
                + ((wx + wz) * 0.07).sin() * 1.5;
            h.round() as i32
        };

        let renderer = self.renderer.as_mut().expect("renderer initialised");
        let total_chunks = CHUNKS * CHUNKS;
        let mut produced_voxels = 0usize;

        for cz in 0..CHUNKS {
            for cx in 0..CHUNKS {
                let origin = Vec3::new(
                    (cx as f32 - CHUNKS as f32 * 0.5) * CHUNK_SIZE as f32,
                    0.0,
                    (cz as f32 - CHUNKS as f32 * 0.5) * CHUNK_SIZE as f32,
                );
                let mut chunk = Chunk::empty();
                chunk.fill_with(|x, y, z| {
                    let wx = origin.x + x as f32;
                    let wz = origin.z + z as f32;
                    let h = height_at(wx, wz);
                    let wy = origin.y as i32 + y as i32;
                    if wy > h {
                        return BlockId::AIR;
                    }
                    // Boundary -> sand strip.
                    if wx.abs() > WORLD_HALF - 2.0 || wz.abs() > WORLD_HALF - 2.0 {
                        return BlockId(4);
                    }
                    if wy == h {
                        BlockId(3) // grass
                    } else if wy >= h - 3 {
                        BlockId(2) // dirt
                    } else {
                        BlockId(1) // stone
                    }
                });
                produced_voxels += chunk.raw().iter().filter(|b| !b.is_air()).count();

                let submeshes = chunk_mesh(&chunk, origin);
                let label_prefix = format!("voxel_chunk_{cx}_{cz}");
                for sub in submeshes {
                    let handle = renderer.create_mesh(
                        &format!("{}_b{}", label_prefix, sub.block.0),
                        &sub.vertices,
                        &sub.indices,
                    );
                    self.submeshes.push((sub.block, handle));
                }
            }
        }
        info!(
            chunks = total_chunks,
            voxels = produced_voxels,
            submeshes = self.submeshes.len(),
            "world built"
        );
    }
}

impl AppHandler for Voxel {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, "voxel_demo started");
        match Renderer::new(window) {
            Ok(r) => self.renderer = Some(r),
            Err(err) => {
                error!(error = %err, "renderer init failed");
                std::process::exit(2);
            }
        }
        self.build_world();
    }

    fn on_window_event(&mut self, _window: &EngineWindow, event: &WindowEvent) {
        match event {
            WindowEvent::MouseInput { state, button, .. } if *button == MouseButton::Middle => {
                self.middle_button = matches!(state, ElementState::Pressed);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let now = [position.x as f32, position.y as f32];
                if let Some(prev) = self.last_mouse {
                    self.mouse_dx += now[0] - prev[0];
                    self.mouse_dy += now[1] - prev[1];
                }
                self.last_mouse = Some(now);
            }
            _ => {}
        }
    }

    fn on_device_event(&mut self, event: &infengine_window::raw_winit::event::DeviceEvent) {
        use infengine_window::raw_winit::event::DeviceEvent;
        if let DeviceEvent::MouseMotion { delta } = event {
            // Prefer raw device delta when the OS grabs the cursor.
            self.mouse_dx += delta.0 as f32;
            self.mouse_dy += delta.1 as f32;
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.clamp(1e-4, 0.1);
        self.fps_ema = self.fps_ema * 0.9 + (1.0 / dt) * 0.1;

        if ctx.input.is_pressed(Key::Escape) {
            self.requested_quit = true;
        }

        // Mouse look.
        let dx = std::mem::take(&mut self.mouse_dx);
        let dy = std::mem::take(&mut self.mouse_dy);
        self.player_yaw -= dx * 0.0025;
        self.player_pitch -= dy * 0.0025;
        // Clamp pitch so the camera doesn't somersault.
        let limit = std::f32::consts::FRAC_PI_2 - 0.05;
        self.player_pitch = self.player_pitch.clamp(-limit, limit);

        let forward = Vec3::new(
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        );
        let forward_horizontal =
            Vec3::new(self.player_yaw.cos(), 0.0, -self.player_yaw.sin()).normalize();
        let right = Vec3::new(forward_horizontal.z, 0.0, -forward_horizontal.x);

        let mut move_axis = 0.0;
        if ctx.input.is_pressed(Key::W) {
            move_axis += 1.0;
        }
        if ctx.input.is_pressed(Key::S) {
            move_axis -= 1.0;
        }
        let mut strafe = 0.0;
        if ctx.input.is_pressed(Key::A) {
            strafe -= 1.0;
        }
        if ctx.input.is_pressed(Key::D) {
            strafe += 1.0;
        }
        let horizontal =
            (forward_horizontal * move_axis + right * strafe).normalize_or_zero() * PLAYER_SPEED;
        self.player_vel.x = horizontal.x;
        self.player_vel.z = horizontal.z;

        if ctx.input.is_pressed(Key::Space) && self.player_vel.y.abs() < 0.01 {
            self.player_vel.y = JUMP_VELOCITY;
        }
        self.player_vel.y += GRAVITY * dt;
        self.player_pos += self.player_vel * dt;

        // Crude ground clamp: the world hill height varies, so just
        // stop the player from falling forever and let them walk on a
        // virtual floor at y = 12 (above the tallest hill). Proper
        // voxel collision is future work.
        let floor = 12.0 + PLAYER_HEIGHT;
        if self.player_pos.y < floor {
            self.player_pos.y = floor;
            self.player_vel.y = 0.0;
        }

        // Clamp inside the world bounds.
        self.player_pos.x = self.player_pos.x.clamp(-WORLD_HALF + 1.0, WORLD_HALF - 1.0);
        self.player_pos.z = self.player_pos.z.clamp(-WORLD_HALF + 1.0, WORLD_HALF - 1.0);

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        {
            let cam = renderer.camera_3d_mut();
            cam.position = self.player_pos;
            cam.target = self.player_pos + forward;
        }
        renderer.set_clear_color(0.50, 0.70, 0.92, 1.0);

        // Submit every chunk submesh with its block's PBR material.
        let model = Mat4::IDENTITY.to_cols_array();
        for (block, handle) in &self.submeshes {
            if let Some(def) = self.registry.get(*block) {
                renderer.draw_mesh_pbr(
                    *handle,
                    model,
                    [def.color[0], def.color[1], def.color[2], 1.0],
                    def.metallic,
                    def.roughness,
                );
            }
        }

        let _ = submesh_instance; // keep helper alive for future per-instance path.

        if let Err(err) = renderer.render(|_, _, _, _, _| {}) {
            warn!(error = ?err, "frame render failed");
        }

        if self.requested_quit {
            std::process::exit(0);
        }
        let _ = window;
    }

    fn on_resize(&mut self, w: u32, h: u32) {
        self.viewport = (w, h);
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

    let config = WindowConfig {
        title: "InfEngine - voxel_demo".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };
    if let Err(err) = run(config, Voxel::new()) {
        error!(error = %err, "event loop failed");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
