//! Singleplayer (world picker) screen.

use egui::{Color32, RichText, ScrollArea, Vec2};

use crate::world_save::layout::WorldSummary;

use super::{
    MenuAction, MenuContext, MenuScreen, default_new_world, shared,
};

/// Render the world picker. The screen's persistent state lives
/// on the `Singleplayer` variant of `MenuScreen` (search field,
/// selected row, inline forms); the function mutates those in
/// place so re-entry without a clean transition keeps the user's
/// current selection.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    ui: &mut egui::Ui,
    ctx: &mut MenuContext<'_>,
    selected_slug: &mut Option<String>,
    search: &mut String,
    // W8.2: the inline new-world popup is gone; "Vytvorit novy svet"
    // now switches to a dedicated `MenuScreen::NewWorld`. The buffer
    // is kept on the variant only for compatibility with the rest
    // of the state machine + still-existing tests.
    _new_world_open: &mut Option<String>,
    confirm_delete: &mut Option<String>,
    actions: &mut Vec<MenuAction>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.heading("Vyber svet");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Hledat:");
            ui.add_sized(Vec2::new(240.0, 20.0), egui::TextEdit::singleline(search));
        });
        ui.add_space(8.0);
    });

    // Filter rows by case-insensitive substring on the world's
    // display name. Slug isn't included on purpose - it's a
    // filesystem detail, not a user-facing label.
    let needle = search.to_lowercase();
    let filtered: Vec<&WorldSummary> = ctx
        .worlds
        .iter()
        .filter(|w| {
            needle.is_empty() || w.name.to_lowercase().contains(&needle)
        })
        .collect();

    ScrollArea::vertical()
        .id_salt("worlds_list")
        .max_height(280.0)
        .show(ui, |ui| {
            if filtered.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        RichText::new(if ctx.worlds.is_empty() {
                            "Zadne svety. Klikni 'Vytvorit novy svet'..."
                        } else {
                            "Zadne svety neodpovidaji hledani."
                        })
                        .italics(),
                    );
                });
                return;
            }
            for w in &filtered {
                draw_world_row(ui, w, ctx.current_slug, selected_slug);
            }
        });

    // Bottom button bar. Greys out actions that need a selection.
    let has_selection = selected_slug.is_some();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if shared::big_button(ui, "Hrat vybrany svet", has_selection)
            && let Some(slug) = selected_slug.as_ref()
        {
            actions.push(MenuAction::PlayOffline { slug: slug.clone() });
        }
        if shared::small_button(ui, "Vytvorit novy svet") {
            // W8.2: switch to the dedicated new-world screen.
            actions.push(MenuAction::SwitchScreen(default_new_world()));
        }
    });
    ui.horizontal(|ui| {
        if shared::small_button(ui, "Upravit") {
            // TODO Phase G.7+: open rename form. For Wave 4 we
            // surface a chat-style toast so the user knows the
            // button is wired but the feature is stubbed.
            actions.push(MenuAction::SwitchScreen(MenuScreen::Singleplayer {
                selected_slug: selected_slug.clone(),
                search: search.clone(),
                new_world_open: None,
                confirm_delete: confirm_delete.clone(),
            }));
        }
        if shared::small_button(ui, "Opet vytvorit") {
            // TODO Phase G.7+: re-roll the worldgen for an
            // existing slot. Stubbed for Wave 4.
        }
        if shared::small_button(ui, "Smazat")
            && let Some(slug) = selected_slug.as_ref()
        {
            *confirm_delete = Some(slug.clone());
        }
        if shared::small_button(ui, "Zpet") {
            actions.push(MenuAction::SwitchScreen(MenuScreen::Main));
        }
    });

    // W8.2: the inline new-world popup was retired - the screen
    // is now a dedicated `MenuScreen::NewWorld` so the user gets
    // the full form (name + seed + generator + game mode) rather
    // than a single name field jammed into a 200 px window.

    // Delete confirm modal.
    if let Some(slug) = confirm_delete.clone() {
        // Look up the display name for the confirm prompt; fall
        // back to the slug if the row vanished between frames.
        let display = ctx
            .worlds
            .iter()
            .find(|w| w.slug == slug)
            .map(|w| w.name.clone())
            .unwrap_or_else(|| slug.clone());
        let mut keep_open = true;
        let mut do_delete = false;
        egui::Window::new("Smazat svet")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "Smazat svet '{display}'? Tato akce nelze vratit zpet."
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let red = ui.add(egui::Button::new(
                        RichText::new("Smazat")
                            .color(Color32::from_rgb(255, 200, 200)),
                    ));
                    if red.clicked() {
                        do_delete = true;
                        keep_open = false;
                    }
                    if ui.button("Zrusit").clicked() {
                        keep_open = false;
                    }
                });
            });
        if do_delete {
            actions.push(MenuAction::DeleteWorld { slug });
            *selected_slug = None;
        }
        if !keep_open {
            *confirm_delete = None;
        }
    }

    shared::footer_identity(ui, &ctx.options.player_name);
    shared::footer_seed(ui, ctx.world_seed);
    shared::footer_version(ui.ctx(), ctx.engine_version);
}

/// Render one world-list row. Updates `selected_slug` on click;
/// highlights the active selection with a coloured border.
fn draw_world_row(
    ui: &mut egui::Ui,
    w: &WorldSummary,
    current_slug: Option<&str>,
    selected_slug: &mut Option<String>,
) {
    let is_selected = selected_slug.as_deref() == Some(w.slug.as_str());
    let is_active = current_slug == Some(w.slug.as_str());
    let frame = if is_selected {
        egui::Frame::group(ui.style())
            .stroke(egui::Stroke::new(2.0, Color32::from_rgb(120, 200, 255)))
    } else {
        egui::Frame::group(ui.style())
    };
    let resp = frame
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                shared::placeholder_thumb(ui, &w.slug);
                ui.vertical(|ui| {
                    let name_line = if is_active {
                        format!("{}  (aktivni)", w.name)
                    } else {
                        w.name.clone()
                    };
                    ui.label(RichText::new(name_line).strong().size(16.0));
                    ui.label(
                        RichText::new(format!(
                            "{}  -  {}",
                            w.slug,
                            shared::format_last_played(w.last_played)
                        ))
                        .small()
                        .color(Color32::from_gray(180)),
                    );
                    let wg_str = if w.worldgen.is_empty() {
                        "(unknown worldgen)"
                    } else {
                        w.worldgen.as_str()
                    };
                    ui.label(
                        RichText::new(format!(
                            "{wg_str}  -  seed {}",
                            w.seed
                        ))
                        .small()
                        .color(Color32::from_gray(160)),
                    );
                });
            });
        })
        .response
        .interact(egui::Sense::click());
    if resp.clicked() {
        *selected_slug = Some(w.slug.clone());
    }
}
