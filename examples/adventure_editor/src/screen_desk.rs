//! A97: a screen laid out by hand, on the workbench.
//!
//! The screen editor was a floating window of numbers: to move a
//! button by four pixels you found its widget in a list and typed into
//! four drag values, and the preview drew images as an empty outline
//! with a path in it. Composing an interface that way is possible in
//! the sense that arithmetic is possible.
//!
//! So a screen opens the way an animation does - it takes over the
//! centre - and the canvas is the interface at its design size, with
//! the pictures actually drawn. Widgets are dragged where they go and
//! resized by their corner; a snap keeps them on whole pixels or on a
//! grid; the selected widget's own fields sit beside the canvas, and
//! the alignment buttons put a row of them in a line without anyone
//! doing sums.

use glam::Vec2;
use infengine_adventure::loca::{LanguageChoice, Locales};
use infengine_adventure::screen::{Align, Screen, ScreenAction, Widget, WidgetKind};

use crate::lang::t;

/// Handle size for the resize corner, in points.
const GRIP: f32 = 9.0;

/// The screen on the workbench.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenDesk {
    pub screen: Screen,
    /// What it was called when opened, so a rename replaces the right
    /// file rather than leaving both.
    pub original_id: String,
    pub selected: Option<usize>,
    /// What a drag lands on, in screen pixels. 1 is pixel by pixel.
    pub snap: i32,
    pub dirty: bool,
}

impl ScreenDesk {
    pub fn open(screen: Screen) -> Self {
        Self {
            original_id: screen.id.clone(),
            screen,
            selected: None,
            snap: 1,
            dirty: false,
        }
    }
}

/// What the workspace asked the app to do.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScreenDeskActions {
    pub save: bool,
    pub close: bool,
}

/// Everything the canvas needs that lives outside the screen.
pub struct Context<'a> {
    pub locales: &'a Locales,
    pub language: &'a LanguageChoice,
    pub pictures: &'a [String],
    pub screens: &'a [String],
    pub scripts: &'a [String],
    pub languages: &'a [String],
    /// Loads a picture, so the canvas can draw what the game will draw.
    pub texture: &'a dyn Fn(&str) -> Option<egui::TextureHandle>,
}

/// Draw the workspace. Returns what it asked for.
pub fn panel(
    ctx: &egui::Context,
    desk: &mut ScreenDesk,
    context: &Context<'_>,
) -> ScreenDeskActions {
    let mut actions = ScreenDeskActions::default();
    egui::CentralPanel::default().show(ctx, |ui| {
        header(ui, desk, &mut actions);
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 300.0).max(200.0));
                canvas(ui, desk, context);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(284.0);
                egui::ScrollArea::vertical()
                    .id_salt("screen_desk_fields")
                    .auto_shrink([false, false])
                    .show(ui, |ui| fields(ui, desk, context));
            });
        });
    });
    actions
}

fn header(ui: &mut egui::Ui, desk: &mut ScreenDesk, actions: &mut ScreenDeskActions) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t("SCREEN", "OBRAZOVKA")).small().weak());
        ui.heading(&desk.screen.id);
        ui.label(
            egui::RichText::new(format!(
                "{} × {}",
                desk.screen.size.x as i32, desk.screen.size.y as i32
            ))
            .small()
            .weak(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("Close", "Zavřít")).clicked() {
                actions.close = true;
            }
            if ui
                .add_enabled(desk.dirty, egui::Button::new(t("Save", "Uložit")))
                .on_disabled_hover_text(t("Nothing has changed", "Nic se nezměnilo"))
                .clicked()
            {
                actions.save = true;
            }
            if desk.dirty {
                ui.label(
                    egui::RichText::new(t("● unsaved", "● neuloženo"))
                        .small()
                        .color(egui::Color32::from_rgb(255, 200, 120)),
                );
            }
        });
    });
}

