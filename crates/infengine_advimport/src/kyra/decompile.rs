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

use glam::Vec2;
use infengine_adventure::interact::{Facing, Hotspot, Shape};
use infengine_adventure::{Action, Condition, Script, Verb};

use super::emc;
use super::functions;
use super::regions;
use super::vm::{self, CallSite, Op};

/// Slots that say something. They differ in how the original drew the
/// text, which is a rendering decision our engine makes for itself.
const SAYS: &[i32] = &[
    1,   // characterSays
    52,  // customPrintTalkString
    60,  // printText
    85,  // fatPrint
    106, // intPrint
    139, // message
];
/// Sets a story flag, and clears one.
const FN_SET_FLAG: i32 = 5;
const FN_RESET_FLAG: i32 = 6;
/// Walks the player to a point.
const FN_WALK_PLAYER: i32 = 11;
/// `placeItemInGenericMapScene(item, index)` - the original drops the
/// item at a random free spot of a wilderness scene.
const FN_PLACE_ITEM_GENERIC: i32 = 31;
/// Walks a named character to one, so its point starts an argument later.
const FN_WALK_CHARACTER: i32 = 81;
/// Starts one of the room's animations, and waits for one to finish.
const FN_SCENE_ANIM_ON: i32 = 16;
const FN_INTERNAL_ANIM_ON: i32 = 37;
const FN_ANIM_UNTIL_DONE: i32 = 21;
/// Plays a sound effect.
const FN_PLAY_SOUND: i32 = 23;
/// Starts the music for a region of the map.
const FN_PLAY_MUSIC: i32 = 24;
/// Moves the player to another scene.
const FN_ENTER_SCENE: i32 = 45;
/// Pauses for a number of seconds.
const FN_DELAY_SECS: i32 = 33;
/// Pauses for a number of ticks, sixty to the second.
const FN_DELAY_TICKS: i32 = 2;
/// Puts an item in the inventory.
const FN_ADD_ITEM: i32 = 105;
/// Puts an item in the hand, and empties it.
const FN_SET_HAND: i32 = 14;
/// Lay an item on this room's floor at a point.
const FN_ITEM_APPEARS: i32 = 124;
const FN_DROP_ITEM: i32 = 12;
/// Open a rectangle of floor to walking, and block one off.
const FN_FLOOR_OPEN: i32 = 9;
const FN_FLOOR_BLOCK: i32 = 10;
const FN_REMOVE_HAND: i32 = 20;
/// The register scene scripts read the held item from.
const REG_HAND: i32 = 4;
/// Asks whether a story flag is set. A condition, not a step.
const FN_QUERY_FLAG: i32 = 4;
/// Answers where the player character stands, on x.
const FN_GET_CHAR_X: i32 = 56;
/// Turns a character to face a direction.
const FN_SET_FACING: i32 = 58;
/// Which argument of [`FN_SET_FACING`] is the direction.
const FACING_ARG: usize = 1;

/// Opens an animation file and binds it to a slot.
const FN_OPEN_WSA: i32 = 41;
/// Releases a slot.
const FN_CLOSE_WSA: i32 = 42;
/// Plays a run of frames from a slot: the room's animations, 854 of
/// them across the game and by far the commonest call left untranslated.
const FN_PLAY_WSA: i32 = 64;
/// Which argument of [`FN_OPEN_WSA`] names the file, and which the slot.
const OPEN_WSA_NAME: usize = 0;
const OPEN_WSA_SLOT: usize = 1;
/// Which argument of [`FN_PLAY_WSA`] carries the slot.
///
/// Found by range rather than by guessing: `openWsaFile` only ever
/// assigns slots 0 to 5, and of the seven arguments this call takes,
/// the sixth is the only one whose values stay inside that. The others
/// run to 112 and 240, which are frame numbers and screen positions.
const PLAY_WSA_SLOT: usize = 5;
/// Where [`FN_PLAY_WSA`]'s screen position starts: arguments 2 and 3.
///
/// Same method as the slot: across all 782 seven-argument calls in the
/// game, argument 2 stays within 0..=320 and argument 3 within 0..=200,
/// and no other pair does - screen coordinates, nothing else fits.
const PLAY_WSA_AT: usize = 2;

/// The verb a click carries in a game that has no verb menu.
const CLICK: Verb = Verb::Use;

/// How many calls a single script may hold before the decompiler stops.
///
/// A room's entry point is tens of instructions; a runaway here would
/// mean the code was misread rather than that a room is enormous.
const MAX_CALLS: usize = 4096;

/// One room's scripts, decompiled.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoomScripts {
    pub scripts: Vec<Script>,
    /// Calls dropped as the original's screen machinery.
    pub plumbing_dropped: usize,
    /// Calls dropped as questions rather than steps.
    pub queries_dropped: usize,
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

/// Which animation file each slot currently holds.
///
/// A script opens a file into a slot, plays from it, and often opens a
/// different file into the same slot later - one of Kyrandia's rooms
/// reuses slot 0 for two animations - so the binding is a thing that
/// changes as the code runs, not a table the file carries. Reading it
/// means walking the calls in order and remembering.
///
/// This is exact only for straight-line code. Where a slot is opened in
/// one branch and played in another the binding here is whichever open
/// came last in the file, which is right whenever the open and the play
/// sit together and is a guess otherwise. A slot nothing has opened
/// resolves to nothing rather than to the last file seen.
///
/// The fallback is the same guess widened to the whole script: an
/// entry point often plays from a slot that a *later* entry point's
/// text happens to bind - the game bound it at run time, in whatever
/// order the player clicked. A slot no open call anywhere names still
/// resolves to nothing.
#[derive(Debug, Clone, Default)]
struct WsaSlots {
    open: std::collections::HashMap<i32, String>,
    fallback: std::collections::HashMap<i32, String>,
}

impl WsaSlots {
    /// Slots seeded with every binding the whole script ever makes.
    fn with_fallback(sites: &[CallSite], texts: &[String]) -> Self {
        // Only the opens: the walk honours closes as it passes them,
        // but the fallback asks a different question - which file did
        // this script EVER put in that slot - and a close would erase
        // exactly the answer. That is how every play sitting after its
        // slot's close (most of them; scripts tidy up) went unnamed.
        let mut all = Self::default();
        for site in sites.iter().filter(|s| s.function == FN_OPEN_WSA) {
            all.observe(site, texts);
        }
        Self {
            open: Default::default(),
            fallback: all.open,
        }
    }
    /// Note what a call does to the slots.
    fn observe(&mut self, site: &CallSite, texts: &[String]) {
        match site.function {
            FN_OPEN_WSA => {
                let (Some(&name), Some(&slot)) = (
                    site.args.get(OPEN_WSA_NAME),
                    site.args.get(OPEN_WSA_SLOT),
                ) else {
                    return;
                };
                if let Some(stem) = wsa_stem(name, texts) {
                    self.open.insert(slot, stem);
                }
            }
            FN_CLOSE_WSA => {
                if let Some(&slot) = site.args.first() {
                    self.open.remove(&slot);
                }
            }
            _ => {}
        }
    }

    fn get(&self, slot: i32) -> Option<&str> {
        self.open
            .get(&slot)
            .or_else(|| self.fallback.get(&slot))
            .map(String::as_str)
    }
}

