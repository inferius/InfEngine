//! A20.3: the play-mode panel - the tester's controls.
//!
//! Kept apart from the editing UI on purpose. Everything here is about
//! *checking* a game rather than authoring one: which flags are set,
//! what the player is carrying, which scene to jump to, and a running
//! log of what the scripts actually did.

use infengine_adventure::Verb;

use crate::play::PlayMode;

/// What the panel asked the host to do.
#[derive(Debug, Default)]
pub struct PlayActions {
    pub start: bool,
    pub stop: bool,
    pub reset: bool,
    /// A flag the tester toggled by hand.
    pub set_flag: Option<(String, bool)>,
    /// A flag the tester typed in.
    pub add_flag: Option<String>,
    pub give_item: Option<String>,
    pub take_item: Option<String>,
    /// A script the tester asked to run.
    pub run_script: Option<String>,
    /// The game window was asked for.
    pub open_game: bool,
}

/// Draft text the panel keeps between frames.
#[derive(Debug, Default)]
pub struct PlayDraft {
    pub flag: String,
    pub item: String,
}

/// The play-mode side panel and, while playing, the subtitle.
pub fn panel(
    ctx: &egui::Context,
    play: &mut PlayMode,
    draft: &mut PlayDraft,
    room: &str,
) -> PlayActions {
    let mut actions = PlayActions::default();

    egui::SidePanel::right("play_panel")
        .default_width(260.0)
        .show(ctx, |ui| {
            ui.heading("Test");
            ui.horizontal(|ui| {
                if play.active {
                    if ui.button("stop").clicked() {
                        actions.stop = true;
                    }
                } else if ui.button("play").clicked() {
                    actions.start = true;
                }
                if ui
                    .button("game...")
                    .on_hover_text("Title, starting room, languages, export")
                    .clicked()
                {
                    actions.open_game = true;
                }
                if ui
                    .button("reset")
                    .on_hover_text("Clear flags, inventory and log, then restart the room")
                    .clicked()
                {
                    actions.reset = true;
                }
            });

            if !play.active {
                ui.label(
                    egui::RichText::new(
                        "Not playing. Flags and inventory below are kept for the next run.",
                    )
                    .weak(),
                );
            }

            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label("verb");
                for verb in Verb::ALL {
                    ui.selectable_value(&mut play.verb, verb, verb.name());
                }
            });
            ui.label(
                egui::RichText::new(format!("cursor: {}", play.hover.label()))
                    .small()
                    .weak(),
            );

            ui.separator();
            scripts_section(ui, play, room, &mut actions);
            ui.separator();
            flags_section(ui, play, draft, &mut actions);
            ui.separator();
            inventory_section(ui, play, draft, &mut actions);
            ui.separator();
            log_section(ui, play);
        });

    if play.active {
        subtitle(ctx, play);
    }
    actions
}

/// The room's scripts, runnable by hand.
///
/// An imported game arrives with its logic decompiled but nothing
/// pointing at it yet, so this is the only way to see what a script
/// does until its hotspot has been drawn.
fn scripts_section(
    ui: &mut egui::Ui,
    play: &PlayMode,
    room: &str,
    actions: &mut PlayActions,
) {
    let ids: Vec<String> = play
        .game
        .scripts_for(room)
        .into_iter()
        .map(str::to_string)
        .collect();
    ui.heading(format!("Scripts ({})", ids.len()));
    if ids.is_empty() {
        ui.label(egui::RichText::new("This room has none.").weak());
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("script_list")
        .max_height(140.0)
        .show(ui, |ui| {
            for id in ids {
                let unknown = play
                    .game
                    .script(&id)
                    .map(|script| script.unknown_count())
                    .unwrap_or(0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(play.active, egui::Button::new("run"))
                        .clicked()
                    {
                        actions.run_script = Some(id.clone());
                    }
                    // The count is the honest part: a script that is a
                    // third untranslated should not look ready.
                    let label = if unknown == 0 {
                        egui::RichText::new(&id).small()
                    } else {
                        egui::RichText::new(format!("{id}  ({unknown} unknown)"))
                            .small()
                            .color(egui::Color32::from_rgb(230, 150, 60))
                    };
                    ui.label(label);
                });
            }
        });
}

