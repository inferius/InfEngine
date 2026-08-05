//! A22.2: collecting a project into something that can be shipped.
//!
//! An export is not a build step so much as an inventory. Every file the
//! game will ask for at run time is named somewhere in the data - a
//! layer's image, an animation's sheet, a line's recording, a script's
//! sound - so the export walks that data, gathers the paths, and copies
//! them.
//!
//! What it will not do is copy the project wholesale. A folder that has
//! been worked in for a year holds abandoned art, half-cut sheets and
//! the odd note to self, and shipping all of it is how a hundred-megabyte
//! download of a small game happens.
//!
//! The missing list matters more than the copy. A path that is written
//! down but not on disk is a crash waiting for whoever plays the export,
//! and it is invisible in the editor because the editor never asked for
//! that file. Listing it is the point.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::loca::{self, LanguageChoice};
use crate::{anim, io, screen, script_io, sound};

/// File naming the game itself, at the project root.
pub const MANIFEST_FILE: &str = "game.ron";

/// What a game is, apart from its rooms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameManifest {
    pub title: String,
    /// Room the game opens in.
    pub start_room: String,
    /// Entry point in that room. Empty means its first.
    #[serde(default)]
    pub start_entry: String,
    /// Bitmap font subtitles draw with. None keeps the system font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Screen shown before the game begins - a title menu. Empty means
    /// the game opens straight into its first room, which is what a
    /// project has before anyone draws a menu.
    #[serde(default)]
    pub start_screen: String,
    /// Room played once when the game is launched, before anything
    /// else: the opening cutscene.
    ///
    /// Separate from `start_room` because they are different moments.
    /// New Game has to reach the first room of the story, and if the
    /// two were the same field it would replay the intro instead -
    /// which is exactly what happened the first time this was tried.
    #[serde(default)]
    pub intro_room: String,
    /// Languages offered, and which is picked by default.
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub default_language: LanguageChoice,
    /// What the player character looks like.
    ///
    /// Optional, because a project has rooms before it has a figure to
    /// walk around them, and drawing an outline is a better answer than
    /// refusing to start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<ActorLook>,
    /// A95: how much of a room the player sees at once, in room pixels.
    ///
    /// The game's own resolution, and the thing a room is measured
    /// against to know whether it scrolls. A background 320x400 in a
    /// 320x200 game is a room the view travels down; without a number
    /// to compare against, "bigger than the screen" has no meaning and
    /// every scrolling room had to be set up by hand.
    #[serde(default = "default_view")]
    pub view: Vec2,
}

/// The resolution these games were drawn for, and a sane default for
/// a project whose manifest predates the field.
fn default_view() -> Vec2 {
    Vec2::new(320.0, 200.0)
}

/// Hand-written rather than derived, because a derived default would
/// give the view zero pixels - and a manifest the editor mints for a
/// project that has none is exactly where that would bite.
impl Default for GameManifest {
    fn default() -> Self {
        Self {
            title: String::new(),
            start_room: String::new(),
            start_entry: String::new(),
            font: None,
            start_screen: String::new(),
            intro_room: String::new(),
            languages: Vec::new(),
            default_language: LanguageChoice::default(),
            actor: None,
            view: default_view(),
        }
    }
}

/// The animations the player character is drawn from.
///
/// Named rather than numbered, and by what the figure is doing rather
/// than by which strip of the sheet they came out of: `walk_left` says
/// what it is for, `brandon_1` says where it was found.
///
/// Only `walk_left` is required to mean anything. A game whose figure
/// has one walk cycle mirrors it, which is what the sprite sheets of
/// this era mostly assume, and a game with no idle animation stands on
/// the first frame of its walk.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActorLook {
    /// Walking towards the left of the screen.
    pub walk_left: String,
    /// Walking towards the right. Empty means mirror `walk_left`.
    #[serde(default)]
    pub walk_right: String,
    /// Walking away from the camera. Empty means use `walk_left`.
    #[serde(default)]
    pub walk_back: String,
    /// Walking towards it. Empty means use `walk_left`.
    #[serde(default)]
    pub walk_front: String,
    /// Standing still. Empty means the first frame of whichever walk
    /// matches the way the figure is facing.
    #[serde(default)]
    pub idle: String,
}