/// The animation name behind a filename index, if the text there is one.
///
/// The file name comes out of the script's own string table, so a room's
/// animations are named in the room. An index that lands on a line of
/// dialogue instead is not one - which happens, and is how a wrong
/// reading of the argument order announced itself.
fn wsa_stem(index: i32, texts: &[String]) -> Option<String> {
    let text = usize::try_from(index).ok().and_then(|i| texts.get(i))?;
    let stem = text.strip_suffix(".WSA").or_else(|| text.strip_suffix(".wsa"))?;
    (!stem.is_empty()).then(|| stem.to_ascii_lowercase())
}

/// The animations the import actually produced.
///
/// A scene animation is called by number, and the number is an index
/// into a table the game builds at run time - not into anything the
/// files contain. Turning it into `altar_2` and hoping was giving 53 of
/// 123 referenced animations no file behind them: an action pointing at
/// nothing, which plays as silence and reads as working.
///
/// So a name has to be checked before it is used. With no set to check
/// against every name is accepted, which is the old behaviour and is
/// what the tests that predate this use.
#[derive(Debug, Clone, Copy, Default)]
pub struct Known<'a> {
    ids: Option<&'a std::collections::HashSet<String>>,
}

impl<'a> Known<'a> {
    /// Check names against this set.
    pub fn these(ids: &'a std::collections::HashSet<String>) -> Self {
        Self { ids: Some(ids) }
    }

    /// Accept any name, because there is nothing to check against.
    pub fn anything() -> Self {
        Self { ids: None }
    }

    fn has(&self, id: &str) -> bool {
        self.ids.is_none_or(|ids| ids.contains(id))
    }
}

/// What one call becomes.
enum Translated {
    Step(Action),
    /// Dropped, and why.
    Dropped(functions::Kind),
}

/// Translate one call into an action, or say why it is not one.
///
/// The slot numbers are Kyrandia's; the decisions about them are ours.
/// A slot with real behaviour we have no action for stays as
/// [`Action::Unknown`] carrying the slot's name - `kyra:shakeScreen`
/// rather than `kyra:107` - because a reader can act on the first and
/// not on the second.
fn translate(
    room: &str,
    site: &CallSite,
    texts: usize,
    slots: &WsaSlots,
    known: Known<'_>,
) -> Translated {
    let action = match site.function {
        f if SAYS.contains(&f) => {
            // The line index is the last constant pushed, which is the
            // call's first argument. A call whose index was computed
            // has no constants and cannot be resolved to a line.
            site.args
                .first()
                .copied()
                .and_then(|arg| line_index(arg, texts))
                .map(|index| Action::Say {
                    line: emc::Script::line_id(room, index),
                    speaker: None,
                })
        }
        FN_ENTER_SCENE => site.args.first().copied().filter(|s| *s >= 0).map(|scene| {
            // Scene numbers have no names in the data, so the room is
            // recorded as the number it is. Inventing a name here would
            // be a guess that later looks like a fact.
            Action::GoToRoom {
                room: format!("scene{scene}"),
                entry: None,
            }
        }),
        FN_SET_FLAG | FN_RESET_FLAG => site.args.first().copied().map(|flag| Action::SetFlag {
            flag: format!("kyra.flag{flag}"),
            value: site.function == FN_SET_FLAG,
        }),
        FN_WALK_PLAYER => point(&site.args, 0).map(|to| Action::Walk { to }),
        // The original rolls a random free spot; a deterministic spread
        // over the playfield keeps the drop reachable and repeatable.
        FN_PLACE_ITEM_GENERIC => site.args.first().map(|item| {
            let index = site.args.get(1).copied().unwrap_or(0).max(0) as f32;
            Action::PlaceItem {
                item: format!("kyra.item{item}"),
                at: glam::Vec2::new(48.0 + (index * 67.0) % 224.0, 104.0 + (index * 13.0) % 32.0),
            }
        }),
        // The character is the first argument here, so the point starts
        // one later. Reading it at zero walks the actor to the
        // character's number.
        FN_WALK_CHARACTER => point(&site.args, 1).map(|to| Action::Walk { to }),
        FN_PLAY_SOUND => site.args.first().copied().map(|sound| Action::PlaySound {
            sound: format!("kyra.sfx{sound}"),
        }),
        FN_PLAY_MUSIC => site.args.first().copied().map(|track| Action::PlayMusic {
            // Commands 0 and 1 of the original's track map are "fade
            // out" and "halt" - a stop, not a tune.
            music: (track > 1).then(|| format!("kyra.music{track}")),
        }),
        FN_SCENE_ANIM_ON | FN_INTERNAL_ANIM_ON | FN_ANIM_UNTIL_DONE => site
            .args
            .first()
            .map(|index| format!("{room}_{index}"))
            .filter(|id| known.has(id))
            .map(|animation| Action::PlayAnim {
                animation,
                wait: site.function == FN_ANIM_UNTIL_DONE,
                at: None,
            }),
        FN_DELAY_SECS => site.args.first().copied().map(|seconds| Action::Wait {
            seconds: seconds as f32,
        }),
        // The other delay counts in ticks, sixty to the second.
        FN_DELAY_TICKS => site
            .args
            .first()
            .copied()
            .filter(|ticks| *ticks >= 0)
            .map(|ticks| Action::Wait {
                seconds: ticks as f32 / 60.0,
            }),
        FN_ADD_ITEM => site.args.first().copied().map(|item| Action::Give {
            item: format!("kyra.item{item}"),
        }),
        // The drawbridge pair: a rectangle of floor opened to walking,
        // or blocked off. Anything past the four corners is stack
        // spillover from the surrounding expression, not geometry.
        FN_FLOOR_OPEN | FN_FLOOR_BLOCK => {
            match (point(&site.args, 0), point(&site.args, 2)) {
                (Some(min), Some(max)) => Some(Action::OpenFloor {
                    min,
                    max,
                    open: site.function == FN_FLOOR_OPEN,
                }),
                _ => None,
            }
        }
        // Both place an item on this room's floor at a point; they
        // differ upstream only in animation flourish.
        FN_ITEM_APPEARS | FN_DROP_ITEM => {
            match (site.args.first().copied(), point(&site.args, 1)) {
                (Some(item), Some(at)) if item >= 0 => Some(Action::PlaceItem {
                    item: format!("kyra.item{item}"),
                    at,
                }),
                _ => None,
            }
        }
        FN_SET_HAND => site.args.first().copied().map(|item| Action::SetHand {
            item: (item >= 0).then(|| format!("kyra.item{item}")),
        }),
        FN_REMOVE_HAND => Some(Action::SetHand { item: None }),
        FN_SET_FACING => site
            .args
            .get(FACING_ARG)
            .copied()
            .and_then(facing)
            .map(|facing| Action::Face { facing }),
        // The slot says which file, and the file is imported under the
        // room's name and its own. A slot nothing opened stays unknown
        // rather than pointing at an animation that is not there.
        //
        // A102: the third and fourth arguments are where the frames go.
        // Measured, not assumed: across all 782 seven-argument calls in
        // the game, argument 2 is always within 0..=320 and argument 3
        // within 0..=200 - screen coordinates, nothing else fits. They
        // were dropped before, which drew every scripted animation at
        // its sheet's own idea of home and scattered the intro.
        FN_PLAY_WSA => site
            .args
            .get(PLAY_WSA_SLOT)
            .and_then(|slot| slots.get(*slot))
            .map(|stem| format!("{room}_{stem}"))
            .filter(|id| known.has(id))
            .map(|animation| Action::PlayAnim {
                animation,
                wait: true,
                at: point(&site.args, PLAY_WSA_AT),
            }),
        _ => None,
    };

    if let Some(action) = action {
        return Translated::Step(action);
    }
    // A call we do translate but whose arguments were computed still has
    // to come through: dropping it would make the room look shorter than
    // it is. It comes through unknown, which is what it now is.
    match functions::kind(site.function, false) {
        functions::Kind::Plumbing => Translated::Dropped(functions::Kind::Plumbing),
        functions::Kind::Query => Translated::Dropped(functions::Kind::Query),
        _ => Translated::Step(unknown(site)),
    }
}

