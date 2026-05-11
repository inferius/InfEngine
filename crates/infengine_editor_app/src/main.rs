//! Standalone editor binary for InfEngine projects.
//!
//! Usage:
//!
//! ```sh
//! editor_app                  # opens ./project.ron
//! editor_app path/to/project.ron
//! ```
//!
//! Phase 9.1 ships project loading, the scene picker, and the
//! per-entity inspector. Multi-window editing, gizmos, undo/redo, and
//! a Play mode arrive in later sub-phases.

use std::path::{Path, PathBuf};

use infengine_assets::AssetServer;
use infengine_core::project::{Project, ProjectError, ProjectScene};
use infengine_core::scene::{AtlasRegion, Entity, EntityId, Scene, SpriteComponent, Transform2D};
use infengine_editor::EditorUi;
use infengine_render::{Renderer, SpriteInstance};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const DEFAULT_PROJECT_FILE: &str = "project.ron";

const UNDO_LIMIT: usize = 64;

struct Editor {
    project_root: PathBuf,
    project_file: PathBuf,
    project: Project,
    current_scene_index: Option<usize>,
    scene: Scene,
    selected: Option<EntityId>,
    status: String,
    undo_stack: Vec<Scene>,
    redo_stack: Vec<Scene>,
    ctrl_held: bool,
    shift_held: bool,
    z_key_prev: bool,
    y_key_prev: bool,

    renderer: Option<Renderer>,
    editor_ui: Option<EditorUi>,
    #[allow(dead_code)]
    assets: AssetServer,
    viewport: (u32, u32),
    zoom: f32,
    fps_ema: f32,
    cursor_pos: [f32; 2],
    left_button: bool,
    left_button_prev: bool,
    dragging: Option<EntityId>,
    drag_offset: [f32; 2],
    drag_axis: DragAxis,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DragAxis {
    Free,
    X,
    Y,
}

const GIZMO_AXIS_LEN: f32 = 80.0;
const GIZMO_HANDLE_HALF: f32 = 8.0;
const GIZMO_PICK_THICKNESS: f32 = 14.0;

impl Editor {
    fn new(project_root: PathBuf, project_file: PathBuf, project: Project) -> Self {
        let asset_root = project.asset_root_full(&project_root);
        let assets = AssetServer::new(asset_root);
        let mut me = Self {
            project_root,
            project_file,
            project,
            current_scene_index: None,
            scene: Scene::default(),
            selected: None,
            status: String::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            ctrl_held: false,
            shift_held: false,
            z_key_prev: false,
            y_key_prev: false,
            renderer: None,
            editor_ui: None,
            assets,
            viewport: (1, 1),
            zoom: 1.0,
            fps_ema: 60.0,
            cursor_pos: [0.0, 0.0],
            left_button: false,
            left_button_prev: false,
            dragging: None,
            drag_offset: [0.0, 0.0],
            drag_axis: DragAxis::Free,
        };
        if !me.project.scenes.is_empty() {
            me.open_scene(0);
        }
        me
    }

    fn open_scene(&mut self, index: usize) {
        let Some(entry) = self.project.scenes.get(index).cloned() else {
            return;
        };
        let full = self.project.scene_full_path(&self.project_root, &entry);
        match Scene::load(&full) {
            Ok(s) => {
                self.scene = s;
                self.selected = self
                    .scene
                    .entities
                    .iter()
                    .find(|e| e.sprite.is_some())
                    .map(|e| e.id);
                self.current_scene_index = Some(index);
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.status = format!("Loaded {}", entry.name);
                info!(scene = entry.name, "scene opened");
            }
            Err(err) => {
                warn!(error = %err, path = %full.display(), "scene load failed");
                self.status = format!("Load failed: {err}");
            }
        }
    }

