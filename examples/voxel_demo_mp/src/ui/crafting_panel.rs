//! W50.3 - 3x3 crafting-table panel.
//!
//! Renders the egui::Window for the crafting_table block. The
//! 3x3 grid lives on `Voxel::craft_grid_3x3`; the panel handles
//! LMB / RMB / shift-click against it, computes the output via
//! `recipes::matcher::match_3x3`, and reports back to the caller
//! whether the player clicked the output slot (so the caller
//! can consume inputs + pop the result into the held stack).
//!
//! Render.rs owns the surrounding snapshot+apply dance to keep
//! the borrow checker happy; the heavy lifting (slot grid +
//! output preview + player inv mirror) lives here so render.rs
//! doesn't grow further.

use infengine_voxel::BlockId;

use crate::container;
use crate::recipes::matcher;

/// All inputs the 3x3 panel needs in one struct so render.rs
/// doesn't have to grow another 10-arg call site.
pub(crate) struct CraftingPanelInputs<'a> {
    pub(crate) ctx: &'a egui::Context,
    pub(crate) grid: &'a mut [Option<(BlockId, u8)>; 9],
    pub(crate) held: &'a mut Option<(u16, u8)>,
    /// Mutable view of the player's inventory in u16 form. The
    /// panel mutates this in place when the player clicks one of
    /// the player-inventory slots below the 3x3 grid. Render.rs
    /// copies the result back into `vx.inventory` after the egui
    /// closure exits.
    pub(crate) inv_view: &'a mut [Option<(u16, u8)>],
    pub(crate) atlas_tex_id: Option<egui::TextureId>,
    pub(crate) icon_uv_resolver: &'a dyn Fn(u16) -> Option<[f32; 4]>,
    pub(crate) open_local: &'a mut bool,
    /// Set to true when the player clicks the output slot. The
    /// caller is responsible for decrementing each non-empty
    /// `grid` cell by 1 and pushing the resolved output stack
    /// into the held stack.
    pub(crate) take_output_out: &'a mut bool,
}

/// Draw the 3x3 crafting table window. Returns immediately when
/// the panel is closed (caller passes `open_local: false`).
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

    let screen = ctx.content_rect();
    egui::Window::new("Crafting Table")
        .open(open_local)
        .collapsible(false)
        .resizable(false)
        .default_pos([
            screen.center().x - 220.0,
            screen.center().y - 260.0,
        ])
        .default_size([440.0, 520.0])
        .show(ctx, |ui| {
            ui.label(
                "ESC to close - LMB take/place, RMB half/one. Shift+LMB unsupported in 3x3 (W50.3b)",
            );
            if let Some((id, n)) = *held {
                ui.label(format!(
                    "Holding: item {} x{}",
                    crate::item::item_display_name(id),
                    n,
                ));
            }
            ui.separator();

            // 3x3 grid view rendered through the generic widget.
            let mut grid_view = container::Container::with_size(9);
            for (i, s) in grid.iter().enumerate() {
                grid_view.set_slot(i, s.map(|(b, c)| (b.0, c)));
            }
            ui.horizontal(|ui| {
                let cfg = container::ui::GridConfig {
                    cols: 3,
                    rows: 3,
                    tile_px: 36.0,
                    title: None,
                };
                let actions = container::ui::draw_grid_with_uv(
                    ui,
                    &cfg,
                    &grid_view,
                    *held,
                    atlas_tex_id,
                    icon_uv_resolver,
                );
                for a in actions {
                    apply_action_to_grid(grid, held, a);
                }

                // Arrow + output slot.
                ui.label("  ->  ");
                let mut out_view = container::Container::with_size(1);
                let match_result =
                    matcher::match_3x3(&grid_as_u16(grid));
                if let Some((id, n, _name)) = match_result {
                    out_view.set_slot(0, Some((id, n)));
                }
                let cfg_out = container::ui::GridConfig {
                    cols: 1,
                    rows: 1,
                    tile_px: 36.0,
                    title: None,
                };
                let out_actions = container::ui::draw_grid_with_uv(
                    ui,
                    &cfg_out,
                    &out_view,
                    *held,
                    atlas_tex_id,
                    icon_uv_resolver,
                );
                for a in out_actions {
                    // Any click on the output slot consumes one
                    // copy of every non-empty grid cell + pushes
                    // the output onto the held stack. RMB / shift
                    // behave the same here; MC's "craft all"
                    // (Shift+LMB) belongs to a follow-up wave.
                    use container::ui::GridAction as A;
                    if matches!(
                        a,
                        A::Take(_)
                            | A::Place(_)
                            | A::TakeHalf(_)
                            | A::PlaceOne(_)
                            | A::Transfer(_)
                    ) && match_result.is_some()
                    {
                        *take_output_out = true;
                    }
                }
            });

            ui.separator();
            ui.label("Inventory (36)");
            let mut iv_container =
                container::Container::with_size(inv_view.len());
            for (i, s) in inv_view.iter().enumerate() {
                iv_container.set_slot(i, *s);
            }
            let inv_cfg = container::ui::GridConfig {
                cols: 9,
                rows: 4,
                tile_px: 36.0,
                title: None,
            };
            let inv_actions_g = container::ui::draw_grid_with_uv(
                ui,
                &inv_cfg,
                &iv_container,
                *held,
                atlas_tex_id,
                icon_uv_resolver,
            );
            for a in inv_actions_g {
                apply_action_to_inv(inv_view, held, a);
            }
        });
}

