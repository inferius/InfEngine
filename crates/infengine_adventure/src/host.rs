//! A36.2: playing a whole game, not one room.
//!
//! [`crate::game`] runs a script against a room. This runs a *game*: it
//! owns the project, keeps the current room loaded, shows menus, turns
//! the runner's events into requests for audio, and remembers where the
//! player is when a save is written.
//!
//! It performs no output either. Everything that needs a file or a
//! device comes back as a [`Request`], so the same host drives the
//! editor's test harness and a shipped game, and the whole of it can be
//! played in a test with no window.
//!
//! What separates this from the editor's old play mode is that it opens
//! rooms itself. A test harness can afford to say "the room the editor
//! has open"; a game cannot.

use crate::game::{Click, Event, Game, GameState};
use crate::loca::LanguageChoice;
use crate::pack::GameManifest;
use crate::project::Project;
use crate::room::Room;
use crate::screen::{Screen, ScreenAction};
use crate::{Verb, save};

use glam::Vec2;

/// How long a subtitle stays up, at minimum.
const SUBTITLE_MIN: f32 = 1.2;
/// Characters a reader gets through in a second.
const CHARS_PER_SECOND: f32 = 14.0;
/// Fallback length for an animation the index does not describe.
const ANIM_FALLBACK: f32 = 1.0;

/// A line on screen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Subtitle {
    pub line: String,
    pub text: String,
    pub speaker: Option<String>,
    pub remaining: f32,
}

impl Subtitle {
    /// Is this the line's id standing in for text it does not have?
    ///
    /// An id in place of a line is the right thing to show in an editor,
    /// where it is the gap you are looking for, and the wrong thing to
    /// show a player, who reads `alchemy.00` as the game leaking a file
    /// name at them. Kyrandia's intro has 64 such lines, all empty, so
    /// this is not a rare case to leave to taste.
    pub fn is_placeholder(&self) -> bool {
        self.text == self.line
    }
}

/// Files the host should start playing, and rooms it should show.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    /// Project-relative path of a recording to play.
    pub voice: Option<String>,
    /// Project-relative path of a sound effect.
    pub sound: Option<String>,
    /// `Some(None)` means stop the music.
    pub music: Option<Option<String>>,
    /// Animation to show in the room.
    pub animation: Option<String>,
    /// Where the script put it, when it said.
    pub animation_at: Option<Vec2>,
    /// The view moved this step.
    pub camera_moved: bool,
    /// A picture to show over the room, and for how long. Zero means
    /// until the room changes.
    pub still: Option<(String, f32)>,
    /// The room changed; anything cached about the old one is stale.
    pub room_changed: bool,
    /// The game asked to close.
    pub quit: bool,
}

impl Request {
    pub fn is_empty(&self) -> bool {
        *self == Request::default()
    }
}

/// Why something did not happen, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    Said(String),
    Sound(String),
    Music(Option<String>),
    Entered(String),
    /// A picture put on screen.
    Image(String),
    /// A step the import could not translate.
    Skipped(String),
    /// Something the data names but the project does not have.
    Missing(String),
}

impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Note::Said(text) => write!(f, "say: {text}"),
            Note::Sound(id) => write!(f, "sound: {id}"),
            Note::Music(Some(id)) => write!(f, "music: {id}"),
            Note::Music(None) => write!(f, "music off"),
            Note::Entered(room) => write!(f, "entered {room}"),
            Note::Image(image) => write!(f, "image: {image}"),
            Note::Skipped(call) => write!(f, "SKIPPED {call}"),
            Note::Missing(what) => write!(f, "MISSING {what}"),
        }
    }
}

