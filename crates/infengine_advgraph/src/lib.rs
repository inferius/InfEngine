//! A25.1: adventure scripts as blueprint graphs.
//!
//! This is the piece that makes the import claim true. Kyrandia's EMC
//! becomes [`Action`]s, and [`Action`]s become a graph you can open,
//! rewire and save - so what comes out the other end is our script in
//! our editor, not a foreign bytecode kept alive by an interpreter
//! bolted to the side.
//!
//! It is a bridge crate on purpose. `infengine_adventure` should not
//! have to depend on a visual editor to describe a game, and
//! `infengine_blueprint` should not know what a hotspot is.
//!
//! The whole thing rests on one property, which the tests state
//! directly: converting a script to a graph and back gives the same
//! script. Anything else means opening a script in the editor quietly
//! damages it.

#![forbid(unsafe_code)]

use std::collections::HashSet;

use glam::Vec2;
use infengine_adventure::action::{Action, Condition, Script};
use infengine_adventure::interact::Facing;
use infengine_blueprint::{Graph, NodeId, NodeKindDef, NodeKindRegistry, PinSpec, PinType};

/// Node the script starts at.
pub const ENTRY: &str = "adv_entry";

/// Horizontal step per level of branch nesting, in graph units.
const COLUMN: f32 = 260.0;
/// Vertical step between consecutive nodes.
const ROW: f32 = 110.0;

const EXEC_IN: &[PinSpec] = &[PinSpec::new("exec", PinType::Exec)];
const EXEC_OUT: &[PinSpec] = &[PinSpec::new("exec", PinType::Exec)];
const BRANCH_OUT: &[PinSpec] = &[
    PinSpec::new("then", PinType::Exec),
    PinSpec::new("else", PinType::Exec),
];

/// Every node kind an adventure script can be built from.
///
/// One per [`Action`] variant plus the entry point, so a graph cannot
/// express something the runtime cannot execute. That is deliberate:
/// a visual language whose pictures do not all mean something is a
/// language that lies.
pub const ADVENTURE_NODES: &[NodeKindDef] = &[
    NodeKindDef {
        id: ENTRY,
        label: "Script Start",
        category: "Adventure",
        inputs: &[],
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_say",
        label: "Say",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_walk",
        label: "Walk To",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_face",
        label: "Face",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_open_screen",
        label: "Show Screen",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_goto_room",
        label: "Go To Room",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_move_camera",
        label: "Move Camera",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_play_anim",
        label: "Play Animation",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_show_image",
        label: "Show Image",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_play_sound",
        label: "Play Sound",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_play_music",
        label: "Play Music",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_wait",
        label: "Wait",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_set_flag",
        label: "Set Flag",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_set_var",
        label: "Set Counter",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_give",
        label: "Give Item",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_take",
        label: "Take Item",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_open_floor",
        label: "Open/Block Floor",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_place_item",
        label: "Place Item",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_pick_up",
        label: "Pick Up Item",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_set_hand",
        label: "Hold Item",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_enable_hotspot",
        label: "Enable Hotspot",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_branch",
        label: "If",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: BRANCH_OUT,
        contexts: &[],
    },
    NodeKindDef {
        id: "adv_unknown",
        label: "Untranslated",
        category: "Adventure",
        inputs: EXEC_IN,
        outputs: EXEC_OUT,
        contexts: &[],
    },
];

/// A registry holding the engine's built-in nodes and the adventure
/// ones, which is what the editor's canvas needs.
pub fn registry() -> NodeKindRegistry {
    let mut registry = NodeKindRegistry::with_builtins();
    for def in ADVENTURE_NODES {
        registry.register(def.clone());
    }
    registry
}

/// Lay a script out as a graph.
pub fn to_graph(script: &Script) -> Graph {
    let registry = registry();
    let mut graph = Graph::default();
    let entry = graph.add_node(ENTRY, [0.0, 0.0]);
    let mut row = 1.0;
    chain(&mut graph, &registry, &script.actions, entry, "exec", 0.0, &mut row);
    graph
}

