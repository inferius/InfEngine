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
    (18, "Sasek Malcolm se osvobodil!"),
    (19, "Ovladl Kyragem..."),
    (20, "...zdroj vsi magie Kyrandie!"),
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
    // The story screen between the storm and the forest. Its would-be
    // recordings (16..18 by the global voc = string - 2/3 rule) are
    // the ones INTRO.VRM does not have: the narration was never
    // recorded, which is why the talkie skips the screen in
    // voice-only mode. Text and six seconds, as the code says.
    ("intro.story0", 18, None),
    ("intro.story1", 19, None),
    ("intro.story2", 20, None),
];

/// The talkie executable: every cue in this file comes off its
/// sequences, because the voiced intro is the one being rebuilt.
const CD_EXE: &str = "C:/tmp/The_Legend_Of_Kyrandia_1992_1.3_04022026_2013/kyrandia1-cd-gog-scummvm-cestina_v1.3/CD/EXE/MAIN.EXE";

/// The window dialogue in the sequence's own print order.
const MAL_LINES: &[&str] = &[
    "intro.mal0", "intro.mal1", "intro.mal2", "intro.mal3", "intro.mal4", "intro.mal5",
    "intro.mal6", "intro.mal7", "intro.mal8", "intro.mal9", "intro.mal10", "intro.mal11",
    "intro.mal12", "intro.mal13", "intro.mal14", "intro.mal15",
];

