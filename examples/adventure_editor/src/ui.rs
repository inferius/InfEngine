//! A6.5: the editor's panels.
//!
//! The UI reads and writes application state through a small context
//! struct rather than owning it, so panel code stays free of rendering
//! and windowing concerns.

use infengine_adventure::{Animation, Room, Verb};

use crate::draw::Overlays;
use crate::edit::{Selection, Tool};
use crate::playback::Playback;
use crate::project::Project;

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
    inspector_panel(ctx, state, &mut actions);
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

fn inspector_panel(ctx: &egui::Context, state: &mut UiState<'_>, actions: &mut UiActions) {
    let response = egui::SidePanel::right("inspector")
        .default_width(320.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let save = egui::Button::new(if state.dirty { "Save *" } else { "Save" });
                if ui.add_enabled(state.dirty, save).clicked() {
                    actions.save = true;
                }
                if ui.button("Reload").clicked() {
                    actions.reload = true;
                }
                if ui.button("Fit").clicked() {
                    actions.fit_view = true;
                }
            });
            ui.separator();

            ui.heading("Tool");
            ui.horizontal_wrapped(|ui| {
                for tool in Tool::ALL {
                    if ui
                        .selectable_label(*state.tool == tool, tool.label())
                        .clicked()
                    {
                        *state.tool = tool;
                    }
                }
            });
            ui.label(
                egui::RichText::new(tool_hint(*state.tool))
                    .small()
                    .weak(),
            );
            ui.separator();

            // Counts in the labels: a toggle that changes nothing looks
            // broken, and the room simply having none is the usual reason.
            ui.heading("Show");
            let floor_count = state.room.walk.solids.len() + state.room.walk.holes.len();
            ui.checkbox(
                &mut state.overlays.floor,
                format!("Walkable floor ({floor_count})"),
            );
            ui.checkbox(
                &mut state.overlays.hotspots,
                format!("Hotspots ({})", state.room.hotspots.len()),
            );
            ui.checkbox(
                &mut state.overlays.exits,
                format!("Exits ({})", state.room.exits.len()),
            );
            ui.checkbox(
                &mut state.overlays.entries,
                format!("Entry points ({})", state.room.entries.len()),
            );
            ui.checkbox(&mut state.overlays.bounds, "Room bounds");
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("inspector_scroll")
                .show(ui, |ui| {
                    room_section(ui, state);
                    ui.separator();
                    layers_section(ui, state);
                    ui.separator();
                    animation_section(ui, state, actions);
                    ui.separator();
                    lines_section(ui, state, actions);
                    ui.separator();
                    selection_section(ui, state, actions);
                    ui.separator();
                    problems_section(ui, state);
                });
        });
    state.content_max.x = response.response.rect.left();
}

fn tool_hint(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "Drag a vertex or hotspot. Right-click an edge to insert a vertex.",
        Tool::DrawFloor => "Click to add points. Right-click to finish the outline.",
        Tool::DrawHotspot => "Drag out a rectangle.",
        Tool::WalkPreview => "Click to route the actor there from the last point.",
    }
}

fn room_section(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    ui.heading("Room");
    egui::Grid::new("room_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("id");
            ui.text_edit_singleline(&mut state.room.id);
            ui.end_row();
            ui.label("name");
            ui.text_edit_singleline(&mut state.room.name);
            ui.end_row();
            ui.label("size");
            ui.label(format!(
                "{} x {}",
                state.room.size.x as i32, state.room.size.y as i32
            ));
            ui.end_row();
            ui.label("floor");
            ui.label(format!(
                "{} solid, {} holes",
                state.room.walk.solids.len(),
                state.room.walk.holes.len()
            ));
            ui.end_row();
        });
}

fn layers_section(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    ui.heading(format!("Layers ({})", state.room.layers.len()));
    for layer in &mut state.room.layers {
        ui.horizontal(|ui| {
            ui.checkbox(&mut layer.visible, "");
            let missing = state.missing_images.contains(&layer.image);
            let mut text = egui::RichText::new(&layer.name);
            if missing {
                text = text.color(egui::Color32::from_rgb(255, 140, 120));
            }
            ui.label(text).on_hover_text(&layer.image);
        });
    }
}