/// Add a run of actions, wiring the first to `(from_node, from_pin)`.
fn chain(
    graph: &mut Graph,
    registry: &NodeKindRegistry,
    actions: &[Action],
    from_node: NodeId,
    from_pin: &str,
    column: f32,
    row: &mut f32,
) {
    let mut previous = (from_node, from_pin.to_string());
    for action in actions {
        let id = graph.add_node(kind_of(action), [column * COLUMN, *row * ROW]);
        *row += 1.0;
        write_params(graph, id, action);
        let _ = graph.connect(registry, previous.0, &previous.1, id, "exec");

        if let Action::Branch { then, otherwise, .. } = action {
            // Each side gets its own column so the two runs do not sit
            // on top of each other, which is the whole reason to look
            // at a branch as a picture rather than as a list.
            chain(graph, registry, then, id, "then", column + 1.0, row);
            chain(graph, registry, otherwise, id, "else", column + 1.0, row);
            // A branch is the end of its own run: whatever follows it
            // in the script belongs to neither side.
        }
        previous = (id, "exec".to_string());
    }
}

/// Read a graph back as actions, starting from its entry node.
///
/// A graph with no entry node yields nothing rather than guessing which
/// node was meant to be first - guessing would silently reorder a
/// script.
pub fn from_graph(graph: &Graph) -> Vec<Action> {
    let Some(entry) = graph.nodes.iter().find(|node| node.kind == ENTRY) else {
        return Vec::new();
    };
    let mut visited = HashSet::new();
    follow(graph, entry.id, "exec", &mut visited)
}

fn follow(
    graph: &Graph,
    from_node: NodeId,
    from_pin: &str,
    visited: &mut HashSet<u32>,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut cursor = Some((from_node, from_pin.to_string()));

    while let Some((node_id, pin)) = cursor.take() {
        let Some(edge) = graph
            .edges
            .iter()
            .find(|edge| edge.from_node == node_id && edge.from_pin == pin)
        else {
            break;
        };
        // The Action IR has no loops, so a cycle can only come from a
        // graph edited by hand. Stopping is better than hanging.
        if !visited.insert(edge.to_node.0) {
            break;
        }
        let Some(node) = graph.nodes.iter().find(|n| n.id == edge.to_node) else {
            break;
        };

        let action = match node.kind.as_str() {
            "adv_branch" => Action::Branch {
                on: condition_of(node),
                then: follow(graph, node.id, "then", visited),
                otherwise: follow(graph, node.id, "else", visited),
            },
            _ => match read_action(node) {
                Some(action) => action,
                // A node kind from outside this catalogue is not
                // something the runtime can run. Reported as untranslated
                // rather than dropped, for the same reason an unknown
                // opcode is.
                None => Action::Unknown {
                    call: node.kind.clone(),
                    args: Vec::new(),
                },
            },
        };
        actions.push(action);
        cursor = Some((node.id, "exec".to_string()));
    }
    actions
}

fn kind_of(action: &Action) -> &'static str {
    match action {
        Action::Say { .. } => "adv_say",
        Action::Walk { .. } => "adv_walk",
        Action::Face { .. } => "adv_face",
        Action::OpenScreen { .. } => "adv_open_screen",
        Action::GoToRoom { .. } => "adv_goto_room",
        Action::PlayAnim { .. } => "adv_play_anim",
        Action::MoveCamera { .. } => "adv_move_camera",
        Action::ShowImage { .. } => "adv_show_image",
        Action::PlaySound { .. } => "adv_play_sound",
        Action::PlayMusic { .. } => "adv_play_music",
        Action::Wait { .. } => "adv_wait",
        Action::SetFlag { .. } => "adv_set_flag",
        Action::SetVar { .. } => "adv_set_var",
        Action::Give { .. } => "adv_give",
        Action::Take { .. } => "adv_take",
        Action::SetHand { .. } => "adv_set_hand",
        Action::PlaceItem { .. } => "adv_place_item",
        Action::OpenFloor { .. } => "adv_open_floor",
        Action::PickUp { .. } => "adv_pick_up",
        Action::EnableHotspot { .. } => "adv_enable_hotspot",
        Action::Branch { .. } => "adv_branch",
        Action::Unknown { .. } => "adv_unknown",
    }
}

