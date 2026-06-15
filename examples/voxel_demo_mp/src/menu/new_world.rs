//! W8.2: dedicated "Vytvorit novy svet" screen. Replaces the
//! inline popup the Singleplayer screen used in Wave 4 with a
//! full-screen form: name + seed + generator + game mode.
//!
//! The form buffers live on the `MenuScreen::NewWorld` variant so
//! a re-entry from the Singleplayer screen keeps the user's
//! pending edits. `Vytvorit svet` emits
//! `MenuAction::NewWorld { name, worldgen, seed, game_mode }`
//! which the bin's `apply_menu_action` routes through the
//! existing create-world-then-enter flow.

use egui::{Color32, RichText, Vec2};

use crate::game_mode::GameMode;

use super::{
    MenuAction, MenuContext, default_singleplayer, parse_seed_field,
    shared, validate_world_name,
};

/// Generator picks the user can choose from the dropdown. The
/// strings match `WorldGen::from_str` in `worldgen.rs` so the bin
/// can route the pick through the existing per-dim worldgen path
/// without translating tags.
pub(crate) const GENERATOR_TAGS: &[&str] = &[
    "mc",
    "natural",
    "realistic",
    "classic",
    "flat",
    "islands",
    "amplified",
];

/// Render the dedicated new-world screen. The function mutates
/// the form buffers in place so re-entry without a clean
/// transition keeps whatever the user typed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    ui: &mut egui::Ui,
    ctx: &mut MenuContext<'_>,
    name: &mut String,
    seed: &mut String,
    generator: &mut String,
    game_mode: &mut GameMode,
    actions: &mut Vec<MenuAction>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.heading("Vytvorit novy svet");
        ui.add_space(8.0);
    });

    let form_width = 360.0;
    let field_width = Vec2::new(form_width, 20.0);

    // Name field. The "Vytvorit" button is greyed out until the
    // name validates, so a user who hits Submit with an empty
    // name doesn't silently produce a slug like `default`.
    ui.vertical_centered(|ui| {
        ui.allocate_ui(Vec2::new(form_width, 0.0), |ui| {
            ui.label("Nazev:");
            ui.add_sized(field_width, egui::TextEdit::singleline(name));
            ui.add_space(8.0);

            // Seed field + small random button. Empty input
            // means "pick a random seed at create time"; numeric
            // input pins it exactly. The hint stays under the
            // field so the user knows the empty-state behaviour.
            ui.label("Seed:");
            ui.horizontal(|ui| {
                ui.add_sized(
                    Vec2::new(form_width - 80.0, 20.0),
                    egui::TextEdit::singleline(seed)
                        .hint_text("(empty = random)"),
                );
                if ui.button("Random").clicked() {
                    *seed = fresh_random_seed_str();
                }
            });
            ui.add_space(8.0);

            // Generator dropdown.
            ui.label("Generator:");
            egui::ComboBox::from_id_salt("new_world_generator")
                .selected_text(generator.as_str())
                .width(form_width)
                .show_ui(ui, |ui| {
                    for tag in GENERATOR_TAGS {
                        ui.selectable_value(generator, (*tag).to_string(), *tag);
                    }
                });
            ui.add_space(8.0);

            // Game mode dropdown.
            ui.label("Rezim hry:");
            egui::ComboBox::from_id_salt("new_world_game_mode")
                .selected_text(game_mode.label_cs())
                .width(form_width)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        game_mode,
                        GameMode::Survival,
                        GameMode::Survival.label_cs(),
                    );
                    ui.selectable_value(
                        game_mode,
                        GameMode::Creative,
                        GameMode::Creative.label_cs(),
                    );
                });
            ui.add_space(16.0);

            // Submit + back.
            let valid = validate_world_name(name).is_some();
            ui.horizontal(|ui| {
                if shared::big_button(ui, "Vytvorit svet", valid)
                    && let Some(clean_name) = validate_world_name(name)
                {
                    actions.push(MenuAction::NewWorld {
                        name: clean_name,
                        worldgen: generator.clone(),
                        seed: parse_seed_field(seed),
                        game_mode: *game_mode,
                    });
                }
                if shared::small_button(ui, "Zpet") {
                    actions.push(MenuAction::SwitchScreen(default_singleplayer()));
                }
            });
            if !valid {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Zadej jmeno sveta (1-64 znaku).")
                        .small()
                        .color(Color32::from_rgb(220, 150, 150)),
                );
            }
        });
    });

    shared::footer_identity(ui, &ctx.options.player_name);
    shared::footer_seed(ui, ctx.world_seed);
    shared::footer_version(ui.ctx(), ctx.engine_version);
}

/// Generate a fresh u32 seed string. Used by the "Random" button
/// to populate the seed field with a sample value the user can
/// then edit if they want a memorable number.
fn fresh_random_seed_str() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        .wrapping_mul(0x9E37_79B1)
        | 1;
    format!("{s}")
}
