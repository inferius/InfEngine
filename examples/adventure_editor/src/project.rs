//! A6.4: a folder of rooms.
//!
//! A project is a directory holding `rooms/` and whatever the rooms
//! reference. Layer images are stored relative to that root, not to the
//! room file, so a room can sit in `rooms/` while its background sits in
//! `images/` without every path starting with `../`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use infengine_adventure::anim::{self, AnimationSet};
use infengine_adventure::action::Script;
use infengine_adventure::loca::{self, Locales};
use infengine_adventure::script_io;

/// Directory inside a project that holds room files.
pub const ROOMS_DIR: &str = "rooms";
/// Suffix identifying a room file.
pub const ROOM_SUFFIX: &str = ".room.ron";

/// An open project.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub root: PathBuf,
    /// Room files found under `rooms/`, sorted.
    pub rooms: Vec<PathBuf>,
    /// Every animation the project describes.
    ///
    /// Loaded once with the project rather than per room: an animation
    /// sheet says nothing about its own frame count, so without the
    /// index nothing here can be played at all.
    pub animations: AnimationSet,
    /// Every language the project ships.
    pub locales: Locales,
    /// Every script, by id. Scripts are grouped per room on disk but
    /// referenced globally, so one room's hotspot can run another's.
    pub scripts: HashMap<String, Script>,
    /// Script files that would not parse, named so they can be fixed.
    pub script_errors: Vec<String>,
}

impl Project {
    /// Open `root`, listing its rooms.
    ///
    /// Accepts either the project root or the `rooms` directory inside
    /// it, because both are things a person will reasonably drag onto
    /// the tool.
    pub fn open(root: &Path) -> Self {
        let (root, rooms_dir) = if root.file_name().is_some_and(|n| n == ROOMS_DIR) {
            (
                root.parent().unwrap_or(root).to_path_buf(),
                root.to_path_buf(),
            )
        } else {
            (root.to_path_buf(), root.join(ROOMS_DIR))
        };

        let mut rooms: Vec<PathBuf> = fs::read_dir(&rooms_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(ROOM_SUFFIX))
            })
            .collect();
        rooms.sort();
        let animations = anim::load_index(&root.join(anim::ANIMATION_DIR)).unwrap_or_default();
        // The fallback is whichever language sorts first, since a
        // project has no say in the matter until one is configured.
        let locales = loca::load_dir(&root.join(loca::LOCALE_DIR), "").unwrap_or_default();
        let (scripts, errors) = script_io::load_dir(&root.join(script_io::SCRIPT_DIR));
        let script_errors = errors.iter().map(ToString::to_string).collect();
        Self {
            root,
            rooms,
            animations,
            locales,
            scripts,
            script_errors,
        }
    }

    /// Localisation ids belonging to a room, in order.
    ///
    /// The importer keys a room's dialogue as `<room>.<index>`, so the
    /// room's lines are exactly the ids under that prefix. Sorting is
    /// what the zero padding in the index is for.
    pub fn lines_for_room(&self, room: &str, language: &str) -> Vec<String> {
        let Some(locale) = self
            .locales
            .get(language)
            .or_else(|| self.locales.get(&self.locales.fallback))
        else {
            return Vec::new();
        };
        let prefix = format!("{room}.");
        let mut ids: Vec<String> = locale
            .lines
            .keys()
            .filter(|id| id.starts_with(&prefix))
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    /// Resolve a layer's image path against the project root.
    pub fn resolve(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Display name of a room file, without directory or suffix.
    pub fn room_label(path: &Path) -> String {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.trim_end_matches(ROOM_SUFFIX).to_string())
            .unwrap_or_else(|| path.display().to_string())
    }

    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("adventure_editor_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(ROOMS_DIR)).expect("create project");
        for room in ["b_room", "a_room"] {
            fs::write(
                dir.join(ROOMS_DIR).join(format!("{room}{ROOM_SUFFIX}")),
                "Room(id: \"x\", size: (10.0, 10.0))",
            )
            .expect("write room");
        }
        fs::write(dir.join(ROOMS_DIR).join("notes.txt"), "ignore me").expect("write");
        dir
    }

    #[test]
    fn opening_a_project_lists_its_rooms_sorted() {
        let dir = temp_project("list");
        let project = Project::open(&dir);
        assert_eq!(project.rooms.len(), 2, "the .txt must not be listed");
        assert!(
            project.animations.is_empty(),
            "a project with no index has no animations"
        );
        assert!(project.locales.is_empty());
        assert_eq!(Project::room_label(&project.rooms[0]), "a_room");
        assert_eq!(Project::room_label(&project.rooms[1]), "b_room");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_the_rooms_directory_finds_the_root_above_it() {
        let dir = temp_project("rooms_dir");
        let project = Project::open(&dir.join(ROOMS_DIR));
        assert_eq!(project.root, dir);
        assert_eq!(project.rooms.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn images_resolve_against_the_root_not_the_room_file() {
        let dir = temp_project("resolve");
        let project = Project::open(&dir);
        assert_eq!(
            project.resolve("images/brins.png"),
            dir.join("images/brins.png")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_directory_opens_empty_instead_of_failing() {
        let project = Project::open(Path::new("definitely/not/here"));
        assert!(project.is_empty());
    }

    #[test]
    fn a_rooms_lines_are_the_ids_under_its_prefix() {
        use infengine_adventure::loca::{Line, Locale};

        let dir = temp_project("lines");
        let mut english = Locale::new("en", "English");
        english.insert("hall.00", Line::text("first"));
        english.insert("hall.01", Line::text("second"));
        english.insert("crypt.00", Line::text("elsewhere"));
        loca::save_locale(&english, &dir.join(loca::LOCALE_DIR)).expect("save");

        let project = Project::open(&dir);
        assert_eq!(project.lines_for_room("hall", "en"), vec!["hall.00", "hall.01"]);
        assert!(project.lines_for_room("nowhere", "en").is_empty());
        // A language the project does not have falls back rather than
        // showing an empty room.
        assert_eq!(project.lines_for_room("hall", "de").len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn room_label_survives_an_odd_name() {
        assert_eq!(Project::room_label(Path::new("x/y/alchemy.room.ron")), "alchemy");
        assert_eq!(Project::room_label(Path::new("plain.ron")), "plain.ron");
    }
}
