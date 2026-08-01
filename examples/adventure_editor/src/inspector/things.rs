//! A41.4: what the player can click.
//!
//! Hotspots and exits together, because from the player's side they are
//! the same gesture - you click a thing and something happens. Which of
//! the two it is only matters to us.
//!
//! Exits get a target box that is checked against the rooms the project
//! actually has. A door pointing at a room that does not exist is the
//! commonest thing wrong with an imported game and the hardest to notice
//! by playing, since it looks exactly like a door you have not tried.

use infengine_adventure::{Project, Verb};

use crate::edit::Selection;
use crate::ui::{UiActions, UiState};

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    selected(ui, state, actions);

    ui.heading(format!("Scenery ({})", state.room.hotspots.len()));
    if state.room.hotspots.is_empty() {
        super::nothing_yet(
            ui,
            "Nothing here can be clicked.",
            "With Box out, drag a rectangle over something in the picture.",
        );
    } else {
        ui.label(
            egui::RichText::new("Click one on the picture to edit it.")
                .small()
                .weak(),
        );
        for (index, hotspot) in state.room.hotspots.iter().enumerate() {
            let chosen = state.selection == Selection::Hotspot(index);
            let name = if hotspot.name.is_empty() {
                hotspot.id.as_str()
            } else {
                hotspot.name.as_str()
            };
            let mut text = egui::RichText::new(format!("{name}  ({})", hotspot.default_verb.name()));
            if !hotspot.enabled {
                text = text.weak();
            }
            ui.selectable_label(chosen, text)
                .on_hover_text(if hotspot.enabled {
                    hotspot.id.clone()
                } else {
                    format!("{} - starts switched off", hotspot.id)
                });
        }
    }

    ui.add_space(10.0);
    exits(ui, state);
}

/// The ways out, each checked against the rooms that exist.
fn exits(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    ui.heading(format!("Ways out ({})", state.room.exits.len()));
    if state.room.exits.is_empty() {
        super::nothing_yet(
            ui,
            "This room leads nowhere.",
            "A room with no way out traps the player unless a script moves them.",
        );
        return;
    }

    let known: Vec<String> = state
        .project
        .rooms
        .iter()
        .map(|path| Project::room_label(path))
        .collect();

    egui::Grid::new("exit_grid")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            for (index, exit) in state.room.exits.iter_mut().enumerate() {
                ui.checkbox(&mut exit.enabled, "")
                    .on_hover_text("Unticked means the player cannot use it yet.");

                let name = if exit.name.is_empty() {
                    exit.id.clone()
                } else {
                    exit.name.clone()
                };
                ui.label(name).on_hover_text(&exit.id);

                // A combo of what exists, so a target can be picked
                // rather than typed and mistyped.
                let missing = !known.contains(&exit.target_room);
                let shown = if exit.target_room.is_empty() {
                    "nowhere".to_string()
                } else {
                    exit.target_room.clone()
                };
                let text = if missing {
                    egui::RichText::new(shown).color(egui::Color32::from_rgb(255, 140, 120))
                } else {
                    egui::RichText::new(shown)
                };
                egui::ComboBox::from_id_salt(format!("exit_target_{index}"))
                    .selected_text(text)
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for room in &known {
                            ui.selectable_value(&mut exit.target_room, room.clone(), room);
                        }
                    });
                ui.end_row();
            }
        });
}

/// The thing under the cursor's last click, in full.
fn selected(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    match state.selection {
        Selection::None => return,
        Selection::Vertex(at) => {
            ui.heading("Selected corner");
            ui.label(format!(
                "{:?} outline {}, corner {}",
                at.kind, at.polygon, at.vertex
            ));
            if ui.button("Delete corner").clicked() {
                actions.delete_selection = true;
            }
        }
        Selection::Hotspot(index) => {
            let Some(hotspot) = state.room.hotspots.get_mut(index) else {
                return;
            };
            ui.heading("Selected");
            egui::Grid::new("hotspot_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("id");
                    ui.text_edit_singleline(&mut hotspot.id)
                        .on_hover_text("What scripts call it.");
                    ui.end_row();
                    ui.label("name");
                    ui.text_edit_singleline(&mut hotspot.name)
                        .on_hover_text("What the player reads when the cursor is over it.");
                    ui.end_row();
                    ui.label("on click");
                    egui::ComboBox::from_id_salt("hotspot_verb")
                        .selected_text(hotspot.default_verb.name())
                        .show_ui(ui, |ui| {
                            for verb in Verb::ALL {
                                ui.selectable_value(&mut hotspot.default_verb, verb, verb.name());
                            }
                        });
                    ui.end_row();
                    ui.label("active");
                    ui.checkbox(&mut hotspot.enabled, "");
                    ui.end_row();
                });
            if ui.button("Delete").clicked() {
                actions.delete_selection = true;
            }
        }
    }
    ui.separator();
}
