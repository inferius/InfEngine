//! A19: playing the game.
//!
//! Three pieces: the [`Actor`] that moves, the [`GameState`] it moves
//! through, and the [`Runner`] that executes scripts against both. On
//! top sits [`Game`], which turns a click into a list of actions and
//! hands it to the runner.
//!
//! Building a click into actions rather than into its own state machine
//! is the whole trick. "Walk to the door, face it, then run its script"
//! *is* a script, so approaching a hotspot needs no separate code path,
//! interrupting it is just starting another script, and the editor can
//! show what a click will do before the player makes it.

mod actor;
mod runner;
mod state;

use std::collections::HashMap;

use glam::Vec2;

use crate::action::{Action, Script};
use crate::interact::{Facing, Verb};
use crate::room::Room;

pub use actor::{Actor, DEFAULT_SPEED};
pub use runner::{Event, Runner};
pub use state::GameState;

/// What clicking somewhere means.
///
/// Returned so the host can show it - the status line under the cursor
/// is this, worked out before the button goes down.
/// How big a ground item draws and clicks, in room pixels.
pub const ITEM_SIZE: f32 = 16.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Click {
    /// Empty, unreachable space.
    Nothing,
    /// Bare floor: walk there - and drop the held item, if any.
    Walk { to: Vec2 },
    /// An item lying on the floor: pick it up, swapping with the hand.
    Item { item: String, at: Vec2 },
    /// A hotspot, with the script the verb resolves to, if any.
    Hotspot {
        id: String,
        name: String,
        verb: Verb,
        script: Option<String>,
    },
    /// An exit to another room.
    Exit {
        id: String,
        name: String,
        target_room: String,
    },
}

impl Click {
    /// Text for the status line.
    pub fn label(&self) -> String {
        match self {
            Click::Nothing => String::new(),
            Click::Walk { .. } => "walk".to_string(),
            Click::Item { item, .. } => format!("take {item}"),
            Click::Hotspot { name, id, verb, .. } => {
                let what = if name.is_empty() { id } else { name };
                format!("{} {what}", verb.name())
            }
            Click::Exit {
                name,
                id,
                target_room,
            } => {
                if !name.is_empty() {
                    name.clone()
                } else if !id.is_empty() {
                    id.clone()
                } else {
                    format!("to {target_room}")
                }
            }
        }
    }
}

/// A game in progress: its state, its scripts, and what is running.
#[derive(Debug, Default)]
pub struct Game {
    pub state: GameState,
    pub runner: Runner,
    /// Every script the project defines, by id.
    scripts: HashMap<String, Script>,
}

impl Game {
    pub fn new(scripts: HashMap<String, Script>) -> Self {
        Self {
            state: GameState::default(),
            runner: Runner::new(),
            scripts,
        }
    }

    /// A fresh game with the same scripts.
    ///
    /// What the editor's reset button needs: throw the play state away
    /// without reloading the project from disk.
    pub fn new_like(other: &Game) -> Self {
        Self::new(other.scripts.clone())
    }

    pub fn script(&self, id: &str) -> Option<&Script> {
        self.scripts.get(id)
    }

