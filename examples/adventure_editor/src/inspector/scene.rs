//! A58.1: watching a scene run, and adjusting the shot.
//!
//! Everything else in this editor edits a room standing still. A
//! cutscene is not a still: it is a logo appearing, a title over it, a
//! view panning down, an animation playing out. None of that could be
//! seen without launching the game and watching the whole intro go by,
//! and none of it could be adjusted without editing a file by hand and
//! launching it again.
//!
//! So this tab runs one of the room's scripts and shows the clock while
//! it does, with the two things a cutscene is made of - where the camera
//! is and how much of the room it sees - editable while it plays. A pan
//! that ends too low is a number you change and watch again, not a file
//! you reopen.

use glam::Vec2;

use crate::ui::{UiActions, UiState};

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    transport(ui, state, actions);
    ui.add_space(10.0);
    camera(ui, state);
    ui.add_space(10.0);
    scripts(ui, state, actions);
}

/// Play, stop, and what the clock says.
fn transport(ui: &mut egui::Ui, state: &UiState<'_>, actions: &mut UiActions) {
    ui.horizontal(|ui| {
        if state.playing {
            if ui.button("Stop").clicked() {
                actions.stop_play = true;
            }
            // The clock is the point: "the pan starts at 4 seconds" is
            // something you can act on, and "it happens after the title"
            // is not.
            ui.label(
                egui::RichText::new(format!("{:.1}s", state.elapsed))
                    .monospace()
                    .strong(),
            );
        } else if ui
            .button("Play scene")
            .on_hover_text("Run this room's entry script from the start")
            .clicked()
        {
            actions.run_script = Some(String::new());
        }
    });

    if state.playing {
        ui.label(
            egui::RichText::new(if state.now.is_empty() {
                "waiting"
            } else {
                state.now.as_str()
            })
            .small()
            .weak(),
        );
    } else {
        ui.label(
            egui::RichText::new("Nothing running. Play runs the room's own script.")
                .small()
                .weak(),
        );
    }
}

/// The shot: how much of the room is seen, and from where.
fn camera(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    ui.heading("Camera");
    let size = state.room.size;
    let mut view = state.room.camera.view;

    let mut whole = view.x <= 0.0 && view.y <= 0.0;
    if ui
        .checkbox(&mut whole, "The whole room is on screen")
        .on_hover_text("Untick for a room the view moves around inside")
        .changed()
    {
        // Turning it off starts from the room's own size rather than
        // from zero, so the first drag is an adjustment rather than a
        // room that has vanished.
        view = if whole { Vec2::ZERO } else { size };
        state.room.camera.view = view;
    }

    if !whole {
        egui::Grid::new("camera_grid").num_columns(2).show(ui, |ui| {
            ui.label("view");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut state.room.camera.view.x)
                        .range(16.0..=size.x.max(16.0))
                        .speed(1.0),
                );
                ui.label("x");
                ui.add(
                    egui::DragValue::new(&mut state.room.camera.view.y)
                        .range(16.0..=size.y.max(16.0))
                        .speed(1.0),
                );
            });
            ui.end_row();
        });
        ui.checkbox(&mut state.room.camera.follow, "Follow the actor")
            .on_hover_text("Off for a cutscene, which has no actor to follow");

        let moves = state.room.camera.view_in(size);
        ui.label(
            egui::RichText::new(format!(
                "{} x {} of {} x {} - the view can travel {} across and {} down",
                moves.x as i32,
                moves.y as i32,
                size.x as i32,
                size.y as i32,
                (size.x - moves.x).max(0.0) as i32,
                (size.y - moves.y).max(0.0) as i32,
            ))
            .small()
            .weak(),
        );
    }

    if state.playing {
        ui.label(
            egui::RichText::new(format!(
                "now at {:.0}, {:.0}",
                state.camera.x, state.camera.y
            ))
            .small()
            .monospace(),
        );
    }
}

/// The scripts this room can run, each one runnable on its own.
fn scripts(ui: &mut egui::Ui, state: &UiState<'_>, actions: &mut UiActions) {
    // By the room's script prefix, not its id: rooms that share a
    // background share their logic, and asking for the id would show an
    // empty list for twelve of the thirteen forest rooms.
    let prefix = format!("{}.", state.room.script_prefix());
    let mut ids: Vec<&String> = state
        .project
        .scripts
        .keys()
        .filter(|id| id.starts_with(&prefix))
        .collect();
    ids.sort();

    ui.heading(format!("Scripts ({})", ids.len()));
    if ids.is_empty() {
        super::nothing_yet(
            ui,
            "This room runs nothing.",
            "A script arrives with an import, or is drawn in the script graph.",
        );
        return;
    }
    ui.label(
        egui::RichText::new("Click one to run it from here.")
            .small()
            .weak(),
    );

    egui::ScrollArea::vertical()
        .id_salt("scene_scripts")
        .max_height(200.0)
        .show(ui, |ui| {
            for id in ids {
                let steps = state
                    .project
                    .scripts
                    .get(id)
                    .map(|script| script.actions.len())
                    .unwrap_or(0);
                let short = id.strip_prefix(&prefix).unwrap_or(id);
                if ui
                    .selectable_label(false, format!("{short}  ({steps} steps)"))
                    .on_hover_text(id)
                    .clicked()
                {
                    actions.run_script = Some(id.clone());
                }
            }
        });
}
