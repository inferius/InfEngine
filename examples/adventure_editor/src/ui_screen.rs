//! A24.2: editing the game's menus, and seeing them work.
//!
//! Two halves. The window is where a screen is built - widgets, their
//! rectangles, what a button does. The preview is the same screen drawn
//! for real, with its text pulled from the chosen language and its
//! buttons live, which is the only way to find out that the Czech word
//! for "Continue" does not fit the button someone sized in English.

use infengine_adventure::action::Condition;
use infengine_adventure::loca::{LanguageChoice, Locales};
use infengine_adventure::screen::{Align, Screen, ScreenAction, Widget, WidgetKind};
use glam::Vec2;

/// State the screen editor keeps between frames.
#[derive(Debug, Default)]
pub struct ScreenUi {
    pub open: bool,
    pub screens: Vec<Screen>,
    pub selected: Option<usize>,
    pub selected_widget: Option<usize>,
    /// Screen being previewed over the room, by index.
    pub preview: Option<usize>,
    pub new_id: String,
    pub status: String,
    pub dirty: bool,
}

impl ScreenUi {
    pub fn current(&self) -> Option<&Screen> {
        self.selected.and_then(|index| self.screens.get(index))
    }

    pub fn preview_screen(&self) -> Option<&Screen> {
        self.preview.and_then(|index| self.screens.get(index))
    }

    /// Show the screen with this id, if there is one.
    pub fn show(&mut self, id: &str) -> bool {
        match self.screens.iter().position(|screen| screen.id == id) {
            Some(index) => {
                self.preview = Some(index);
                true
            }
            None => false,
        }
    }
}

#[derive(Debug, Default)]
pub struct ScreenActions {
    pub save: bool,
    pub reload: bool,
    /// Id for a screen to create.
    pub create: Option<String>,
    /// An action a previewed button fired.
    pub fired: Option<ScreenAction>,
}

/// The screen editor window.
pub fn window(
    ctx: &egui::Context,
    ui_state: &mut ScreenUi,
    languages: &[String],
) -> ScreenActions {
    let mut actions = ScreenActions::default();
    let mut open = ui_state.open;

    egui::Window::new("Screens")
        .open(&mut open)
        .default_width(460.0)
        .show(ctx, |ui| {
            list_row(ui, ui_state, &mut actions);
            ui.separator();

            let Some(index) = ui_state.selected else {
                ui.label(egui::RichText::new("Pick a screen, or make one.").weak());
                return;
            };
            let Some(screen) = ui_state.screens.get_mut(index) else {
                return;
            };

            ui.horizontal(|ui| {
                ui.label("size");
                ui_state.dirty |= ui
                    .add(egui::DragValue::new(&mut screen.size.x).range(1.0..=4096.0))
                    .changed();
                ui.label("x");
                ui_state.dirty |= ui
                    .add(egui::DragValue::new(&mut screen.size.y).range(1.0..=4096.0))
                    .changed();
                ui_state.dirty |= ui
                    .checkbox(&mut screen.overlay, "overlay")
                    .on_hover_text("Keep drawing what is behind it - a pause menu, not a title")
                    .changed();
            });

            widget_list(ui, screen, &mut ui_state.selected_widget, &mut ui_state.dirty);
            if let Some(widget_index) = ui_state.selected_widget
                && let Some(widget) = screen.widgets.get_mut(widget_index)
            {
                ui.separator();
                widget_editor(ui, widget, languages, &mut ui_state.dirty);
            }

            problems(ui, screen);

            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(ui_state.dirty, egui::Button::new("save"))
                    .clicked()
                {
                    actions.save = true;
                }
                if ui.button("reload").clicked() {
                    actions.reload = true;
                }
                let previewing = ui_state.preview == Some(index);
                if ui
                    .selectable_label(previewing, "preview")
                    .on_hover_text("Draw it over the room, with live buttons")
                    .clicked()
                {
                    ui_state.preview = (!previewing).then_some(index);
                }
            });
            if !ui_state.status.is_empty() {
                ui.label(egui::RichText::new(&ui_state.status).small().weak());
            }
        });

    ui_state.open = open;
    actions
}

