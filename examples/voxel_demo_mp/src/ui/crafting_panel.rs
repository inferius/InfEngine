//! W50.3 + W54.2 - 3x3 crafting-table panel.
//!
//! Renders the egui::Window for the crafting_table block. The
//! 3x3 grid lives on `Voxel::craft_grid_3x3`; the panel handles
//! LMB / RMB / Shift+click / drag-distribute against it, computes
//! the output via `recipes::matcher::match_3x3`, and reports back
//! to the caller whether the player clicked the output slot.
//!
//! Mouse handling (W54.2, matches MC `AbstractContainerScreen`):
//! - LMB on slot with no held stack -> take whole stack.
//! - LMB on slot with held stack -> place / merge / swap.
//! - RMB on slot with no held stack -> take half (rounded up).
//! - RMB on slot with held stack -> place one (merge same item).
//! - LMB drag across N slots while holding a stack -> distribute
//!   the cursor stack as evenly as possible; remainder back on
//!   cursor.
//! - RMB drag across N slots while holding a stack -> place one
//!   item per slot; remainder back on cursor.
//! - Held stack always painted as a floating sprite at the mouse
//!   pointer so the player sees what's on the cursor.

use infengine_voxel::BlockId;

use crate::container;
use crate::recipes::matcher;

/// All inputs the 3x3 panel needs in one struct so render.rs
/// doesn't have to grow another 10-arg call site.
pub(crate) struct CraftingPanelInputs<'a> {
    pub(crate) ctx: &'a egui::Context,
    pub(crate) grid: &'a mut [Option<(BlockId, u8)>; 9],
    pub(crate) held: &'a mut Option<(u16, u8)>,
    pub(crate) inv_view: &'a mut [Option<(u16, u8)>],
    pub(crate) atlas_tex_id: Option<egui::TextureId>,
    pub(crate) icon_uv_resolver: &'a dyn Fn(u16) -> Option<[f32; 4]>,
    pub(crate) open_local: &'a mut bool,
    pub(crate) take_output_out: &'a mut bool,
}