impl ActorLook {
    /// The animation to draw for a figure facing this way.
    ///
    /// Falls back to `walk_left` rather than to nothing: a figure drawn
    /// facing the wrong way is a bug someone can see and fix, and a
    /// figure not drawn at all looks like the room failed to load.
    pub fn walking(&self, facing: crate::interact::Facing) -> &str {
        let (own, other) = self.sides(facing);
        // Whatever exists, in order of how close it is to what was
        // asked for. A figure facing the wrong way is a bug someone can
        // see; a figure not drawn looks like the room failed to load.
        [own, other]
            .into_iter()
            .chain([
                &self.walk_left,
                &self.walk_right,
                &self.walk_front,
                &self.walk_back,
            ])
            .find(|name| !name.is_empty())
            .map(String::as_str)
            .unwrap_or_default()
    }

    /// Whether the figure has to be mirrored to face this way.
    ///
    /// A sheet with one side view is drawn facing one direction, and the
    /// other side is the same frames flipped. That is what an empty
    /// `walk_left` or `walk_right` means, and saying so here keeps the
    /// flip out of the drawing code's guesswork.
    ///
    /// Only the two side views mirror. Flipping a figure walking towards
    /// the camera turns it into a figure walking towards the camera with
    /// its sword in the wrong hand.
    pub fn mirrored(&self, facing: crate::interact::Facing) -> bool {
        use crate::interact::Facing;
        if !matches!(facing, Facing::Left | Facing::Right) {
            return false;
        }
        let (own, other) = self.sides(facing);
        own.is_empty() && !other.is_empty()
    }

    /// The animation for this facing, and the one it falls back to.
    fn sides(&self, facing: crate::interact::Facing) -> (&String, &String) {
        use crate::interact::Facing;
        match facing {
            Facing::Left => (&self.walk_left, &self.walk_right),
            Facing::Right => (&self.walk_right, &self.walk_left),
            Facing::Back => (&self.walk_back, &self.walk_left),
            Facing::Front => (&self.walk_front, &self.walk_left),
        }
    }
}

impl GameManifest {
    pub fn load(root: &Path) -> Option<Self> {
        let text = fs::read_to_string(root.join(MANIFEST_FILE)).ok()?;
        ron::from_str(&text).ok()
    }

    pub fn save(&self, root: &Path) -> Result<(), PackError> {
        let text = ron::ser::to_string_pretty(
            self,
            ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
        )
        .map_err(|err| PackError::Encode(err.to_string()))?;
        let path = root.join(MANIFEST_FILE);
        fs::write(&path, text).map_err(|source| PackError::Write {
            path: path.display().to_string(),
            source,
        })
    }
}

#[derive(Debug)]
pub enum PackError {
    Encode(String),
    Write { path: String, source: std::io::Error },
    Copy { path: String, source: std::io::Error },
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackError::Encode(detail) => write!(f, "{detail}"),
            PackError::Write { path, source } => write!(f, "writing {path}: {source}"),
            PackError::Copy { path, source } => write!(f, "copying {path}: {source}"),
        }
    }
}

impl std::error::Error for PackError {}

/// Everything an export would take, and everything it cannot find.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Project-relative paths that exist and would be copied.
    pub files: BTreeSet<String>,
    /// Paths the data names but that are not on disk.
    pub missing: BTreeSet<String>,
    /// Sound and music ids no index maps to a file.
    ///
    /// Kept apart from `missing` because they fail differently: a
    /// missing file is a path someone wrote down wrongly, an unmapped
    /// id is a sound nobody has supplied yet. The first is a mistake,
    /// the second is work outstanding - and an imported game arrives
    /// with hundreds of the second kind, which would drown the first.
    pub unmapped: BTreeSet<String>,
    /// Total size of the files, in bytes.
    pub bytes: u64,
}

impl Plan {
    pub fn is_shippable(&self) -> bool {
        self.missing.is_empty() && !self.files.is_empty()
    }
}

