//! Main menu screen. Title banner, animated splash text, four big
//! buttons, and combined footer. W21.1 redesign.

use egui::{Color32, RichText};

use super::{
    MenuAction, MenuContext, MenuScreen, default_multiplayer,
    default_singleplayer, shared,
};

/// Render the root menu. Emits `SwitchScreen` actions for the
/// three navigation buttons + `Quit` for the exit button. The
/// version footer is drawn on the surrounding `egui::Context` so
/// it stays anchored to the bottom-right regardless of the central
/// panel layout.
pub(crate) fn draw(
    ui: &mut egui::Ui,
    ctx: &mut MenuContext<'_>,
    actions: &mut Vec<MenuAction>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);

        // --- Title banner (W21.1) ---
        // "InfEngine" in large bold text.
        ui.label(
            RichText::new("InfEngine")
                .size(56.0)
                .strong()
                .color(Color32::from_rgb(255, 255, 255)),
        );
        // Subtitle line.
        ui.label(
            RichText::new("Voxel Demo")
                .size(18.0)
                .color(Color32::from_rgb(160, 180, 200)),
        );

        // --- Splash text (W21.1, W23.1) ---
        // Yellow italic line with a subtle alpha pulse animation.
        // W23.1: switched from scale-pulse to alpha-pulse so the
        // label's bounding box stays constant (fixed 16 px) and
        // the surrounding layout (button stack) does not shift up/down
        // every frame. Alpha oscillates between 0.6 and 1.0.
        if let Some(splash) = ctx.splash_text {
            let t = ui.input(|i| i.time) as f32;
            // Alpha cycles 0.6 -> 1.0 -> 0.6 with a ~1.6 s period.
            let a = (0.6 + 0.4 * ((t * 4.0).sin() * 0.5 + 0.5)).clamp(0.6, 1.0);
            let alpha = (a * 255.0) as u8;
            ui.label(
                RichText::new(splash)
                    .size(16.0)
                    .italics()
                    .color(Color32::from_rgba_unmultiplied(255, 255, 0, alpha)),
            );
            // Request repaint so the pulse animates every frame.
            ui.ctx().request_repaint();
        }

        ui.add_space(32.0);

        if shared::big_button(ui, "Hra jednoho hrace", true) {
            actions.push(MenuAction::SwitchScreen(default_singleplayer()));
        }
        ui.add_space(8.0);
        if shared::big_button(ui, "Hra vice hracu", true) {
            actions.push(MenuAction::SwitchScreen(default_multiplayer()));
        }
        ui.add_space(8.0);
        if shared::big_button(ui, "Moznosti...", true) {
            actions.push(MenuAction::SwitchScreen(MenuScreen::Options));
        }
        ui.add_space(8.0);
        if shared::big_button(ui, "Ukoncit hru", true) {
            actions.push(MenuAction::Quit);
        }

        ui.add_space(32.0);

        // --- Combined footer (W21.4) ---
        shared::footer_combined(
            ui,
            &ctx.options.player_name,
            ctx.world_seed,
            ctx.pack_name,
            ctx.pack_block_count,
        );
    });

    // Bottom-right version footer. Anchored on the egui context so
    // the central panel doesn't have to reserve space for it.
    shared::footer_version(ui.ctx(), ctx.engine_version);
}
