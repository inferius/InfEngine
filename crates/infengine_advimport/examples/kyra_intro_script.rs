//! A117: the intro's rooms, scripts and subtitles, written by a tool.
//!
//! The choreography numbers here - placements, music tracks, effect
//! cues and their tick offsets, text order - were all read out of
//! MAIN.EXE's sequence bytecode (see `kyra_seq`). What used to be
//! ad-hoc edits on the project is now a step that can be re-run: the
//! subtitle texts come from the executable's own string table, the
//! Czech translations ride along here, and the four rooms and scripts
//! are written whole.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_intro_script -- <floppy dir> <project> [vrm dir]
//! ```
//!
//! With a directory holding `INTRO.VRM` - the CD release keeps the
//! intro's speech there, numbered in scene order with its own gaps -
//! the recordings are written under `voice/intro/` and the
//! voice-language lines point at them. Which recording speaks which
//! line is [`LINES`]' third column, read out of the CD executable's
//! sequence bytecode; it is not the string index.

#![allow(clippy::print_stdout)]

use std::fmt::Write as _;
use std::path::PathBuf;

/// Reading speed the engine uses; a Say blocks for this long.
const SUBTITLE_MIN: f32 = 1.2;
const CHARS_PER_SECOND: f32 = 14.0;

/// Czech for the intro lines, keyed by the EXE string index.
const CZECH: &[(usize, &str)] = &[
    (2, "Bud zdrav, Kallaku..."),
    (3, "...mocny vudce Kralovskych mystiku!"),
    (4, "Lekl jsem te?"),
    (5, "Malcolme!"),
    (6, "Vcera jsem se doslechl o tvem uteku."),
    (7, "Cekal jsem te uz driv."),
    (8, "Proc bych spechal?"),
    (9, "Teto zemi vladnu ja!"),
    (10, "Tva uboha kletba me nezastavi."),
    (11, "Tak me zabij hned!"),
    (12, "Mnoho magie mi uz nezbylo."),
    (13, "Ublizit ti chci, o tom nepochybuj..."),
    (14, "...Ted jsi kamen..."),
    (15, "...Ale oci ti nechavam."),
    (16, "Pro Kyrandii slzy neronim..."),
    (17, "...ale tve ti neuprem."),
    (22, "Tys mi podrazil nohy!"),
    (23, "Ale ne, pro tebe mam neco zvlastniho..."),
    (24, "Zertovne zaklinadlo!"),
    (25, "Ale poctive te varuji."),
    (26, "Neskakej na TENHLE strom!"),
    (28, "Buu!"),
    (29, "Tak je to zabavnejsi..."),
    (30, "...nebo neni?"),
    (31, "Ha ha ha..."),
    (46, "Draha Brynn..."),
    (47, "Malcolm se osvobodil!"),
    (48, "Brzy si prijde i pro me."),
    (49, "Prosim, pomoz Brandonovi..."),
];

/// Which EXE string each of our line ids means, and which `INTRO.VRM`
/// recording speaks it.
///
/// The recording numbers are NOT the string indexes - they run in
/// scene order with their own gaps, and the pairing comes from the CD
/// executable's own sequence bytecode, where every `printTalkText`
/// sits next to its `playVocFile` (decode the CD `EXE` directory with
/// `kyra_seq` to see it). Guessing "recording = string index" put
/// Kallak's outro shout and Brandon's reunion line into the forest,
/// which is exactly how wrong that guess sounds.
///
/// The line sets are the floppy sequences' own `printTalkText` lists:
/// the forest speaks strings 31, 22..26, 28..30 - the "fingers" pair
/// (32, 33) belongs to restored Kallak in the outro, not here.
const LINES: &[(&str, usize, Option<usize>)] = &[
    ("intro.let0", 46, Some(43)),
    ("intro.let1", 47, Some(44)),
    ("intro.let2", 48, Some(45)),
    ("intro.let3", 49, Some(46)),
    ("intro.tree0", 31, Some(28)),
    ("intro.tree1", 22, Some(19)),
    ("intro.tree2", 23, Some(20)),
    ("intro.tree3", 24, Some(21)),
    ("intro.tree4", 25, Some(22)),
    ("intro.tree5", 26, Some(23)),
    ("intro.tree6", 28, Some(25)),
    ("intro.tree7", 29, Some(27)),
    // "...Is it not?" rides recording 27 with the line before it.
    ("intro.tree8", 30, None),
    ("intro.mal0", 2, Some(0)),
    ("intro.mal1", 3, Some(1)),
    ("intro.mal2", 4, Some(2)),
    ("intro.mal3", 5, Some(3)),
    ("intro.mal4", 6, Some(4)),
    ("intro.mal5", 7, Some(5)),
    ("intro.mal6", 8, Some(6)),
    ("intro.mal7", 9, Some(7)),
    ("intro.mal8", 10, Some(8)),
    ("intro.mal9", 11, Some(9)),
    ("intro.mal10", 12, Some(10)),
    ("intro.mal11", 13, Some(11)),
    ("intro.mal12", 14, Some(12)),
    ("intro.mal13", 15, Some(13)),
    ("intro.mal14", 16, Some(14)),
    ("intro.mal15", 17, Some(15)),
    // Malcolm's parting laugh over the window scene's last beat.
    ("intro.mal16", 31, Some(28)),
];

