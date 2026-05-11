//! Phase 14 voxel showcase.
//!
//! Generates a grid of 16^3 chunks with a simple hill-height
//! function, builds per-block submeshes with `infengine_voxel`, and
//! lets the player walk around the world with WASD + mouse-look. A
//! main menu / settings / pause flow built on egui sits on top so the
//! player can toggle cursor lock, hide, and mouse sensitivity at
//! runtime - matching the polish level of `demo_game`.

use std::process::ExitCode;

use glam::{Mat4, Vec3};
use infengine_editor::EditorUi;
use infengine_render::{MeshHandle, Renderer};
use infengine_voxel::{
    BlockDef, BlockId, BlockRegistry, CHUNK_SIZE, Chunk, ChunkCoord, RayHit, World, chunk_mesh,
    raycast,
};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const CHUNKS: i32 = 5;
const WORLD_HALF: f32 = (CHUNKS as f32 * CHUNK_SIZE as f32) * 0.5;
const PLAYER_SPEED: f32 = 6.0;
const JUMP_VELOCITY: f32 = 6.0;
const GRAVITY: f32 = -18.0;
const PLAYER_HEIGHT: f32 = 1.7;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum AppState {
    MainMenu,
    Playing,
    Paused,
    Settings { back_to: BackTo },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum BackTo {
    MainMenu,
    Pause,
}

#[derive(Debug, Clone)]
struct Settings {
    mouse_sensitivity: f32,
    lock_cursor: bool,
    hide_cursor: bool,
    show_fps: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.6,
            lock_cursor: true,
            hide_cursor: true,
            show_fps: true,
        }
    }
}

struct Voxel {
    state: AppState,
    settings: Settings,
    requested_quit: bool,

    renderer: Option<Renderer>,
    ui: Option<EditorUi>,
    registry: BlockRegistry,
    world: World,
    /// Mesh handles grouped by the chunk that produced them. Lets the
    /// editor rebuild only the affected chunk after a block edit
    /// (registry leaks the old handles; a future change will add a
    /// remove API on the renderer).
    chunk_meshes: Vec<(ChunkCoord, BlockId, MeshHandle)>,
    /// Most recent ray hit while looking around in Play. Drives the
    /// crosshair / wireframe overlay and the click-to-edit step.
    target: Option<RayHit>,

    player_pos: Vec3,
    player_vel: Vec3,
    player_yaw: f32,
    player_pitch: f32,

