//! A36.5: what the player reads and presses.
//!
//! Subtitles, the status line under the cursor, and whatever screen is
//! stacked over the room. Nothing here is an editor panel - a shipped
//! game shows the game and nothing else.

use std::collections::{HashMap, HashSet};

use infengine_adventure::host::Host;
use infengine_adventure::screen::{Align, Screen, ScreenAction, WidgetKind};

/// How long a widget that just appeared pops for, in seconds.
const POP: f32 = 0.45;

/// What the player pressed this frame.
#[derive(Debug, Default)]
pub struct UiEvents {
    pub fired: Option<ScreenAction>,
}

/// The interface's own state between frames.
///
/// Screens are data; this is what drawing them accumulates: their art
/// as textures, and the little clocks behind appearance animations.
/// When a widget's condition comes true mid-game - a spell learned, a
/// chapter turned - the widget pops in rather than blinking into
/// place, which is what tells the player something new arrived.
#[derive(Default)]
pub struct ScreenFx {
    /// Which screen the bookkeeping below is about.
    screen: String,
    /// Widgets shown last frame, to notice appearances.
    seen: HashSet<String>,
    /// Seconds since each recent appearance, while under [`POP`].
    pops: HashMap<String, f32>,
    /// Screen art, loaded the first time a widget wants it. `None`
    /// records a miss so a broken path is not retried every frame.
    art: HashMap<String, Option<egui::TextureHandle>>,
}

/// Draw the game's own interface.
pub fn build(ctx: &egui::Context, host: &Host, status: &str, fx: &mut ScreenFx) -> UiEvents {
    let mut events = UiEvents::default();
    if let Some(screen) = host.active_screen() {
        events.fired = draw_screen(ctx, screen, host, fx);
        return events;
    }
    fx.screen.clear();
    fx.seen.clear();
    fx.pops.clear();
    status_line(ctx, status);
    subtitle(ctx, host, fx);
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

/// The game's bitmap font, when the manifest names one it has.
fn pix_font(host: &Host) -> Option<&infengine_adventure::pixfont::PixFont> {
    let wanted = host
        .project
        .manifest
        .as_ref()
        .and_then(|m| m.font.as_deref())?;
    host.project.fonts.iter().find(|f| f.id == wanted)
}

/// Paint text with a bitmap font, centred on `centre`.
fn pix_text(
    ctx: &egui::Context,
    fx: &mut ScreenFx,
    host: &Host,
    font: &infengine_adventure::pixfont::PixFont,
    centre: egui::Pos2,
    text: &str,
    tint: egui::Color32,
) {
    let Some(texture) = art_for(ctx, host, &mut fx.art, &font.image).clone() else {
        return;
    };
    let atlas = texture.size_vec2();
    // Whole multiples of the pixel keep the lettering crisp; the game
    // was drawn for a screen a third this wide.
    let area = ctx.content_rect();
    let scale = (area.width() / 480.0).clamp(1.0, 4.0).floor();
    let total = font.measure(text) * scale;
    let origin = centre - egui::vec2(total.x * 0.5, total.y * 0.5);

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("pix_subtitle"),
    ));
    for placed in font.layout(text) {
        let size = (placed.atlas_max - placed.atlas_min) * scale;
        let at = origin + egui::vec2(placed.offset.x, placed.offset.y) * scale;
        let rect = egui::Rect::from_min_size(at, egui::vec2(size.x, size.y));
        let uv = egui::Rect::from_min_max(
            egui::pos2(placed.atlas_min.x / atlas.x, placed.atlas_min.y / atlas.y),
            egui::pos2(placed.atlas_max.x / atlas.x, placed.atlas_max.y / atlas.y),
        );
        painter.image(texture.id(), rect, uv, tint);
    }
}

