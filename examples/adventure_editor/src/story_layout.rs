//! A99: where the story map's nodes sit, kept with the game.
//!
//! Twine and every node editor since agree on this: the arrangement is
//! the author's work, not the tool's. An auto-layout is a starting
//! point and a rescue, but the map a person made by hand - this branch
//! above that one, the cellars over on the left - carries meaning the
//! graph itself does not, and losing it on every launch would make
//! arranging pointless.
//!
//! So positions live beside the rooms, in the project, in a file of
//! their own: `story.layout.ron`. Not in the room files, because where
//! a room sits on a map is not a property of the room; not in the
//! user's profile, because two people opening one game should see the
//! same map.

use std::collections::BTreeMap;
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// File holding the arrangement, relative to the project root.
pub const LAYOUT_FILE: &str = "story.layout.ron";

/// Where each node sits, by id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default)]
    pub at: BTreeMap<String, Vec2>,
}

impl Layout {
    /// Read the arrangement a project has, or an empty one.
    ///
    /// A missing or broken file is not an error worth stopping for: the
    /// map still draws, laid out automatically, and saving writes a
    /// good file over the bad one.
    pub fn load(root: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(root.join(LAYOUT_FILE)) else {
            return Self::default();
        };
        ron::from_str(&text).unwrap_or_default()
    }

    /// Write it back beside the rooms.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(std::io::Error::other)?;
        std::fs::write(root.join(LAYOUT_FILE), text)
    }

    pub fn get(&self, id: &str) -> Option<Vec2> {
        self.at.get(id).copied()
    }

    pub fn set(&mut self, id: &str, at: Vec2) {
        self.at.insert(id.to_string(), at);
    }

    /// Forget nodes the game no longer has, so a renamed room does not
    /// leave a ghost holding a place on the map for ever.
    pub fn keep_only(&mut self, alive: &[String]) {
        self.at.retain(|id, _| alive.iter().any(|k| k == id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back() {
        let dir = std::env::temp_dir().join("infengine_story_layout");
        let _ = std::fs::create_dir_all(&dir);
        let mut layout = Layout::default();
        layout.set("hall", Vec2::new(40.0, 80.0));
        layout.save(&dir).expect("saved");
        assert_eq!(Layout::load(&dir), layout);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_layout_yet_gets_an_empty_one() {
        let empty = Layout::load(Path::new("nowhere_at_all"));
        assert!(empty.at.is_empty());
    }

    #[test]
    fn renamed_rooms_do_not_hold_places_for_ever() {
        let mut layout = Layout::default();
        layout.set("hall", Vec2::ZERO);
        layout.set("old_name", Vec2::ZERO);
        layout.keep_only(&["hall".to_string()]);
        assert_eq!(layout.at.len(), 1);
        assert!(layout.get("hall").is_some());
    }
}