/// Write an action's fields into the node's parameter slots.
///
/// Optional fields are written only when they are `Some`, so absent and
/// empty stay different things - a `GoToRoom` with no entry point is not
/// the same as one aimed at an entry point called "".
fn write_params(graph: &mut Graph, id: NodeId, action: &Action) {
    let Some(node) = graph.nodes.iter_mut().find(|node| node.id == id) else {
        return;
    };
    let mut set = |key: &str, value: String| {
        node.params.insert(key.to_string(), value);
    };

    match action {
        Action::Say { line, speaker } => {
            set("line", line.clone());
            if let Some(speaker) = speaker {
                set("speaker", speaker.clone());
            }
        }
        Action::Walk { to } => {
            set("x", to.x.to_string());
            set("y", to.y.to_string());
        }
        Action::Face { facing } => set("facing", format!("{facing:?}")),
        Action::OpenScreen { screen } => set("screen", screen.clone()),
        Action::GoToRoom { room, entry } => {
            set("room", room.clone());
            if let Some(entry) = entry {
                set("entry", entry.clone());
            }
        }
        Action::PlayAnim { animation, wait, at } => {
            set("animation", animation.clone());
            set("wait", wait.to_string());
            if let Some(at) = at {
                set("x", at.x.to_string());
                set("y", at.y.to_string());
            }
        }
        Action::MoveCamera { to, seconds } => {
            set("x", to.x.to_string());
            set("y", to.y.to_string());
            set("seconds", seconds.to_string());
        }
        Action::ShowImage { image, seconds } => {
            set("image", image.clone());
            set("seconds", seconds.to_string());
        }
        Action::PlaySound { sound } => set("sound", sound.clone()),
        Action::PlayMusic { music } => {
            if let Some(music) = music {
                set("music", music.clone());
            }
        }
        Action::Wait { seconds } => set("seconds", seconds.to_string()),
        Action::SetFlag { flag, value } => {
            set("flag", flag.clone());
            set("value", value.to_string());
        }
        Action::SetVar { var, value } => {
            set("var", var.clone());
            set("value", value.to_string());
        }
        Action::Give { item } | Action::Take { item } => set("item", item.clone()),
        Action::PlaceItem { item, at } => {
            set("item", item.clone());
            set("x", at.x.to_string());
            set("y", at.y.to_string());
        }
        Action::OpenFloor { min, max, open } => {
            set("x1", min.x.to_string());
            set("y1", min.y.to_string());
            set("x2", max.x.to_string());
            set("y2", max.y.to_string());
            set("open", open.to_string());
        }
        Action::PickUp { item } => set("item", item.clone()),
        // An empty hand is the absence of the parameter, the same way
        // every other optional field works here.
        Action::SetHand { item } => {
            if let Some(item) = item {
                set("item", item.clone());
            }
        }
        Action::EnableHotspot { hotspot, enabled } => {
            set("hotspot", hotspot.clone());
            set("enabled", enabled.to_string());
        }
        // A branch keeps its question in the same two parameters an
        // unresolved step uses, so the graph has one way of saying "the
        // import could not read this" rather than two.
        Action::Branch { on, .. } => match on {
            Condition::Flag(flag) => set("flag", flag.clone()),
            Condition::Var { var, op, value } => {
                set("var", var.clone());
                set("op", op.sign().to_string());
                set("value", value.to_string());
            }
            // The marker parameter keeps "holding nothing" distinct
            // from "no condition written yet".
            Condition::Holding(Some(item)) => set("held", item.clone()),
            Condition::Holding(None) => set("empty_hand", "true".into()),
            Condition::ClickPastActor => set("click_past", "true".into()),
            Condition::Unresolved { call, args } => {
                set("call", call.clone());
                if !args.is_empty() {
                    set("args", join_args(args));
                }
            }
        },
        Action::Unknown { call, args } => {
            set("call", call.clone());
            if !args.is_empty() {
                set("args", join_args(args));
            }
        }
    }
}

