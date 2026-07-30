//! W47.27 - slot widget egui draw helpers (relocated from main.rs).
//!
//! W472: the drag&drop plumbing (`slot_with_dnd`, `hover_key_ops`)
//! is GONE - the inventory screen is a cursor-stack menu
//! (`ui::inventory_screen`). What remains are the plain painters
//! the HUD hotbar uses.

#![allow(unused_imports)]
#![allow(clippy::too_many_arguments)]

use crate::*;

/// Draw one inventory cell. Returns the response so the caller
/// can wire click handlers / drag detection / tooltips on top.
///
/// - `slot`: `Some((color, count, uv_rect))` for a stack, `None`
///   for an empty cell.
/// - `atlas`: optional egui texture id of the block atlas. When
///   set the slot paints the matching tile; when `None` it falls
///   back to a flat color swatch from `slot.0`.
/// - `selected`: outlines the cell in gold (active hotbar slot).
/// - `sense`: the egui interaction model (click, click_and_drag,
///   etc.) the underlying response should report.
pub(crate) fn draw_slot_widget(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    selected: bool,
    sense: egui::Sense,
) -> egui::Response {
    // W5.2b: callers that need a durability bar route through
    // `draw_slot_widget_with_durability` below. The 5-arg shim
    // keeps every existing call site untouched.
    draw_slot_widget_with_durability(ui, slot, atlas, selected, sense, None)
}

/// W5.2b: like `draw_slot_widget` but paints a remaining-uses bar
/// at the bottom of the cell when `durability` is `Some((cur, max))`
/// and `cur < max`. Gradient red -> yellow -> green tracks the
/// fraction.
pub(crate) fn draw_slot_widget_with_durability(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    selected: bool,
    sense: egui::Sense,
    durability: Option<(u16, u16)>,
) -> egui::Response {
    let size = egui::vec2(40.0, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter_at(rect);
    // Slot background: dark gray when empty, otherwise either the
    // atlas tile (preferred) or a flat color swatch.
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(24));
    if let Some((c, _, uv, _)) = slot {
        match atlas {
            Some(tex_id) => {
                let uv_rect = egui::Rect::from_min_max(
                    egui::pos2(uv[0], uv[1]),
                    egui::pos2(uv[2], uv[3]),
                );
                // Inset by 2px so the outline frame stays visible.
                let icon_rect = rect.shrink(2.0);
                painter.image(tex_id, icon_rect, uv_rect, egui::Color32::WHITE);
            }
            None => {
                let fill = egui::Color32::from_rgb(
                    (c[0] * 255.0) as u8,
                    (c[1] * 255.0) as u8,
                    (c[2] * 255.0) as u8,
                );
                painter.rect_filled(rect.shrink(2.0), 2.0, fill);
            }
        }
    }
    let outline = if selected {
        egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 220, 80))
    } else if resp.hovered() {
        egui::Stroke::new(2.0, egui::Color32::from_gray(200))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(120))
    };
    painter.rect_stroke(rect, 4.0, outline, egui::StrokeKind::Inside);
    if let Some((_, count, _, _)) = slot {
        // Stack count badge in the bottom-right corner. Hide for
        // singletons because Minecraft does the same.
        if count > 1 {
            let pos = egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0);
            // Drop shadow first for readability over light tiles.
            painter.text(
                pos + egui::vec2(1.0, 1.0),
                egui::Align2::RIGHT_BOTTOM,
                count.to_string(),
                egui::FontId::proportional(14.0),
                egui::Color32::BLACK,
            );
            painter.text(
                pos,
                egui::Align2::RIGHT_BOTTOM,
                count.to_string(),
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            );
        }
    }
    // W5.2b/W471: durability bar along the bottom edge. Hidden
    // when cur == max (untouched item), matching MC
    // `isBarVisible` (damage > 0). The MC bar is 13 device pixels
    // wide (`getBarWidth` = round(13 - damage*13/max)); our cell
    // is wider, so the width fraction is quantized to the same 13
    // steps instead. Color follows `getBarColor` exactly:
    // hsv(remaining_frac / 3, 1, 1) - a red -> yellow -> green
    // hue sweep over the CONTINUOUS remaining fraction (the RGB
    // piecewise form below is that hue formula expanded).
    if let Some((cur, max)) = durability
        && cur < max
        && max > 0
    {
        let frac = (cur as f32 / max as f32).clamp(0.0, 1.0);
        // getBarWidth: round(13 * remaining/max) thirteenths.
        let width_frac = (frac * 13.0).round() / 13.0;
        // Bar geometry: inset 4px on each side, 4px tall, sitting
        // 4px above the cell's bottom edge.
        let bar_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, rect.max.y - 8.0),
            egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0),
        );
        // Dark backing so the gradient stays legible against the
        // bright stack-count badge or icon.
        painter.rect_filled(bar_rect, 1.0, egui::Color32::from_gray(20));
        // hsvToRgb(frac / 3, 1, 1) expanded: hue 0 = red, 1/6 =
        // yellow, 1/3 = green.
        let r = if frac < 0.5 { 255 } else { (255.0 * (1.0 - frac) * 2.0) as u8 };
        let g = if frac < 0.5 { (255.0 * frac * 2.0) as u8 } else { 255 };
        let fill_rect = egui::Rect::from_min_max(
            bar_rect.min,
            egui::pos2(
                bar_rect.min.x + bar_rect.width() * width_frac,
                bar_rect.max.y,
            ),
        );
        painter.rect_filled(fill_rect, 1.0, egui::Color32::from_rgb(r, g, 0));
    }
    // W364: hover tooltip with the item name (the SlotIcon has
    // carried it since W5 - it was just never shown; R20 request).
    if let Some((_, _, _, name)) = slot {
        return resp.on_hover_text(name);
    }
    resp
}
