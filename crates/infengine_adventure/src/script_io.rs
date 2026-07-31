//! A20.1: reading and writing a project's scripts.
//!
//! Scripts are grouped one file per room - `scripts/<room>.script.ron`
//! holding a list of [`Script`] - because that is the unit an author
//! works on and the unit a version-control diff should stay small for.
//! Ids are global all the same, so a script can be shared: the file is
//! where it lives, not what it belongs to.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::action::Script;

/// Directory holding the scripts, relative to the project root.
pub const SCRIPT_DIR: &str = "scripts";

/// Suffix identifying a script file.
pub const SCRIPT_SUFFIX: &str = ".script.ron";

#[derive(Debug)]
pub enum ScriptIoError {
    Read { path: String, source: std::io::Error },
    Parse { path: String, detail: String },
}

impl std::fmt::Display for ScriptIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptIoError::Read { path, source } => write!(f, "{path}: {source}"),
            ScriptIoError::Parse { path, detail } => write!(f, "{path}: {detail}"),
        }
    }
}

impl std::error::Error for ScriptIoError {}

/// Parse one file's worth of scripts.
pub fn from_string(text: &str, origin: &str) -> Result<Vec<Script>, ScriptIoError> {
    ron::from_str(text).map_err(|err| ScriptIoError::Parse {
        path: origin.to_string(),
        detail: err.to_string(),
    })
}

pub fn to_string(scripts: &[Script]) -> Result<String, ScriptIoError> {
    ron::ser::to_string_pretty(
        &scripts,
        ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
    )
    .map_err(|err| ScriptIoError::Parse {
        path: "<memory>".to_string(),
        detail: err.to_string(),
    })
}

/// Load every script in a directory, keyed by id.
///
/// A file that will not parse is reported and the rest are kept: one
/// broken room must not cost the editor every other room's logic.
/// Returns the scripts and whatever went wrong.
pub fn load_dir(dir: &Path) -> (HashMap<String, Script>, Vec<ScriptIoError>) {
    let mut scripts = HashMap::new();
    let mut errors = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) => {
            // A project with no scripts yet is not an error worth
            // reporting - it is what a new project looks like.
            if source.kind() != std::io::ErrorKind::NotFound {
                errors.push(ScriptIoError::Read {
                    path: dir.display().to_string(),
                    source,
                });
            }
            return (scripts, errors);
        }
    };

    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(SCRIPT_SUFFIX))
        })
        .collect();
    paths.sort();

    for path in paths {
        let origin = path.display().to_string();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) => {
                errors.push(ScriptIoError::Read { path: origin, source });
                continue;
            }
        };
        match from_string(&text, &origin) {
            Ok(loaded) => {
                for script in loaded {
                    scripts.insert(script.id.clone(), script);
                }
            }
            Err(err) => errors.push(err),
        }
    }
    (scripts, errors)
}

/// Write one room's scripts.
pub fn save_room(room: &str, scripts: &[Script], dir: &Path) -> Result<(), ScriptIoError> {
    let text = to_string(scripts)?;
    let path = dir.join(format!("{room}{SCRIPT_SUFFIX}"));
    fs::create_dir_all(dir).map_err(|source| ScriptIoError::Read {
        path: dir.display().to_string(),
        source,
    })?;
    fs::write(&path, text).map_err(|source| ScriptIoError::Read {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("infengine_scripts_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn scripts_round_trip_through_a_directory() {
        let dir = temp_dir("round_trip");
        let mut look = Script::new("hall.painting.look");
        look.push(Action::Say {
            line: "hall.00".into(),
            speaker: None,
        });
        let mut entry = Script::new("hall.entry");
        entry.push(Action::SetFlag {
            flag: "arrived".into(),
            value: true,
        });

        save_room("hall", &[look.clone(), entry.clone()], &dir).expect("save");
        let (scripts, errors) = load_dir(&dir);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(scripts.get("hall.painting.look"), Some(&look));
        assert_eq!(scripts.get("hall.entry"), Some(&entry));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_broken_file_does_not_cost_the_others() {
        let dir = temp_dir("broken");
        save_room("good", &[Script::new("good.entry")], &dir).expect("save");
        fs::write(dir.join("bad.script.ron"), "this is not ron").expect("write");

        let (scripts, errors) = load_dir(&dir);
        assert!(scripts.contains_key("good.entry"));
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].to_string().contains("bad.script.ron"),
            "the error names the file: {}",
            errors[0]
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_scripts_is_not_an_error() {
        let dir = std::env::temp_dir().join("infengine_scripts_missing");
        let _ = fs::remove_dir_all(&dir);
        let (scripts, errors) = load_dir(&dir);
        assert!(scripts.is_empty());
        assert!(errors.is_empty(), "a new project has no scripts yet");
    }
}