/// Rebuild an action from a node's parameters.
fn read_action(node: &infengine_blueprint::Node) -> Option<Action> {
    let action = match node.kind.as_str() {
        "adv_say" => Action::Say {
            line: param(node, "line"),
            speaker: optional(node, "speaker"),
        },
        "adv_walk" => Action::Walk {
            to: Vec2::new(number(node, "x"), number(node, "y")),
        },
        "adv_face" => Action::Face {
            facing: facing(&param(node, "facing")),
        },
        "adv_open_screen" => Action::OpenScreen {
            screen: param(node, "screen"),
        },
        "adv_goto_room" => Action::GoToRoom {
            room: param(node, "room"),
            entry: optional(node, "entry"),
        },
        "adv_play_anim" => Action::PlayAnim {
            animation: param(node, "animation"),
            wait: flag(node, "wait"),
            // A node with no position left it to the animation, which is
            // not the same as putting it at the room's origin.
            at: (node.params.contains_key("x") || node.params.contains_key("y"))
                .then(|| Vec2::new(number(node, "x"), number(node, "y"))),
        },
        "adv_move_camera" => Action::MoveCamera {
            to: Vec2::new(number(node, "x"), number(node, "y")),
            seconds: number(node, "seconds"),
        },
        "adv_show_image" => Action::ShowImage {
            image: param(node, "image"),
            seconds: number(node, "seconds"),
        },
        "adv_play_sound" => Action::PlaySound {
            sound: param(node, "sound"),
        },
        "adv_play_music" => Action::PlayMusic {
            music: optional(node, "music"),
        },
        "adv_wait" => Action::Wait {
            seconds: number(node, "seconds"),
        },
        "adv_set_flag" => Action::SetFlag {
            flag: param(node, "flag"),
            value: flag(node, "value"),
        },
        "adv_set_var" => Action::SetVar {
            var: param(node, "var"),
            value: param(node, "value").parse().unwrap_or(0),
        },
        "adv_give" => Action::Give {
            item: param(node, "item"),
        },
        "adv_open_floor" => Action::OpenFloor {
            min: glam::Vec2::new(number(node, "x1"), number(node, "y1")),
            max: glam::Vec2::new(number(node, "x2"), number(node, "y2")),
            open: param(node, "open") == "true",
        },
        "adv_place_item" => Action::PlaceItem {
            item: param(node, "item"),
            at: glam::Vec2::new(number(node, "x"), number(node, "y")),
        },
        "adv_pick_up" => Action::PickUp {
            item: param(node, "item"),
        },
        "adv_set_hand" => Action::SetHand {
            item: optional(node, "item"),
        },
        "adv_take" => Action::Take {
            item: param(node, "item"),
        },
        "adv_enable_hotspot" => Action::EnableHotspot {
            hotspot: param(node, "hotspot"),
            enabled: flag(node, "enabled"),
        },
        "adv_unknown" => Action::Unknown {
            call: param(node, "call"),
            args: split_args(&param(node, "args")),
        },
        _ => return None,
    };
    Some(action)
}

fn param(node: &infengine_blueprint::Node, key: &str) -> String {
    node.params.get(key).cloned().unwrap_or_default()
}

/// Arguments as one parameter value.
fn join_args(args: &[i32]) -> String {
    args.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

/// Arguments read back out of one.
///
/// Anything that is not a number is dropped rather than defaulted to
/// zero: a zero that was really a typo is worse than an argument that is
/// visibly missing.
fn split_args(value: &str) -> Vec<i32> {
    value
        .split(',')
        .filter_map(|arg| arg.trim().parse().ok())
        .collect()
}

/// The question a branch node asks.
///
/// A node with a call parameter carries a question the import could not
/// phrase; otherwise it tests a flag. A branch nobody has filled in yet
/// has neither, and an empty flag is the friendlier of the two readings.
fn condition_of(node: &infengine_blueprint::Node) -> Condition {
    if let Some(held) = optional(node, "held") {
        return Condition::Holding(Some(held));
    }
    if node.params.contains_key("empty_hand") {
        return Condition::Holding(None);
    }
    if node.params.contains_key("click_past") {
        return Condition::ClickPastActor;
    }
    let call = param(node, "call");
    if call.is_empty() {
        let var = param(node, "var");
        if !var.is_empty() {
            let op = match param(node, "op").as_str() {
                "!=" => infengine_adventure::action::Compare::Ne,
                ">" => infengine_adventure::action::Compare::Gt,
                ">=" => infengine_adventure::action::Compare::Ge,
                "<" => infengine_adventure::action::Compare::Lt,
                "<=" => infengine_adventure::action::Compare::Le,
                _ => infengine_adventure::action::Compare::Eq,
            };
            return Condition::Var {
                var,
                op,
                value: param(node, "value").parse().unwrap_or(0),
            };
        }
        Condition::Flag(param(node, "flag"))
    } else {
        Condition::Unresolved {
            call,
            args: split_args(&param(node, "args")),
        }
    }
}

fn optional(node: &infengine_blueprint::Node, key: &str) -> Option<String> {
    node.params.get(key).cloned()
}

fn number(node: &infengine_blueprint::Node, key: &str) -> f32 {
    node.params
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0)
}