    last_mouse: Option<[f32; 2]>,
    mouse_dx: f32,
    mouse_dy: f32,
    esc_prev: bool,
    lmb_pending: bool,
    rmb_pending: bool,
    /// Block id placed by a right-click. Future versions of the demo
    /// will let the player cycle through this; today it's fixed to
    /// the stone block id.
    place_block: BlockId,

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
            state: AppState::MainMenu,
            settings: Settings::default(),
            requested_quit: false,
            renderer: None,
            ui: None,
            registry,
            world: World::new(),
            chunk_meshes: Vec::new(),
            target: None,
            player_pos: Vec3::new(0.0, 32.0, 0.0),
            player_vel: Vec3::ZERO,
            player_yaw: 0.0,
            player_pitch: -0.4,
            last_mouse: None,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            esc_prev: false,
            lmb_pending: false,
            rmb_pending: false,
            place_block: BlockId(1),
            viewport: (1, 1),
            fps_ema: 60.0,
        }
    }

    fn build_world(&mut self) {
        let height_at = |wx: f32, wz: f32| -> i32 {
            let h = 8.0
                + (wx * 0.13).sin() * 2.5
                + (wz * 0.17).cos() * 2.0
                + ((wx + wz) * 0.07).sin() * 1.5;
            h.round() as i32
        };

        let mut produced_voxels = 0usize;
        for cz in 0..CHUNKS {
            for cx in 0..CHUNKS {
                let cc: ChunkCoord = [cx - CHUNKS / 2, 0, cz - CHUNKS / 2];
                let origin = World::chunk_world_origin(cc);
                let mut chunk = Chunk::empty();
                chunk.fill_with(|x, y, z| {
                    let wx = origin.x + x as f32;
                    let wz = origin.z + z as f32;
                    let h = height_at(wx, wz);
                    let wy = origin.y as i32 + y as i32;
                    if wy > h {
                        return BlockId::AIR;
                    }
                    if wx.abs() > WORLD_HALF - 2.0 || wz.abs() > WORLD_HALF - 2.0 {
                        return BlockId(4);
                    }
                    if wy == h {
                        BlockId(3)
                    } else if wy >= h - 3 {
                        BlockId(2)
                    } else {
                        BlockId(1)
                    }
                });
                produced_voxels += chunk.raw().iter().filter(|b| !b.is_air()).count();
                self.world.insert_chunk(cc, chunk);
            }
        }
        self.rebuild_all_meshes();
        info!(
            chunks = CHUNKS * CHUNKS,
            voxels = produced_voxels,
            submeshes = self.chunk_meshes.len(),
            "world built"
        );
    }

    fn affected_chunks(world_voxel: [i32; 3]) -> Vec<ChunkCoord> {
        // The edited voxel always belongs to one chunk. If it sits on
        // a chunk boundary (local 0 or CHUNK_SIZE-1 on any axis) the
        // neighbour chunk's faces facing it become visible/hidden too,
        // so we rebuild that chunk as well.
        let cs = CHUNK_SIZE as i32;
        let cc = [
            world_voxel[0].div_euclid(cs),
            world_voxel[1].div_euclid(cs),
            world_voxel[2].div_euclid(cs),
        ];
        let lc = [
            world_voxel[0].rem_euclid(cs),
            world_voxel[1].rem_euclid(cs),
            world_voxel[2].rem_euclid(cs),
        ];
        let mut out = vec![cc];
        if lc[0] == 0 {
            out.push([cc[0] - 1, cc[1], cc[2]]);
        }
        if lc[0] == cs - 1 {
            out.push([cc[0] + 1, cc[1], cc[2]]);
        }
        if lc[1] == 0 {
            out.push([cc[0], cc[1] - 1, cc[2]]);
        }
        if lc[1] == cs - 1 {
            out.push([cc[0], cc[1] + 1, cc[2]]);
        }
        if lc[2] == 0 {
            out.push([cc[0], cc[1], cc[2] - 1]);
        }
        if lc[2] == cs - 1 {
            out.push([cc[0], cc[1], cc[2] + 1]);
        }
        out
    }

    fn rebuild_chunks(&mut self, coords: &[ChunkCoord]) {
        let renderer = self.renderer.as_mut().expect("renderer ready");
        // Drop existing mesh handles for the affected chunks. The
        // mesh registry leaks the old buffer slots until the renderer
        // gains a remove API; for an editor-scale demo this is fine.
        self.chunk_meshes.retain(|(c, _, _)| !coords.contains(c));
        for coord in coords {
            let Some(chunk) = self.world.chunk(*coord) else {
                continue;
            };
            let origin = World::chunk_world_origin(*coord);
            for sub in chunk_mesh(chunk, origin) {
                let handle = renderer.create_mesh(
                    &format!(
                        "voxel_chunk_{}_{}_{}_b{}",
                        coord[0], coord[1], coord[2], sub.block.0
                    ),
                    &sub.vertices,
                    &sub.indices,
                );
                self.chunk_meshes.push((*coord, sub.block, handle));
            }
        }
    }

    /// Walk every chunk in the world and register its mesh handles.
    /// Used at startup. Edits use `rebuild_chunks` for the affected
    /// chunk + its solid neighbours so the cost stays local.
    fn rebuild_all_meshes(&mut self) {
        self.chunk_meshes.clear();
        let renderer = self.renderer.as_mut().expect("renderer ready");
        let coords: Vec<ChunkCoord> = self.world.iter_chunks().map(|(c, _)| *c).collect();
        for coord in coords {
            let chunk = self.world.chunk(coord).expect("chunk present");
            let origin = World::chunk_world_origin(coord);
            let subs = chunk_mesh(chunk, origin);
            for sub in subs {
                let handle = renderer.create_mesh(
                    &format!(
                        "voxel_chunk_{}_{}_{}_b{}",
                        coord[0], coord[1], coord[2], sub.block.0
                    ),
                    &sub.vertices,
                    &sub.indices,
                );
                self.chunk_meshes.push((coord, sub.block, handle));
            }
        }
    }

    fn apply_cursor_settings(&self, window: &EngineWindow) {
        use infengine_window::raw_winit::window::CursorGrabMode;
        let playing = matches!(self.state, AppState::Playing);
        let raw = window.raw();
        let grab = if playing && self.settings.lock_cursor {
            if self.settings.hide_cursor {
                CursorGrabMode::Locked
            } else {
                CursorGrabMode::Confined
            }
        } else {
            CursorGrabMode::None
        };
        if raw.set_cursor_grab(grab).is_err() && grab == CursorGrabMode::Locked {
            let _ = raw.set_cursor_grab(CursorGrabMode::Confined);
        }
        raw.set_cursor_visible(!(playing && self.settings.hide_cursor));
    }

    fn update_play(&mut self, ctx: &FrameContext<'_>, dt: f32) {
        let dx = std::mem::take(&mut self.mouse_dx);
        let dy = std::mem::take(&mut self.mouse_dy);
        let sens = self.settings.mouse_sensitivity * 0.0035;
        self.player_yaw -= dx * sens;
        self.player_pitch -= dy * sens;
        let limit = std::f32::consts::FRAC_PI_2 - 0.05;
        self.player_pitch = self.player_pitch.clamp(-limit, limit);

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

        let floor = 12.0 + PLAYER_HEIGHT;
        if self.player_pos.y < floor {
            self.player_pos.y = floor;
            self.player_vel.y = 0.0;
        }
        self.player_pos.x = self.player_pos.x.clamp(-WORLD_HALF + 1.0, WORLD_HALF - 1.0);
        self.player_pos.z = self.player_pos.z.clamp(-WORLD_HALF + 1.0, WORLD_HALF - 1.0);
    }

    fn render_ui(&mut self, window: &EngineWindow) {
        let state = self.state;
        let fps = self.fps_ema;
        let show_fps = self.settings.show_fps;
        let settings_edit = &mut self.settings;
        let mut action_start = false;
        let mut action_resume = false;
        let mut action_quit = false;
        let mut action_menu_to_settings = false;
        let mut action_pause_to_settings = false;
        let mut action_back_settings = false;
        let mut action_main_menu = false;

        let Some(ui) = self.ui.as_mut() else {
            return;
        };
        ui.run(window.raw(), |ctx| match state {
            AppState::MainMenu => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading("InfEngine - Voxel Demo");
                        ui.add_space(20.0);
                        if ui.button("Play").clicked() {
                            action_start = true;
                        }
                        if ui.button("Settings").clicked() {
                            action_menu_to_settings = true;
                        }
                        if ui.button("Quit").clicked() {
                            action_quit = true;
                        }
                    });
                });
            }
            AppState::Paused => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading("Paused");
                        ui.add_space(20.0);
                        if ui.button("Resume").clicked() {
                            action_resume = true;
                        }
                        if ui.button("Settings").clicked() {
                            action_pause_to_settings = true;
                        }
                        if ui.button("Main Menu").clicked() {
                            action_main_menu = true;
                        }
                    });
                });
            }
            AppState::Settings { .. } => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.add(
                        egui::Slider::new(&mut settings_edit.mouse_sensitivity, 0.1..=2.0)
                            .text("Mouse sensitivity"),
                    );
                    ui.checkbox(&mut settings_edit.show_fps, "Show FPS");
                    ui.checkbox(
                        &mut settings_edit.lock_cursor,
                        "Lock cursor in window (Play)",
                    );
                    ui.checkbox(&mut settings_edit.hide_cursor, "Hide cursor (Play)");
                    ui.separator();
                    if ui.button("Back").clicked() {
                        action_back_settings = true;
                    }
                });
            }
            AppState::Playing => {
                egui::Area::new(egui::Id::new("voxel_hud"))
                    .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label("Mouse aim, WASD, Space jump, Esc menu");
                            ui.label("LMB break, RMB place");
                            if show_fps {
                                ui.separator();
                                ui.label(format!("FPS: {fps:.0}"));
                            }
                        });
                    });
                // Crosshair drawn directly on the egui screen layer.
                // Two thin segments through the screen centre.
                let screen = ctx.content_rect();
                let center = screen.center();
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("voxel_crosshair"),
                ));
                let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(240, 240, 240));
                let half = 8.0_f32;
                painter.line_segment(
                    [
                        egui::pos2(center.x - half, center.y),
                        egui::pos2(center.x + half, center.y),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        egui::pos2(center.x, center.y - half),
                        egui::pos2(center.x, center.y + half),
                    ],
                    stroke,
                );
            }
        });

        if action_start {
            self.state = AppState::Playing;
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }
        if action_resume {
            self.state = AppState::Playing;
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }
        if action_quit {
            self.requested_quit = true;
        }
        if action_menu_to_settings {
            self.state = AppState::Settings {
                back_to: BackTo::MainMenu,
            };
        }
        if action_pause_to_settings {
            self.state = AppState::Settings {
                back_to: BackTo::Pause,
            };
        }
        if action_back_settings && let AppState::Settings { back_to } = self.state {
            self.state = match back_to {
                BackTo::MainMenu => AppState::MainMenu,
                BackTo::Pause => AppState::Paused,
            };
        }
        if action_main_menu {
            self.state = AppState::MainMenu;
        }
    }
}

