//! A16.1: what a script does, as data.
//!
//! Everything that makes an adventure game happen - a line of dialogue,
//! a walk, a door opening - reduces to a short list of actions. Keeping
//! that list as *data* rather than as code is what lets three different
//! front ends produce the same thing:
//!
//! ```text
//!   inspector (verb x object)  ─┐
//!   blueprint graph            ─┼─>  Vec<Action>  ─>  runtime
//!   imported game script       ─┘
//! ```
//!
//! The third one is the reason this exists in the engine rather than in
//! the importer. A converted Kyrandia room should become *our* actions,
//! editable in the graph like anything else - not a foreign bytecode
//! kept alive by an interpreter bolted to the side of the engine.
//!
//! [`Action::Unknown`] carries that honestly. An imported call nobody
//! has identified yet becomes a visible node saying so, rather than
//! being dropped: a room whose logic silently lost three steps looks
//! finished and is not.
//!
//! Lines are referenced by their localisation id, never by their text,
//! so translating a game never means touching its logic.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// One step of a script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Speak a line. `line` is a [`crate::loca`] id.
    Say {
        line: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speaker: Option<String>,
    },
    /// Walk the actor to a point in the room.
    Walk { to: Vec2 },
    /// Face a direction without moving.
    Face { facing: crate::interact::Facing },
    /// Show a screen: a menu, a title card, the end of a chapter.
    ///
    /// A script needs this because a cutscene has to hand over to
    /// something when it finishes, and what it hands over to is usually
    /// not another room.
    OpenScreen { screen: String },
    /// Change room.
    GoToRoom {
        room: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry: Option<String>,
    },
    /// Play an animation, optionally blocking until it finishes.
    PlayAnim {
        animation: String,
        #[serde(default)]
        wait: bool,
    },
    /// Play a sound or a recorded line.
    PlaySound { sound: String },
    /// Play or change the music.
    PlayMusic {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        music: Option<String>,
    },
    /// Pause.
    Wait { seconds: f32 },
    /// Set a story flag.
    SetFlag { flag: String, value: bool },
    /// Put an item in the inventory.
    Give { item: String },
    /// Remove one.
    Take { item: String },
    /// Show or hide a hotspot.
    EnableHotspot { hotspot: String, enabled: bool },
    /// Take one of two paths.
    Branch {
        on: Condition,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        then: Vec<Action>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        otherwise: Vec<Action>,
    },
    /// Something an import could not translate.
    ///
    /// Kept deliberately. Dropping it would leave a script that looks
    /// complete while quietly missing a step; as a node it shows up in
    /// the editor as work still to do.
    Unknown {
        /// What the source called it - an opcode number, a function
        /// name, whatever identifies it in the original.
        call: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<i32>,
    },
}

/// What a branch tests.
///
/// A flag is the condition an author writes. The other one exists
/// because an imported game asks questions we cannot yet phrase - is
/// this item in the marble vase, is the cauldron lit - and the shape of
/// the logic is worth keeping even where the question is not readable
/// yet. Dropping the branch instead would flatten an if-else into both
/// halves running one after the other, which is not what the game does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    /// A story flag, true when it has been set.
    Flag(String),
    /// A test the import could not express.
    ///
    /// False at run time, so the branch takes its `otherwise` side, and
    /// counted by the editor as work still to do.
    Unresolved {
        call: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<i32>,
    },
}

impl Condition {
    /// A short label for the editor.
    pub fn label(&self) -> String {
        match self {
            Condition::Flag(flag) => flag.clone(),
            Condition::Unresolved { call, .. } => format!("{call}?"),
        }
    }

    /// Is this a question we can actually answer?
    pub fn is_resolved(&self) -> bool {
        matches!(self, Condition::Flag(_))
    }
}

impl Action {
    /// A short label for the editor's node list.
    pub fn label(&self) -> String {
        match self {
            Action::Say { line, .. } => format!("say {line}"),
            Action::Walk { to } => format!("walk to {:.0},{:.0}", to.x, to.y),
            Action::Face { facing } => format!("face {facing:?}"),
            Action::OpenScreen { screen } => format!("show {screen}"),
            Action::GoToRoom { room, .. } => format!("go to {room}"),
            Action::PlayAnim { animation, .. } => format!("play {animation}"),
            Action::PlaySound { sound } => format!("sound {sound}"),
            Action::PlayMusic { music } => match music {
                Some(m) => format!("music {m}"),
                None => "music off".to_string(),
            },
            Action::Wait { seconds } => format!("wait {seconds}s"),
            Action::SetFlag { flag, value } => format!("set {flag} = {value}"),
            Action::Give { item } => format!("give {item}"),
            Action::Take { item } => format!("take {item}"),
            Action::EnableHotspot { hotspot, enabled } => {
                format!("{} {hotspot}", if *enabled { "enable" } else { "disable" })
            }
            Action::Branch { on, .. } => format!("if {}", on.label()),
            Action::Unknown { call, .. } => format!("unknown {call}"),
        }
    }

    /// Is this a step nobody has translated yet?
    pub fn is_unknown(&self) -> bool {
        matches!(self, Action::Unknown { .. })
    }

    /// Walk this action and everything nested inside it.
    pub fn walk(&self, visit: &mut impl FnMut(&Action)) {
        visit(self);
        if let Action::Branch { then, otherwise, .. } = self {
            for action in then.iter().chain(otherwise) {
                action.walk(visit);
            }
        }
    }
}

/// A named list of actions: what one interaction does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Script {
    /// Stable id, referenced from a hotspot's verb table.
    pub id: String,
    #[serde(default)]
    pub actions: Vec<Action>,
}