/// Work out what an export would copy.
pub fn plan(root: &Path) -> Plan {
    let mut wanted: BTreeSet<String> = BTreeSet::new();

    if root.join(MANIFEST_FILE).is_file() {
        wanted.insert(MANIFEST_FILE.to_string());
    }

    // Rooms, and the images their layers draw.
    for path in room_files(root) {
        let Some(relative) = relative_to(root, &path) else {
            continue;
        };
        wanted.insert(relative);
        if let Ok(room) = io::load(&path) {
            for layer in &room.layers {
                wanted.insert(layer.image.clone());
            }
        }
    }

    // Animations, and their sheets.
    let animations = anim::load_index(&root.join(anim::ANIMATION_DIR)).unwrap_or_default();
    if !animations.is_empty() {
        wanted.insert(format!("{}/index.ron", anim::ANIMATION_DIR));
    }
    for animation in animations.iter() {
        wanted.insert(animation.image.clone());
    }

    // Scripts, and the sounds and music they name. Names, not paths:
    // the index below is what turns one into the other.
    let sounds = sound::load_index(&root.join(sound::SOUND_DIR)).unwrap_or_default();
    if !sounds.is_empty() {
        wanted.insert(format!("{}/{}", sound::SOUND_DIR, sound::INDEX_FILE));
    }
    // Everything the index maps, whether or not a script has got round
    // to naming it. The index is a statement that these sounds belong
    // to the game; a soundtrack nothing points at yet is pending work,
    // not dead weight, and the animation index is treated the same way.
    wanted.extend(sounds.sounds.values().cloned());
    let mut unmapped: BTreeSet<String> = BTreeSet::new();
    let (scripts, _) = script_io::load_dir(&root.join(script_io::SCRIPT_DIR));
    for entry in fs::read_dir(root.join(script_io::SCRIPT_DIR)).into_iter().flatten().flatten() {
        if let Some(relative) = relative_to(root, &entry.path())
            && relative.ends_with(script_io::SCRIPT_SUFFIX)
        {
            wanted.insert(relative);
        }
    }
    for script in scripts.values() {
        script.walk(&mut |action| {
            let id = match action {
                Action::PlaySound { sound } => sound,
                Action::PlayMusic { music: Some(track) } => track,
                _ => return,
            };
            match sounds.get(id) {
                Some(file) => {
                    wanted.insert(file.to_string());
                }
                None => {
                    unmapped.insert(id.clone());
                }
            }
        });
    }

    // Screens, and the art they draw. Their text rides along with the
    // locales below, since it is referenced the same way a line is.
    let (screens, _) = screen::load_dir(&root.join(screen::SCREEN_DIR));
    for one in &screens {
        wanted.insert(format!(
            "{}/{}{}",
            screen::SCREEN_DIR,
            one.id,
            screen::SCREEN_SUFFIX
        ));
        wanted.extend(one.images());
    }

    // Languages, and every recording they point at.
    let locales = loca::load_dir(&root.join(loca::LOCALE_DIR), "").unwrap_or_default();
    for language in locales.languages() {
        wanted.insert(format!("{}/{language}{}", loca::LOCALE_DIR, loca::LOCALE_SUFFIX));
        let Some(locale) = locales.get(language) else {
            continue;
        };
        for line in locale.lines.values() {
            if let Some(voice) = &line.voice {
                wanted.insert(voice.clone());
            }
        }
    }

    let mut out = Plan {
        unmapped,
        ..Default::default()
    };
    for relative in wanted {
        let path = root.join(&relative);
        match fs::metadata(&path) {
            Ok(meta) if meta.is_file() => {
                out.bytes += meta.len();
                out.files.insert(relative);
            }
            _ => {
                out.missing.insert(relative);
            }
        }
    }
    out
}

/// Copy a plan's files into `dest`, keeping their layout.
///
/// Missing files are skipped rather than aborting: an export with a
/// hole in it that says where the hole is beats no export at all, and
/// the plan already named them.
pub fn export(root: &Path, dest: &Path, plan: &Plan) -> Result<u64, PackError> {
    let mut copied = 0;
    for relative in &plan.files {
        let from = root.join(relative);
        let to = dest.join(relative);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|source| PackError::Write {
                path: parent.display().to_string(),
                source,
            })?;
        }
        fs::copy(&from, &to).map_err(|source| PackError::Copy {
            path: relative.clone(),
            source,
        })?;
        copied += 1;
    }
    Ok(copied)
}

fn room_files(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(root.join(io::ROOM_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(io::ROOM_SUFFIX))
        })
        .collect();
    paths.sort();
    paths
}

