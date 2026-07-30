//! W410 - smithing table panel.
//!
//! Lean session UI in the stonecutter style (no floating cursor):
//! three input slots (template / base / addition, MC SmithingMenu
//! order) + the deterministic result + the player inventory.
//! Interaction:
//! - click an inventory stack -> routes into its slot (smithing
//!   templates -> template, netherite ingot -> addition, anything
//!   else -> base); a prior occupant returns to the inventory;
//! - click a filled input slot -> returns it to the inventory;
//! - click the result -> reports the craft; the caller consumes one
//!   of each input + adds the output.
//!
//! W469: stack-native. The base slot's components matter: the MC
//! netherite upgrade keeps the input's damage + enchants, so the
//! apply pass in `render.rs` clones the BASE stack's components
//! onto the upgrade output.
//!
//! Mutations land on the snapshots (`slots`, `inv_view`);
//! `render.rs` applies them back to `Voxel` after the egui pass.

use crate::item::ItemStack;
use crate::recipes::smithing::{is_smithing_template, smith_result};
use crate::ui::SlotIcon;
use crate::ui::stonecutter_panel::give_to_inv;

pub(crate) struct SmithingPanelInputs<'a> {
    pub(crate) ctx: &'a egui::Context,
    /// [template, base, addition].
    pub(crate) slots: &'a mut [Option<ItemStack>; 3],
    pub(crate) inv_view: &'a mut [Option<ItemStack>],
    pub(crate) atlas_tex_id: Option<egui::TextureId>,
    pub(crate) icon_uv_resolver: &'a dyn Fn(u16) -> Option<[f32; 4]>,
    pub(crate) open_local: &'a mut bool,
    /// Set when the player clicks the result slot.
    pub(crate) craft_out: &'a mut bool,
}

fn slot_icon(
    id: u16,
    count: u8,
    resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
) -> SlotIcon {
    let color = crate::block::block_type(id)
        .map(|b| b.color)
        .unwrap_or([0.7, 0.7, 0.7]);
    let uv = resolver(id).unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let name = crate::item::item_type(id).map(|it| it.name).unwrap_or("");
    (color, count, uv, name)
}

/// Which smithing slot does this item route to? Mirrors the MC menu
/// slot predicates: templates into 0, the addition material into 2,
/// everything else is a base.
fn route_slot(id: u16) -> usize {
    if is_smithing_template(id) {
        0
    } else if crate::recipes::smithing::SMITHING_RECIPES
        .iter()
        .any(|r| r.addition == id)
    {
        2
    } else {
        1
    }
}

pub(crate) fn render(inp: SmithingPanelInputs<'_>) {
    if !*inp.open_local {
        return;
    }
    let SmithingPanelInputs {
        ctx,
        slots,
        inv_view,
        atlas_tex_id,
        icon_uv_resolver,
        open_local,
        craft_out,
    } = inp;

    let mut open = true;
    egui::Window::new("Smithing Table")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            // --- input slots ------------------------------------
            ui.horizontal(|ui| {
                for (i, label) in
                    ["Template:", "Base:", "Addition:"].iter().enumerate()
                {
                    ui.label(*label);
                    let icon = slots[i].as_ref().map(|st| {
                        slot_icon(st.id, st.count, icon_uv_resolver)
                    });
                    let resp = crate::ui::slot_widget::draw_slot_widget(
                        ui,
                        icon,
                        atlas_tex_id,
                        false,
                        egui::Sense::click(),
                    );
                    if resp.clicked()
                        && let Some(stack) = slots[i].take()
                    {
                        give_to_inv(inv_view, stack);
                    }
                }
            });

            ui.separator();

            // --- result -----------------------------------------
            ui.horizontal(|ui| {
                ui.label("Result:");
                let result = smith_result(
                    slots[0].as_ref().map(|st| st.id),
                    slots[1].as_ref().map(|st| st.id),
                    slots[2].as_ref().map(|st| st.id),
                );
                let icon = result
                    .map(|r| slot_icon(r.output, 1, icon_uv_resolver));
                let resp = crate::ui::slot_widget::draw_slot_widget(
                    ui,
                    icon,
                    atlas_tex_id,
                    false,
                    egui::Sense::click(),
                );
                if let Some(r) = result {
                    if resp.on_hover_text(r.name).clicked() {
                        *craft_out = true;
                    }
                } else if slots.iter().any(|s| s.is_some()) {
                    ui.label("No matching upgrade.");
                }
            });

            ui.separator();

            // --- player inventory -------------------------------
            ui.label("Inventory:");
            egui::Grid::new("smithing_inv")
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    for i in 0..inv_view.len() {
                        let icon = inv_view[i].as_ref().map(|st| {
                            slot_icon(st.id, st.count, icon_uv_resolver)
                        });
                        let resp = crate::ui::slot_widget::draw_slot_widget(
                            ui,
                            icon,
                            atlas_tex_id,
                            false,
                            egui::Sense::click(),
                        );
                        if resp.clicked()
                            && let Some(id) =
                                inv_view[i].as_ref().map(|st| st.id)
                        {
                            let si = route_slot(id);
                            let prior = slots[si].take();
                            slots[si] = inv_view[i].take();
                            if let Some(p) = prior {
                                give_to_inv(inv_view, p);
                            }
                        }
                        if (i + 1) % 9 == 0 {
                            ui.end_row();
                        }
                    }
                });
        });

    if !open {
        *open_local = false;
    }
}
