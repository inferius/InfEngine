//! A16.3: turning a room's bytecode into our own actions.
//!
//! The point of this module is that the imported logic stops being
//! Kyrandia's and becomes ours: a list of
//! [`Action`](infengine_adventure::Action)s the editor can show, the
//! blueprint graph can draw, and a person can change. Nothing here
//! executes anything.
//!
//! What that costs in honesty is [`Action::Unknown`]. Of the engine's
//! function table only a handful of entries are identified, so most
//! calls come through named but untranslated. That is the correct
//! outcome: a room whose script quietly lost half its steps would look
//! finished, and the editor can count what is still unresolved.
//!
//! The function numbers below are properties of Kyrandia's data, taken
//! from the format's public documentation. The reading of them is our
//! own.

use infengine_adventure::{Action, Script};

use super::emc;
use super::vm::{self, CallSite};

/// Speaks a line of dialogue.
const FN_CHARACTER_SAYS: i32 = 1;
/// Prints a line without a speaker.
const FN_PRINT_TEXT: i32 = 60;
/// Prints a line in the talk style.
const FN_PRINT_TALK: i32 = 52;
/// Moves the actor to another scene.
const FN_ENTER_SCENE: i32 = 45;
/// Sets a story flag.
const FN_SET_FLAG: i32 = 5;
/// Walks the player to a point in the room.
const FN_WALK_TO: i32 = 11;
/// Plays a sound effect.
const FN_PLAY_SOUND: i32 = 23;
/// Starts one of the room's animations.
const FN_ANIM_ON: i32 = 24;
/// Stops one.
const FN_ANIM_OFF: i32 = 42;

/// Calls that describe how the original renderer worked rather than
/// what a room does.
///
/// Page flips, cursor visibility, background preservation, facing
/// queries: all of it is 1992 plumbing our engine has no equivalent
/// for. Carrying them through as `Unknown` would bury the handful of
/// genuinely unidentified calls under thousands of irrelevant ones, so
/// they are dropped on purpose and counted separately - discarded is
/// not the same claim as unrecognised.
const PLUMBING: &[i32] = &[
    3,  // draw a scene shape directly to the screen
    4,  // read a game flag, a query rather than a step
    33, // show or hide the mouse cursor
    53, // a hard-coded special-case display
    56, // preserve object backgrounds before redrawing
    58, // read a character's facing
    64, // flip the drawing page
];

/// How many calls a single script may hold before the decompiler stops.
///
/// A room's entry point is tens of instructions; a runaway here would
/// mean the code was misread rather than that a room is enormous.
const MAX_CALLS: usize = 4096;

/// One room's scripts, decompiled.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoomScripts {
    pub scripts: Vec<Script>,
    /// Calls dropped as engine plumbing rather than kept as unknown.
    pub plumbing_dropped: usize,
    /// Scene numbers this room's script can move to. Kyrandia numbers
    /// its scenes; turning those into room ids needs a table that lives
    /// in the game executable rather than in the data, so they are kept
    /// as numbers for the editor to resolve.
    pub scene_targets: Vec<i32>,
}

impl RoomScripts {
    /// Steps that came through untranslated, across every script.
    pub fn unknown_count(&self) -> usize {
        self.scripts.iter().map(Script::unknown_count).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.scripts.iter().all(Script::is_empty)
    }
}

/// Translate one call into an action, or into an honest placeholder.
///
/// Returns `None` for a call that describes the original renderer
/// rather than the room.
fn action_for(room: &str, site: &CallSite, texts: usize) -> Option<Action> {
    if PLUMBING.contains(&site.function) {
        return None;
    }
    Some(match site.function {
        FN_CHARACTER_SAYS | FN_PRINT_TEXT | FN_PRINT_TALK => {
            // The line index is the last constant pushed, which is the
            // call's first argument. A call whose index was computed
            // has no constants and cannot be resolved to a line.
            match site.args.first().copied().and_then(|arg| line_index(arg, texts)) {
                Some(index) => Action::Say {
                    line: emc::Script::line_id(room, index),
                    speaker: None,
                },
                None => unknown(site),
            }
        }
        FN_ENTER_SCENE => match site.args.first().copied() {
            // Scene numbers have no names in the data, so the room is
            // recorded as the number it is. Inventing a name here would
            // be a guess that later looks like a fact.
            Some(scene) if scene >= 0 => Action::GoToRoom {
                room: format!("scene{scene}"),
                entry: None,
            },
            _ => unknown(site),
        },
        FN_SET_FLAG => match site.args.first().copied() {
            Some(flag) => Action::SetFlag {
                flag: format!("kyra.flag{flag}"),
                value: true,
            },
            None => unknown(site),
        },
        FN_WALK_TO => match (site.args.first(), site.args.get(1)) {
            (Some(&x), Some(&y)) => Action::Walk {
                to: glam::Vec2::new(x as f32, y as f32),
            },
            _ => unknown(site),
        },
        FN_PLAY_SOUND => match site.args.first().copied() {
            Some(sound) => Action::PlaySound {
                sound: format!("kyra.sfx{sound}"),
            },
            None => unknown(site),
        },
        FN_ANIM_ON | FN_ANIM_OFF => match site.args.first().copied() {
            Some(index) => Action::PlayAnim {
                animation: format!("{room}_{index}"),
                wait: false,
            },
            None => unknown(site),
        },
        _ => unknown(site),
    })
}

