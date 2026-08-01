//! A41.3: where the actor may stand.
//!
//! The floor and the arrival points belong together: an entry point that
//! lands off the floor puts the actor somewhere they cannot walk out of,
//! and that is the failure this tab exists to make visible. So each
//! entry is checked against the walk area and says so.

use crate::edit::Tool;
use crate::ui::UiState;

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    let solids = state.room.walk.solids.len();
    let holes = state.room.walk.holes.len();

    ui.heading("Walkable area");
    if solids == 0 {
        super::nothing_yet(
            ui,
            "No floor yet.",
            "With Outline, click around the part of the picture the actor \
             should be able to reach, then right-click to close it. \
             Until then the actor cannot move.",
        );
    } else {
        ui.label(format!(
            "{solids} outline{} and {holes} hole{}",
            plural(solids),
            plural(holes)
        ));
        ui.label(
            egui::RichText::new(
                "A hole is an area inside the floor the actor must walk around - \
                 a table, a pond.",
            )
            .small()
            .weak(),
        );
        if *state.tool != Tool::WalkPreview {
            ui.label(
                egui::RichText::new("Test walk checks it without leaving the editor.")
                    .small()
                    .weak(),
            );
        }
    }

    ui.add_space(10.0);
    ui.heading("Arrival points");
    ui.label(
        egui::RichText::new("Where the actor appears when a door leads here.")
            .small()
            .weak(),
    );

    if state.room.entries.is_empty() {
        super::nothing_yet(
            ui,
            "None.",
            "Without one, an actor entering this room starts at the origin.",
        );
        return;
    }

    let walk = state.room.walk.clone();
    egui::Grid::new("entry_grid")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            for entry in &mut state.room.entries {
                ui.text_edit_singleline(&mut entry.id);
                ui.label(format!(
                    "{}, {}",
                    entry.position.x as i32, entry.position.y as i32
                ));
                // An arrival point off the floor is the bug this tab is
                // for; naming it here beats finding it by playing.
                if walk.is_empty() {
                    ui.label(egui::RichText::new("no floor to check").small().weak());
                } else if walk.contains(entry.position) {
                    ui.label(egui::RichText::new("on the floor").small().weak());
                } else {
                    ui.label(
                        egui::RichText::new("off the floor")
                            .small()
                            .color(egui::Color32::from_rgb(255, 140, 120)),
                    )
                    .on_hover_text("The actor would arrive somewhere they cannot walk out of.");
                }
                ui.end_row();
            }
        });
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals_read_naturally() {
        assert_eq!(plural(1), "");
        assert_eq!(plural(0), "s");
        assert_eq!(plural(2), "s");
    }
}