    fn save_current(&mut self) {
        let Some(idx) = self.current_scene_index else {
            self.status = "No scene selected".into();
            return;
        };
        let entry = self.project.scenes[idx].clone();
        let full = self.project.scene_full_path(&self.project_root, &entry);
        match self.scene.save(&full) {
            Ok(()) => self.status = format!("Saved {}", entry.name),
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn save_project(&mut self) {
        match self.project.save(&self.project_file) {
            Ok(()) => self.status = format!("Project saved -> {}", self.project_file.display()),
            Err(err) => self.status = format!("Project save failed: {err}"),
        }
    }

    fn add_scene(&mut self) {
        let suffix = self.project.scenes.len() + 1;
        let name = format!("scene_{suffix}");
        let file = format!("{name}.ron");
        let scene_path = PathBuf::from(&file);
        let full = self.project_root.join(&scene_path);

        let mut new_scene = Scene {
            name: name.clone(),
            ..Scene::default()
        };
        let i = new_scene.add_entity("entity_1");
        new_scene.entities[i].sprite =
            Some(SpriteComponent::new([64.0, 64.0], AtlasRegion::Checker));
        if let Err(err) = new_scene.save(&full) {
            self.status = format!("Failed to write {name}: {err}");
            return;
        }
        self.project.scenes.push(ProjectScene {
            name: name.clone(),
            path: scene_path,
        });
        let idx = self.project.scenes.len() - 1;
        let _ = self.project.save(&self.project_file);
        self.open_scene(idx);
        self.status = format!("Created {name}");
    }

    fn push_snapshot(&mut self) {
        if self.undo_stack.len() >= UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.scene.clone());
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack
                .push(std::mem::replace(&mut self.scene, prev));
            self.selected = self
                .scene
                .entities
                .iter()
                .find(|e| e.sprite.is_some())
                .map(|e| e.id);
            self.status = format!("Undo ({} left)", self.undo_stack.len());
        } else {
            self.status = "Nothing to undo".into();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack
                .push(std::mem::replace(&mut self.scene, next));
            self.selected = self
                .scene
                .entities
                .iter()
                .find(|e| e.sprite.is_some())
                .map(|e| e.id);
            self.status = format!("Redo ({} left)", self.redo_stack.len());
        } else {
            self.status = "Nothing to redo".into();
        }
    }

    fn camera_pos(&self) -> [f32; 2] {
        if let Some(id) = self.selected
            && let Some(e) = self.scene.get(id)
        {
            e.transform.position
        } else {
            [0.0, 0.0]
        }
    }
}

fn cursor_to_world(
    cursor: [f32; 2],
    viewport: (u32, u32),
    camera_pos: [f32; 2],
    zoom: f32,
) -> [f32; 2] {
    let half_w = viewport.0 as f32 * 0.5;
    let half_h = viewport.1 as f32 * 0.5;
    [
        camera_pos[0] + (cursor[0] - half_w) / zoom,
        camera_pos[1] - (cursor[1] - half_h) / zoom,
    ]
}

/// Hit-test the translate gizmo of the currently-selected entity.
/// Returns the axis the user clicked on, if any. The gizmo is drawn
/// in world units, so test against world-space coordinates.
fn pick_gizmo(scene: &Scene, selected: Option<EntityId>, world: [f32; 2]) -> Option<DragAxis> {
    let id = selected?;
    let entity = scene.get(id)?;
    let p = entity.transform.position;
    let dx = world[0] - p[0];
    let dy = world[1] - p[1];

    // Center handle (free).
    if dx.abs() <= GIZMO_HANDLE_HALF && dy.abs() <= GIZMO_HANDLE_HALF {
        return Some(DragAxis::Free);
    }
    // X handle: along [+0, +GIZMO_AXIS_LEN] with thickness.
    if (0.0..=GIZMO_AXIS_LEN).contains(&dx) && dy.abs() <= GIZMO_PICK_THICKNESS * 0.5 {
        return Some(DragAxis::X);
    }
    // Y handle.
    if (0.0..=GIZMO_AXIS_LEN).contains(&dy) && dx.abs() <= GIZMO_PICK_THICKNESS * 0.5 {
        return Some(DragAxis::Y);
    }
    None
}

fn pick_entity(scene: &Scene, world_point: [f32; 2]) -> Option<(EntityId, [f32; 2])> {
    let mut hit = None;
    for entity in &scene.entities {
        let Some(sprite) = &entity.sprite else {
            continue;
        };
        let half_x = sprite.size[0] * entity.transform.scale[0] * 0.5;
        let half_y = sprite.size[1] * entity.transform.scale[1] * 0.5;
        let p = entity.transform.position;
        if world_point[0] >= p[0] - half_x
            && world_point[0] <= p[0] + half_x
            && world_point[1] >= p[1] - half_y
            && world_point[1] <= p[1] + half_y
        {
            hit = Some((entity.id, [world_point[0] - p[0], world_point[1] - p[1]]));
        }
    }
    hit
}

fn region_label(r: &AtlasRegion) -> &'static str {
    match r {
        AtlasRegion::Full => "Full",
        AtlasRegion::Checker => "Checker",
        AtlasRegion::Dots => "Dots",
        AtlasRegion::Stripes => "Stripes",
        AtlasRegion::Cross => "Cross",
    }
}