/// The narrator over the island storm: recordings with no text
/// anywhere in the data - the CD shows nothing and only speaks.
const ISLAND: &[(&str, usize)] = &[
    ("intro.isl0", 55),
    ("intro.isl1", 56),
    ("intro.isl2", 57),
    ("intro.isl3", 58),
    ("intro.isl4", 59),
    ("intro.isl5", 60),
];

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_intro_script <floppy dir with MAIN.EXE> <project>");
        return;
    };
    let vrm_dir = args.next().map(PathBuf::from);
    let game = PathBuf::from(game);
    let project = PathBuf::from(project);

    let Ok(exe) = std::fs::read(game.join("MAIN.EXE")) else {
        println!("no MAIN.EXE");
        return;
    };
    let Some(english) = intro_strings(&exe) else {
        println!("the intro string table was not where it should be");
        return;
    };

    // The spoken lines, when the CD's INTRO.VRM is at hand. Keyed by
    // recording number; the line tables above say which line each one
    // speaks.
    let mut voiced: std::collections::HashMap<usize, String> = Default::default();
    let wanted: std::collections::BTreeSet<usize> = LINES
        .iter()
        .filter_map(|(_, _, voc)| *voc)
        .chain(ISLAND.iter().map(|(_, voc)| *voc))
        .collect();
    if let Some(dir) = &vrm_dir {
        match std::fs::read(dir.join("INTRO.VRM")) {
            Ok(bytes) => match infengine_advimport::kyra::pak::Pak::parse(&bytes) {
                Ok(pak) => {
                    for voc in &wanted {
                        let name = format!("{voc:03}.VOC");
                        let Some(at) = pak
                            .entries()
                            .iter()
                            .position(|e| e.name.eq_ignore_ascii_case(&name))
                        else {
                            continue;
                        };
                        let Some(data) = pak.data_at(at) else { continue };
                        let Ok(voice) = infengine_advimport::kyra::voc::decode(data) else {
                            println!("intro voice {voc:03}: did not decode");
                            continue;
                        };
                        let rel = format!("voice/intro/{voc:03}.wav");
                        let path = project.join(&rel);
                        if let Some(parent) = path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if std::fs::write(&path, voice.to_wav()).is_ok() {
                            voiced.insert(*voc, rel);
                        }
                    }
                    println!(
                        "intro speech: {} of {} recordings wired",
                        voiced.len(),
                        wanted.len()
                    );
                }
                Err(err) => println!("INTRO.VRM: {err}"),
            },
            Err(err) => println!("INTRO.VRM: {err}"),
        }
    }

    // Subtitles into both locales; existing voice fields stay.
    let loca_dir = project.join(infengine_loca::LOCALE_DIR);
    for (language, czech) in [("en", false), ("cs", true)] {
        let path = loca_dir.join(format!("{language}.loca.ron"));
        let mut locale = match infengine_loca::load_locale(&path) {
            Ok(locale) => locale,
            Err(err) => {
                println!("could not read the {language} locale: {err}");
                return;
            }
        };
        for (id, index, voc) in LINES {
            let text = if czech {
                CZECH
                    .iter()
                    .find(|(at, _)| at == index)
                    .map(|(_, text)| text.to_string())
            } else {
                english.get(*index).cloned()
            };
            let Some(text) = text else { continue };
            let line = locale.lines.entry(id.to_string()).or_default();
            line.text = text;
            // The voice rides on the voice language's locale only.
            if language == "en"
                && let Some(rel) = voc.and_then(|voc| voiced.get(&voc))
            {
                line.voice = Some(rel.clone());
            }
        }
        // The narrator's lines have no text in any language: an empty
        // line with a recording shows nothing and speaks.
        if language == "en" {
            for (id, voc) in ISLAND {
                if let Some(rel) = voiced.get(voc) {
                    let line = locale.lines.entry(id.to_string()).or_default();
                    line.text = String::new();
                    line.voice = Some(rel.clone());
                }
            }
        }
        match infengine_loca::save_locale(&locale, &loca_dir) {
            Ok(_) => println!("{language}: {} intro lines", LINES.len()),
            Err(err) => println!("{language}: {err}"),
        }
    }

    let rt = |id: &str| -> f32 {
        let index = LINES.iter().find(|(l, _, _)| *l == id).map(|(_, i, _)| *i);
        let text = index.and_then(|i| english.get(i)).cloned().unwrap_or_default();
        SUBTITLE_MIN + text.chars().count() as f32 / CHARS_PER_SECOND
    };

    for (name, content) in rooms() {
        write(&project.join("rooms").join(name), &content);
    }
    for (name, content) in scripts(&rt) {
        write(&project.join("scripts").join(name), &content);
    }

    // The manifest points at the front door and the first room.
    if let Some(mut manifest) = infengine_adventure::pack::GameManifest::load(&project) {
        manifest.start_room = "gemcut".into();
        manifest.start_entry = "entry0".into();
        manifest.intro_room = "_intro".into();
        match manifest.save(&project) {
            Ok(()) => println!("manifest points at the intro and gemcut"),
            Err(err) => println!("game.ron: {err}"),
        }
    }
}