/// The room's animations, and a transport for the one being previewed.
fn animation_section(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    ui.heading(format!("Animations ({})", state.animations.len()));
    if state.animations.is_empty() {
        ui.label(egui::RichText::new("This room has none.").weak());
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("animation_list")
        .max_height(140.0)
        .show(ui, |ui| {
            for animation in state.animations {
                let selected = state.playback.selected.as_deref() == Some(&animation.id);
                let label = format!(
                    "{}  ({} frames, {}x{})",
                    animation.id,
                    animation.frames,
                    animation.frame_size.x as i32,
                    animation.frame_size.y as i32
                );
                if ui.selectable_label(selected, label).clicked() {
                    actions.play_animation = Some(animation.id.clone());
                }
            }
        });

    let Some(current) = state
        .playback
        .selected
        .as_deref()
        .and_then(|id| state.animations.iter().find(|a| a.id == id))
    else {
        return;
    };

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(if state.playback.playing { "pause" } else { "play" }).clicked() {
            state.playback.playing = !state.playback.playing;
        }
        if ui.button("<").clicked() {
            state.playback.step(-1, current);
        }
        if ui.button(">").clicked() {
            state.playback.step(1, current);
        }
        if ui.button("stop").clicked() {
            state.playback.clear();
        }
    });

    let mut frame = state.playback.frame(current);
    let last = current.frames.saturating_sub(1);
    if ui
        .add(egui::Slider::new(&mut frame, 0..=last).text("frame"))
        .changed()
    {
        state.playback.scrub_to(frame, current);
    }
    ui.add(egui::Slider::new(&mut state.playback.speed, 0.1..=4.0).text("speed"));
}

/// The room's dialogue, with the recording for each line.
fn lines_section(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    let voiced = state.lines.iter().filter(|(_, _, has)| *has).count();
    ui.heading(format!("Lines ({}, {voiced} voiced)", state.lines.len()));

    if state.languages.len() > 1 {
        ui.horizontal(|ui| {
            ui.label("text");
            language_picker(ui, "text_lang", state.languages, state.text_language);
            ui.label("voice");
            language_picker(ui, "voice_lang", state.languages, state.voice_language);
        });
    }

    if state.lines.is_empty() {
        ui.label(egui::RichText::new("This room says nothing.").weak());
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("line_list")
        .max_height(220.0)
        .show(ui, |ui| {
            for (id, text, has_voice) in state.lines {
                ui.horizontal_top(|ui| {
                    // A line with no recording gets a disabled button
                    // rather than none, so the column stays aligned and
                    // the gap is visible.
                    if ui
                        .add_enabled(*has_voice, egui::Button::new("play"))
                        .clicked()
                    {
                        actions.play_line = Some(id.clone());
                    }
                    ui.label(egui::RichText::new(text).small())
                        .on_hover_text(id);
                });
            }
        });
}

fn language_picker(
    ui: &mut egui::Ui,
    salt: &str,
    languages: &[String],
    current: &mut String,
) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(current.clone())
        .width(60.0)
        .show_ui(ui, |ui| {
            for language in languages {
                ui.selectable_value(current, language.clone(), language);
            }
        });
}

fn selection_section(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    ui.heading("Selection");
    match state.selection {
        Selection::None => {
            ui.label(egui::RichText::new("Nothing selected").weak());
        }
        Selection::Vertex(at) => {
            ui.label(format!(
                "{:?} polygon {} vertex {}",
                at.kind, at.polygon, at.vertex
            ));
            if ui.button("Delete vertex").clicked() {
                actions.delete_selection = true;
            }
        }
        Selection::Hotspot(index) => {
            let Some(hotspot) = state.room.hotspots.get_mut(index) else {
                ui.label(egui::RichText::new("Stale selection").weak());
                return;
            };
            egui::Grid::new("hotspot_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("id");
                    ui.text_edit_singleline(&mut hotspot.id);
                    ui.end_row();
                    ui.label("name");
                    ui.text_edit_singleline(&mut hotspot.name);
                    ui.end_row();
                    ui.label("verb");
                    egui::ComboBox::from_id_salt("hotspot_verb")
                        .selected_text(hotspot.default_verb.name())
                        .show_ui(ui, |ui| {
                            for verb in Verb::ALL {
                                ui.selectable_value(
                                    &mut hotspot.default_verb,
                                    verb,
                                    verb.name(),
                                );
                            }
                        });
                    ui.end_row();
                    ui.label("enabled");
                    ui.checkbox(&mut hotspot.enabled, "");
                    ui.end_row();
                });
            if ui.button("Delete hotspot").clicked() {
                actions.delete_selection = true;
            }
        }
    }
}

fn problems_section(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    let issues = state.room.validate();
    if issues.is_empty() && state.missing_images.is_empty() {
        ui.label(egui::RichText::new("No problems").weak());
        return;
    }
    ui.heading("Problems");
    for image in state.missing_images {
        ui.label(
            egui::RichText::new(format!("image not found: {image}"))
                .color(egui::Color32::from_rgb(255, 140, 120)),
        );
    }
    for issue in issues {
        ui.label(egui::RichText::new(issue).color(egui::Color32::from_rgb(255, 200, 120)));
    }
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

    #[test]
    fn every_tool_has_a_hint() {
        for tool in Tool::ALL {
            assert!(!tool_hint(tool).is_empty(), "{tool:?} has no hint");
        }
    }

    fn project_of(names: &[&str]) -> Project {
        Project {
            root: std::path::PathBuf::new(),
            rooms: names
                .iter()
                .map(|n| std::path::PathBuf::from(format!("a/{n}.room.ron")))
                .collect(),
            animations: Default::default(),
            locales: Default::default(),
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