impl AppHandler for Editor {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, project = %self.project.name, "editor_app started");
        match Renderer::new(window) {
            Ok(r) => {
                let editor = EditorUi::new(r.wgpu_device(), r.surface_format(), window.raw(), 1);
                self.editor_ui = Some(editor);
                self.renderer = Some(r);
            }
            Err(err) => {
                error!(error = %err, "renderer init failed");
                std::process::exit(2);
            }
        }
    }

    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(editor) = self.editor_ui.as_mut() {
            editor.handle_engine_event(window, event);
        }
        let egui_wants = self
            .editor_ui
            .as_ref()
            .is_some_and(|e| e.wants_pointer_input());
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = [position.x as f32, position.y as f32];
            }
            WindowEvent::MouseInput { state, button, .. } if *button == MouseButton::Left => {
                if !egui_wants {
                    self.left_button = matches!(state, ElementState::Pressed);
                } else if matches!(state, ElementState::Released) {
                    self.left_button = false;
                }
            }
            _ => {}
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.max(1e-4);
        self.fps_ema = self.fps_ema * 0.9 + (1.0 / dt) * 0.1;

        // Modifier + Z / Y edge-triggered undo / redo.
        self.ctrl_held = ctx
            .input
            .any_pressed(&[Key::ControlLeft, Key::ControlRight]);
        self.shift_held = ctx.input.any_pressed(&[Key::ShiftLeft, Key::ShiftRight]);
        let z_now = ctx.input.is_pressed(Key::Z);
        let y_now = ctx.input.is_pressed(Key::Y);
        let z_just = z_now && !self.z_key_prev;
        let y_just = y_now && !self.y_key_prev;
        self.z_key_prev = z_now;
        self.y_key_prev = y_now;
        if self.ctrl_held && z_just {
            if self.shift_held {
                self.redo();
            } else {
                self.undo();
            }
        }
        if self.ctrl_held && y_just {
            self.redo();
        }

        let mut zoom_step = 0.0_f32;
        if ctx.input.is_pressed(Key::Q) {
            zoom_step -= 1.0;
        }
        if ctx.input.is_pressed(Key::E) {
            zoom_step += 1.0;
        }
        if zoom_step != 0.0 {
            let factor = (zoom_step * 1.6 * dt).exp();
            self.zoom = (self.zoom * factor).clamp(0.25, 4.0);
        }

        let cursor_world =
            cursor_to_world(self.cursor_pos, self.viewport, self.camera_pos(), self.zoom);
        let left_just_pressed = self.left_button && !self.left_button_prev;
        let left_just_released = !self.left_button && self.left_button_prev;
        self.left_button_prev = self.left_button;
        let egui_pointer = self
            .editor_ui
            .as_ref()
            .is_some_and(|e| e.wants_pointer_input());

        if left_just_pressed && !egui_pointer {
            // Gizmo on the selected entity takes priority over body pick.
            let gizmo_hit = pick_gizmo(&self.scene, self.selected, cursor_world);
            if let Some(axis) = gizmo_hit
                && let Some(id) = self.selected
            {
                let p = self
                    .scene
                    .get(id)
                    .map(|e| e.transform.position)
                    .unwrap_or([0.0, 0.0]);
                self.push_snapshot();
                self.dragging = Some(id);
                self.drag_axis = axis;
                self.drag_offset = [cursor_world[0] - p[0], cursor_world[1] - p[1]];
            } else if let Some((id, ofs)) = pick_entity(&self.scene, cursor_world) {
                self.push_snapshot();
                self.selected = Some(id);
                self.dragging = Some(id);
                self.drag_axis = DragAxis::Free;
                self.drag_offset = ofs;
            }
        }
        if left_just_released {
            self.dragging = None;
        }
        if self.left_button
            && let Some(id) = self.dragging
            && let Some(entity) = self.scene.get_mut(id)
        {
            let new_x = cursor_world[0] - self.drag_offset[0];
            let new_y = cursor_world[1] - self.drag_offset[1];
            match self.drag_axis {
                DragAxis::Free => {
                    entity.transform.position[0] = new_x;
                    entity.transform.position[1] = new_y;
                }
                DragAxis::X => {
                    entity.transform.position[0] = new_x;
                }
                DragAxis::Y => {
                    entity.transform.position[1] = new_y;
                }
            }
        }

        let fps = self.fps_ema;
        let mut action_save = false;
        let mut action_save_all = false;
        let mut action_add_scene = false;
        let mut action_add_entity_sprite = false;
        let mut action_add_entity_empty = false;
        let mut action_undo = false;
        let mut action_redo = false;
        let mut switch_to: Option<usize> = None;
        let mut delete_entity: Option<EntityId> = None;
        let undo_count = self.undo_stack.len();
        let redo_count = self.redo_stack.len();

        if let Some(editor_ui) = self.editor_ui.as_mut() {
            let project_name = self.project.name.clone();
            let scenes = self.project.scenes.clone();
            let current = self.current_scene_index;
            let selected_id = self.selected;
            let status = self.status.clone();
            let scene_ref = &mut self.scene;
            let selected_ref = &mut self.selected;
            // Inspector UI.
            editor_ui.run(window.raw(), |ctx| {
                egui::SidePanel::left("inspector")
                    .resizable(true)
                    .default_width(320.0)
                    .show(ctx, |ui| {
                        ui.heading(format!("Project: {project_name}"));
                        ui.label(format!("FPS: {fps:.0}"));
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    undo_count > 0,
                                    egui::Button::new(format!("Undo ({undo_count})")),
                                )
                                .clicked()
                            {
                                action_undo = true;
                            }
                            if ui
                                .add_enabled(
                                    redo_count > 0,
                                    egui::Button::new(format!("Redo ({redo_count})")),
                                )
                                .clicked()
                            {
                                action_redo = true;
                            }
                        });
                        if !status.is_empty() {
                            ui.small(status.clone());
                        }
                        ui.separator();

                        ui.collapsing("Scenes", |ui| {
                            for (i, entry) in scenes.iter().enumerate() {
                                let selected = current == Some(i);
                                if ui.selectable_label(selected, &entry.name).clicked() {
                                    switch_to = Some(i);
                                }
                            }
                            ui.horizontal(|ui| {
                                if ui.button("Save scene").clicked() {
                                    action_save = true;
                                }
                                if ui.button("Save project").clicked() {
                                    action_save_all = true;
                                }
                                if ui.button("New scene").clicked() {
                                    action_add_scene = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Background");
                                ui.color_edit_button_rgb(&mut scene_ref.clear_color);
                            });
                        });

                        ui.collapsing("Hierarchy", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    for entity in &scene_ref.entities {
                                        let is_sel = Some(entity.id) == selected_id;
                                        let glyph =
                                            if entity.sprite.is_some() { "S " } else { "  " };
                                        let label =
                                            format!("{glyph}{} (#{})", entity.name, entity.id.0);
                                        ui.horizontal(|ui| {
                                            if ui.selectable_label(is_sel, label).clicked() {
                                                *selected_ref = Some(entity.id);
                                            }
                                            if ui.small_button("X").clicked() {
                                                delete_entity = Some(entity.id);
                                            }
                                        });
                                    }
                                });
                            ui.horizontal(|ui| {
                                if ui.button("Add sprite").clicked() {
                                    action_add_entity_sprite = true;
                                }
                                if ui.button("Add empty").clicked() {
                                    action_add_entity_empty = true;
                                }
                            });
                        });

                        ui.collapsing("Selection", |ui| {
                            inspector_for_selection(ui, scene_ref, selected_ref);
                        });

                        ui.separator();
                        ui.label("Q / E = zoom; click = pick + drag; Esc = quit");
                        ui.label("Ctrl+Z = undo; Ctrl+Y or Ctrl+Shift+Z = redo");
                    });
            });
        }

        if action_undo {
            self.undo();
        }
        if action_redo {
            self.redo();
        }
        // Apply deferred actions outside the immutable closure context.
        if let Some(id) = delete_entity {
            self.push_snapshot();
            self.scene.remove(id);
            if self.selected == Some(id) {
                self.selected = self
                    .scene
                    .entities
                    .iter()
                    .find(|e| e.sprite.is_some())
                    .map(|e| e.id);
            }
        }
        if let Some(idx) = switch_to {
            self.open_scene(idx);
        }
        if action_save {
            self.save_current();
        }
        if action_save_all {
            self.save_project();
        }
        if action_add_scene {
            self.add_scene();
        }
        if action_add_entity_sprite {
            self.push_snapshot();
            let i = self
                .scene
                .add_entity(format!("entity_{}", self.scene.next_id));
            self.scene.entities[i].sprite =
                Some(SpriteComponent::new([64.0, 64.0], AtlasRegion::Checker));
            self.selected = Some(self.scene.entities[i].id);
        }
        if action_add_entity_empty {
            self.push_snapshot();
            let i = self
                .scene
                .add_entity(format!("entity_{}", self.scene.next_id));
            self.selected = Some(self.scene.entities[i].id);
        }

        // 3. Render the world now that scene state is final.
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        {
            let camera = renderer.camera_mut();
            if let Some(id) = self.selected
                && let Some(e) = self.scene.get(id)
            {
                camera.position = e.transform.position;
            } else {
                camera.position = [0.0, 0.0];
            }
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
        if let Some(id) = self.selected
            && let Some(entity) = self.scene.get(id)
        {
            let p = entity.transform.position;
            if let Some(sprite) = &entity.sprite {
                let hx = sprite.size[0] * entity.transform.scale[0] * 0.5;
                let hy = sprite.size[1] * entity.transform.scale[1] * 0.5;
                renderer.debug_rect(
                    [p[0] - hx, p[1] - hy],
                    [p[0] + hx, p[1] + hy],
                    [1.0, 0.9, 0.2, 1.0],
                );
            }
            // Translate gizmo arms.
            let red = [1.0, 0.3, 0.3, 1.0];
            let green = [0.3, 1.0, 0.3, 1.0];
            let yellow = [1.0, 1.0, 0.3, 1.0];
            renderer.debug_line(p, [p[0] + GIZMO_AXIS_LEN, p[1]], red);
            renderer.debug_line(p, [p[0], p[1] + GIZMO_AXIS_LEN], green);
            // Tip arrows (small chevrons).
            let tip_x = [p[0] + GIZMO_AXIS_LEN, p[1]];
            renderer.debug_line(tip_x, [tip_x[0] - 10.0, tip_x[1] + 6.0], red);
            renderer.debug_line(tip_x, [tip_x[0] - 10.0, tip_x[1] - 6.0], red);
            let tip_y = [p[0], p[1] + GIZMO_AXIS_LEN];
            renderer.debug_line(tip_y, [tip_y[0] - 6.0, tip_y[1] - 10.0], green);
            renderer.debug_line(tip_y, [tip_y[0] + 6.0, tip_y[1] - 10.0], green);
            // Center handle.
            renderer.debug_rect(
                [p[0] - GIZMO_HANDLE_HALF, p[1] - GIZMO_HANDLE_HALF],
                [p[0] + GIZMO_HANDLE_HALF, p[1] + GIZMO_HANDLE_HALF],
                yellow,
            );
        }

        let render_result = if let Some(editor_ui) = self.editor_ui.as_mut() {
            renderer.render(|d, q, e, v, s| editor_ui.render(d, q, e, v, s))
        } else {
            renderer.render(|_, _, _, _, _| {})
        };
        if let Err(err) = render_result {
            warn!(error = ?err, "frame render failed");
        }
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        self.viewport = (width, height);
        if let Some(r) = self.renderer.as_mut() {
            r.resize(width, height);
        }
    }
}