    pub fn script_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.scripts.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }

    /// Scripts belonging to a room, in order.
    ///
    /// By id prefix, which is the only thing that ties a script to a
    /// room: they are grouped per file for convenience, not owned.
    pub fn scripts_for(&self, room: &str) -> Vec<&str> {
        let prefix = format!("{room}.");
        let mut ids: Vec<&str> = self
            .scripts
            .keys()
            .filter(|id| id.starts_with(&prefix))
            .map(String::as_str)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Enter a room and run its entry script, if it has one.
    ///
    /// `<room>.entry` is what an authored room uses. `<room>.entry0` is
    /// the fallback because an imported game numbers its scripts by the
    /// source's own entry table, and the first of those is the routine
    /// the original ran on arrival.
    pub fn enter(&mut self, room: &Room, entry: Option<&str>) {
        self.state.enter(room, entry);
        self.runner.stop();
        let prefix = room.script_prefix();
        let id = [format!("{prefix}.entry"), format!("{prefix}.entry0")]
            .into_iter()
            .find(|id| self.scripts.contains_key(id));
        if let Some(script) = id.and_then(|id| self.scripts.get(&id)).cloned() {
            self.runner.start(&script);
        }
    }

    /// Run a script by id, whatever is running now.
    ///
    /// The editor calls this to test a script that nothing points at
    /// yet - an imported game arrives with its logic decompiled but its
    /// hotspots still to be drawn.
    pub fn run_script(&mut self, id: &str) -> bool {
        let Some(script) = self.scripts.get(id).cloned() else {
            return false;
        };
        self.runner.start(&script);
        true
    }

    /// What a click at `point` would do, without doing it.
    ///
    /// Two rules decide between the things under the cursor, and both
    /// exist because an imported game broke without them.
    ///
    /// A click carrying [`Verb::Walk`] is a player who chose no verb -
    /// which is every click in a game with no verb menu - so the thing
    /// clicked answers with its own. Kyrandia's hotspots all bind their
    /// script to Use, and taking the incoming Walk literally made every
    /// one of the 408 imported hotspots do nothing at all.
    ///
    /// And a hotspot with nothing to run loses to a door underneath it.
    /// The imported hotspots are rectangles from bytecode and the exits
    /// are strips along the room's edge; where they overlap, the one
    /// that does something should win, or a room becomes impossible to
    /// leave from that side.
    pub fn resolve(&self, room: &Room, point: Vec2, verb: Verb) -> Click {
        // Items lie on top of everything, so they take the click first.
        if let Some(click) = self.item_at(room, point) {
            return click;
        }
        if let Some(hotspot) = self.state.hotspot_at(room, point) {
            let verb = if verb == Verb::default() {
                hotspot.default_verb
            } else {
                verb
            };
            let script = hotspot.action(verb).map(str::to_string);
            let covered = room
                .exits
                .iter()
                .any(|exit| exit.enabled && exit.shape.contains(point));
            if script.is_some() || !covered {
                return Click::Hotspot {
                    id: hotspot.id.clone(),
                    name: hotspot.name.clone(),
                    verb,
                    script,
                };
            }
        }
        if let Some(exit) = room
            .exits
            .iter()
            .filter(|e| e.enabled && e.shape.contains(point))
            .min_by(|a, b| {
                a.shape
                    .area()
                    .partial_cmp(&b.shape.area())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            return Click::Exit {
                id: exit.id.clone(),
                name: exit.name.clone(),
                target_room: exit.target_room.clone(),
            };
        }
        if room.walk.clamp(point).is_some() {
            return Click::Walk { to: point };
        }
        Click::Nothing
    }

    /// Click, and start whatever it means.
    ///
    /// Returns what was resolved so the caller can report it. A click
    /// while something is blocked on the host is ignored: a line being
    /// spoken should not be cut short by a stray click on the floor.
    /// The ground item under a point, if any. Items draw on top of
    /// scenery, so they win the click over hotspots the same way.
    fn item_at(&self, room: &Room, point: Vec2) -> Option<Click> {
        let _ = room;
        self.state
            .ground_in(&self.state.room)
            .iter()
            .find(|ground| {
                point.x >= ground.at.x
                    && point.x <= ground.at.x + ITEM_SIZE
                    && point.y >= ground.at.y
                    && point.y <= ground.at.y + ITEM_SIZE
            })
            .map(|ground| Click::Item {
                item: ground.item.clone(),
                at: ground.at,
            })
    }

    pub fn click(&mut self, room: &Room, point: Vec2, verb: Verb) -> Click {
        let click = self.resolve(room, point, verb);
        if self.runner.is_blocked() {
            return click;
        }
        let actions = self.actions_for(room, &click);
        if !actions.is_empty() {
            self.runner.start_actions(actions);
        }
        click
    }

    /// Build the action list a click turns into.
    fn actions_for(&self, room: &Room, click: &Click) -> Vec<Action> {
        match click {
            Click::Nothing => Vec::new(),
            // Walking with a full hand is how a thing is put down: go
            // there, lay it where clicked, and the hand is free.
            Click::Walk { to } => match self.state.hand.clone() {
                Some(item) => vec![
                    Action::Walk { to: *to },
                    Action::PlaceItem {
                        item,
                        at: *to - glam::Vec2::new(ITEM_SIZE * 0.5, ITEM_SIZE),
                    },
                    Action::SetHand { item: None },
                ],
                None => vec![Action::Walk { to: *to }],
            },
            Click::Item { item, at } => vec![
                Action::Walk {
                    to: *at + glam::Vec2::new(ITEM_SIZE * 0.5, ITEM_SIZE),
                },
                Action::PickUp { item: item.clone() },
            ],
            Click::Hotspot { id, script, .. } => {
                let Some(hotspot) = room.hotspots.iter().find(|h| &h.id == id) else {
                    return Vec::new();
                };
                let mut actions = Vec::new();
                if let Some(walk_to) = hotspot.walk_to {
                    actions.push(Action::Walk { to: walk_to });
                }
                if hotspot.facing != Facing::default() || hotspot.walk_to.is_some() {
                    actions.push(Action::Face {
                        facing: hotspot.facing,
                    });
                }
                // An unbound verb still walks the actor over. Doing
                // nothing at all reads as a dropped click.
                if let Some(script) = script.as_deref().and_then(|id| self.scripts.get(id)) {
                    actions.extend(script.actions.iter().cloned());
                }
                actions
            }
            Click::Exit { id, .. } => {
                let Some(exit) = room.exits.iter().find(|e| &e.id == id) else {
                    return Vec::new();
                };
                let mut actions = Vec::new();
                if let Some(walk_to) = exit.walk_to {
                    actions.push(Action::Walk { to: walk_to });
                }
                actions.push(Action::GoToRoom {
                    room: exit.target_room.clone(),
                    entry: (!exit.target_entry.is_empty()).then(|| exit.target_entry.clone()),
                });
                actions
            }
        }
    }

    /// Advance the game by `dt` seconds.
    pub fn step(&mut self, room: &Room, dt: f32) -> Vec<Event> {
        self.runner.step(&mut self.state, room, dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interact::{Exit, Hotspot, Shape};
    use crate::walk::Polygon;

    /// A room where a clickable thing sits on top of a doorway, which
    /// is what an imported game looks like: the things come from
    /// bytecode and the doors are strips along the room's edge.
    #[test]
    fn a_ground_item_takes_the_click_and_swaps_with_the_hand() {
        let room = room();
        let mut game = Game::default();
        game.state.enter(&room, None);
        game.state.place("hall", "kyra.item5", Vec2::new(120.0, 40.0));

        // Over the painting hotspot, but the item lies on top.
        let click = game.resolve(&room, Vec2::new(125.0, 45.0), Verb::Walk);
        assert!(matches!(&click, Click::Item { item, .. } if item == "kyra.item5"));

        // The composed gesture: walk under it, then the atomic swap.
        let actions = game.actions_for(&room, &click);
        assert!(matches!(actions.last(), Some(Action::PickUp { .. })));

        // The swap itself, straight through the runner's rules.
        game.state.hand = Some("kyra.item9".into());
        game.runner.start_actions(vec![Action::PickUp {
            item: "kyra.item5".into(),
        }]);
        game.step(&room, 0.016);
        assert_eq!(game.state.hand.as_deref(), Some("kyra.item5"));
        let ground = game.state.ground_in("hall");
        assert_eq!(ground.len(), 1);
        assert_eq!(ground[0].item, "kyra.item9");
        assert_eq!(ground[0].at, Vec2::new(120.0, 40.0), "left where the other lay");
    }

    #[test]
    fn walking_with_a_full_hand_lays_the_item_down() {
        let room = room();
        let mut game = Game::default();
        game.state.enter(&room, None);
        game.state.hand = Some("kyra.item3".into());

        let click = game.resolve(&room, Vec2::new(50.0, 150.0), Verb::Walk);
        let actions = game.actions_for(&room, &click);
        assert!(
            matches!(actions.first(), Some(Action::Walk { .. })),
            "{actions:?}"
        );
        assert!(matches!(&actions[1], Action::PlaceItem { item, .. } if item == "kyra.item3"));
        assert_eq!(actions.last(), Some(&Action::SetHand { item: None }));
    }

    fn room_with_overlap(script: bool) -> Room {
        let mut room = Room::new("cave", Vec2::new(320.0, 200.0));
        room.walk
            .solids
            .push(Polygon::rect(Vec2::ZERO, Vec2::new(320.0, 200.0)));
        room.hotspots.push(Hotspot {
            id: "spot1".into(),
            name: "spot1".into(),
            shape: Shape::Rect {
                min: Vec2::ZERO,
                max: Vec2::new(60.0, 200.0),
            },
            default_verb: Verb::Use,
            actions: if script {
                vec![(Verb::Use, "cave.spot1".into())]
            } else {
                Vec::new()
            },
            enabled: true,
            ..Default::default()
        });
        room.exits.push(Exit {
            id: "west".into(),
            name: "west".into(),
            shape: Shape::Rect {
                min: Vec2::ZERO,
                max: Vec2::new(14.0, 200.0),
            },
            target_room: "broken".into(),
            enabled: true,
            ..Default::default()
        });
        room
    }

    #[test]
    fn a_click_with_no_verb_uses_the_things_own() {
        // Every click in a game with no verb menu arrives as Walk.
        // Taking that literally left all 408 hotspots of an imported
        // game running nothing at all, because they bind to Use.
        let game = Game::default();
        let room = room_with_overlap(true);
        let click = game.resolve(&room, Vec2::new(40.0, 100.0), Verb::default());
        let Click::Hotspot { verb, script, .. } = click else {
            panic!("expected the thing, got {click:?}");
        };
        assert_eq!(verb, Verb::Use);
        assert_eq!(script.as_deref(), Some("cave.spot1"));
    }

    #[test]
    fn a_chosen_verb_is_not_overridden() {
        let game = Game::default();
        let room = room_with_overlap(true);
        let click = game.resolve(&room, Vec2::new(40.0, 100.0), Verb::Look);
        let Click::Hotspot { verb, script, .. } = click else {
            panic!("expected the thing");
        };
        assert_eq!(verb, Verb::Look);
        assert!(script.is_none(), "nothing is bound to Look here");
    }

    #[test]
    fn a_thing_that_does_nothing_loses_to_the_door_under_it() {
        // Otherwise a rectangle sitting over the room's edge makes that
        // side impossible to leave, and the click looks like it was
        // dropped.
        let game = Game::default();
        let room = room_with_overlap(false);
        let click = game.resolve(&room, Vec2::new(7.0, 100.0), Verb::default());
        assert!(
            matches!(click, Click::Exit { ref target_room, .. } if target_room == "broken"),
            "{click:?}"
        );
    }

    #[test]
    fn a_thing_that_does_something_keeps_the_click() {
        let game = Game::default();
        let room = room_with_overlap(true);
        let click = game.resolve(&room, Vec2::new(7.0, 100.0), Verb::default());
        assert!(matches!(click, Click::Hotspot { .. }), "{click:?}");
    }

    #[test]
    fn a_thing_away_from_any_door_keeps_the_click_even_with_nothing_to_run() {
        // The rule is about doors, not about being empty: a scenery
        // hotspot with no script still answers, so the status line can
        // name what the cursor is over.
        let game = Game::default();
        let room = room_with_overlap(false);
        let click = game.resolve(&room, Vec2::new(40.0, 100.0), Verb::default());
        assert!(matches!(click, Click::Hotspot { .. }), "{click:?}");
    }

    fn room() -> Room {
        let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
        room.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        room.hotspots.push(Hotspot {
            id: "painting".into(),
            name: "a painting".into(),
            shape: Shape::Rect {
                min: Vec2::new(100.0, 20.0),
                max: Vec2::new(160.0, 80.0),
            },
            walk_to: Some(Vec2::new(130.0, 150.0)),
            facing: Facing::Back,
            actions: vec![(Verb::Look, "hall.painting.look".into())],
            enabled: true,
            ..Default::default()
        });
        room.exits.push(Exit {
            id: "door".into(),
            name: "to the courtyard".into(),
            shape: Shape::Rect {
                min: Vec2::new(290.0, 60.0),
                max: Vec2::new(320.0, 160.0),
            },
            target_room: "courtyard".into(),
            target_entry: "gate".into(),
            walk_to: Some(Vec2::new(280.0, 150.0)),
            enabled: true,
        });
        room
    }

    fn game() -> Game {
        let mut scripts = HashMap::new();
        let mut look = Script::new("hall.painting.look");
        look.push(Action::Say {
            line: "hall.00".into(),
            speaker: None,
        });
        scripts.insert(look.id.clone(), look);
        Game::new(scripts)
    }

    #[test]
    fn a_click_on_the_floor_walks() {
        let room = room();
        let mut game = game();
        game.enter(&room, None);
        let click = game.click(&room, Vec2::new(200.0, 180.0), Verb::Walk);
        assert_eq!(
            click,
            Click::Walk {
                to: Vec2::new(200.0, 180.0)
            }
        );
        game.step(&room, 0.016);
        assert!(game.state.actor.is_walking());
    }

    #[test]
    fn a_hotspot_is_approached_before_its_script_runs() {
        let room = room();
        let mut game = game();
        game.enter(&room, None);
        game.state.actor.place(Vec2::new(10.0, 190.0), Facing::Right);

        let click = game.click(&room, Vec2::new(130.0, 50.0), Verb::Look);
        assert!(matches!(click, Click::Hotspot { .. }));
        assert_eq!(click.label(), "look a painting");

        // Nothing is said until the actor gets there.
        let mut spoke = false;
        for _ in 0..600 {
            for event in game.step(&room, 0.016) {
                if matches!(event, Event::Say { .. }) {
                    spoke = true;
                }
            }
            if spoke {
                break;
            }
        }
        assert!(spoke, "the script eventually runs");
        assert!(
            game.state
                .actor
                .position
                .abs_diff_eq(Vec2::new(130.0, 150.0), 1.0),
            "and only after arriving at the walk-to point"
        );
        assert_eq!(game.state.actor.facing, Facing::Back);
    }

    #[test]
    fn a_verb_with_no_script_still_walks_the_actor_over() {
        let room = room();
        let mut game = game();
        game.enter(&room, None);
        game.state.actor.place(Vec2::new(10.0, 190.0), Facing::Right);

        // Nothing is bound to `use` on the painting.
        let click = game.click(&room, Vec2::new(130.0, 50.0), Verb::Use);
        assert!(matches!(click, Click::Hotspot { script: None, .. }));
        game.step(&room, 0.016);
        assert!(game.state.actor.is_walking(), "a click must not vanish");
    }

    #[test]
    fn an_exit_walks_over_and_then_changes_room() {
        let room = room();
        let mut game = game();
        game.enter(&room, None);
        game.state.actor.place(Vec2::new(10.0, 190.0), Facing::Right);
        game.click(&room, Vec2::new(300.0, 100.0), Verb::Walk);

        let mut change = None;
        for _ in 0..600 {
            for event in game.step(&room, 0.016) {
                if let Event::EnterRoom { room, entry } = event {
                    change = Some((room, entry));
                }
            }
            if change.is_some() {
                break;
            }
        }
        assert_eq!(
            change,
            Some(("courtyard".to_string(), Some("gate".to_string())))
        );
    }

    #[test]
    fn a_click_is_ignored_while_a_line_is_being_spoken() {
        let room = room();
        let mut game = game();
        game.enter(&room, None);
        game.state.actor.place(Vec2::new(130.0, 150.0), Facing::Back);
        game.click(&room, Vec2::new(130.0, 50.0), Verb::Look);
        game.step(&room, 0.016);
        assert!(game.runner.is_blocked());

        game.click(&room, Vec2::new(20.0, 190.0), Verb::Walk);
        game.step(&room, 0.016);
        assert!(
            !game.state.actor.is_walking(),
            "the line finishes before the player moves on"
        );
    }

    #[test]
    fn a_click_outside_the_floor_and_off_everything_means_nothing() {
        let room = room();
        let game = game();
        // Above the floor, over no hotspot: clamp still finds a spot, so
        // it walks. Only a room with no floor at all resolves to
        // nothing, which is what an unauthored room looks like.
        assert!(matches!(
            game.resolve(&room, Vec2::new(10.0, 10.0), Verb::Walk),
            Click::Walk { .. }
        ));

        let empty = Room::new("void", Vec2::new(320.0, 200.0));
        assert_eq!(
            game.resolve(&empty, Vec2::new(10.0, 10.0), Verb::Walk),
            Click::Nothing
        );
    }

    #[test]
    fn an_imported_rooms_first_entry_script_runs_on_arrival() {
        let room = room();
        let mut scripts = HashMap::new();
        let mut entry = Script::new("hall.entry0");
        entry.push(Action::SetFlag {
            flag: "arrived".into(),
            value: true,
        });
        scripts.insert(entry.id.clone(), entry);
        scripts.insert(
            "hall.entry7".into(),
            Script::new("hall.entry7"),
        );

        let mut game = Game::new(scripts);
        game.enter(&room, None);
        game.step(&room, 0.016);
        assert!(game.state.flag("arrived"));
        assert_eq!(game.scripts_for("hall"), vec!["hall.entry0", "hall.entry7"]);
    }

    #[test]
    fn a_script_can_be_run_by_hand() {
        let room = room();
        let mut game = game();
        game.enter(&room, None);
        assert!(!game.run_script("nothing.like.this"));
        assert!(game.run_script("hall.painting.look"));
        let events = game.step(&room, 0.016);
        assert!(matches!(events.first(), Some(Event::Say { .. })));
    }

    #[test]
    fn entering_a_room_runs_its_entry_script() {
        let room = room();
        let mut scripts = HashMap::new();
        let mut entry = Script::new("hall.entry");
        entry.push(Action::SetFlag {
            flag: "arrived".into(),
            value: true,
        });
        scripts.insert(entry.id.clone(), entry);

        let mut game = Game::new(scripts);
        game.enter(&room, None);
        game.step(&room, 0.016);
        assert!(game.state.flag("arrived"));
    }

    #[test]
    fn a_room_with_no_floor_can_still_be_left_through_its_exits() {
        // Black Mirror ships doorways but not walkable floors, and
        // drawing 259 of those by hand before anything can be tested
        // would be a poor trade. An exit is resolved from its own shape,
        // and the walk in front of it simply finds nowhere to go and
        // does not block, so the room change still happens.
        let mut room = room();
        room.walk = Default::default();
        let mut game = game();
        game.enter(&room, None);

        let click = game.click(&room, Vec2::new(300.0, 100.0), Verb::Walk);
        assert!(matches!(click, Click::Exit { .. }), "{click:?}");

        let mut change = None;
        for _ in 0..600 {
            for event in game.step(&room, 0.016) {
                if let Event::EnterRoom { room, .. } = event {
                    change = Some(room);
                }
            }
            if change.is_some() {
                break;
            }
        }
        assert_eq!(change.as_deref(), Some("courtyard"));
    }

    #[test]
    fn a_disabled_hotspot_is_not_what_a_click_resolves_to() {
        let room = room();
        let mut game = game();
        game.enter(&room, None);
        game.state.set_hotspot("painting", false);
        assert!(matches!(
            game.resolve(&room, Vec2::new(130.0, 50.0), Verb::Look),
            Click::Walk { .. }
        ));
    }
}