/// Section + slot index pair. Lets the drag tracker remember
/// which container a hovered slot belongs to so a drag that
/// crosses panel sections (3x3 grid <-> player inv) doesn't
/// merge them by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotRef {
    Grid(usize),
    Inv(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragButton {
    Left,
    Right,
}

/// Drag state stored in egui's per-context memory so it persists
/// across frames without growing Voxel. Keyed by a stable id.
#[derive(Debug, Clone, Default)]
struct DragState {
    button: Option<DragButton>,
    slots: Vec<SlotRef>,
}

pub(crate) fn render(inputs: CraftingPanelInputs<'_>) {
    if !*inputs.open_local {
        return;
    }
    let CraftingPanelInputs {
        ctx,
        grid,
        held,
        inv_view,
        atlas_tex_id,
        icon_uv_resolver,
        open_local,
        take_output_out,
    } = inputs;

    // Pointer state needed by the drag state machine. Pulled once
    // at the top so every per-slot loop sees the same values.
    let (
        lmb_pressed,
        lmb_released,
        lmb_down,
        rmb_pressed,
        rmb_released,
        rmb_down,
        shift_down,
        cursor_pos,
    ) = ctx.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.primary_down(),
            i.pointer.secondary_pressed(),
            i.pointer.secondary_released(),
            i.pointer.secondary_down(),
            i.modifiers.shift,
            i.pointer.hover_pos(),
        )
    });

    // Load drag state from egui memory.
    let drag_id = egui::Id::new("crafting_panel_drag");
    let mut drag: DragState = ctx
        .memory(|m| m.data.get_temp::<DragState>(drag_id).clone())
        .unwrap_or_default();

    // Collected single-click actions (only fire when no multi-slot
    // drag completed this frame).
    let mut clicks: Vec<(SlotRef, ClickKind)> = Vec::new();

    // Recipe book toggle persisted across frames via egui memory.
    let book_id = egui::Id::new("crafting_panel_recipe_book");
    let mut book_open: bool = ctx
        .memory(|m| m.data.get_temp::<bool>(book_id).unwrap_or(false));

    let screen = ctx.content_rect();
    egui::Window::new("Crafting Table")
        .open(open_local)
        .collapsible(false)
        .resizable(false)
        .default_pos([
            screen.center().x - 320.0,
            screen.center().y - 260.0,
        ])
        .default_size([620.0, 540.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(if book_open {
                        "Hide recipe book"
                    } else {
                        "Recipe book"
                    })
                    .clicked()
                {
                    book_open = !book_open;
                }
                ui.label(
                    "LMB take/place, RMB half/one, LMB-drag distribute, RMB-drag one-per-slot",
                );
            });
            ui.separator();

            // ---- 3x3 grid + output -----------------------------
            ui.horizontal(|ui| {
                for r in 0..3 {
                    ui.vertical(|ui| {
                        for c in 0..3 {
                            let idx = r * 3 + c;
                            let slot = grid[idx].map(|(b, n)| (b.0, n));
                            let resp = draw_slot(
                                ui,
                                slot,
                                36.0,
                                atlas_tex_id,
                                icon_uv_resolver,
                            );
                            track_slot(
                                &mut drag,
                                &mut clicks,
                                resp,
                                SlotRef::Grid(idx),
                                held.is_some(),
                                lmb_pressed,
                                lmb_down,
                                rmb_pressed,
                                rmb_down,
                                shift_down,
                            );
                        }
                    });
                }

                ui.label("  ->  ");

                // Output slot.
                let match_result = matcher::match_3x3(&grid_as_u16(grid));
                let out_slot = match_result.map(|(id, n, _)| (id, n));
                let resp = draw_slot(
                    ui,
                    out_slot,
                    36.0,
                    atlas_tex_id,
                    icon_uv_resolver,
                );
                if (resp.clicked() || resp.secondary_clicked())
                    && match_result.is_some()
                {
                    *take_output_out = true;
                }
            });

            ui.separator();
            ui.label("Inventory (36)");
            // Player inventory mirror (4 rows x 9 cols).
            for r in 0..4 {
                ui.horizontal(|ui| {
                    for c in 0..9 {
                        let idx = r * 9 + c;
                        let slot = inv_view.get(idx).copied().flatten();
                        let resp = draw_slot(
                            ui,
                            slot,
                            36.0,
                            atlas_tex_id,
                            icon_uv_resolver,
                        );
                        track_slot(
                            &mut drag,
                            &mut clicks,
                            resp,
                            SlotRef::Inv(idx),
                            held.is_some(),
                            lmb_pressed,
                            lmb_down,
                            rmb_pressed,
                            rmb_down,
                            shift_down,
                        );
                    }
                });
            }

            // ---- Recipe book ---------------------------------
            if book_open {
                ui.separator();
                ui.label("Recipe Book");
                draw_recipe_book(
                    ui,
                    atlas_tex_id,
                    icon_uv_resolver,
                    inv_view,
                    grid,
                );
            }
        });
    // Persist recipe book state.
    ctx.memory_mut(|m| m.data.insert_temp(book_id, book_open));

    // ---- Drag state machine: react to release. -----------------
    let mut drag_committed = false;
    if lmb_released
        && drag.button == Some(DragButton::Left)
        && drag.slots.len() >= 2
    {
        apply_drag_distribute(grid, inv_view, held, &drag.slots);
        drag_committed = true;
    } else if rmb_released
        && drag.button == Some(DragButton::Right)
        && drag.slots.len() >= 2
    {
        apply_drag_one_per_slot(grid, inv_view, held, &drag.slots);
        drag_committed = true;
    }
    // Release of any button clears the drag tracker.
    if (lmb_released && drag.button == Some(DragButton::Left))
        || (rmb_released && drag.button == Some(DragButton::Right))
    {
        drag = DragState::default();
    }
    // Apply single-slot clicks ONLY when no multi-slot drag fired
    // this frame. Otherwise a 2-slot drag would also fire a click
    // on the slot the user pressed on.
    if !drag_committed {
        for (sref, kind) in clicks {
            apply_click(grid, inv_view, held, sref, kind);
        }
    }
    // Persist drag state for the next frame.
    ctx.memory_mut(|m| m.data.insert_temp(drag_id, drag));

    // ---- Floating cursor sprite. ------------------------------
    if let (Some(pos), Some((id, count))) = (cursor_pos, *held) {
        paint_cursor(ctx, pos, id, count, atlas_tex_id, icon_uv_resolver);
    }
}

