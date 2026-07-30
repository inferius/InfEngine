//! W472 - the survival inventory screen (E) as a real MC
//! `InventoryMenu`: craft 2x2 + result, 4 armor slots (type
//! filters + 1-item caps), main rows + hotbar - ONE cursor-stack
//! menu with the full doClick interaction set, replacing the old
//! egui drag&drop path.
//!
//! The panel is INPUT + PAINT only: every mutation routes through
//! the shared screen core (`inventory::inv_screen_apply_op` /
//! `inv_screen_apply_drag`), the same functions the server-side
//! wire applier uses - local prediction and authority cannot
//! drift. The caller (ui::render) snapshots craft / armor /
//! inventory / cursor into locals, hands them in mutably, writes
//! them back post-egui and forwards the recorded ops on the wire
//! when running as a client.
//!
//! Interactions (MC `InventoryMenu` + `AbstractContainerMenu`):
//! - LMB / RMB -> PICKUP against the cursor.
//! - Shift+LMB -> QUICK_MOVE per `InventoryMenu.quickMoveStack`
//!   (armor items AUTO-EQUIP into their empty armor slot; result
//!   empties into the player inv hotbar-end first).
//! - LMB / RMB drag-paint -> QUICK_CRAFT, applied on release.
//! - Double-click -> PICKUP_ALL collect.
//! - Number keys -> SWAP with the hotbar; Q / Ctrl+Q -> THROW.
//! - Creative middle-click -> CLONE.
//! - The held cursor paints as a floating sprite at the mouse.

use crate::inventory::{
    ARMOR_SLOTS, HOTBAR_SLOTS, INVENTORY_COLS, INVENTORY_ROWS,
    INVENTORY_SLOTS, SlotKind,
};
use crate::item::ItemStack;
use crate::net::{InvDragMsg, InvOpMsg, NetSlot};

/// All inputs the inventory screen needs in one struct (5+ args
/// rule).
pub(crate) struct InventoryScreenInputs<'a> {
    pub(crate) ctx: &'a egui::Context,
    pub(crate) craft: &'a mut [Option<ItemStack>; 4],
    pub(crate) armor: &'a mut [Option<ItemStack>; ARMOR_SLOTS],
    /// 36-slot player inventory view (hotbar-first array order).
    pub(crate) inv: &'a mut [Option<ItemStack>],
    pub(crate) cursor: &'a mut Option<ItemStack>,
    pub(crate) selected_slot: usize,
    pub(crate) atlas_tex_id: Option<egui::TextureId>,
    pub(crate) icon_uv_resolver: &'a dyn Fn(u16) -> Option<[f32; 4]>,
    pub(crate) open_local: &'a mut bool,
    /// Creative flag for the CLONE arm.
    pub(crate) creative: bool,
    /// Stacks thrown this frame (Q / Ctrl+Q / result throws) -
    /// render.rs spawns them (no-op in the client role).
    pub(crate) thrown_out: &'a mut Vec<ItemStack>,
    /// Every op applied this frame, in order - the client forwards
    /// these to the host (`vx.inv_op`).
    pub(crate) ops_out: &'a mut Vec<InvOpMsg>,
    /// A completed drag-paint (`vx.inv_drag` for clients).
    pub(crate) drag_out: &'a mut Option<InvDragMsg>,
}

