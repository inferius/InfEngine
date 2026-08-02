//! A22.3: what a sound id points at.
//!
//! Scripts name sounds and music by id, never by path - the same reason
//! dialogue is keyed by localisation id. A script that says
//! `PlaySound { sound: "door_opens" }` keeps working when the recording
//! is replaced, remastered or moved, and it stays readable.
//!
//! The index is where an id becomes a file. It is also where the gap
//! shows: an imported game arrives with ids its original resolved in
//! code, and until they are mapped they are silence. Listing them
//! unmapped is better than inventing a path that does not exist.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Directory holding the sound index, relative to the project root.
pub const SOUND_DIR: &str = "sounds";
/// File inside it.
pub const INDEX_FILE: &str = "index.ron";

/// Every sound the project knows, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundSet {
    /// Id to project-relative file path.
    #[serde(default)]
    pub sounds: BTreeMap<String, String>,
}

impl SoundSet {
    pub fn insert(&mut self, id: impl Into<String>, file: impl Into<String>) {
        self.sounds.insert(id.into(), file.into());
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.sounds.get(id).map(String::as_str)
    }

    /// Every id, sorted - which a BTreeMap already is.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.sounds.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    /// Ids whose file is not where the index says.
    pub fn missing_files(&self, root: &Path) -> Vec<&str> {
        self.sounds
            .iter()
            .filter(|(_, file)| !root.join(file).is_file())
            .map(|(id, _)| id.as_str())
            .collect()
    }
}

#[derive(Debug)]
pub enum SoundIoError {
    Read { path: String, source: std::io::Error },
    Parse { path: String, detail: String },
}

impl std::fmt::Display for SoundIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoundIoError::Read { path, source } => write!(f, "{path}: {source}"),
            SoundIoError::Parse { path, detail } => write!(f, "{path}: {detail}"),
        }
    }
}

impl std::error::Error for SoundIoError {}

/// Read the index, or an empty set if the project has none yet.
pub fn load_index(dir: &Path) -> Result<SoundSet, SoundIoError> {
    let path = dir.join(INDEX_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SoundSet::default());
        }
        Err(source) => {
            return Err(SoundIoError::Read {
                path: path.display().to_string(),
                source,
            });
        }
    };
    ron::from_str(&text).map_err(|err| SoundIoError::Parse {
        path: path.display().to_string(),
        detail: err.to_string(),
    })
}

pub fn save_index(set: &SoundSet, dir: &Path) -> Result<(), SoundIoError> {
    let text = ron::ser::to_string_pretty(
        set,
        ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
    )
    .map_err(|err| SoundIoError::Parse {
        path: INDEX_FILE.to_string(),
        detail: err.to_string(),
    })?;
    fs::create_dir_all(dir).map_err(|source| SoundIoError::Read {
        path: dir.display().to_string(),
        source,
    })?;
    let path = dir.join(INDEX_FILE);
    fs::write(&path, text).map_err(|source| SoundIoError::Read {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("infengine_sound_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn an_index_round_trips() {
        let dir = temp("round_trip");
        let mut set = SoundSet::default();
        set.insert("door_opens", "sfx/door.wav");
        set.insert("theme", "music/theme.mid");

        save_index(&set, &dir).expect("save");
        assert_eq!(load_index(&dir).expect("load"), set);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_index_has_no_sounds_rather_than_an_error() {
        let dir = temp("absent");
        let _ = fs::remove_dir_all(&dir);
        let set = load_index(&dir).expect("an absent index is not a failure");
        assert!(set.is_empty());
    }

    #[test]
    fn an_id_whose_file_is_gone_is_named() {
        let dir = temp("gone");
        fs::create_dir_all(dir.join("sfx")).expect("dirs");
        fs::write(dir.join("sfx/door.wav"), b"x").expect("write");

        let mut set = SoundSet::default();
        set.insert("door_opens", "sfx/door.wav");
        set.insert("window_breaks", "sfx/window.wav");
        assert_eq!(set.missing_files(&dir), vec!["window_breaks"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_index_names_itself_rather_than_reading_as_empty() {
        let dir = temp("broken");
        fs::write(dir.join(INDEX_FILE), "not ron at all").expect("write");
        let err = load_index(&dir).expect_err("a broken index is an error");
        assert!(err.to_string().contains(INDEX_FILE), "{err}");

        let _ = fs::remove_dir_all(&dir);
    }
}