/// The interface at its design size, dragged into shape.
fn canvas(ui: &mut egui::Ui, desk: &mut ScreenDesk, context: &Context<'_>) {
    let available = ui.available_size();
    let size = desk.screen.size.max(Vec2::ONE);
    // Whole-number scaling while it still fits: a menu drawn for 320
    // pixels reads wrong at 1.37 times its size, and half the point of
    // this canvas is judging the lettering.
    let raw = (available.x / size.x).min(available.y / size.y).max(0.1);
    let scale = if raw >= 1.0 { raw.floor() } else { raw };
    let drawn = egui::vec2(size.x * scale, size.y * scale);
    let (rect, _) = ui.allocate_exact_size(drawn, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        0.0,
        egui::Color32::from_black_alpha(if desk.screen.overlay { 170 } else { 250 }),
    );

    let to_screen = |p: Vec2| rect.min + egui::vec2(p.x * scale, p.y * scale);
    let snap = desk.snap.max(1) as f32;
    let round = |v: f32| (v / snap).round() * snap;

    let mut clicked: Option<usize> = None;
    let mut moved = false;
    let selected = desk.selected;
    for (index, widget) in desk.screen.widgets.iter_mut().enumerate() {
        let lo = widget.min.min(widget.max);
        let hi = widget.min.max(widget.max);
        let bounds = egui::Rect::from_min_max(to_screen(lo), to_screen(hi));
        let is_selected = selected == Some(index);

        // What the game will draw, drawn.
        match &widget.kind {
            WidgetKind::Image { image } => match (context.texture)(image) {
                Some(texture) => {
                    egui::Image::new((texture.id(), bounds.size())).paint_at(ui, bounds);
                }
                None => {
                    painter.rect_filled(bounds, 0.0, egui::Color32::from_gray(40));
                    painter.text(
                        bounds.center(),
                        egui::Align2::CENTER_CENTER,
                        if image.is_empty() { "(no picture)" } else { image },
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_gray(150),
                    );
                }
            },
            WidgetKind::Label { line, size, align }
            | WidgetKind::Button { line, size, align, .. } => {
                let text = {
                    let text = context.locales.text(line, &context.language.text);
                    if text.is_empty() { line.clone() } else { text.to_string() }
                };
                let (anchor, at) = match align {
                    Align::Left => (egui::Align2::LEFT_CENTER, bounds.left_center()),
                    Align::Center => (egui::Align2::CENTER_CENTER, bounds.center()),
                    Align::Right => (egui::Align2::RIGHT_CENTER, bounds.right_center()),
                };
                painter.text(
                    at,
                    anchor,
                    text,
                    egui::FontId::proportional((size * scale).max(6.0)),
                    if matches!(widget.kind, WidgetKind::Button { .. }) {
                        egui::Color32::from_rgb(255, 235, 170)
                    } else {
                        egui::Color32::WHITE
                    },
                );
            }
        }

        // The body drags, the corner resizes. Both in screen pixels,
        // both landing on the snap.
        let body = ui.interact(
            bounds,
            egui::Id::new(("screen_widget_body", &widget.id, index)),
            egui::Sense::click_and_drag(),
        );
        if body.clicked() || body.drag_started() {
            clicked = Some(index);
        }
        if body.dragged() {
            let delta = body.drag_delta() / scale;
            let by = Vec2::new(delta.x, delta.y);
            widget.min += by;
            widget.max += by;
            widget.min = Vec2::new(round(widget.min.x), round(widget.min.y));
            widget.max = Vec2::new(round(widget.max.x), round(widget.max.y));
            moved = true;
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }

        let grip = egui::Rect::from_min_size(
            bounds.max - egui::vec2(GRIP, GRIP),
            egui::vec2(GRIP, GRIP),
        );
        let corner = ui.interact(
            grip,
            egui::Id::new(("screen_widget_grip", &widget.id, index)),
            egui::Sense::drag(),
        );
        if corner.dragged() {
            let delta = corner.drag_delta() / scale;
            let hi = widget.min.max(widget.max) + Vec2::new(delta.x, delta.y);
            let lo = widget.min.min(widget.max);
            widget.min = lo;
            widget.max = Vec2::new(
                round(hi.x).max(lo.x + 2.0),
                round(hi.y).max(lo.y + 2.0),
            );
            moved = true;
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }

        let outline = if is_selected {
            egui::Color32::from_rgb(255, 235, 140)
        } else if body.hovered() {
            egui::Color32::from_white_alpha(140)
        } else {
            egui::Color32::from_white_alpha(50)
        };
        painter.rect_stroke(
            bounds,
            0.0,
            egui::Stroke::new(if is_selected { 2.0 } else { 1.0 }, outline),
            egui::StrokeKind::Inside,
        );
        if is_selected {
            painter.rect_filled(grip, 1.0, egui::Color32::from_rgb(255, 235, 140));
        }
    }

    if let Some(index) = clicked {
        desk.selected = Some(index);
    }
    if moved {
        desk.dirty = true;
    }

    ui.label(
        egui::RichText::new(t(
            "Drag a widget to move it, its corner to resize. Click empty space to deselect.",
            "Prvek přesuneš tažením, roh mění velikost. Klikem do prázdna zrušíš výběr.",
        ))
        .small()
        .weak(),
    );
}