/// Per-frame click intents collected while painting.
#[derive(Debug, Clone, Copy)]
enum ClickKind {
    Lmb,
    Rmb,
    ShiftLmb,
    Collect,
    SwapHotbar(u8),
    ThrowOne,
    ThrowAll,
    CloneStack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragButton {
    Left,
    Right,
}

/// Drag state persisted in egui memory across frames.
#[derive(Debug, Clone, Default)]
struct DragState {
    button: Option<DragButton>,
    slots: Vec<SlotKind>,
}

pub(crate) fn render(inputs: InventoryScreenInputs<'_>) {
    let InventoryScreenInputs {
        ctx,
        craft,
        armor,
        inv,
        cursor,
        selected_slot,
        atlas_tex_id,
        icon_uv_resolver,
        open_local,
        creative,
        thrown_out,
        ops_out,
        drag_out,
    } = inputs;
    if !*open_local {
        return;
    }

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

    let drag_id = egui::Id::new("inventory_screen_drag");
    let mut drag: DragState = ctx
        .memory(|m| m.data.get_temp::<DragState>(drag_id).clone())
        .unwrap_or_default();
    let mut clicks: Vec<(SlotKind, ClickKind)> = Vec::new();
    // Result-slot intents (virtual cell - not a SlotKind the drag
    // can visit).
    let mut result_clicks: Vec<ClickKind> = Vec::new();

    let screen = ctx.content_rect();
    egui::Window::new("Inventory")
        .open(open_local)
        .collapsible(false)
        .resizable(false)
        .default_pos([screen.center().x - 240.0, screen.center().y - 250.0])
        .default_size([480.0, 460.0])
        .show(ctx, |ui| {
            ui.label(
                "E to close - LMB take/place, RMB half/one, Shift quick-move, drag to spread",
            );
            ui.separator();
            ui.horizontal(|ui| {
                // Armor column (MC left rail: helmet -> boots).
                ui.vertical(|ui| {
                    ui.label("Armor");
                    for (a, piece) in armor.iter().enumerate() {
                        let kind = SlotKind::Armor(a);
                        let resp = draw_stack_slot(
                            ui,
                            piece.as_ref(),
                            36.0,
                            false,
                            atlas_tex_id,
                            icon_uv_resolver,
                        );
                        track_slot(TrackCtx {
                            drag: &mut drag,
                            clicks: &mut clicks,
                            resp,
                            kind,
                            slot: piece.as_ref(),
                            held: cursor.as_ref(),
                            lmb_pressed,
                            lmb_down,
                            rmb_pressed,
                            rmb_down,
                            shift_down,
                            ui,
                        });
                    }
                });
                ui.add_space(18.0);
                // Craft 2x2 + result.
                ui.vertical(|ui| {
                    ui.label("Craft");
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            for r in 0..2 {
                                ui.horizontal(|ui| {
                                    for c in 0..2 {
                                        let idx = r * 2 + c;
                                        let kind = SlotKind::Craft(idx);
                                        let resp = draw_stack_slot(
                                            ui,
                                            craft[idx].as_ref(),
                                            36.0,
                                            false,
                                            atlas_tex_id,
                                            icon_uv_resolver,
                                        );
                                        track_slot(TrackCtx {
                                            drag: &mut drag,
                                            clicks: &mut clicks,
                                            resp,
                                            kind,
                                            slot: craft[idx].as_ref(),
                                            held: cursor.as_ref(),
                                            lmb_pressed,
                                            lmb_down,
                                            rmb_pressed,
                                            rmb_down,
                                            shift_down,
                                            ui,
                                        });
                                    }
                                });
                            }
                        });
                        ui.label("  ->  ");
                        // Virtual result cell.
                        let grid: [Option<ItemStack>; 4] =
                            std::array::from_fn(|i| craft[i].clone());
                        let out = crate::match_recipe(&grid)
                            .map(|(b, n)| ItemStack::new(b.0, n));
                        let resp = draw_stack_slot(
                            ui,
                            out.as_ref(),
                            36.0,
                            false,
                            atlas_tex_id,
                            icon_uv_resolver,
                        );
                        track_result_slot(
                            ui,
                            &resp,
                            out.is_some(),
                            cursor.as_ref(),
                            shift_down,
                            &mut result_clicks,
                        );
                    });
                });
            });
            ui.separator();
            // Storage rows (array slots 9..36).
            for row in 0..(INVENTORY_ROWS - 1) {
                ui.horizontal(|ui| {
                    for col in 0..INVENTORY_COLS {
                        let idx = HOTBAR_SLOTS + row * INVENTORY_COLS + col;
                        let kind = SlotKind::Inv(idx);
                        let slot = inv.get(idx).and_then(|s| s.as_ref());
                        let resp = draw_stack_slot(
                            ui,
                            slot,
                            36.0,
                            false,
                            atlas_tex_id,
                            icon_uv_resolver,
                        );
                        track_slot(TrackCtx {
                            drag: &mut drag,
                            clicks: &mut clicks,
                            resp,
                            kind,
                            slot,
                            held: cursor.as_ref(),
                            lmb_pressed,
                            lmb_down,
                            rmb_pressed,
                            rmb_down,
                            shift_down,
                            ui,
                        });
                    }
                });
            }
            ui.add_space(8.0);
            // Hotbar mirror (array slots 0..9). Clicking picks the
            // stack up (MC) - selection changes stay on the HUD
            // hotbar / number keys outside screens.
            ui.horizontal(|ui| {
                for idx in 0..HOTBAR_SLOTS.min(inv.len()) {
                    let kind = SlotKind::Inv(idx);
                    let slot = inv.get(idx).and_then(|s| s.as_ref());
                    let resp = draw_stack_slot(
                        ui,
                        slot,
                        36.0,
                        idx == selected_slot,
                        atlas_tex_id,
                        icon_uv_resolver,
                    );
                    track_slot(TrackCtx {
                        drag: &mut drag,
                        clicks: &mut clicks,
                        resp,
                        kind,
                        slot,
                        held: cursor.as_ref(),
                        lmb_pressed,
                        lmb_down,
                        rmb_pressed,
                        rmb_down,
                        shift_down,
                        ui,
                    });
                }
            });
        });

    // ---- Drag release (QUICK_CRAFT phase 2). -------------------
    let mut drag_committed = false;
    let finish_drag = |slots: &[SlotKind], one_per: bool| InvDragMsg {
        one_per_slot: one_per,
        slots: slots.iter().map(|&k| k.into()).collect(),
    };
    if lmb_released
        && drag.button == Some(DragButton::Left)
        && drag.slots.len() >= 2
    {
        crate::inventory::inv_screen_apply_drag(
            craft, armor, inv, cursor, &drag.slots, false,
        );
        *drag_out = Some(finish_drag(&drag.slots, false));
        drag_committed = true;
    } else if rmb_released
        && drag.button == Some(DragButton::Right)
        && drag.slots.len() >= 2
    {
        crate::inventory::inv_screen_apply_drag(
            craft, armor, inv, cursor, &drag.slots, true,
        );
        *drag_out = Some(finish_drag(&drag.slots, true));
        drag_committed = true;
    }
    if (lmb_released && drag.button == Some(DragButton::Left))
        || (rmb_released && drag.button == Some(DragButton::Right))
    {
        drag = DragState::default();
    }

    // ---- Apply clicks through the shared core. -----------------
    if !drag_committed {
        for kind in result_clicks {
            if let Some(op) = op_for(SlotKind::CraftOut, kind) {
                thrown_out.extend(crate::inventory::inv_screen_apply_op(
                    craft, armor, inv, cursor, op, creative,
                ));
                ops_out.push(op);
            }
        }
        for (kind, click) in clicks {
            if let Some(op) = op_for(kind, click) {
                thrown_out.extend(crate::inventory::inv_screen_apply_op(
                    craft, armor, inv, cursor, op, creative,
                ));
                ops_out.push(op);
            }
        }
    }
    ctx.memory_mut(|m| m.data.insert_temp(drag_id, drag));

    // ---- Floating cursor sprite. -------------------------------
    if let (Some(pos), Some(st)) = (cursor_pos, cursor.as_ref()) {
        paint_cursor(ctx, pos, st, atlas_tex_id, icon_uv_resolver);
    }
}

