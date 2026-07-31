//! A6.6: the editor application - glue only.
//!
//! State lives here; behaviour lives in `edit`, `draw`, `view`, `ui` and
//! `project`. `on_update` is a sequence of calls into those, not a place
//! for logic.

use std::collections::HashMap;
use std::path::PathBuf;

use glam::Vec2;
use infengine_adventure::loca::LanguageChoice;
use infengine_adventure::{Animation, Room, find_path};
use infengine_audio::AudioSystem;
use infengine_editor::EditorUi;
use infengine_render::{Renderer, Texture2dHandle, TextureFilter};
use infengine_window::raw_winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key};
use tracing::{info, warn};

use crate::draw::{self, Overlays};
use crate::edit::{self, Selection, Tool};
use crate::playback::Playback;
use crate::project::Project;
use crate::ui::{self, UiActions, UiState};

/// How close, in screen pixels, the cursor must be to grab a vertex.
const GRAB_PIXELS: f32 = 7.0;
/// Smallest hotspot a drag may produce, in room units.
const MIN_HOTSPOT: f32 = 4.0;
/// Zoom change per wheel notch.
const ZOOM_STEP: f32 = 1.15;

/// What the mouse is currently doing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Drag {
    #[default]
    None,
    Pan,
    Vertex(edit::VertexRef),
    Hotspot(usize),
    HotspotRect(Vec2),
}

pub struct Editor {
    renderer: Option<Renderer>,
    ui: Option<EditorUi>,

    project: Project,
    current_room: Option<usize>,
    room: Room,
    room_path: Option<PathBuf>,
    dirty: bool,

    textures: HashMap<String, Texture2dHandle>,
    missing_images: Vec<String>,

    view: crate::view::View,
    overlays: Overlays,
    tool: Tool,
    selection: Selection,

    cursor: Vec2,
    last_cursor: Vec2,
    drag: Drag,
    floor_draft: Vec<Vec2>,
    walk_from: Option<Vec2>,
    path: Vec<Vec2>,
    room_filter: String,
    playback: Playback,
    /// Animations offered for this room: its own first, then the
    /// ones belonging to no room.
    room_animations: Vec<Animation>,
    /// Sheet textures, keyed by the image path they came from.
    anim_textures: HashMap<String, Texture2dHandle>,
    audio: Option<AudioSystem>,
    languages: Vec<String>,
    language: LanguageChoice,
    /// This room's lines: id, text in the chosen language, and
    /// whether a recording exists in the chosen voice language.
    lines: Vec<(String, String, bool)>,
    last_language: LanguageChoice,
    status: String,
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor")
            .field("room", &self.room.id)
            .field("dirty", &self.dirty)
            .finish()
    }
}

impl Editor {
    pub fn new(root: PathBuf) -> Self {
        let project = Project::open(&root);
        let status = if project.is_empty() {
            format!("no rooms under {}", project.root.display())
        } else {
            format!("{} rooms", project.rooms.len())
        };
        Self {
            renderer: None,
            ui: None,
            project,
            current_room: None,
            room: Room::default(),
            room_path: None,
            dirty: false,
            textures: HashMap::new(),
            missing_images: Vec::new(),
            view: crate::view::View::default(),
            overlays: Overlays::default(),
            tool: Tool::default(),
            selection: Selection::None,
            cursor: Vec2::ZERO,
            last_cursor: Vec2::ZERO,
            drag: Drag::None,
            floor_draft: Vec::new(),
            walk_from: None,
            path: Vec::new(),
            room_filter: String::new(),
            playback: Playback::default(),
            room_animations: Vec::new(),
            anim_textures: HashMap::new(),
            audio: AudioSystem::new().ok(),
            languages: Vec::new(),
            language: LanguageChoice::default(),
            lines: Vec::new(),
            last_language: LanguageChoice::default(),
            status,
        }
    }

    fn open_room(&mut self, index: usize) {
        let Some(path) = self.project.rooms.get(index).cloned() else {
            return;
        };
        match infengine_adventure::load(&path) {
            Ok(room) => {
                self.room = room;
                self.room_path = Some(path);
                self.current_room = Some(index);
                self.dirty = false;
                self.selection = Selection::None;
                self.floor_draft.clear();
                self.path.clear();
                self.walk_from = None;
                self.view.room_size = self.room.size;
                self.view.fit();
                self.load_textures();
                self.collect_animations();
                self.collect_lines();
                self.status = format!("opened {}", self.room.id);
                info!(room = %self.room.id, "opened");
            }
            Err(err) => {
                warn!(%err, "could not open room");
                self.status = err.to_string();
            }
        }
    }