/// The widget list, the selected widget's fields, and the screen's own.
fn fields(ui: &mut egui::Ui, desk: &mut ScreenDesk, context: &Context<'_>) {
    ui.horizontal(|ui| {
        ui.label(t("size", "rozměr"));
        desk.dirty |= ui
            .add(egui::DragValue::new(&mut desk.screen.size.x).range(32.0..=4096.0))
            .changed();
        desk.dirty |= ui
            .add(egui::DragValue::new(&mut desk.screen.size.y).range(32.0..=4096.0))
            .changed();
        desk.dirty |= ui
            .checkbox(&mut desk.screen.overlay, t("over the game", "přes hru"))
            .on_hover_text(t(
                "A pause menu keeps the room behind it; a title screen does not.",
                "Menu pauzy nechá místnost za sebou vidět, titulní obrazovka ne.",
            ))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(t("snap", "krok"))
            .on_hover_text(t(
                "Dragged widgets land on multiples of this",
                "Tažené prvky dosednou na násobky tohoto čísla",
            ));
        ui.add(egui::DragValue::new(&mut desk.snap).range(1..=64));
    });

    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!("Prvky ({})", desk.screen.widgets.len())
            } else {
                format!("Widgets ({})", desk.screen.widgets.len())
            })
            .strong(),
        );
    });
    add_buttons(ui, desk, context);

    egui::ScrollArea::vertical()
        .id_salt("desk_widget_list")
        .max_height(120.0)
        .show(ui, |ui| {
            for (index, widget) in desk.screen.widgets.iter().enumerate() {
                let what = match &widget.kind {
                    WidgetKind::Image { .. } => t("picture", "obrázek").to_string(),
                    WidgetKind::Label { .. } => t("text", "text").to_string(),
                    WidgetKind::Button { action, .. } => action.label(),
                };
                if ui
                    .selectable_label(desk.selected == Some(index), format!("{}  {what}", widget.id))
                    .clicked()
                {
                    desk.selected = Some(index);
                }
            }
        });

    let Some(index) = desk.selected.filter(|i| *i < desk.screen.widgets.len()) else {
        ui.label(
            egui::RichText::new(t(
                "Nothing selected. Click a widget on the canvas.",
                "Nic není vybráno. Klikni na prvek na plátně.",
            ))
            .small()
            .weak(),
        );
        return;
    };

    ui.separator();
    align_buttons(ui, desk, index);
    let screen_size = desk.screen.size;
    let widget = &mut desk.screen.widgets[index];
    let mut dirty = false;
    widget_fields(ui, widget, screen_size, context, &mut dirty);
    ui.add_space(6.0);
    if ui.button(t("Remove widget", "Odebrat prvek")).clicked() {
        desk.screen.widgets.remove(index);
        desk.selected = None;
        dirty = true;
    }
    desk.dirty |= dirty;
}