/// Map a UI click intent to the wire-shaped op the shared core
/// consumes.
fn op_for(kind: SlotKind, click: ClickKind) -> Option<InvOpMsg> {
    let slot: NetSlot = kind.into();
    Some(match click {
        ClickKind::Lmb => InvOpMsg::Click { slot, secondary: false },
        ClickKind::Rmb => InvOpMsg::Click { slot, secondary: true },
        ClickKind::ShiftLmb => InvOpMsg::QuickMove { slot },
        ClickKind::Collect => InvOpMsg::Collect { slot },
        ClickKind::SwapHotbar(h) => InvOpMsg::Swap { slot, hotbar: h },
        ClickKind::ThrowOne => InvOpMsg::Throw { slot, all: false },
        ClickKind::ThrowAll => InvOpMsg::Throw { slot, all: true },
        ClickKind::CloneStack => InvOpMsg::CloneSlot { slot },
    })
}

/// Per-slot tracking inputs (struct per the 5+ args rule).
struct TrackCtx<'a> {
    drag: &'a mut DragState,
    clicks: &'a mut Vec<(SlotKind, ClickKind)>,
    resp: egui::Response,
    kind: SlotKind,
    slot: Option<&'a ItemStack>,
    held: Option<&'a ItemStack>,
    lmb_pressed: bool,
    lmb_down: bool,
    rmb_pressed: bool,
    rmb_down: bool,
    shift_down: bool,
    ui: &'a egui::Ui,
}