fn list_row(ui: &mut egui::Ui, ui_state: &mut ScreenUi, actions: &mut ScreenActions) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("screen_pick")
            .selected_text(
                ui_state
                    .current()
                    .map(|screen| screen.id.clone())
                    .unwrap_or_else(|| "-".into()),
            )
            .show_ui(ui, |ui| {
                for (index, screen) in ui_state.screens.iter().enumerate() {
                    ui.selectable_value(&mut ui_state.selected, Some(index), &screen.id);
                }
            });
        ui.add(
            egui::TextEdit::singleline(&mut ui_state.new_id)
                .hint_text("new screen id")
                .desired_width(120.0),
        );
        if ui.button("+").clicked() && !ui_state.new_id.trim().is_empty() {
            actions.create = Some(ui_state.new_id.trim().to_string());
            ui_state.new_id.clear();
        }
    });
}

fn widget_list(
    ui: &mut egui::Ui,
    screen: &mut Screen,
    selected: &mut Option<usize>,
    dirty: &mut bool,
) {
    ui.heading(format!("Widgets ({})", screen.widgets.len()));
    egui::ScrollArea::vertical()
        .id_salt("widget_list")
        .max_height(140.0)
        .show(ui, |ui| {
            for (index, widget) in screen.widgets.iter().enumerate() {
                let label = format!(
                    "{}  {}",
                    widget.id,
                    match &widget.kind {
                        WidgetKind::Image { .. } => "image".to_string(),
                        WidgetKind::Label { .. } => "label".to_string(),
                        WidgetKind::Button { action, .. } => action.label(),
                    }
                );
                if ui.selectable_label(*selected == Some(index), label).clicked() {
                    *selected = Some(index);
                }
            }
        });

    ui.horizontal(|ui| {
        for (name, kind) in [
            ("+ image", WidgetKind::Image { image: String::new() }),
            (
                "+ label",
                WidgetKind::Label {
                    line: String::new(),
                    size: 16.0,
                    align: Align::Center,
                },
            ),
            (
                "+ button",
                WidgetKind::Button {
                    line: String::new(),
                    size: 16.0,
                    align: Align::Center,
                    action: ScreenAction::None,
                },
            ),
        ] {
            if ui.button(name).clicked() {
                let id = format!("widget{}", screen.widgets.len());
                // Placed in the middle at a size you can see and grab,
                // rather than at the origin where it hides under the
                // background.
                let middle = screen.size * 0.5;
                screen.widgets.push(Widget {
                    id,
                    min: middle - Vec2::new(60.0, 12.0),
                    max: middle + Vec2::new(60.0, 12.0),
                    kind,
                    visible: true,
            shown_if: None,
                });
                *selected = Some(screen.widgets.len() - 1);
                *dirty = true;
            }
        }
        if ui
            .add_enabled(selected.is_some(), egui::Button::new("remove"))
            .clicked()
            && let Some(index) = *selected
            && index < screen.widgets.len()
        {
            screen.widgets.remove(index);
            *selected = None;
            *dirty = true;
        }
    });
}

fn widget_editor(
    ui: &mut egui::Ui,
    widget: &mut Widget,
    languages: &[String],
    dirty: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label("id");
        *dirty |= ui.text_edit_singleline(&mut widget.id).changed();
        *dirty |= ui.checkbox(&mut widget.visible, "visible").changed();
    });
    // The condition: how the interface follows the story. A widget can
    // wait for a flag - a spell button appears when the scene teaching
    // the spell sets it.
    ui.horizontal(|ui| {
        let mut gated = widget.shown_if.is_some();
        if ui
            .checkbox(&mut gated, "only after a flag")
            .on_hover_text("Shown once the named flag is set; scripts set flags with a Set flag step")
            .changed()
        {
            widget.shown_if = gated.then(|| Condition::Flag(String::new()));
            *dirty = true;
        }
        match widget.shown_if.as_mut() {
            Some(Condition::Flag(flag)) => {
                *dirty |= ui.text_edit_singleline(flag).changed();
            }
            Some(Condition::Unresolved { call, .. }) => {
                ui.label(
                    egui::RichText::new(format!("{call}? (from an import, not runnable)"))
                        .small()
                        .color(egui::Color32::from_rgb(255, 180, 100)),
                );
            }
            Some(other) => {
                // Screens gate on flags; anything else reads but is
                // authored elsewhere.
                ui.label(egui::RichText::new(other.label()).small().weak());
            }
            None => {}
        }
    });
    ui.horizontal(|ui| {
        ui.label("from");
        *dirty |= ui.add(egui::DragValue::new(&mut widget.min.x)).changed();
        *dirty |= ui.add(egui::DragValue::new(&mut widget.min.y)).changed();
        ui.label("to");
        *dirty |= ui.add(egui::DragValue::new(&mut widget.max.x)).changed();
        *dirty |= ui.add(egui::DragValue::new(&mut widget.max.y)).changed();
    });

    match &mut widget.kind {
        WidgetKind::Image { image } => {
            ui.horizontal(|ui| {
                ui.label("image");
                *dirty |= ui.text_edit_singleline(image).changed();
            });
        }
        WidgetKind::Label { line, size, align } => {
            text_fields(ui, line, size, align, dirty);
        }
        WidgetKind::Button {
            line,
            size,
            align,
            action,
        } => {
            text_fields(ui, line, size, align, dirty);
            action_editor(ui, action, languages, dirty);
        }
    }
}