    /// Upload every layer image the room references.
    ///
    /// Textures are keyed by the room-relative path, so two layers
    /// sharing a sheet upload it once.
    fn load_textures(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        for handle in self.textures.values() {
            renderer.destroy_texture(*handle);
        }
        self.textures.clear();
        self.missing_images.clear();

        for layer in &self.room.layers {
            if self.textures.contains_key(&layer.image) {
                continue;
            }
            let path = self.project.resolve(&layer.image);
            match renderer.create_texture_from_file(&path, TextureFilter::Nearest) {
                Ok(handle) => {
                    self.textures.insert(layer.image.clone(), handle);
                }
                Err(err) => {
                    warn!(image = %layer.image, %err, "layer image did not load");
                    self.missing_images.push(layer.image.clone());
                }
            }
        }
    }

    /// Gather the animations offered for the open room.
    ///
    /// The room's own first, then those tied to no room - cutscenes and
    /// title sequences are worth previewing from anywhere rather than
    /// being reachable only from whichever room happens to be open.
    fn collect_animations(&mut self) {
        self.playback.clear();
        self.room_animations = self
            .project
            .animations
            .for_room(&self.room.id)
            .chain(self.project.animations.loose())
            .cloned()
            .collect();
    }

    /// Gather the open room's dialogue in the chosen languages.
    fn collect_lines(&mut self) {
        if self.languages.is_empty() {
            self.languages = self
                .project
                .locales
                .languages()
                .into_iter()
                .map(str::to_string)
                .collect();
            if let Some(first) = self.languages.first() {
                self.language = LanguageChoice::same(first);
            }
        }
        let ids = self
            .project
            .lines_for_room(&self.room.id, &self.language.text);
        self.lines = ids
            .into_iter()
            .map(|id| {
                let text = self
                    .project
                    .locales
                    .text(&id, &self.language.text)
                    .to_string();
                let voiced = self
                    .project
                    .locales
                    .line(&id, &self.language.voice)
                    .is_some_and(|line| line.is_voiced());
                (id, text, voiced)
            })
            .collect();
    }

    /// Play the recording for a line, in the chosen voice language.
    fn play_line(&mut self, id: &str) {
        let Some(path) = self
            .project
            .locales
            .line(id, &self.language.voice)
            .and_then(|line| line.voice.clone())
        else {
            self.status = format!("{id} has no recording");
            return;
        };
        let Some(audio) = self.audio.as_mut() else {
            self.status = "no audio device".into();
            return;
        };
        let full = self.project.resolve(&path);
        match audio.load_sound(&full).and_then(|s| audio.play_sound(&s)) {
            Ok(()) => self.status = format!("playing {path}"),
            Err(err) => {
                warn!(%err, "could not play the recording");
                self.status = err.to_string();
            }
        }
    }

    /// Upload a sheet if it is not already loaded, and return it.
    fn animation_texture(&mut self, image: &str) -> Option<Texture2dHandle> {
        if let Some(handle) = self.anim_textures.get(image) {
            return Some(*handle);
        }
        let renderer = self.renderer.as_mut()?;
        let path = self.project.resolve(image);
        match renderer.create_texture_from_file(&path, TextureFilter::Nearest) {
            Ok(handle) => {
                self.anim_textures.insert(image.to_string(), handle);
                Some(handle)
            }
            Err(err) => {
                warn!(image, %err, "animation sheet did not load");
                None
            }
        }
    }

