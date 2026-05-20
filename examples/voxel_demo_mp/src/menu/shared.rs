//! Shared egui widgets reused across menu screens.
//!
//! Centralised so the Main / Singleplayer / Multiplayer / Options
//! screens all use the same Minecraft-flavoured button shape +
//! footer style. Pulled out so a future re-skin only has to touch
//! one file.
//!
//! Note: while the helpers live in their own module, they don't
//! own any state; each one takes the parent `egui::Ui` + the
//! current frame's data references.
//!
//! W21.3: buttons now carry MC-flavoured stone/beige background
//! colours, a 2 px border, and a subtle shadow on the label.

use egui::{Color32, RichText, Sense, Stroke, Vec2};

// ---------------------------------------------------------------------------
// Button colours (W21.3)
// ---------------------------------------------------------------------------

/// Stone-grey fill used for normal (un-hovered) MC-style buttons.
const BTN_BG: Color32 = Color32::from_rgb(84, 84, 84);
/// Slightly lighter fill shown on hover.
const BTN_BG_HOVER: Color32 = Color32::from_rgb(106, 106, 106);
/// Even lighter fill while the mouse button is held down.
const BTN_BG_PRESS: Color32 = Color32::from_rgb(60, 60, 60);
/// Disabled-state fill (very dark grey).
const BTN_BG_DISABLED: Color32 = Color32::from_rgb(50, 50, 50);
/// Bright border highlight - the top/left 1 px edge.
const BTN_BORDER_LIGHT: Color32 = Color32::from_rgb(170, 170, 170);
/// Dark border shadow - the bottom/right 1 px edge.
const BTN_BORDER_DARK: Color32 = Color32::from_rgb(40, 40, 40);
/// Label text colour for active buttons.
const BTN_LABEL: Color32 = Color32::from_rgb(255, 255, 160);
/// Label text colour for disabled buttons.
const BTN_LABEL_DISABLED: Color32 = Color32::from_rgb(100, 100, 100);
/// Drop-shadow offset in pixels for the label.
const SHADOW_OFFSET: f32 = 1.5;
/// Corner rounding for the button rectangle.
const BTN_ROUNDING: f32 = 2.0;

// ---------------------------------------------------------------------------
// Helpers for drawing MC-style buttons
// ---------------------------------------------------------------------------

/// Draw a single MC-style button of the given size. Returns true
/// exactly on the click frame.
fn mc_button_sized(
    ui: &mut egui::Ui,
    label: &str,
    desired: Vec2,
    enabled: bool,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        let bg = if !enabled {
            BTN_BG_DISABLED
        } else if resp.is_pointer_button_down_on() {
            BTN_BG_PRESS
        } else if resp.hovered() {
            BTN_BG_HOVER
        } else {
            BTN_BG
        };

        // Background fill.
        painter.rect_filled(rect, BTN_ROUNDING, bg);

        // 1 px highlight on top + left edges.
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, BTN_BORDER_LIGHT),
        );
        painter.line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1.0, BTN_BORDER_LIGHT),
        );
        // 1 px shadow on bottom + right edges.
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, BTN_BORDER_DARK),
        );
        painter.line_segment(
            [rect.right_top(), rect.right_bottom()],
            Stroke::new(1.0, BTN_BORDER_DARK),
        );

        // Label with a 1.5 px drop shadow.
        let text_colour = if enabled { BTN_LABEL } else { BTN_LABEL_DISABLED };
        let shadow_colour = Color32::from_rgba_unmultiplied(0, 0, 0, 160);
        let font_id = egui::FontId::proportional(14.0);
        let centre = rect.center();

        // Shadow pass - offset down-right.
        painter.text(
            centre + egui::vec2(SHADOW_OFFSET, SHADOW_OFFSET),
            egui::Align2::CENTER_CENTER,
            label,
            font_id.clone(),
            shadow_colour,
        );
        // Main label.
        painter.text(
            centre,
            egui::Align2::CENTER_CENTER,
            label,
            font_id,
            text_colour,
        );
    }

    enabled && resp.clicked()
}

// ---------------------------------------------------------------------------
// Public button widgets
// ---------------------------------------------------------------------------

/// Big MC-style menu button. Fixed 320x32 footprint matches the
/// proportions in the user's MC reference screenshots; the
/// `enabled` flag greys out non-applicable actions (e.g. "Hrat
/// vybrany svet" before a slot is selected).
///
/// Returns `true` exactly on the click frame so the caller can
/// emit a `MenuAction` once per click.
pub(crate) fn big_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    mc_button_sized(ui, label, Vec2::new(320.0, 32.0), enabled)
}