#[derive(Debug, Clone, Copy)]
enum ClickKind {
    Lmb,
    Rmb,
    ShiftLmb,
}

#[allow(clippy::too_many_arguments)]
fn track_slot(
    drag: &mut DragState,
    clicks: &mut Vec<(SlotRef, ClickKind)>,
    resp: egui::Response,
    sref: SlotRef,
    has_held: bool,
    lmb_pressed: bool,
    lmb_down: bool,
    rmb_pressed: bool,
    rmb_down: bool,
    shift_down: bool,
) {
    // Drag tracking: while the matching button is held down and
    // the cursor hovers a slot, add it to the visit list (unique).
    // Requires `has_held` so empty-cursor drag doesn't accidentally
    // hoover up stacks.
    if has_held && resp.hovered() {
        if lmb_down && drag.button != Some(DragButton::Right) {
            if drag.button.is_none() && lmb_pressed {
                drag.button = Some(DragButton::Left);
            }
            if drag.button == Some(DragButton::Left)
                && !drag.slots.contains(&sref)
            {
                drag.slots.push(sref);
            }
        } else if rmb_down && drag.button != Some(DragButton::Left) {
            if drag.button.is_none() && rmb_pressed {
                drag.button = Some(DragButton::Right);
            }
            if drag.button == Some(DragButton::Right)
                && !drag.slots.contains(&sref)
            {
                drag.slots.push(sref);
            }
        }
    }
    // Standard click handling: fires on release inside the widget.
    if resp.clicked() {
        clicks.push((
            sref,
            if shift_down {
                ClickKind::ShiftLmb
            } else {
                ClickKind::Lmb
            },
        ));
    } else if resp.secondary_clicked() {
        clicks.push((sref, ClickKind::Rmb));
    }
}

fn apply_click(
    grid: &mut [Option<(BlockId, u8)>; 9],
    inv: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    sref: SlotRef,
    kind: ClickKind,
) {
    match kind {
        ClickKind::Lmb => match sref {
            SlotRef::Grid(i) => slot_lmb_grid(grid, held, i),
            SlotRef::Inv(i) => slot_lmb_inv(inv, held, i),
        },
        ClickKind::Rmb => match sref {
            SlotRef::Grid(i) => slot_rmb_grid(grid, held, i),
            SlotRef::Inv(i) => slot_rmb_inv(inv, held, i),
        },
        ClickKind::ShiftLmb => match sref {
            SlotRef::Grid(i) => shift_grid_to_inv(grid, inv, i),
            SlotRef::Inv(i) => shift_inv_to_grid(inv, grid, i),
        },
    }
}

fn cap(id: u16) -> u8 {
    crate::item::item_max_stack(id)
}

// ---- single-slot LMB ----------------------------------------

fn slot_lmb_grid(
    grid: &mut [Option<(BlockId, u8)>; 9],
    held: &mut Option<(u16, u8)>,
    idx: usize,
) {
    let Some(slot) = grid.get_mut(idx) else { return };
    match (*slot, *held) {
        (Some((b, c)), None) => {
            *held = Some((b.0, c));
            *slot = None;
        }
        (None, Some((hid, hn))) => {
            *slot = Some((BlockId(hid), hn));
            *held = None;
        }
        (Some((b, c)), Some((hid, hn))) if b.0 == hid => {
            let cap = cap(hid);
            let room = cap.saturating_sub(c);
            let merge = room.min(hn);
            *slot = Some((b, c + merge));
            let left = hn - merge;
            *held = if left == 0 { None } else { Some((hid, left)) };
        }
        (Some((b, c)), Some((hid, hn))) => {
            *slot = Some((BlockId(hid), hn));
            *held = Some((b.0, c));
        }
        (None, None) => {}
    }
}

fn slot_lmb_inv(
    inv: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    idx: usize,
) {
    let Some(slot) = inv.get_mut(idx) else { return };
    match (*slot, *held) {
        (Some(s), None) => {
            *held = Some(s);
            *slot = None;
        }
        (None, Some(h)) => {
            *slot = Some(h);
            *held = None;
        }
        (Some((sid, sc)), Some((hid, hn))) if sid == hid => {
            let cap = cap(hid);
            let room = cap.saturating_sub(sc);
            let merge = room.min(hn);
            *slot = Some((sid, sc + merge));
            let left = hn - merge;
            *held = if left == 0 { None } else { Some((hid, left)) };
        }
        (Some(s), Some(h)) => {
            *slot = Some(h);
            *held = Some(s);
        }
        (None, None) => {}
    }
}

