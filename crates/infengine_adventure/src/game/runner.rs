//! A19.2: running a script against the game state.
//!
//! The runner is a small interpreter over [`Action`]. It owns no
//! resources and performs no output: it changes the [`GameState`] and
//! emits [`Event`]s describing what the host should do. That split is
//! what lets the same scripts run in the editor's preview, in an
//! exported game, and in a headless test without any of them sharing
//! code beyond this file.
//!
//! Blocking is explicit. A `Say` stops the script until the host says
//! the line has finished, because how long a line takes depends on the
//! recording and on the reading speed the player chose - neither of
//! which the interpreter can know.

use glam::Vec2;

use crate::action::{Action, Script};
use crate::room::Room;

use super::state::GameState;

/// Something the host has to carry out.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Show a subtitle and play its recording. The host calls
    /// [`Runner::finish_speech`] when it is over.
    Say {
        line: String,
        speaker: Option<String>,
    },
    PlaySound(String),
    /// `None` means stop the music.
    PlayMusic(Option<String>),
    PlayAnim {
        animation: String,
        /// Whether the script is waiting for it to finish.
        wait: bool,
        /// Where the script wants it, if it said.
        at: Option<Vec2>,
    },
    /// Move the view. `seconds` of zero means cut.
    MoveCamera { to: Vec2, seconds: f32 },
    /// Show a picture for a while. Zero means until the room changes.
    ShowImage { image: String, seconds: f32 },
    /// Show a screen. The script carries on underneath it; the host
    /// decides whether that means paused.
    OpenScreen(String),
    /// Load another room and place the actor. The host calls
    /// [`Runner::finish_room_change`] once it has.
    EnterRoom {
        room: String,
        entry: Option<String>,
    },
    /// A step the importer could not translate. Emitted rather than
    /// silently skipped, so a test run reports what it did not do.
    Skipped { call: String },
}

/// What the runner is waiting for.
///
/// Absent - `Option::None` - is the running state; a variant here always
/// means something is holding the script up.
#[derive(Debug, Clone, PartialEq)]
enum Block {
    /// A `Wait`, counting down.
    Timer(f32),
    /// The actor walking. Cleared when it stops moving.
    Walking,
    /// A line being spoken, cleared by the host.
    Speech,
    /// A blocking animation, cleared by the host when *that* animation
    /// ends. Named so a different animation finishing cannot release it.
    Animation(String),
    /// A room change, cleared by the host once the room is loaded.
    RoomChange,
    /// The view panning. Counted down here rather than cleared by the
    /// host, because unlike an animation the runner knows how long it
    /// takes - it said so itself.
    Camera(f32),
}

/// One level of the action tree being executed.
#[derive(Debug, Clone)]
struct Frame {
    actions: Vec<Action>,
    next: usize,
}

/// Executes a script, one action at a time.
#[derive(Debug, Clone, Default)]
pub struct Runner {
    frames: Vec<Frame>,
    block: Option<Block>,
}

impl Runner {
    /// A runner with nothing to do.
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a script, abandoning anything already running.
    pub fn start(&mut self, script: &Script) {
        self.frames = vec![Frame {
            actions: script.actions.clone(),
            next: 0,
        }];
        self.block = None;
    }

    /// Begin a bare list of actions - what a hotspot's verb produces
    /// before it has been given a name.
    pub fn start_actions(&mut self, actions: Vec<Action>) {
        self.frames = vec![Frame { actions, next: 0 }];
        self.block = None;
    }

    /// Drop everything, leaving the state as it stands.
    pub fn stop(&mut self) {
        self.frames.clear();
        self.block = None;
    }

    /// Is a script running, whether stepping or waiting?
    pub fn is_running(&self) -> bool {
        !self.frames.is_empty() || self.block.is_some()
    }

    /// Is the runner waiting on the host rather than on itself?
    ///
    /// The editor greys the transport out while this holds: nothing will
    /// advance until a line, an animation or a room change completes.
    pub fn is_blocked(&self) -> bool {
        matches!(
            self.block,
            Some(Block::Speech | Block::Animation(_) | Block::RoomChange)
        )
    }

    /// The host finished the line that was being spoken.
    pub fn finish_speech(&mut self) {
        if self.block == Some(Block::Speech) {
            self.block = None;
        }
    }

    /// The host finished an animation. Only releases the script if it is
    /// the one it was waiting for.
    pub fn finish_animation(&mut self, animation: &str) {
        if self.block.as_ref() == Some(&Block::Animation(animation.to_string())) {
            self.block = None;
        }
    }