/// Adding a widget: it lands in the middle at a size you can grab.
fn add_buttons(ui: &mut egui::Ui, desk: &mut ScreenDesk, context: &Context<'_>) {
    ui.horizontal_wrapped(|ui| {
        let entries = [
            (
                t("+ picture", "+ obrázek"),
                WidgetKind::Image {
                    image: context.pictures.first().cloned().unwrap_or_default(),
                },
            ),
            (
                t("+ text", "+ text"),
                WidgetKind::Label {
                    line: String::new(),
                    size: 16.0,
                    align: Align::Center,
                },
            ),
            (
                t("+ button", "+ tlačítko"),
                WidgetKind::Button {
                    line: String::new(),
                    size: 16.0,
                    align: Align::Center,
                    action: ScreenAction::None,
                },
            ),
        ];
        for (label, kind) in entries {
            if ui.button(label).clicked() {
                let middle = desk.screen.size * 0.5;
                let half = match kind {
                    WidgetKind::Image { .. } => Vec2::new(40.0, 30.0),
                    _ => Vec2::new(60.0, 12.0),
                };
                desk.screen.widgets.push(Widget {
                    id: format!("widget{}", desk.screen.widgets.len()),
                    min: middle - half,
                    max: middle + half,
                    kind,
                    visible: true,
                    shown_if: None,
                });
                desk.selected = Some(desk.screen.widgets.len() - 1);
                desk.dirty = true;
            }
        }
    });
}

/// Putting a widget where a designer would put it, without sums.
fn align_buttons(ui: &mut egui::Ui, desk: &mut ScreenDesk, index: usize) {
    let size = desk.screen.size;
    let widget = &mut desk.screen.widgets[index];
    let lo = widget.min.min(widget.max);
    let hi = widget.min.max(widget.max);
    let span = hi - lo;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t("align", "zarovnat")).small().weak());
        let mut place = |x: Option<f32>, y: Option<f32>| {
            let mut at = lo;
            if let Some(x) = x {
                at.x = x;
            }
            if let Some(y) = y {
                at.y = y;
            }
            widget.min = at;
            widget.max = at + span;
        };
        if ui.small_button("⇤").on_hover_text(t("Left edge", "K levému okraji")).clicked() {
            place(Some(0.0), None);
            desk.dirty = true;
        }
        if ui.small_button("↔").on_hover_text(t("Centred across", "Na střed vodorovně")).clicked() {
            place(Some((size.x - span.x) * 0.5), None);
            desk.dirty = true;
        }
        if ui.small_button("⇥").on_hover_text(t("Right edge", "K pravému okraji")).clicked() {
            place(Some(size.x - span.x), None);
            desk.dirty = true;
        }
        if ui.small_button("↕").on_hover_text(t("Centred down", "Na střed svisle")).clicked() {
            place(None, Some((size.y - span.y) * 0.5));
            desk.dirty = true;
        }
    });
}

/// One widget's own fields.
fn widget_fields(
    ui: &mut egui::Ui,
    widget: &mut Widget,
    screen_size: Vec2,
    context: &Context<'_>,
    dirty: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label("id");
        *dirty |= ui.text_edit_singleline(&mut widget.id).changed();
        *dirty |= ui.checkbox(&mut widget.visible, t("shown", "vidět")).changed();
    });
    ui.horizontal(|ui| {
        ui.label(t("from", "od"));
        *dirty |= ui.add(egui::DragValue::new(&mut widget.min.x)).changed();
        *dirty |= ui.add(egui::DragValue::new(&mut widget.min.y)).changed();
        ui.label(t("to", "do"));
        *dirty |= ui.add(egui::DragValue::new(&mut widget.max.x)).changed();
        *dirty |= ui.add(egui::DragValue::new(&mut widget.max.y)).changed();
    });
    let _ = screen_size;

    match &mut widget.kind {
        WidgetKind::Image { image } => {
            ui.horizontal(|ui| {
                ui.label(t("picture", "obrázek"));
                egui::ComboBox::from_id_salt(("desk_image", &widget.id))
                    .selected_text(if image.is_empty() { "-" } else { image.as_str() })
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for name in context.pictures {
                            if ui.selectable_label(image == name, name).clicked() {
                                *image = name.clone();
                                *dirty = true;
                            }
                        }
                    });
            });
        }
        WidgetKind::Label { line, size, align } => {
            text_fields(ui, line, size, align, dirty);
        }
        WidgetKind::Button { line, size, align, action } => {
            text_fields(ui, line, size, align, dirty);
            action_fields(ui, action, context, dirty);
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
        ui.label(t("line id", "id textu"));
        *dirty |= ui
            .add(egui::TextEdit::singleline(line).hint_text("menu.new_game"))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(t("size", "velikost"));
        *dirty |= ui.add(egui::DragValue::new(size).range(6.0..=96.0)).changed();
        for option in [Align::Left, Align::Center, Align::Right] {
            *dirty |= ui
                .selectable_value(align, option, format!("{option:?}"))
                .changed();
        }
    });
}