// ---- single-slot RMB ----------------------------------------

fn slot_rmb_grid(
    grid: &mut [Option<(BlockId, u8)>; 9],
    held: &mut Option<(u16, u8)>,
    idx: usize,
) {
    let Some(slot) = grid.get_mut(idx) else { return };
    match (*slot, *held) {
        (Some((b, c)), None) => {
            let half = (c as u16).div_ceil(2) as u8;
            let rem = c - half;
            if half > 0 {
                *held = Some((b.0, half));
            }
            *slot = if rem == 0 { None } else { Some((b, rem)) };
        }
        (None, Some((hid, hn))) if hn > 0 => {
            *slot = Some((BlockId(hid), 1));
            let left = hn - 1;
            *held = if left == 0 { None } else { Some((hid, left)) };
        }
        (Some((b, c)), Some((hid, hn))) if b.0 == hid && c < cap(hid) => {
            *slot = Some((b, c + 1));
            let left = hn.saturating_sub(1);
            *held = if left == 0 { None } else { Some((hid, left)) };
        }
        _ => {}
    }
}

fn slot_rmb_inv(
    inv: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    idx: usize,
) {
    let Some(slot) = inv.get_mut(idx) else { return };
    match (*slot, *held) {
        (Some((id, c)), None) => {
            let half = (c as u16).div_ceil(2) as u8;
            let rem = c - half;
            if half > 0 {
                *held = Some((id, half));
            }
            *slot = if rem == 0 { None } else { Some((id, rem)) };
        }
        (None, Some((hid, hn))) if hn > 0 => {
            *slot = Some((hid, 1));
            let left = hn - 1;
            *held = if left == 0 { None } else { Some((hid, left)) };
        }
        (Some((sid, sc)), Some((hid, hn))) if sid == hid && sc < cap(hid) => {
            *slot = Some((sid, sc + 1));
            let left = hn.saturating_sub(1);
            *held = if left == 0 { None } else { Some((hid, left)) };
        }
        _ => {}
    }
}

// ---- shift-click quick-move ---------------------------------

fn shift_grid_to_inv(
    grid: &mut [Option<(BlockId, u8)>; 9],
    inv: &mut [Option<(u16, u8)>],
    idx: usize,
) {
    let Some(slot) = grid.get_mut(idx) else { return };
    let Some((b, c)) = slot.take() else { return };
    let leftover = inv_insert(inv, b.0, c);
    if leftover > 0 {
        *slot = Some((b, leftover));
    }
}

fn shift_inv_to_grid(
    inv: &mut [Option<(u16, u8)>],
    grid: &mut [Option<(BlockId, u8)>; 9],
    idx: usize,
) {
    let Some(slot) = inv.get_mut(idx) else { return };
    let Some((id, c)) = slot.take() else { return };
    let mut remaining = c;
    // Pass 1: top up matching stacks.
    let cap = cap(id);
    for cell in grid.iter_mut() {
        if remaining == 0 {
            break;
        }
        if let Some((b, sc)) = cell.as_mut()
            && b.0 == id
            && *sc < cap
        {
            let room = cap - *sc;
            let take = room.min(remaining);
            *sc += take;
            remaining -= take;
        }
    }
    // Pass 2: claim empty cells.
    for cell in grid.iter_mut() {
        if remaining == 0 {
            break;
        }
        if cell.is_none() {
            let take = cap.min(remaining);
            *cell = Some((BlockId(id), take));
            remaining -= take;
        }
    }
    if remaining > 0 {
        *slot = Some((id, remaining));
    }
}

