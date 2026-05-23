//! W47.27 - slot widget egui draw helpers (relocated from main.rs).

#![allow(unused_imports)]
#![allow(clippy::too_many_arguments)]

use crate::*;



/// Helper to render a slot with full drag&drop integration.
///
/// - `kind` identifies this slot (used as the DnD payload + as the
///   drop target id).
/// - When the slot is non-empty and `draggable` is true, the cell
///   becomes a drag source (the widget paints itself at the
///   cursor while held).
/// - Every slot is a drop target. On release, the action is
///   appended to `actions` for the post-pass to apply.
/// - Returns the underlying widget response so callers can layer
///   click handlers / tooltips on top.
pub(crate) fn slot_with_dnd(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    kind: SlotKind,
    selected: bool,
    draggable: bool,
    actions: &mut Vec<InvAction>,
) -> egui::Response {
    slot_with_dnd_dur(ui, slot, atlas, kind, selected, draggable, actions, None)
}

/// W5.2b: `slot_with_dnd` variant that threads a durability
/// `(cur, max)` pair through to `draw_slot_widget_with_durability`.
/// New call sites that render the hotbar / inventory pass the
/// flint+steel durability here; everywhere else the legacy shim
/// above is fine.
#[allow(clippy::too_many_arguments)]
pub(crate) fn slot_with_dnd_dur(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    kind: SlotKind,
    selected: bool,
    draggable: bool,
    actions: &mut Vec<InvAction>,
    durability: Option<(u16, u16)>,
) -> egui::Response {
    // Scope the drag-source id through the parent ui so the same
    // SlotKind can appear in multiple layers (e.g. the hotbar Area
    // + the hotbar mirror inside the Inventory window) without
    // egui flagging it as a layer-id switch on the same widget.
    let id = ui.make_persistent_id(("inv_slot", kind));
    let resp = if draggable && slot.is_some() {
        ui.dnd_drag_source(id, kind, |ui| {
            draw_slot_widget_with_durability(
                ui, slot, atlas, selected, egui::Sense::click_and_drag(), durability,
            )
        })
        .response
    } else {
        draw_slot_widget_with_durability(
            ui, slot, atlas, selected, egui::Sense::click(), durability,
        )
    };
    if let Some(from) = resp.dnd_release_payload::<SlotKind>() {
        let from = *from;
        if from != kind {
            // Modifier mapping:
            //   Ctrl/Cmd held = move exactly one item (good for
            //                   filling craft slots).
            //   Shift held    = move half the stack (split-in-half
            //                   matches Minecraft's right-click
            //                   pickup behaviour).
            //   No modifier   = move the whole stack.
            let (shift, ctrl) =
                ui.input(|i| (i.modifiers.shift, i.modifiers.command));
            let split = if ctrl {
                SplitMode::One
            } else if shift {
                SplitMode::Half
            } else {
                SplitMode::All
            };
            actions.push(InvAction::Drag { from, to: kind, split });
        }
    }
    // Hover tooltip: show the block name + stack count. The wrapped
    // response is dropped silently because all we want is the side
    // effect of registering the tooltip.
    if let Some((_, count, _, name)) = slot {
        let resp = if count > 1 {
            resp.on_hover_text(format!("{name} x{count}"))
        } else {
            resp.on_hover_text(name)
        };
        return resp;
    }
    resp
}

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
    // W5.2b: durability bar along the bottom edge. Hidden when
    // cur == max (untouched tool) so the cell stays clean until
    // the player actually wears it down.
    if let Some((cur, max)) = durability
        && cur < max
        && max > 0
    {
        let frac = (cur as f32 / max as f32).clamp(0.0, 1.0);
        // Bar geometry: inset 4px on each side, 4px tall, sitting
        // 4px above the cell's bottom edge.
        let bar_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, rect.max.y - 8.0),
            egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0),
        );
        // Dark backing so the gradient stays legible against the
        // bright stack-count badge or icon.
        painter.rect_filled(bar_rect, 1.0, egui::Color32::from_gray(20));
        // Gradient: red (0%) -> yellow (50%) -> green (100%).
        let r = if frac < 0.5 { 255 } else { (255.0 * (1.0 - frac) * 2.0) as u8 };
        let g = if frac < 0.5 { (255.0 * frac * 2.0) as u8 } else { 255 };
        let fill_rect = egui::Rect::from_min_max(
            bar_rect.min,
            egui::pos2(bar_rect.min.x + bar_rect.width() * frac, bar_rect.max.y),
        );
        painter.rect_filled(fill_rect, 1.0, egui::Color32::from_rgb(r, g, 0));
    }
    resp
}
