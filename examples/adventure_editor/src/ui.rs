//! A6.5: the editor's panels.
//!
//! The UI reads and writes application state through a small context
//! struct rather than owning it, so panel code stays free of rendering
//! and windowing concerns.

use infengine_adventure::{Room, Verb};

use crate::draw::Overlays;
use crate::edit::{Selection, Tool};
use crate::project::Project;

/// Requests the UI can make of the app, returned rather than performed
/// so the app decides when a room is loaded or written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiActions {
    /// Index into [`Project::rooms`] to open.
    pub open_room: Option<usize>,
    pub save: bool,
    pub reload: bool,
    pub fit_view: bool,
    pub delete_selection: bool,
}

/// Everything the panels touch.
pub struct UiState<'a> {
    pub project: &'a Project,
    pub current_room: Option<usize>,
    pub room: &'a mut Room,
    pub tool: &'a mut Tool,
    pub overlays: &'a mut Overlays,
    pub selection: Selection,
    pub dirty: bool,
    pub missing_images: &'a [String],
    pub status: &'a str,
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
    rooms_panel(ctx, state, &mut actions);
    inspector_panel(ctx, state, &mut actions);
    status_bar(ctx, state);
    actions
}

fn rooms_panel(ctx: &egui::Context, state: &mut UiState<'_>, actions: &mut UiActions) {
    egui::SidePanel::left("rooms")
        .default_width(200.0)
        .show(ctx, |ui| {
            ui.heading("Rooms");
            ui.label(
                egui::RichText::new(state.project.root.display().to_string())
                    .small()
                    .weak(),
            );
            ui.separator();

            if state.project.is_empty() {
                ui.label("No rooms found.");
                ui.label(
                    egui::RichText::new("Point the editor at a folder containing rooms/.")
                        .small()
                        .weak(),
                );
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("room_list")
                .show(ui, |ui| {
                    for (index, path) in state.project.rooms.iter().enumerate() {
                        let selected = state.current_room == Some(index);
                        let label = Project::room_label(path);
                        if ui.selectable_label(selected, label).clicked() {
                            actions.open_room = Some(index);
                        }
                    }
                });
        });
}

fn inspector_panel(ctx: &egui::Context, state: &mut UiState<'_>, actions: &mut UiActions) {
    egui::SidePanel::right("inspector")
        .default_width(300.0)
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

            ui.heading("Show");
            ui.checkbox(&mut state.overlays.floor, "Walkable floor");
            ui.checkbox(&mut state.overlays.hotspots, "Hotspots");
            ui.checkbox(&mut state.overlays.exits, "Exits");
            ui.checkbox(&mut state.overlays.entries, "Entry points");
            ui.checkbox(&mut state.overlays.bounds, "Room bounds");
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("inspector_scroll")
                .show(ui, |ui| {
                    room_section(ui, state);
                    ui.separator();
                    layers_section(ui, state);
                    ui.separator();
                    selection_section(ui, state, actions);
                    ui.separator();
                    problems_section(ui, state);
                });
        });
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

    #[test]
    fn actions_default_to_doing_nothing() {
        let actions = UiActions::default();
        assert_eq!(actions, UiActions::default());
        assert!(actions.open_room.is_none());
        assert!(!actions.save && !actions.reload && !actions.fit_view);
        assert!(!actions.delete_selection);
    }
}