/// A game being played.
pub struct Host {
    pub project: Project,
    pub game: Game,
    /// The room on screen. Empty until [`Host::start`].
    pub room: Room,
    pub subtitle: Option<Subtitle>,
    pub language: LanguageChoice,
    /// Verb the next click carries.
    pub verb: Verb,
    /// Screen stacked over the room, if any.
    pub screen: Option<String>,
    /// Screens the open one was opened from, oldest first.
    ///
    /// A pause menu opens a save menu, and Back from the save menu means
    /// the pause menu rather than the game. Without somewhere to
    /// remember that, Close can only ever mean "put the player back in
    /// the room", and the way out of a submenu drops them two levels.
    beneath: Vec<String>,
    /// Whether the launch intro is still running - the stretch the
    /// player is allowed to skip with a click or Escape.
    pub intro: bool,
    /// A100: whether the player has actually entered the game.
    ///
    /// A title screen is the front door, not a pause: Escape there used
    /// to dismiss it into a room nobody had started, where the next
    /// Escape offered Resume and Save for a game that had never begun.
    /// One flag says which side of the door we are on.
    pub started: bool,
    /// What happened, newest last.
    pub log: Vec<Note>,
    /// How much of the log to keep.
    pub log_limit: usize,
    anim_wait: Option<(String, f32)>,
    /// Effect bank the last music change loaded, the way the original
    /// keeps one sound file for music and effects together.
    sfx_bank: Option<String>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("room", &self.room.id)
            .field("screen", &self.screen)
            .finish()
    }
}

impl Host {
    /// Open a project and get ready to play it.
    pub fn new(project: Project) -> Self {
        let language = project
            .manifest
            .as_ref()
            .map(|manifest| manifest.default_language.clone())
            .filter(|choice| !choice.text.is_empty())
            .or_else(|| {
                project
                    .locales
                    .languages()
                    .first()
                    .map(|first| LanguageChoice::same(*first))
            })
            .unwrap_or_default();
        let game = Game::new(project.scripts.clone());
        Self {
            project,
            game,
            room: Room::default(),
            subtitle: None,
            language,
            verb: Verb::Walk,
            screen: None,
            beneath: Vec::new(),
            started: false,
            intro: false,
            log: Vec::new(),
            log_limit: 200,
            anim_wait: None,
            sfx_bank: None,
        }
    }

    pub fn manifest(&self) -> GameManifest {
        self.project.manifest.clone().unwrap_or_default()
    }

    /// Start where the manifest says, or in the first room there is.
    ///
    /// A game with a title screen shows it instead; the screen's own
    /// New Game button is what enters the world.
    pub fn start(&mut self) -> Request {
        let manifest = self.manifest();
        if !manifest.intro_room.is_empty() {
            let intro = manifest.intro_room.clone();
            if self.project.load_room(&intro).is_some() {
                self.intro = true;
                return self.enter(&intro, None);
            }
        }
        if !manifest.start_screen.is_empty() && self.project.screen(&manifest.start_screen).is_some()
        {
            self.screen = Some(manifest.start_screen);
            return Request::default();
        }
        self.new_game()
    }

    /// Begin the story: the starting room, with a fresh state.
    pub fn new_game(&mut self) -> Request {
        self.intro = false;
        self.started = true;
        let manifest = self.manifest();
        self.game.state = GameState::default();
        self.screen = None;
        let start = if manifest.start_room.is_empty() {
            self.project.room_ids().first().cloned().unwrap_or_default()
        } else {
            manifest.start_room
        };
        let entry = (!manifest.start_entry.is_empty()).then_some(manifest.start_entry);
        self.enter(&start, entry.as_deref())
    }

    /// Load a room and put the actor in it.
    pub fn enter(&mut self, id: &str, entry: Option<&str>) -> Request {
        let Some(room) = self.project.load_room(id) else {
            // Deliberately left where it was: a missing room is a hole
            // in the game, and teleporting somewhere else would hide it.
            self.note(Note::Missing(format!("room {id}")));
            return Request::default();
        };
        self.room = room;
        self.game.enter(&self.room, entry);
        self.subtitle = None;
        self.anim_wait = None;
        self.note(Note::Entered(id.to_string()));
        Request {
            room_changed: true,
            ..Default::default()
        }
    }

    /// The screen currently stacked over the room.
    pub fn active_screen(&self) -> Option<&Screen> {
        self.screen
            .as_deref()
            .and_then(|id| self.project.screen(id))
    }

    /// Cut the launch intro short: stop everything it is doing and go
    /// where it was heading - the title screen when there is one, the
    /// first room otherwise. A no-op outside the intro, so the host
    /// can be asked without checking first.
    pub fn skip_intro(&mut self) -> Request {
        if !self.intro {
            return Request::default();
        }
        self.intro = false;
        self.game.runner.stop();
        self.subtitle = None;
        let manifest = self.manifest();
        if !manifest.start_screen.is_empty()
            && self.project.screen(&manifest.start_screen).is_some()
        {
            self.screen = Some(manifest.start_screen);
            self.beneath.clear();
            return Request::default();
        }
        self.new_game()
    }