fn track_slot(t: TrackCtx<'_>) {
    let TrackCtx {
        drag,
        clicks,
        resp,
        kind,
        slot,
        held,
        lmb_pressed,
        lmb_down,
        rmb_pressed,
        rmb_down,
        shift_down,
        ui,
    } = t;
    // Drag-paint visit tracking (`contains_pointer`, not
    // `hovered` - the W54.4 lesson).
    let pointer_over = resp.contains_pointer();
    if let Some(cur) = held
        && pointer_over
    {
        // Add-slot eligibility (source lines 354-358): quick-
        // replaceable + slot accepts + cursor count > visited.
        let accept_ok = match kind {
            SlotKind::Armor(a) => crate::inventory::ARMOR_ACCEPT[a]
                .map(|pred| pred(cur.id))
                .unwrap_or(true),
            _ => true,
        };
        let eligible = crate::container::click::can_add_drag_slot(
            slot,
            accept_ok,
            cur,
            drag.slots.len(),
        );
        if lmb_down && drag.button != Some(DragButton::Right) {
            if drag.button.is_none() && lmb_pressed {
                drag.button = Some(DragButton::Left);
            }
            if drag.button == Some(DragButton::Left)
                && eligible
                && !drag.slots.contains(&kind)
            {
                drag.slots.push(kind);
            }
        } else if rmb_down && drag.button != Some(DragButton::Left) {
            if drag.button.is_none() && rmb_pressed {
                drag.button = Some(DragButton::Right);
            }
            if drag.button == Some(DragButton::Right)
                && eligible
                && !drag.slots.contains(&kind)
            {
                drag.slots.push(kind);
            }
        }
    }
    // Hovered-slot keyboard ops.
    if resp.hovered() && !ui.ctx().wants_keyboard_input() {
        ui.input(|i| {
            const DIGITS: [egui::Key; 9] = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ];
            for (n, key) in DIGITS.iter().enumerate() {
                if i.key_pressed(*key) {
                    clicks.push((kind, ClickKind::SwapHotbar(n as u8)));
                    break;
                }
            }
            if slot.is_some() && i.key_pressed(egui::Key::Q) {
                clicks.push((
                    kind,
                    if i.modifiers.command {
                        ClickKind::ThrowAll
                    } else {
                        ClickKind::ThrowOne
                    },
                ));
            }
        });
    }
    // Click intents. Double-click first (the second click of a
    // double also reports `clicked()`).
    if resp.double_clicked() {
        if held.is_some() {
            clicks.push((kind, ClickKind::Collect));
        }
    } else if resp.clicked() {
        clicks.push((
            kind,
            if shift_down {
                ClickKind::ShiftLmb
            } else {
                ClickKind::Lmb
            },
        ));
    } else if resp.secondary_clicked() {
        clicks.push((kind, ClickKind::Rmb));
    } else if resp.middle_clicked() && slot.is_some() {
        clicks.push((kind, ClickKind::CloneStack));
    }
}

/// Result-slot intent tracking (take-only virtual cell): LMB / RMB
/// take, shift quick-moves, number keys craft into an empty hotbar
/// slot, Q throws. No drag visits, no collect, clone allowed.
fn track_result_slot(
    ui: &egui::Ui,
    resp: &egui::Response,
    has_out: bool,
    held: Option<&ItemStack>,
    shift_down: bool,
    result_clicks: &mut Vec<ClickKind>,
) {
    if !has_out {
        return;
    }
    if resp.hovered() && !ui.ctx().wants_keyboard_input() {
        ui.input(|i| {
            const DIGITS: [egui::Key; 9] = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ];
            for (n, key) in DIGITS.iter().enumerate() {
                if i.key_pressed(*key) {
                    result_clicks.push(ClickKind::SwapHotbar(n as u8));
                    break;
                }
            }
            if i.key_pressed(egui::Key::Q) {
                result_clicks.push(if i.modifiers.command {
                    ClickKind::ThrowAll
                } else {
                    ClickKind::ThrowOne
                });
            }
        });
    }
    if resp.double_clicked() {
        // PICKUP_ALL on the matching result slot is blocked by the
        // guard (slot holds a pickable item); the plain click of
        // the double already took one craft.
        if held.is_none() {
            result_clicks.push(ClickKind::Lmb);
        }
    } else if resp.clicked() {
        result_clicks.push(if shift_down {
            ClickKind::ShiftLmb
        } else {
            ClickKind::Lmb
        });
    } else if resp.secondary_clicked() {
        result_clicks.push(ClickKind::Rmb);
    } else if resp.middle_clicked() {
        result_clicks.push(ClickKind::CloneStack);
    }
}