/// The direction a facing number means, if it is one we can express.
///
/// Kyrandia turns its characters through eight directions and our actor
/// has four, so the general mapping would be a decision with nothing to
/// check it against. It does not have to be made: across the whole game
/// every one of the 408 calls asks for 3 or 5, which are the two
/// diagonals toward the camera - a character turned to its right and to
/// its left. Those two are unambiguous, and anything else is refused
/// rather than rounded to the nearest thing we happen to have.
fn facing(direction: i32) -> Option<Facing> {
    match direction {
        3 => Some(Facing::Right),
        5 => Some(Facing::Left),
        _ => None,
    }
}

/// Everything the structuring pass carries around.
struct Pass<'a> {
    room: &'a str,
    instructions: &'a [vm::Instruction],
    texts: &'a [String],
    known: Known<'a>,
    /// Which animation file each slot holds, as the walk reaches it.
    slots: WsaSlots,
    plumbing_dropped: usize,
    queries_dropped: usize,
}

/// Word index a jump points at, with the marker bit removed.
fn jump_target(operand: i32) -> usize {
    (operand as u32 & 0x7FFF) as usize
}

/// The first instruction at or after `word`.
fn at_or_after(instructions: &[vm::Instruction], word: usize) -> usize {
    instructions.partition_point(|instruction| instruction.at / 2 < word)
}

