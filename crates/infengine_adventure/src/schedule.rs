//! A59.1: a script laid out in time.
//!
//! A [`Script`] is a sequence, not a timeline: it says what happens
//! next, never when. That is the right model to execute - a line takes
//! as long as its recording, and the interpreter cannot know that - but
//! it is the wrong model to *tune*. "The pan starts after the title" is
//! not something anyone can act on; "the pan starts at 4.2 seconds and
//! runs for 3" is.
//!
//! So this works out when each step begins and how long it lasts, given
//! what the caller knows about animations and reading speed. What comes
//! out is what a timeline draws and what a person drags.
//!
//! Two things it is careful about. A step that blocks on nothing takes
//! no time, and a run of those all share one instant rather than being
//! spread out for looks - a timeline that lies about the clock is worse
//! than none. And a branch is laid out down its `then` side only, with
//! the other side reported separately: two sides of a choice do not both
//! happen, so drawing them end to end would invent a scene twice as long
//! as the one that runs.

use crate::action::{Action, Script};

/// How long an unknown animation is assumed to run.
///
/// The same fallback the host uses, so a timeline and a play-through
/// agree about a scene whose animation index is incomplete.
pub const ANIM_FALLBACK: f32 = 1.0;
/// Shortest a line stays up.
pub const SUBTITLE_MIN: f32 = 1.2;
/// Characters a reader gets through in a second.
pub const CHARS_PER_SECOND: f32 = 14.0;

/// One step, placed on the clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// Where the step sits in the script's own action tree.
    ///
    /// The path through nested branches, so the editor can find the
    /// action a bar stands for and change it.
    pub path: Vec<usize>,
    /// Seconds from the start of the script.
    pub at: f32,
    /// How long it holds the script up. Zero for a step that does not.
    pub seconds: f32,
    /// What to write on the bar.
    pub label: String,
    /// How deep in nested branches it is, for indenting.
    pub depth: usize,
    /// Whether its length is a number in the script rather than a
    /// consequence of something else.
    ///
    /// A wait and a pan say how long they take; a line takes as long as
    /// its text and an animation as long as its frames. Only the first
    /// kind can be dragged, and a timeline that let you drag the others
    /// would be offering an edit it cannot make.
    pub adjustable: bool,
}

/// What a script looks like on a clock.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timeline {
    pub slots: Vec<Slot>,
    /// How long the whole thing takes.
    pub seconds: f32,
    /// Branches whose other side is not drawn, by label.
    ///
    /// Reported rather than hidden: a scene laid out down one side of a
    /// choice is only half the story, and the editor should say so
    /// rather than let someone tune a version the player may not see.
    pub untaken: Vec<String>,
}

/// What the caller knows that the script does not.
#[allow(missing_debug_implementations)]
pub struct Timings<'a> {
    /// How long an animation runs, by id.
    pub animation: &'a dyn Fn(&str) -> Option<f32>,
    /// The text of a line, for how long it stays up.
    pub line: &'a dyn Fn(&str) -> Option<String>,
}

impl Timings<'_> {
    fn anim_seconds(&self, id: &str) -> f32 {
        (self.animation)(id).filter(|s| *s > 0.0).unwrap_or(ANIM_FALLBACK)
    }

    fn say_seconds(&self, id: &str) -> f32 {
        let text = (self.line)(id).unwrap_or_default();
        reading_time(&text)
    }
}

/// How long a line stays on screen.
pub fn reading_time(text: &str) -> f32 {
    SUBTITLE_MIN.max(text.chars().count() as f32 / CHARS_PER_SECOND)
}

/// Lay a script out on a clock.
pub fn timeline(script: &Script, timings: &Timings<'_>) -> Timeline {
    let mut out = Timeline::default();
    let mut at = 0.0f32;
    walk(&script.actions, &mut Vec::new(), 0, &mut at, timings, &mut out);
    out.seconds = at;
    out
}

fn walk(
    actions: &[Action],
    path: &mut Vec<usize>,
    depth: usize,
    at: &mut f32,
    timings: &Timings<'_>,
    out: &mut Timeline,
) {
    for (index, action) in actions.iter().enumerate() {
        path.push(index);
        let (seconds, adjustable) = length(action, timings);
        out.slots.push(Slot {
            path: path.clone(),
            at: *at,
            seconds,
            label: action.label(),
            depth,
            adjustable,
        });
        *at += seconds;

        if let Action::Branch { on, then, otherwise } = action {
            // Down the `then` side, because a scene has to be laid out
            // down one of them and that is the one the condition is
            // written for.
            walk(then, path, depth + 1, at, timings, out);
            if !otherwise.is_empty() {
                out.untaken.push(format!("else of {}", on.label()));
            }
        }
        path.pop();
    }
}