fn inspector_for_selection(ui: &mut egui::Ui, scene: &mut Scene, selected: &mut Option<EntityId>) {
    let Some(id) = *selected else {
        ui.label("(no entity selected)");
        return;
    };
    let Some(entity) = scene.get_mut(id) else {
        ui.label("(selection lost)");
        return;
    };
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut entity.name);
    });
    ui.label(format!("Id: {}", id.0));
    transform_editor(ui, &mut entity.transform);
    sprite_editor(ui, entity);
}

fn transform_editor(ui: &mut egui::Ui, t: &mut Transform2D) {
    ui.add(
        egui::DragValue::new(&mut t.position[0])
            .prefix("X: ")
            .speed(1.0),
    );
    ui.add(
        egui::DragValue::new(&mut t.position[1])
            .prefix("Y: ")
            .speed(1.0),
    );
    ui.add(egui::Slider::new(&mut t.scale[0], 0.25..=4.0).text("Scale X"));
    ui.add(egui::Slider::new(&mut t.scale[1], 0.25..=4.0).text("Scale Y"));
}

fn sprite_editor(ui: &mut egui::Ui, entity: &mut Entity) {
    let has_sprite = entity.sprite.is_some();
    let mut keep = has_sprite;
    if has_sprite {
        ui.checkbox(&mut keep, "Sprite component");
    } else if ui.button("Add sprite component").clicked() {
        entity.sprite = Some(SpriteComponent::new([64.0, 64.0], AtlasRegion::Checker));
        keep = true;
    }
    if has_sprite && !keep {
        entity.sprite = None;
        return;
    }
    let Some(sprite) = entity.sprite.as_mut() else {
        return;
    };
    ui.add(egui::Slider::new(&mut sprite.size[0], 8.0..=512.0).text("Width"));
    ui.add(egui::Slider::new(&mut sprite.size[1], 8.0..=512.0).text("Height"));
    ui.horizontal(|ui| {
        ui.label("Color");
        ui.color_edit_button_rgba_unmultiplied(&mut sprite.color);
    });
    egui::ComboBox::from_label("Region")
        .selected_text(region_label(&sprite.region))
        .show_ui(ui, |ui| {
            for r in [
                AtlasRegion::Checker,
                AtlasRegion::Dots,
                AtlasRegion::Stripes,
                AtlasRegion::Cross,
                AtlasRegion::Full,
            ] {
                let label = region_label(&r);
                let is_selected =
                    std::mem::discriminant(&sprite.region) == std::mem::discriminant(&r);
                if ui.selectable_label(is_selected, label).clicked() {
                    sprite.region = r;
                }
            }
        });
}

fn load_or_create_project(path: &Path) -> Result<Project, ProjectError> {
    if path.exists() {
        Project::load(path)
    } else {
        let project = Project {
            name: "Untitled".to_owned(),
            asset_root: PathBuf::from("assets"),
            scenes: Vec::new(),
        };
        project.save(path)?;
        info!(path = %path.display(), "created blank project");
        Ok(project)
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let arg = std::env::args().nth(1);
    let project_file = PathBuf::from(arg.unwrap_or_else(|| DEFAULT_PROJECT_FILE.to_string()));
    let project_root = project_file
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let project = match load_or_create_project(&project_file) {
        Ok(p) => p,
        Err(err) => {
            error!(error = %err, "failed to open project");
            std::process::exit(1);
        }
    };
    info!(
        project = %project.name,
        scenes = project.scenes.len(),
        file = %project_file.display(),
        "project loaded"
    );

    let editor = Editor::new(project_root, project_file, project);
    let config = WindowConfig {
        title: format!("InfEngine Editor - {}", editor.project.name),
        width: 1440,
        height: 900,
        resizable: true,
    };
    if let Err(err) = run(config, editor) {
        error!(error = %err, "event loop failed");
        std::process::exit(1);
    }
}
