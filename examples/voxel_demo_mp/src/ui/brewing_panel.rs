//! W50.5 - brewing stand UI panel.
//!
//! Renders the egui::Window for the brewing_stand block:
//! ingredient slot + fuel slot + 3 base bottle slots + a
//! horizontal progress bar showing the current brew cycle. The
//! player inventory mirror below routes player-side clicks
//! through `inv_actions_out` so render.rs can apply them.
//!
//! Mirrors the structure of the chest / furnace panels: the
//! caller owns the snapshot+apply dance; this module just paints
//! the widgets and records click actions.

use crate::block::brewing::{
    BREW_MS_PER_OUTPUT, BREWING_SLOTS, BrewingRuntime, SLOT_BOTTLE_0,
    SLOT_BOTTLE_1, SLOT_BOTTLE_2, SLOT_FUEL, SLOT_INGREDIENT,
};
use crate::container;

/// Inputs the brewing panel needs from render.rs.
pub(crate) struct BrewingPanelInputs<'a> {
    pub(crate) ctx: &'a egui::Context,
    pub(crate) stand_snapshot: &'a container::Container,
    pub(crate) runtime_snapshot: BrewingRuntime,
    pub(crate) inv_plain_snapshot: &'a [Option<(u16, u8)>],
    pub(crate) atlas_tex_id: Option<egui::TextureId>,
    pub(crate) icon_uv_resolver: &'a dyn Fn(u16) -> Option<[f32; 4]>,
    pub(crate) open_local: &'a mut bool,
    pub(crate) held_local: Option<(u16, u8)>,
    /// Click actions emitted by the stand grid. First field is
    /// false (stand-side); render.rs applies them against the
    /// real `Voxel::brewing_stands` entry.
    pub(crate) stand_actions_out: &'a mut Vec<(bool, container::ui::GridAction)>,
}

pub(crate) fn render(inputs: BrewingPanelInputs<'_>) {
    if !*inputs.open_local {
        return;
    }
    let BrewingPanelInputs {
        ctx,
        stand_snapshot,
        runtime_snapshot,
        inv_plain_snapshot,
        atlas_tex_id,
        icon_uv_resolver,
        open_local,
        held_local,
        stand_actions_out,
    } = inputs;

    let screen = ctx.content_rect();
    egui::Window::new("Brewing Stand")
        .open(open_local)
        .collapsible(false)
        .resizable(false)
        .default_pos([
            screen.center().x - 220.0,
            screen.center().y - 240.0,
        ])
        .default_size([440.0, 460.0])
        .show(ctx, |ui| {
            ui.label(
                "ESC to close - LMB take/place, RMB half/one, Shift+LMB transfer",
            );
            if let Some((id, n)) = held_local {
                ui.label(format!(
                    "Holding: item {} x{}",
                    crate::item::item_display_name(id),
                    n,
                ));
            }
            ui.separator();

            // Header strip: ingredient slot + fuel slot + brew
            // progress bar + 3 bottle slots.
            ui.horizontal(|ui| {
                ui.label("Ingredient");
                draw_single(
                    ui,
                    stand_snapshot,
                    SLOT_INGREDIENT,
                    held_local,
                    atlas_tex_id,
                    icon_uv_resolver,
                    stand_actions_out,
                );
                ui.label("Fuel");
                draw_single(
                    ui,
                    stand_snapshot,
                    SLOT_FUEL,
                    held_local,
                    atlas_tex_id,
                    icon_uv_resolver,
                    stand_actions_out,
                );
                ui.label("Brew");
                draw_progress_bar(ui, runtime_snapshot.progress_ms);
                for slot in [SLOT_BOTTLE_0, SLOT_BOTTLE_1, SLOT_BOTTLE_2] {
                    draw_single(
                        ui,
                        stand_snapshot,
                        slot,
                        held_local,
                        atlas_tex_id,
                        icon_uv_resolver,
                        stand_actions_out,
                    );
                }
            });

            ui.separator();
            ui.label("Inventory (36)");
            // Mirror of the player inventory. Clicks routed
            // through stand_actions_out tagged as player_side.
            let mut inv_view =
                container::Container::with_size(inv_plain_snapshot.len());
            for (i, s) in inv_plain_snapshot.iter().enumerate() {
                inv_view.set_slot(i, *s);
            }
            let inv_cfg = container::ui::GridConfig {
                cols: 9,
                rows: 4,
                tile_px: 36.0,
                title: None,
            };
            let inv_actions = container::ui::draw_grid_with_uv(
                ui,
                &inv_cfg,
                &inv_view,
                held_local,
                atlas_tex_id,
                icon_uv_resolver,
            );
            for a in inv_actions {
                stand_actions_out.push((true, a));
            }
            // Keep the consts public so this file documents the
            // wire shape clearly even when render.rs upgrades.
            let _ = BREWING_SLOTS;
        });
}

fn draw_single(
    ui: &mut egui::Ui,
    stand: &container::Container,
    slot_idx: usize,
    held: Option<(u16, u8)>,
    atlas_tex_id: Option<egui::TextureId>,
    icon_uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
    actions_out: &mut Vec<(bool, container::ui::GridAction)>,
) {
    let cfg = container::ui::GridConfig {
        cols: 1,
        rows: 1,
        tile_px: 36.0,
        title: None,
    };
    let mut single = container::Container::with_size(1);
    single.set_slot(0, stand.slot(slot_idx));
    let acts = container::ui::draw_grid_with_uv(
        ui,
        &cfg,
        &single,
        held,
        atlas_tex_id,
        icon_uv_resolver,
    );
    for a in acts {
        actions_out.push((false, remap_action(a, slot_idx)));
    }
}

/// Remap a single-slot widget's GridAction(0) to the real
/// container slot index. Mirrors furnace_panel's helper.
fn remap_action(
    a: container::ui::GridAction,
    real_idx: usize,
) -> container::ui::GridAction {
    use container::ui::GridAction as A;
    match a {
        A::Take(_) => A::Take(real_idx),
        A::Place(_) => A::Place(real_idx),
        A::TakeHalf(_) => A::TakeHalf(real_idx),
        A::PlaceOne(_) => A::PlaceOne(real_idx),
        A::Transfer(_) => A::Transfer(real_idx),
    }
}

fn draw_progress_bar(ui: &mut egui::Ui, progress_ms: u32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(48.0, 16.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_gray(40));
    let frac = (progress_ms as f32 / BREW_MS_PER_OUTPUT as f32).clamp(0.0, 1.0);
    let fill_w = rect.width() * frac;
    let fill_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.min.x + fill_w, rect.max.y),
    );
    painter.rect_filled(
        fill_rect,
        2.0,
        egui::Color32::from_rgb(180, 100, 220),
    );
}
