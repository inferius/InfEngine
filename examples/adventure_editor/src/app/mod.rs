//! A6.6: the editor application - glue only.
//!
//! State lives here; behaviour lives in `edit`, `draw`, `view`, `ui` and
//! `project`. `on_update` is a sequence of calls into those, not a place
//! for logic.

mod input;
mod screens;
mod update;

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
use crate::draw::Overlays;
use crate::edit::{self, Selection, Tool};
use crate::mode::Mode;
use crate::play::{self, PlayMode};
use crate::playback::Playback;
use infengine_adventure::Project;
use crate::sheet::SheetDraft;
use crate::ui::UiActions;
use crate::ui_game::{self, GameUi};
use crate::ui_graph::{self, GraphUi};
use crate::ui_play::{self, PlayDraft};
use crate::ui_screen::ScreenUi;

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
    /// What the editor is being used for: the pills at the top set it.
    mode: Mode,
    /// The page that explains the window.
    help: crate::help::HelpUi,
    /// The script the timeline is laying out, if any.
    scene_script: Option<String>,
    /// How wide a second is drawn.
    timeline_zoom: f32,
    /// What dragged edges land on.
    timeline_snap: crate::timeline::Snap,
    /// Whether Animate mode shows the timeline at all.
    timeline_open: bool,
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
    /// Rooms as they were before each canvas edit, newest last.
    undo: Vec<Room>,

    cursor: Vec2,
    last_cursor: Vec2,
    drag: Drag,
    floor_draft: Vec<Vec2>,
    walk_from: Option<Vec2>,
    path: Vec<Vec2>,
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
        crate::lang::set_czech(settings.language == "cs");
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
            mode: Mode::default(),
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
            timeline_open: settings.timeline_open,
            timeline_reveal: None,
            timeline_selected: None,
            scripts_dirty: false,
            ui_scale: settings.ui_scale,
            tool: Tool::default(),
            selection: Selection::None,
            undo: Vec::new(),
            cursor: Vec2::ZERO,
            last_cursor: Vec2::ZERO,
            drag: Drag::None,
            floor_draft: Vec::new(),
            walk_from: None,
            path: Vec::new(),
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

    /// Switch what the editor is being used for.
    ///
    /// The mode owns its side effects: entering Test starts the game,
    /// leaving it stops, entering Script puts the room's own script on
    /// an empty canvas. Modes that need a tool pick their first, so the
    /// mouse always does something sensible.
    fn set_mode(&mut self, mode: Mode) {
        if mode == self.mode {
            return;
        }
        if self.mode == Mode::Test && self.play.active {
            self.play.end();
        }
        self.mode = mode;
        mode.reveal(&mut self.overlays);
        if let Some(first) = mode.tools().first() {
            self.tool = *first;
        }
        match mode {
            Mode::Test => {
                if self.current_room.is_some() && !self.play.active {
                    self.start_play();
                }
            }
            Mode::Script => {
                if self.graph_ui.script_id.is_none() {
                    let id = format!("{}.entry", self.room.script_prefix());
                    if self.project.scripts.contains_key(&id) {
                        self.open_graph(&id);
                    }
                }
            }
            Mode::Room | Mode::Animate => {}
        }
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
            // Placed exactly as the game would place it: where the
            // script said, or where the animation says about itself -
            // so the preview is the scene, not an approximation of it.
            let fallback = self
                .project
                .animations
                .get(&animation)
                .map(|a| a.position_in(self.room.size));
            if let Some(at) = request.animation_at.or(fallback) {
                self.playback.position = at;
            }
            self.playback.select(animation);
        }
        if let Some(sound) = request.sound {
            self.play_sound_id(&sound);
        }
        if let Some(music) = request.music {
            match music {
                Some(track) => self.play_sound_id(&track),
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
    /// Keep the room as it is now, so the next edit can be taken back.
    pub(super) fn remember(&mut self) {
        if self.undo.last() == Some(&self.room) {
            return;
        }
        self.undo.push(self.room.clone());
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
    }

    /// Take the last canvas edit back.
    fn undo(&mut self) {
        let Some(room) = self.undo.pop() else {
            self.status = "nothing to undo".into();
            return;
        };
        self.room = room;
        self.selection = Selection::None;
        self.floor_draft.clear();
        self.dirty = true;
        self.status = "undone".into();
    }

    fn play_file(&mut self, relative: &str) {
        if let Err(message) = assets::play_file(self.audio.as_mut(), &self.project, relative) {
            // Where the author is looking, not just in the log: a
            // silent play with the reason buried is a bug report
            // waiting to be written.
            self.status = format!("{relative}: {message}");
            self.play.note(message);
        }
    }

    /// Play something a script named: a sound id first, a project
    /// path as the fallback.
    ///
    /// Scripts speak in ids - music.intro_0, kyra.sfx7 - and handing
    /// an id to the file player made the editor silent with a
    /// file-not-found buried in the log, which is exactly how the
    /// missing-music report happened.
    fn play_sound_id(&mut self, id: &str) {
        match self.project.sounds.get(id).map(str::to_string) {
            Some(file) => self.play_file(&file),
            None => self.play_file(id),
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
            // Asked for from the Test column: the canvas lives in
            // Script mode, so go where it is.
            self.set_mode(Mode::Script);
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
        if let Some(mode) = actions.mode {
            self.set_mode(mode);
        }
        if actions.open_game {
            self.game_ui.open = true;
        }
        if actions.open_screens {
            self.screen_ui.open = true;
        }
        if actions.undo {
            self.undo();
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
            // The Play button is "play the game": Test mode, running.
            self.set_mode(Mode::Test);
            if !self.play.active {
                self.start_play();
            }
        }
        if actions.stop_play {
            self.play.end();
            self.status = "stopped".into();
        }
    }

    /// Put something new in the room, centred on the view, and switch to
    /// the mode that edits it. Landing somewhere that does not show what
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
        self.set_mode(Mode::Room);
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
        if let Some(action) = actions.insert_first {
            let script = self
                .project
                .scripts
                .entry(id.clone())
                .or_insert_with(|| infengine_adventure::Script::new(id.clone()));
            let landed = crate::timeline::insert_front(&mut script.actions, action);
            self.status = "added ahead of the scene - it is selected on the timeline".into();
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
        if let Some(id) = actions.hear {
            self.play_sound_id(&id);
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
            self.remember();
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
        if let Some(id) = actions.open_graph_script {
            self.open_graph(&id);
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
            timeline_open: self.timeline_open,
            language: if crate::lang::is_czech() { "cs" } else { "en" }.into(),
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
                    self.set_mode(Mode::Test);
                    if !self.play.active {
                        self.start_play();
                    }
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
        self.frame(window, ctx);
    }
}