    /// The host loaded the new room.
    pub fn finish_room_change(&mut self) {
        if self.block == Some(Block::RoomChange) {
            self.block = None;
        }
    }

    /// Advance by `dt` seconds and return what the host must do.
    ///
    /// `room` is the room the actor is in *now*; after an `EnterRoom`
    /// event the host is expected to pass the new one.
    pub fn step(&mut self, state: &mut GameState, room: &Room, dt: f32) -> Vec<Event> {
        // The actor keeps walking whether or not a script is running -
        // clicking the floor with no script is the common case.
        state.actor.advance(dt);
        state.step_camera(room, dt);

        let mut events = Vec::new();
        match &mut self.block {
            Some(Block::Timer(left)) => {
                *left -= dt;
                if *left > 0.0 {
                    return events;
                }
                self.block = None;
            }
            Some(Block::Walking) => {
                if state.actor.is_walking() {
                    return events;
                }
                self.block = None;
            }
            // A pan is the one wait the runner can time itself, because
            // the script said how long it takes.
            Some(Block::Camera(left)) => {
                *left -= dt;
                if *left > 0.0 {
                    return events;
                }
                self.block = None;
            }
            // Waiting on the host: only its notification releases these.
            Some(_) => return events,
            None => {}
        }

        while self.block.is_none() {
            let Some(action) = self.next_action() else {
                break;
            };
            self.run(&action, state, room, &mut events);
        }
        events
    }

    /// Take the next action, unwinding finished frames.
    fn next_action(&mut self) -> Option<Action> {
        loop {
            let frame = self.frames.last_mut()?;
            match frame.actions.get(frame.next) {
                Some(action) => {
                    let action = action.clone();
                    frame.next += 1;
                    return Some(action);
                }
                None => {
                    self.frames.pop();
                }
            }
        }
    }

