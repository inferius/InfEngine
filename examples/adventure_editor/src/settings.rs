//! A59.3: editor preferences that survive a restart.
//!
//! The interface scale and the timeline zoom are things a person sets
//! once for their monitor and their eyes, and losing them on every
//! launch turns a preference into a chore. They are not project data -
//! two people opening the same project want different scales - so they
//! live in the user's profile rather than next to the rooms.
//!
//! The format is deliberately plain: one `key value` per line, no
//! dependency, nothing that can half-parse. A file written by a newer
//! editor with keys this one does not know keeps working, because
//! unknown keys are skipped rather than refused.

use std::path::PathBuf;

/// What is remembered between runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub ui_scale: f32,
    pub timeline_zoom: f32,
    pub timeline_snap_parts: bool,
    pub timeline_snap_time: bool,
    pub timeline_open: bool,
    /// The interface language: "en" or "cs".
    pub language: String,
    /// Whether the library shows cells of picture rather than a list.
    pub library_grid: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            timeline_zoom: 1.0,
            timeline_snap_parts: true,
            timeline_snap_time: true,
            timeline_open: true,
            language: "en".into(),
            library_grid: false,
        }
    }
}

/// Where the file lives: the user's profile, so it follows the person
/// rather than the project.
fn path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(base).join("InfEngine").join("adventure_editor.conf"))
}

impl Settings {
    pub fn load() -> Self {
        let Some(path) = path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        Self::parse(&text)
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Best effort on purpose: a preferences file that cannot be
        // written is an inconvenience, and failing to exit over one
        // would be worse than forgetting the zoom level.
        let _ = std::fs::write(path, self.serialise());
    }

    fn serialise(&self) -> String {
        format!(
            "ui_scale {}\ntimeline_zoom {}\ntimeline_snap_parts {}\ntimeline_snap_time {}\ntimeline_open {}\nlanguage {}\nlibrary_grid {}\n",
            self.ui_scale,
            self.timeline_zoom,
            self.timeline_snap_parts as u8,
            self.timeline_snap_time as u8,
            self.timeline_open as u8,
            self.language,
            self.library_grid as u8,
        )
    }

    fn parse(text: &str) -> Self {
        let mut out = Self::default();
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
                continue;
            };
            // The one key whose value is a word, read before the number
            // parse can throw it away.
            if key == "language" {
                if matches!(value, "en" | "cs") {
                    out.language = value.to_string();
                }
                continue;
            }
            let Ok(value) = value.parse::<f32>() else {
                continue;
            };
            if !value.is_finite() {
                continue;
            }
            match key {
                "ui_scale" => out.ui_scale = value,
                "timeline_zoom" => out.timeline_zoom = value,
                "timeline_snap_parts" => out.timeline_snap_parts = value != 0.0,
                "timeline_snap_time" => out.timeline_snap_time = value != 0.0,
                "timeline_open" => out.timeline_open = value != 0.0,
                "library_grid" => out.library_grid = value != 0.0,
                // A newer editor's keys are not this one's problem.
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back() {
        let settings = Settings {
            ui_scale: 1.4,
            timeline_zoom: 0.5,
            timeline_snap_parts: false,
            timeline_snap_time: true,
            timeline_open: false,
            language: "cs".into(),
            library_grid: true,
        };
        assert_eq!(Settings::parse(&settings.serialise()), settings);
    }

    #[test]
    fn an_unknown_language_keeps_the_default() {
        assert_eq!(Settings::parse("language xx").language, "en");
        assert_eq!(Settings::parse("language cs").language, "cs");
    }

    #[test]
    fn an_unknown_key_is_skipped_rather_than_fatal() {
        let parsed = Settings::parse("future_thing 42\nui_scale 2.0\n");
        assert_eq!(parsed.ui_scale, 2.0);
        assert_eq!(parsed.timeline_zoom, 1.0);
    }

    #[test]
    fn garbage_yields_the_defaults() {
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("ui_scale banana"), Settings::default());
        assert_eq!(
            Settings::parse("ui_scale inf").ui_scale,
            1.0,
            "an infinite scale draws nothing"
        );
    }
}
