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
pub(crate) fn draw_grid(
    ui: &mut egui::Ui,
    cfg: &GridConfig,
    container: &Container,
    held: Option<(u16, u8)>,
    atlas: Option<egui::TextureId>,
) -> Vec<GridAction> {
    draw_grid_with_uv(ui, cfg, container, held, atlas, &|_| None)
}

/// W9.3: same as `draw_grid` but the caller plumbs a per-item UV
/// resolver (typically powered by the loaded MC resource pack)
/// so non-block items get their dedicated MC icon instead of the
/// shared placeholder tile at the (0, 0) atlas slot.
pub(crate) fn draw_grid_with_uv(
    ui: &mut egui::Ui,
    cfg: &GridConfig,
    container: &Container,
    held: Option<(u16, u8)>,
    atlas: Option<egui::TextureId>,
    uv_resolver: UvResolver<'_>,
) -> Vec<GridAction> {
    let mut actions: Vec<GridAction> = Vec::new();
    if let Some(title) = &cfg.title {
        ui.label(title.as_str());
    }
    let total = cfg.cols * cfg.rows;
    // Grid id: derive from the title when set, otherwise fall back
    // to the slot rectangle dims so two side-by-side default grids
    // (chest + player inv) still get distinct ids.
    let grid_id_src = (
        cfg.title.as_deref().unwrap_or("container_grid"),
        cfg.cols,
        cfg.rows,
    );
    egui::Grid::new(grid_id_src)
        .spacing(egui::vec2(2.0, 2.0))
        .show(ui, |ui| {
            for idx in 0..total {
                let slot = container.slot(idx);
                let resp = draw_one_slot(ui, slot, cfg.tile_px, atlas, uv_resolver);
                // Modifier mapping. We sample once per slot so a
                // Shift-LMB and a plain LMB on the same slot don't
                // both fire.
                let (shift, _ctrl) =
                    ui.input(|i| (i.modifiers.shift, i.modifiers.command));
                let is_lmb = resp.clicked();
                let is_rmb = resp.secondary_clicked();
                if is_lmb {
                    if shift && slot.is_some() {
                        actions.push(GridAction::Transfer(idx));
                    } else if held.is_some() {
                        actions.push(GridAction::Place(idx));
                    } else if slot.is_some() {
                        actions.push(GridAction::Take(idx));
                    }
                } else if is_rmb {
                    if held.is_some() {
                        actions.push(GridAction::PlaceOne(idx));
                    } else if slot.is_some() {
                        actions.push(GridAction::TakeHalf(idx));
                    }
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
    slot: super::Slot,
    tile_px: f32,
    atlas: Option<egui::TextureId>,
    uv_resolver: UvResolver<'_>,
) -> egui::Response {
    let size = egui::vec2(tile_px, tile_px);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(24));
    if let Some((id, count)) = slot {
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
    resp
}