impl Pass<'_> {
    /// Read a range of instructions as a list of steps.
    ///
    /// The shape of the code is recovered, not just its calls. A branch
    /// becomes a branch, with each side read the same way, so an
    /// imported script says what the game says instead of running both
    /// halves of every choice one after the other.
    ///
    /// This works because Kyrandia's scripts have no loops at all -
    /// across 1513 conditional jumps in the retail data not one goes
    /// backwards - so every jump forward nests, and nesting is the only
    /// thing this has to handle. A backward jump is left as a step
    /// rather than turned into something invented.
    fn structure(&mut self, range: std::ops::Range<usize>) -> Vec<Action> {
        let mut out = Vec::new();
        let mut index = range.start;
        while index < range.end.min(self.instructions.len()) {
            let instruction = self.instructions[index];

            if let (Op::JumpIfFalse, Some(target)) = (instruction.op, instruction.operand) {
                let end = at_or_after(self.instructions, jump_target(target));
                if end > index && end <= range.end {
                    index = self.branch(&mut out, index, end, range.end);
                    continue;
                }
            }

            if let Some(site) = vm::call_at(self.instructions, index) {
                self.slots.observe(&site, self.texts);
                self.step(&mut out, &site);
            }
            index += 1;
        }
        out
    }

    /// Read one conditional and both of its sides. Returns where to
    /// carry on.
    fn branch(
        &mut self,
        out: &mut Vec<Action>,
        at: usize,
        mut then_end: usize,
        limit: usize,
    ) -> usize {
        // An else side announces itself: the last thing the then side
        // does is jump forward over it.
        let mut otherwise = then_end..then_end;
        let mut resume = then_end;
        if then_end > at + 1
            && let Some(last) = self.instructions.get(then_end - 1)
            && let (Op::JumpOrConst, Some(target)) = (last.op, last.operand)
        {
            let after = at_or_after(self.instructions, jump_target(target));
            if after > then_end && after <= limit {
                otherwise = then_end..after;
                resume = after;
                // The jump itself is not a step on either side.
                then_end -= 1;
            }
        }

        let (on, flipped) = self.condition(at);
        let mut then = self.structure(at + 1..then_end);
        let mut otherwise = self.structure(otherwise);
        // A negated test - `if (!flag)` - reads as the flag itself
        // with the two sides swapped, which is the same behaviour
        // without a negation the runtime would need to learn.
        if flipped {
            std::mem::swap(&mut then, &mut otherwise);
        }

        // A branch that decides nothing and does nothing is not worth a
        // node; its condition was machinery.
        if then.is_empty() && otherwise.is_empty() {
            return resume;
        }
        out.push(Action::Branch {
            on,
            then,
            otherwise,
        });
        resume
    }

    /// What the conditional at `at` is testing, and whether its two
    /// sides are swapped.
    ///
    /// The retail scripts phrase a test as a call, its answer pushed,
    /// and then at most one negation or one comparison against a
    /// constant: `!f()`, `f() == 0`, `f() == -1`. The comparison
    /// operator numbers were measured, not assumed - see the
    /// `emc_evals` example; equality is pinned by the do-once idiom,
    /// where `if (queryGameFlag(f) == 0)` sets the same flag on its
    /// then side, and it does in every occurrence.
    ///
    /// A flag query is the one question the runtime can answer, so
    /// `!flag` and `flag == 0` come out as [`Condition::Flag`] with the
    /// branch sides swapped. Every other question keeps its name and
    /// its comparison in the label - `kyra:getHandItem == -1` - so the
    /// editor shows a branch on something a reader can act on, and the
    /// runtime takes the else side, a decision it can explain.
    fn condition(&self, at: usize) -> (Condition, bool) {
        // Back to the source of the value being tested: a call's
        // answer, or a register the engine filled before the script
        // ran - the held item travels in register 4.
        let mut back = at;
        let source = loop {
            if back == 0 {
                return (unresolved_condition(None), false);
            }
            back -= 1;
            let previous = self.instructions[back];
            match previous.op {
                Op::PushReturn
                | Op::AddStack
                | Op::Negate
                | Op::Eval
                | Op::Push
                | Op::PushAlt
                | Op::JumpOrConst
                | Op::PopFrameNeg
                | Op::PushFrameNeg
                | Op::PopReturn => continue,
                Op::Call => break None,
                Op::PushReg => match previous.operand {
                    Some(register) => break Some(register),
                    None => return (unresolved_condition(None), false),
                },
                _ => return (unresolved_condition(None), false),
            }
        };
        let call = back;
        let site = match source {
            None => match vm::call_at(self.instructions, call) {
                Some(site) => Some(site),
                None => return (unresolved_condition(None), false),
            },
            Some(_) => None,
        };

        // Forward over what happens to the answer.
        let mut answered = source.is_some();
        let mut flip = false;
        let mut constant: Option<i32> = None;
        let mut compare: Option<(i32, i32)> = None;
        let mut lone_eval: Option<i32> = None;
        for previous in &self.instructions[call + 1..at] {
            match previous.op {
                Op::AddStack | Op::PopFrameNeg | Op::PushFrameNeg | Op::PopReturn => {}
                Op::PushReturn => answered = true,
                Op::Negate => {
                    // The operand picks the negation; only the logical
                    // one (or none written, which the interpreter reads
                    // as it) is a branch with its sides swapped.
                    let logical = previous
                        .operand
                        .is_none_or(|which| which == NEGATE_LOGICAL);
                    if logical && compare.is_none() && constant.is_none() {
                        flip = !flip;
                    } else {
                        // Arithmetic negation, or a negation between
                        // the comparison's parts: shapes this does not
                        // read.
                        return (unresolved_condition(site.as_ref()), false);
                    }
                }
                Op::Push | Op::PushAlt | Op::JumpOrConst => match previous.operand {
                    Some(value) if constant.is_none() => constant = Some(value),
                    _ => return (unresolved_condition(site.as_ref()), false),
                },
                Op::Eval => match (previous.operand, constant.take(), compare, lone_eval) {
                    (Some(op), Some(value), None, _) => compare = Some((op, value)),
                    // An Eval with nothing pushed between the call and
                    // itself compares the answer against something from
                    // before the call - the click-versus-actor idiom.
                    // Remembered; whether it reads depends on what that
                    // something was.
                    (Some(op), None, None, None) => lone_eval = Some(op),
                    _ => return (unresolved_condition(site.as_ref()), false),
                },
                _ => return (unresolved_condition(site.as_ref()), false),
            }
        }
        if !answered {
            // Whatever is being tested, it is not this call's answer.
            return (unresolved_condition(None), false);
        }

        // A register condition: the held item, or a register we can
        // only name.
        if let Some(register) = source {
            let held = |value: i32| {
                Condition::Holding((value >= 0).then(|| format!("kyra.item{value}")))
            };
            return match compare {
                Some((EVAL_EQ, value)) if register == REG_HAND => (held(value), flip),
                Some((EVAL_NE, value)) if register == REG_HAND => (held(value), !flip),
                Some((op, value)) => (
                    Condition::Unresolved {
                        call: format!("kyra:reg{register} {} {}", sign(op), value),
                        args: Vec::new(),
                    },
                    flip,
                ),
                None => (
                    Condition::Unresolved {
                        call: format!("kyra:reg{register}"),
                        args: Vec::new(),
                    },
                    flip,
                ),
            };
        }
        let site = site.expect("a call site when no register was the source");

        // "Is the click at or past the actor": the click's x, pushed
        // before the call, at-least-compared against the actor's x.
        // The two forms carry the click the two ways the engine hands
        // it over - register 1 in room scripts, the first local in the
        // hit-test entries - measured at 91 sites each, and no other
        // shape uses a lone comparison here.
        if compare.is_none()
            && lone_eval == Some(5)
            && site.function == FN_GET_CHAR_X
            && self.click_x_before(call)
        {
            return (Condition::ClickPastActor, flip);
        }

        let flag = (site.function == FN_QUERY_FLAG)
            .then(|| site.args.first().map(|flag| format!("kyra.flag{flag}")))
            .flatten();
        match (compare, flag) {
            (None, Some(flag)) => (Condition::Flag(flag), flip),
            (Some((EVAL_EQ, 0)), Some(flag)) => (Condition::Flag(flag), !flip),
            (Some((EVAL_EQ, _)), Some(flag)) => (Condition::Flag(flag), flip),
            (Some((EVAL_NE, 0)), Some(flag)) => (Condition::Flag(flag), flip),
            (Some((EVAL_NE, _)), Some(flag)) => (Condition::Flag(flag), !flip),
            (None, None) => (unresolved_condition(Some(&site)), flip),
            (Some((op, value)), _) => (
                Condition::Unresolved {
                    call: format!("{} {} {}", functions::label(site.function), sign(op), value),
                    args: site.args.clone(),
                },
                flip,
            ),
        }
    }

    /// Whether the value pushed just before a call's own arguments is
    /// the click's x: register 1, or the hit-test's first local.
    fn click_x_before(&self, call: usize) -> bool {
        let mut back = call;
        while back > 0 {
            back -= 1;
            let previous = self.instructions[back];
            match previous.op {
                Op::Push | Op::PushAlt | Op::JumpOrConst | Op::Eval => continue,
                Op::PushReg | Op::PushFrame => return previous.operand == Some(1),
                _ => return false,
            }
        }
        false
    }

    /// Add one call as a step, or count why it is not one.
    fn step(&mut self, out: &mut Vec<Action>, site: &CallSite) {
        match translate(self.room, site, self.texts.len(), &self.slots, self.known) {
            Translated::Step(action) => out.push(action),
            Translated::Dropped(functions::Kind::Query) => self.queries_dropped += 1,
            Translated::Dropped(_) => self.plumbing_dropped += 1,
        }
    }
}

/// The comparison operators of the Eval instruction.
///
/// Two independent sources agree on equality: the published interpreter
/// table (ScummVM's EMC eval), and a measurement over the retail
/// scripts (the `emc_evals` example) - the do-once idiom, where
/// `if (queryGameFlag(f) == 0)` sets that same flag on its then side,
/// holds in every occurrence only with 2 as equality. The rest of the
/// mapping follows the published table, in which 4..7 read `>`, `>=`,
/// `<`, `<=` - greater-than first, the reverse of the tempting guess.
const EVAL_EQ: i32 = 2;
const EVAL_NE: i32 = 3;

/// What negation the Negate instruction's operand asks for. Only the
/// logical one may swap branch sides.
const NEGATE_LOGICAL: i32 = 0;

/// The symbol an Eval operand compares with, for composed labels.
///
/// Reads as "answer OP constant": the constant is pushed after the
/// answer, and the interpreter applies the operator second-to-top
/// first.
fn sign(op: i32) -> String {
    match op {
        2 => "==".into(),
        3 => "!=".into(),
        4 => ">".into(),
        5 => ">=".into(),
        6 => "<".into(),
        7 => "<=".into(),
        other => format!("cmp{other}"),
    }
}

/// A condition we could not phrase, named as far as it can be.
fn unresolved_condition(site: Option<&CallSite>) -> Condition {
    match site {
        Some(site) => Condition::Unresolved {
            call: functions::label(site.function),
            args: site.args.clone(),
        },
        None => Condition::Unresolved {
            call: "kyra:condition".into(),
            args: Vec::new(),
        },
    }
}

/// Two consecutive arguments read as a point.
fn point(args: &[i32], from: usize) -> Option<glam::Vec2> {
    let x = *args.get(from)?;
    let y = *args.get(from + 1)?;
    Some(glam::Vec2::new(x as f32, y as f32))
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
        call: functions::label(site.function),
        args: site.args.clone(),
    }
}

/// Decompile a room's script.
///
/// One [`Script`] per entry point the `ORDR` chunk names, so the
/// editor sees the same divisions the game did rather than one flat
/// list.
pub fn decompile(room: &str, script: &emc::Script) -> RoomScripts {
    decompile_known(room, script, Known::anything())
}

