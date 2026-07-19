//! Options screen. Edits an `OptionsSnapshot` and ships the
//! whole struct back via `MenuAction::ApplyOptions`.

use egui::{Slider, Vec2};

use super::{MenuAction, MenuContext, MenuScreen, shared};
use crate::VIEW_DISTANCE_MAX;

/// Render the Options screen. Returns to Main on `Zpet`; emits
/// `ApplyOptions` (with a cloned snapshot) on `Pouzit`.
pub(crate) fn draw(
    ui: &mut egui::Ui,
    ctx: &mut MenuContext<'_>,
    actions: &mut Vec<MenuAction>,
) {
    // W311: entry snapshot for the instant-apply diff below.
    let options_at_entry = ctx.options.clone();
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
        Slider::new(&mut ctx.options.view_distance, 2..=VIEW_DISTANCE_MAX as u8)
            .step_by(1.0)
            .text("Render distance (chunky)"),
    );
    ui.add(
        Slider::new(
            &mut ctx.options.lod_distance,
            0..=crate::lod::LOD_DISTANCE_MAX as u8,
        )
        .step_by(1.0)
        .text("LOD distance (chunky, 0 = vyp)"),
    );
    ui.add(
        Slider::new(&mut ctx.options.fov_deg, 30.0..=120.0)
            .step_by(1.0)
            .text("FOV (stupne)"),
    );
    ui.add(
        Slider::new(&mut ctx.options.ui_scale, 0.5..=3.0)
            .text("Meritko UI (GUI scale)"),
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

    // W124: UI language. The discovered-language list is process-
    // stable, so compute it once. Selection applies IMMEDIATELY
    // (sync_menu_options re-mirrors options from settings every
    // frame, so a deferred pick would visually snap back).
    ui.separator();
    static LANG_CHOICES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let choices = LANG_CHOICES.get_or_init(crate::lang::available_languages);
    ui.horizontal(|ui| {
        ui.label("Jazyk / Language:");
        let before = ctx.options.language.clone();
        egui::ComboBox::from_id_salt("options_language_combo")
            .selected_text(ctx.options.language.clone())
            .height(240.0)
            .show_ui(ui, |ui| {
                for code in choices {
                    ui.selectable_value(
                        &mut ctx.options.language,
                        code.clone(),
                        code,
                    );
                }
            });
        if ctx.options.language != before {
            actions.push(MenuAction::ApplyOptions {
                snapshot: ctx.options.clone(),
            });
        }
    });

    // W9.7: resource pack section. Shows the discovered path +
    // a Reload button. The "Cesta" field is a free-form text
    // input; an empty value falls back to the auto-discovered
    // default. The "Aktivni" line is read-only.
    ui.separator();
    ui.label("Resource pack (Minecraft 26.1.x)");
    ui.horizontal(|ui| {
        ui.label("Cesta:");
        ui.add_sized(
            Vec2::new(360.0, 20.0),
            egui::TextEdit::singleline(&mut ctx.options.mc_assets_path)
                .hint_text("auto (z %APPDATA%/.minecraft)"),
        );
    });
    let status_label = if ctx.options.mc_assets_is_fallback {
        format!("Aktivni: (placeholder) {}", ctx.options.mc_assets_resolved)
    } else {
        format!("Aktivni: {}", ctx.options.mc_assets_resolved)
    };
    ui.label(status_label);
    ui.horizontal(|ui| {
        if shared::small_button(ui, "Znovu nacist") {
            actions.push(MenuAction::ReloadResourcePack {
                path: ctx.options.mc_assets_path.clone(),
            });
        }
        if shared::small_button(ui, "Vychozi") {
            ctx.options.mc_assets_path.clear();
            actions.push(MenuAction::ReloadResourcePack {
                path: String::new(),
            });
        }
    });

    // W311: MC-style instant apply - any change commits right away,
    // so Zpet without Pouzit no longer silently discards edits.
    if *ctx.options != options_at_entry {
        actions.push(MenuAction::ApplyOptions {
            snapshot: ctx.options.clone(),
        });
    }
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