/// The intro text table: NUL-separated strings anchored on the first
/// line of dialogue, with the two test strings in front of it.
fn intro_strings(exe: &[u8]) -> Option<Vec<String>> {
    let anchor = b"This is a text test - 1";
    let at = exe.windows(anchor.len()).position(|w| w == anchor)?;
    let mut out = Vec::new();
    let mut p = at;
    while out.len() < 49 && p < exe.len() {
        let end = p + exe[p..].iter().position(|&b| b == 0)?;
        out.push(String::from_utf8_lossy(&exe[p..end]).into_owned());
        p = end + 1;
    }
    (out.len() >= 49).then_some(out)
}

struct Body(String);

impl Body {
    fn new() -> Self {
        Body(String::new())
    }
    fn act(&mut self, name: &str, fields: &[(&str, String)]) {
        let _ = writeln!(self.0, "      {name}(");
        for (key, value) in fields {
            let _ = writeln!(self.0, "        {key}: {value},");
        }
        let _ = writeln!(self.0, "      ),");
    }
    fn music(&mut self, id: &str) {
        self.act("PlayMusic", &[("music", format!("Some(\"music.{id}\")"))]);
    }
    fn anim(&mut self, id: &str, wait: bool, x: f32, y: f32) {
        self.act(
            "PlayAnim",
            &[
                ("animation", format!("\"{id}\"")),
                ("wait", wait.to_string()),
                ("at", format!("Some(Vec2({x:.1}, {y:.1}))")),
            ],
        );
    }
    fn wait(&mut self, seconds: f32) {
        if seconds > 0.01 {
            self.act("Wait", &[("seconds", format!("{seconds:.2}"))]);
        }
    }
    fn sfx(&mut self, n: u32) {
        self.act("PlaySound", &[("sound", format!("\"sfx.intro_{n}\""))]);
    }
    fn say(&mut self, line: &str) {
        self.act("Say", &[("line", format!("\"{line}\""))]);
    }
    fn goto(&mut self, room: &str) {
        self.act("GoToRoom", &[("room", format!("\"{room}\""))]);
    }
    /// Take the held last frame off - the original fades here, and a
    /// part-built logo left hanging rides the pan as a ghost.
    fn clear_anim(&mut self) {
        self.act("PlayAnim", &[("animation", "\"\"".into()), ("wait", "false".into())]);
    }
    fn script(self, id: &str) -> String {
        format!(
            "[\n  Script(\n    id: \"{id}\",\n    actions: [\n{}    ],\n  ),\n]\n",
            self.0
        )
    }
}

