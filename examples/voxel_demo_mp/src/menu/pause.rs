//! W21.5: in-game pause menu rendered over the 3-D world.
//!
//! Extracted from the inline `AppState::Paused` block in `main.rs`
//! so the styling is consistent with the main menu's `big_button`
//! widget. Background: 50% black tint rendered before the central
//! panel via a background-layer `egui::Painter`.
//!
//! Returns a `PauseAction` so the caller in `main.rs` stays
//! decoupled from the egui details.

use egui::Color32;

use super::shared;

/// Actions the pause panel can emit. Drained by `main.rs` after
/// the egui pass exactly as `MenuAction` is for the main menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PauseAction {
    /// Resume gameplay (close overlay).
    Resume,
    /// Open the in-game settings screen.
    Settings,
    /// Disconnect + return to the main menu.
    MainMenu,
}

/// Draw the pause overlay. Must be called from inside the egui
/// closure. Pushes any emitted actions into `actions`.
///
/// `world_seed` is displayed in the footer so the player can
/// confirm which world is loaded without diving into settings.
pub(crate) fn draw(
    ctx: &egui::Context,
    world_seed: u32,
    actions: &mut Vec<PauseAction>,
) {
    // --- 50% black background tint (W21.5) ---
    let screen = ctx.content_rect();
    let bg_layer = egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("pause_bg_tint"),
    );
    ctx.layer_painter(bg_layer).rect_filled(
        screen,
        0.0,
        Color32::from_rgba_unmultiplied(0, 0, 0, 128),
    );

    // --- Central pause panel ---
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);

                ui.label(
                    egui::RichText::new("Paused")
                        .size(36.0)
                        .strong()
                        .color(Color32::from_rgb(255, 255, 255)),
                );
                ui.add_space(24.0);

                if shared::big_button(ui, "Resume", true) {
                    actions.push(PauseAction::Resume);
                }
                ui.add_space(8.0);
                if shared::big_button(ui, "Settings", true) {
                    actions.push(PauseAction::Settings);
                }
                ui.add_space(8.0);
                if shared::big_button(ui, "Main Menu", true) {
                    actions.push(PauseAction::MainMenu);
                }

                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new(format!("Seed: {world_seed}"))
                        .color(Color32::from_rgb(150, 180, 180))
                        .small()
                        .italics(),
                );
            });
        });
}
