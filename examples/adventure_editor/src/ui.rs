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
    /// A script the Scene tab asked to run. Empty means the room's own.
    pub run_script: Option<String>,
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
    /// Whether a scene is running, how far into it, and what it is doing.
    pub playing: bool,
    pub elapsed: f32,
    pub now: String,
    /// Where the view is, while one runs.
    pub camera: glam::Vec2,
    /// Thumbnails and summaries for the scene list.
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

/// Build every panel for one frame.
pub fn build(ctx: &egui::Context, state: &mut UiState<'_>) -> UiActions {
    let mut actions = UiActions::default();
    let screen = ctx.content_rect();
    state.content_min = egui::vec2(screen.left(), screen.top());
    state.content_max = egui::vec2(screen.right(), screen.bottom());
    // Answer a few of the scene list's thumbnail requests from last
    // frame before drawing it again.
    state.cards.tick(ctx, state.project, 3);
    rooms_panel(ctx, state, &mut actions);
    state.content_max.x = crate::inspector::panel(ctx, state, &mut actions);
    status_bar(ctx, state);
    actions
}

fn rooms_panel(ctx: &egui::Context, state: &mut UiState<'_>, actions: &mut UiActions) {
    let response = egui::SidePanel::left("rooms")
        .default_width(230.0)
        .show(ctx, |ui| {
            ui.heading("Scenes");
            ui.label(
                egui::RichText::new(state.project.root.display().to_string())
                    .small()
                    .weak(),
            );

            if state.project.is_empty() {
                ui.separator();
                ui.label("No scenes found.");
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
                .show(ui, |ui| scene_list(ui, state, actions, &matches));
        });
    state.content_min.x = response.response.rect.right();
}

/// The list body: scene cards, with same-background families folded.
///
/// `gencavb_127` through `gencavb_140` are one cave photographed
/// fourteen times, and a flat list of them buries the scenes that are
/// actually distinct. Folding a family behind its shared name keeps the
/// panel the length of the game, not the length of the data. A search
/// unfolds everything, because someone typing a name wants the row
/// itself, not the drawer it is in.
fn scene_list(
    ui: &mut egui::Ui,
    state: &mut UiState<'_>,
    actions: &mut UiActions,
    matches: &[usize],
) {
    let labels: Vec<(usize, String)> = matches
        .iter()
        .filter_map(|&index| {
            let path = state.project.rooms.get(index)?;
            Some((index, Project::room_label(path)))
        })
        .collect();
    let searching = !state.room_filter.trim().is_empty();
    for (family, members) in group_by_family(&labels) {
        if members.len() == 1 || searching {
            for &(index, ref label) in &members {
                scene_row(ui, state, actions, index, label);
            }
            continue;
        }
        let holds_current = members
            .iter()
            .any(|(index, _)| state.current_room == Some(*index));
        egui::CollapsingHeader::new(format!("{family} \u{b7} {}", members.len()))
            .id_salt(("family", family))
            .default_open(holds_current)
            .show(ui, |ui| {
                for &(index, ref label) in &members {
                    scene_row(ui, state, actions, index, label);
                }
            });
    }
}

/// One scene's row: its background, its name, and what it holds.
fn scene_row(
    ui: &mut egui::Ui,
    state: &mut UiState<'_>,
    actions: &mut UiActions,
    index: usize,
    label: &str,
) {
    if let Some(path) = state.project.rooms.get(index) {
        state.cards.request(label, path);
    }
    let card = state.cards.get(label);
    let selected = state.current_room == Some(index);

    let mut text = egui::text::LayoutJob::default();
    text.append(
        label,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(13.0),
            color: ui.visuals().strong_text_color(),
            ..Default::default()
        },
    );
    if let Some(card) = card {
        text.append(
            &format!("\n{}", crate::cards::caption(card)),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(10.0),
                color: ui.visuals().weak_text_color(),
                ..Default::default()
            },
        );
    }

    let button = match card.and_then(|card| card.thumb.as_ref()) {
        Some(texture) => {
            let size = texture.size_vec2();
            let scale = crate::cards::THUMB / size.x.max(1.0);
            egui::Button::image_and_text(
                egui::Image::new((texture.id(), size * scale)),
                text,
            )
        }
        None => egui::Button::new(text),
    };
    let row = ui.add_sized(
        [ui.available_width(), 30.0],
        button.selected(selected).frame(selected),
    );
    if row.clicked() {
        actions.open_room = Some(index);
    }
}

/// Which family a scene belongs to: its name up to a numbered suffix.
///
/// The numbered suffixes are how instances of one background are told
/// apart, so the family is exactly "the scenes sharing a picture".
pub fn family(label: &str) -> &str {
    match label.rsplit_once('_') {
        Some((head, tail))
            if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => label,
    }
}

/// Group labelled rooms by family, keeping first-seen order.
pub fn group_by_family(labels: &[(usize, String)]) -> Vec<(String, Vec<(usize, String)>)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<(usize, String)>> = Default::default();
    for (index, label) in labels {
        let family = family(label).to_string();
        if !groups.contains_key(&family) {
            order.push(family.clone());
        }
        groups
            .entry(family)
            .or_default()
            .push((*index, label.clone()));
    }
    order
        .into_iter()
        .map(|family| {
            let members = groups.remove(&family).unwrap_or_default();
            (family, members)
        })
        .collect()
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

/// The bottom row: what just happened, and what to do next.
///
/// It used to hold one label that was empty most of the time. A row that
/// is usually blank teaches the reader not to look at it, which wastes
/// the one place in the window that can answer "now what".
fn status_bar(ctx: &egui::Context, state: &UiState<'_>) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if state.status.is_empty() {
                ui.label(egui::RichText::new(next_step(state)).weak());
            } else {
                ui.label(state.status);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(crate::inspector::tool_hint(*state.tool))
                        .small()
                        .weak(),
                );
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
        return "No floor here yet - the Floor tab draws one, and without it                 the actor cannot move."
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
            fonts: Vec::new(),
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
    fn a_family_is_the_name_before_the_numbered_suffix() {
        assert_eq!(family("gencavb_127"), "gencavb");
        assert_eq!(family("bridge"), "bridge");
        assert_eq!(family("_intro"), "_intro", "no name before the underscore");
        assert_eq!(family("note_v2_final"), "note_v2_final", "letters in the tail");
    }

    #[test]
    fn grouping_keeps_first_seen_order_and_gathers_families() {
        let labels = vec![
            (0, "bridge".to_string()),
            (1, "gencavb_127".to_string()),
            (2, "gencavb_128".to_string()),
            (3, "well".to_string()),
        ];
        let groups = group_by_family(&labels);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, "bridge");
        assert_eq!(groups[1].0, "gencavb");
        assert_eq!(groups[1].1.len(), 2);
        assert_eq!(groups[2].0, "well");
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
