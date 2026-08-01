//! A41.6: what is said here.
//!
//! Text and recordings are one list, not two, because the question worth
//! asking is never "how many lines" but "which lines have no voice yet".
//! The two language pickers stay separate: checking a translation
//! against the original delivery is a normal thing to want, and it needs
//! the text in one language and the audio in another.

use crate::ui::{UiActions, UiState};

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    let total = state.lines.len();
    let voiced = state.lines.iter().filter(|(_, _, has)| *has).count();

    if state.languages.len() > 1 {
        ui.horizontal(|ui| {
            ui.label("read");
            picker(ui, "text_lang", state.languages, state.text_language);
            ui.label("hear");
            picker(ui, "voice_lang", state.languages, state.voice_language);
        });
        ui.label(
            egui::RichText::new("Two pickers so a translation can be checked against the original.")
                .small()
                .weak(),
        );
        ui.separator();
    }

    if total == 0 {
        super::nothing_yet(
            ui,
            "Nobody says anything here.",
            "Lines arrive with the room's script, or are written in the script editor.",
        );
        return;
    }

    ui.heading(format!("{total} lines"));
    if voiced == total {
        ui.label(egui::RichText::new("All recorded.").small().weak());
    } else {
        ui.label(
            egui::RichText::new(format!("{} without a recording.", total - voiced))
                .small()
                .weak(),
        );
    }

    egui::ScrollArea::vertical()
        .id_salt("line_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (id, text, has_voice) in state.lines {
                ui.horizontal_top(|ui| {
                    // A line with no recording gets a disabled button
                    // rather than none, so the column stays aligned and
                    // the gap is what you notice.
                    if ui
                        .add_enabled(*has_voice, egui::Button::new("Play"))
                        .on_hover_text(if *has_voice {
                            "Hear this line"
                        } else {
                            "No recording in the chosen voice language"
                        })
                        .clicked()
                    {
                        actions.play_line = Some(id.clone());
                    }
                    ui.label(egui::RichText::new(text).small())
                        .on_hover_text(id);
                });
            }
        });
}

fn picker(ui: &mut egui::Ui, salt: &str, languages: &[String], current: &mut String) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(current.clone())
        .width(60.0)
        .show_ui(ui, |ui| {
            for language in languages {
                ui.selectable_value(current, language.clone(), language);
            }
        });
}