fn text_fields(
    ui: &mut egui::Ui,
    line: &mut String,
    size: &mut f32,
    align: &mut Align,
    dirty: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label("line id");
        *dirty |= ui
            .add(egui::TextEdit::singleline(line).hint_text("menu.new_game"))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("size");
        *dirty |= ui
            .add(egui::DragValue::new(size).range(6.0..=96.0))
            .changed();
        for option in [Align::Left, Align::Center, Align::Right] {
            *dirty |= ui
                .selectable_value(align, option, format!("{option:?}"))
                .changed();
        }
    });
}

fn action_editor(
    ui: &mut egui::Ui,
    action: &mut ScreenAction,
    languages: &[String],
    dirty: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label("does");
        egui::ComboBox::from_id_salt("widget_action")
            .selected_text(action.label())
            .show_ui(ui, |ui| {
                for option in [
                    ScreenAction::NewGame,
                    ScreenAction::Resume,
                    ScreenAction::Close,
                    ScreenAction::Quit,
                    ScreenAction::Open {
                        screen: String::new(),
                    },
                    ScreenAction::SaveGame { slot: 0 },
                    ScreenAction::LoadGame { slot: 0 },
                    ScreenAction::RunScript {
                        script: String::new(),
                    },
                    ScreenAction::None,
                ] {
                    let label = option.label();
                    *dirty |= ui.selectable_value(action, option, label).changed();
                }
                for language in languages {
                    let text = ScreenAction::SetTextLanguage {
                        language: language.clone(),
                    };
                    let label = text.label();
                    *dirty |= ui.selectable_value(action, text, label).changed();
                    let voice = ScreenAction::SetVoiceLanguage {
                        language: language.clone(),
                    };
                    let label = voice.label();
                    *dirty |= ui.selectable_value(action, voice, label).changed();
                }
            });
    });

    // The variants that carry a name need somewhere to type it.
    match action {
        ScreenAction::Open { screen } => {
            ui.horizontal(|ui| {
                ui.label("screen");
                *dirty |= ui.text_edit_singleline(screen).changed();
            });
        }
        ScreenAction::RunScript { script } => {
            ui.horizontal(|ui| {
                ui.label("script");
                *dirty |= ui.text_edit_singleline(script).changed();
            });
        }
        ScreenAction::SaveGame { slot } | ScreenAction::LoadGame { slot } => {
            ui.horizontal(|ui| {
                ui.label("slot");
                *dirty |= ui.add(egui::DragValue::new(slot).range(0..=99)).changed();
            });
        }
        _ => {}
    }
}

fn problems(ui: &mut egui::Ui, screen: &Screen) {
    let problems = screen.problems();
    if problems.is_empty() {
        return;
    }
    ui.separator();
    for problem in problems {
        ui.label(
            egui::RichText::new(problem)
                .small()
                .color(egui::Color32::from_rgb(230, 150, 60)),
        );
    }
}