/// The forest lines in the sequence's own print order.
const TREE_LINES: &[&str] = &[
    "intro.tree0", "intro.tree1", "intro.tree2", "intro.tree3", "intro.tree4",
    "intro.tree5", "intro.tree6", "intro.tree7", "intro.tree8",
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


    for (name, content) in rooms() {
        write(&project.join("rooms").join(name), &content);
    }
    // Every scene on the talkie's own clock, loops executed: the
    // intro we reproduce is the voiced one, and its sequences are
    // paced around the speech. Nothing here is carried over from the
    // floppy or scaled - each cue is the tick the interpreter fires it
    // on, and every animation was baked from these same streams.
    use infengine_advimport::kyra::seqtime::{self, Did};
    let Ok(cd) = std::fs::read(CD_EXE) else {
        println!("no talkie executable at {CD_EXE}");
        return;
    };
    let secs = |tick: u32| tick as f32 / 60.0;
    let scene = |at: usize| seqtime::expand_cd(&cd, at);
    let ends = |played: &[(u32, Did)], pad: f32| secs(seqtime::total_ticks(played)) + pad;
    // Where a movie slot first shows anything: when one animation
    // hands the scene over to the next.
    let starts_at = |played: &[(u32, Did)], slot: u8| -> Option<f32> {
        played.iter().find_map(|(tick, did)| match did {
            Did::Frame { slot: s, .. } | Did::Next { slot: s } | Did::Prev { slot: s }
                if *s == slot =>
            {
                Some(secs(*tick))
            }
            _ => None,
        })
    };
    // A scene's spoken lines and effects, in the order the stream
    // fires them; the caller pairs the n-th line with its own table.
    let cues_of = |played: &[(u32, Did)], skip_effect: &[u8]| -> Vec<(f32, ForestCue)> {
        let mut out = Vec::new();
        let mut line = 0usize;
        for (tick, did) in played {
            match did {
                Did::Effect(n) if !skip_effect.contains(n) => {
                    out.push((secs(*tick), ForestCue::Sfx(*n)));
                }
                Did::Talk(_) => {
                    out.push((secs(*tick), ForestCue::Say(line)));
                    line += 1;
                }
                _ => {}
            }
        }
        out
    };

    // Effect 38 has no INTRO.ADL track: the original played silence
    // there, and so do we, by leaving it out.
    let forest_played = scene(0x30894);
    let forest = cues_of(&forest_played, &[38]);
    let forest_end = ends(&forest_played, 0.0);
    let tree2_at = starts_at(&forest_played, 2).unwrap_or(forest_end);
    println!(
        "forest: {} cues over {forest_end:.1}s, TREE2 takes over at {tree2_at:.1}s",
        forest.len()
    );

    let island_played = scene(0x30E6D);
    let island: Vec<(f32, u8)> = island_played
        .iter()
        .filter_map(|(t, d)| match d {
            Did::Effect(n) => Some((secs(*t), *n)),
            _ => None,
        })
        .collect();
    let island_end = ends(&island_played, 1.0);
    let island_voc: Vec<(f32, usize)> = island_played
        .iter()
        .filter_map(|(t, d)| match d {
            Did::Voc(v @ 55..=60) => Some((secs(*t), (*v - 55) as usize)),
            _ => None,
        })
        .collect();
    println!(
        "island: {} thunder, {} narration lines over {island_end:.1}s",
        island.len(),
        island_voc.len()
    );

    // Kallak writes: the letter has no subtitles at all - the words
    // appear on the parchment - so its lines ride their recordings.
    let letter_played = scene(0x3008F);
    let letter_sfx: Vec<(f32, u8)> = letter_played
        .iter()
        .filter_map(|(t, d)| match d {
            Did::Effect(n) => Some((secs(*t), *n)),
            _ => None,
        })
        .collect();
    let letter_voc: Vec<(f32, usize)> = letter_played
        .iter()
        .filter_map(|(t, d)| match d {
            Did::Voc(v @ 43..=46) => Some((secs(*t), (*v - 43) as usize)),
            _ => None,
        })
        .collect();
    let letter_end = ends(&letter_played, 0.5);
    println!(
        "letter: {} scratches, {} lines over {letter_end:.1}s",
        letter_sfx.len(),
        letter_voc.len()
    );

    let hut_played = scene(0x30BB5);
    let hut = cues_of(&hut_played, &[]);
    let hut_end = ends(&hut_played, 0.5);
    let hut_laugh = hut_played.iter().find_map(|(t, d)| match d {
        Did::Voc(28) => Some(secs(*t)),
        _ => None,
    });
    println!(
        "hut: {} cues over {hut_end:.1}s, laugh at {hut_laugh:?}",
        hut.len()
    );

    let cues = Cues {
        island,
        island_voc,
        island_end,
        forest,
        forest_end,
        tree2_at,
        letter_sfx,
        letter_voc,
        letter_end,
        hut,
        hut_laugh,
        hut_end,
    };

    for (name, content) in scripts(&cues) {
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
        // The intro narrates over the action the way the sequences do:
        // speech never stands the scene still, the cue times are the
        // pacing.
        self.act(
            "Say",
            &[("line", format!("\"{line}\"")), ("wait", "false".into())],
        );
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
    /// Hand the scene to another animation, without waiting on it.
    Anim(&'a str, f32, f32),
}

fn beats(body: &mut Body, steps: &[(f32, Step)], tail: f32) {
    let mut clock = 0.0f32;
    for (at, step) in steps {
        if *at > clock {
            body.wait(*at - clock);
            clock = *at;
        }
        match step {
            Step::Sfx(n) => body.sfx(*n),
            Step::Say(line) => body.say(line),
            Step::Anim(id, x, y) => body.anim(id, false, *x, *y),
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
        // The story scenes are the original's whole 320x200 screen -
        // it draws them at y 16 and 58 with the rest left black, and
        // that black is part of the picture. Showing only the top 144
        // rows cropped the scene and left a band of the backdrop's own
        // black above it, which reads as a picture pushed off centre.
        (
            "_intro_tree.room.ron",
            plain("_intro_tree", Some("pictures/tree.png"), (320.0, 200.0), (320.0, 200.0)),
        ),
        (
            "_intro_kallak.room.ron",
            plain("_intro_kallak", None, (320.0, 200.0), (320.0, 200.0)),
        ),
        (
            "_intro_malcolm.room.ron",
            plain("_intro_malcolm", Some("pictures/mal_kal.png"), (320.0, 200.0), (320.0, 200.0)),
        ),
    ]
}

/// One scene moment: an effect track, or the n-th spoken line.
enum ForestCue {
    Sfx(u8),
    Say(usize),
}

/// Every scene's cues, all read off the executables' own clocks.
struct Cues {
    island: Vec<(f32, u8)>,
    island_voc: Vec<(f32, usize)>,
    island_end: f32,
    forest: Vec<(f32, ForestCue)>,
    forest_end: f32,
    tree2_at: f32,
    letter_sfx: Vec<(f32, u8)>,
    letter_voc: Vec<(f32, usize)>,
    letter_end: f32,
    hut: Vec<(f32, ForestCue)>,
    hut_laugh: Option<f32>,
    hut_end: f32,
}

fn scripts(cues: &Cues) -> Vec<(&'static str, String)> {
    use Step::{Say, Sfx};
    let mut out = Vec::new();

    // Logos on the strip, the pan, the island storm. Placements and
    // cue times are the measured ones (see kyra_seq / kyra_intro_fit).
    let mut b = Body::new();
    b.music("intro_2");
    b.wait(0.5);
    b.anim("intro1_westwood", true, 0.0, 39.0);
    b.clear_anim();
    // One second between the logos, then the pan right after the
    // second one: delay(60 ticks) and the scroll loop, verbatim from
    // the interpreter.
    b.wait(1.0);
    b.anim("intro1_kyrandia", true, 0.0, 39.0);
    b.clear_anim();
    // The scroll is one row every two ticks, 112 rows: 3.73 seconds,
    // from the interpreter's own arithmetic.
    b.act("MoveCamera", &[("to", "Vec2(0.0, 112.0)".into()), ("seconds", "3.73".into())]);
    b.wait(3.73);
    // The storm, narrated. The waves are the room's own ambient; the
    // lightning is one animation whose baked timing carries the ten
    // calm seconds and the holds between strikes, so it starts at
    // zero and the island stays whole until the bolts have earned the
    // clearing. Thunder lands on the sequence's own expanded clock;
    // the narrator (six recordings with no text anywhere) reads over
    // the calm stretch, as on the CD.
    b.anim("intro1_destruct", false, 152.0, 160.0);
    let mut storm: Vec<(f32, Step)> = cues
        .island
        .iter()
        .map(|(seconds, effect)| (*seconds, Sfx(u32::from(*effect))))
        .collect();
    storm.extend(cues.island_voc.iter().filter_map(|(seconds, line)| {
        ISLAND.get(*line).map(|(id, _)| (*seconds, Say(id)))
    }));
    storm.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    beats(&mut b, &storm, cues.island_end);
    // No story screen: the talkie skips TEXT.CPS whenever speech is on
    // (the narrator over the storm tells the same story), and so do
    // we. The lines stay in the locale for a text-only mode someday.
    b.goto("_intro_tree");
    out.push(("_intro.script.ron", b.script("_intro.entry")));

    // The forest: Malcolm's taunts and the effects at the ticks the
    // sequence's own loops put them on, while TREE1 runs. The talk
    // order is the sequence's print order, which is what the tree0..8
    // ids were named after.
    let mut b = Body::new();
    b.music("intro_3");
    b.anim("intro2_tree1", false, 0.0, 16.0);
    let steps: Vec<(f32, Step)> = cues
        .forest
        .iter()
        .filter_map(|(at, cue)| match cue {
            ForestCue::Sfx(n) => Some((*at, Sfx(u32::from(*n)))),
            ForestCue::Say(index) => TREE_LINES.get(*index).map(|line| (*at, Say(line))),
        })
        .collect();
    // TREE2 takes the scene over where the stream hands it across -
    // not after TREE1 has held its last frame for the eleven seconds
    // the rest of the dialogue takes.
    let mut steps = steps;
    steps.push((cues.tree2_at, Step::Anim("intro2_tree2", 0.0, 16.0)));
    steps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    beats(&mut b, &steps, cues.forest_end);
    b.goto("_intro_kallak");
    out.push(("_intro_tree.script.ron", b.script("_intro_tree.entry")));

    // Kallak writes: the quill scratches on the sequence's own clock,
    // the four spoken lines where the CD puts their recordings,
    // carried over by those same scratches.
    let mut b = Body::new();
    b.music("intro_4");
    b.anim("intro3_kallak", false, 0.0, 16.0);
    const LETTER_LINES: [&str; 4] = ["intro.let0", "intro.let1", "intro.let2", "intro.let3"];
    let mut letter: Vec<(f32, Step)> = cues
        .letter_sfx
        .iter()
        .map(|(at, n)| (*at, Sfx(u32::from(*n))))
        .collect();
    letter.extend(cues.letter_voc.iter().filter_map(|(at, line)| {
        LETTER_LINES.get(*line).map(|id| (*at, Say(id)))
    }));
    letter.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    beats(&mut b, &letter, cues.letter_end);
    b.goto("_intro_malcolm");
    out.push(("_intro_kallak.script.ron", b.script("_intro_kallak.entry")));

    // Malcolm and Kallak: every line on its print tick, the
    // petrification burst where the bytecode fires it, and the parting
    // laugh where the CD's clock puts recording 28.
    let mut b = Body::new();
    b.music("intro_5");
    b.anim("intro4_mal-kal", false, 16.0, 58.0);
    let mut window: Vec<(f32, Step)> = cues
        .hut
        .iter()
        .filter_map(|(at, cue)| match cue {
            ForestCue::Sfx(n) => Some((*at, Sfx(u32::from(*n)))),
            ForestCue::Say(index) => {
                MAL_LINES.get(*index).map(|line| (*at, Say(line)))
            }
        })
        .collect();
    if let Some(at) = cues.hut_laugh {
        window.push((at, Say("intro.mal16")));
    }
    window.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    beats(&mut b, &window, cues.hut_end);
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
