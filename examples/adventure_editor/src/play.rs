//! A20.2: play mode - the editor's test harness.
//!
//! Play mode is not a second application. It is the same room, drawn the
//! same way, with a [`Game`] driving it and the overlays still available.
//! The point is testing: set a flag, jump to a scene, click a hotspot and
//! see what the script actually does, including what it fails to do.
//!
//! Everything here is host-independent. The tick takes the room, the
//! locales and a time step, and returns a [`Request`] saying which files
//! the host should play - nothing in this module touches audio, textures
//! or the window, which is what lets it be tested without either.

use std::collections::VecDeque;

use infengine_adventure::game::{Click, Event, Game};
use infengine_adventure::loca::{LanguageChoice, Locales};
use infengine_adventure::{Animation, AnimationSet, Room, Verb};

/// How long a subtitle stays up, at minimum.
const SUBTITLE_MIN: f32 = 1.2;
/// Characters a reader gets through in a second. Deliberately unhurried:
/// a test run that outruns the tester is no use.
const CHARS_PER_SECOND: f32 = 14.0;
/// Longest the log keeps. It is a running commentary, not a record.
const LOG_LIMIT: usize = 200;
/// Fallback length for an animation whose frame rate says nothing.
const ANIM_FALLBACK: f32 = 1.0;

/// A line on screen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Subtitle {
    pub id: String,
    pub text: String,
    pub speaker: Option<String>,
    /// Seconds left before it clears itself.
    pub remaining: f32,
}

/// Files the host should start playing this tick.
///
/// Returned rather than performed so the whole of play mode can run in a
/// test with no audio device and no window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    /// Localisation id whose recording should play.
    pub speak: Option<String>,
    pub sound: Option<String>,
    /// `Some(None)` means stop the music.
    pub music: Option<Option<String>>,
    /// Animation to show in the room.
    pub animation: Option<String>,
    /// Room to load, and where to put the actor.
    pub enter_room: Option<(String, Option<String>)>,
}

impl Request {
    pub fn is_empty(&self) -> bool {
        *self == Request::default()
    }
}

/// The editor's play state.
pub struct PlayMode {
    pub active: bool,
    pub game: Game,
    pub subtitle: Option<Subtitle>,
    /// What the last click resolved to, for the status line.
    pub hover: Click,
    /// Verb the next click carries.
    pub verb: Verb,
    /// Running commentary: what the scripts did, newest last.
    pub log: VecDeque<String>,
    /// A blocking animation and the time left on it.
    anim_wait: Option<(String, f32)>,
}

impl std::fmt::Debug for PlayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayMode")
            .field("active", &self.active)
            .field("room", &self.game.state.room)
            .finish()
    }
}

impl Default for PlayMode {
    fn default() -> Self {
        Self {
            active: false,
            game: Game::default(),
            subtitle: None,
            hover: Click::Nothing,
            verb: Verb::Walk,
            log: VecDeque::new(),
            anim_wait: None,
        }
    }
}

impl PlayMode {
    /// A play mode over a project's scripts.
    pub fn with_scripts(scripts: std::collections::HashMap<String, infengine_adventure::action::Script>) -> Self {
        Self {
            game: Game::new(scripts),
            ..Default::default()
        }
    }

    /// Start playing in `room`, keeping the scripts already loaded.
    ///
    /// The state is *not* reset: a tester who has set up flags by hand
    /// wants to keep them when jumping to the scene they are testing.
    /// Resetting is a separate, deliberate button.
    pub fn begin(&mut self, room: &Room) {
        self.active = true;
        self.subtitle = None;
        self.anim_wait = None;
        self.game.enter(room, None);
        self.note(format!("entered {}", room.id));
    }

    /// Stop playing and leave the state where it stands.
    pub fn end(&mut self) {
        self.active = false;
        self.subtitle = None;
        self.anim_wait = None;
        self.game.runner.stop();
    }

    /// Throw away the play state and start the room over.
    pub fn reset(&mut self, room: &Room) {
        self.game = Game::new_like(&self.game);
        self.log.clear();
        self.begin(room);
    }