/// Decompile, accepting only animations that exist.
pub fn decompile_known(
    room: &str,
    script: &emc::Script,
    known: Known<'_>,
) -> RoomScripts {
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
        // The slots are read from the whole script up to here, not just
        // from this entry point: a file opened by one entry point is
        // still open when another plays from it.
        let mut pass = Pass {
            room,
            instructions: &instructions,
            texts: &script.texts,
            known,
            slots: WsaSlots::with_fallback(&sites, &script.texts),
            plumbing_dropped: 0,
            queries_dropped: 0,
        };
        for site in sites.iter().take_while(|s| s.at < *start) {
            pass.slots.observe(site, &script.texts);
        }
        let from = at_or_after(&instructions, *start / 2);
        let to = if end == usize::MAX {
            instructions.len()
        } else {
            at_or_after(&instructions, end / 2)
        };
        for action in pass.structure(from..to) {
            action.walk(&mut |step| {
                if let Action::GoToRoom { room, .. } = step
                    && let Some(number) =
                        room.strip_prefix("scene").and_then(|n| n.parse().ok())
                    && !out.scene_targets.contains(&number)
                {
                    out.scene_targets.push(number);
                }
            });
            compiled.push(action);
        }
        out.plumbing_dropped += pass.plumbing_dropped;
        out.queries_dropped += pass.queries_dropped;
        if !compiled.is_empty() {
            out.scripts.push(compiled);
        }
    }
    out
}

/// The clickable things a room's script defines, and what each does.
///
/// Kyrandia has no verb menu - a click on a thing runs that thing's
/// code - so each hotspot gets one script under a single verb rather
/// than a table of them.
///
/// That verb is `Use` rather than the engine's default of `Walk`.
/// `Walk` means "there is nothing here, go to it", which is what the
/// floor does; every rectangle found here is something the game has an
/// answer for, and importing them all as walk targets would say the
/// opposite of what the script proves.
///
/// Rectangles that do not fit the room are dropped. They come from calls
/// whose arguments were computed, where the constants left further up
/// the stack read as a rectangle the size of a scene number, and putting
/// one on the room would place a clickable thing where the game has
/// none.
pub fn hotspots(
    room: &str,
    script: &emc::Script,
    size: (i32, i32),
) -> Vec<(Hotspot, Script)> {
    hotspots_known(room, script, size, Known::anything())
}

