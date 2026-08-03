//! A6.6: the editor application - glue only.
//!
//! State lives here; behaviour lives in `edit`, `draw`, `view`, `ui` and
//! `project`. `on_update` is a sequence of calls into those, not a place
//! for logic.

mod input;
mod screens;

use std::collections::HashMap;
use std::path::PathBuf;

use glam::Vec2;
use infengine_adventure::loca::LanguageChoice;
use infengine_adventure::{Animation, Room};
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
use infengine_adventure::Project;
use crate::sheet::SheetDraft;
use crate::ui::{self, UiActions, UiState};
use crate::ui_game::{self, GameUi};
use crate::ui_graph::{self, GraphUi};
use crate::ui_play::{self, PlayDraft};
use crate::ui_screen::{self, ScreenUi};

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
    /// Which inspector job is open.
    task: crate::inspector::Task,
    /// The page that explains the window.
    help: crate::help::HelpUi,
    /// The script the timeline is laying out, if any.
    scene_script: Option<String>,
    /// How wide a second is drawn.
    timeline_zoom: f32,
    /// What dragged edges land on.
    timeline_snap: crate::timeline::Snap,
    /// A step to scroll the strip to, once - the one just added.
    timeline_reveal: Option<Vec<usize>>,
    /// The step the timeline has selected.
    timeline_selected: Option<Vec<usize>>,
    /// Scripts changed by dragging, which are saved with the room.
    scripts_dirty: bool,
    /// The map of the whole game, and its window.
    story_ui: crate::ui_story::StoryUi,
    library_ui: crate::library::LibraryUi,
    /// The play preview's still: which image, and its texture.
    play_still: Option<(String, infengine_render::Texture2dHandle)>,
    /// Scene-list thumbnails and summaries, filled a few per frame.
    cards: crate::cards::Cards,
    /// Built when the window opens; dropped when a save changes what it
    /// would show.
    story_cache: Option<infengine_adventure::story::Story>,
    /// Project-relative image paths under pictures/, for the timeline's
    /// image picker. Refreshed when a room opens, which is often enough
    /// for a folder that changes when an import runs.
    pictures: Vec<String>,
    /// How big the interface is drawn. Applied to egui each frame.
    ui_scale: f32,
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
    game_ui: GameUi,
    /// Saved games in the project, refreshed when one is written.
    slots: Vec<infengine_adventure::save::SlotInfo>,
    screen_ui: ScreenUi,
    graph_ui: GraphUi,
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
        let settings = crate::settings::Settings::load();
        let project = Project::open(&root);
        let play = PlayMode::with_scripts(project.scripts.clone());
        // A project with no manifest yet gets an empty one rather than
        // none: the panel is how you write the first.
        let game_ui = GameUi {
            manifest: infengine_adventure::pack::GameManifest::load(&project.root)
                .unwrap_or_default(),
            ..Default::default()
        };
        let (screens, screen_errors) =
            infengine_adventure::screen::load_dir(&project.root.join(
                infengine_adventure::screen::SCREEN_DIR,
            ));
        for err in &screen_errors {
            warn!(%err, "a screen did not load");
        }
        let screen_ui = ScreenUi {
            selected: (!screens.is_empty()).then_some(0),
            screens,
            ..Default::default()
        };
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
            task: crate::inspector::Task::default(),
            help: crate::help::HelpUi::default(),
            pictures: Vec::new(),
            story_ui: crate::ui_story::StoryUi::default(),
            library_ui: crate::library::LibraryUi::default(),
            play_still: None,
            cards: crate::cards::Cards::default(),
            story_cache: None,
            scene_script: None,
            timeline_zoom: settings.timeline_zoom,
            timeline_snap: crate::timeline::Snap {
                parts: settings.timeline_snap_parts,
                time: settings.timeline_snap_time,
            },
            timeline_reveal: None,
            timeline_selected: None,
            scripts_dirty: false,
            ui_scale: settings.ui_scale,
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
            game_ui,
            slots: Vec::new(),
            screen_ui,
            graph_ui: GraphUi::default(),
            status,
        }
    }

    fn open_room(&mut self, index: usize) {
        self.refresh_pictures();
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
        if let Some(image) = request.still {
            self.play_still = self
                .renderer
                .as_mut()
                .and_then(|renderer| assets::upload_one(renderer, &self.project, &image))
                .map(|handle| (image, handle));
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

    /// Put a script on the graph canvas.
    fn open_graph(&mut self, id: &str) {
        match self.project.scripts.get(id) {
            Some(script) => self.graph_ui.load(script),
            None => self.graph_ui.status = format!("no script called {id}"),
        }
    }

    /// Apply what the script graph asked for.
    fn apply_graph_actions(&mut self, actions: ui_graph::GraphActions) {
        if let Some(id) = actions.open_script {
            self.open_graph(&id);
        }
        if actions.save {
            self.save_graph();
        }
        if actions.run
            && let Some(script) = self.graph_ui.to_script()
        {
            // Straight from the canvas, unsaved: trying an edit should
            // not mean committing to it first.
            self.play.game.runner.start(&script);
            self.play.note(format!("run {} from the canvas", script.id));
            self.graph_ui.status = format!("running {}", script.id);
        }
    }

    /// Write the canvas back out as a script file.
    ///
    /// Scripts are grouped per room on disk, so saving one means
    /// rewriting the file it belongs to with every script in it.
    fn save_graph(&mut self) {
        use infengine_adventure::script_io;

        let Some(script) = self.graph_ui.to_script() else {
            return;
        };
        let id = script.id.clone();
        self.project.scripts.insert(id.clone(), script);

        let room = id.split_once('.').map(|(room, _)| room).unwrap_or(&id).to_string();
        let mut group: Vec<_> = self
            .project
            .scripts
            .values()
            .filter(|script| script.id.starts_with(&format!("{room}.")) || script.id == room)
            .cloned()
            .collect();
        group.sort_by(|a, b| a.id.cmp(&b.id));

        let dir = self.project.root.join(script_io::SCRIPT_DIR);
        self.graph_ui.status = match script_io::save_room(&room, &group, &dir) {
            Ok(()) => {
                self.graph_ui.dirty = false;
                // The game holds its own copy, so it has to be told.
                self.play.game = infengine_adventure::Game::new(self.project.scripts.clone());
                format!("saved {} scripts to {room}", group.len())
            }
            Err(err) => {
                warn!(%err, "could not save the script");
                err.to_string()
            }
        };
    }

    /// Where saved games go.
    ///
    /// The project directory, which is right for the editor: a test save
    /// belongs with the thing being tested. A shipped game would pass
    /// somewhere writable per user instead, which is why `save` takes a
    /// directory rather than working one out.
    fn save_dir(&self) -> std::path::PathBuf {
        self.project.root.join(infengine_adventure::save::SAVE_DIR)
    }

    fn refresh_slots(&mut self) {
        self.slots = infengine_adventure::save::list(&self.save_dir());
    }

    /// Write the play state to a slot.
    ///
    /// `u32::MAX` means "the next free one", so saving twice in a row
    /// does not quietly overwrite the first.
    fn save_slot(&mut self, slot: u32) {
        use infengine_adventure::save;

        let dir = self.save_dir();
        let slot = if slot == u32::MAX {
            save::next_free(&dir)
        } else {
            slot
        };
        let name = if self.play_draft.save_name.trim().is_empty() {
            self.play.game.state.room.clone()
        } else {
            self.play_draft.save_name.trim().to_string()
        };
        let entry = save::Save::new(
            self.game_ui.manifest.title.clone(),
            name,
            self.play.game.state.clone(),
        );
        match save::write(&dir, slot, &entry) {
            Ok(_) => {
                self.play_draft.save_name.clear();
                self.status = format!("saved to slot {slot}");
                self.play.note(format!("saved to slot {slot}"));
            }
            Err(err) => {
                warn!(%err, "could not save");
                self.status = err.to_string();
            }
        }
        self.refresh_slots();
    }

    /// Read a slot back and put the game where it was.
    fn load_slot(&mut self, slot: u32) {
        use infengine_adventure::save;

        // An empty expected title accepts whatever is there: the editor
        // is often testing a project whose manifest has no title yet.
        let title = self.game_ui.manifest.title.clone();
        let entry = match save::read(&self.save_dir(), slot, &title) {
            Ok(entry) => entry,
            Err(err) => {
                warn!(%err, "could not load");
                self.status = err.to_string();
                return;
            }
        };
        let room = entry.state.room.clone();
        self.play.game.state = entry.state;
        self.play.subtitle = None;
        self.play.game.runner.stop();
        // The room the save was in has to be the one on screen, or the
        // actor stands in a room it was never in.
        if !room.is_empty() && room != self.room.id {
            self.enter_room_by_id(&room, None);
        }
        self.play.active = true;
        self.play.note(format!("loaded slot {slot} in {room}"));
        self.status = format!("loaded slot {slot}");
    }

    /// Apply what the game window asked for.
    fn apply_game_actions(&mut self, actions: ui_game::GameActions) {
        use infengine_adventure::pack;

        if actions.save_manifest {
            self.game_ui.status = match self.game_ui.manifest.save(&self.project.root) {
                Ok(()) => format!("wrote {}", pack::MANIFEST_FILE),
                Err(err) => err.to_string(),
            };
        }
        if actions.check {
            let plan = pack::plan(&self.project.root);
            self.game_ui.status = format!("checked {} files", plan.files.len());
            self.game_ui.plan = Some(plan);
        }
        if actions.export
            && let Some(plan) = self.game_ui.plan.as_ref()
        {
            let dest = std::path::Path::new(self.game_ui.export_to.trim());
            self.game_ui.status = match pack::export(&self.project.root, dest, plan) {
                Ok(copied) => format!("copied {copied} files to {}", dest.display()),
                Err(err) => {
                    warn!(%err, "export failed");
                    err.to_string()
                }
            };
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
        if actions.open_game {
            self.game_ui.open = true;
        }
        if actions.open_screens {
            self.screen_ui.open = true;
        }
        if let Some(slot) = actions.save_slot {
            self.save_slot(slot);
        }
        if let Some(slot) = actions.load_slot {
            self.load_slot(slot);
        }
        if let Some(slot) = actions.delete_slot {
            if let Err(err) = infengine_adventure::save::remove(&self.save_dir(), slot) {
                warn!(%err, "could not remove the save");
            }
            self.refresh_slots();
        }
        if let Some(id) = actions.open_graph {
            self.graph_ui.open = true;
            if !id.is_empty() {
                self.open_graph(&id);
            }
        }
    }

    /// Write back the script file the timeline changed.
    ///
    /// Whole file rather than one script: they are stored per room and
    /// rewriting one of them would drop the rest.
    fn save_scripts(&mut self) {
        use infengine_adventure::script_io;
        let prefix = self.room.script_prefix().to_string();
        let mut scripts: Vec<_> = self
            .project
            .scripts
            .iter()
            .filter(|(id, _)| id.starts_with(&format!("{prefix}.")))
            .map(|(_, script)| script.clone())
            .collect();
        scripts.sort_by(|a, b| a.id.cmp(&b.id));
        if scripts.is_empty() {
            return;
        }
        let dir = self.project.root.join(script_io::SCRIPT_DIR);
        match script_io::save_room(&prefix, &scripts, &dir) {
            Ok(()) => self.scripts_dirty = false,
            Err(err) => {
                warn!(%err, "scripts not saved");
                self.status = err.to_string();
            }
        }
    }

    fn save(&mut self) {
        let Some(path) = self.room_path.clone() else {
            return;
        };
        // A script dragged on the timeline goes with the room, because
        // the two are one edit: a scene is a room and the script that
        // plays in it, and saving half of that is a scene that has
        // changed its mind.
        if self.scripts_dirty {
            self.save_scripts();
            self.cards.forget(&self.room.id.clone());
        }
        self.story_cache = None;
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

    /// Apply what the menu bar asked for.
    ///
    /// Menu items route to the same methods as the buttons that do the
    /// same thing, so there is one implementation of each action and
    /// two ways to reach it.
    fn apply_menu(&mut self, actions: crate::menu::MenuActions) {
        if actions.save {
            self.save();
        }
        if actions.reload && let Some(index) = self.current_room {
            self.open_room(index);
        }
        if actions.fit_view {
            self.view.fit();
        }
        if let Some(what) = actions.add {
            self.add_to_room(what);
        }
        if let Some(task) = actions.task {
            self.task = task;
            task.reveal(&mut self.overlays);
            if let Some(first) = task.tools().first() {
                self.tool = *first;
            }
        }
        if actions.open_game {
            self.game_ui.open = true;
        }
        if actions.open_screens {
            self.screen_ui.open = true;
        }
        if actions.open_graph {
            self.graph_ui.open = true;
        }
        if actions.open_library {
            self.refresh_pictures();
            self.library_ui.open = true;
        }
        if actions.open_story {
            self.story_ui.open = true;
            // Fresh on every open: the map is cheap next to being wrong.
            self.story_cache = None;
        }
        if actions.open_help {
            self.help.open = true;
        }
        if actions.start_play {
            self.start_play();
        }
        if actions.stop_play {
            self.play.end();
            self.status = "stopped".into();
        }
    }

    /// Put something new in the room, centred on the view, and switch to
    /// the tab that edits it. Landing on a tab that does not show what
    /// you just added is the worst kind of nothing-happened.
    fn add_to_room(&mut self, what: crate::menu::Addable) {
        use crate::menu::Addable;
        let at = self.view.center;
        let name = match what {
            Addable::Hotspot => {
                let index = crate::add::hotspot(&mut self.room, at);
                self.selection = Selection::Hotspot(index);
                self.room.hotspots[index].id.clone()
            }
            Addable::Exit => {
                let index = crate::add::exit(&mut self.room, at);
                self.room.exits[index].id.clone()
            }
            Addable::Entry => {
                let index = crate::add::entry(&mut self.room, at);
                self.room.entries[index].id.clone()
            }
        };
        self.task = what.task();
        self.task.reveal(&mut self.overlays);
        self.dirty = true;
        self.status = format!("added {name} - drag it into place");
    }

    /// The script the Scene tab is showing, laid out on a clock.
    fn scene_timeline(&self) -> Option<infengine_adventure::schedule::Timeline> {
        use infengine_adventure::schedule::{self, Timings};
        // Whatever was last run, or the room's own if nothing has been.
        let id = self
            .scene_script
            .clone()
            .unwrap_or_else(|| format!("{}.entry", self.room.script_prefix()));
        let script = self.project.scripts.get(&id)?;
        let animations = &self.project.animations;
        let locales = &self.project.locales;
        let language = self.language.text.clone();
        Some(schedule::timeline(
            script,
            &Timings {
                animation: &|id| animations.get(id).map(|a| a.duration()),
                line: &|id| Some(locales.text(id, &language).to_string()),
            },
        ))
    }

    /// List the images a Show Image step can pick from.
    fn refresh_pictures(&mut self) {
        let mut out = Vec::new();
        for dir in ["pictures", "sprites"] {
            let Ok(entries) = std::fs::read_dir(self.project.root.join(dir)) else {
                continue;
            };
            for path in entries.flatten().map(|e| e.path()) {
                if path.extension().is_some_and(|e| e == "png")
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                {
                    out.push(format!("{dir}/{name}"));
                }
            }
        }
        out.sort();
        self.pictures = out;
    }

    /// The script the timeline is showing.
    fn scene_script_id(&self) -> String {
        self.scene_script
            .clone()
            .unwrap_or_else(|| format!("{}.entry", self.room.script_prefix()))
    }

    /// Apply what the timeline asked for.
    fn apply_timeline(&mut self, actions: crate::timeline::TimelineActions) {
        if let Some(path) = actions.selected {
            self.timeline_selected = Some(path);
        }
        let id = self.scene_script_id();

        // Adding a step to a script that does not exist yet creates it:
        // the point of the Add menu is starting a scene from nothing,
        // and refusing until a file exists would make the first step
        // impossible to take in the same place as the rest.
        if let Some((after, action)) = actions.insert {
            let script = self
                .project
                .scripts
                .entry(id.clone())
                .or_insert_with(|| infengine_adventure::Script::new(id.clone()));
            let landed =
                crate::timeline::insert_after(&mut script.actions, after.as_deref(), action);
            self.status = format!(
                "added {} - it is selected on the timeline",
                crate::timeline::action_at(&script.actions, &landed)
                    .map(|a| a.label())
                    .unwrap_or_default()
            );
            self.timeline_reveal = Some(landed.clone());
            self.timeline_selected = Some(landed);
            self.scripts_dirty = true;
            self.dirty = true;
        }
        if let Some((from, to)) = actions.reorder
            && let Some(script) = self.project.scripts.get_mut(&id)
            && let Some(landed) = crate::timeline::reorder(&mut script.actions, &from, to)
        {
            self.timeline_selected = Some(landed);
            self.scripts_dirty = true;
            self.dirty = true;
            self.status = "step moved".into();
        }
        if actions.play {
            let script = self.scene_script_id();
            self.play.begin(&self.room);
            if !self.play.game.run_script(&script) {
                self.status = format!("{script} does not exist yet - add a step first");
                self.play.end();
            }
        }
        if actions.stop {
            self.play.end();
            self.status = "stopped".into();
        }

        let mut touched = false;
        if let Some(script) = self.project.scripts.get_mut(&id) {
            if let Some((path, seconds)) = actions.set_seconds {
                touched |= crate::timeline::set_seconds(&mut script.actions, &path, seconds);
            }
            if let Some((path, action)) = actions.edit {
                touched |= crate::timeline::replace(&mut script.actions, &path, action);
            }
            if let Some(path) = actions.remove
                && crate::timeline::remove(&mut script.actions, &path)
            {
                self.timeline_selected = None;
                touched = true;
            }
        }
        if touched {
            self.scripts_dirty = true;
            self.dirty = true;
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
        if let Some(id) = actions.run_script {
            self.scene_script = Some(if id.is_empty() {
                format!("{}.entry", self.room.script_prefix())
            } else {
                id.clone()
            });
            // An empty id means the room's own script, which is what
            // "play this scene" means when nobody picked one.
            if id.is_empty() {
                self.start_play();
            } else {
                if !self.play.active {
                    self.start_play();
                }
                if !self.play.game.run_script(&id) {
                    self.status = format!("no script {id}");
                }
            }
        }
        if actions.stop_play {
            self.play.end();
            self.status = "stopped".into();
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
        self.refresh_slots();
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

    fn on_exit(&mut self) {
        // The scale and the zoom are set once for a monitor and a pair
        // of eyes; losing them every launch turns a preference into a
        // chore.
        crate::settings::Settings {
            ui_scale: self.ui_scale,
            timeline_zoom: self.timeline_zoom,
            timeline_snap_parts: self.timeline_snap.parts,
            timeline_snap_time: self.timeline_snap.time,
        }
        .save();
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
                self.play.game.state.camera,
            );
            if let Some((position, scale)) = actor {
                draw::draw_actor(renderer, &self.view, position, scale);
            }
            // The scene's still goes over everything the room drew,
            // which is where a script put it.
            if self.play.active
                && self.play.still.is_some()
                && let Some((_, handle)) = self.play_still
            {
                draw::draw_still(renderer, &self.room, &self.view, handle);
            }
        }
        if self.play.still.is_none() {
            self.play_still = None;
        }

        // Build the UI, then apply what it asked for after the borrow of
        // the room ends.
        let mut actions = UiActions::default();
        // Snapshotted before the UI borrows the language pickers: the
        // screen preview only reads it, but the borrow checker cannot
        // see that through the closure.
        let language = self.language.clone();
        let locales = self.project.locales.clone();
        // For the screen preview: which flags the test run has set, so
        // conditional widgets show as the game would show them.
        let play_flags = self.play.game.state.flags.clone();
        let mut play_actions = ui_play::PlayActions::default();
        let mut game_actions = ui_game::GameActions::default();
        let mut screen_actions = ui_screen::ScreenActions::default();
        let mut graph_actions = ui_graph::GraphActions::default();
        let mut menu_actions = crate::menu::MenuActions::default();
        let mut timeline_actions = crate::timeline::TimelineActions::default();
        let mut story_actions = crate::ui_story::StoryActions::default();
        let mut library_actions = crate::library::LibraryActions::default();
        if self.story_ui.open && self.story_cache.is_none() {
            self.story_cache =
                Some(infengine_adventure::story::Story::of(&self.project));
        }
        let story_start = self
            .project
            .manifest
            .as_ref()
            .map(|manifest| manifest.start_room.clone())
            .unwrap_or_default();
        // Only laid out when the Scene tab is open: a timeline is a tab's
        // worth of screen, and a room being traced does not want one.
        let scene_line = (self.task == crate::inspector::Task::Scene)
            .then(|| self.scene_timeline())
            .flatten();
        // The selected step's current values, for the details column.
        let scene_detail: Option<infengine_adventure::Action> = self
            .timeline_selected
            .as_ref()
            .zip(self.project.scripts.get(&self.scene_script_id()))
            .and_then(|(path, script)| {
                crate::timeline::action_at(&script.actions, path).cloned()
            });
        let picker_animations: Vec<String> =
            self.project.animations.iter().map(|a| a.id.clone()).collect();
        let picker_sounds: Vec<String> = self.project.sounds.ids().map(str::to_string).collect();
        let picker_rooms: Vec<String> = self.project.room_ids();
        let picker_images = self.pictures.clone();
        let mut content = None;
        if let Some(ui) = self.ui.as_mut() {
            let mut state = UiState {
                project: &self.project,
                current_room: self.current_room,
                room: &mut self.room,
                task: &mut self.task,
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
                playing: self.play.active,
                elapsed: self.play.elapsed,
                now: if self.play.active {
                    self.play.doing()
                } else {
                    String::new()
                },
                camera: self.play.game.state.camera,
                content_min: egui::Vec2::ZERO,
                content_max: egui::Vec2::ZERO,
                cards: &mut self.cards,
            };
            let play = &mut self.play;
            let play_draft = &mut self.play_draft;
            let room_id = state.room.id.clone();
            let slots = &self.slots;
            let game_ui = &mut self.game_ui;
            let room_names: Vec<String> = state
                .project
                .rooms
                .iter()
                .map(|path| Project::room_label(path))
                .collect();
            let languages = self.languages.clone();
            let screen_ui = &mut self.screen_ui;
            let graph_ui = &mut self.graph_ui;
            let script_ids: Vec<String> = {
                let mut ids: Vec<String> = state.project.scripts.keys().cloned().collect();
                ids.sort();
                ids
            };
            let menu_ctx = crate::menu::MenuContext {
                has_room: state.current_room.is_some(),
                dirty: state.dirty,
                playing: play.active,
            };
            self.help.offer_once();
            let help_ui = &mut self.help;
            let ui_scale = &mut self.ui_scale;
            let timeline_zoom = &mut self.timeline_zoom;
            let timeline_snap = &mut self.timeline_snap;
            let timeline_reveal = self.timeline_reveal.take();
            let story_ui = &mut self.story_ui;
            let library_ui = &mut self.library_ui;
            let story_cache = &self.story_cache;
            let timeline_selected = &self.timeline_selected;
            ui.run(window.raw(), |ctx| {
                // Applied before anything is laid out, so the whole
                // frame is measured at the size it will be drawn at.
                ctx.set_zoom_factor(*ui_scale);
                menu_actions =
                    crate::menu::bar(ctx, menu_ctx, state.overlays, *state.task, ui_scale);
                crate::help::window(ctx, help_ui);
                play_actions = ui_play::panel(ctx, play, play_draft, &room_id, slots);
                game_actions = ui_game::window(ctx, game_ui, &room_names, &languages);
                screen_actions = ui_screen::window(ctx, screen_ui, &languages);
                graph_actions = ui_graph::window(ctx, graph_ui, &script_ids);
                if story_ui.open
                    && let Some(story) = story_cache.as_ref()
                {
                    story_actions =
                        crate::ui_story::window(ctx, story_ui, story, &story_start);
                }
                if library_ui.open {
                    library_actions =
                        crate::library::window(ctx, library_ui, state.project, &picker_images);
                }
                if let Some(line) = scene_line.as_ref() {
                    timeline_actions = crate::timeline::panel(
                        ctx,
                        line,
                        play.active,
                        play.elapsed,
                        timeline_zoom,
                        timeline_snap,
                        timeline_selected,
                        timeline_reveal.as_deref(),
                        scene_detail.as_ref(),
                        &crate::timeline::Pickers {
                            animations: &picker_animations,
                            sounds: &picker_sounds,
                            rooms: &picker_rooms,
                            images: &picker_images,
                        },
                    );
                }
                if let Some(screen) = screen_ui.preview_screen() {
                    screen_actions.fired =
                        ui_screen::preview(ctx, screen, &locales, &language, &play_flags);
                }
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
        if let Some(id) = story_actions.open_room
            && let Some(index) = self
                .project
                .rooms
                .iter()
                .position(|path| infengine_adventure::Project::room_label(path) == id)
        {
            self.open_room(index);
        }
        if let Some(action) = library_actions.add {
            // Land it in the scene's timeline and turn the inspector
            // there, so the step arrives somewhere visible.
            self.task = crate::inspector::Task::Scene;
            self.apply_timeline(crate::timeline::TimelineActions {
                insert: Some((self.timeline_selected.clone(), action)),
                ..Default::default()
            });
            self.status = "step added - it is on the scene's timeline".into();
        }
        if let Some(id) = library_actions.preview {
            self.playback.select(id);
        }
        self.apply_timeline(timeline_actions);
        self.apply_menu(menu_actions);
        self.apply_play_actions(play_actions);
        self.apply_game_actions(game_actions);
        self.apply_screen_actions(screen_actions);
        self.apply_graph_actions(graph_actions);
    }
}
