//! Phase 11 demo game.
//!
//! A small third-person showcase that stitches together the
//! renderer (PBR mesh + shadow + albedo texture), the windowing
//! input, and the egui-based UI layer. The goal is not to ship a
//! polished game but to prove the engine surfaces are usable end
//! to end: main menu -> 3D play state -> pause + settings ->
//! inventory pickup.
//!
//! Controls (in Play):
//!   WASD       move
//!   Space      jump (placeholder vertical impulse)
//!   E          interact / pick up nearby item
//!   I          toggle inventory
//!   Esc        pause
//!
//! The world is generated procedurally on first start: a flat
//! ground plate plus a handful of coloured cubes placed in a
//! ring. Picking up a cube removes it from the world and adds it
//! to the inventory.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};
use infengine_editor::EditorUi;
use infengine_render::Renderer;
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum ItemKind {
    Coin,
    Gem,
    Key,
}

impl ItemKind {
    fn label(self) -> &'static str {
        match self {
            ItemKind::Coin => "Coin",
            ItemKind::Gem => "Gem",
            ItemKind::Key => "Key",
        }
    }

    fn color(self) -> [f32; 4] {
        match self {
            ItemKind::Coin => [1.0, 0.82, 0.20, 1.0],
            ItemKind::Gem => [0.30, 0.70, 1.0, 1.0],
            ItemKind::Key => [0.85, 0.85, 0.95, 1.0],
        }
    }

    fn metallic(self) -> f32 {
        match self {
            ItemKind::Coin | ItemKind::Key => 0.9,
            ItemKind::Gem => 0.1,
        }
    }

    fn roughness(self) -> f32 {
        match self {
            ItemKind::Coin => 0.30,
            ItemKind::Gem => 0.10,
            ItemKind::Key => 0.45,
        }
    }
}

#[derive(Debug, Clone)]
struct Item {
    kind: ItemKind,
    position: Vec3,
    bob_phase: f32,
}

#[derive(Debug, Clone)]
struct Settings {
    master_volume: f32,
    mouse_sensitivity: f32,
    show_fps: bool,
    /// Confine + lock the mouse to the window while in Play state.
    lock_cursor: bool,
    /// Hide the OS cursor while in Play state.
    hide_cursor: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            master_volume: 0.75,
            mouse_sensitivity: 0.6,
            show_fps: true,
            lock_cursor: true,
            hide_cursor: true,
        }
    }
}

struct DemoGame {
    state: AppState,
    settings: Settings,
    player_pos: Vec3,
    player_vel: Vec3,
    player_yaw: f32,
    items: Vec<Item>,
    inventory: HashMap<ItemKind, u32>,
    inventory_open: bool,
    elapsed: f32,
    fps_ema: f32,
    pickup_message: Option<(String, f32)>,
    requested_quit: bool,

    renderer: Option<Renderer>,
    ui: Option<EditorUi>,
    viewport: (u32, u32),
    esc_prev: bool,
    e_prev: bool,
    i_prev: bool,
    space_prev: bool,
    /// Last reported cursor position; `None` after a state change so the
    /// next CursorMoved event seeds the previous-position cache without
    /// producing a huge yaw spike.
    last_mouse: Option<[f32; 2]>,
    /// Accumulated horizontal mouse delta in physical pixels collected
    /// since the previous `update_play` call.
    mouse_dx_accum: f32,
}

const ITEM_PICKUP_RADIUS: f32 = 1.2;
const GROUND_SIZE: f32 = 30.0;
const PLAYER_SPEED: f32 = 4.0;
const JUMP_VELOCITY: f32 = 4.5;
const GRAVITY: f32 = -12.0;
const PLAYER_HEIGHT: f32 = 0.9;