    fn save(&mut self) {
        let Some(path) = self.room_path.clone() else {
            return;
        };
        match infengine_adventure::save(&self.room, &path) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("saved {}", path.display());
            }
            Err(err) => {
                warn!(%err, "save failed");
                self.status = err.to_string();
            }
        }
    }

    fn apply_actions(&mut self, actions: UiActions) {
        if let Some(index) = actions.open_room {
            self.open_room(index);
        }
        if actions.save {
            self.save();
        }
        if actions.reload && let Some(index) = self.current_room {
            self.open_room(index);
        }
        if actions.fit_view {
            self.view.fit();
        }
        if actions.delete_selection {
            self.delete_selection();
        }
        if let Some(id) = actions.play_animation {
            self.playback.select(id);
        }
        if let Some(id) = actions.play_line {
            self.play_line(&id);
        }
    }

    fn delete_selection(&mut self) {
        match self.selection {
            Selection::Vertex(at) => {
                if edit::delete_vertex(&mut self.room, at) {
                    self.dirty = true;
                }
            }
            Selection::Hotspot(index) if index < self.room.hotspots.len() => {
                self.room.hotspots.remove(index);
                self.dirty = true;
            }
            _ => {}
        }
        self.selection = Selection::None;
    }

    /// Room-space position of the cursor.
    fn cursor_room(&self) -> Vec2 {
        self.view.window_to_room(self.cursor)
    }

    fn grab_radius(&self) -> f32 {
        self.view.screen_pixels_in_room(GRAB_PIXELS)
    }

    fn on_left_press(&mut self) {
        let p = self.cursor_room();
        match self.tool {
            Tool::Select => {
                if let Some(at) = edit::vertex_at(&self.room, p, self.grab_radius()) {
                    self.selection = Selection::Vertex(at);
                    self.drag = Drag::Vertex(at);
                } else if let Some(index) = edit::hotspot_at(&self.room, p) {
                    self.selection = Selection::Hotspot(index);
                    self.drag = Drag::Hotspot(index);
                } else {
                    self.selection = Selection::None;
                    self.drag = Drag::Pan;
                }
            }
            Tool::DrawFloor => {
                edit::extend_floor(&mut self.room, &mut self.floor_draft, p);
                self.dirty = true;
            }
            Tool::DrawHotspot => self.drag = Drag::HotspotRect(p),
            Tool::WalkPreview => self.preview_walk(p),
        }
    }

    /// Route the actor to `goal` and keep the result for drawing.
    fn preview_walk(&mut self, goal: Vec2) {
        let start = self.walk_from.or_else(|| {
            self.room
                .entries
                .first()
                .map(|e| e.position)
                .or_else(|| self.room.walk.clamp(goal))
        });
        let Some(start) = start else {
            self.status = "this room has no floor to walk on".into();
            return;
        };
        match find_path(&self.room.walk, start, goal) {
            Ok(path) => {
                self.status = format!(
                    "{} legs, {:.0} px",
                    path.len().saturating_sub(1),
                    infengine_adventure::path_length(&path)
                );
                self.walk_from = path.last().copied();
                self.path = path;
            }
            Err(err) => {
                self.path.clear();
                self.status = format!("no route: {err:?}");
            }
        }
    }

    fn on_left_release(&mut self) {
        if let Drag::HotspotRect(from) = self.drag {
            let to = self.cursor_room();
            if let Some(index) =
                edit::finish_hotspot_drag(&mut self.room, from, to, MIN_HOTSPOT)
            {
                self.selection = Selection::Hotspot(index);
                self.dirty = true;
            }
        }
        self.drag = Drag::None;
    }

    fn on_right_press(&mut self) {
        match self.tool {
            Tool::DrawFloor => {
                self.floor_draft.clear();
                self.status = "outline finished".into();
            }
            Tool::Select => {
                let p = self.cursor_room();
                let radius = self.grab_radius();
                if let Some(at) = edit::insert_vertex_on_edge(&mut self.room, p, radius) {
                    self.selection = Selection::Vertex(at);
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    fn on_cursor_move(&mut self) {
        let delta = self.cursor - self.last_cursor;
        match self.drag {
            Drag::Pan => self.view.pan(delta),
            Drag::Vertex(at) => {
                let target = self.cursor_room();
                if edit::move_vertex(&mut self.room, at, target) {
                    self.dirty = true;
                }
            }
            Drag::Hotspot(index) => {
                let room_delta = delta / self.view.zoom.max(f32::EPSILON);
                if edit::move_hotspot(&mut self.room, index, room_delta) {
                    self.dirty = true;
                }
            }
            Drag::None | Drag::HotspotRect(_) => {}
        }
    }

    /// Points drawn with the selection highlight.
    fn selected_points(&self) -> Vec<Vec2> {
        match self.selection {
            Selection::Vertex(at) => {
                let list = match at.kind {
                    edit::PolyKind::Solid => &self.room.walk.solids,
                    edit::PolyKind::Hole => &self.room.walk.holes,
                };
                list.get(at.polygon)
                    .and_then(|p| p.points.get(at.vertex))
                    .copied()
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

impl AppHandler for Editor {
    fn on_start(&mut self, window: &EngineWindow) {
        match Renderer::new(window) {
            Ok(mut renderer) => {
                renderer.set_clear_color(0.06, 0.06, 0.08, 1.0);
                self.ui = Some(EditorUi::new(
                    renderer.wgpu_device(),
                    renderer.surface_format(),
                    window.raw(),
                    1,
                ));
                self.renderer = Some(renderer);
            }
            Err(err) => warn!(%err, "renderer failed to start"),
        }
        let (w, h) = window.size();
        self.view.viewport = Vec2::new(w as f32, h as f32);
        if !self.project.is_empty() {
            self.open_room(0);
        }
    }

    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(ui) = self.ui.as_mut() {
            ui.handle_window_event(window.raw(), event);
        }
        let over_ui = self
            .ui
            .as_ref()
            .is_some_and(|ui| ui.context().wants_pointer_input());

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale = window.scale_factor() as f32;
                self.last_cursor = self.cursor;
                self.cursor = Vec2::new(position.x as f32, position.y as f32) / scale;
                if !over_ui || self.drag != Drag::None {
                    self.on_cursor_move();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = *state == ElementState::Pressed;
                match (button, pressed) {
                    (MouseButton::Left, true) if !over_ui => self.on_left_press(),
                    (MouseButton::Left, false) => self.on_left_release(),
                    (MouseButton::Right, true) if !over_ui => self.on_right_press(),
                    (MouseButton::Middle, true) if !over_ui => self.drag = Drag::Pan,
                    (MouseButton::Middle, false) => self.drag = Drag::None,
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !over_ui => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                if notches.abs() > f32::EPSILON {
                    self.view.zoom_at(self.cursor, ZOOM_STEP.powf(notches));
                }
            }
            _ => {}
        }
    }

    fn on_key(&mut self, key: Key, pressed: bool) {
        if !pressed {
            return;
        }
        match key {
            Key::Backspace => self.delete_selection(),
            Key::F => self.view.fit(),
            Key::S => self.save(),
            Key::Escape => {
                self.selection = Selection::None;
                self.floor_draft.clear();
                self.path.clear();
                self.walk_from = None;
            }
            _ => {}
        }
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(width, height);
            let (w, h) = renderer.camera_mut().viewport();
            self.view.viewport = Vec2::new(w as f32, h as f32);
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        if self.renderer.is_none() {
            return;
        }
        // Advance the preview before anything is drawn, so the frame
        // shown is the one for this instant.
        let current = self
            .playback
            .selected
            .clone()
            .and_then(|id| self.room_animations.iter().find(|a| a.id == id).cloned());
        self.playback.tick(ctx.delta_time, current.as_ref());
        let preview = current.and_then(|animation| {
            let frame = self.playback.frame(&animation);
            let handle = self.animation_texture(&animation.image)?;
            Some((animation, handle, frame))
        });

        // Compute what depends on &self before taking the renderer out
        // mutably; the borrow checker cannot see that they are disjoint.
        let selected = self.selected_points();
        if let Some(renderer) = self.renderer.as_mut() {
            draw::sync_camera(renderer, &self.view);
            draw::draw_layers(renderer, &self.room, &self.view, &self.textures);
            if let Some((animation, handle, frame)) = &preview {
                draw::draw_animation_frame(
                    renderer,
                    &self.view,
                    animation,
                    *handle,
                    *frame,
                    self.playback.position,
                );
            }
            draw::draw_overlays(
                renderer,
                &self.room,
                &self.view,
                &self.overlays,
                &selected,
                &self.path,
            );
        }

        // Build the UI, then apply what it asked for after the borrow of
        // the room ends.
        let mut actions = UiActions::default();
        let mut content = None;
        if let Some(ui) = self.ui.as_mut() {
            let mut state = UiState {
                project: &self.project,
                current_room: self.current_room,
                room: &mut self.room,
                tool: &mut self.tool,
                overlays: &mut self.overlays,
                selection: self.selection,
                dirty: self.dirty,
                missing_images: &self.missing_images,
                status: &self.status,
                room_filter: &mut self.room_filter,
                playback: &mut self.playback,
                animations: &self.room_animations,
                languages: &self.languages,
                text_language: &mut self.language.text,
                voice_language: &mut self.language.voice,
                lines: &self.lines,
                content_min: egui::Vec2::ZERO,
                content_max: egui::Vec2::ZERO,
            };
            ui.run(window.raw(), |ctx| {
                actions = ui::build(ctx, &mut state);
            });
            content = Some((state.content_min, state.content_max));
        }
        if let Some((min, max)) = content {
            self.view
                .set_content_rect(Vec2::new(min.x, min.y), Vec2::new(max.x, max.y));
        }

        if let (Some(renderer), Some(ui)) = (self.renderer.as_mut(), self.ui.as_mut()) {
            let _ = renderer.render(|device, queue, encoder, view, size| {
                ui.render(device, queue, encoder, view, size);
            });
        }

        // The picker writes straight into the choice, so notice the
        // change here rather than routing it through an action.
        let chosen = self.language.clone();
        if chosen != self.last_language {
            self.last_language = chosen;
            self.collect_lines();
        }

        self.apply_actions(actions);
    }
}