/// What pressing the button does, picked from what the project has.
fn action_fields(
    ui: &mut egui::Ui,
    action: &mut ScreenAction,
    context: &Context<'_>,
    dirty: &mut bool,
) {
    let label = action.label();
    ui.horizontal(|ui| {
        ui.label(t("does", "dělá"));
        egui::ComboBox::from_id_salt("desk_action")
            .selected_text(label)
            .width(170.0)
            .show_ui(ui, |ui| {
                let options = [
                    ("new game", ScreenAction::NewGame),
                    ("resume", ScreenAction::Resume),
                    ("close", ScreenAction::Close),
                    ("quit", ScreenAction::Quit),
                    (
                        "open a screen",
                        ScreenAction::Open {
                            screen: context.screens.first().cloned().unwrap_or_default(),
                        },
                    ),
                    (
                        "run a script",
                        ScreenAction::RunScript {
                            script: context.scripts.first().cloned().unwrap_or_default(),
                        },
                    ),
                    (
                        "set the text language",
                        ScreenAction::SetTextLanguage {
                            language: context.languages.first().cloned().unwrap_or_default(),
                        },
                    ),
                    ("nothing", ScreenAction::None),
                ];
                for (name, option) in options {
                    if ui.selectable_label(false, name).clicked() {
                        *action = option;
                        *dirty = true;
                    }
                }
            });
    });
    // The chosen action's own argument, as a picker of what exists.
    match action {
        ScreenAction::Open { screen } => {
            pick(ui, t("screen", "obrazovka"), screen, context.screens, dirty);
        }
        ScreenAction::RunScript { script } => {
            pick(ui, t("script", "skript"), script, context.scripts, dirty);
        }
        ScreenAction::SetTextLanguage { language }
        | ScreenAction::SetVoiceLanguage { language } => {
            pick(ui, t("language", "jazyk"), language, context.languages, dirty);
        }
        ScreenAction::SaveGame { slot } | ScreenAction::LoadGame { slot } => {
            ui.horizontal(|ui| {
                ui.label(t("slot", "pozice"));
                *dirty |= ui.add(egui::DragValue::new(slot).range(0..=99)).changed();
            });
        }
        _ => {}
    }
}

fn pick(ui: &mut egui::Ui, label: &str, value: &mut String, options: &[String], dirty: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(label);
        if options.is_empty() {
            *dirty |= ui.text_edit_singleline(value).changed();
            return;
        }
        egui::ComboBox::from_id_salt(("desk_pick", label))
            .selected_text(if value.is_empty() { "-" } else { value.as_str() })
            .width(160.0)
            .show_ui(ui, |ui| {
                for option in options {
                    if ui.selectable_label(&*value == option, option).clicked() {
                        *value = option.clone();
                        *dirty = true;
                    }
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_remembers_the_id_so_a_rename_replaces_the_file() {
        let mut desk = ScreenDesk::open(Screen::new("title", Vec2::new(320.0, 200.0)));
        desk.screen.id = "main_menu".into();
        assert_eq!(desk.original_id, "title");
    }

    #[test]
    fn a_fresh_desk_has_nothing_selected_and_nothing_to_save() {
        let desk = ScreenDesk::open(Screen::new("title", Vec2::new(320.0, 200.0)));
        assert!(desk.selected.is_none());
        assert!(!desk.dirty);
        assert_eq!(desk.snap, 1);
    }
}