impl DemoGame {
    fn new() -> Self {
        let mut me = Self {
            state: AppState::MainMenu,
            settings: Settings::default(),
            player_pos: Vec3::new(0.0, PLAYER_HEIGHT, 0.0),
            player_vel: Vec3::ZERO,
            player_yaw: 0.0,
            items: Vec::new(),
            inventory: HashMap::new(),
            inventory_open: false,
            elapsed: 0.0,
            fps_ema: 60.0,
            pickup_message: None,
            requested_quit: false,
            renderer: None,
            ui: None,
            viewport: (1, 1),
            esc_prev: false,
            e_prev: false,
            i_prev: false,
            space_prev: false,
            last_mouse: None,
            mouse_dx_accum: 0.0,
        };
        me.reset_world();
        me
    }

    fn reset_world(&mut self) {
        self.player_pos = Vec3::new(0.0, PLAYER_HEIGHT, 0.0);
        self.player_vel = Vec3::ZERO;
        self.player_yaw = 0.0;
        self.inventory.clear();
        self.pickup_message = None;
        self.items.clear();
        let kinds = [
            ItemKind::Coin,
            ItemKind::Gem,
            ItemKind::Key,
            ItemKind::Coin,
            ItemKind::Coin,
            ItemKind::Gem,
            ItemKind::Coin,
            ItemKind::Key,
            ItemKind::Gem,
            ItemKind::Coin,
        ];
        let count = kinds.len();
        let radius = 7.5;
        for (i, kind) in kinds.iter().enumerate() {
            let angle = (i as f32 / count as f32) * std::f32::consts::TAU;
            let x = angle.cos() * radius;
            let z = angle.sin() * radius;
            self.items.push(Item {
                kind: *kind,
                position: Vec3::new(x, 0.6, z),
                bob_phase: i as f32 * 0.6,
            });
        }
    }

    fn update_play(&mut self, ctx: &FrameContext<'_>, dt: f32) {
        // Yaw is mouse-driven: consume the horizontal pixel delta
        // collected in on_window_event since the previous update and
        // scale by the settings sensitivity. Negative dx -> turn left
        // (counter-clockwise in our y-up world).
        let dx = std::mem::take(&mut self.mouse_dx_accum);
        let sensitivity = self.settings.mouse_sensitivity * 0.0035;
        self.player_yaw -= dx * sensitivity;
        let forward = Vec3::new(self.player_yaw.cos(), 0.0, -self.player_yaw.sin());
        // W/S = forward/back, A/D = strafe.
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
        let right = Vec3::new(forward.z, 0.0, -forward.x);
        let horizontal = (forward * move_axis + right * strafe).normalize_or_zero() * PLAYER_SPEED;
        self.player_vel.x = horizontal.x;
        self.player_vel.z = horizontal.z;

        // Vertical: gravity-driven jump.
        let space_now = ctx.input.is_pressed(Key::Space);
        let space_just = space_now && !self.space_prev;
        self.space_prev = space_now;
        if space_just && self.player_pos.y <= PLAYER_HEIGHT + 0.01 {
            self.player_vel.y = JUMP_VELOCITY;
        }
        self.player_vel.y += GRAVITY * dt;
        self.player_pos += self.player_vel * dt;
        if self.player_pos.y < PLAYER_HEIGHT {
            self.player_pos.y = PLAYER_HEIGHT;
            self.player_vel.y = 0.0;
        }
        // Keep the player on the ground plate.
        let half = GROUND_SIZE * 0.5 - 0.5;
        self.player_pos.x = self.player_pos.x.clamp(-half, half);
        self.player_pos.z = self.player_pos.z.clamp(-half, half);

        // Pickup: edge-trigger E to grab the nearest item within radius.
        let e_now = ctx.input.is_pressed(Key::E);
        let e_just = e_now && !self.e_prev;
        self.e_prev = e_now;
        if e_just {
            let mut nearest: Option<(usize, f32)> = None;
            for (i, item) in self.items.iter().enumerate() {
                let d = (item.position - self.player_pos).length();
                if d < ITEM_PICKUP_RADIUS && nearest.is_none_or(|(_, best)| d < best) {
                    nearest = Some((i, d));
                }
            }
            if let Some((i, _)) = nearest {
                let item = self.items.remove(i);
                *self.inventory.entry(item.kind).or_insert(0) += 1;
                self.pickup_message = Some((format!("Picked up {}", item.kind.label()), 2.0));
            }
        }

        // Inventory toggle (I).
        let i_now = ctx.input.is_pressed(Key::I);
        let i_just = i_now && !self.i_prev;
        self.i_prev = i_now;
        if i_just {
            self.inventory_open = !self.inventory_open;
        }

        // Item bob animation.
        for item in &mut self.items {
            item.bob_phase += dt * 2.0;
        }
        if let Some((_, ref mut t)) = self.pickup_message {
            *t -= dt;
        }
        if matches!(self.pickup_message, Some((_, t)) if t <= 0.0) {
            self.pickup_message = None;
        }
    }