    fn run(
        &mut self,
        action: &Action,
        state: &mut GameState,
        room: &Room,
        events: &mut Vec<Event>,
    ) {
        match action {
            Action::Say { line, speaker } => {
                self.block = Some(Block::Speech);
                events.push(Event::Say {
                    line: line.clone(),
                    speaker: speaker.clone(),
                });
            }
            Action::Walk { to } => {
                if state.actor.walk_to(&room.walk, *to) {
                    self.block = Some(Block::Walking);
                }
            }
            Action::Face { facing } => state.actor.facing = *facing,
            Action::OpenScreen { screen } => {
                events.push(Event::OpenScreen(screen.clone()));
            }
            Action::GoToRoom { room: target, entry } => {
                self.block = Some(Block::RoomChange);
                events.push(Event::EnterRoom {
                    room: target.clone(),
                    entry: entry.clone(),
                });
            }
            Action::PlayAnim { animation, wait, at } => {
                if *wait {
                    self.block = Some(Block::Animation(animation.clone()));
                }
                events.push(Event::PlayAnim {
                    animation: animation.clone(),
                    wait: *wait,
                    at: *at,
                });
            }
            Action::MoveCamera { to, seconds } => {
                state.camera_to(*to, *seconds);
                if *seconds > 0.0 {
                    self.block = Some(Block::Camera(*seconds));
                }
                events.push(Event::MoveCamera {
                    to: *to,
                    seconds: *seconds,
                });
            }
            Action::ShowImage { image, seconds } => {
                // The script waits the picture out: a still is shown so
                // it can be looked at, and running on underneath it
                // would end it the moment something else happened.
                if *seconds > 0.0 {
                    self.block = Some(Block::Timer(*seconds));
                }
                events.push(Event::ShowImage {
                    image: image.clone(),
                    seconds: *seconds,
                });
            }
            Action::PlaySound { sound } => events.push(Event::PlaySound(sound.clone())),
            Action::PlayMusic { music } => events.push(Event::PlayMusic(music.clone())),
            Action::Wait { seconds } => {
                if *seconds > 0.0 {
                    self.block = Some(Block::Timer(*seconds));
                }
            }
            Action::SetFlag { flag, value } => state.set_flag(flag.clone(), *value),
            Action::Give { item } => state.give(item.clone()),
            Action::Take { item } => state.take(item),
            Action::EnableHotspot { hotspot, enabled } => {
                state.set_hotspot(hotspot.clone(), *enabled);
            }
            Action::Branch {
                on,
                then,
                otherwise,
            } => {
                // A question we cannot answer is false rather than a
                // guess, and it says so: a branch silently taking its
                // else side would look like the game deciding, not like
                // the importer not knowing.
                let yes = match on {
                    crate::action::Condition::Flag(flag) => state.flag(flag),
                    crate::action::Condition::Holding(item) => state.hand == *item,
                    crate::action::Condition::Unresolved { call, .. } => {
                        events.push(Event::Skipped { call: call.clone() });
                        false
                    }
                };
                let taken = if yes { then } else { otherwise };
                if !taken.is_empty() {
                    self.frames.push(Frame {
                        actions: taken.clone(),
                        next: 0,
                    });
                }
            }
            Action::SetHand { item } => state.hand = item.clone(),
            Action::PlaceItem { item, at } => {
                let room = state.room.clone();
                state.place(room, item.clone(), *at);
            }
            Action::PickUp { item } => {
                let room = state.room.clone();
                if let Some(ground) = state.take_ground(&room, item) {
                    // A full hand leaves its item where this one lay -
                    // the classic swap, atomic on purpose.
                    if let Some(held) = state.hand.replace(ground.item) {
                        state.place(room, held, ground.at);
                    }
                }
            }
            Action::Unknown { call, .. } => events.push(Event::Skipped { call: call.clone() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Condition;
    use glam::Vec2;

    use crate::interact::Facing;
    use crate::walk::Polygon;

    fn room() -> Room {
        let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
        room.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        room
    }

    fn run_to_completion(runner: &mut Runner, state: &mut GameState, room: &Room) -> Vec<Event> {
        let mut all = Vec::new();
        for _ in 0..1000 {
            if !runner.is_running() {
                break;
            }
            all.extend(runner.step(state, room, 1.0 / 60.0));
            // Stand in for a host that answers instantly.
            runner.finish_speech();
            runner.finish_room_change();
            if let Some(Block::Animation(id)) = runner.block.clone() {
                runner.finish_animation(&id);
            }
        }
        all
    }

    #[test]
    fn state_changing_actions_need_no_host_at_all() {
        let room = room();
        let mut state = GameState::start(&room, None);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::SetFlag {
                flag: "opened".into(),
                value: true,
            },
            Action::Give { item: "key".into() },
            Action::EnableHotspot {
                hotspot: "brick".into(),
                enabled: false,
            },
        ]);

        let events = runner.step(&mut state, &room, 0.016);
        assert!(events.is_empty(), "nothing for the host to do");
        assert!(!runner.is_running(), "and it ran to the end in one step");
        assert!(state.flag("opened"));
        assert!(state.has("key"));
        assert_eq!(state.hotspots.get("brick"), Some(&false));
    }

    #[test]
    fn a_line_holds_the_script_until_the_host_answers() {
        let room = room();
        let mut state = GameState::start(&room, None);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::Say {
                line: "hall.00".into(),
                speaker: None,
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);

        let events = runner.step(&mut state, &room, 0.016);
        assert_eq!(events.len(), 1);
        assert!(runner.is_blocked());
        // A minute of ticking must not skip past a line nobody ended.
        for _ in 0..3600 {
            runner.step(&mut state, &room, 0.016);
        }
        assert!(!state.flag("after"));

        runner.finish_speech();
        runner.step(&mut state, &room, 0.016);
        assert!(state.flag("after"));
    }

    #[test]
    fn a_different_animation_finishing_does_not_release_the_wait() {
        let room = room();
        let mut state = GameState::start(&room, None);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::PlayAnim {
                animation: "door_opens".into(),
                wait: true,
                at: None,
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);

        runner.step(&mut state, &room, 0.016);
        runner.finish_animation("torch_flickers");
        runner.step(&mut state, &room, 0.016);
        assert!(!state.flag("after"), "the wrong animation must not count");

        runner.finish_animation("door_opens");
        runner.step(&mut state, &room, 0.016);
        assert!(state.flag("after"));
    }

    #[test]
    fn an_unwaited_animation_does_not_stop_the_script() {
        let room = room();
        let mut state = GameState::start(&room, None);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::PlayAnim {
                animation: "torch".into(),
                wait: false,
                at: None,
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);
        runner.step(&mut state, &room, 0.016);
        assert!(state.flag("after"));
        assert!(!runner.is_running());
    }

    #[test]
    fn a_wait_counts_down_in_real_time() {
        let room = room();
        let mut state = GameState::start(&room, None);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::Wait { seconds: 0.5 },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);

        // The first step is the one that *starts* the wait; the clock
        // runs from the tick after it.
        runner.step(&mut state, &room, 0.016);
        runner.step(&mut state, &room, 0.3);
        assert!(!state.flag("after"));
        runner.step(&mut state, &room, 0.3);
        assert!(state.flag("after"));
    }

    #[test]
    fn a_walk_blocks_until_the_actor_arrives() {
        let room = room();
        let mut state = GameState::start(&room, None);
        state.actor.place(Vec2::new(10.0, 150.0), Facing::Right);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::Walk {
                to: Vec2::new(250.0, 150.0),
            },
            Action::SetFlag {
                flag: "arrived".into(),
                value: true,
            },
        ]);