/// A path below `root`, with forward slashes.
///
/// Forward slashes because the paths inside the data use them, and an
/// export written on Windows has to be readable on anything else.
fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    Some(
        rest.components()
            .filter_map(|part| part.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_side_view_is_mirrored_for_the_other_side() {
        use crate::interact::Facing;
        // The sheet faces right, so walking left is the same frames
        // flipped. Naming both sides after one sheet would draw the
        // figure walking left while facing right, which is what the
        // first attempt did.
        let look = ActorLook {
            walk_right: "brandon_side".into(),
            walk_back: "brandon_back".into(),
            walk_front: "brandon_front".into(),
            ..Default::default()
        };
        assert_eq!(look.walking(Facing::Right), "brandon_side");
        assert_eq!(look.walking(Facing::Left), "brandon_side");
        assert!(look.mirrored(Facing::Left));
        assert!(!look.mirrored(Facing::Right));
    }

    #[test]
    fn a_figure_drawn_from_both_sides_is_never_mirrored() {
        use crate::interact::Facing;
        let look = ActorLook {
            walk_left: "left".into(),
            walk_right: "right".into(),
            ..Default::default()
        };
        assert_eq!(look.walking(Facing::Left), "left");
        assert_eq!(look.walking(Facing::Right), "right");
        assert!(!look.mirrored(Facing::Left) && !look.mirrored(Facing::Right));
    }

    #[test]
    fn walking_towards_the_camera_is_never_mirrored() {
        use crate::interact::Facing;
        // Flipping it would hand the character its sword the other way
        // round rather than turn it.
        let look = ActorLook {
            walk_right: "side".into(),
            ..Default::default()
        };
        assert!(!look.mirrored(Facing::Front));
        assert!(!look.mirrored(Facing::Back));
        assert_eq!(look.walking(Facing::Front), "side", "better than nothing");
    }
    use glam::Vec2;

    use crate::action::Script;
    use crate::anim::Animation;
    use crate::loca::{Line, Locale};
    use crate::room::{Layer, LayerKind, Room};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("infengine_pack_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn touch(root: &Path, relative: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        fs::write(&path, b"x").expect("write");
    }

    /// A project with one of everything that carries a file reference.
    fn build(root: &Path) {
        let mut sounds = sound::SoundSet::default();
        sounds.insert("door_opens", "sfx/door.wav");
        sounds.insert("theme", "music/theme.mid");
        sound::save_index(&sounds, &root.join(sound::SOUND_DIR)).expect("sounds");

        let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
        room.layers.push(Layer {
            name: "background".into(),
            image: "images/hall.png".into(),
            offset: Vec2::ZERO,
            kind: LayerKind::Background,
            visible: true,
            shown_if: None,
        });
        io::save(&room, &root.join(io::ROOM_DIR).join("hall.room.ron")).expect("room");
        touch(root, "images/hall.png");

        let mut animations = crate::AnimationSet::default();
        animations.insert(Animation::new(
            "torch",
            "animations/torch.png",
            4,
            Vec2::new(16.0, 16.0),
        ));
        anim::save_index(&animations, &root.join(anim::ANIMATION_DIR)).expect("index");
        touch(root, "animations/torch.png");

        let mut script = Script::new("hall.entry");
        script.push(Action::PlaySound {
            sound: "door_opens".into(),
        });
        script.push(Action::PlayMusic {
            music: Some("theme".into()),
        });
        script_io::save_room("hall", &[script], &root.join(script_io::SCRIPT_DIR))
            .expect("script");
        touch(root, "sfx/door.wav");
        touch(root, "music/theme.mid");

        let mut english = Locale::new("en", "English");
        english.insert(
            "hall.00",
            Line {
                text: "A dusty hall.".into(),
                voice: Some("voice/hall/00.wav".into()),
                ..Default::default()
            },
        );
        loca::save_locale(&english, &root.join(loca::LOCALE_DIR)).expect("locale");
        touch(root, "voice/hall/00.wav");

        GameManifest {
            title: "Test".into(),
            start_room: "hall".into(),
            ..Default::default()
        }
        .save(root)
        .expect("manifest");
    }

    #[test]
    fn a_plan_names_every_file_the_data_points_at() {
        let root = temp("plan");
        build(&root);

        let plan = plan(&root);
        for expected in [
            "game.ron",
            "rooms/hall.room.ron",
            "images/hall.png",
            "animations/index.ron",
            "animations/torch.png",
            "scripts/hall.script.ron",
            "sfx/door.wav",
            "music/theme.mid",
            "loca/en.loca.ron",
            "voice/hall/00.wav",
            "sounds/index.ron",
        ] {
            assert!(plan.files.contains(expected), "missing {expected} from {plan:?}");
        }
        assert!(plan.missing.is_empty(), "{:?}", plan.missing);
        assert!(plan.unmapped.is_empty(), "{:?}", plan.unmapped);
        assert!(plan.is_shippable());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_is_named_but_absent_is_reported_not_ignored() {
        let root = temp("missing");
        build(&root);
        fs::remove_file(root.join("voice/hall/00.wav")).expect("remove");

        let plan = plan(&root);
        assert!(plan.missing.contains("voice/hall/00.wav"));
        assert!(
            !plan.is_shippable(),
            "a hole in the export is not something to ship quietly"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn screens_and_their_art_come_along() {
        let root = temp("screens");
        build(&root);
        let mut title = screen::Screen::new("title", Vec2::new(320.0, 200.0));
        title.widgets.push(screen::Widget {
            id: "backdrop".into(),
            min: Vec2::ZERO,
            max: Vec2::new(320.0, 200.0),
            kind: screen::WidgetKind::Image {
                image: "images/title.png".into(),
            },
            visible: true,
            shown_if: None,
        });
        screen::save(&title, &root.join(screen::SCREEN_DIR)).expect("screen");
        touch(&root, "images/title.png");

        let plan = plan(&root);
        assert!(plan.files.contains("screens/title.screen.ron"));
        assert!(plan.files.contains("images/title.png"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_sound_id_with_no_entry_is_unmapped_not_missing() {
        let root = temp("unmapped");
        build(&root);
        let mut script = Script::new("hall.look");
        script.push(Action::PlaySound {
            sound: "kyra.sfx42".into(),
        });
        script_io::save_room("extra", &[script], &root.join(script_io::SCRIPT_DIR))
            .expect("script");

        let plan = plan(&root);
        assert!(plan.unmapped.contains("kyra.sfx42"));
        assert!(
            !plan.missing.contains("kyra.sfx42"),
            "an id is not a path, and reporting it as one is noise"
        );
        assert!(
            plan.is_shippable(),
            "a sound nobody has supplied is silence, not a broken export"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn everything_the_sound_index_maps_comes_along() {
        let root = temp("sound_index");
        build(&root);
        // Mapped, but no script says its name yet.
        let mut sounds =
            sound::load_index(&root.join(sound::SOUND_DIR)).expect("index");
        sounds.insert("music.title", "music/title.ogg");
        sound::save_index(&sounds, &root.join(sound::SOUND_DIR)).expect("save");
        touch(&root, "music/title.ogg");

        let plan = plan(&root);
        assert!(
            plan.files.contains("music/title.ogg"),
            "a soundtrack nothing points at yet is pending work, not dead weight"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_the_data_does_not_mention_is_taken_along() {
        let root = temp("stray");
        build(&root);
        touch(&root, "images/abandoned_sketch.png");
        touch(&root, "notes.txt");

        let plan = plan(&root);
        assert!(!plan.files.contains("images/abandoned_sketch.png"));
        assert!(!plan.files.contains("notes.txt"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_export_reproduces_the_layout() {
        let root = temp("export");
        build(&root);
        let dest = temp("export_out");

        let plan = plan(&root);
        let copied = export(&root, &dest, &plan).expect("export");
        assert_eq!(copied as usize, plan.files.len());
        assert!(dest.join("voice/hall/00.wav").is_file());
        assert!(dest.join("rooms/hall.room.ron").is_file());
        // And the export is loadable as a project in its own right.
        assert_eq!(plan_files(&dest), plan.files);

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&dest);
    }

    fn plan_files(root: &Path) -> BTreeSet<String> {
        plan(root).files
    }

    #[test]
    fn a_manifest_round_trips() {
        let root = temp("manifest");
        let manifest = GameManifest {
            font: None,
            title: "The Legend".into(),
            start_room: "hall".into(),
            start_entry: "door".into(),
            start_screen: String::new(),
            intro_room: String::new(),
            languages: vec!["en".into(), "cs".into()],
            default_language: LanguageChoice::same("cs"),
            actor: Some(ActorLook {
                walk_right: "brandon_side".into(),
                walk_back: "brandon_back".into(),
                walk_front: "brandon_front".into(),
                ..Default::default()
            }),
            view: Vec2::new(320.0, 200.0),
        };
        manifest.save(&root).expect("save");
        assert_eq!(GameManifest::load(&root), Some(manifest));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_manifest_written_before_the_window_existed_gets_the_usual_one() {
        // A95: without a default, an older game.ron would load with a
        // zero-pixel window and every room would count as oversized.
        let root = temp("manifest_old");
        fs::create_dir_all(&root).expect("dir");
        fs::write(
            root.join(MANIFEST_FILE),
            r#"(title: "Old", start_room: "hall")"#,
        )
        .expect("write");
        let manifest = GameManifest::load(&root).expect("loads");
        assert_eq!(manifest.view, Vec2::new(320.0, 200.0));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_project_with_nothing_in_it_is_not_shippable() {
        let root = temp("empty");
        let plan = plan(&root);
        assert!(plan.files.is_empty());
        assert!(!plan.is_shippable());

        let _ = fs::remove_dir_all(&root);
    }
}
