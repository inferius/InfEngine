//! A41.2 / A93: what this screen is, and what it wears.
//!
//! Identity and pictures. The layer list is here rather than under its
//! own heading because a room's layers *are* what the room looks like -
//! asking "what is this screen" and being shown its name but not its
//! backdrop would be a strange answer.
//!
//! Since A93 a layer can carry a condition: shown only while a flag
//! agrees. That is how one room wears its variants - the table with
//! the letter and the table without are two layers over one
//! background, not two backgrounds. Each layer's position is editable
//! in room pixels, because a cut-out overlay has to land exactly where
//! the picture it came from had it.

use infengine_adventure::room::LayerCondition;

use crate::lang::t;
use crate::ui::{UiActions, UiState};

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    egui::Grid::new("room_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("id");
            ui.text_edit_singleline(&mut state.room.id)
                .on_hover_text(t(
                    "What exits and save files call this room. Changing it breaks both.",
                    "Jak tuhle místnost jmenují východy a uložené pozice. Změna rozbije obojí.",
                ));
            ui.end_row();

            ui.label(t("name", "název"));
            ui.text_edit_singleline(&mut state.room.name)
                .on_hover_text(t(
                    "What the player sees, if the game shows a room title.",
                    "Co vidí hráč, pokud hra ukazuje název místnosti.",
                ));
            ui.end_row();

            ui.label(t("size", "rozměr"));
            ui.label(format!(
                "{} x {}",
                state.room.size.x as i32, state.room.size.y as i32
            ))
            .on_hover_text(t(
                "Taken from the background when the room was imported.",
                "Převzato z pozadí při importu místnosti.",
            ));
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.heading(t("Pictures", "Obrázky"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(t("+ Picture", "+ Obrázek"), |ui| {
                ui.label(
                    egui::RichText::new(t(
                        "Add one on top. Give it a condition to make it a variant.",
                        "Přidej navrch. S podmínkou z něj bude varianta.",
                    ))
                    .small()
                    .weak(),
                );
                egui::ScrollArea::vertical()
                    .id_salt("add_layer_pictures")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for image in state.pictures {
                            if ui.button(image).clicked() {
                                actions.add_layer = Some(image.clone());
                                ui.close();
                            }
                        }
                    });
            });
        });
    });

    if state.room.layers.is_empty() {
        super::nothing_yet(
            ui,
            t("Nothing is painted here.", "Není tu nic namalovaného."),
            t(
                "A room with no layers draws as empty black.",
                "Místnost bez vrstev se kreslí jako černá plocha.",
            ),
        );
        return;
    }

    ui.label(
        egui::RichText::new(t(
            "Painted back to front. Untick one to see behind it.",
            "Kreslí se zezadu dopředu. Odškrtnutím se podíváš za vrstvu.",
        ))
        .small()
        .weak(),
    );

    let missing = state.missing_images.to_vec();
    let flags: Vec<String> = state.known_flags.to_vec();
    let mut remove: Option<usize> = None;
    for (index, layer) in state.room.layers.iter_mut().enumerate() {
        let gone = missing.contains(&layer.image);
        let title = if layer.name.is_empty() {
            layer.image.clone()
        } else {
            layer.name.clone()
        };
        let header = match layer.shown_if.as_ref() {
            Some(condition) => format!(
                "{title}  ({}{})",
                if condition.value { "" } else { "!" },
                condition.flag
            ),
            None => title,
        };
        egui::CollapsingHeader::new(if gone {
            egui::RichText::new(header).color(egui::Color32::from_rgb(255, 140, 120))
        } else {
            egui::RichText::new(header)
        })
        .id_salt(("layer", index))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut layer.visible, t("show", "zobrazit"))
                    .on_hover_text(t(
                        "The author's own switch. The game never sees it.",
                        "Autorský vypínač. Hra ho nevidí.",
                    ));
                ui.label(egui::RichText::new(&layer.image).small().weak());
            });

            ui.horizontal(|ui| {
                ui.label(t("at", "na"));
                ui.add(egui::DragValue::new(&mut layer.offset.x).speed(1.0));
                ui.add(egui::DragValue::new(&mut layer.offset.y).speed(1.0));
                ui.label(
                    egui::RichText::new(t("room pixels", "pixelů místnosti"))
                        .small()
                        .weak(),
                );
            });

            condition(ui, layer, &flags);

            if ui
                .small_button(t("Remove picture", "Odebrat obrázek"))
                .clicked()
            {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        actions.remove_layer = Some(index);
    }
}

/// When this layer exists, in the game's terms.
fn condition(
    ui: &mut egui::Ui,
    layer: &mut infengine_adventure::room::Layer,
    flags: &[String],
) {
    let mut conditional = layer.shown_if.is_some();
    if ui
        .checkbox(&mut conditional, t("only with a flag", "jen s přepínačem"))
        .on_hover_text(t(
            "Two layers over one background, each shown in its own state of the room.",
            "Dvě vrstvy nad jedním pozadím, každá pro svůj stav místnosti.",
        ))
        .changed()
    {
        layer.shown_if = conditional.then(|| LayerCondition {
            flag: flags.first().cloned().unwrap_or_default(),
            value: true,
        });
    }
    let Some(condition) = layer.shown_if.as_mut() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(t("flag", "přepínač"));
        // Typed or picked: a flag a script sets but nothing has set yet
        // does not appear in the game's map, and refusing to name it
        // would make the condition impossible to write first.
        ui.add(
            egui::TextEdit::singleline(&mut condition.flag)
                .desired_width(130.0)
                .hint_text(t("flag name", "jméno přepínače")),
        );
        if !flags.is_empty() {
            egui::ComboBox::from_id_salt(("layer_flag", &condition.flag))
                .selected_text("…")
                .width(24.0)
                .show_ui(ui, |ui| {
                    for flag in flags {
                        if ui.selectable_label(false, flag).clicked() {
                            condition.flag = flag.clone();
                        }
                    }
                });
        }
    });
    ui.horizontal(|ui| {
        ui.label(t("shown when", "zobrazit když"));
        ui.selectable_value(&mut condition.value, true, t("set", "je zapnutý"));
        ui.selectable_value(&mut condition.value, false, t("not set", "není zapnutý"));
    });
}
