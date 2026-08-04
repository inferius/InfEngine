//! A83: Script mode's inspector - the room's scripts, and the one on
//! the canvas spelled out as readable steps.
//!
//! The graph in the centre is where a script is rewired; this column is
//! where it is read. The same logic in two shapes: boxes and wires for
//! editing structure, a plain list of steps for seeing at a glance what
//! the scene actually does. Click a script to put it on the canvas.

use infengine_adventure::action::Action;

use crate::ui::{UiActions, UiState};

pub fn show(
    ui: &mut egui::Ui,
    state: &mut UiState<'_>,
    actions: &mut UiActions,
    on_canvas: Option<&str>,
) {
    // By the room's script prefix, not its id: rooms that share a
    // background share their logic.
    let prefix = format!("{}.", state.room.script_prefix());
    let mut ids: Vec<&String> = state
        .project
        .scripts
        .keys()
        .filter(|id| id.starts_with(&prefix))
        .collect();
    ids.sort();

    ui.heading(format!("Scripts here ({})", ids.len()));
    if ids.is_empty() {
        super::nothing_yet(
            ui,
            "This room runs nothing.",
            "A script arrives with an import, or is drawn on the canvas.",
        );
    } else {
        ui.label(
            egui::RichText::new("Click one to put it on the canvas.")
                .small()
                .weak(),
        );
        for id in ids {
            let script = state.project.scripts.get(id);
            let steps = script.map(|s| s.actions.len()).unwrap_or(0);
            let unknown = script.map(|s| s.unknown_count()).unwrap_or(0);
            let short = id.strip_prefix(&prefix).unwrap_or(id);
            let chosen = on_canvas == Some(id.as_str());
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(chosen, format!("{short}  ({steps} steps)"))
                    .on_hover_text(id.as_str())
                    .clicked()
                {
                    actions.open_graph_script = Some(id.clone());
                }
                if unknown > 0 {
                    // The honest part: a script a third untranslated
                    // should not look ready.
                    ui.label(
                        egui::RichText::new(format!("{unknown} unknown"))
                            .small()
                            .color(egui::Color32::from_rgb(230, 150, 60)),
                    );
                }
                if ui
                    .add_enabled(state.playing, egui::Button::new("run").small())
                    .on_hover_text("Run it in the game, as it is saved")
                    .on_disabled_hover_text("Switch to Test mode to run scripts")
                    .clicked()
                {
                    actions.run_script = Some(id.clone());
                }
            });
        }
    }

    ui.add_space(10.0);
    source(ui, state, on_canvas);
}

/// The canvas script as readable steps, one line each.
fn source(ui: &mut egui::Ui, state: &UiState<'_>, on_canvas: Option<&str>) {
    ui.heading("Source");
    let Some(script) = on_canvas.and_then(|id| state.project.scripts.get(id)) else {
        ui.label(
            egui::RichText::new("Nothing on the canvas yet. Pick a script above.")
                .small()
                .weak(),
        );
        return;
    };
    ui.label(
        egui::RichText::new("What the canvas holds as saved, one step a line.")
            .small()
            .weak(),
    );
    egui::ScrollArea::vertical()
        .id_salt("script_source")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                lines(ui, &script.actions, 0);
            });
        });
}

/// One indentation level of steps, branches unfolded beneath their test.
fn lines(ui: &mut egui::Ui, actions: &[Action], depth: usize) {
    for action in actions {
        let indent = "    ".repeat(depth);
        match action {
            Action::Branch { then, otherwise, .. } => {
                ui.label(
                    egui::RichText::new(format!("{indent}{}", action.label()))
                        .monospace()
                        .small(),
                );
                lines(ui, then, depth + 1);
                if !otherwise.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("{indent}else"))
                            .monospace()
                            .small()
                            .weak(),
                    );
                    lines(ui, otherwise, depth + 1);
                }
            }
            other => {
                let text = egui::RichText::new(format!("{indent}{}", other.label()))
                    .monospace()
                    .small();
                // An untranslated step is the one line worth spotting.
                if matches!(other, Action::Unknown { .. }) {
                    ui.label(text.color(egui::Color32::from_rgb(230, 150, 60)));
                } else {
                    ui.label(text);
                }
            }
        }
    }
}