impl Script {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            actions: Vec::new(),
        }
    }

    pub fn push(&mut self, action: Action) -> &mut Self {
        self.actions.push(action);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Every action, including those nested in branches.
    pub fn walk(&self, visit: &mut impl FnMut(&Action)) {
        for action in &self.actions {
            action.walk(visit);
        }
    }

    /// How many steps are still untranslated, at any depth.
    ///
    /// The editor shows this so a half-imported script cannot pass for
    /// a finished one.
    pub fn unknown_count(&self) -> usize {
        let mut count = 0;
        self.walk(&mut |action| {
            if action.is_unknown() {
                count += 1;
            }
        });
        count
    }

    /// Localisation ids this script speaks, in order.
    pub fn spoken_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        self.walk(&mut |action| {
            if let Action::Say { line, .. } = action {
                lines.push(line.clone());
            }
        });
        lines
    }

    /// Rooms this script can move the actor to.
    pub fn destinations(&self) -> Vec<String> {
        let mut rooms = Vec::new();
        self.walk(&mut |action| {
            if let Action::GoToRoom { room, .. } = action
                && !rooms.contains(room)
            {
                rooms.push(room.clone());
            }
        });
        rooms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interact::Facing;

    fn sample() -> Script {
        let mut script = Script::new("open_door");
        script
            .push(Action::Walk {
                to: Vec2::new(100.0, 150.0),
            })
            .push(Action::Say {
                line: "hall.03".into(),
                speaker: Some("Brandon".into()),
            })
            .push(Action::Branch {
                on: Condition::Flag("has_key".into()),
                then: vec![
                    Action::PlaySound {
                        sound: "sfx/unlock.wav".into(),
                    },
                    Action::GoToRoom {
                        room: "crypt".into(),
                        entry: Some("stairs".into()),
                    },
                ],
                otherwise: vec![Action::Say {
                    line: "hall.04".into(),
                    speaker: None,
                }],
            });
        script
    }

    #[test]
    fn a_script_round_trips_through_ron() {
        let script = sample();
        let text = ron::ser::to_string_pretty(&script, ron::ser::PrettyConfig::new())
            .expect("serialize");
        let back: Script = ron::from_str(&text).expect("parse");
        assert_eq!(script, back);
    }

    #[test]
    fn walking_reaches_actions_nested_in_branches() {
        let mut seen = 0;
        sample().walk(&mut |_| seen += 1);
        // Three at the top level plus three inside the branch.
        assert_eq!(seen, 6);
    }

    #[test]
    fn spoken_lines_come_out_in_order_and_include_branches() {
        assert_eq!(sample().spoken_lines(), vec!["hall.03", "hall.04"]);
    }

    #[test]
    fn destinations_are_collected_without_duplicates() {
        let mut script = sample();
        script.push(Action::GoToRoom {
            room: "crypt".into(),
            entry: None,
        });
        assert_eq!(script.destinations(), vec!["crypt"]);
    }

    #[test]
    fn untranslated_steps_are_counted_at_any_depth() {
        // The point of keeping Unknown: a script missing three steps
        // must not look finished.
        let mut script = Script::new("half_imported");
        script
            .push(Action::Unknown {
                call: "native 17".into(),
                args: vec![1, 2],
            })
            .push(Action::Branch {
                on: Condition::Flag("f".into()),
                then: vec![Action::Unknown {
                    call: "native 42".into(),
                    args: Vec::new(),
                }],
                otherwise: Vec::new(),
            });
        assert_eq!(script.unknown_count(), 2);
        assert_eq!(sample().unknown_count(), 0);
    }

    #[test]
    fn every_action_has_a_readable_label() {
        let actions = [
            Action::Say {
                line: "a.00".into(),
                speaker: None,
            },
            Action::Walk { to: Vec2::ZERO },
            Action::Face {
                facing: Facing::Left,
            },
            Action::GoToRoom {
                room: "crypt".into(),
                entry: None,
            },
            Action::PlayAnim {
                animation: "river".into(),
                wait: false,
            },
            Action::PlaySound {
                sound: "s.wav".into(),
            },
            Action::PlayMusic { music: None },
            Action::Wait { seconds: 1.5 },
            Action::SetFlag {
                flag: "f".into(),
                value: true,
            },
            Action::Give { item: "key".into() },
            Action::Take { item: "key".into() },
            Action::EnableHotspot {
                hotspot: "door".into(),
                enabled: false,
            },
            Action::Branch {
                on: Condition::Flag("f".into()),
                then: Vec::new(),
                otherwise: Vec::new(),
            },
            Action::Unknown {
                call: "native 9".into(),
                args: Vec::new(),
            },
        ];
        for action in &actions {
            assert!(!action.label().is_empty(), "{action:?} has no label");
        }
        assert_eq!(actions[0].label(), "say a.00");
        assert_eq!(actions[6].label(), "music off");
        assert_eq!(actions[11].label(), "disable door");
    }

    #[test]
    fn only_unknown_reports_itself_as_unknown() {
        assert!(
            Action::Unknown {
                call: "x".into(),
                args: Vec::new()
            }
            .is_unknown()
        );
        assert!(!Action::Wait { seconds: 1.0 }.is_unknown());
    }

    #[test]
    fn an_empty_script_is_empty() {
        assert!(Script::new("nothing").is_empty());
        assert!(!sample().is_empty());
    }

    #[test]
    fn a_line_is_referenced_by_id_not_by_text() {
        // Translating a game must never mean editing its logic, which
        // only holds if the action stores the key.
        let script = sample();
        assert!(
            script
                .spoken_lines()
                .iter()
                .all(|line| line.contains('.') && !line.contains(' ')),
            "lines should be ids, got {:?}",
            script.spoken_lines()
        );
    }
}
