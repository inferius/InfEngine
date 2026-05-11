//! Sprite demo - Phase 5.2 of InfEngine.
//!
//! The scene is now data: entities live in an `infengine_core::Scene`
//! that is saved to / loaded from `assets/scene.ron`. The inspector
//! edits the selected entity in place; WASD / arrows move the selection.

use std::path::PathBuf;

use infengine_assets::{AssetEvent, AssetHandle, AssetServer, AssetWatcher, ImageAsset};
use infengine_audio::AudioSystem;
use infengine_core::scene::{AtlasRegion, Entity, EntityId, Scene, SpriteComponent, Transform2D};
use infengine_editor::EditorUi;
use infengine_render::{Renderer, SpriteInstance, TileId, Tilemap};
use infengine_window::raw_winit::event::WindowEvent;
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const TILE_SIZE: f32 = 64.0;
const TILEMAP_W: u32 = 40;
const TILEMAP_H: u32 = 30;
const MOVE_SPEED: f32 = 400.0;
const ZOOM_RATE: f32 = 1.6;
const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 4.0;

const SCENE_PATH: &str = "assets/scene.ron";

struct Demo {
    renderer: Option<Renderer>,
    editor: Option<EditorUi>,
    tilemap: Tilemap,
    assets: AssetServer,
    asset_watcher: Option<AssetWatcher>,
    sample_image: Option<AssetHandle<ImageAsset>>,
    asset_listing: Vec<PathBuf>,
    selected_asset: Option<PathBuf>,
    audio: Option<AudioSystem>,
    reload_count: u32,
    beep_count: u32,
    scene: Scene,
    selected: Option<EntityId>,
    save_status: String,
    zoom: f32,
    show_debug_overlay: bool,
    follow_camera: bool,
    viewport: (u32, u32),
    fps_ema: f32,
    frame: u64,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            renderer: None,
            editor: None,
            tilemap: build_tilemap(),
            assets: AssetServer::new(PathBuf::from("assets")),
            asset_watcher: None,
            sample_image: None,
            asset_listing: Vec::new(),
            selected_asset: None,
            audio: None,
            reload_count: 0,
            beep_count: 0,
            scene: Scene::default(),
            selected: None,
            save_status: String::new(),
            zoom: 1.0,
            show_debug_overlay: true,
            follow_camera: true,
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

fn default_scene() -> Scene {
    let mut scene = Scene {
        name: "demo".to_owned(),
        ..Scene::default()
    };
    let i = scene.add_entity("player");
    scene.entities[i].transform = Transform2D::default();
    scene.entities[i].sprite = Some(SpriteComponent {
        size: [64.0, 64.0],
        color: [1.0, 0.85, 0.4, 1.0],
        region: AtlasRegion::Checker,
    });
    let i = scene.add_entity("orb_a");
    scene.entities[i].transform.position = [-300.0, 200.0];
    scene.entities[i].sprite = Some(SpriteComponent {
        size: [80.0, 80.0],
        color: [0.4, 0.9, 1.0, 1.0],
        region: AtlasRegion::Dots,
    });
    let i = scene.add_entity("orb_b");
    scene.entities[i].transform.position = [300.0, -200.0];
    scene.entities[i].sprite = Some(SpriteComponent {
        size: [80.0, 80.0],
        color: [1.0, 0.6, 0.6, 1.0],
        region: AtlasRegion::Cross,
    });
    scene
}

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

impl Demo {
    fn refresh_asset_listing(&mut self) {
        match self.assets.list_files() {
            Ok(list) => self.asset_listing = list,
            Err(err) => warn!(error = %err, "failed to list assets"),
        }
    }
}

fn first_sprite_entity(scene: &Scene) -> Option<EntityId> {
    scene
        .entities
        .iter()
        .find(|e| e.sprite.is_some())
        .map(|e| e.id)
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

impl AppHandler for Demo {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, "sprite_demo started");

        match Renderer::new(window) {
            Ok(r) => {
                let editor = EditorUi::new(r.wgpu_device(), r.surface_format(), window.raw(), 1);
                self.editor = Some(editor);
                self.renderer = Some(r);
            }
            Err(err) => {
                error!(error = %err, "failed to initialize renderer");
                std::process::exit(2);
            }
        }

        if let Err(err) = ensure_sample_image(self.assets.root()) {
            warn!(error = %err, "failed to write sample.png");
        }
        if let Ok(handle) = self.assets.load_image("sample.png") {
            self.sample_image = Some(handle);
        }
        match self.assets.watch() {
            Ok(w) => self.asset_watcher = Some(w),
            Err(err) => warn!(error = %err, "filesystem watcher unavailable"),
        }
        match AudioSystem::new() {
            Ok(a) => self.audio = Some(a),
            Err(err) => warn!(error = %err, "audio system unavailable"),
        }

        // Load scene from disk if it exists; otherwise build a default
        // scene and persist it so the next run picks up the same content.
        let scene_path = PathBuf::from(SCENE_PATH);
        self.scene = match Scene::load(&scene_path) {
            Ok(s) => {
                info!(path = %scene_path.display(), entities = s.entities.len(), "scene loaded");
                s
            }
            Err(err) => {
                info!(error = %err, "scene file missing, building default");
                let s = default_scene();
                if let Err(save_err) = s.save(&scene_path) {
                    warn!(error = %save_err, "failed to write default scene");
                }
                s
            }
        };
        self.selected = first_sprite_entity(&self.scene);
        self.refresh_asset_listing();
    }

    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(editor) = self.editor.as_mut() {
            editor.handle_engine_event(window, event);
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

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.max(1e-4);
        let instant_fps = 1.0 / dt;
        self.fps_ema = self.fps_ema * 0.9 + instant_fps * 0.1;

        let mut any_event = false;
        if let Some(w) = self.asset_watcher.as_ref() {
            while let Ok(event) = w.receiver.try_recv() {
                any_event = true;
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
        if any_event {
            self.refresh_asset_listing();
        }

        // Movement input drives the selected entity.
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
        if (dx != 0.0 || dy != 0.0)
            && let Some(id) = self.selected
            && let Some(entity) = self.scene.get_mut(id)
        {
            let len = (dx * dx + dy * dy).sqrt();
            dx /= len;
            dy /= len;
            entity.transform.position[0] += dx * MOVE_SPEED * dt;
            entity.transform.position[1] += dy * MOVE_SPEED * dt;
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

        if ctx.input.is_pressed(Key::R)
            && let Some(id) = self.selected
            && let Some(entity) = self.scene.get_mut(id)
        {
            entity.transform.position = [0.0, 0.0];
            self.zoom = 1.0;
        }

        // Clamp the selected entity into the playable area.
        let (half_w, half_h) = map_half_extent();
        if let Some(id) = self.selected
            && let Some(entity) = self.scene.get_mut(id)
            && let Some(sprite) = &entity.sprite
        {
            let pad_x = sprite.size[0] * entity.transform.scale[0] * 0.5;
            let pad_y = sprite.size[1] * entity.transform.scale[1] * 0.5;
            entity.transform.position[0] =
                entity.transform.position[0].clamp(-(half_w - pad_x), half_w - pad_x);
            entity.transform.position[1] =
                entity.transform.position[1].clamp(-(half_h - pad_y), half_h - pad_y);
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        {
            let camera = renderer.camera_mut();
            if self.follow_camera
                && let Some(id) = self.selected
                && let Some(entity) = self.scene.get(id)
            {
                camera.position = entity.transform.position;
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

        renderer.draw_tilemap(&self.tilemap);
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

        if self.show_debug_overlay {
            renderer.debug_line([-24.0, 0.0], [24.0, 0.0], [1.0, 0.4, 0.4, 1.0]);
            renderer.debug_line([0.0, -24.0], [0.0, 24.0], [0.4, 1.0, 0.4, 1.0]);
            if let Some(id) = self.selected
                && let Some(entity) = self.scene.get(id)
                && let Some(sprite) = &entity.sprite
            {
                let half_x = sprite.size[0] * entity.transform.scale[0] * 0.5;
                let half_y = sprite.size[1] * entity.transform.scale[1] * 0.5;
                let p = entity.transform.position;
                renderer.debug_rect(
                    [p[0] - half_x, p[1] - half_y],
                    [p[0] + half_x, p[1] + half_y],
                    [1.0, 1.0, 0.0, 1.0],
                );
            }
        }

        let queued = renderer.queued_sprite_count();
        let fps = self.fps_ema;
        let reloads = self.reload_count;
        let beeps = self.beep_count;
        let (map_half_w, map_half_h) = map_half_extent();

        // Borrows for inspector edits.
        let zoom_ref = &mut self.zoom;
        let show_debug_ref = &mut self.show_debug_overlay;
        let follow_camera_ref = &mut self.follow_camera;
        let scene_ref = &mut self.scene;
        let selected_ref = &mut self.selected;
        let save_status_ref = &mut self.save_status;
        let asset_listing_ref = &self.asset_listing;
        let selected_asset_ref = &mut self.selected_asset;

        if let Some(editor) = self.editor.as_mut() {
            editor.run(window.raw(), |ctx| {
                egui::SidePanel::left("inspector")
                    .resizable(true)
                    .default_width(300.0)
                    .show(ctx, |ui| {
                        ui.heading("InfEngine - Phase 5.4");
                        ui.label(format!("FPS: {fps:.0}   Sprites: {queued}"));
                        ui.label(format!("Reloads: {reloads}   Beeps: {beeps}"));
                        ui.separator();

                        ui.collapsing("Scene", |ui| {
                            ui.horizontal(|ui| {
                                if ui.button("Save").clicked() {
                                    match scene_ref.save(SCENE_PATH) {
                                        Ok(()) => {
                                            *save_status_ref = format!("Saved -> {SCENE_PATH}");
                                        }
                                        Err(err) => {
                                            *save_status_ref = format!("Save failed: {err}");
                                        }
                                    }
                                }
                                if ui.button("Load").clicked() {
                                    match Scene::load(SCENE_PATH) {
                                        Ok(s) => {
                                            *scene_ref = s;
                                            *selected_ref = first_sprite_entity(scene_ref);
                                            *save_status_ref = format!("Loaded {SCENE_PATH}");
                                        }
                                        Err(err) => {
                                            *save_status_ref = format!("Load failed: {err}");
                                        }
                                    }
                                }
                                if ui.button("Reset").clicked() {
                                    *scene_ref = default_scene();
                                    *selected_ref = first_sprite_entity(scene_ref);
                                    *save_status_ref = "Reset to default scene".to_owned();
                                }
                            });
                            if !save_status_ref.is_empty() {
                                ui.small(save_status_ref.clone());
                            }
                            ui.horizontal(|ui| {
                                ui.label("Background");
                                ui.color_edit_button_rgb(&mut scene_ref.clear_color);
                            });
                        });

                        ui.collapsing("Camera", |ui| {
                            ui.checkbox(follow_camera_ref, "Follow selection");
                            ui.add(
                                egui::Slider::new(zoom_ref, ZOOM_MIN..=ZOOM_MAX)
                                    .text("Zoom")
                                    .logarithmic(true),
                            );
                            ui.checkbox(show_debug_ref, "Debug overlay");
                        });

                        ui.collapsing("Hierarchy", |ui| {
                            hierarchy_panel(ui, scene_ref, selected_ref);
                        });

                        ui.collapsing("Assets", |ui| {
                            asset_browser(ui, asset_listing_ref, selected_asset_ref);
                        });

                        ui.collapsing("Selection", |ui| {
                            entity_inspector(ui, scene_ref, selected_ref, map_half_w, map_half_h);
                        });

                        ui.separator();
                        ui.label("Controls:");
                        ui.label("WASD / arrows = move selection");
                        ui.label("Q / E = zoom out / in");
                        ui.label("Space = beep, R = recenter, Esc = quit");
                    });
            });
        }

        let render_result = if let Some(editor) = self.editor.as_mut() {
            renderer.render(|d, q, e, v, s| editor.render(d, q, e, v, s))
        } else {
            renderer.render(|_, _, _, _, _| {})
        };
        if let Err(err) = render_result {
            warn!(error = ?err, "frame render failed");
        }

        self.frame = self.frame.wrapping_add(1);
        if self.frame.is_multiple_of(120) {
            info!(
                frame = self.frame,
                fps = self.fps_ema,
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

fn asset_browser(ui: &mut egui::Ui, entries: &[PathBuf], selected: &mut Option<PathBuf>) {
    if entries.is_empty() {
        ui.label("(no files under assets/)");
        return;
    }
    egui::ScrollArea::vertical()
        .max_height(200.0)
        .show(ui, |ui| {
            for entry in entries {
                let label = entry.display().to_string();
                let is_selected = selected.as_ref() == Some(entry);
                if ui.selectable_label(is_selected, label).clicked() {
                    *selected = Some(entry.clone());
                }
            }
        });
    if let Some(path) = selected {
        ui.separator();
        ui.label(format!("Selected: {}", path.display()));
    }
}

fn hierarchy_panel(ui: &mut egui::Ui, scene: &mut Scene, selected: &mut Option<EntityId>) {
    let mut to_remove: Option<EntityId> = None;
    egui::ScrollArea::vertical()
        .max_height(200.0)
        .show(ui, |ui| {
            for entity in &scene.entities {
                let is_selected = *selected == Some(entity.id);
                let glyph = if entity.sprite.is_some() { "S " } else { "  " };
                let label = format!("{glyph}{} (#{})", entity.name, entity.id.0);
                ui.horizontal(|ui| {
                    if ui.selectable_label(is_selected, label).clicked() {
                        *selected = Some(entity.id);
                    }
                    if ui.small_button("X").clicked() {
                        to_remove = Some(entity.id);
                    }
                });
            }
        });
    if let Some(id) = to_remove {
        scene.remove(id);
        if *selected == Some(id) {
            *selected = first_sprite_entity(scene);
        }
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Add sprite entity").clicked() {
            let i = scene.add_entity(format!("entity_{}", scene.next_id));
            scene.entities[i].sprite =
                Some(SpriteComponent::new([64.0, 64.0], AtlasRegion::Checker));
            *selected = Some(scene.entities[i].id);
        }
        if ui.button("Add empty entity").clicked() {
            let i = scene.add_entity(format!("entity_{}", scene.next_id));
            *selected = Some(scene.entities[i].id);
        }
    });
}

fn entity_inspector(
    ui: &mut egui::Ui,
    scene: &mut Scene,
    selected: &mut Option<EntityId>,
    map_half_w: f32,
    map_half_h: f32,
) {
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
    transform_editor(ui, &mut entity.transform, map_half_w, map_half_h);
    sprite_editor(ui, entity);
}

fn transform_editor(ui: &mut egui::Ui, t: &mut Transform2D, map_w: f32, map_h: f32) {
    ui.add(egui::Slider::new(&mut t.position[0], -map_w..=map_w).text("X"));
    ui.add(egui::Slider::new(&mut t.position[1], -map_h..=map_h).text("Y"));
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
    ui.add(egui::Slider::new(&mut sprite.size[0], 8.0..=256.0).text("Width"));
    ui.add(egui::Slider::new(&mut sprite.size[1], 8.0..=256.0).text("Height"));
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = WindowConfig {
        title: "InfEngine - sprite_demo (Phase 5.4)".into(),
        width: 1280,
        height: 720,
        resizable: true,
    };

    if let Err(err) = run(config, Demo::default()) {
        tracing::error!(error = %err, "engine event loop failed");
        std::process::exit(1);
    }
}
