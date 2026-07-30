//! egui widget that draws a generic `Container` as a slot grid and
//! turns mouse / keyboard input into `GridAction`s the caller
//! applies against the live container(s).
//!
//! The widget itself owns no state - the cursor-held stack and the
//! mapping of "other container" for shift-click transfer live on
//! the caller. That keeps the same `Container` widget usable for
//! the chest UI (player <-> chest), the furnace UI (player <-> three
//! furnace slots), and the future migrated player inventory.
//!
//! Interaction model (Minecraft-compatible):
//! - LMB on a non-empty slot with no held stack -> `Take(idx)`.
//! - LMB on any slot with a held stack -> `Place(idx)`. The caller
//!   merges / swaps based on whether the destination is empty,
//!   same-item, or different-item.
//! - RMB on a non-empty slot with no held stack -> `TakeHalf(idx)`.
//! - RMB on any slot with a held stack -> `PlaceOne(idx)`.
//! - Shift + LMB -> `Transfer(idx)` (quick-move to the partner
//!   container).
//!
//! `draw_grid` is kept as a free function (parallel to the existing
//! `slot_with_dnd` helper in `main.rs`). When C.3 wires the chest
//! UI we'll have a clearer picture of any shared per-frame state
//! worth pulling into a struct; for now a free fn keeps the API
//! surface flat and trivially compose-able from multiple call sites.

use super::Container;
use crate::item::ItemStack;
use crate::item::icons::{icon_for, icon_uv_rect};

/// W9.3: per-item UV resolver. Callers pass a closure that turns an
/// item id into the atlas UV rect to paint (typically by routing
/// through the loaded `ResourcePack::icon_ref`). Returning `None`
/// falls back to the legacy 4x4-atlas icon table.
pub(crate) type UvResolver<'a> = &'a dyn Fn(u16) -> Option<[f32; 4]>;

/// One slot-interaction emitted by `draw_grid`. The caller applies
/// these against the live `Container`(s) via `Container::take` /
/// `insert_at` / `transfer_from`.
///
/// The widget is "dumb": it does NOT mutate the container, and it
/// does NOT decide what "transfer" means (the caller knows which
/// other container is the destination).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridAction {
    /// Pick up the whole stack at `slot_idx`. Becomes the held
    /// stack on the cursor.
    Take(usize),
    /// Drop the held stack into `slot_idx` (or merge if it fits).
    Place(usize),
    /// Take half of `slot_idx`. RMB semantics on a non-empty slot.
    TakeHalf(usize),
    /// Place one item from the held stack into `slot_idx`. RMB
    /// semantics while holding a stack.
    PlaceOne(usize),
    /// Shift-click: transfer entire stack to the partner container.
    Transfer(usize),
    /// W470: double-click with a cursor stack - collect every
    /// same-stackable stack on the screen (MC PICKUP_ALL).
    Collect(usize),
    /// W470: number key 1-9 over a hovered slot - swap it with the
    /// matching hotbar slot (MC SWAP). Second field = hotbar index
    /// 0..8.
    SwapHotbar(usize, u8),
    /// W470: Q over a hovered slot with an empty cursor - throw
    /// one item (MC THROW, button 0).
    ThrowOne(usize),
    /// W470: Ctrl+Q - throw the hovered slot's whole stack (MC
    /// THROW, button 1).
    ThrowAll(usize),
    /// W470: creative middle-click - cursor receives a full
    /// max-stack copy (MC CLONE). Local-only (never sent on the
    /// wire; the host cannot validate a remote creative flag).
    CloneSlot(usize),
}

/// W470: which mouse button an in-flight drag-paint uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragButton {
    Left,
    Right,
}

/// W470: panel-wide drag-paint tracker (MC QUICK_CRAFT phases 0+1).
/// The caller persists it across frames (egui temp memory) and
/// hands it to every `draw_grid_with_uv` call of the SAME panel so
/// a drag can cross sections (container grid <-> player inv). On
/// release the caller converts `slots` into one drag-apply op -
/// nothing mutates while the button is down (apply-on-release, as
/// the source does).
#[derive(Debug, Clone, Default)]
pub(crate) struct PanelDrag {
    pub(crate) button: Option<DragButton>,
    /// Visit order of (player_side, slot_idx); deduped. Slots are
    /// only added when eligible at hover time (source lines
    /// 354-358: quick-replaceable + accepted + cursor count >
    /// visited count).
    pub(crate) slots: Vec<(bool, usize)>,
}

/// W470: per-grid-call interaction context for
/// `draw_grid_with_uv`. Identifies which screen section the grid's
/// widget indices belong to and where they start (the furnace /
/// brewing panels draw single-slot strips whose widget index 0 is
/// really container slot `base_index`).
pub(crate) struct GridInteract<'a> {
    pub(crate) drag: &'a mut PanelDrag,
    pub(crate) player_side: bool,
    pub(crate) base_index: usize,
}