/// Insert into the u16-inv (mirrors `inventory::inv_pickup_n`).
/// Returns leftover.
fn inv_insert(inv: &mut [Option<(u16, u8)>], id: u16, n: u8) -> u8 {
    let cap = cap(id);
    let mut remaining = n;
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return 0;
        }
        if let Some((sid, sc)) = slot.as_mut()
            && *sid == id
            && *sc < cap
        {
            let room = cap - *sc;
            let take = room.min(remaining);
            *sc += take;
            remaining -= take;
        }
    }
    for slot in inv.iter_mut() {
        if remaining == 0 {
            return 0;
        }
        if slot.is_none() {
            let take = cap.min(remaining);
            *slot = Some((id, take));
            remaining -= take;
        }
    }
    remaining
}

// ---- drag actions -------------------------------------------

/// LMB-drag: distribute the held stack across the visited slots
/// as evenly as possible (rounded down per slot, remainder back
/// on the cursor). MC convention: same-item slots get the share
/// merged on top of their existing count; empty slots become a
/// fresh stack. Mismatched items are skipped (their slot doesn't
/// receive a share + the share goes back on the cursor).
fn apply_drag_distribute(
    grid: &mut [Option<(BlockId, u8)>; 9],
    inv: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    slots: &[SlotRef],
) {
    let Some((hid, total)) = *held else { return };
    if slots.is_empty() {
        return;
    }
    let per = total / slots.len() as u8;
    if per == 0 {
        return;
    }
    let mut placed: u16 = 0;
    let cap = cap(hid);
    for sref in slots {
        match sref {
            SlotRef::Grid(i) => {
                let Some(slot) = grid.get_mut(*i) else { continue };
                match *slot {
                    None => {
                        let take = per.min(cap);
                        *slot = Some((BlockId(hid), take));
                        placed += take as u16;
                    }
                    Some((b, c)) if b.0 == hid && c < cap => {
                        let room = cap - c;
                        let take = per.min(room);
                        *slot = Some((b, c + take));
                        placed += take as u16;
                    }
                    _ => {}
                }
            }
            SlotRef::Inv(i) => {
                let Some(slot) = inv.get_mut(*i) else { continue };
                match *slot {
                    None => {
                        let take = per.min(cap);
                        *slot = Some((hid, take));
                        placed += take as u16;
                    }
                    Some((sid, sc)) if sid == hid && sc < cap => {
                        let room = cap - sc;
                        let take = per.min(room);
                        *slot = Some((sid, sc + take));
                        placed += take as u16;
                    }
                    _ => {}
                }
            }
        }
    }
    let leftover = (total as u16).saturating_sub(placed) as u8;
    *held = if leftover == 0 {
        None
    } else {
        Some((hid, leftover))
    };
}

/// RMB-drag: place exactly one item per visited slot. Mismatched
/// items skipped; remainder back on the cursor.
fn apply_drag_one_per_slot(
    grid: &mut [Option<(BlockId, u8)>; 9],
    inv: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    slots: &[SlotRef],
) {
    let Some((hid, total)) = *held else { return };
    if slots.is_empty() {
        return;
    }
    let mut remaining = total;
    let cap = cap(hid);
    for sref in slots {
        if remaining == 0 {
            break;
        }
        match sref {
            SlotRef::Grid(i) => {
                let Some(slot) = grid.get_mut(*i) else { continue };
                match *slot {
                    None => {
                        *slot = Some((BlockId(hid), 1));
                        remaining -= 1;
                    }
                    Some((b, c)) if b.0 == hid && c < cap => {
                        *slot = Some((b, c + 1));
                        remaining -= 1;
                    }
                    _ => {}
                }
            }
            SlotRef::Inv(i) => {
                let Some(slot) = inv.get_mut(*i) else { continue };
                match *slot {
                    None => {
                        *slot = Some((hid, 1));
                        remaining -= 1;
                    }
                    Some((sid, sc)) if sid == hid && sc < cap => {
                        *slot = Some((sid, sc + 1));
                        remaining -= 1;
                    }
                    _ => {}
                }
            }
        }
    }
    *held = if remaining == 0 {
        None
    } else {
        Some((hid, remaining))
    };
}

// ---- slot rendering ----------------------------------------

fn draw_slot(
    ui: &mut egui::Ui,
    slot: Option<(u16, u8)>,
    tile_px: f32,
    atlas: Option<egui::TextureId>,
    uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
) -> egui::Response {
    let size = egui::vec2(tile_px, tile_px);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(24));
    paint_slot_inner(&painter, rect, slot, atlas, uv_resolver);
    let outline = if resp.hovered() {
        egui::Stroke::new(2.0, egui::Color32::from_gray(200))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(120))
    };
    painter.rect_stroke(rect, 4.0, outline, egui::StrokeKind::Inside);
    resp
}