        runner.step(&mut state, &room, 0.016);
        assert!(state.actor.is_walking());
        assert!(!state.flag("arrived"));

        for _ in 0..300 {
            runner.step(&mut state, &room, 0.016);
        }
        assert!(state.flag("arrived"));
        assert!(state.actor.position.abs_diff_eq(Vec2::new(250.0, 150.0), 1.0));
    }

    #[test]
    fn walking_to_where_the_actor_stands_does_not_deadlock() {
        let room = room();
        let mut state = GameState::start(&room, None);
        state.actor.place(Vec2::new(10.0, 150.0), Facing::Right);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::Walk {
                to: Vec2::new(10.0, 150.0),
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);
        runner.step(&mut state, &room, 0.016);
        assert!(state.flag("after"), "a zero-length walk is not a wait");
    }

    #[test]
    fn a_branch_runs_one_side_and_returns_to_the_parent() {
        let room = room();
        let mut state = GameState::start(&room, None);
        state.set_flag("has_key", true);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::Branch {
                on: Condition::Flag("has_key".into()),
                then: vec![Action::SetFlag {
                    flag: "opened".into(),
                    value: true,
                }],
                otherwise: vec![Action::SetFlag {
                    flag: "locked".into(),
                    value: true,
                }],
            },
            Action::SetFlag {
                flag: "afterwards".into(),
                value: true,
            },
        ]);

        run_to_completion(&mut runner, &mut state, &room);
        assert!(state.flag("opened"));
        assert!(!state.flag("locked"));
        assert!(state.flag("afterwards"), "and the parent frame resumed");
    }

    #[test]
    fn an_untranslated_step_is_reported_not_swallowed() {
        let room = room();
        let mut state = GameState::start(&room, None);
        let mut runner = Runner::new();
        runner.start_actions(vec![Action::Unknown {
            call: "kyra:40".into(),
            args: vec![1, 2],
        }]);
        let events = runner.step(&mut state, &room, 0.016);
        assert_eq!(
            events,
            vec![Event::Skipped {
                call: "kyra:40".into()
            }]
        );
    }

    #[test]
    fn a_room_change_waits_for_the_host_to_load_it() {
        let room = room();
        let mut state = GameState::start(&room, None);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::GoToRoom {
                room: "crypt".into(),
                entry: Some("stairs".into()),
            },
            Action::SetFlag {
                flag: "after".into(),
                value: true,
            },
        ]);

        let events = runner.step(&mut state, &room, 0.016);
        assert_eq!(
            events,
            vec![Event::EnterRoom {
                room: "crypt".into(),
                entry: Some("stairs".into()),
            }]
        );
        // The state still says the old room: only the host, which owns
        // the room files, can decide the change actually happened.
        assert_eq!(state.room, "hall");
        assert!(runner.is_blocked());

        let mut crypt = Room::new("crypt", Vec2::new(320.0, 200.0));
        crypt.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        state.enter(&crypt, Some("stairs"));
        runner.finish_room_change();
        runner.step(&mut state, &crypt, 0.016);
        assert!(state.flag("after"));
        assert_eq!(state.room, "crypt");
    }

    #[test]
    fn starting_a_script_abandons_the_one_running() {
        let room = room();
        let mut state = GameState::start(&room, None);
        let mut runner = Runner::new();
        runner.start_actions(vec![
            Action::Say {
                line: "a".into(),
                speaker: None,
            },
            Action::SetFlag {
                flag: "old".into(),
                value: true,
            },
        ]);
        runner.step(&mut state, &room, 0.016);

        runner.start_actions(vec![Action::SetFlag {
            flag: "new".into(),
            value: true,
        }]);
        runner.step(&mut state, &room, 0.016);
        assert!(state.flag("new"));
        assert!(!state.flag("old"), "the interrupted script does not resume");
    }

    #[test]
    fn the_actor_walks_even_with_no_script_running() {
        let room = room();
        let mut state = GameState::start(&room, None);
        state.actor.place(Vec2::new(10.0, 150.0), Facing::Right);
        state.actor.walk_to(&room.walk, Vec2::new(90.0, 150.0));

        let mut runner = Runner::new();
        assert!(!runner.is_running());
        runner.step(&mut state, &room, 1.0);
        assert!(state.actor.position.abs_diff_eq(Vec2::new(90.0, 150.0), 1e-3));
    }
}