/// Layout + cosmetic config for a grid draw call.
pub(crate) struct GridConfig {
    /// Slot columns. The grid is laid out row-major.
    pub(crate) cols: usize,
    /// Slot rows. `cols * rows` should equal `container.len()` for
    /// the rectangle to fully populate; extra slots past the end
    /// of the container render as empty.
    pub(crate) rows: usize,
    /// Pixel size of one slot tile (square).
    pub(crate) tile_px: f32,
    /// Tooltip label prefix shown above the grid. `None` skips the
    /// header row.
    pub(crate) title: Option<String>,
    /// W56.3: unique salt for the egui Grid id. Two grids that
    /// share `(cols, rows)` AND have no title (or the same title)
    /// would otherwise clash. Caller threads a stable per-call
    /// string here; default empty means "no extra salt".
    #[allow(dead_code)] // optional - older callers leave it empty
    pub(crate) id_salt: &'static str,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            cols: 0,
            rows: 0,
            tile_px: 36.0,
            title: None,
            id_salt: "",
        }
    }
}

/// Draw the grid + collect a frame of `GridAction`s.
///
/// `held` is the cursor's currently-held stack; the widget uses it
/// to decide whether a click means "take" or "place". The caller is
/// responsible for actually rendering a held-stack overlay on the
/// cursor (the existing `inventory.rs` DnD path does that already).
///
/// `atlas` is the optional egui texture handle for the shared block
/// atlas. When `None`, slots fall back to a flat coloured swatch
/// so the widget still works without the atlas (e.g. headless
/// tests or pre-asset-load frames).
///
/// W470: `interact` threads the panel-wide drag-paint tracker +
/// this grid's section identity. Beyond the classic click actions
/// the widget now emits the full MC interaction set: double-click
/// `Collect`, number-key `SwapHotbar`, Q / Ctrl+Q `ThrowOne` /
/// `ThrowAll`, creative middle-click `CloneSlot`, and it records
/// drag-paint visits into `interact.drag` (applied on release by
/// the caller - slots are never mutated mid-drag).
pub(crate) fn draw_grid_with_uv(
    ui: &mut egui::Ui,
    cfg: &GridConfig,
    container: &Container,
    held: Option<&ItemStack>,
    atlas: Option<egui::TextureId>,
    uv_resolver: UvResolver<'_>,
    interact: &mut GridInteract<'_>,
) -> Vec<GridAction> {
    let mut actions: Vec<GridAction> = Vec::new();
    if let Some(title) = &cfg.title {
        ui.label(title.as_str());
    }
    let total = cfg.cols * cfg.rows;
    // Pointer + key state sampled once per grid call.
    let (lmb_pressed, lmb_down, rmb_pressed, rmb_down) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.secondary_pressed(),
            i.pointer.secondary_down(),
        )
    });
    // Key ops are suppressed while a text field owns the keyboard
    // (recipe-book search etc).
    let keys_ok = !ui.ctx().wants_keyboard_input();
    // Grid id: derive from the title when set, otherwise fall back
    // to the slot rectangle dims so two side-by-side default grids
    // (chest + player inv) still get distinct ids.
    let grid_id_src = (
        cfg.id_salt,
        cfg.title.as_deref().unwrap_or("container_grid"),
        cfg.cols,
        cfg.rows,
    );
    egui::Grid::new(grid_id_src)
        .spacing(egui::vec2(2.0, 2.0))
        .show(ui, |ui| {
            for idx in 0..total {
                // W467: borrow the stack - the widget only reads
                // id + count, no need to clone components per tile.
                let slot = container.slot_ref(idx);
                let real_idx = interact.base_index + idx;
                let resp = draw_one_slot(ui, slot, cfg.tile_px, atlas, uv_resolver);
                // Modifier mapping. We sample once per slot so a
                // Shift-LMB and a plain LMB on the same slot don't
                // both fire.
                let (shift, _ctrl) =
                    ui.input(|i| (i.modifiers.shift, i.modifiers.command));
                // W470: drag-paint visit tracking. Uses
                // `contains_pointer()` (not `hovered()`) - egui
                // reports hovered=false on widgets the cursor
                // crosses during an active drag (W54.4 lesson).
                if let Some(cur) = held
                    && resp.contains_pointer()
                {
                    let drag = &mut *interact.drag;
                    let sref = (interact.player_side, real_idx);
                    let eligible = crate::container::click::can_add_drag_slot(
                        slot,
                        container.accepts(idx, cur.id),
                        cur,
                        drag.slots.len(),
                    );
                    if lmb_down && drag.button != Some(DragButton::Right) {
                        if drag.button.is_none() && lmb_pressed {
                            drag.button = Some(DragButton::Left);
                        }
                        if drag.button == Some(DragButton::Left)
                            && eligible
                            && !drag.slots.contains(&sref)
                        {
                            drag.slots.push(sref);
                        }
                    } else if rmb_down && drag.button != Some(DragButton::Left) {
                        if drag.button.is_none() && rmb_pressed {
                            drag.button = Some(DragButton::Right);
                        }
                        if drag.button == Some(DragButton::Right)
                            && eligible
                            && !drag.slots.contains(&sref)
                        {
                            drag.slots.push(sref);
                        }
                    }
                }
                // W470: keyboard ops on the hovered slot. Number
                // keys swap with the hotbar; Q / Ctrl+Q throw.
                if keys_ok && resp.hovered() {
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
                                actions.push(GridAction::SwapHotbar(
                                    real_idx, n as u8,
                                ));
                                break;
                            }
                        }
                        if i.key_pressed(egui::Key::Q) && slot.is_some() {
                            actions.push(if i.modifiers.command {
                                GridAction::ThrowAll(real_idx)
                            } else {
                                GridAction::ThrowOne(real_idx)
                            });
                        }
                    });
                }
                // W470: double-click first - on the second click of
                // a double, `clicked()` is ALSO true; MC's flow is
                // click #1 picks the stack up (previous frame),
                // click #2 collects.
                if resp.double_clicked() {
                    if held.is_some() {
                        actions.push(GridAction::Collect(real_idx));
                    }
                } else if resp.clicked() {
                    if shift && slot.is_some() {
                        actions.push(GridAction::Transfer(real_idx));
                    } else if held.is_some() {
                        actions.push(GridAction::Place(real_idx));
                    } else if slot.is_some() {
                        actions.push(GridAction::Take(real_idx));
                    }
                } else if resp.secondary_clicked() {
                    if held.is_some() {
                        actions.push(GridAction::PlaceOne(real_idx));
                    } else if slot.is_some() {
                        actions.push(GridAction::TakeHalf(real_idx));
                    }
                } else if resp.middle_clicked() && slot.is_some() {
                    actions.push(GridAction::CloneSlot(real_idx));
                }
                if (idx + 1) % cfg.cols == 0 {
                    ui.end_row();
                }
            }
        });
    actions
}

