//! A6.5: the state the panels share, and the status bar.
//!
//! The UI reads and writes application state through a small context
//! struct rather than owning it, so panel code stays free of rendering
//! and windowing concerns.
//!
//! The panels themselves live elsewhere: the library on the left is
//! [`crate::library`], the inspector on the right is
//! [`crate::inspector`], the timeline is [`crate::timeline`]. What
//! stays here is the shape of the state they share and the status bar
//! along the bottom.

use infengine_adventure::{Animation, Room};

use crate::draw::Overlays;
use crate::edit::{Selection, Tool};
use crate::mode::Mode;
use crate::playback::Playback;
use infengine_adventure::Project;

/// Requests the UI can make of the app, returned rather than performed
/// so the app decides when a room is loaded or written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiActions {
    /// Index into [`Project::rooms`] to open.
    pub open_room: Option<usize>,
    /// Animation the user asked to preview.
    pub play_animation: Option<String>,
    /// Line the user asked to hear.
    pub play_line: Option<String>,
    /// Sheet the user opened for cutting.
    pub open_sheet: Option<String>,
    /// The draft was accepted into the project's animations.
    pub save_sheet: bool,
    /// The draft was abandoned.
    pub close_sheet: bool,
    /// A script the inspector asked to run. Empty means the room's own.
    pub run_script: Option<String>,
    /// A script to put on the graph canvas.
    pub open_graph_script: Option<String>,
    /// It asked to stop instead.
    pub stop_play: bool,
    pub save: bool,
    pub reload: bool,
    pub fit_view: bool,
    pub delete_selection: bool,
}

/// Everything the panels touch.
pub struct UiState<'a> {
    pub project: &'a Project,
    /// Where the free area between the panels ended up, filled in by
    /// the UI so the view can frame the room where it is visible.
    pub content_min: egui::Vec2,
    pub content_max: egui::Vec2,
    pub current_room: Option<usize>,
    pub room: &'a mut Room,
    /// What the editor is being used for; the pills at the top set it.
    pub mode: Mode,
    pub tool: &'a mut Tool,
    pub overlays: &'a mut Overlays,
    pub selection: Selection,
    pub dirty: bool,
    pub missing_images: &'a [String],
    pub status: &'a str,
    pub playback: &'a mut Playback,
    /// Animations this room owns, then the ones tied to no room.
    pub animations: &'a [Animation],
    /// Languages the project ships, sorted.
    pub languages: &'a [String],
    /// Language whose text is shown.
    pub text_language: &'a mut String,
    /// Language whose recordings are played. Kept apart because
    /// checking a translation against the original delivery is a
    /// normal thing to want.
    pub voice_language: &'a mut String,
    /// This room's lines: id, text, and whether audio exists.
    pub lines: &'a [(String, String, bool)],
    /// The sheet being cut into frames, if any.
    pub sheet: &'a mut Option<crate::sheet::SheetDraft>,
    /// Whether a scene is running, and what it is doing right now.
    pub playing: bool,
    pub now: String,
    /// Where the view is, while one runs.
    pub camera: glam::Vec2,
    /// Where the cursor is, in room pixels.
    pub cursor_room: glam::Vec2,
    /// Whether Animate mode shows the timeline. The status bar's
    /// toggle; the panel is worth a third of the window and a way to
    /// give it back.
    pub timeline_open: &'a mut bool,
    /// Thumbnails and summaries for the room list.
    pub cards: &'a mut crate::cards::Cards,
}

impl std::fmt::Debug for UiState<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiState")
            .field("current_room", &self.current_room)
            .field("dirty", &self.dirty)
            .finish()
    }
}

/// The bottom row: what just happened, and what to do next.
///
/// It used to hold one label that was empty most of the time. A row that
/// is usually blank teaches the reader not to look at it, which wastes
/// the one place in the window that can answer "now what".
pub fn status_bar(ctx: &egui::Context, state: &mut UiState<'_>) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} mode", state.mode.label().to_lowercase()))
                    .small()
                    .weak(),
            );
            if state.current_room.is_some() {
                // Where the cursor is, in the room's own pixels: the
                // number a hand-placed thing is placed by.
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "x {} · y {}",
                        state.cursor_room.x.round() as i32,
                        state.cursor_room.y.round() as i32
                    ))
                    .small()
                    .monospace()
                    .weak(),
                );
                let corners: usize =
                    state.room.walk.solids.iter().map(|p| p.points.len()).sum();
                if corners > 0 {
                    ui.label(
                        egui::RichText::new(format!("floor: {corners} corners"))
                            .small()
                            .weak(),
                    );
                }
            }
            ui.separator();
            if state.playing && !state.now.is_empty() {
                // While a scene runs, the most useful thing the bar can
                // say is what the script is doing this instant.
                ui.label(egui::RichText::new(format!("▶ {}", state.now)).strong());
            } else if state.status.is_empty() {
                ui.label(egui::RichText::new(next_step(state)).weak());
            } else {
                ui.label(state.status);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if state.mode == Mode::Animate {
                    let label = if *state.timeline_open {
                        "Hide timeline"
                    } else {
                        "Show timeline"
                    };
                    if ui.small_button(label).clicked() {
                        *state.timeline_open = !*state.timeline_open;
                    }
                }
            });
        });
    });
}

/// The most useful thing to say when nothing has just happened.
///
/// Ordered by how badly the room is missing it: a room with no floor is
/// unplayable, one with no way out is a trap, one with nothing to click
/// is scenery. Saying all three at once would say nothing.
pub fn next_step(state: &UiState<'_>) -> String {
    if state.current_room.is_none() {
        return "Pick a room on the left to start.".into();
    }
    let room = &state.room;
    if room.walk.solids.is_empty() {
        return "No floor here yet - the Outline tool draws one, and without it \
                the actor cannot move."
            .into();
    }
    if room.exits.is_empty() {
        return "Nothing leads out of this room - add a way out from the Room menu.".into();
    }
    if room.hotspots.is_empty() {
        return "Nothing here can be clicked - add one from the Room menu.".into();
    }
    format!(
        "{} - {} things, {} ways out. Help is under the ? at the top right.",
        room.id,
        room.hotspots.len(),
        room.exits.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_default_to_doing_nothing() {
        let actions = UiActions::default();
        assert_eq!(actions, UiActions::default());
        assert!(actions.open_room.is_none());
        assert!(!actions.save && !actions.reload && !actions.fit_view);
        assert!(!actions.delete_selection);
    }
}