    fn apply_cursor_settings(&self, window: &EngineWindow) {
        use infengine_window::raw_winit::window::CursorGrabMode;
        let playing = matches!(self.state, AppState::Playing);
        let raw = window.raw();
        // Three modes:
        //  - Playing + lock + hide: Locked grab (cursor pinned to one
        //    point) + invisible. Yaw is driven by raw DeviceEvent
        //    deltas, so the cursor staying put is desirable.
        //  - Playing + lock only:   Confined grab (visible cursor that
        //    cannot leave the window).
        //  - everything else:       no grab.
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
            // Wayland / X11 prefer the opposite mode; fall back.
            let _ = raw.set_cursor_grab(CursorGrabMode::Confined);
        }
        raw.set_cursor_visible(!(playing && self.settings.hide_cursor));
    }

    fn render_world(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        renderer.set_clear_color(0.18, 0.22, 0.30, 1.0);
        // Aim the camera behind and slightly above the player.
        let forward = Vec3::new(self.player_yaw.cos(), 0.0, -self.player_yaw.sin());
        let cam_offset = -forward * 6.0 + Vec3::Y * 3.5;
        let camera = renderer.camera_3d_mut();
        camera.position = self.player_pos + cam_offset;
        camera.target = self.player_pos + Vec3::Y * 0.5;
        camera.up = Vec3::Y;

        let cube = renderer.cube_handle();
        // Ground plate.
        let ground = Mat4::from_scale_rotation_translation(
            Vec3::new(GROUND_SIZE, 0.2, GROUND_SIZE),
            Quat::IDENTITY,
            Vec3::new(0.0, -0.1, 0.0),
        );
        renderer.draw_mesh_pbr(
            cube,
            ground.to_cols_array(),
            [0.55, 0.55, 0.58, 1.0],
            0.0,
            0.85,
        );

        // Decorative pillars at the corners so shadows are visible.
        let pillar_color = [0.45, 0.40, 0.35, 1.0];
        let h = GROUND_SIZE * 0.5 - 1.0;
        for &(x, z) in &[(-h, -h), (h, -h), (-h, h), (h, h)] {
            let m = Mat4::from_scale_rotation_translation(
                Vec3::new(0.8, 3.0, 0.8),
                Quat::IDENTITY,
                Vec3::new(x, 1.5, z),
            );
            renderer.draw_mesh_pbr(cube, m.to_cols_array(), pillar_color, 0.1, 0.7);
        }

        // Player.
        let player_mat = Mat4::from_scale_rotation_translation(
            Vec3::new(0.6, 0.9, 0.6),
            Quat::from_rotation_y(self.player_yaw),
            self.player_pos,
        );
        renderer.draw_mesh_pbr(
            cube,
            player_mat.to_cols_array(),
            [0.30, 0.85, 0.55, 1.0],
            0.0,
            0.45,
        );

        // Items.
        for item in &self.items {
            let bob = (item.bob_phase.sin() * 0.15) + 0.5;
            let m = Mat4::from_scale_rotation_translation(
                Vec3::splat(0.35),
                Quat::from_rotation_y(item.bob_phase),
                item.position + Vec3::Y * bob,
            );
            renderer.draw_mesh_pbr(
                cube,
                m.to_cols_array(),
                item.kind.color(),
                item.kind.metallic(),
                item.kind.roughness(),
            );
        }
    }

    fn render_ui(&mut self, window: &EngineWindow) {
        let Some(ui) = self.ui.as_mut() else {
            return;
        };
        let state = self.state;
        let settings = self.settings.clone();
        let inventory = self.inventory.clone();
        let inventory_open = self.inventory_open;
        let pickup_message = self.pickup_message.clone();
        let fps = self.fps_ema;
        let show_fps = self.settings.show_fps;
        let mut action_start = false;
        let mut action_resume = false;
        let mut action_pause_to_settings = false;
        let mut action_menu_to_settings = false;
        let mut action_to_menu = false;
        let mut action_back_settings = false;
        let mut action_quit = false;
        let mut settings_edit = settings.clone();
        let mut inventory_close = false;
        ui.run(window.raw(), |ctx| match state {
            AppState::MainMenu => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading("InfEngine Demo");
                        ui.label("Phase 11 showcase");
                        ui.add_space(40.0);
                        if ui
                            .add_sized([220.0, 40.0], egui::Button::new("Play"))
                            .clicked()
                        {
                            action_start = true;
                        }
                        ui.add_space(8.0);
                        if ui
                            .add_sized([220.0, 40.0], egui::Button::new("Settings"))
                            .clicked()
                        {
                            action_menu_to_settings = true;
                        }
                        ui.add_space(8.0);
                        if ui
                            .add_sized([220.0, 40.0], egui::Button::new("Quit"))
                            .clicked()
                        {
                            action_quit = true;
                        }
                    });
                });
            }
            AppState::Playing => {
                egui::TopBottomPanel::top("hud_top").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Coins: {}   Gems: {}   Keys: {}",
                            inventory.get(&ItemKind::Coin).copied().unwrap_or(0),
                            inventory.get(&ItemKind::Gem).copied().unwrap_or(0),
                            inventory.get(&ItemKind::Key).copied().unwrap_or(0),
                        ));
                        ui.separator();
                        ui.label(
                            "Mouse aim, WASD move, Space jump, E pick up, I inventory, Esc menu",
                        );
                        if show_fps {
                            ui.separator();
                            ui.label(format!("FPS: {fps:.0}"));
                        }
                    });
                });
                if let Some((msg, _)) = pickup_message.as_ref() {
                    egui::TopBottomPanel::bottom("hud_bottom").show(ctx, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.colored_label(egui::Color32::from_rgb(255, 220, 120), msg.as_str());
                        });
                    });
                }
                if inventory_open {
                    egui::Window::new("Inventory")
                        .collapsible(false)
                        .resizable(false)
                        .default_pos([100.0, 100.0])
                        .show(ctx, |ui| {
                            for kind in [ItemKind::Coin, ItemKind::Gem, ItemKind::Key] {
                                let count = inventory.get(&kind).copied().unwrap_or(0);
                                ui.horizontal(|ui| {
                                    let color = kind.color();
                                    let swatch = egui::Color32::from_rgba_premultiplied(
                                        (color[0] * 255.0) as u8,
                                        (color[1] * 255.0) as u8,
                                        (color[2] * 255.0) as u8,
                                        255,
                                    );
                                    ui.colored_label(swatch, "*");
                                    ui.label(format!("{}: {count}", kind.label()));
                                });
                            }
                            ui.separator();
                            if ui.button("Close (I)").clicked() {
                                inventory_close = true;
                            }
                        });
                }
            }
            AppState::Paused => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.heading("Paused");
                        ui.add_space(20.0);
                        if ui
                            .add_sized([200.0, 36.0], egui::Button::new("Resume"))
                            .clicked()
                        {
                            action_resume = true;
                        }
                        if ui
                            .add_sized([200.0, 36.0], egui::Button::new("Settings"))
                            .clicked()
                        {
                            action_pause_to_settings = true;
                        }
                        if ui
                            .add_sized([200.0, 36.0], egui::Button::new("Main menu"))
                            .clicked()
                        {
                            action_to_menu = true;
                        }
                        if ui
                            .add_sized([200.0, 36.0], egui::Button::new("Quit"))
                            .clicked()
                        {
                            action_quit = true;
                        }
                    });
                });
            }
            AppState::Settings { .. } => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.separator();
                    ui.add(
                        egui::Slider::new(&mut settings_edit.master_volume, 0.0..=1.0)
                            .text("Master volume"),
                    );
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
        });
        self.settings = settings_edit;
        if inventory_close {
            self.inventory_open = false;
        }
        if action_start {
            self.reset_world();
            self.state = AppState::Playing;
        }
        if action_resume {
            self.state = AppState::Playing;
        }
        if action_pause_to_settings {
            self.state = AppState::Settings {
                back_to: BackTo::Pause,
            };
        }
        if action_menu_to_settings {
            self.state = AppState::Settings {
                back_to: BackTo::MainMenu,
            };
        }
        if action_to_menu {
            self.state = AppState::MainMenu;
        }
        if action_back_settings && let AppState::Settings { back_to } = self.state {
            self.state = match back_to {
                BackTo::MainMenu => AppState::MainMenu,
                BackTo::Pause => AppState::Paused,
            };
        }
        if action_quit {
            self.requested_quit = true;
        }
    }
}