// ---- slot painting -----------------------------------------

/// Paint one slot from an optional stack: icon, count badge,
/// durability bar (13-step MC gradient), selected outline.
fn draw_stack_slot(
    ui: &mut egui::Ui,
    slot: Option<&ItemStack>,
    tile_px: f32,
    selected: bool,
    atlas: Option<egui::TextureId>,
    uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
) -> egui::Response {
    let size = egui::vec2(tile_px, tile_px);
    let (rect, resp) =
        ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(24));
    if let Some(st) = slot {
        paint_stack(&painter, rect, st, atlas, uv_resolver);
    }
    let outline = if selected {
        egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 220, 80))
    } else if resp.hovered() {
        egui::Stroke::new(2.0, egui::Color32::from_gray(200))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(120))
    };
    painter.rect_stroke(rect, 4.0, outline, egui::StrokeKind::Inside);
    if let Some(st) = slot {
        return resp.on_hover_text(crate::item::item_display_name(st.id));
    }
    resp
}

/// Icon + count badge + durability bar for one stack.
fn paint_stack(
    painter: &egui::Painter,
    rect: egui::Rect,
    st: &ItemStack,
    atlas: Option<egui::TextureId>,
    uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
) {
    match (atlas, uv_resolver(st.id)) {
        (Some(tex), Some(uv)) => {
            let uv_rect = egui::Rect::from_min_max(
                egui::pos2(uv[0], uv[1]),
                egui::pos2(uv[2], uv[3]),
            );
            painter.image(
                tex,
                rect.shrink(2.0),
                uv_rect,
                egui::Color32::WHITE,
            );
        }
        _ => {
            let r = (st.id.wrapping_mul(73) & 0xFF) as u8;
            let g = (st.id.wrapping_mul(151) & 0xFF) as u8;
            let b = (st.id.wrapping_mul(211) & 0xFF) as u8;
            painter.rect_filled(
                rect.shrink(2.0),
                2.0,
                egui::Color32::from_rgb(r, g, b),
            );
        }
    }
    if st.count > 1 {
        let pos = egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0);
        painter.text(
            pos + egui::vec2(1.0, 1.0),
            egui::Align2::RIGHT_BOTTOM,
            st.count.to_string(),
            egui::FontId::proportional(14.0),
            egui::Color32::BLACK,
        );
        painter.text(
            pos,
            egui::Align2::RIGHT_BOTTOM,
            st.count.to_string(),
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        );
    }
    // Durability bar (W471 rules: hidden until damaged; 13-step
    // width; red->yellow->green hue sweep).
    if st.components.damage > 0
        && let Some(max) = crate::item::max_damage_for(st.id)
    {
        let cur = max.saturating_sub(st.components.damage);
        let frac = (cur as f32 / max as f32).clamp(0.0, 1.0);
        let width_frac = (frac * 13.0).round() / 13.0;
        let bar_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, rect.max.y - 8.0),
            egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0),
        );
        painter.rect_filled(bar_rect, 1.0, egui::Color32::from_gray(20));
        let r = if frac < 0.5 {
            255
        } else {
            (255.0 * (1.0 - frac) * 2.0) as u8
        };
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
}

/// Paint the held stack as a floating sprite at the mouse, above
/// every window.
fn paint_cursor(
    ctx: &egui::Context,
    pos: egui::Pos2,
    st: &ItemStack,
    atlas: Option<egui::TextureId>,
    uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
) {
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("inventory_screen_cursor"),
    ));
    let rect = egui::Rect::from_center_size(pos, egui::vec2(32.0, 32.0));
    paint_stack(&layer, rect, st, atlas, uv_resolver);
}

// Keep the INVENTORY_SLOTS import earning its keep (the row loops
// derive from ROWS/COLS).
const _: () = assert!(INVENTORY_SLOTS == INVENTORY_COLS * INVENTORY_ROWS);