/// Draw a screen for real, with live buttons.
///
/// Returns whatever was pressed. Text comes from the chosen language,
/// and an id with no translation shows as the id: a menu with a hole in
/// it should look like one.
pub fn preview(
    ctx: &egui::Context,
    screen: &Screen,
    locales: &Locales,
    language: &LanguageChoice,
    flags: &std::collections::BTreeMap<String, bool>,
) -> Option<ScreenAction> {
    let area = ctx.content_rect();
    // Fit the design size into the free area, keeping its proportions -
    // a menu authored for 320x200 has to stay that shape or the layout
    // being checked is not the one that ships.
    let scale = (area.width() / screen.size.x.max(1.0))
        .min(area.height() / screen.size.y.max(1.0))
        .max(0.05);
    let drawn = egui::vec2(screen.size.x * scale, screen.size.y * scale);
    let origin = area.center() - drawn * 0.5;

    let mut fired = None;
    egui::Area::new("screen_preview".into())
        .fixed_pos(origin)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(drawn, egui::Sense::hover());
            ui.painter().rect_filled(
                rect,
                0.0,
                egui::Color32::from_black_alpha(if screen.overlay { 150 } else { 245 }),
            );

            for widget in &screen.widgets {
                if !widget.visible {
                    continue;
                }
                // Unlike the game, the preview draws what a condition
                // hides - ghosted, tagged with the flag it waits for -
                // because an author needs to see what exists, not what
                // one particular save would show.
                let ghost =
                    !widget.shown(&|flag| flags.get(flag).copied().unwrap_or(false));
                let lo = widget.min.min(widget.max);
                let hi = widget.min.max(widget.max);
                let bounds = egui::Rect::from_min_max(
                    rect.min + egui::vec2(lo.x * scale, lo.y * scale),
                    rect.min + egui::vec2(hi.x * scale, hi.y * scale),
                );
                let action =
                    draw_widget(ui, widget, bounds, scale, locales, language, ghost);
                if let Some(action) = action
                    && !ghost
                {
                    fired = Some(action);
                }
                if ghost && let Some(Condition::Flag(flag)) = &widget.shown_if {
                    ui.painter().text(
                        bounds.right_top() + egui::vec2(-2.0, 1.0),
                        egui::Align2::RIGHT_TOP,
                        format!("⚑ {flag}"),
                        egui::FontId::proportional(9.0),
                        egui::Color32::from_rgb(180, 160, 100),
                    );
                }
            }
        });
    fired
}

fn draw_widget(
    ui: &mut egui::Ui,
    widget: &Widget,
    bounds: egui::Rect,
    scale: f32,
    locales: &Locales,
    language: &LanguageChoice,
    ghost: bool,
) -> Option<ScreenAction> {
    let ui = &mut ui.new_child(
        egui::UiBuilder::new()
            .max_rect(ui.max_rect())
            .layout(*ui.layout()),
    );
    if ghost {
        ui.set_opacity(0.35);
    }
    match &widget.kind {
        WidgetKind::Image { image } => {
            // The preview does not upload art; it shows where it goes.
            ui.painter()
                .rect_stroke(bounds, 0.0, egui::Stroke::new(1.0, egui::Color32::DARK_GRAY), egui::StrokeKind::Inside);
            ui.painter().text(
                bounds.min + egui::vec2(4.0, 2.0),
                egui::Align2::LEFT_TOP,
                image,
                egui::FontId::proportional(11.0),
                egui::Color32::DARK_GRAY,
            );
            None
        }
        WidgetKind::Label { line, size, align } => {
            paint_text(ui, bounds, &text_of(locales, line, language), *size * scale, *align, egui::Color32::WHITE);
            None
        }
        WidgetKind::Button {
            line,
            size,
            align,
            action,
        } => {
            let response = ui.interact(
                bounds,
                egui::Id::new(("screen_widget", &widget.id)),
                egui::Sense::click(),
            );
            let colour = if response.hovered() {
                egui::Color32::from_rgb(255, 230, 150)
            } else {
                egui::Color32::WHITE
            };
            paint_text(ui, bounds, &text_of(locales, line, language), *size * scale, *align, colour);
            response.clicked().then(|| action.clone())
        }
    }
}

fn text_of(locales: &Locales, line: &str, language: &LanguageChoice) -> String {
    let text = locales.text(line, &language.text);
    if text.is_empty() {
        line.to_string()
    } else {
        text.to_string()
    }
}

fn paint_text(
    ui: &mut egui::Ui,
    bounds: egui::Rect,
    text: &str,
    size: f32,
    align: Align,
    colour: egui::Color32,
) {
    let (anchor, at) = match align {
        Align::Left => (egui::Align2::LEFT_CENTER, bounds.left_center()),
        Align::Center => (egui::Align2::CENTER_CENTER, bounds.center()),
        Align::Right => (egui::Align2::RIGHT_CENTER, bounds.right_center()),
    };
    ui.painter().text(
        at,
        anchor,
        text,
        egui::FontId::proportional(size.max(6.0)),
        colour,
    );
}