impl AppHandler for Voxel {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, "voxel_demo started");
        match Renderer::new(window) {
            Ok(r) => {
                let ui = EditorUi::new(r.wgpu_device(), r.surface_format(), window.raw(), 1);
                self.ui = Some(ui);
                self.renderer = Some(r);
            }
            Err(err) => {
                error!(error = %err, "renderer init failed");
                std::process::exit(2);
            }
        }
        self.build_world();
    }

    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(ui) = self.ui.as_mut() {
            ui.handle_engine_event(window, event);
        }
        // Track cursor moved deltas only when no grab is active (i.e. in
        // menus this is the only source; in Play raw DeviceEvent
        // supplies the deltas).
        if let WindowEvent::CursorMoved { position, .. } = event
            && matches!(self.state, AppState::Playing)
            && !(self.settings.lock_cursor || self.settings.hide_cursor)
        {
            let now = [position.x as f32, position.y as f32];
            if let Some(prev) = self.last_mouse {
                self.mouse_dx += now[0] - prev[0];
                self.mouse_dy += now[1] - prev[1];
            }
            self.last_mouse = Some(now);
        }
        // Edge-triggered LMB / RMB while in Play state. Both flags are
        // consumed in `on_update` next frame.
        if matches!(self.state, AppState::Playing)
            && let WindowEvent::MouseInput { state, button, .. } = event
            && matches!(state, ElementState::Pressed)
        {
            match button {
                MouseButton::Left => self.lmb_pending = true,
                MouseButton::Right => self.rmb_pending = true,
                _ => {}
            }
        }
    }

    fn on_device_event(&mut self, event: &infengine_window::raw_winit::event::DeviceEvent) {
        use infengine_window::raw_winit::event::DeviceEvent;
        if !matches!(self.state, AppState::Playing) {
            return;
        }
        if !(self.settings.lock_cursor || self.settings.hide_cursor) {
            return;
        }
        if let DeviceEvent::MouseMotion { delta } = event {
            self.mouse_dx += delta.0 as f32;
            self.mouse_dy += delta.1 as f32;
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.clamp(1e-4, 0.1);
        self.fps_ema = self.fps_ema * 0.9 + (1.0 / dt) * 0.1;

        // Esc cycles Play <-> Paused and Settings -> back. Never quits.
        let esc_now = ctx.input.is_pressed(Key::Escape);
        let esc_just = esc_now && !self.esc_prev;
        self.esc_prev = esc_now;
        if esc_just {
            match self.state {
                AppState::Playing => self.state = AppState::Paused,
                AppState::Paused => self.state = AppState::Playing,
                AppState::Settings { back_to } => {
                    self.state = match back_to {
                        BackTo::MainMenu => AppState::MainMenu,
                        BackTo::Pause => AppState::Paused,
                    };
                }
                AppState::MainMenu => {}
            }
        }

        if matches!(self.state, AppState::Playing) {
            self.update_play(ctx, dt);
        } else {
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }
        self.apply_cursor_settings(window);

        // Build UI state first so the immediate-mode pass mutates self
        // freely. The renderer is borrowed afterwards once UI state has
        // settled.
        self.render_ui(window);

        let forward = Vec3::new(
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        );

        // Cast a ray from the eye along the look direction so the
        // crosshair / wireframe / edit code can target the voxel the
        // player is currently aiming at.
        self.target = if matches!(self.state, AppState::Playing) {
            raycast(&self.world, self.player_pos, forward, 6.0)
        } else {
            None
        };

        // Apply pending mouse-button edits before rendering.
        let lmb = self.lmb_pending;
        let rmb = self.rmb_pending;
        self.lmb_pending = false;
        self.rmb_pending = false;
        if let Some(hit) = self.target {
            if lmb {
                self.world.set(hit.voxel, BlockId::AIR);
                let coords = Self::affected_chunks(hit.voxel);
                self.rebuild_chunks(&coords);
            } else if rmb {
                // Don't place a block on top of the player's head /
                // body - skip if the destination overlaps the player's
                // bounding column.
                let p = self.player_pos;
                let pvx = p.x.floor() as i32;
                let pvz = p.z.floor() as i32;
                let player_feet = (p.y - PLAYER_HEIGHT).floor() as i32;
                let player_head = p.y.floor() as i32;
                let overlaps_player = hit.place_at[0] == pvx
                    && hit.place_at[2] == pvz
                    && (player_feet..=player_head).contains(&hit.place_at[1]);
                if !overlaps_player {
                    self.world.set(hit.place_at, self.place_block);
                    let coords = Self::affected_chunks(hit.place_at);
                    self.rebuild_chunks(&coords);
                }
            }
        }

        let player_pos = self.player_pos;
        let chunk_meshes = self.chunk_meshes.clone();
        let registry = &self.registry;
        let target = self.target;
        let ui = self.ui.as_mut();
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        {
            let cam = renderer.camera_3d_mut();
            cam.position = player_pos;
            cam.target = player_pos + forward;
        }
        renderer.set_clear_color(0.50, 0.70, 0.92, 1.0);
        let model = Mat4::IDENTITY.to_cols_array();
        for (_coord, block, handle) in &chunk_meshes {
            if let Some(def) = registry.get(*block) {
                renderer.draw_mesh_pbr(
                    *handle,
                    model,
                    [def.color[0], def.color[1], def.color[2], 1.0],
                    def.metallic,
                    def.roughness,
                );
            }
        }
        // Wire outline around the voxel the player is currently aiming
        // at. The outline is slightly inflated (0.005 unit margin) so
        // the lines are not z-fighting with the surface they hug.
        if let Some(hit) = target {
            let v = hit.voxel;
            let min = [
                v[0] as f32 - 0.005,
                v[1] as f32 - 0.005,
                v[2] as f32 - 0.005,
            ];
            let max = [
                v[0] as f32 + 1.005,
                v[1] as f32 + 1.005,
                v[2] as f32 + 1.005,
            ];
            renderer.debug_box_3d(min, max, [0.0, 0.0, 0.0, 0.9]);
        }
        let render_result = if let Some(ui) = ui {
            renderer.render(|d, q, e, v, s| ui.render(d, q, e, v, s))
        } else {
            renderer.render(|_, _, _, _, _| {})
        };
        if let Err(err) = render_result {
            warn!(error = ?err, "frame render failed");
        }

        if self.requested_quit {
            std::process::exit(0);
        }
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