fn paint_slot_inner(
    painter: &egui::Painter,
    rect: egui::Rect,
    slot: Option<(u16, u8)>,
    atlas: Option<egui::TextureId>,
    uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
) {
    let Some((id, count)) = slot else { return };
    let resolved_uv = uv_resolver(id);
    match (atlas, resolved_uv) {
        (Some(tex), Some(uv)) => {
            let uv_rect = egui::Rect::from_min_max(
                egui::pos2(uv[0], uv[1]),
                egui::pos2(uv[2], uv[3]),
            );
            painter.image(tex, rect.shrink(2.0), uv_rect, egui::Color32::WHITE);
        }
        _ => {
            // Coloured swatch fallback (no atlas / pack missing icon).
            let r = (id.wrapping_mul(73) & 0xFF) as u8;
            let g = (id.wrapping_mul(151) & 0xFF) as u8;
            let b = (id.wrapping_mul(211) & 0xFF) as u8;
            painter.rect_filled(rect.shrink(2.0), 2.0, egui::Color32::from_rgb(r, g, b));
        }
    }
    if count > 1 {
        let pos = egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0);
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

/// Paint the held stack as a floating sprite at the cursor.
/// Routed through the foreground egui layer so it sits above the
/// crafting panel + everything else in the HUD.
fn paint_cursor(
    ctx: &egui::Context,
    pos: egui::Pos2,
    id: u16,
    count: u8,
    atlas: Option<egui::TextureId>,
    uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
) {
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("crafting_panel_cursor"),
    ));
    let size = egui::vec2(32.0, 32.0);
    let rect = egui::Rect::from_center_size(pos, size);
    paint_slot_inner(&layer, rect, Some((id, count)), atlas, uv_resolver);
}

/// Convert the BlockId-typed grid to plain u16 cells for the
/// matcher (which is BlockId-agnostic).
fn grid_as_u16(g: &[Option<(BlockId, u8)>; 9]) -> [Option<(u16, u8)>; 9] {
    std::array::from_fn(|i| g[i].map(|(b, c)| (b.0, c)))
}

/// W54.3 - Recipe book: scrollable grid of every known shaped +
/// shapeless recipe. Each tile shows the output icon; hover reads
/// the recipe name + ingredient list. Click auto-fills the 3x3
/// grid from the player inventory (if enough materials are
/// present); otherwise it just previews the pattern in the grid.
fn draw_recipe_book(
    ui: &mut egui::Ui,
    atlas_tex_id: Option<egui::TextureId>,
    icon_uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
    inv_view: &mut [Option<(u16, u8)>],
    grid: &mut [Option<(BlockId, u8)>; 9],
) {
    use crate::recipes::shaped::SHAPED_RECIPES;
    use crate::recipes::RECIPES;
    egui::ScrollArea::vertical()
        .max_height(180.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                // Shapeless 2x2 recipes (small craft only - the table
                // lists them anyway since 2x2 is a subset of 3x3).
                for recipe in RECIPES {
                    let entry = recipe_entry_shapeless(recipe);
                    draw_recipe_tile(
                        ui,
                        atlas_tex_id,
                        icon_uv_resolver,
                        &entry,
                        inv_view,
                        grid,
                    );
                }
                // Shaped 3x3 recipes.
                for recipe in SHAPED_RECIPES {
                    let entry = recipe_entry_shaped(recipe);
                    draw_recipe_tile(
                        ui,
                        atlas_tex_id,
                        icon_uv_resolver,
                        &entry,
                        inv_view,
                        grid,
                    );
                }
            });
        });
}

struct RecipeEntry {
    output: (u16, u8),
    name: &'static str,
    /// Per-cell input layout (row-major, 9 cells). `None` = empty.
    /// Shapeless recipes spread their inputs across the first N
    /// non-empty cells in row-major order.
    cells: [Option<u16>; 9],
}

fn recipe_entry_shapeless(r: &crate::recipes::Recipe) -> RecipeEntry {
    let mut cells: [Option<u16>; 9] = [None; 9];
    for (i, (id, _)) in r.inputs.iter().take(9).enumerate() {
        cells[i] = Some(*id);
    }
    RecipeEntry {
        output: r.output,
        name: r.name,
        cells,
    }
}