    /// What a click at `point` in the room would do.
    pub fn resolve(&self, point: Vec2) -> Click {
        self.game.resolve(&self.room, point, self.verb)
    }

    /// Click in the room. Ignored while a screen is up.
    pub fn click(&mut self, point: Vec2) -> Click {
        if self.screen.is_some() {
            return Click::Nothing;
        }
        if self.subtitle.take().is_some() {
            // Clicking through a line reads it away rather than doing
            // two things at once.
            self.game.runner.finish_speech();
            return Click::Nothing;
        }
        let verb = self.verb;
        self.game.click(&self.room, point, verb)
    }

    /// Carry out what a screen's button says.
    pub fn fire(&mut self, action: &ScreenAction) -> Request {
        match action {
            ScreenAction::NewGame => self.new_game(),
            // Back, which is one level; and Resume, which is all of
            // them. Two words for two different things.
            ScreenAction::Close => {
                self.screen = self.beneath.pop();
                Request::default()
            }
            ScreenAction::Resume => {
                self.beneath.clear();
                self.screen = None;
                Request::default()
            }
            // Loading is entering the game as much as New Game is.
            ScreenAction::Open { screen } => {
                if self.project.screen(screen).is_some() {
                    if let Some(open) = self.screen.replace(screen.clone()) {
                        self.beneath.push(open);
                    }
                } else {
                    self.note(Note::Missing(format!("screen {screen}")));
                }
                Request::default()
            }
            ScreenAction::Quit => Request {
                quit: true,
                ..Default::default()
            },
            ScreenAction::SetTextLanguage { language } => {
                self.language.text = language.clone();
                Request::default()
            }
            ScreenAction::SetVoiceLanguage { language } => {
                self.language.voice = language.clone();
                Request::default()
            }
            ScreenAction::SaveGame { slot } => {
                self.save(*slot);
                Request::default()
            }
            ScreenAction::LoadGame { slot } => {
                let request = self.load(*slot);
                self.started = true;
                request
            }
            ScreenAction::RunScript { script } => {
                if !self.game.run_script(script) {
                    self.note(Note::Missing(format!("script {script}")));
                }
                self.screen = None;
                Request::default()
            }
            ScreenAction::None => Request::default(),
        }
    }

    /// Where saves go. Under the project while testing; a shipped game
    /// passes somewhere writable instead.
    pub fn save_dir(&self) -> std::path::PathBuf {
        self.project.root.join(save::SAVE_DIR)
    }

    pub fn save(&mut self, slot: u32) {
        let title = self.manifest().title;
        let name = self.game.state.room.clone();
        let entry = save::Save::new(title, name, self.game.state.clone());
        if let Err(err) = save::write(&self.save_dir(), slot, &entry) {
            self.note(Note::Missing(err.to_string()));
        }
    }

    pub fn load(&mut self, slot: u32) -> Request {
        let title = self.manifest().title;
        let entry = match save::read(&self.save_dir(), slot, &title) {
            Ok(entry) => entry,
            Err(err) => {
                self.note(Note::Missing(err.to_string()));
                return Request::default();
            }
        };
        let room = entry.state.room.clone();
        self.game.state = entry.state;
        self.game.runner.stop();
        self.screen = None;
        self.subtitle = None;
        // The actor's position came out of the save, so the room is
        // loaded without being re-entered - entering would move it to
        // a doorway.
        let Some(loaded) = self.project.load_room(&room) else {
            self.note(Note::Missing(format!("room {room}")));
            return Request::default();
        };
        self.room = loaded;
        self.note(Note::Entered(room));
        Request {
            room_changed: true,
            ..Default::default()
        }
    }

    /// Cut the line being spoken short.
    pub fn skip_subtitle(&mut self) {
        if self.subtitle.take().is_some() {
            self.game.runner.finish_speech();
        }
    }