fn flag(node: &infengine_blueprint::Node, key: &str) -> bool {
    node.params.get(key).map(String::as_str) == Some("true")
}

fn facing(name: &str) -> Facing {
    match name {
        "Left" => Facing::Left,
        "Right" => Facing::Right,
        "Back" => Facing::Back,
        _ => Facing::Front,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Script {
        let mut script = Script::new("hall.door.use");
        script.push(Action::Say {
            line: "hall.00".into(),
            speaker: Some("Brandon".into()),
        });
        script.push(Action::Walk {
            to: Vec2::new(120.0, 150.0),
        });
        script.push(Action::Face {
            facing: Facing::Back,
        });
        script.push(Action::PlayAnim {
            animation: "door_opens".into(),
            wait: true,
            at: None,
        });
        script.push(Action::PlaySound {
            sound: "door_creak".into(),
        });
        script.push(Action::ShowImage {
            image: "pictures/note.png".into(),
            seconds: 3.0,
        });
        script.push(Action::PlayMusic { music: None });
        script.push(Action::Wait { seconds: 1.5 });
        script.push(Action::Give { item: "key".into() });
        script.push(Action::Take { item: "coin".into() });
        script.push(Action::EnableHotspot {
            hotspot: "brick".into(),
            enabled: false,
        });
        script.push(Action::Unknown {
            call: "kyra:40".into(),
            args: vec![72, 192, 136],
        });
        script.push(Action::GoToRoom {
            room: "crypt".into(),
            entry: Some("stairs".into()),
        });
        script
    }

    #[test]
    fn a_script_survives_the_round_trip() {
        let script = sample();
        let graph = to_graph(&script);
        assert_eq!(from_graph(&graph), script.actions);
    }

    #[test]
    fn a_question_the_import_could_not_phrase_survives_too() {
        // An imported game is mostly branches on things we cannot yet
        // express. If opening one in the editor turned its condition
        // into an empty flag, saving would quietly rewrite the logic of
        // every room to take the same side every time.
        let mut script = Script::new("altar.look");
        script.push(Action::Branch {
            on: Condition::Unresolved {
                call: "kyra:getItemInMarbleVase".into(),
                args: vec![0, 4],
            },
            then: vec![Action::Say {
                line: "altar.02".into(),
                speaker: None,
            }],
            otherwise: vec![Action::Face {
                facing: Facing::Left,
            }],
        });

        let back = from_graph(&to_graph(&script));
        assert_eq!(back, script.actions);
    }

    #[test]
    fn a_branch_with_no_question_at_all_reads_back_as_an_empty_flag() {
        // A node the author dropped on the canvas and has not filled in.
        // Two empty readings are possible and the friendlier one wins.
        let mut script = Script::new("x");
        script.push(Action::Branch {
            on: Condition::Flag(String::new()),
            then: Vec::new(),
            otherwise: Vec::new(),
        });
        assert_eq!(from_graph(&to_graph(&script)), script.actions);
    }

    #[test]
    fn a_branch_survives_with_both_sides_in_order() {
        let mut script = Script::new("hall.chest.use");
        script.push(Action::Branch {
            on: Condition::Flag("has_key".into()),
            then: vec![
                Action::Say {
                    line: "hall.01".into(),
                    speaker: None,
                },
                Action::SetFlag {
                    flag: "opened".into(),
                    value: true,
                },
            ],
            otherwise: vec![Action::Say {
                line: "hall.02".into(),
                speaker: None,
            }],
        });

        let graph = to_graph(&script);
        assert_eq!(from_graph(&graph), script.actions);
    }

    #[test]
    fn nested_branches_survive_too() {
        let mut script = Script::new("deep");
        script.push(Action::Branch {
            on: Condition::Flag("outer".into()),
            then: vec![Action::Branch {
                on: Condition::Flag("inner".into()),
                then: vec![Action::Give { item: "a".into() }],
                otherwise: vec![Action::Give { item: "b".into() }],
            }],
            otherwise: vec![],
        });
        let graph = to_graph(&script);
        assert_eq!(from_graph(&graph), script.actions);
    }

    #[test]
    fn absent_and_empty_stay_different() {
        // A room change with no entry point is not the same as one
        // aimed at an entry point named "", and a round trip that
        // confuses them silently changes where the player lands.
        let mut script = Script::new("optionals");
        script.push(Action::GoToRoom {
            room: "crypt".into(),
            entry: None,
        });
        script.push(Action::GoToRoom {
            room: "crypt".into(),
            entry: Some(String::new()),
        });
        script.push(Action::PlayMusic { music: None });
        script.push(Action::PlayMusic {
            music: Some(String::new()),
        });

        let graph = to_graph(&script);
        assert_eq!(from_graph(&graph), script.actions);
    }

    #[test]
    fn every_action_variant_has_a_node_kind() {
        let registry = registry();
        for action in [
            Action::Say {
                line: String::new(),
                speaker: None,
            },
            Action::Walk { to: Vec2::ZERO },
            Action::Face {
                facing: Facing::Front,
            },
            Action::OpenScreen {
                screen: String::new(),
            },
            Action::GoToRoom {
                room: String::new(),
                entry: None,
            },
            Action::PlayAnim {
                animation: String::new(),
                wait: false,
                at: None,
            },
            Action::PlaySound {
                sound: String::new(),
            },
            Action::PlayMusic { music: None },
            Action::Wait { seconds: 0.0 },
            Action::SetFlag {
                flag: String::new(),
                value: false,
            },
            Action::Give {
                item: String::new(),
            },
            Action::Take {
                item: String::new(),
            },
            Action::EnableHotspot {
                hotspot: String::new(),
                enabled: false,
            },
            Action::Branch {
                on: Condition::Flag(String::new()),
                then: Vec::new(),
                otherwise: Vec::new(),
            },
            Action::Unknown {
                call: String::new(),
                args: Vec::new(),
            },
        ] {
            let kind = kind_of(&action);
            assert!(
                registry.get(kind).is_some(),
                "{kind} is produced but not registered"
            );
        }
    }

    #[test]
    fn a_graph_with_no_entry_node_yields_nothing() {
        let mut graph = Graph::default();
        graph.add_node("adv_say", [0.0, 0.0]);
        assert!(
            from_graph(&graph).is_empty(),
            "guessing which node came first would silently reorder a script"
        );
    }

    #[test]
    fn a_cycle_stops_instead_of_hanging() {
        let script = sample();
        let mut graph = to_graph(&script);
        // Wire the last node back to the second, which no compiler
        // would produce but a hand edit can.
        let last = graph.nodes.last().expect("nodes").id;
        let second = graph.nodes[1].id;
        graph.edges.push(infengine_blueprint::Edge {
            from_node: last,
            from_pin: "exec".into(),
            to_node: second,
            to_pin: "exec".into(),
        });
        let actions = from_graph(&graph);
        assert_eq!(actions.len(), script.actions.len());
    }

    #[test]
    fn a_node_kind_the_runtime_does_not_know_becomes_untranslated() {
        let mut graph = Graph::default();
        let entry = graph.add_node(ENTRY, [0.0, 0.0]);
        let odd = graph.add_node("const_float", [0.0, 100.0]);
        graph.edges.push(infengine_blueprint::Edge {
            from_node: entry,
            from_pin: "exec".into(),
            to_node: odd,
            to_pin: "exec".into(),
        });
        assert_eq!(
            from_graph(&graph),
            vec![Action::Unknown {
                call: "const_float".into(),
                args: Vec::new()
            }],
            "dropping it would leave a script that looks complete"
        );
    }

    #[test]
    fn the_layout_puts_the_two_sides_of_a_branch_in_different_columns() {
        let mut script = Script::new("branchy");
        script.push(Action::Branch {
            on: Condition::Flag("f".into()),
            then: vec![Action::Give { item: "a".into() }],
            otherwise: vec![Action::Give { item: "b".into() }],
        });
        let graph = to_graph(&script);
        let branch = graph
            .nodes
            .iter()
            .find(|node| node.kind == "adv_branch")
            .expect("a branch");
        for node in graph.nodes.iter().filter(|node| node.kind == "adv_give") {
            assert!(
                node.position[0] > branch.position[0],
                "a branch's arms belong beside it, not on top of it"
            );
        }
    }
}