/// Narrower button variant for tertiary actions ("Zpet",
/// "Obnovit"). Same 32 px height so the button bar lines up.
pub(crate) fn small_button(ui: &mut egui::Ui, label: &str) -> bool {
    mc_button_sized(ui, label, Vec2::new(140.0, 32.0), true)
}

// ---------------------------------------------------------------------------
// Footer helpers
// ---------------------------------------------------------------------------

/// Footer label "Playing as: <name>" shown on every screen.
pub(crate) fn footer_identity(ui: &mut egui::Ui, name: &str) {
    ui.label(
        RichText::new(format!("Playing as: {name}"))
            .color(Color32::from_rgb(180, 200, 200))
            .italics(),
    );
}

/// Footer label "Seed: <seed>".
pub(crate) fn footer_seed(ui: &mut egui::Ui, seed: u32) {
    ui.label(
        RichText::new(format!("Seed: {seed}"))
            .color(Color32::from_rgb(150, 180, 180))
            .small()
            .italics(),
    );
}

/// W21.4: combined single-line footer showing player identity +
/// world seed on one row. When a real MC resource pack is loaded
/// (`pack_block_count > 0`), a bottom-left `egui::Area` shows
/// the pack name + block count.
pub(crate) fn footer_combined(
    ui: &mut egui::Ui,
    name: &str,
    seed: u32,
    pack_name: &str,
    pack_block_count: usize,
) {
    // Single horizontal row: "Playing as: Michal | Seed: 12345"
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Playing as: {name}"))
                .color(Color32::from_rgb(180, 200, 200))
                .small()
                .italics(),
        );
        ui.label(
            RichText::new("|")
                .color(Color32::from_rgb(100, 120, 120))
                .small(),
        );
        ui.label(
            RichText::new(format!("Seed: {seed}"))
                .color(Color32::from_rgb(150, 180, 180))
                .small()
                .italics(),
        );
    });

    // Pack indicator in a bottom-left Area (outside the ui flow).
    if pack_block_count > 0 {
        egui::Area::new(egui::Id::new("menu_pack_footer"))
            .anchor(egui::Align2::LEFT_BOTTOM, [8.0, -8.0])
            .show(ui.ctx(), |ui| {
                ui.label(
                    RichText::new(format!(
                        "MC pack: {pack_name} | {pack_block_count} blocks"
                    ))
                    .color(Color32::from_rgb(120, 160, 120))
                    .small()
                    .italics(),
                );
            });
    }
}

/// Footer label "v0.0.1" anchored to the bottom-right via
/// `egui::Area`. Drawn once per screen from the main draw fn.
pub(crate) fn footer_version(ctx: &egui::Context, version: &str) {
    egui::Area::new(egui::Id::new("menu_version_footer"))
        .anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -8.0])
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!("v{version}"))
                    .color(Color32::from_rgb(120, 140, 140))
                    .small()
                    .italics(),
            );
        });
}

/// Format a `chrono::DateTime<Utc>` as `dd.MM.yy HH:mm` in local
/// time. Falls back to the raw RFC3339 string if the conversion
/// fails (won't happen in practice; the `chrono` API is total).
pub(crate) fn format_last_played(ts: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::Local;
    let local = ts.with_timezone(&Local);
    local.format("%d.%m.%y %H:%M").to_string()
}

/// 16x16 placeholder thumbnail. Picks a stable colour from the
/// slug's hash so each row gets a visually distinct chip. Real
/// thumbnails ship in G.7 (deferred).
pub(crate) fn placeholder_thumb(ui: &mut egui::Ui, slug: &str) {
    let mut h: u32 = 0x811c_9dc5;
    for b in slug.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let r = (h & 0xff) as u8;
    let g = ((h >> 8) & 0xff) as u8;
    let b = ((h >> 16) & 0xff) as u8;
    let size = Vec2::new(32.0, 32.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn format_last_played_uses_dd_mm_yy_hh_mm() {
        let ts = Utc.with_ymd_and_hms(2026, 5, 19, 13, 45, 0).unwrap();
        // Local timezone may shift the hour/day; just make sure
        // the structure matches the dd.MM.yy HH:mm template (5
        // digits, 4 separators).
        let s = format_last_played(ts);
        let parts: Vec<&str> = s.split([' ', '.', ':']).collect();
        assert_eq!(parts.len(), 5, "expected dd.MM.yy HH:mm: got {s:?}");
    }
}