/// Convert the BlockId-typed grid to plain u16 cells for the
/// matcher (which is BlockId-agnostic).
fn grid_as_u16(g: &[Option<(BlockId, u8)>; 9]) -> [Option<(u16, u8)>; 9] {
    std::array::from_fn(|i| g[i].map(|(b, c)| (b.0, c)))
}

/// Mutate `grid` + `held` from a single GridAction. Mirrors
/// `inventory::inv_apply_drag` but specialised for the 3x3.
fn apply_action_to_grid(
    grid: &mut [Option<(BlockId, u8)>; 9],
    held: &mut Option<(u16, u8)>,
    action: container::ui::GridAction,
) {
    use container::ui::GridAction as A;
    let stack_max = crate::inventory::STACK_MAX;
    match action {
        A::Take(i) => {
            if let Some(slot) = grid.get_mut(i)
                && held.is_none()
                && let Some((b, c)) = slot.take()
            {
                *held = Some((b.0, c));
            }
        }
        A::TakeHalf(i) => {
            if let Some(slot) = grid.get_mut(i)
                && held.is_none()
                && let Some((b, c)) = *slot
            {
                let half = (c as u16).div_ceil(2) as u8;
                let rem = c - half;
                if half > 0 {
                    *held = Some((b.0, half));
                }
                *slot = if rem == 0 { None } else { Some((b, rem)) };
            }
        }
        A::Place(i) => {
            if let Some(slot) = grid.get_mut(i)
                && let Some((hid, hn)) = *held
            {
                match *slot {
                    None => {
                        *slot = Some((BlockId(hid), hn));
                        *held = None;
                    }
                    Some((b, c)) if b.0 == hid => {
                        let room = stack_max.saturating_sub(c);
                        let move_n = room.min(hn);
                        *slot = Some((b, c + move_n));
                        let left = hn - move_n;
                        *held = if left == 0 {
                            None
                        } else {
                            Some((hid, left))
                        };
                    }
                    Some((b, c)) => {
                        *slot = Some((BlockId(hid), hn));
                        *held = Some((b.0, c));
                    }
                }
            }
        }
        A::PlaceOne(i) => {
            if let Some(slot) = grid.get_mut(i)
                && let Some((hid, hn)) = *held
                && hn > 0
            {
                match *slot {
                    None => {
                        *slot = Some((BlockId(hid), 1));
                        let left = hn - 1;
                        *held = if left == 0 { None } else { Some((hid, left)) };
                    }
                    Some((b, c)) if b.0 == hid && c < stack_max => {
                        *slot = Some((b, c + 1));
                        let left = hn - 1;
                        *held = if left == 0 { None } else { Some((hid, left)) };
                    }
                    _ => {}
                }
            }
        }
        A::Transfer(_) => {
            // W50.3b: cross-container transfer deferred.
        }
    }
}

/// Same shape as `apply_action_to_grid` but for the u16-typed
/// player inventory view. Used by the player-inv grid that sits
/// below the 3x3.
fn apply_action_to_inv(
    inv: &mut [Option<(u16, u8)>],
    held: &mut Option<(u16, u8)>,
    action: container::ui::GridAction,
) {
    use container::ui::GridAction as A;
    let stack_max = crate::inventory::STACK_MAX;
    match action {
        A::Take(i) => {
            if let Some(slot) = inv.get_mut(i)
                && held.is_none()
                && let Some(s) = slot.take()
            {
                *held = Some(s);
            }
        }
        A::TakeHalf(i) => {
            if let Some(slot) = inv.get_mut(i)
                && held.is_none()
                && let Some((id, c)) = *slot
            {
                let half = (c as u16).div_ceil(2) as u8;
                let rem = c - half;
                if half > 0 {
                    *held = Some((id, half));
                }
                *slot = if rem == 0 { None } else { Some((id, rem)) };
            }
        }
        A::Place(i) => {
            if let Some(slot) = inv.get_mut(i)
                && let Some((hid, hn)) = *held
            {
                match *slot {
                    None => {
                        *slot = Some((hid, hn));
                        *held = None;
                    }
                    Some((sid, sc)) if sid == hid => {
                        let room = stack_max.saturating_sub(sc);
                        let move_n = room.min(hn);
                        *slot = Some((sid, sc + move_n));
                        let left = hn - move_n;
                        *held = if left == 0 { None } else { Some((hid, left)) };
                    }
                    Some(s) => {
                        *slot = Some((hid, hn));
                        *held = Some(s);
                    }
                }
            }
        }
        A::PlaceOne(i) => {
            if let Some(slot) = inv.get_mut(i)
                && let Some((hid, hn)) = *held
                && hn > 0
            {
                match *slot {
                    None => {
                        *slot = Some((hid, 1));
                        let left = hn - 1;
                        *held = if left == 0 { None } else { Some((hid, left)) };
                    }
                    Some((sid, sc)) if sid == hid && sc < stack_max => {
                        *slot = Some((sid, sc + 1));
                        let left = hn - 1;
                        *held = if left == 0 { None } else { Some((hid, left)) };
                    }
                    _ => {}
                }
            }
        }
        A::Transfer(_) => {
            // W50.3b deferred.
        }
    }
}
