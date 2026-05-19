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

use egui::{Color32, RichText, Sense, Vec2};

/// Big MC-style menu button. Fixed 320x32 footprint matches the
/// proportions in the user's MC reference screenshots; the
/// `enabled` flag greys out non-applicable actions (e.g. "Hrat
/// vybrany svet" before a slot is selected).
///
/// Returns `true` exactly on the click frame so the caller can
/// emit a `MenuAction` once per click.
pub(crate) fn big_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let desired = Vec2::new(320.0, 32.0);
    if enabled {
        let resp = ui.add_sized(desired, egui::Button::new(label));
        resp.clicked()
    } else {
        // Greyed-out variant: same footprint, no click. We could
        // use `ui.add_enabled` here but rolling it manually keeps
        // the colour control in this module.
        let resp = ui.add_sized(
            desired,
            egui::Button::new(RichText::new(label).color(Color32::from_gray(120))),
        );
        // Swallow the response so the click never fires even if
        // egui's add_enabled wrapper isn't perfectly tight.
        let _ = resp;
        false
    }
}

/// Narrower button variant for tertiary actions ("Zpet",
/// "Obnovit"). Same 32 px height so the button bar lines up.
pub(crate) fn small_button(ui: &mut egui::Ui, label: &str) -> bool {
    let desired = Vec2::new(140.0, 32.0);
    ui.add_sized(desired, egui::Button::new(label)).clicked()
}

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
