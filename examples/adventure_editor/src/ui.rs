//! A6.5: the editor's panels.
//!
//! The UI reads and writes application state through a small context
//! struct rather than owning it, so panel code stays free of rendering
//! and windowing concerns.
//!
//! The shell lives here - the room list on the left, the status bar
//! along the bottom, and the shape of the state the panels share. What
//! goes in the right-hand panel is [`crate::inspector`], which is split
//! by the job being done rather than by the kind of field.

use infengine_adventure::{Animation, Room};

use crate::draw::Overlays;
use crate::edit::{Selection, Tool};
use crate::inspector::Task;
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
    pub save: bool,
    pub reload: bool,
    pub fit_view: bool,
    pub delete_selection: bool,
}

/// Everything the panels touch.
pub struct UiState<'a> {
    pub project: &'a Project,
    /// What the room filter box holds.
    pub room_filter: &'a mut String,
    /// Where the free area between the panels ended up, filled in by
    /// the UI so the view can frame the room where it is visible.
    pub content_min: egui::Vec2,
    pub content_max: egui::Vec2,
    pub current_room: Option<usize>,
    pub room: &'a mut Room,
    /// Which job the inspector is showing.
    pub task: &'a mut Task,
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
}

impl std::fmt::Debug for UiState<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiState")
            .field("current_room", &self.current_room)
            .field("dirty", &self.dirty)
            .finish()
    }
}

/// Build every panel for one frame.
pub fn build(ctx: &egui::Context, state: &mut UiState<'_>) -> UiActions {
    let mut actions = UiActions::default();
    let screen = ctx.content_rect();
    state.content_min = egui::vec2(screen.left(), screen.top());
    state.content_max = egui::vec2(screen.right(), screen.bottom());
    rooms_panel(ctx, state, &mut actions);
    state.content_max.x = crate::inspector::panel(ctx, state, &mut actions);
    status_bar(ctx, state);
    actions
}

fn rooms_panel(ctx: &egui::Context, state: &mut UiState<'_>, actions: &mut UiActions) {
    let response = egui::SidePanel::left("rooms")
        .default_width(220.0)
        .show(ctx, |ui| {
            ui.heading("Rooms");
            ui.label(
                egui::RichText::new(state.project.root.display().to_string())
                    .small()
                    .weak(),
            );

            if state.project.is_empty() {
                ui.separator();
                ui.label("No rooms found.");
                ui.label(
                    egui::RichText::new("Point the editor at a folder containing rooms/.")
                        .small()
                        .weak(),
                );
                return;
            }

            ui.horizontal(|ui| {
                ui.label("Find");
                ui.text_edit_singleline(state.room_filter);
                if ui.small_button("clear").clicked() {
                    state.room_filter.clear();
                }
            });
            let matches = matching_rooms(state.project, state.room_filter);
            ui.label(
                egui::RichText::new(format!(
                    "{} of {}",
                    matches.len(),
                    state.project.rooms.len()
                ))
                .small()
                .weak(),
            );
            ui.separator();

            // The scroll area fills the panel so its bar sits at the
            // panel edge rather than hugging the widest entry.
            egui::ScrollArea::vertical()
                .id_salt("room_list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for index in matches {
                        let Some(path) = state.project.rooms.get(index) else {
                            continue;
                        };
                        let selected = state.current_room == Some(index);
                        if ui
                            .selectable_label(selected, Project::room_label(path))
                            .clicked()
                        {
                            actions.open_room = Some(index);
                        }
                    }
                });
        });
    state.content_min.x = response.response.rect.right();
}

/// Indices of the rooms whose name contains `filter`, case-insensitively.
///
/// An empty filter matches everything, so the box starts out showing the
/// whole project rather than nothing.
pub fn matching_rooms(project: &Project, filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_ascii_lowercase();
    project
        .rooms
        .iter()
        .enumerate()
        .filter(|(_, path)| {
            needle.is_empty()
                || Project::room_label(path)
                    .to_ascii_lowercase()
                    .contains(&needle)
        })
        .map(|(index, _)| index)
        .collect()
}

fn status_bar(ctx: &egui::Context, state: &UiState<'_>) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(state.status);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_of(names: &[&str]) -> Project {
        Project {
            root: std::path::PathBuf::new(),
            rooms: names
                .iter()
                .map(|n| std::path::PathBuf::from(format!("a/{n}.room.ron")))
                .collect(),
            animations: Default::default(),
            locales: Default::default(),
            sheets: Vec::new(),
            scripts: Default::default(),
            script_errors: Vec::new(),
            screens: Vec::new(),
            sounds: Default::default(),
            manifest: None,
        }
    }

    #[test]
    fn an_empty_filter_matches_every_room() {
        let project = project_of(&["alchemy", "bridge"]);
        assert_eq!(matching_rooms(&project, ""), vec![0, 1]);
        assert_eq!(matching_rooms(&project, "   "), vec![0, 1]);
    }

    #[test]
    fn the_filter_ignores_case_and_matches_anywhere_in_the_name() {
        let project = project_of(&["alchemy", "bridge", "catacom"]);
        assert_eq!(matching_rooms(&project, "RID"), vec![1]);
        assert_eq!(matching_rooms(&project, "c"), vec![0, 2]);
        assert!(matching_rooms(&project, "zzz").is_empty());
    }

    #[test]
    fn actions_default_to_doing_nothing() {
        let actions = UiActions::default();
        assert_eq!(actions, UiActions::default());
        assert!(actions.open_room.is_none());
        assert!(!actions.save && !actions.reload && !actions.fit_view);
        assert!(!actions.delete_selection);
    }
}
