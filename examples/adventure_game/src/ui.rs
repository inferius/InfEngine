//! A36.5: what the player reads and presses.
//!
//! Subtitles, the status line under the cursor, and whatever screen is
//! stacked over the room. Nothing here is an editor panel - a shipped
//! game shows the game and nothing else.

use infengine_adventure::host::Host;
use infengine_adventure::screen::{Align, Screen, ScreenAction, WidgetKind};

/// What the player pressed this frame.
#[derive(Debug, Default)]
pub struct UiEvents {
    pub fired: Option<ScreenAction>,
}

/// Draw the game's own interface.
pub fn build(ctx: &egui::Context, host: &Host, status: &str) -> UiEvents {
    let mut events = UiEvents::default();
    if let Some(screen) = host.active_screen() {
        events.fired = draw_screen(ctx, screen, host);
        return events;
    }
    status_line(ctx, status);
    subtitle(ctx, host);
    events
}

/// The name of whatever is under the cursor, along the top.
fn status_line(ctx: &egui::Context, status: &str) {
    if status.is_empty() {
        return;
    }
    let area = ctx.content_rect();
    egui::Area::new("status".into())
        .fixed_pos(egui::pos2(area.center().x, area.min.y + 18.0))
        .pivot(egui::Align2::CENTER_CENTER)
        .interactable(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(status)
                    .size(18.0)
                    .color(egui::Color32::from_rgb(230, 225, 200)),
            );
        });
}

/// The line being spoken, across the bottom.
fn subtitle(ctx: &egui::Context, host: &Host) {
    let Some(subtitle) = host.subtitle.as_ref() else {
        return;
    };
    // A line with no text shows as its own id, which is useful in an
    // editor and is a file name leaking onto the screen in a game.
    if subtitle.is_placeholder() {
        return;
    }
    let area = ctx.content_rect();
    egui::Area::new("subtitle".into())
        .fixed_pos(egui::pos2(area.center().x, area.max.y - 80.0))
        .pivot(egui::Align2::CENTER_CENTER)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_black_alpha(190))
                .show(ui, |ui| {
                    ui.set_max_width(area.width() * 0.7);
                    if let Some(speaker) = &subtitle.speaker {
                        ui.label(
                            egui::RichText::new(speaker)
                                .small()
                                .color(egui::Color32::from_rgb(200, 190, 140)),
                        );
                    }
                    ui.label(
                        egui::RichText::new(&subtitle.text)
                            .size(20.0)
                            .color(egui::Color32::WHITE),
                    );
                });
        });
}

/// Draw a screen and return whatever was pressed.
fn draw_screen(ctx: &egui::Context, screen: &Screen, host: &Host) -> Option<ScreenAction> {
    let area = ctx.content_rect();
    let scale = (area.width() / screen.size.x.max(1.0))
        .min(area.height() / screen.size.y.max(1.0))
        .max(0.05);
    let drawn = egui::vec2(screen.size.x * scale, screen.size.y * scale);
    let origin = area.center() - drawn * 0.5;

    let mut fired = None;
    egui::Area::new("screen".into())
        .fixed_pos(origin)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(drawn, egui::Sense::hover());
            // A screen that is not an overlay hides the room behind it;
            // a pause menu lets it show through.
            ui.painter().rect_filled(
                rect,
                0.0,
                egui::Color32::from_black_alpha(if screen.overlay { 160 } else { 240 }),
            );

            for widget in &screen.widgets {
                if !widget.visible {
                    continue;
                }
                let lo = widget.min.min(widget.max);
                let hi = widget.min.max(widget.max);
                let bounds = egui::Rect::from_min_max(
                    rect.min + egui::vec2(lo.x * scale, lo.y * scale),
                    rect.min + egui::vec2(hi.x * scale, hi.y * scale),
                );
                if let Some(action) = widget_ui(ui, widget, bounds, scale, host) {
                    fired = Some(action);
                }
            }
        });
    fired
}

fn widget_ui(
    ui: &mut egui::Ui,
    widget: &infengine_adventure::screen::Widget,
    bounds: egui::Rect,
    scale: f32,
    host: &Host,
) -> Option<ScreenAction> {
    match &widget.kind {
        // The picture is the backdrop the room draws; a screen's own
        // art is not uploaded here yet, so its place is left empty
        // rather than filled with a placeholder the player would see.
        WidgetKind::Image { .. } => None,
        WidgetKind::Label { line, size, align } => {
            paint(ui, bounds, &text_of(host, line), *size * scale, *align, egui::Color32::WHITE);
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
                egui::Color32::from_rgb(255, 235, 160)
            } else {
                egui::Color32::from_rgb(225, 220, 205)
            };
            paint(ui, bounds, &text_of(host, line), *size * scale, *align, colour);
            response.clicked().then(|| action.clone())
        }
    }
}

/// A line in the player's language, or its id when there is no text.
///
/// Showing the id is deliberate: a menu with a hole in it should look
/// like one rather than like a button that does nothing.
fn text_of(host: &Host, line: &str) -> String {
    let text = host.project.locales.text(line, &host.language.text);
    if text.is_empty() {
        line.to_string()
    } else {
        text.to_string()
    }
}

fn paint(
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
        egui::FontId::proportional(size.max(8.0)),
        colour,
    );
}