/// Timed steps: waits are emitted between the given moments, and a Say
/// advances the clock by its reading time.
enum Step<'a> {
    Sfx(u32),
    Say(&'a str),
    Anim(&'a str, bool, f32, f32),
}

fn beats(body: &mut Body, rt: &dyn Fn(&str) -> f32, steps: &[(f32, Step)], tail: f32) {
    let mut clock = 0.0f32;
    for (at, step) in steps {
        if *at > clock {
            body.wait(*at - clock);
            clock = *at;
        }
        match step {
            Step::Sfx(n) => body.sfx(*n),
            Step::Anim(id, wait, x, y) => body.anim(id, *wait, *x, *y),
            Step::Say(line) => {
                body.say(line);
                clock += rt(line);
            }
        }
    }
    if tail > clock {
        body.wait(tail - clock);
    }
}

fn rooms() -> Vec<(&'static str, String)> {
    let plain = |id: &str, layer: Option<&str>, size: (f32, f32), view: (f32, f32)| {
        let layers = match layer {
            Some(image) => format!(
                "[\n    Layer(\n      name: \"background\",\n      image: \"{image}\",\n      offset: Vec2(0.0, 0.0),\n      kind: Background,\n      visible: true,\n    ),\n  ]"
            ),
            None => "[]".into(),
        };
        format!(
            "Room(\n  id: \"{id}\",\n  name: \"Intro\",\n  size: Vec2({}, {}),\n  camera: Camera(view: Vec2({}, {}), follow: false),\n  layers: {layers},\n  walk: WalkArea(solids: [], holes: []),\n  scale: ScaleRamp(zones: []),\n  hotspots: [],\n  exits: [],\n  entries: [],\n)\n",
            size.0, size.1, view.0, view.1
        )
    };
    vec![
        (
            "_intro.room.ron",
            plain("_intro", Some("pictures/intro_sky.png"), (320.0, 280.0), (320.0, 168.0)),
        ),
        (
            "_intro_tree.room.ron",
            plain("_intro_tree", Some("pictures/tree.png"), (320.0, 200.0), (320.0, 144.0)),
        ),
        (
            "_intro_kallak.room.ron",
            plain("_intro_kallak", None, (320.0, 200.0), (320.0, 144.0)),
        ),
        (
            "_intro_malcolm.room.ron",
            plain("_intro_malcolm", Some("pictures/mal_kal.png"), (320.0, 200.0), (320.0, 144.0)),
        ),
    ]
}

fn scripts(rt: &dyn Fn(&str) -> f32) -> Vec<(&'static str, String)> {
    use Step::{Anim, Say, Sfx};
    let mut out = Vec::new();

    // Logos on the strip, the pan, the island storm. Placements and
    // cue times are the measured ones (see kyra_seq / kyra_intro_fit).
    let mut b = Body::new();
    b.music("intro_2");
    b.wait(0.8);
    b.anim("intro1_westwood", true, 0.0, 39.0);
    b.clear_anim();
    b.wait(0.3);
    b.anim("intro1_kyrandia", true, 0.0, 39.0);
    b.clear_anim();
    b.wait(0.3);
    b.act("MoveCamera", &[("to", "Vec2(0.0, 112.0)".into()), ("seconds", "3.7".into())]);
    b.wait(0.4);
    b.anim("intro1_shore", false, 0.0, 112.0);
    // The storm, narrated: the CD speaks six recordings over it (there
    // is no text for them anywhere), thunder between, the lightning
    // strike on the shore in the middle of it.
    beats(
        &mut b,
        rt,
        &[
            (0.9, Say("intro.isl0")),
            (1.9, Sfx(22)),
            (4.1, Say("intro.isl1")),
            (4.3, Sfx(22)),
            (5.0, Say("intro.isl2")),
            (6.0, Sfx(37)),
            (6.3, Say("intro.isl3")),
            (7.0, Sfx(23)),
            (7.0, Anim("intro1_destruct", false, 152.0, 160.0)),
            (7.1, Sfx(37)),
            (7.8, Sfx(24)),
            (8.0, Say("intro.isl4")),
            (8.9, Sfx(37)),
            (9.8, Say("intro.isl5")),
        ],
        12.5,
    );
    b.goto("_intro_tree");
    out.push(("_intro.script.ron", b.script("_intro.entry")));

    // The forest: Malcolm's taunts while TREE1 runs its 22.0s, then
    // TREE2 carries straight on. The cue times are the sequence's own
    // print and effect times, scaled 22.0/13.3 - the flat bytecode
    // clock undercounts because the walk-in loops, and the baked
    // animation carries the loops' real length. Effect 38 is in the
    // bytecode but no INTRO.ADL track 38 exists; the original played
    // silence there and so do we, by leaving it out.
    let mut b = Body::new();
    b.music("intro_3");
    b.anim("intro2_tree1", false, 0.0, 16.0);
    beats(
        &mut b,
        rt,
        &[
            (0.0, Sfx(30)),
            (0.0, Sfx(31)),
            (0.2, Sfx(25)),
            (0.3, Say("intro.tree0")),
            (0.5, Sfx(31)),
            (1.0, Sfx(36)),
            (1.0, Sfx(31)),
            (1.2, Sfx(30)),
            (1.3, Say("intro.tree1")),
            (1.7, Sfx(30)),
            (2.0, Sfx(31)),
            (2.6, Sfx(30)),
            (2.8, Sfx(26)),
            (3.1, Sfx(26)),
            (3.1, Say("intro.tree2")),
            (5.0, Sfx(30)),
            (5.8, Say("intro.tree3")),
            (10.6, Sfx(27)),
            (11.2, Say("intro.tree4")),
            (12.9, Say("intro.tree5")),
            (13.1, Say("intro.tree6")),
            (14.9, Sfx(28)),
            (15.0, Say("intro.tree7")),
            (16.0, Say("intro.tree8")),
        ],
        22.0,
    );
    b.anim("intro2_tree2", true, 0.0, 16.0);
    b.wait(0.3);
    b.goto("_intro_kallak");
    out.push(("_intro_tree.script.ron", b.script("_intro_tree.entry")));

    // Kallak writes for 21.6s; the letter lines land at the bytecode's
    // print times, the quill scratches around them.
    let mut b = Body::new();
    b.music("intro_4");
    b.anim("intro3_kallak", false, 0.0, 16.0);
    beats(
        &mut b,
        rt,
        &[
            (0.05, Sfx(29)),
            (0.1, Say("intro.let0")),
            (0.6, Sfx(29)),
            (1.5, Sfx(29)),
            (2.9, Say("intro.let1")),
            (3.3, Sfx(29)),
            (4.4, Sfx(29)),
            (6.1, Say("intro.let2")),
            (6.6, Sfx(29)),
            (7.8, Sfx(29)),
            (10.4, Say("intro.let3")),
            (11.5, Sfx(31)),
            (12.2, Sfx(30)),
        ],
        22.1,
    );
    b.goto("_intro_malcolm");
    out.push(("_intro_kallak.script.ron", b.script("_intro_kallak.entry")));

    // Malcolm and Kallak: the full dialogue in speaking order, the
    // petrification burst right after "Now you are stone".
    let mut b = Body::new();
    b.music("intro_5");
    b.anim("intro4_mal-kal", false, 16.0, 58.0);
    b.sfx(35);
    b.wait(0.5);
    for line in 0..16 {
        let id = format!("intro.mal{line}");
        b.say(&id);
        b.wait(0.25);
        if line == 12 {
            b.sfx(32);
            b.wait(0.2);
            b.sfx(33);
            b.wait(0.2);
            b.sfx(34);
            b.wait(0.5);
        }
    }
    // The parting laugh the CD gives him on his way off the sill.
    b.wait(1.0);
    b.sfx(35);
    b.say("intro.mal16");
    b.wait(7.0);
    b.act("OpenScreen", &[("screen", "\"title\"".into())]);
    out.push(("_intro_malcolm.script.ron", b.script("_intro_malcolm.entry")));

    out
}

fn write(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(path, content) {
        Ok(()) => println!("wrote {}", path.display()),
        Err(err) => println!("{}: {err}", path.display()),
    }
}
