//! A22.4: the game panel - what the project is, and shipping it.
//!
//! One window, opened from the play panel, holding the things that
//! belong to the game rather than to a room: its title, where it
//! starts, which languages it offers, and what an export would take.
//!
//! The plan is shown before the export runs, because the interesting
//! part of an export is what it *cannot* find. Copying is the easy half.

use infengine_adventure::pack::{GameManifest, Plan};

/// State the panel keeps between frames.
#[derive(Debug, Default)]
pub struct GameUi {
    pub open: bool,
    pub manifest: GameManifest,
    /// The last plan worked out, and whether it is still current.
    pub plan: Option<Plan>,
    pub export_to: String,
    pub status: String,
}

#[derive(Debug, Default)]
pub struct GameActions {
    pub save_manifest: bool,
    /// Work out what an export would take.
    pub check: bool,
    /// Copy it to `export_to`.
    pub export: bool,
}

/// The game window. Returns what it asked for.
pub fn window(
    ctx: &egui::Context,
    game: &mut GameUi,
    rooms: &[String],
    languages: &[String],
) -> GameActions {
    let mut actions = GameActions::default();
    let mut open = game.open;

    egui::Window::new("Game")
        .open(&mut open)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("title");
                ui.text_edit_singleline(&mut game.manifest.title);
            });

            // A95: the game's own resolution. Every room is measured
            // against it to know whether it scrolls, so it belongs
            // with the title rather than buried per room.
            ui.horizontal(|ui| {
                ui.label(crate::lang::t("window", "okno"))
                    .on_hover_text(crate::lang::t(
                        "How much of a room the player sees at once. A bigger room scrolls.",
                        "Kolik z místnosti hráč vidí najednou. Větší místnost se posouvá.",
                    ));
                ui.add(
                    egui::DragValue::new(&mut game.manifest.view.x)
                        .range(64.0..=4096.0)
                        .speed(1.0),
                );
                ui.label("x");
                ui.add(
                    egui::DragValue::new(&mut game.manifest.view.y)
                        .range(64.0..=4096.0)
                        .speed(1.0),
                );
            });

            ui.horizontal(|ui| {
                ui.label("starts in");
                egui::ComboBox::from_id_salt("start_room")
                    .selected_text(game.manifest.start_room.clone())
                    .show_ui(ui, |ui| {
                        for room in rooms {
                            ui.selectable_value(
                                &mut game.manifest.start_room,
                                room.clone(),
                                room,
                            );
                        }
                    });
                ui.label("at");
                ui.add(
                    egui::TextEdit::singleline(&mut game.manifest.start_entry)
                        .hint_text("first entry")
                        .desired_width(110.0),
                );
            });

            languages_row(ui, game, languages);

            if ui.button("save game.ron").clicked() {
                actions.save_manifest = true;
            }

            ui.separator();
            ui.heading("Export");
            if ui
                .button("check")
                .on_hover_text("List what an export would take, and what it cannot find")
                .clicked()
            {
                actions.check = true;
            }
            if let Some(plan) = &game.plan {
                plan_report(ui, plan);
            }

            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut game.export_to)
                        .hint_text("destination folder")
                        .desired_width(240.0),
                );
                let ready = game.plan.is_some() && !game.export_to.trim().is_empty();
                if ui.add_enabled(ready, egui::Button::new("export")).clicked() {
                    actions.export = true;
                }
            });

            if !game.status.is_empty() {
                ui.label(egui::RichText::new(&game.status).small().weak());
            }
        });

    game.open = open;
    actions
}

fn languages_row(ui: &mut egui::Ui, game: &mut GameUi, languages: &[String]) {
    ui.horizontal_wrapped(|ui| {
        ui.label("languages");
        for language in languages {
            let mut on = game.manifest.languages.contains(language);
            if ui.checkbox(&mut on, language).changed() {
                if on {
                    game.manifest.languages.push(language.clone());
                    game.manifest.languages.sort();
                } else {
                    game.manifest.languages.retain(|kept| kept != language);
                }
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("default text");
        language_pick(ui, "default_text", languages, &mut game.manifest.default_language.text);
        ui.label("voice");
        language_pick(ui, "default_voice", languages, &mut game.manifest.default_language.voice);
    });
}

fn language_pick(ui: &mut egui::Ui, salt: &str, languages: &[String], current: &mut String) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(current.clone())
        .width(60.0)
        .show_ui(ui, |ui| {
            for language in languages {
                ui.selectable_value(current, language.clone(), language);
            }
        });
}

fn plan_report(ui: &mut egui::Ui, plan: &Plan) {
    ui.label(format!(
        "{} files, {:.1} MB",
        plan.files.len(),
        plan.bytes as f64 / 1_048_576.0
    ));

    if !plan.missing.is_empty() {
        // Loud, and listed by name. This is the one thing that turns an
        // export into a game that crashes on somebody else's machine.
        ui.label(
            egui::RichText::new(format!("{} files missing", plan.missing.len()))
                .color(egui::Color32::from_rgb(230, 90, 70)),
        );
        egui::ScrollArea::vertical()
            .id_salt("missing_list")
            .max_height(120.0)
            .show(ui, |ui| {
                for path in &plan.missing {
                    ui.label(egui::RichText::new(path).small());
                }
            });
    }

    if !plan.unmapped.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "{} sound ids with no file - they will be silent",
                plan.unmapped.len()
            ))
            .color(egui::Color32::from_rgb(230, 150, 60)),
        );
    }

    if plan.is_shippable() && plan.unmapped.is_empty() {
        ui.label(egui::RichText::new("nothing missing").weak());
    }
}
