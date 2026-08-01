//! A41.2: what this screen is.
//!
//! Identity and pictures. The layer list is here rather than under its
//! own heading because a room's layers *are* what the room looks like -
//! asking "what is this screen" and being shown its name but not its
//! backdrop would be a strange answer.

use crate::ui::UiState;

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    egui::Grid::new("room_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("id");
            ui.text_edit_singleline(&mut state.room.id)
                .on_hover_text("What exits and save files call this room. Changing it breaks both.");
            ui.end_row();

            ui.label("name");
            ui.text_edit_singleline(&mut state.room.name)
                .on_hover_text("What the player sees, if the game shows a room title.");
            ui.end_row();

            ui.label("size");
            ui.label(format!(
                "{} x {}",
                state.room.size.x as i32, state.room.size.y as i32
            ))
            .on_hover_text("Taken from the background when the room was imported.");
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.heading("Pictures");
    if state.room.layers.is_empty() {
        super::nothing_yet(
            ui,
            "Nothing is painted here.",
            "A room with no layers draws as empty black.",
        );
        return;
    }

    ui.label(
        egui::RichText::new("Painted back to front. Untick one to see behind it.")
            .small()
            .weak(),
    );
    for layer in &mut state.room.layers {
        ui.horizontal(|ui| {
            ui.checkbox(&mut layer.visible, "");
            let missing = state.missing_images.contains(&layer.image);
            let mut text = egui::RichText::new(if layer.name.is_empty() {
                layer.image.as_str()
            } else {
                layer.name.as_str()
            });
            if missing {
                text = text.color(egui::Color32::from_rgb(255, 140, 120));
            }
            ui.label(text).on_hover_text(if missing {
                format!("{} - not found on disk", layer.image)
            } else {
                layer.image.clone()
            });
        });
    }
}
