//! Main menu screen. Four big buttons + identity/seed footer.

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
        ui.add_space(60.0);
        ui.heading("InfEngine - Voxel Demo");
        ui.add_space(40.0);

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

        ui.add_space(40.0);
        shared::footer_identity(ui, &ctx.options.player_name);
        shared::footer_seed(ui, ctx.world_seed);
    });
    // Bottom-right version footer. Anchored on the egui context so
    // the central panel doesn't have to reserve space for it.
    shared::footer_version(ui.ctx(), ctx.engine_version);
}
