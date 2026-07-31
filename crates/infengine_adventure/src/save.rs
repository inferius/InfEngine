//! A28.1: saved games.
//!
//! A slot holds a [`GameState`] and enough about it to draw a menu
//! entry: which game it belongs to, which room the player was in, and
//! when it was written.
//!
//! Two decisions worth stating, because both are the kind that only
//! hurt later:
//!
//! The directory is a parameter, never derived from the project. A
//! shipped game is often installed read-only, and writing saves next to
//! the artwork is how a game works on the developer's machine and
//! nowhere else. The host picks somewhere writable; this only writes
//! where it is told.
//!
//! Each slot records the title of the game that wrote it. Loading a save
//! from a different game would otherwise drop the player into a room id
//! that happens to match, or none at all, and the failure would look
//! like a corrupt save rather than the wrong one.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::game::GameState;

/// Conventional directory name for saves under a writable root.
pub const SAVE_DIR: &str = "saves";
/// Suffix identifying a save file.
pub const SAVE_SUFFIX: &str = ".save.ron";

/// Format version. Bumped when the shape of a save changes in a way an
/// older reader would misread rather than reject.
pub const VERSION: u32 = 1;

/// A saved game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Save {
    #[serde(default)]
    pub version: u32,
    /// Title from the game manifest, so a save can say what it belongs to.
    #[serde(default)]
    pub game: String,
    /// What the player called it, or an automatic name.
    #[serde(default)]
    pub name: String,
    /// Seconds since the Unix epoch, or zero when the clock was unusable.
    #[serde(default)]
    pub saved_at: u64,
    pub state: GameState,
}

impl Save {
    pub fn new(game: impl Into<String>, name: impl Into<String>, state: GameState) -> Self {
        Self {
            version: VERSION,
            game: game.into(),
            name: name.into(),
            saved_at: now(),
            state,
        }
    }

    /// Room the player was in, for the menu line.
    pub fn room(&self) -> &str {
        &self.state.room
    }
}

/// Seconds since the epoch, or zero if the clock is before it.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What a menu needs to list a slot without loading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    pub slot: u32,
    pub name: String,
    pub game: String,
    pub room: String,
    pub saved_at: u64,
}

#[derive(Debug)]
pub enum SaveError {
    Io { path: String, source: std::io::Error },
    Parse { path: String, detail: String },
    /// Written by a newer version of the game than this one can read.
    Newer { path: String, version: u32 },
    /// Written by a different game.
    WrongGame { path: String, game: String },
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io { path, source } => write!(f, "{path}: {source}"),
            SaveError::Parse { path, detail } => write!(f, "{path}: {detail}"),
            SaveError::Newer { path, version } => write!(
                f,
                "{path} was written by version {version}; this game reads {VERSION}"
            ),
            SaveError::WrongGame { path, game } => {
                write!(f, "{path} is a saved game of {game}")
            }
        }
    }
}

impl std::error::Error for SaveError {}

/// Path of a slot inside `dir`.
///
/// Zero padded so a directory listing sorts the way a menu does.
pub fn slot_path(dir: &Path, slot: u32) -> PathBuf {
    dir.join(format!("slot{slot:03}{SAVE_SUFFIX}"))
}

/// Write a slot.
pub fn write(dir: &Path, slot: u32, save: &Save) -> Result<PathBuf, SaveError> {
    let text = ron::ser::to_string_pretty(
        save,
        ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
    )
    .map_err(|err| SaveError::Parse {
        path: format!("slot {slot}"),
        detail: err.to_string(),
    })?;
    fs::create_dir_all(dir).map_err(|source| SaveError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let path = slot_path(dir, slot);
    fs::write(&path, text).map_err(|source| SaveError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(path)
}

/// Read a slot.
///
/// `game` is the title this game expects; pass an empty string to accept
/// whatever is there, which is what the editor does when it is testing
/// somebody else's project.
pub fn read(dir: &Path, slot: u32, game: &str) -> Result<Save, SaveError> {
    let path = slot_path(dir, slot);
    let origin = path.display().to_string();
    let text = fs::read_to_string(&path).map_err(|source| SaveError::Io {
        path: origin.clone(),
        source,
    })?;
    let save: Save = ron::from_str(&text).map_err(|err| SaveError::Parse {
        path: origin.clone(),
        detail: err.to_string(),
    })?;
    if save.version > VERSION {
        return Err(SaveError::Newer {
            path: origin,
            version: save.version,
        });
    }
    if !game.is_empty() && !save.game.is_empty() && save.game != game {
        return Err(SaveError::WrongGame {
            path: origin,
            game: save.game,
        });
    }
    Ok(save)
}

/// Every slot in `dir`, by slot number.
///
/// A file that will not parse is left out rather than failing the whole
/// listing: one damaged save must not make the load menu unopenable.
pub fn list(dir: &Path) -> Vec<SlotInfo> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut slots: Vec<SlotInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let number = name
                .strip_prefix("slot")?
                .strip_suffix(SAVE_SUFFIX)?
                .parse()
                .ok()?;
            let text = fs::read_to_string(&path).ok()?;
            let save: Save = ron::from_str(&text).ok()?;
            Some(SlotInfo {
                slot: number,
                name: save.name.clone(),
                game: save.game.clone(),
                room: save.state.room.clone(),
                saved_at: save.saved_at,
            })
        })
        .collect();
    slots.sort_by_key(|info| info.slot);
    slots
}