fn flags_section(
    ui: &mut egui::Ui,
    play: &mut PlayMode,
    draft: &mut PlayDraft,
    actions: &mut PlayActions,
) {
    ui.heading(format!("Flags ({})", play.game.state.flags.len()));
    egui::ScrollArea::vertical()
        .id_salt("flag_list")
        .max_height(160.0)
        .show(ui, |ui| {
            // Collected first: the checkbox writes into the map that is
            // being iterated.
            let flags: Vec<(String, bool)> = play
                .game
                .state
                .flags
                .iter()
                .map(|(name, value)| (name.clone(), *value))
                .collect();
            for (name, value) in flags {
                let mut on = value;
                if ui.checkbox(&mut on, &name).changed() {
                    actions.set_flag = Some((name, on));
                }
            }
        });

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut draft.flag)
                .hint_text("new flag")
                .desired_width(140.0),
        );
        // A flag a script has never set does not exist yet, so setting
        // up a mid-game scene means being able to invent one.
        if ui.button("+").clicked() && !draft.flag.trim().is_empty() {
            actions.add_flag = Some(draft.flag.trim().to_string());
            draft.flag.clear();
        }
    });
}

fn inventory_section(
    ui: &mut egui::Ui,
    play: &mut PlayMode,
    draft: &mut PlayDraft,
    actions: &mut PlayActions,
) {
    ui.heading(format!("Inventory ({})", play.game.state.inventory.len()));
    for item in play.game.state.inventory.clone() {
        ui.horizontal(|ui| {
            if ui.small_button("x").clicked() {
                actions.take_item = Some(item.clone());
            }
            ui.label(item);
        });
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut draft.item)
                .hint_text("give item")
                .desired_width(140.0),
        );
        if ui.button("+").clicked() && !draft.item.trim().is_empty() {
            actions.give_item = Some(draft.item.trim().to_string());
            draft.item.clear();
        }
    });
}

fn log_section(ui: &mut egui::Ui, play: &PlayMode) {
    let skipped = play
        .log
        .iter()
        .filter(|line| line.starts_with("SKIPPED"))
        .count();
    ui.heading(if skipped == 0 {
        "Log".to_string()
    } else {
        format!("Log ({skipped} skipped)")
    });
    egui::ScrollArea::vertical()
        .id_salt("play_log")
        .max_height(220.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in &play.log {
                // An untranslated step is the one line worth spotting
                // from across the room.
                let text = egui::RichText::new(line).small();
                if line.starts_with("SKIPPED") {
                    ui.label(text.color(egui::Color32::from_rgb(230, 150, 60)));
                } else {
                    ui.label(text.weak());
                }
            }
        });
}

/// The line being spoken, across the bottom of the room.
fn subtitle(ctx: &egui::Context, play: &PlayMode) {
    let Some(subtitle) = play.subtitle.as_ref() else {
        return;
    };
    let screen = ctx.content_rect();
    egui::Area::new("subtitle".into())
        .fixed_pos(egui::pos2(screen.center().x, screen.max.y - 90.0))
        .pivot(egui::Align2::CENTER_CENTER)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_black_alpha(200))
                .show(ui, |ui| {
                    ui.set_max_width(screen.width() * 0.6);
                    if let Some(speaker) = &subtitle.speaker {
                        ui.label(
                            egui::RichText::new(speaker)
                                .small()
                                .color(egui::Color32::from_rgb(200, 190, 140)),
                        );
                    }
                    ui.label(
                        egui::RichText::new(&subtitle.text)
                            .size(18.0)
                            .color(egui::Color32::WHITE),
                    );
                });
        });
}