fn recipe_entry_shaped(r: &crate::recipes::shaped::ShapedRecipe) -> RecipeEntry {
    let mut cells: [Option<u16>; 9] = [None; 9];
    for row in 0..3 {
        for col in 0..3 {
            let id = r.pattern[row][col];
            cells[row * 3 + col] = if id == 0 { None } else { Some(id) };
        }
    }
    RecipeEntry {
        output: r.output,
        name: r.name,
        cells,
    }
}

fn draw_recipe_tile(
    ui: &mut egui::Ui,
    atlas_tex_id: Option<egui::TextureId>,
    icon_uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
    entry: &RecipeEntry,
    inv_view: &mut [Option<(u16, u8)>],
    grid: &mut [Option<(BlockId, u8)>; 9],
) {
    let resp = draw_slot(
        ui,
        Some(entry.output),
        28.0,
        atlas_tex_id,
        icon_uv_resolver,
    );
    let tooltip = build_recipe_tooltip(entry);
    let resp = resp.on_hover_text(tooltip);
    if resp.clicked() {
        // Auto-fill: pull each required input from the player inv
        // into the matching grid cell. Bail if any input is missing.
        autofill_grid_from_inv(entry, inv_view, grid);
    }
}

fn build_recipe_tooltip(entry: &RecipeEntry) -> String {
    let mut s = String::new();
    s.push_str(entry.name);
    s.push('\n');
    s.push_str(&format!(
        "-> {} x{}\n",
        crate::item::item_display_name(entry.output.0),
        entry.output.1,
    ));
    let mut tally: Vec<(u16, u32)> = Vec::new();
    for id in entry.cells.iter().flatten() {
        if let Some(t) = tally.iter_mut().find(|(b, _)| *b == *id) {
            t.1 += 1;
        } else {
            tally.push((*id, 1));
        }
    }
    s.push_str("Needs: ");
    for (i, (id, c)) in tally.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!(
            "{} x{}",
            crate::item::item_display_name(*id),
            c,
        ));
    }
    s
}

/// W54.3 - autofill: for each non-empty cell in the recipe, find
/// a matching item in the player inventory and move 1 into the
/// matching grid cell. Bails (leaves grid + inv untouched) if any
/// required input is missing.
fn autofill_grid_from_inv(
    entry: &RecipeEntry,
    inv_view: &mut [Option<(u16, u8)>],
    grid: &mut [Option<(BlockId, u8)>; 9],
) {
    // First sweep: check feasibility against a tally + a working
    // copy of the inventory. Mismatch -> bail without side effects.
    let mut needed: Vec<(u16, u32)> = Vec::new();
    for id in entry.cells.iter().flatten() {
        if let Some(t) = needed.iter_mut().find(|(b, _)| *b == *id) {
            t.1 += 1;
        } else {
            needed.push((*id, 1));
        }
    }
    for (id, count) in &needed {
        let available: u32 = inv_view
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|(sid, _)| *sid == *id)
            .map(|(_, c)| *c as u32)
            .sum();
        if available < *count {
            return; // not enough; abort.
        }
    }
    // Return whatever is already on the grid to the inventory so
    // the autofill starts from a clean grid.
    for cell in grid.iter_mut() {
        if let Some((b, c)) = cell.take() {
            let _ = inv_insert(inv_view, b.0, c);
        }
    }
    // Second sweep: actually move 1 of each required input into
    // the matching cell. Inventory mutated in place.
    for (idx, maybe_id) in entry.cells.iter().enumerate() {
        let Some(id) = maybe_id else { continue };
        if let Some(slot) = inv_view.iter_mut().find(|s| {
            s.as_ref().is_some_and(|(sid, c)| sid == id && *c > 0)
        }) {
            let (sid, sc) = slot.unwrap();
            let new_c = sc - 1;
            *slot = if new_c == 0 { None } else { Some((sid, new_c)) };
            grid[idx] = Some((BlockId(*id), 1));
        }
    }
}

// Keep the `container` import quiet about unused submodule.
const _: fn() = || {
    let _ = container::Container::with_size(0);
};