    pub fn note(&mut self, message: impl Into<String>) {
        self.log.push_back(message.into());
        while self.log.len() > LOG_LIMIT {
            self.log.pop_front();
        }
    }

    /// Cut the current line short - what clicking through does.
    pub fn skip_subtitle(&mut self) {
        if self.subtitle.take().is_some() {
            self.game.runner.finish_speech();
        }
    }

    /// The host loaded the room the game asked for.
    pub fn room_loaded(&mut self, room: &Room, entry: Option<&str>) {
        self.game.state.enter(room, entry);
        self.game.runner.finish_room_change();
        self.note(format!("entered {}", room.id));
    }
}

/// Advance play mode by `dt` seconds.
///
/// `room` must be the room the actor is in now. Returns what the host
/// should play, and drains the runner's events into the log.
pub fn tick(
    play: &mut PlayMode,
    room: &Room,
    animations: &AnimationSet,
    locales: &Locales,
    language: &LanguageChoice,
    dt: f32,
) -> Request {
    let mut request = Request::default();
    if !play.active {
        return request;
    }

    // Time-based blocks expire before the script is stepped, so a line
    // and the action after it can share one frame rather than costing an
    // extra one each.
    if let Some(subtitle) = play.subtitle.as_mut() {
        subtitle.remaining -= dt;
        if subtitle.remaining <= 0.0 {
            play.subtitle = None;
            play.game.runner.finish_speech();
        }
    }
    if let Some((id, left)) = play.anim_wait.as_mut() {
        *left -= dt;
        if *left <= 0.0 {
            let id = id.clone();
            play.anim_wait = None;
            play.game.runner.finish_animation(&id);
        }
    }

    for event in play.game.step(room, dt) {
        apply(play, &mut request, event, animations, locales, language);
    }
    request
}