/// The line being spoken, across the bottom.
fn subtitle(ctx: &egui::Context, host: &Host, fx: &mut ScreenFx) {
    let Some(subtitle) = host.subtitle.as_ref() else {
        return;
    };
    // A line with no text shows as its own id, which is useful in an
    // editor and is a file name leaking onto the screen in a game.
    if subtitle.is_placeholder() {
        return;
    }
    // The game's own lettering, when it has some: subtitles belong to
    // the picture, and the picture has a typeface.
    if let Some(font) = pix_font(host) {
        let font = font.clone();
        let area = ctx.content_rect();
        let centre = egui::pos2(area.center().x, area.max.y - 70.0);
        if let Some(speaker) = subtitle.speaker.clone() {
            pix_text(
                ctx,
                fx,
                host,
                &font,
                centre - egui::vec2(0.0, font.line_height as f32 * 2.5),
                &speaker,
                egui::Color32::from_rgb(220, 200, 120),
            );
        }
        pix_text(
            ctx,
            fx,
            host,
            &font,
            centre,
            &subtitle.text,
            egui::Color32::from_rgb(250, 245, 225),
        );
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
fn draw_screen(
    ctx: &egui::Context,
    screen: &Screen,
    host: &Host,
    fx: &mut ScreenFx,
) -> Option<ScreenAction> {
    let area = ctx.content_rect();
    let scale = (area.width() / screen.size.x.max(1.0))
        .min(area.height() / screen.size.y.max(1.0))
        .max(0.05);
    let drawn = egui::vec2(screen.size.x * scale, screen.size.y * scale);
    let origin = area.center() - drawn * 0.5;

    // Which widgets the game shows right now, conditions included.
    let holds = |flag: &str| host.game.state.flag(flag);
    let shown: Vec<&infengine_adventure::screen::Widget> = screen
        .widgets
        .iter()
        .filter(|widget| widget.shown(&holds))
        .collect();

    // Notice appearances. On the first frame of a screen everything is
    // new, and popping a whole menu open would be noise - so that frame
    // only records. Pops are for what arrives *while* the player looks:
    // a script sets a flag, and the button it reveals announces itself.
    let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.1);
    for left in fx.pops.values_mut() {
        *left += dt;
    }
    fx.pops.retain(|_, t| *t < POP);
    let now: HashSet<String> = shown.iter().map(|w| w.id.clone()).collect();
    if fx.screen == screen.id {
        for id in now.difference(&fx.seen) {
            fx.pops.insert(id.clone(), 0.0);
        }
    } else {
        fx.screen = screen.id.clone();
        fx.pops.clear();
    }
    fx.seen = now;
    if !fx.pops.is_empty() {
        ctx.request_repaint();
    }

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

            let face = pix_font(host).cloned();
            for widget in shown {
                let lo = widget.min.min(widget.max);
                let hi = widget.min.max(widget.max);
                let mut bounds = egui::Rect::from_min_max(
                    rect.min + egui::vec2(lo.x * scale, lo.y * scale),
                    rect.min + egui::vec2(hi.x * scale, hi.y * scale),
                );
                // The pop: briefly larger, settling to place.
                let mut boost = 1.0;
                if let Some(t) = fx.pops.get(&widget.id) {
                    let eased = 1.0 - (t / POP).clamp(0.0, 1.0);
                    boost += 0.28 * eased * eased;
                    bounds = egui::Rect::from_center_size(
                        bounds.center(),
                        bounds.size() * boost,
                    );
                }
                if let Some(action) = widget_ui(
                    ui,
                    widget,
                    bounds,
                    scale * boost,
                    host,
                    face.as_ref(),
                    &mut fx.art,
                ) {
                    fired = Some(action);
                }
            }
        });
    fired
}

/// Fetch a widget's art, loading it on first use.
///
/// Resolved in the player's text language, the same rule room layers
/// follow - a note is a picture of words.
fn art_for<'a>(
    ctx: &egui::Context,
    host: &Host,
    cache: &'a mut HashMap<String, Option<egui::TextureHandle>>,
    image: &str,
) -> &'a Option<egui::TextureHandle> {
    cache.entry(image.to_string()).or_insert_with(|| {
        let path = host.project.resolve_in(image, &host.language.text);
        let loaded = image::open(path).ok()?;
        let rgba = loaded.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        Some(ctx.load_texture(
            format!("screen:{image}"),
            pixels,
            egui::TextureOptions::NEAREST,
        ))
    })
}

#[allow(clippy::too_many_arguments)]
fn widget_ui(
    ui: &mut egui::Ui,
    widget: &infengine_adventure::screen::Widget,
    bounds: egui::Rect,
    scale: f32,
    host: &Host,
    face: Option<&infengine_adventure::pixfont::PixFont>,
    art: &mut HashMap<String, Option<egui::TextureHandle>>,
) -> Option<ScreenAction> {
    match &widget.kind {
        WidgetKind::Image { image } => {
            if let Some(texture) = art_for(ui.ctx(), host, art, image) {
                egui::Image::new((texture.id(), bounds.size())).paint_at(ui, bounds);
            }
            None
        }
        WidgetKind::Label { line, size, align } => {
            draw_widget_text(
                ui,
                bounds,
                &text_of(host, line),
                *size * scale,
                *align,
                egui::Color32::WHITE,
                host,
                face,
                art,
            );
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
            draw_widget_text(
                ui,
                bounds,
                &text_of(host, line),
                *size * scale,
                *align,
                colour,
                host,
                face,
                art,
            );
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

/// A widget's words: the game's own lettering when it has some, the
/// system font otherwise.
#[allow(clippy::too_many_arguments)]
fn draw_widget_text(
    ui: &mut egui::Ui,
    bounds: egui::Rect,
    text: &str,
    size: f32,
    align: Align,
    colour: egui::Color32,
    host: &Host,
    face: Option<&infengine_adventure::pixfont::PixFont>,
    art: &mut HashMap<String, Option<egui::TextureHandle>>,
) {
    let Some(font) = face else {
        return paint(ui, bounds, text, size, align, colour);
    };
    let Some(texture) = art_for(ui.ctx(), host, art, &font.image).clone() else {
        return paint(ui, bounds, text, size, align, colour);
    };
    let atlas = texture.size_vec2();
    // Sized like the widget asked, in whole pixels of the face.
    let scale = (size / font.line_height.max(1) as f32).floor().max(1.0);
    let total = font.measure(text) * scale;
    let origin = match align {
        Align::Left => egui::pos2(bounds.left(), bounds.center().y - total.y * 0.5),
        Align::Center => bounds.center() - egui::vec2(total.x * 0.5, total.y * 0.5),
        Align::Right => {
            egui::pos2(bounds.right() - total.x, bounds.center().y - total.y * 0.5)
        }
    };
    for placed in font.layout(text) {
        let cell = (placed.atlas_max - placed.atlas_min) * scale;
        let at = origin + egui::vec2(placed.offset.x, placed.offset.y) * scale;
        let uv = egui::Rect::from_min_max(
            egui::pos2(placed.atlas_min.x / atlas.x, placed.atlas_min.y / atlas.y),
            egui::pos2(placed.atlas_max.x / atlas.x, placed.atlas_max.y / atlas.y),
        );
        ui.painter().image(
            texture.id(),
            egui::Rect::from_min_size(at, egui::vec2(cell.x, cell.y)),
            uv,
            colour,
        );
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