impl AppHandler for DemoGame {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, "demo_game started");
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
    }

    fn on_window_event(
        &mut self,
        window: &EngineWindow,
        event: &infengine_window::raw_winit::event::WindowEvent,
    ) {
        if let Some(ui) = self.ui.as_mut() {
            ui.handle_engine_event(window, event);
        }
        // CursorMoved is only used as a fallback when the cursor is
        // free (neither locked nor hidden); the FPS path consumes raw
        // mouse deltas via on_device_event instead.
        use infengine_window::raw_winit::event::WindowEvent;
        if let WindowEvent::CursorMoved { position, .. } = event
            && matches!(self.state, AppState::Playing)
            && !(self.settings.lock_cursor || self.settings.hide_cursor)
        {
            let now = [position.x as f32, position.y as f32];
            if let Some(prev) = self.last_mouse {
                self.mouse_dx_accum += now[0] - prev[0];
            }
            self.last_mouse = Some(now);
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
            self.mouse_dx_accum += delta.0 as f32;
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.max(1e-4);
        self.elapsed += dt;
        self.fps_ema = self.fps_ema * 0.9 + (1.0 / dt) * 0.1;

        // Esc transitions between Playing <-> Paused.
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
                // From the menu Esc is a no-op; the user must use the
                // Quit button to leave.
                AppState::MainMenu => {}
            }
        }
        // Reset the mouse delta cache whenever the player leaves Play
        // mode so re-entry doesn't apply a stale jump.
        if !matches!(self.state, AppState::Playing) {
            self.mouse_dx_accum = 0.0;
        }

        if matches!(self.state, AppState::Playing) {
            self.update_play(ctx, dt);
        }
        self.apply_cursor_settings(window);
        self.render_world();
        self.render_ui(window);

        if self.requested_quit {
            // Soft-exit: stop rendering and let the OS close the window.
            warn!("quit requested - exiting");
            std::process::exit(0);
        }

        let render_result = if let Some(ui) = self.ui.as_mut() {
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.render(|d, q, e, v, s| ui.render(d, q, e, v, s))
            } else {
                Ok(())
            }
        } else if let Some(renderer) = self.renderer.as_mut() {
            renderer.render(|_, _, _, _, _| {})
        } else {
            Ok(())
        };
        if let Err(err) = render_result {
            warn!(error = ?err, "frame render failed");
        }
    }

    fn on_resize(&mut self, w: u32, h: u32) {
        self.viewport = (w, h);
        if let Some(r) = self.renderer.as_mut() {
            r.resize(w, h);
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let config = WindowConfig {
        title: "InfEngine - Demo Game".to_owned(),
        width: 1280,
        height: 800,
        resizable: true,
    };
    if let Err(err) = run(config, DemoGame::new()) {
        error!(error = %err, "event loop failed");
        std::process::exit(1);
    }
}
