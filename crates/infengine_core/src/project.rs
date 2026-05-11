//! Project metadata: a manifest grouping multiple scenes under one
//! content directory.
//!
//! A `project.ron` file lives at the project root. Editor tooling opens
//! that file, reads the asset root + scene list, and lets the user pick
//! which scene to edit.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One scene entry inside a project's scene list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectScene {
    /// Short display name shown in the editor's scene picker.
    pub name: String,
    /// Path to the `.ron` scene file, relative to the project root.
    pub path: PathBuf,
}

/// Top-level project manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    /// Asset root relative to the project root. Defaults to `"assets"`.
    #[serde(default = "default_asset_root")]
    pub asset_root: PathBuf,
    /// Ordered list of scenes; index 0 is the default.
    #[serde(default)]
    pub scenes: Vec<ProjectScene>,
}

fn default_asset_root() -> PathBuf {
    PathBuf::from("assets")
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "Untitled".to_owned(),
            asset_root: default_asset_root(),
            scenes: Vec::new(),
        }
    }
}

/// Errors raised by project load / save.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("project file {path} not found: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("project file {path} parse error: {source}")]
    Parse {
        path: PathBuf,
        source: ron::error::SpannedError,
    },
    #[error("project file {path} serialize error: {source}")]
    Serialize {
        path: PathBuf,
        source: ron::error::Error,
    },
}

impl Project {
    /// Load a project manifest from `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| ProjectError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        ron::de::from_str(&text).map_err(|source| ProjectError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Save the project manifest to `path` with pretty formatting.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        let path = path.as_ref();
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(
            |source| ProjectError::Serialize {
                path: path.to_path_buf(),
                source,
            },
        )?;
        std::fs::write(path, text).map_err(|source| ProjectError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Resolve an entry's path against the supplied project root.
    pub fn scene_full_path(&self, root: &Path, scene: &ProjectScene) -> PathBuf {
        root.join(&scene.path)
    }

    /// Asset root joined with the project root.
    pub fn asset_root_full(&self, root: &Path) -> PathBuf {
        root.join(&self.asset_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_serde() {
        let p = Project {
            name: "demo".to_owned(),
            asset_root: "assets".into(),
            scenes: vec![ProjectScene {
                name: "main".to_owned(),
                path: "scene.ron".into(),
            }],
        };
        let text = ron::ser::to_string(&p).unwrap();
        let back: Project = ron::de::from_str(&text).unwrap();
        assert_eq!(back.name, "demo");
        assert_eq!(back.scenes.len(), 1);
        assert_eq!(back.scenes[0].name, "main");
    }
}