/// The clickable things, accepting only animations that exist.
pub fn hotspots_known(
    room: &str,
    script: &emc::Script,
    size: (i32, i32),
    known: Known<'_>,
) -> Vec<(Hotspot, Script)> {
    let instructions = vm::disassemble(&script.code);
    let mut out = Vec::new();

    for region in regions::regions(&instructions) {
        if !region.fits(size.0, size.1) {
            continue;
        }
        // Numbered from one in the order the script tests them, which is
        // the order the game checks clicks in. Stable across imports as
        // long as the script is, which is what an id has to be.
        let id = format!("spot{}", out.len() + 1);
        let script_id = format!("{room}.{id}");

        let mut compiled = Script::new(script_id.clone());
        // Everything before the body, so a file opened when the room
        // was entered is still bound when a click plays from it.
        let mut pass = Pass {
            room,
            instructions: &instructions,
            texts: &script.texts,
            known,
            slots: WsaSlots::with_fallback(&vm::call_sites(&instructions), &script.texts),
            plumbing_dropped: 0,
            queries_dropped: 0,
        };
        let before = instructions.get(..region.body.start).unwrap_or_default();
        for site in vm::call_sites(before) {
            pass.slots.observe(&site, &script.texts);
        }
        for action in pass.structure(region.body.clone()) {
            compiled.push(action);
        }

        out.push((
            Hotspot {
                name: id.clone(),
                id,
                shape: Shape::Rect {
                    min: Vec2::new(region.x as f32, region.y as f32),
                    max: Vec2::new(
                        (region.x + region.width) as f32,
                        (region.y + region.height) as f32,
                    ),
                },
                default_verb: CLICK,
                actions: vec![(CLICK, script_id)],
                enabled: true,
                ..Default::default()
            },
            compiled,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first slot in [`SAYS`], for tests that need any of them.
    const FN_SAYS: i32 = 1;

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

    /// A script whose string table is given, rather than made up.
    fn script_saying(code: Vec<u8>, texts: &[&str]) -> emc::Script {
        emc::Script {
            entry_points: vec![0],
            texts: texts.iter().map(|s| (*s).to_string()).collect(),
            code,
        }
    }

    /// A call to `openWsaFile(name, slot, 0, 0)`.
    fn open_wsa(name: i8, slot: i8) -> Vec<u8> {
        // Pushed last argument first, so the name ends up nearest.
        let mut v = push(0);
        v.extend(push(0));
        v.extend(push(slot));
        v.extend(push(name));
        v.extend(call(FN_OPEN_WSA as u16));
        v
    }

    /// A call to `displayWsaSequentialFrames` with `slot` in its sixth
    /// argument and the screen position in its third and fourth, where
    /// the game puts them: frames 0..20, at 120,8, delay 14.
    fn play_wsa(slot: i8) -> Vec<u8> {
        // Pushed last argument first, so argument 0 lands nearest the
        // call.
        let mut v = push(1);
        v.extend(push(slot));
        v.extend(push(14));
        v.extend(push(8));
        v.extend(push(120));
        v.extend(push(20));
        v.extend(push(0));
        v.extend(call(FN_PLAY_WSA as u16));
        v
    }

    #[test]
    fn an_animation_is_named_by_the_file_the_slot_was_opened_with() {
        // The name is in the script's own string table, so a room names
        // its own animations - and the call's own coordinates say where
        // the frames go, which is the A102 fix: dropping them drew every
        // scripted animation at its sheet's idea of home.
        let mut code = open_wsa(1, 0);
        code.extend(play_wsa(0));
        let out = decompile("belroom", &script_saying(code, &["x", "RINGBELL.WSA"]));
        assert_eq!(
            out.scripts[0].actions.last(),
            Some(&Action::PlayAnim {
                animation: "belroom_ringbell".into(),
                wait: true,
                at: Some(glam::Vec2::new(120.0, 8.0)),
            })
        );
    }

    #[test]
    fn reopening_a_slot_changes_what_plays_from_it() {
        // One of the rooms uses slot 0 for two different animations, so
        // the binding has to follow the code rather than be read once.
        let mut code = open_wsa(1, 0);
        code.extend(play_wsa(0));
        code.extend(open_wsa(2, 0));
        code.extend(play_wsa(0));
        let out = decompile(
            "belroom",
            &script_saying(code, &["x", "RINGBELL.WSA", "RAISEPIC.WSA"]),
        );
        let played: Vec<&str> = out.scripts[0]
            .actions
            .iter()
            .filter_map(|a| match a {
                Action::PlayAnim { animation, .. } => Some(animation.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(played, vec!["belroom_ringbell", "belroom_raisepic"]);
    }

    #[test]
    fn playing_from_a_slot_nothing_opened_stays_unresolved() {
        // Falling back to the last file seen would put the wrong
        // animation in the room and look like it worked.
        let out = decompile("belroom", &script_saying(play_wsa(3), &["RINGBELL.WSA"]));
        assert!(matches!(
            out.scripts[0].actions.first(),
            Some(Action::Unknown { .. })
        ));
    }

    #[test]
    fn a_filename_index_landing_on_dialogue_is_not_a_filename() {
        // How a wrong reading of the argument order announced itself:
        // every animation resolved to a line of speech.
        let mut code = open_wsa(0, 0);
        code.extend(play_wsa(0));
        let out = decompile("x", &script_saying(code, &["Huh?", "REAL.WSA"]));
        assert!(matches!(
            out.scripts[0].actions.last(),
            Some(Action::Unknown { .. })
        ));
    }

    #[test]
    fn an_animation_that_was_never_produced_is_not_pointed_at() {
        // Scene animations are called by a number that indexes a table
        // the game builds at run time, not anything in the files.
        // Turning it into a name and hoping left 53 of 123 references
        // with nothing behind them.
        let mut code = push(2);
        code.extend(call(FN_SCENE_ANIM_ON as u16));
        let script = script_with(code, 4, vec![0]);

        let anything = decompile("altar", &script);
        assert!(matches!(
            anything.scripts[0].actions[0],
            Action::PlayAnim { .. }
        ));

        let ids = std::collections::HashSet::from(["altar_9".to_string()]);
        let checked = decompile_known("altar", &script, Known::these(&ids));
        assert!(matches!(
            checked.scripts[0].actions[0],
            Action::Unknown { .. }
        ));
    }

    /// An unconditional jump to a word.
    fn jump(word: u16) -> Vec<u8> {
        let mut v = vec![0x20, 0];
        v.extend_from_slice(&(0x8000 | word).to_be_bytes());
        v
    }

    /// A conditional jump, taken when the top of the stack is false.
    fn jump_if_false(word: u16) -> Vec<u8> {
        let mut v = vec![0x20 | 15, 0];
        v.extend_from_slice(&(0x8000 | word).to_be_bytes());
        v
    }

    /// `queryGameFlag(flag)` and the pushing of its answer, which is
    /// how every readable condition in the retail scripts is written.
    fn ask_flag(flag: i8) -> Vec<u8> {
        let mut v = push(flag);
        v.extend(call(FN_QUERY_FLAG as u16));
        v.extend_from_slice(&[0x40 | 12, 1]); // AddStack 1
        v.extend_from_slice(&[0x40 | 2, 0]); // PushReturn 0
        v
    }

    /// One step whose only job is to be recognisable in a branch.
    fn set_flag(flag: i8) -> Vec<u8> {
        let mut v = push(flag);
        v.extend(call(FN_SET_FLAG as u16));
        v
    }

    #[test]
    fn an_if_else_becomes_a_branch_with_two_sides() {
        // Flattening this would run both halves of a choice one after
        // the other, which is not what the game does.
        let mut code = ask_flag(7);
        let then = set_flag(1);
        let otherwise = set_flag(2);
        // Words: what is written so far, plus the conditional jump.
        let after_test = code.len() / 2 + 2;
        let else_at = after_test + then.len() / 2 + 2;
        let end_at = else_at + otherwise.len() / 2;

        code.extend(jump_if_false(else_at as u16));
        code.extend(then);
        code.extend(jump(end_at as u16));
        code.extend(otherwise);

        let out = decompile("x", &script_with(code, 4, vec![0]));
        let Some(Action::Branch { on, then, otherwise }) = out.scripts[0].actions.first() else {
            panic!("not a branch: {:?}", out.scripts[0].actions);
        };
        assert_eq!(*on, Condition::Flag("kyra.flag7".into()));
        assert_eq!(then.len(), 1, "{then:?}");
        assert_eq!(otherwise.len(), 1, "{otherwise:?}");
        assert_eq!(
            then[0],
            Action::SetFlag {
                flag: "kyra.flag1".into(),
                value: true
            }
        );
        assert_eq!(
            otherwise[0],
            Action::SetFlag {
                flag: "kyra.flag2".into(),
                value: true
            }
        );
    }

    /// A logical negation of the top of the stack.
    fn negate() -> Vec<u8> {
        vec![16, 0]
    }

    /// A comparison of the top of the stack against a constant: the
    /// constant, then Eval with the operator.
    fn compare(op: u8, with: i8) -> Vec<u8> {
        vec![0x40 | 4, with as u8, 0x40 | 17, op]
    }

    /// The branch sides around a test already emitted into `code`.
    fn branch_around(mut code: Vec<u8>, then: Vec<u8>, otherwise: Vec<u8>) -> Vec<u8> {
        let after_test = code.len() / 2 + 2;
        let else_at = after_test + then.len() / 2 + 2;
        let end_at = else_at + otherwise.len() / 2;
        code.extend(jump_if_false(else_at as u16));
        code.extend(then);
        code.extend(jump(end_at as u16));
        code.extend(otherwise);
        code
    }

    /// The branch a script starts with, with its sides as flag numbers.
    fn first_branch(code: Vec<u8>) -> (Condition, i32, i32) {
        let out = decompile("x", &script_with(code, 4, vec![0]));
        let Some(Action::Branch { on, then, otherwise }) = out.scripts[0].actions.first()
        else {
            panic!("not a branch: {:?}", out.scripts[0].actions);
        };
        let side = |actions: &[Action]| match actions.first() {
            Some(Action::SetFlag { flag, .. }) => {
                flag.trim_start_matches("kyra.flag").parse().unwrap()
            }
            other => panic!("not a set flag: {other:?}"),
        };
        (on.clone(), side(then), side(otherwise))
    }

    #[test]
    fn a_negated_test_swaps_the_branch_sides() {
        // `if (!queryGameFlag(7)) { A } else { B }` must run A when the
        // flag is clear. Reading it as the flag itself and swapping the
        // sides says exactly that without teaching the runtime negation.
        let mut code = ask_flag(7);
        code.extend(negate());
        let (on, then, otherwise) = first_branch(branch_around(code, set_flag(1), set_flag(2)));
        assert_eq!(on, Condition::Flag("kyra.flag7".into()));
        assert_eq!((then, otherwise), (2, 1), "sides swapped");
    }

    #[test]
    fn comparing_a_flag_with_zero_swaps_the_branch_sides() {
        // `if (queryGameFlag(7) == 0)` is the same question as the
        // negation, and the retail scripts write both.
        let mut code = ask_flag(7);
        code.extend(compare(2, 0));
        let (on, then, otherwise) = first_branch(branch_around(code, set_flag(1), set_flag(2)));
        assert_eq!(on, Condition::Flag("kyra.flag7".into()));
        assert_eq!((then, otherwise), (2, 1), "sides swapped");
    }

    #[test]
    fn comparing_a_flag_with_one_keeps_the_sides() {
        let mut code = ask_flag(7);
        code.extend(compare(2, 1));
        let (on, then, otherwise) = first_branch(branch_around(code, set_flag(1), set_flag(2)));
        assert_eq!(on, Condition::Flag("kyra.flag7".into()));
        assert_eq!((then, otherwise), (1, 2));
    }

    #[test]
    fn another_question_keeps_its_comparison_in_the_label() {
        // The runtime cannot answer it, but the editor can show it.
        let mut code = call(34);
        code.extend_from_slice(&[0x40 | 2, 0]); // PushReturn
        code.extend(compare(2, -1));
        let (on, _, _) = first_branch(branch_around(code, set_flag(1), set_flag(2)));
        assert_eq!(
            on,
            Condition::Unresolved {
                call: "kyra:getCharacterScene == -1".into(),
                args: vec![],
            }
        );
    }

    #[test]
    fn the_held_item_register_reads_as_holding() {
        // Scene scripts get the held item in register 4; comparing it
        // with an item number is the game's "using X on this?".
        let mut code = vec![0x40 | 5, 4];
        code.extend(compare(2, 23));
        let (on, then, otherwise) = first_branch(branch_around(code, set_flag(1), set_flag(2)));
        assert_eq!(on, Condition::Holding(Some("kyra.item23".into())));
        assert_eq!((then, otherwise), (1, 2));
    }

    #[test]
    fn comparing_the_hand_with_minus_one_means_empty_handed() {
        let mut code = vec![0x40 | 5, 4];
        code.extend(compare(2, -1));
        let (on, _, _) = first_branch(branch_around(code, set_flag(1), set_flag(2)));
        assert_eq!(on, Condition::Holding(None));
    }

    /// Push a constant too big for the byte form, as the word form.
    fn push_word(value: u16) -> Vec<u8> {
        let mut v = vec![0x20 | 4, 0];
        v.extend_from_slice(&value.to_be_bytes());
        v
    }

    #[test]
    fn a_play_after_its_slots_close_still_resolves() {
        // Scripts tidy up: open, play, close - and a later entry point
        // plays from the slot again. The fallback must remember what
        // the slot EVER held, or most of the game's animations go
        // unnamed (which is exactly the bug this pins).
        let mut code = open_wsa(1, 0);
        code.extend(push(0));
        code.extend(call(FN_CLOSE_WSA as u16));
        code.extend(play_wsa(0));
        let out = decompile("belroom", &script_saying(code, &["x", "RINGBELL.WSA"]));
        assert_eq!(
            out.scripts[0].actions.last(),
            Some(&Action::PlayAnim {
                animation: "belroom_ringbell".into(),
                wait: true,
                at: Some(glam::Vec2::new(120.0, 8.0)),
            }),
            "{:?}",
            out.scripts[0].actions
        );
    }

    #[test]
    fn a_seven_argument_sequential_play_resolves_through_the_slot() {
        // The retail shape: [start, end, x, y, delay, slot, flag], with
        // x past the byte range, after an open that bound slot 0.
        let mut code = open_wsa(1, 0);
        for value in [1i8, 0, 10] {
            code.extend(push(value));
        }
        code.extend(push_word(64));
        code.extend(push_word(184));
        code.extend(push(13));
        code.extend(push(9));
        code.extend(call(FN_PLAY_WSA as u16));
        let out = decompile("belroom", &script_saying(code, &["x", "RINGBELL.WSA"]));
        assert_eq!(
            out.scripts[0].actions.last(),
            Some(&Action::PlayAnim {
                animation: "belroom_ringbell".into(),
                wait: true,
                at: Some(glam::Vec2::new(184.0, 64.0)),
            }),
            "{:?}",
            out.scripts[0].actions
        );
    }

    #[test]
    fn the_drawbridge_pair_becomes_open_floor() {
        // blockInWalkableRegion(x1, y1, x2, y2), pushed last first.
        let mut code = push(119);
        code.extend(push(106));
        code.extend(push(108));
        code.extend(push(70));
        code.extend(call(FN_FLOOR_OPEN as u16));
        let out = decompile("cgate", &script_with(code, 4, vec![0]));
        assert_eq!(
            out.scripts[0].actions.first(),
            Some(&Action::OpenFloor {
                min: Vec2::new(70.0, 108.0),
                max: Vec2::new(106.0, 119.0),
                open: true,
            })
        );
    }

    #[test]
    fn an_item_appearing_on_the_ground_becomes_place_item() {
        // itemAppearsOnGround(item, x, y): pushed last argument first.
        let mut code = push(90);
        code.extend(push(120));
        code.extend(push(23));
        code.extend(call(124));
        let out = decompile("x", &script_with(code, 4, vec![0]));
        assert_eq!(
            out.scripts[0].actions.first(),
            Some(&Action::PlaceItem {
                item: "kyra.item23".into(),
                at: Vec2::new(120.0, 90.0),
            })
        );
    }

    #[test]
    fn hand_calls_become_set_hand_steps() {
        let mut code = push(23);
        code.extend(call(FN_SET_HAND as u16));
        code.extend(call(FN_REMOVE_HAND as u16));
        let out = decompile("x", &script_with(code, 4, vec![0]));
        assert_eq!(
            out.scripts[0].actions,
            vec![
                Action::SetHand {
                    item: Some("kyra.item23".into())
                },
                Action::SetHand { item: None },
            ]
        );
    }

    #[test]
    fn click_versus_character_x_reads_as_click_past_actor() {
        for push_click in [vec![0x40 | 5, 1], vec![0x40 | 7, 1]] {
            // Register 1 in room scripts, the first local in hit-test
            // entries: both carry the click's x.
            let mut code = push_click;
            code.extend(push(0));
            code.extend(call(FN_GET_CHAR_X as u16));
            code.extend(vec![0x40 | 12, 1, 0x40 | 2, 0, 0x40 | 17, 5]);
            let (on, then, otherwise) =
                first_branch(branch_around(code, set_flag(1), set_flag(2)));
            assert_eq!(on, Condition::ClickPastActor);
            assert_eq!((then, otherwise), (1, 2));
        }
    }

    #[test]
    fn a_lone_comparison_from_elsewhere_stays_unresolved() {
        // The same shape but fed from register 2 is not the click's x,
        // and pretending it is would run the wrong side of the story.
        let mut code = vec![0x40 | 5, 2];
        code.extend(push(0));
        code.extend(call(FN_GET_CHAR_X as u16));
        code.extend(vec![0x40 | 12, 1, 0x40 | 2, 0, 0x40 | 17, 5]);
        let (on, _, _) = first_branch(branch_around(code, set_flag(1), set_flag(2)));
        assert!(matches!(on, Condition::Unresolved { .. }), "{on:?}");
    }

    #[test]
    fn a_call_past_slot_127_is_read_unsigned() {
        // The byte operand encoding is signed; a function index is not.
        // 139 is `message`, which says a line - narration all over the
        // game, and a fifth of it was arriving as `kyra:-117`.
        let mut code = push(1);
        code.extend(vec![0x40 | 14, 139]);
        let out = decompile("x", &script_with(code, 4, vec![0]));
        assert_eq!(
            out.scripts[0].actions.first(),
            Some(&Action::Say {
                line: emc::Script::line_id("x", 1),
                speaker: None,
            })
        );
    }

    #[test]
    fn a_delay_in_ticks_becomes_seconds() {
        let mut code = push(90);
        code.extend(call(FN_DELAY_TICKS as u16));
        let out = decompile("x", &script_with(code, 4, vec![0]));
        assert_eq!(
            out.scripts[0].actions.first(),
            Some(&Action::Wait { seconds: 1.5 })
        );
    }

    #[test]
    fn a_slot_opened_by_a_later_entry_point_still_names_the_animation() {
        // The game binds slots at run time in whatever order the player
        // clicks, so an early entry point may play from a slot whose
        // open call sits later in the file.
        let mut code = play_wsa(0);
        let second_entry = code.len() as u16;
        code.extend(open_wsa(1, 0));
        let out = decompile(
            "belroom",
            &emc::Script {
                entry_points: vec![0, second_entry],
                texts: vec!["x".into(), "RINGBELL.WSA".into()],
                code,
            },
        );
        assert_eq!(
            out.scripts[0].actions.first(),
            Some(&Action::PlayAnim {
                animation: "belroom_ringbell".into(),
                wait: true,
                at: Some(glam::Vec2::new(120.0, 8.0)),
            })
        );
    }

    #[test]
    fn an_if_with_no_else_has_an_empty_other_side() {
        let mut code = ask_flag(3);
        let then = set_flag(1);
        let after_test = code.len() / 2 + 2;
        let end_at = after_test + then.len() / 2;

        code.extend(jump_if_false(end_at as u16));
        code.extend(then);
        code.extend(set_flag(9)); // after the branch, on neither side

        let out = decompile("x", &script_with(code, 4, vec![0]));
        let Some(Action::Branch { then, otherwise, .. }) = out.scripts[0].actions.first() else {
            panic!("not a branch: {:?}", out.scripts[0].actions);
        };
        assert_eq!(then.len(), 1);
        assert!(otherwise.is_empty());
        assert_eq!(
            out.scripts[0].actions.len(),
            2,
            "what follows the branch is not part of it"
        );
    }

    #[test]
    fn a_condition_we_cannot_phrase_keeps_the_shape_and_says_so() {
        // The alternative is dropping the branch, which turns a choice
        // into both outcomes happening.
        let mut code = push(0);
        code.extend(call(103)); // getItemInMarbleVase
        code.extend_from_slice(&[0x40 | 12, 1]);
        code.extend_from_slice(&[0x40 | 2, 0]);
        let then = set_flag(1);
        let after_test = code.len() / 2 + 2;
        let end_at = after_test + then.len() / 2;
        code.extend(jump_if_false(end_at as u16));
        code.extend(then);

        let out = decompile("x", &script_with(code, 4, vec![0]));
        let Some(Action::Branch { on, then, .. }) = out.scripts[0].actions.first() else {
            panic!("not a branch: {:?}", out.scripts[0].actions);
        };
        assert_eq!(
            *on,
            Condition::Unresolved {
                call: "kyra:getItemInMarbleVase".into(),
                args: vec![0]
            }
        );
        assert!(!on.is_resolved());
        assert_eq!(then.len(), 1);
    }

    #[test]
    fn a_branch_where_neither_side_does_anything_is_not_kept() {
        // Its condition was machinery deciding how to draw something.
        let mut code = ask_flag(7);
        let after_test = code.len() / 2 + 2;
        code.extend(jump_if_false(after_test as u16));
        let out = decompile("x", &script_with(code, 4, vec![0]));
        assert!(out.is_empty(), "{:?}", out.scripts);
    }

    #[test]
    fn the_two_directions_the_game_actually_uses_are_translated() {
        // Eight directions to our four would be a decision with nothing
        // to check it against - except that the game only ever asks for
        // two of them, and those two are unambiguous.
        assert_eq!(facing(3), Some(Facing::Right));
        assert_eq!(facing(5), Some(Facing::Left));
        for other in [0, 1, 2, 4, 6, 7, -1] {
            assert_eq!(facing(other), None, "{other} was rounded to something");
        }

        // setCharacterFacing(character, direction, delay)
        let mut code = push(0);
        code.extend(push(5));
        code.extend(push(0));
        code.extend(call(FN_SET_FACING as u16));
        let out = decompile("x", &script_with(code, 2, vec![0]));
        assert_eq!(
            out.scripts[0].actions[0],
            Action::Face {
                facing: Facing::Left
            }
        );
    }

    #[test]
    fn a_dialogue_call_becomes_a_say_referencing_a_line_id() {
        let mut code = push(2);
        code.extend(call(FN_SAYS as u16));
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
        for function in SAYS.iter().copied() {
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
        code.extend(call(FN_SAYS as u16));
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
        // These two were 64 and 33 until the table was read properly.
        // 64 displays animation frames and 33 waits for a couple of
        // seconds - neither is machinery, and both had a confident
        // comment saying they were.
        let mut code = push(1);
        code.extend(call(101)); // pageFlip
        code.extend(push(1));
        code.extend(call(84)); // setLogicPage
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
                function: FN_WALK_PLAYER,
                args: vec![90, 40],
                check: |a| matches!(a, Action::Walk { .. }),
            },
            Case {
                function: FN_PLAY_SOUND,
                args: vec![12],
                check: |a| matches!(a, Action::PlaySound { .. }),
            },
            Case {
                function: FN_SCENE_ANIM_ON,
                args: vec![2],
                check: |a| matches!(a, Action::PlayAnim { wait: false, .. }),
            },
            Case {
                function: FN_ANIM_UNTIL_DONE,
                args: vec![2],
                check: |a| matches!(a, Action::PlayAnim { wait: true, .. }),
            },
            Case {
                function: FN_PLAY_MUSIC,
                args: vec![3],
                check: |a| matches!(a, Action::PlayMusic { .. }),
            },
            Case {
                function: FN_DELAY_SECS,
                args: vec![2],
                check: |a| matches!(a, Action::Wait { .. }),
            },
            Case {
                function: FN_ADD_ITEM,
                args: vec![5],
                check: |a| matches!(a, Action::Give { .. }),
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
        code.extend(call(FN_WALK_PLAYER as u16));
        let out = decompile("hall", &script_with(code, 0, vec![0]));
        assert_eq!(
            out.scripts[0].actions[0],
            Action::Walk {
                to: glam::Vec2::new(90.0, 40.0)
            }
        );
    }

    #[test]
    fn an_unidentified_call_is_kept_under_the_name_of_what_it_does() {
        // The whole reason Unknown exists - and it carries the slot's
        // name, because a reader can act on `kyra:shakeScreen` and
        // cannot act on `kyra:107`.
        let mut code = push(3);
        code.extend(call(107));
        let out = decompile("x", &script_with(code, 8, vec![0]));
        assert_eq!(
            out.scripts[0].actions[0],
            Action::Unknown {
                call: "kyra:shakeScreen".into(),
                args: vec![3]
            }
        );
        assert_eq!(out.unknown_count(), 1);
    }

    #[test]
    fn a_call_past_the_table_still_comes_through_as_its_number() {
        let mut code = push(3);
        code.extend(call(900));
        let out = decompile("x", &script_with(code, 8, vec![0]));
        assert_eq!(
            out.scripts[0].actions[0],
            Action::Unknown {
                call: "kyra:900".into(),
                args: vec![3]
            }
        );
    }

    #[test]
    fn entry_points_divide_the_code_into_separate_scripts() {
        let mut code = push(0);
        code.extend(call(FN_SAYS as u16));
        let split = code.len() as u16;
        code.extend(push(1));
        code.extend(call(FN_SAYS as u16));

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
        code.extend(call(FN_SAYS as u16));
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
