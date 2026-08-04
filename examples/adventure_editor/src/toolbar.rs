//! A85: the strip above the canvas - the mode's tools, what the active
//! one does, and the zoom.
//!
//! The tools used to live inside the inspector, which meant the thing
//! that decides what the mouse does was a column away from where the
//! mouse works. Now they sit over the canvas with the active tool's
//! hint beside them, and the zoom - presets, the current figure, Fit -
//! keeps to the right edge the way every picture tool keeps it.
//!
//! Script mode draws no toolbar: the graph fills the centre and carries
//! its own header, and offering zoom buttons for a room nobody can see
//! would be a lie.

use crate::mode::Mode;
use crate::ui::UiState;
use crate::view::View;

/// The zoom figures worth a dedicated button: half, actual, double.
const PRESETS: [(&str, f32); 3] = [("50%", 0.5), ("100%", 1.0), ("200%", 2.0)];

pub fn panel(ctx: &egui::Context, state: &mut UiState<'_>, view: &mut View) {
    if state.mode == Mode::Script {
        return;
    }
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let tools = state.mode.tools();
            if tools.is_empty() {
                ui.label(egui::RichText::new(state.mode.hint()).small().weak());
            } else {
                for tool in tools {
                    if ui
                        .selectable_label(
                            *state.tool == *tool,
                            crate::inspector::tool_label(*tool),
                        )
                        .on_hover_text(crate::inspector::tool_hint(*tool))
                        .clicked()
                    {
                        *state.tool = *tool;
                    }
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(crate::inspector::tool_hint(*state.tool))
                        .small()
                        .weak(),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                use crate::lang::t;
                if ui
                    .button(t("Fit", "Zarámovat"))
                    .on_hover_text(t("Frame the whole room (F)", "Ukázat celou místnost (F)"))
                    .clicked()
                {
                    view.fit();
                }
                for (label, zoom) in PRESETS {
                    let on = (view.zoom - zoom).abs() < 0.005;
                    if ui
                        .selectable_label(on, label)
                        .on_hover_text(t(
                            "Pixels of screen per pixel of room",
                            "Pixely obrazovky na pixel místnosti",
                        ))
                        .clicked()
                    {
                        view.zoom = zoom;
                    }
                }
                // The live figure, because the wheel lands between the
                // presets and the number is the honest answer.
                ui.label(
                    egui::RichText::new(format!("{:.0}%", view.zoom * 100.0))
                        .small()
                        .monospace()
                        .weak(),
                );
            });
        });
    });
}
