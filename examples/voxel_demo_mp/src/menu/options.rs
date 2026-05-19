//! Options screen. Edits an `OptionsSnapshot` and ships the
//! whole struct back via `MenuAction::ApplyOptions`.

use egui::{Slider, Vec2};

use super::{MenuAction, MenuContext, MenuScreen, shared};

/// Render the Options screen. Returns to Main on `Zpet`; emits
/// `ApplyOptions` (with a cloned snapshot) on `Pouzit`.
pub(crate) fn draw(
    ui: &mut egui::Ui,
    ctx: &mut MenuContext<'_>,
    actions: &mut Vec<MenuAction>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.heading("Moznosti");
        ui.add_space(8.0);
    });

    ui.horizontal(|ui| {
        ui.label("Jmeno hrace:");
        ui.add_sized(
            Vec2::new(220.0, 20.0),
            egui::TextEdit::singleline(&mut ctx.options.player_name),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Master server:");
        ui.add_sized(
            Vec2::new(220.0, 20.0),
            egui::TextEdit::singleline(&mut ctx.options.master_addr),
        );
    });

    ui.separator();
    ui.add(
        Slider::new(&mut ctx.options.view_distance, 2..=16)
            .text("Render distance (chunky)"),
    );
    ui.add(
        Slider::new(&mut ctx.options.fov_deg, 30.0..=120.0)
            .text("FOV (stupne)"),
    );
    ui.add(
        Slider::new(&mut ctx.options.mouse_sensitivity, 0.05..=4.0)
            .text("Citlivost mysi"),
    );
    ui.separator();
    ui.checkbox(&mut ctx.options.show_fps, "Zobrazit FPS");
    ui.checkbox(&mut ctx.options.head_bob, "Animace hlavy pri chuzi");
    ui.checkbox(&mut ctx.options.invert_mouse_y, "Invertovat osu Y");
    ui.checkbox(&mut ctx.options.invert_wheel, "Invertovat kolecko");
    ui.checkbox(&mut ctx.options.lock_cursor, "Zamknout kurzor");
    ui.checkbox(&mut ctx.options.hide_cursor, "Skryt kurzor");

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if shared::big_button(ui, "Pouzit", true) {
            // Clone the (already in-place) snapshot so the action
            // doesn't share a borrow with `ctx`. `clamp` happens
            // on the consumer side so the screen can keep showing
            // whatever the user typed.
            actions.push(MenuAction::ApplyOptions {
                snapshot: ctx.options.clone(),
            });
        }
        if shared::small_button(ui, "Zpet") {
            actions.push(MenuAction::SwitchScreen(MenuScreen::Main));
        }
    });

    shared::footer_version(ui.ctx(), ctx.engine_version);
}