/// How long a step holds the script up, and whether that is its own
/// number.
fn length(action: &Action, timings: &Timings<'_>) -> (f32, bool) {
    match action {
        Action::Wait { seconds } => (seconds.max(0.0), true),
        Action::MoveCamera { seconds, .. } => (seconds.max(0.0), true),
        Action::PlayAnim { animation, wait, .. } => {
            // An animation nobody waits for takes no time on the clock,
            // however long it runs: the script has already moved on.
            (if *wait { timings.anim_seconds(animation) } else { 0.0 }, false)
        }
        Action::Say { line, .. } => (timings.say_seconds(line), false),
        _ => (0.0, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Condition;
    use glam::Vec2;

    type AnimFn = Box<dyn Fn(&str) -> Option<f32>>;
    type LineFn = Box<dyn Fn(&str) -> Option<String>>;

    fn timings<'a>(
        animation: &'a dyn Fn(&str) -> Option<f32>,
        line: &'a dyn Fn(&str) -> Option<String>,
    ) -> Timings<'a> {
        Timings { animation, line }
    }

    fn plain() -> (AnimFn, LineFn) {
        (Box::new(|_| Some(2.0)), Box::new(|_| Some("hello".into())))
    }

    #[test]
    fn steps_are_placed_end_to_end_on_the_clock() {
        let (anim, line) = plain();
        let mut script = Script::new("x");
        script.push(Action::Wait { seconds: 1.0 });
        script.push(Action::PlayAnim {
            animation: "a".into(),
            wait: true,
            at: None,
        });
        script.push(Action::MoveCamera {
            to: Vec2::ZERO,
            seconds: 3.0,
        });

        let out = timeline(&script, &timings(&*anim, &*line));
        assert_eq!(out.slots.len(), 3);
        assert_eq!(out.slots[0].at, 0.0);
        assert_eq!(out.slots[1].at, 1.0, "after the wait");
        assert_eq!(out.slots[2].at, 3.0, "after the animation");
        assert_eq!(out.seconds, 6.0);
    }

    #[test]
    fn a_step_that_blocks_on_nothing_takes_no_time() {
        // A run of them sharing one instant is the truth; spreading
        // them out for looks would be a timeline that lies.
        let (anim, line) = plain();
        let mut script = Script::new("x");
        script.push(Action::SetFlag {
            flag: "a".into(),
            value: true,
        });
        script.push(Action::SetFlag {
            flag: "b".into(),
            value: true,
        });
        script.push(Action::PlaySound { sound: "s".into() });

        let out = timeline(&script, &timings(&*anim, &*line));
        assert!(out.slots.iter().all(|slot| slot.at == 0.0));
        assert_eq!(out.seconds, 0.0);
    }

    #[test]
    fn an_animation_nobody_waits_for_costs_nothing() {
        let (anim, line) = plain();
        let mut script = Script::new("x");
        script.push(Action::PlayAnim {
            animation: "a".into(),
            wait: false,
            at: None,
        });
        script.push(Action::Wait { seconds: 1.0 });

        let out = timeline(&script, &timings(&*anim, &*line));
        assert_eq!(out.slots[0].seconds, 0.0);
        assert_eq!(out.slots[1].at, 0.0);
    }

    #[test]
    fn only_a_length_the_script_states_can_be_dragged() {
        // Offering to drag a line's length would be offering an edit
        // the editor cannot make: it comes from the text.
        let (anim, line) = plain();
        let mut script = Script::new("x");
        script.push(Action::Wait { seconds: 1.0 });
        script.push(Action::Say {
            line: "l".into(),
            speaker: None,
        });
        script.push(Action::MoveCamera {
            to: Vec2::ZERO,
            seconds: 2.0,
        });

        let out = timeline(&script, &timings(&*anim, &*line));
        let draggable: Vec<bool> = out.slots.iter().map(|s| s.adjustable).collect();
        assert_eq!(draggable, vec![true, false, true]);
    }

    #[test]
    fn a_branch_is_laid_out_down_one_side_and_says_so() {
        // Drawing both would invent a scene twice as long as the one
        // that runs.
        let (anim, line) = plain();
        let mut script = Script::new("x");
        script.push(Action::Branch {
            on: Condition::Flag("f".into()),
            then: vec![Action::Wait { seconds: 1.0 }],
            otherwise: vec![Action::Wait { seconds: 9.0 }],
        });

        let out = timeline(&script, &timings(&*anim, &*line));
        assert_eq!(out.seconds, 1.0, "the then side, not both");
        assert_eq!(out.untaken.len(), 1);
        assert_eq!(out.slots[1].depth, 1, "and it is drawn nested");
    }

    #[test]
    fn a_slot_knows_which_action_it_stands_for() {
        // Dragging a bar has to reach the action, including inside a
        // branch, or the timeline is a picture rather than an editor.
        let (anim, line) = plain();
        let mut script = Script::new("x");
        script.push(Action::Branch {
            on: Condition::Flag("f".into()),
            then: vec![Action::Wait { seconds: 1.0 }],
            otherwise: Vec::new(),
        });

        let out = timeline(&script, &timings(&*anim, &*line));
        assert_eq!(out.slots[0].path, vec![0]);
        assert_eq!(out.slots[1].path, vec![0, 0]);
    }

    #[test]
    fn a_line_stays_up_at_least_a_moment() {
        assert_eq!(reading_time(""), SUBTITLE_MIN);
        assert!(reading_time(&"x".repeat(140)) > SUBTITLE_MIN);
    }

    #[test]
    fn an_animation_the_project_does_not_know_still_takes_a_moment() {
        // Zero would collapse the scene, and a timeline that shows a
        // cutscene as instantaneous hides the thing being tuned.
        let anim: AnimFn = Box::new(|_| None);
        let line: LineFn = Box::new(|_| None);
        let mut script = Script::new("x");
        script.push(Action::PlayAnim {
            animation: "missing".into(),
            wait: true,
            at: None,
        });
        let out = timeline(&script, &timings(&*anim, &*line));
        assert_eq!(out.seconds, ANIM_FALLBACK);
    }
}