/// Which line a dialogue call's argument refers to.
///
/// Usually the index itself. Some scripts add a multiple of a thousand
/// first: `_NPC.EMC` speaks its 114 lines as 9000 upwards, and
/// `FORESTB.EMC` uses 4000. What the thousands select is not known -
/// there is one string table per script, so it is not choosing between
/// tables here - but the low part is verifiably the index: of the 113
/// calls in `_NPC.EMC`, 111 land inside its 114 lines, and read back in
/// order they are consecutive dialogue ("Let's try this.", "Here
/// goes...", "I feel kind of strange..."). The two that do not land
/// stay untranslated rather than being bent to fit.
fn line_index(arg: i32, texts: usize) -> Option<usize> {
    if arg < 0 {
        return None;
    }
    let arg = arg as usize;
    if arg < texts {
        return Some(arg);
    }
    let index = arg % 1000;
    (arg >= 1000 && index < texts).then_some(index)
}

fn unknown(site: &CallSite) -> Action {
    Action::Unknown {
        call: format!("kyra:{}", site.function),
        args: site.args.clone(),
    }
}

/// Decompile a room's script.
///
/// One [`Script`] per entry point the `ORDR` chunk names, so the
/// editor sees the same divisions the game did rather than one flat
/// list.
pub fn decompile(room: &str, script: &emc::Script) -> RoomScripts {
    let instructions = vm::disassemble(&script.code);
    let sites = vm::call_sites(&instructions);
    if sites.len() > MAX_CALLS {
        return RoomScripts::default();
    }

    // Entry points divide the code; a call belongs to the last entry
    // point at or before it.
    //
    // The numbering is the game's, not a running count. Every one of
    // the retail scripts starts its first entry point past offset zero,
    // so counting the divisions would shift every id by one and
    // `entry3` would mean the game's entry 2 - which matters, because
    // the seven entry points are a fixed set of hooks and which is
    // which is the next thing to work out.
    let mut named: Vec<(String, usize)> = script
        .entry_points
        .iter()
        .enumerate()
        .map(|(index, at)| (format!("{room}.entry{index}"), *at as usize))
        .collect();
    named.sort_by_key(|(_, at)| *at);
    named.dedup_by_key(|(_, at)| *at);

    // Whatever sits before the first entry point is reachable only by
    // being jumped into. Named rather than numbered, so it cannot be
    // mistaken for one of the seven.
    let mut starts: Vec<(String, usize)> = Vec::new();
    if named.first().is_none_or(|(_, at)| *at != 0) {
        starts.push((format!("{room}.prologue"), 0));
    }
    starts.extend(named);

    let mut out = RoomScripts::default();
    for (index, (id, start)) in starts.iter().enumerate() {
        let end = starts.get(index + 1).map(|(_, at)| *at).unwrap_or(usize::MAX);
        let mut compiled = Script::new(id.clone());
        for site in sites.iter().filter(|s| s.at >= *start && s.at < end) {
            let Some(action) = action_for(room, site, script.texts.len()) else {
                out.plumbing_dropped += 1;
                continue;
            };
            if let Action::GoToRoom { room, .. } = &action
                && let Some(number) = room.strip_prefix("scene").and_then(|n| n.parse().ok())
                && !out.scene_targets.contains(&number)
            {
                out.scene_targets.push(number);
            }
            compiled.push(action);
        }
        if !compiled.is_empty() {
            out.scripts.push(compiled);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a push of a constant, followed by the stack bookkeeping
    /// the retail scripts put between pushes.
    fn push(value: i8) -> Vec<u8> {
        vec![0x40 | 4, value as u8, 0x40 | 17, 8]
    }

    /// Encode a call to a function.
    fn call(function: u16) -> Vec<u8> {
        let mut v = vec![0x20 | 14, 0];
        v.extend_from_slice(&function.to_be_bytes());
        v
    }

    fn script_with(code: Vec<u8>, texts: usize, entry_points: Vec<u16>) -> emc::Script {
        emc::Script {
            entry_points,
            texts: (0..texts).map(|i| format!("line {i}")).collect(),
            code,
        }
    }

    #[test]
    fn a_dialogue_call_becomes_a_say_referencing_a_line_id() {
        let mut code = push(2);
        code.extend(call(FN_CHARACTER_SAYS as u16));
        let out = decompile("brins", &script_with(code, 8, vec![0]));
        assert_eq!(out.scripts.len(), 1);
        assert_eq!(
            out.scripts[0].actions[0],
            Action::Say {
                line: "brins.02".into(),
                speaker: None
            }
        );
    }

    #[test]
    fn all_three_text_functions_produce_a_say() {
        for function in [FN_CHARACTER_SAYS, FN_PRINT_TEXT, FN_PRINT_TALK] {
            let mut code = push(1);
            code.extend(call(function as u16));
            let out = decompile("hall", &script_with(code, 4, vec![0]));
            assert!(
                matches!(out.scripts[0].actions[0], Action::Say { .. }),
                "function {function} should speak"
            );
        }
    }

    #[test]
    fn entry_points_keep_the_games_own_numbering() {
        // Every retail script starts its first entry past zero, so a
        // running count would call the game's entry 0 "entry1".
        let mut code = push(1);
        code.extend(call(FN_SET_FLAG as u16));
        code.extend(push(2));
        code.extend(call(FN_SET_FLAG as u16));
        let script = script_with(code, 0, vec![6, 0]);
        let out = decompile("hall", &script);
        let ids: Vec<&str> = out.scripts.iter().map(|s| s.id.as_str()).collect();
        assert!(
            ids.contains(&"hall.entry0") || ids.contains(&"hall.entry1"),
            "entries are numbered as the ORDR chunk lists them: {ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id.ends_with(".entry2")),
            "two entry points cannot produce a third id: {ids:?}"
        );
    }

    #[test]
    fn a_thousands_prefix_on_a_line_reference_is_stripped() {
        // _NPC.EMC speaks its lines as 9000 upwards and FORESTB.EMC as
        // 4000; the low part is the index into that script's own text.
        assert_eq!(line_index(9000, 114), Some(0));
        assert_eq!(line_index(9013, 114), Some(13));
        assert_eq!(line_index(4000, 16), Some(0));
        // Past the end even after stripping: left alone.
        assert_eq!(line_index(9114, 114), None);
        assert_eq!(line_index(-1, 114), None);
        // A plain index still works, and is preferred over stripping.
        assert_eq!(line_index(7, 114), Some(7));
    }

    #[test]
    fn a_line_index_outside_the_rooms_dialogue_is_not_invented() {
        // Pointing at a line that does not exist would put a dangling
        // id in the script; better to keep the call unresolved.
        let mut code = push(9);
        code.extend(call(FN_CHARACTER_SAYS as u16));
        let out = decompile("brins", &script_with(code, 4, vec![0]));
        assert!(out.scripts[0].actions[0].is_unknown());
    }

    #[test]
    fn a_scene_change_becomes_a_room_move_and_is_collected() {
        let mut code = Vec::new();
        for value in [1i8, 68, 11, 19] {
            code.extend(push(value));
        }
        code.extend(call(FN_ENTER_SCENE as u16));
        let out = decompile("balcony", &script_with(code, 0, vec![0]));
        assert_eq!(
            out.scripts[0].actions[0],
            Action::GoToRoom {
                room: "scene19".into(),
                entry: None
            }
        );
        assert_eq!(out.scene_targets, vec![19]);
    }

    #[test]
    fn a_scene_number_is_kept_as_a_number_rather_than_named() {
        // The number-to-name table is not in the data. Making a name up
        // here would later read as a fact.
        let mut code = push(7);
        code.extend(call(FN_ENTER_SCENE as u16));
        let out = decompile("x", &script_with(code, 0, vec![0]));
        assert_eq!(out.scripts[0].actions[0].label(), "go to scene7");
    }

    #[test]
    fn engine_plumbing_is_dropped_rather_than_called_unknown() {
        // Thousands of page flips would bury the handful of calls
        // nobody has worked out yet.
        let mut code = push(1);
        code.extend(call(64)); // pageFlip
        code.extend(push(1));
        code.extend(call(33)); // showMouse
        let out = decompile("x", &script_with(code, 4, vec![0]));
        assert!(out.is_empty(), "nothing here describes the room");
        assert_eq!(out.plumbing_dropped, 2);
        assert_eq!(out.unknown_count(), 0);
    }

    /// One expectation: a function, the constants it is given, and a
    /// predicate the resulting action must satisfy.
    struct Case {
        function: i32,
        args: Vec<i8>,
        check: fn(&Action) -> bool,
    }

    #[test]
    fn the_calls_that_do_describe_the_room_are_translated() {
        let cases = vec![
            Case {
                function: FN_SET_FLAG,
                args: vec![7],
                check: |a| matches!(a, Action::SetFlag { .. }),
            },
            Case {
                function: FN_WALK_TO,
                args: vec![90, 40],
                check: |a| matches!(a, Action::Walk { .. }),
            },
            Case {
                function: FN_PLAY_SOUND,
                args: vec![12],
                check: |a| matches!(a, Action::PlaySound { .. }),
            },
            Case {
                function: FN_ANIM_ON,
                args: vec![2],
                check: |a| matches!(a, Action::PlayAnim { .. }),
            },
            Case {
                function: FN_ANIM_OFF,
                args: vec![2],
                check: |a| matches!(a, Action::PlayAnim { .. }),
            },
        ];
        for Case {
            function,
            args,
            check,
        } in cases
        {
            let mut code = Vec::new();
            // Pushed in reverse so the first argument ends up first.
            for value in args.iter().rev() {
                code.extend(push(*value));
            }
            code.extend(call(function as u16));
            let out = decompile("hall", &script_with(code, 4, vec![0]));
            assert!(
                check(&out.scripts[0].actions[0]),
                "function {function} translated to {:?}",
                out.scripts[0].actions[0]
            );
        }
    }

    #[test]
    fn a_walk_keeps_its_coordinates() {
        let mut code = push(40);
        code.extend(push(90));
        code.extend(call(FN_WALK_TO as u16));
        let out = decompile("hall", &script_with(code, 0, vec![0]));
        assert_eq!(
            out.scripts[0].actions[0],
            Action::Walk {
                to: glam::Vec2::new(90.0, 40.0)
            }
        );
    }

    #[test]
    fn an_unidentified_call_is_kept_rather_than_dropped() {
        // The whole reason Unknown exists.
        let mut code = push(3);
        code.extend(call(99));
        let out = decompile("x", &script_with(code, 8, vec![0]));
        assert_eq!(
            out.scripts[0].actions[0],
            Action::Unknown {
                call: "kyra:99".into(),
                args: vec![3]
            }
        );
        assert_eq!(out.unknown_count(), 1);
    }

    #[test]
    fn entry_points_divide_the_code_into_separate_scripts() {
        let mut code = push(0);
        code.extend(call(FN_CHARACTER_SAYS as u16));
        let split = code.len() as u16;
        code.extend(push(1));
        code.extend(call(FN_CHARACTER_SAYS as u16));

        let out = decompile("hall", &script_with(code, 4, vec![0, split]));
        assert_eq!(out.scripts.len(), 2);
        assert_eq!(out.scripts[0].id, "hall.entry0");
        assert_eq!(out.scripts[1].id, "hall.entry1");
        assert_eq!(out.scripts[0].spoken_lines(), vec!["hall.00"]);
        assert_eq!(out.scripts[1].spoken_lines(), vec!["hall.01"]);
    }

    #[test]
    fn code_before_the_first_entry_point_is_not_lost() {
        let mut code = push(0);
        code.extend(call(FN_CHARACTER_SAYS as u16));
        let out = decompile("hall", &script_with(code.clone(), 4, vec![900]));
        assert_eq!(
            out.scripts.len(),
            1,
            "an entry point past the code must not hide what came before it"
        );
        assert_eq!(out.scripts[0].spoken_lines(), vec!["hall.00"]);
    }

    #[test]
    fn a_script_with_no_calls_produces_nothing() {
        let out = decompile("empty", &script_with(Vec::new(), 0, vec![0]));
        assert!(out.is_empty());
        assert!(out.scripts.is_empty());
        assert_eq!(out.unknown_count(), 0);
    }

    #[test]
    fn duplicate_scene_targets_are_collected_once() {
        let mut code = Vec::new();
        for _ in 0..2 {
            code.extend(push(19));
            code.extend(call(FN_ENTER_SCENE as u16));
        }
        let out = decompile("x", &script_with(code, 0, vec![0]));
        assert_eq!(out.scene_targets, vec![19]);
    }
}
