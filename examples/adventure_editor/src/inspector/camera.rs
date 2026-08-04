//! A83: the shot - how much of the room is seen, and from where.
//!
//! Lives in Room mode because the camera is a property of the room:
//! a scrolling room carries its view size with it wherever a scene
//! points the camera.

use glam::Vec2;

use crate::ui::UiState;

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>) {
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