    /// Advance by `dt` seconds.
    pub fn tick(&mut self, dt: f32) -> Request {
        let mut request = Request::default();
        // A screen stops the world: a pause menu should pause.
        if self.screen.is_some() {
            return request;
        }

        if let Some(subtitle) = self.subtitle.as_mut() {
            subtitle.remaining -= dt;
            if subtitle.remaining <= 0.0 {
                self.subtitle = None;
                self.game.runner.finish_speech();
            }
        }
        if let Some((id, left)) = self.anim_wait.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                let id = id.clone();
                self.anim_wait = None;
                self.game.runner.finish_animation(&id);
            }
        }

        for event in self.game.step(&self.room, dt) {
            self.apply(event, &mut request);
        }
        request
    }

    fn apply(&mut self, event: Event, request: &mut Request) {
        match event {
            Event::Say { line, speaker } => {
                let text = self.project.locales.text(&line, &self.language.text);
                // An id with no translation shows as the id. Blanking it
                // hides exactly the gap worth seeing.
                let text = if text.is_empty() {
                    line.clone()
                } else {
                    text.to_string()
                };
                self.note(Note::Said(text.clone()));
                request.voice = self
                    .project
                    .locales
                    .line(&line, &self.language.voice)
                    .and_then(|line| line.voice.clone());
                self.subtitle = Some(Subtitle {
                    remaining: reading_time(&text),
                    line,
                    text,
                    speaker,
                });
            }
            Event::PlaySound(id) => {
                self.note(Note::Sound(id.clone()));
                // The bank the last music change loaded speaks first:
                // "effect 35" is a different recording per chapter, and
                // the music is what says which chapter this is.
                let banked = self
                    .sfx_bank
                    .as_deref()
                    .zip(id.split_once(".sfx"))
                    .map(|(bank, (_, n))| format!("{bank}.sfx{n}"))
                    .filter(|banked| self.project.sound_file(banked).is_some());
                match banked.as_deref().or_else(|| self.project.sound_file(&id)) {
                    Some(file) => request.sound = Some(file.to_string()),
                    None => self.note(Note::Missing(format!("sound {id}"))),
                }
            }
            Event::PlayMusic(id) => {
                self.note(Note::Music(id.clone()));
                request.music = Some(id.and_then(|id| {
                    if let Some(bank) = self.project.sounds.sfx_banks.get(&id).cloned() {
                        self.sfx_bank = Some(bank);
                    }
                    self.project
                        .sound_file(&id)
                        .map(str::to_string)
                        .or_else(|| {
                            self.log.push(Note::Missing(format!("music {id}")));
                            None
                        })
                }));
            }
            Event::ShowImage { image, seconds } => {
                self.note(Note::Image(image.clone()));
                request.still = Some((image, seconds));
            }
            Event::MoveCamera { .. } => {
                // The state already knows where it is going; the host
                // only has to say the view changed, so a renderer that
                // caches anything about it knows to look again.
                request.camera_moved = true;
            }
            Event::PlayAnim { animation, wait, at } => {
                request.animation_at = at;
                // An empty name is "show nothing", which is how a card
                // that holds its last frame is taken off again.
                if animation.is_empty() {
                    self.anim_wait = None;
                    request.animation = Some(String::new());
                    return;
                }
                if wait {
                    let seconds = self
                        .project
                        .animations
                        .get(&animation)
                        .map(|a| a.duration())
                        .filter(|s| *s > 0.0)
                        .unwrap_or(ANIM_FALLBACK);
                    self.anim_wait = Some((animation.clone(), seconds));
                }
                request.animation = Some(animation);
            }
            Event::OpenScreen(id) => {
                if self.project.screen(&id).is_some() {
                    self.screen = Some(id);
                } else {
                    self.note(Note::Missing(format!("screen {id}")));
                }
            }
            Event::EnterRoom { room, entry } => {
                let outcome = self.enter(&room, entry.as_deref());
                request.room_changed |= outcome.room_changed;
                self.game.runner.finish_room_change();
            }
            Event::Skipped { call } => self.note(Note::Skipped(call)),
        }
    }

    fn note(&mut self, note: Note) {
        self.log.push(note);
        let limit = self.log_limit;
        if self.log.len() > limit {
            let excess = self.log.len() - limit;
            self.log.drain(..excess);
        }
    }
}

