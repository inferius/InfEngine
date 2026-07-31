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
use infengine_render::{Renderer, Texture2dHandle};
use infengine_window::raw_winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key};
use tracing::{info, warn};

use crate::assets;
use crate::draw::{self, Overlays};
use crate::edit::{self, Selection, Tool};
use crate::play::{self, PlayMode};
use crate::playback::Playback;
use crate::project::Project;
use crate::sheet::SheetDraft;
use crate::ui::{self, UiActions, UiState};
use crate::ui_play::{self, PlayDraft};

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
    play: PlayMode,
    play_draft: PlayDraft,
    /// The sheet being cut into frames, if any.
    sheet: Option<SheetDraft>,
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
        let play = PlayMode::with_scripts(project.scripts.clone());
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
            play,
            play_draft: PlayDraft::default(),
            sheet: None,
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

    fn load_textures(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        self.missing_images =
            assets::upload_layers(renderer, &self.project, &mut self.textures, &self.room);
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
        self.status = match assets::play_file(self.audio.as_mut(), &self.project, &path) {
            Ok(message) | Err(message) => message,
        };
    }

    /// Start cutting a loose image into frames.
    ///
    /// The sheet has to be uploaded first: its size is what decides
    /// every sensible frame width, and guessing without it would only
    /// produce a cut that has to be thrown away.
    fn open_sheet(&mut self, image: &str) {
        let Some(handle) = self.animation_texture(image) else {
            self.status = format!("{image} did not load");
            return;
        };
        let Some((width, height)) = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.texture_size(handle))
        else {
            return;
        };
        self.sheet = Some(SheetDraft::guess(
            image,
            Vec2::new(width as f32, height as f32),
        ));
        self.status = format!("cutting {image}");
    }

    /// Add the draft to the project's animations and write the index.
    fn save_sheet(&mut self) {
        let Some(draft) = self.sheet.as_ref() else {
            return;
        };
        let mut animation = draft.to_animation();
        // An empty room field means "this room" - the checkbox cannot
        // know the room's name, and asking the user to retype it would
        // be a way to get it wrong.
        if animation.room.as_deref() == Some("") {
            animation.room = Some(self.room.id.clone());
        }
        let id = animation.id.clone();
        self.project.animations.insert(animation);
        let dir = self.project.root.join(infengine_adventure::anim::ANIMATION_DIR);
        match infengine_adventure::anim::save_index(&self.project.animations, &dir) {
            Ok(()) => {
                self.status = format!("added {id}");
                self.sheet = None;
                self.collect_animations();
                self.playback.select(id);
            }
            Err(err) => {
                warn!(%err, "could not write the animation index");
                self.status = err.to_string();
            }
        }
    }

    /// Upload a sheet if it is not already loaded, and return it.
    fn animation_texture(&mut self, image: &str) -> Option<Texture2dHandle> {
        let renderer = self.renderer.as_mut()?;
        assets::texture_for(renderer, &self.project, &mut self.anim_textures, image)
    }

    /// Enter play mode in the open room.
    fn start_play(&mut self) {
        if self.project.scripts.is_empty() {
            self.status = "this project has no scripts to run".into();
        }
        self.play.begin(&self.room);
        self.status = format!("playing {}", self.room.id);
    }

    /// Carry out what a play tick asked for.
    ///
    /// Split out because it is the only part of play mode that touches
    /// files and devices; everything else runs headless in `play`.
    fn perform(&mut self, request: play::Request) {
        if let Some(id) = request.speak {
            self.play_voice(&id);
        }
        if let Some(animation) = request.animation {
            self.playback.select(animation);
        }
        if let Some(sound) = request.sound {
            self.play_file(&sound);
        }
        if let Some(music) = request.music {
            match music {
                Some(track) => self.play_file(&track),
                // Nothing to stop yet: music is one-shot playback until
                // the audio system grows a handle to hold on to.
                None => self.status = "music off".into(),
            }
        }
        if let Some((room, entry)) = request.enter_room {
            self.enter_room_by_id(&room, entry.as_deref());
        }
    }

    /// Load the room a script asked for and hand it back to the game.
    fn enter_room_by_id(&mut self, id: &str, entry: Option<&str>) {
        let target = self
            .project
            .rooms
            .iter()
            .position(|path| Project::room_label(path) == id);
        let Some(index) = target else {
            // Deliberately left blocked: the script stalls where the
            // missing room is, which is the bug the test run just found.
            self.play.note(format!("MISSING room {id}"));
            self.status = format!("no room file for {id}");
            return;
        };
        self.open_room(index);
        self.play.room_loaded(&self.room, entry);
    }

    /// Play a recording by localisation id.
    fn play_voice(&mut self, id: &str) {
        let path = self
            .project
            .locales
            .line(id, &self.language.voice)
            .and_then(|line| line.voice.clone());
        match path {
            Some(path) => self.play_file(&path),
            None => self.play.note(format!("no recording for {id}")),
        }
    }

    /// Play a file given as a project-relative path, noting failures in
    /// the play log rather than the status line - during a test run the
    /// log is the thing being read.
    fn play_file(&mut self, relative: &str) {
        if let Err(message) = assets::play_file(self.audio.as_mut(), &self.project, relative) {
            self.play.note(message);
        }
    }

    /// Apply what the play panel asked for.
    fn apply_play_actions(&mut self, actions: ui_play::PlayActions) {
        if actions.start {
            self.start_play();
        }
        if actions.stop {
            self.play.end();
            self.status = "stopped".into();
        }
        if actions.reset {
            self.play.reset(&self.room);
            self.status = format!("reset in {}", self.room.id);
        }
        if let Some((flag, value)) = actions.set_flag {
            self.play.game.state.set_flag(flag, value);
        }
        if let Some(flag) = actions.add_flag {
            self.play.game.state.set_flag(flag, true);
        }
        if let Some(item) = actions.give_item {
            self.play.game.state.give(item);
        }
        if let Some(item) = actions.take_item {
            self.play.game.state.take(&item);
        }
        if let Some(id) = actions.run_script
            && self.play.game.run_script(&id)
        {
            self.play.note(format!("run {id}"));
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
        if let Some(image) = actions.open_sheet {
            self.open_sheet(&image);
        }
        if actions.save_sheet {
            self.save_sheet();
        }
        if actions.close_sheet {
            self.sheet = None;
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
        if self.play.active {
            // A click through a line reads it away; only then does it
            // mean something in the room.
            if self.play.subtitle.is_some() {
                self.play.skip_subtitle();
                return;
            }
            let verb = self.play.verb;
            let click = self.play.game.click(&self.room, p, verb);
            self.status = click.label();
            self.play.hover = click;
            return;
        }
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
            Key::P => {
                if self.play.active {
                    self.play.end();
                } else {
                    self.start_play();
                }
            }
            Key::S => self.save(),
            Key::Escape if self.play.active => {
                self.play.end();
                self.status = "stopped".into();
            }
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
        let request = play::tick(
            &mut self.play,
            &self.room,
            &self.project.animations,
            &self.project.locales,
            &self.language,
            ctx.delta_time,
        );
        if !request.is_empty() {
            self.perform(request);
        }
        if self.play.active {
            let p = self.cursor_room();
            let verb = self.play.verb;
            self.play.hover = self.play.game.resolve(&self.room, p, verb);
        }

        // Advance the preview before anything is drawn, so the frame
        // shown is the one for this instant.
        let current = match self.sheet.as_ref() {
            // While a sheet is being cut, the cut is what plays: the
            // whole point is to see the frames land.
            Some(draft) => Some(draft.to_animation()),
            None => self
                .playback
                .selected
                .clone()
                .and_then(|id| self.room_animations.iter().find(|a| a.id == id).cloned()),
        };
        self.playback.tick(ctx.delta_time, current.as_ref());
        let preview = current.and_then(|animation| {
            let frame = self.playback.frame(&animation);
            let handle = self.animation_texture(&animation.image)?;
            Some((animation, handle, frame))
        });

        // Compute what depends on &self before taking the renderer out
        // mutably; the borrow checker cannot see that they are disjoint.
        let selected = self.selected_points();
        let actor = self.play.active.then(|| {
            let position = self.play.game.state.actor.position;
            (position, self.room.scale.scale_at(position.y))
        });
        let walk_line: Vec<Vec2> = if self.play.active {
            std::iter::once(self.play.game.state.actor.position)
                .chain(self.play.game.state.actor.path().iter().copied())
                .collect()
        } else {
            self.path.clone()
        };
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
                &walk_line,
            );
            if let Some((position, scale)) = actor {
                draw::draw_actor(renderer, &self.view, position, scale);
            }
        }

        // Build the UI, then apply what it asked for after the borrow of
        // the room ends.
        let mut actions = UiActions::default();
        let mut play_actions = ui_play::PlayActions::default();
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
                sheet: &mut self.sheet,
                content_min: egui::Vec2::ZERO,
                content_max: egui::Vec2::ZERO,
            };
            let play = &mut self.play;
            let play_draft = &mut self.play_draft;
            let room_id = state.room.id.clone();
            ui.run(window.raw(), |ctx| {
                play_actions = ui_play::panel(ctx, play, play_draft, &room_id);
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
        self.apply_play_actions(play_actions);
    }
}
