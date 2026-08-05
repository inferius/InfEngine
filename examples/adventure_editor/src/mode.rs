//! A83: the editor's four modes.
//!
//! One question at the top of the window: what am I doing right now?
//! Building a room, animating a scene, wiring its logic, or playing
//! the game. Everything else follows from the answer - which tools the
//! toolbar offers, what the inspector shows, what fills the centre of
//! the screen. Before this the same choices were spread over six
//! inspector tabs and four floating windows, and finding a feature
//! meant knowing which drawer it had been put in.

use crate::draw::Overlays;
use crate::edit::Tool;

/// What the editor is being used for right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Building the room: pictures, floor, things to click.
    #[default]
    Room,
    /// Composing what happens over time: animations and the timeline.
    Animate,
    /// Wiring the logic: scripts as a graph.
    Script,
    /// Playing the game with the tester's controls at hand.
    Test,
    /// The whole game as a map: every place and the ways between.
    Story,
}

impl Mode {
    pub const ALL: [Mode; 5] =
        [Mode::Room, Mode::Animate, Mode::Script, Mode::Test, Mode::Story];

    /// The pill's label.
    pub fn label(self) -> &'static str {
        use crate::lang::t;
        match self {
            Mode::Room => t("Room", "Místnost"),
            Mode::Animate => t("Animate", "Animace"),
            Mode::Script => t("Script", "Skript"),
            Mode::Test => t("Test", "Test"),
            Mode::Story => t("Story", "Příběh"),
        }
    }

    /// What switching here means, on hover.
    pub fn hint(self) -> &'static str {
        use crate::lang::t;
        match self {
            Mode::Room => t(
                "Build the room: its pictures, its floor, the things to click.",
                "Postav místnost: její obrázky, podlahu a věci na klikání.",
            ),
            Mode::Animate => t(
                "Compose the scene: animations, sounds and the timeline.",
                "Slož scénu: animace, zvuky a časovou osu.",
            ),
            Mode::Script => t(
                "Wire the logic: what happens when, as a graph.",
                "Zapoj logiku: co se kdy stane, jako graf.",
            ),
            Mode::Test => t(
                "Play the game, with flags and the log at hand.",
                "Hraj hru, s přepínači a záznamem po ruce.",
            ),
            Mode::Story => t(
                "The whole game as a map: every place, and the ways between them.",
                "Celá hra jako mapa: všechna místa a cesty mezi nimi.",
            ),
        }
    }

    /// The canvas tools this mode offers. The first is picked when the
    /// mode is entered, so the mouse always does something sensible.
    pub fn tools(self) -> &'static [Tool] {
        match self {
            Mode::Room => &[
                Tool::Select,
                Tool::DrawFloor,
                Tool::DrawHotspot,
                Tool::WalkPreview,
            ],
            // The canvas still selects and pans; composing happens on
            // the timeline below and the shelves beside it.
            Mode::Animate => &[Tool::Select],
            Mode::Script | Mode::Test | Mode::Story => &[],
        }
    }

    /// Turn on the overlays this mode needs to see. Only ever sets, so
    /// a deliberate toggle afterwards is not undone by switching back.
    pub fn reveal(self, overlays: &mut Overlays) {
        match self {
            Mode::Room => {
                overlays.floor = true;
                overlays.hotspots = true;
                overlays.exits = true;
                overlays.entries = true;
                overlays.bounds = true;
            }
            Mode::Animate => {
                overlays.bounds = true;
                overlays.camera = true;
            }
            Mode::Script | Mode::Test | Mode::Story => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_says_what_it_is_for() {
        for mode in Mode::ALL {
            assert!(!mode.label().is_empty(), "{mode:?} has no label");
            assert!(
                mode.hint().len() > 20,
                "{mode:?} needs a real sentence, not a word"
            );
        }
    }

    #[test]
    fn room_mode_starts_with_the_selection_tool() {
        // Entering the editor and clicking should adjust, not draw.
        assert_eq!(Mode::Room.tools().first(), Some(&Tool::Select));
    }

    #[test]
    fn every_tool_belongs_to_some_mode() {
        for tool in Tool::ALL {
            assert!(
                Mode::ALL.iter().any(|m| m.tools().contains(&tool)),
                "{tool:?} is offered by no mode"
            );
        }
    }

    #[test]
    fn revealing_never_turns_an_overlay_off() {
        let full = Overlays::default();
        for mode in Mode::ALL {
            let mut overlays = full;
            mode.reveal(&mut overlays);
            assert_eq!(overlays, full, "{mode:?} turned something off");
        }
    }
}