/// Remove a slot. Missing is not an error - the slot is gone either way.
pub fn remove(dir: &Path, slot: u32) -> Result<(), SaveError> {
    let path = slot_path(dir, slot);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SaveError::Io {
            path: path.display().to_string(),
            source,
        }),
    }
}

/// Lowest slot number not in use.
pub fn next_free(dir: &Path) -> u32 {
    let used: Vec<u32> = list(dir).into_iter().map(|info| info.slot).collect();
    (0..).find(|slot| !used.contains(slot)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    use crate::room::Room;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("infengine_save_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn state() -> GameState {
        let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
        room.walk.solids.push(crate::walk::Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        let mut state = GameState::start(&room, None);
        state.set_flag("met_brynn", true);
        state.give("amulet");
        state
    }

    #[test]
    fn a_save_round_trips() {
        let dir = temp("round_trip");
        let save = Save::new("The Legend", "before the bridge", state());
        write(&dir, 3, &save).expect("write");

        let back = read(&dir, 3, "The Legend").expect("read");
        assert_eq!(back, save);
        assert_eq!(back.room(), "hall");
        assert!(back.state.flag("met_brynn"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_from_another_game_is_refused_by_name() {
        let dir = temp("wrong_game");
        write(&dir, 0, &Save::new("Some Other Game", "x", state())).expect("write");

        let err = read(&dir, 0, "The Legend").expect_err("refused");
        assert!(
            err.to_string().contains("Some Other Game"),
            "the error has to say whose save it is: {err}"
        );
        // The editor testing somebody else's project takes what it finds.
        assert!(read(&dir, 0, "").is_ok());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_from_the_future_is_refused_rather_than_half_read() {
        let dir = temp("newer");
        let mut save = Save::new("The Legend", "x", state());
        save.version = VERSION + 1;
        write(&dir, 0, &save).expect("write");

        let err = read(&dir, 0, "The Legend").expect_err("refused");
        assert!(err.to_string().contains(&(VERSION + 1).to_string()), "{err}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn listing_sorts_by_slot_and_survives_a_damaged_file() {
        let dir = temp("listing");
        write(&dir, 5, &Save::new("g", "fifth", state())).expect("write");
        write(&dir, 1, &Save::new("g", "first", state())).expect("write");
        fs::write(dir.join("slot009.save.ron"), "not ron").expect("write");

        let slots = list(&dir);
        assert_eq!(
            slots.iter().map(|s| s.slot).collect::<Vec<_>>(),
            vec![1, 5],
            "a damaged save must not make the menu unopenable"
        );
        assert_eq!(slots[0].name, "first");
        assert_eq!(slots[1].room, "hall");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_next_free_slot_fills_gaps() {
        let dir = temp("next_free");
        assert_eq!(next_free(&dir), 0, "an empty directory starts at zero");
        write(&dir, 0, &Save::new("g", "a", state())).expect("write");
        write(&dir, 2, &Save::new("g", "b", state())).expect("write");
        assert_eq!(next_free(&dir), 1, "the gap is reused before the end");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_a_slot_that_is_not_there_is_not_a_failure() {
        let dir = temp("remove");
        remove(&dir, 7).expect("gone either way");
        write(&dir, 7, &Save::new("g", "a", state())).expect("write");
        remove(&dir, 7).expect("removed");
        assert!(list(&dir).is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn slots_are_padded_so_a_listing_sorts_like_a_menu() {
        let dir = Path::new("saves");
        assert_eq!(slot_path(dir, 2), dir.join("slot002.save.ron"));
        assert_eq!(slot_path(dir, 10), dir.join("slot010.save.ron"));
        let mut names = [
            slot_path(dir, 10).display().to_string(),
            slot_path(dir, 2).display().to_string(),
        ];
        names.sort();
        assert!(names[0].contains("002"), "two sorts before ten");
    }

    #[test]
    fn a_save_carries_a_time_that_moves_forward() {
        let save = Save::new("g", "a", state());
        assert!(save.saved_at > 1_600_000_000, "a plausible epoch second");
    }
}