fn apply(
    play: &mut PlayMode,
    request: &mut Request,
    event: Event,
    animations: &AnimationSet,
    locales: &Locales,
    language: &LanguageChoice,
) {
    match event {
        Event::MoveCamera { .. } => {}
        Event::Say { line, speaker } => {
            let text = locales.text(&line, &language.text).to_string();
            // An id with no translation shows as the id. Blanking it
            // would hide exactly the gap a test run exists to find.
            let text = if text.is_empty() { line.clone() } else { text };
            play.subtitle = Some(Subtitle {
                remaining: reading_time(&text),
                id: line.clone(),
                text: text.clone(),
                speaker,
            });
            play.note(format!("say: {text}"));
            request.speak = Some(line);
        }
        Event::PlaySound(sound) => {
            play.note(format!("sound: {sound}"));
            request.sound = Some(sound);
        }
        Event::PlayMusic(music) => {
            play.note(match &music {
                Some(track) => format!("music: {track}"),
                None => "music off".to_string(),
            });
            request.music = Some(music);
        }
        Event::PlayAnim { animation, wait, .. } => {
            if wait {
                // An animation the index does not describe still has
                // to end, or the script waits for ever on a typo.
                let seconds = animations
                    .get(&animation)
                    .map(Animation::duration)
                    .filter(|seconds| *seconds > 0.0)
                    .unwrap_or(ANIM_FALLBACK);
                play.anim_wait = Some((animation.clone(), seconds));
            }
            play.note(format!("play: {animation}"));
            request.animation = Some(animation);
        }
        Event::OpenScreen(id) => {
            // The test harness has its own screen picker, so a script
            // asking for one is noted rather than obeyed.
            play.note(format!("show {id}"));
        }
        Event::EnterRoom { room, entry } => {
            play.note(format!("go to {room}"));
            request.enter_room = Some((room, entry));
        }
        Event::Skipped { call } => {
            // The loudest thing in the log, because it is the one thing
            // that means the import is not finished.
            play.note(format!("SKIPPED {call}"));
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
    use glam::Vec2;
    use infengine_adventure::action::{Action, Script};
    use infengine_adventure::loca::{Line, Locale};
    use infengine_adventure::walk::Polygon;
    use std::collections::HashMap;

    fn room() -> Room {
        let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
        room.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        room
    }

    fn locales() -> Locales {
        let mut english = Locale::new("en", "English");
        english.insert("hall.00", Line::text("A dusty hall."));
        let mut locales = Locales::default();
        locales.insert(english);
        locales.fallback = "en".into();
        locales
    }

    fn play_with(actions: Vec<Action>) -> PlayMode {
        let mut scripts = HashMap::new();
        let mut entry = Script::new("hall.entry");
        entry.actions = actions;
        scripts.insert(entry.id.clone(), entry);
        PlayMode {
            game: Game::new(scripts),
            ..Default::default()
        }
    }

    #[test]
    fn nothing_happens_until_play_mode_is_active() {
        let mut play = play_with(vec![Action::SetFlag {
            flag: "ran".into(),
            value: true,
        }]);
        let request = tick(
            &mut play,
            &room(),
            &AnimationSet::default(),
            &locales(),
            &LanguageChoice::same("en"),
            0.016,
        );
        assert!(request.is_empty());
        assert!(!play.game.state.flag("ran"), "the editor is not the game");
    }

    #[test]
    fn a_line_shows_its_text_and_asks_for_its_recording() {
        let room = room();
        let mut play = play_with(vec![Action::Say {
            line: "hall.00".into(),
            speaker: Some("Brandon".into()),
        }]);
        play.begin(&room);

        let request = tick(
            &mut play,
            &room,
            &AnimationSet::default(),
            &locales(),
            &LanguageChoice::same("en"),
            0.016,
        );
        assert_eq!(request.speak.as_deref(), Some("hall.00"));
        let subtitle = play.subtitle.clone().expect("a subtitle");
        assert_eq!(subtitle.text, "A dusty hall.");
        assert_eq!(subtitle.speaker.as_deref(), Some("Brandon"));
    }

    #[test]
    fn an_untranslated_line_shows_its_id_rather_than_nothing() {
        let room = room();
        let mut play = play_with(vec![Action::Say {
            line: "hall.99".into(),
            speaker: None,
        }]);
        play.begin(&room);
        tick(
            &mut play,
            &room,
            &AnimationSet::default(),
            &locales(),
            &LanguageChoice::same("en"),
            0.016,
        );
        assert_eq!(
            play.subtitle.as_ref().map(|s| s.text.as_str()),
            Some("hall.99"),
            "a missing translation must be visible, not blank"
        );
    }

    #[test]
    fn a_subtitle_clears_itself_and_lets_the_script_go_on() {
        let room = room();
        let mut play = play_with(vec![
            Action::Say {
                line: "hall.00".into(),
                speaker: None,
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);
        play.begin(&room);
        let locales = locales();
        let language = LanguageChoice::same("en");

        tick(&mut play, &room, &AnimationSet::default(), &locales, &language, 0.016);
        assert!(!play.game.state.flag("after"));

        // Long enough for any line: the point is that it ends by itself.
        for _ in 0..600 {
            tick(&mut play, &room, &AnimationSet::default(), &locales, &language, 0.016);
        }
        assert!(play.subtitle.is_none());
        assert!(play.game.state.flag("after"));
    }

    #[test]
    fn clicking_through_a_line_skips_it() {
        let room = room();
        let mut play = play_with(vec![
            Action::Say {
                line: "hall.00".into(),
                speaker: None,
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);
        play.begin(&room);
        let locales = locales();
        let language = LanguageChoice::same("en");
        tick(&mut play, &room, &AnimationSet::default(), &locales, &language, 0.016);

        play.skip_subtitle();
        tick(&mut play, &room, &AnimationSet::default(), &locales, &language, 0.016);
        assert!(play.game.state.flag("after"));
    }

    #[test]
    fn an_untranslated_step_shouts_in_the_log() {
        let room = room();
        let mut play = play_with(vec![Action::Unknown {
            call: "kyra:40".into(),
            args: vec![],
        }]);
        play.begin(&room);
        tick(
            &mut play,
            &room,
            &AnimationSet::default(),
            &locales(),
            &LanguageChoice::same("en"),
            0.016,
        );
        assert!(
            play.log.iter().any(|line| line.contains("SKIPPED kyra:40")),
            "log was {:?}",
            play.log
        );
    }

    #[test]
    fn a_blocking_animation_runs_for_its_own_length() {
        let room = room();
        let mut animations = AnimationSet::default();
        let mut door = Animation::new("door", "door.png", 10, Vec2::new(32.0, 32.0)).in_room("hall");
        door.fps = 10.0;
        door.looping = false;
        animations.insert(door);

        let mut play = play_with(vec![
            Action::PlayAnim {
                animation: "door".into(),
                wait: true,
                at: None,
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);
        play.begin(&room);
        let locales = locales();
        let language = LanguageChoice::same("en");

        let request = tick(&mut play, &room, &animations, &locales, &language, 0.016);
        assert_eq!(request.animation.as_deref(), Some("door"));

        // Ten frames at ten a second is one second: half of it is not
        // enough.
        for _ in 0..30 {
            tick(&mut play, &room, &animations, &locales, &language, 0.016);
        }
        assert!(!play.game.state.flag("after"));
        for _ in 0..60 {
            tick(&mut play, &room, &animations, &locales, &language, 0.016);
        }
        assert!(play.game.state.flag("after"));
    }

    #[test]
    fn a_room_change_is_handed_to_the_host_and_waits_for_it() {
        let room = room();
        let mut play = play_with(vec![
            Action::GoToRoom {
                room: "crypt".into(),
                entry: Some("stairs".into()),
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);
        play.begin(&room);
        let locales = locales();
        let language = LanguageChoice::same("en");

        let request = tick(&mut play, &room, &AnimationSet::default(), &locales, &language, 0.016);
        assert_eq!(
            request.enter_room,
            Some(("crypt".to_string(), Some("stairs".to_string())))
        );

        // Until the host answers, nothing moves on - a missing room file
        // must stall visibly rather than teleport the actor.
        for _ in 0..120 {
            tick(&mut play, &room, &AnimationSet::default(), &locales, &language, 0.016);
        }
        assert!(!play.game.state.flag("after"));

        let mut crypt = Room::new("crypt", Vec2::new(320.0, 200.0));
        crypt.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        play.room_loaded(&crypt, Some("stairs"));
        tick(&mut play, &crypt, &AnimationSet::default(), &locales, &language, 0.016);
        assert!(play.game.state.flag("after"));
        assert_eq!(play.game.state.room, "crypt");
    }

    #[test]
    fn leaving_play_mode_keeps_the_state_for_the_next_run() {
        let room = room();
        let mut play = play_with(vec![Action::SetFlag {
            flag: "ran".into(),
            value: true,
        }]);
        play.begin(&room);
        tick(
            &mut play,
            &room,
            &AnimationSet::default(),
            &locales(),
            &LanguageChoice::same("en"),
            0.016,
        );
        play.end();
        assert!(
            play.game.state.flag("ran"),
            "a tester's flags survive stepping out of play mode"
        );
    }

    #[test]
    fn the_log_does_not_grow_without_bound() {
        let mut play = PlayMode::default();
        for i in 0..LOG_LIMIT * 2 {
            play.note(format!("line {i}"));
        }
        assert_eq!(play.log.len(), LOG_LIMIT);
        assert_eq!(
            play.log.back().map(String::as_str),
            Some(format!("line {}", LOG_LIMIT * 2 - 1).as_str()),
            "the newest is kept, not the oldest"
        );
    }

    #[test]
    fn a_longer_line_stays_up_longer() {
        assert!(reading_time("A dusty hall.") < reading_time(
            "A dusty hall, and the shelves along it are heavy with books nobody has opened."
        ));
        assert!(reading_time("") >= SUBTITLE_MIN, "even a blank line is readable");
    }
}