/// How long to leave a subtitle up.
pub fn reading_time(text: &str) -> f32 {
    SUBTITLE_MIN + text.chars().count() as f32 / CHARS_PER_SECOND
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen;

    #[test]
    fn a_game_is_not_started_until_somebody_starts_it() {
        // A100: the front door. Escape on a title screen used to dismiss
        // it into a room nobody had entered, where the next Escape
        // offered to resume and save a game that had never begun.
        let root = temp("front_door");
        build(&root);
        screen::save(
            &Screen::new("title", Vec2::new(320.0, 200.0)),
            &root.join(screen::SCREEN_DIR),
        )
        .expect("screen");
        crate::pack::GameManifest {
            start_room: "hall".into(),
            start_screen: "title".into(),
            ..Default::default()
        }
        .save(&root)
        .expect("manifest");

        let mut host = Host::new(Project::open(&root));
        host.start();
        assert_eq!(host.screen.as_deref(), Some("title"));
        assert!(!host.started, "showing a title is not playing");

        host.fire(&ScreenAction::NewGame);
        assert!(host.started, "and pressing New Game is");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn back_out_of_a_submenu_lands_on_the_menu_it_came_from() {
        // Pause opens Save; Back from Save means Pause, not the room.
        // Without somewhere to remember that, the way out of a submenu
        // dropped the player two levels.
        let root = temp("screen_stack");
        build(&root);
        for id in ["pause", "save"] {
            screen::save(
                &Screen::new(id, Vec2::new(320.0, 200.0)),
                &root.join(screen::SCREEN_DIR),
            )
            .expect("screen");
        }

        let mut host = Host::new(Project::open(&root));
        host.start();
        host.screen = Some("pause".into());
        host.fire(&ScreenAction::Open {
            screen: "save".into(),
        });
        assert_eq!(host.screen.as_deref(), Some("save"));

        host.fire(&ScreenAction::Close);
        assert_eq!(host.screen.as_deref(), Some("pause"));

        host.fire(&ScreenAction::Close);
        assert!(host.screen.is_none(), "and then the game");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resume_leaves_every_menu_at_once() {
        let root = temp("screen_resume");
        build(&root);
        for id in ["pause", "save"] {
            screen::save(
                &Screen::new(id, Vec2::new(320.0, 200.0)),
                &root.join(screen::SCREEN_DIR),
            )
            .expect("screen");
        }

        let mut host = Host::new(Project::open(&root));
        host.start();
        host.screen = Some("pause".into());
        host.fire(&ScreenAction::Open {
            screen: "save".into(),
        });
        host.fire(&ScreenAction::Resume);
        assert!(host.screen.is_none());

        // And nothing is left underneath to reappear on the next Back.
        host.screen = Some("pause".into());
        host.fire(&ScreenAction::Close);
        assert!(host.screen.is_none(), "a stale menu came back");

        let _ = fs::remove_dir_all(&root);
    }
    use crate::action::{Action, Script};
    use crate::interact::{EntryPoint, Exit, Shape};
    use crate::io;
    use crate::loca::{Line, Locale};
    use crate::walk::Polygon;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("infengine_host_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// A two-room game with a door between them and a line to say.
    fn build(root: &Path) {
        let floor = |room: &mut Room| {
            room.walk.solids.push(Polygon::rect(
                Vec2::new(0.0, 100.0),
                Vec2::new(320.0, 200.0),
            ));
        };
        let mut hall = Room::new("hall", Vec2::new(320.0, 200.0));
        floor(&mut hall);
        hall.entries.push(EntryPoint {
            id: "start".into(),
            position: Vec2::new(30.0, 150.0),
            ..Default::default()
        });
        hall.exits.push(Exit {
            id: "door".into(),
            name: "to the crypt".into(),
            shape: Shape::Rect {
                min: Vec2::new(280.0, 110.0),
                max: Vec2::new(320.0, 190.0),
            },
            target_room: "crypt".into(),
            target_entry: "stairs".into(),
            enabled: true,
            ..Default::default()
        });
        io::save(&hall, &root.join("rooms/hall.room.ron")).expect("hall");

        let mut crypt = Room::new("crypt", Vec2::new(320.0, 200.0));
        floor(&mut crypt);
        crypt.entries.push(EntryPoint {
            id: "stairs".into(),
            position: Vec2::new(300.0, 150.0),
            ..Default::default()
        });
        io::save(&crypt, &root.join("rooms/crypt.room.ron")).expect("crypt");

        let mut entry = Script::new("hall.entry");
        entry.push(Action::Say {
            line: "hall.00".into(),
            speaker: None,
        });
        crate::script_io::save_room("hall", &[entry], &root.join(crate::script_io::SCRIPT_DIR))
            .expect("script");

        let mut english = Locale::new("en", "English");
        english.insert("hall.00", Line::text("A dusty hall."));
        crate::loca::save_locale(&english, &root.join(crate::loca::LOCALE_DIR)).expect("loca");

        GameManifest {
            title: "Test".into(),
            start_room: "hall".into(),
            start_entry: "start".into(),
            ..Default::default()
        }
        .save(root)
        .expect("manifest");
    }

    fn host(name: &str) -> (Host, PathBuf) {
        let root = temp(name);
        build(&root);
        (Host::new(Project::open(&root)), root)
    }

    #[test]
    fn starting_puts_the_player_where_the_manifest_says() {
        let (mut host, root) = host("start");
        let request = host.start();
        assert!(request.room_changed);
        assert_eq!(host.room.id, "hall");
        assert_eq!(host.game.state.actor.position, Vec2::new(30.0, 150.0));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_rooms_entry_script_speaks_and_the_voice_is_asked_for() {
        let (mut host, root) = host("speak");
        host.start();
        host.tick(0.016);
        let subtitle = host.subtitle.clone().expect("a line");
        assert_eq!(subtitle.text, "A dusty hall.");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn walking_through_a_door_loads_the_other_room() {
        let (mut host, root) = host("door");
        host.start();
        host.skip_subtitle();
        host.click(Vec2::new(300.0, 150.0));

        let mut changed = false;
        for _ in 0..600 {
            changed |= host.tick(0.016).room_changed;
            if changed {
                break;
            }
        }
        assert!(changed, "the door has to lead somewhere");
        assert_eq!(host.room.id, "crypt");
        assert_eq!(host.game.state.actor.position, Vec2::new(300.0, 150.0));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_room_is_reported_and_the_player_stays_put() {
        let (mut host, root) = host("missing");
        host.start();
        host.enter("nowhere", None);
        assert_eq!(host.room.id, "hall", "a hole in the game is not a teleport");
        assert!(host.log.iter().any(|n| matches!(n, Note::Missing(_))));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn saving_and_loading_puts_the_player_back_exactly() {
        let (mut host, root) = host("save");
        host.start();
        host.skip_subtitle();
        host.game.state.set_flag("met_brynn", true);
        host.game.state.actor.place(Vec2::new(111.0, 160.0), Default::default());
        host.save(0);

        host.enter("crypt", Some("stairs"));
        assert_eq!(host.room.id, "crypt");

        let request = host.load(0);
        assert!(request.room_changed);
        assert_eq!(host.room.id, "hall");
        assert_eq!(
            host.game.state.actor.position,
            Vec2::new(111.0, 160.0),
            "loading restores where you stood, not the doorway"
        );
        assert!(host.game.state.flag("met_brynn"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_screen_pauses_the_world() {
        let (mut host, root) = host("pause");
        host.start();
        host.screen = Some("pause".into());
        // Nothing advances while a menu is up, so the opening line is
        // still waiting when it closes.
        for _ in 0..600 {
            assert!(host.tick(0.016).is_empty());
        }
        assert!(host.subtitle.is_none());
        host.fire(&ScreenAction::Close);
        host.tick(0.016);
        assert!(host.subtitle.is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn quitting_says_so_rather_than_doing_it() {
        let (mut host, root) = host("quit");
        assert!(host.fire(&ScreenAction::Quit).quit);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_log_does_not_grow_without_bound() {
        let (mut host, root) = host("log");
        host.log_limit = 8;
        for i in 0..40 {
            host.note(Note::Sound(format!("{i}")));
        }
        assert_eq!(host.log.len(), 8);
        assert_eq!(host.log.last(), Some(&Note::Sound("39".into())));
        let _ = fs::remove_dir_all(&root);
    }
}