/// Render one slot tile + return its `egui::Response` so the caller
/// can read click state. Style mirrors `main::draw_slot_widget` so
/// the chest UI feels consistent with the host's inventory once
/// C.3 swaps it in.
fn draw_one_slot(
    ui: &mut egui::Ui,
    slot: Option<&ItemStack>,
    tile_px: f32,
    atlas: Option<egui::TextureId>,
    uv_resolver: UvResolver<'_>,
) -> egui::Response {
    let size = egui::vec2(tile_px, tile_px);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(24));
    if let Some(st) = slot {
        let (id, count) = (st.id, st.count);
        // W9.3: prefer the resource_pack UV when supplied + non-None,
        // then the legacy 4x4-atlas icon, then the tinted swatch.
        let resolved_uv = uv_resolver(id).or_else(|| icon_for(id).map(icon_uv_rect));
        match (atlas, resolved_uv) {
            (Some(tex_id), Some(uv)) => {
                let uv_rect = egui::Rect::from_min_max(
                    egui::pos2(uv[0], uv[1]),
                    egui::pos2(uv[2], uv[3]),
                );
                painter.image(
                    tex_id,
                    rect.shrink(2.0),
                    uv_rect,
                    egui::Color32::WHITE,
                );
            }
            _ => {
                // Cheap tinted fallback - derive a stable colour
                // from the item id so different ids look distinct
                // even without the atlas. (The real renderer in
                // main.rs uses BlockType::tint; we don't depend on
                // that here so the widget stays item-agnostic.)
                let r = (id.wrapping_mul(73) & 0xFF) as u8;
                let g = (id.wrapping_mul(151) & 0xFF) as u8;
                let b = (id.wrapping_mul(211) & 0xFF) as u8;
                painter.rect_filled(
                    rect.shrink(2.0),
                    2.0,
                    egui::Color32::from_rgb(r, g, b),
                );
            }
        }
        // Stack-count badge in the bottom-right corner. Hide
        // singletons - matches the host's existing inventory.
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
    let outline = if resp.hovered() {
        egui::Stroke::new(2.0, egui::Color32::from_gray(200))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(120))
    };
    painter.rect_stroke(rect, 4.0, outline, egui::StrokeKind::Inside);
    // W364: item-name hover tooltip (R20 request).
    if let Some(st) = slot {
        return resp.on_hover_text(crate::item::item_display_name(st.id));
    }
    resp
}

