//! W171 - stonecutter cut-select panel.
//!
//! A lean container UI (no floating cursor, unlike the 3x3 crafting
//! table): one input slot + a wrap-grid of the cut outputs for that
//! input + the player inventory. Interaction:
//! - click an inventory stack of a cuttable material -> loads it into
//!   the input slot (any prior input returns to the inventory);
//! - click the input slot -> returns it to the inventory;
//! - click a cut output -> reports the chosen index; the caller
//!   consumes 1 input + adds the output to the inventory.
//!
//! All mutations are on the snapshots passed in (`input`, `inv_view`);
//! `render.rs` applies them back to `Voxel` after the egui pass.

use crate::recipes::stonecutter::cuts_for;
use crate::ui::SlotIcon;

pub(crate) struct StonecutterPanelInputs<'a> {
    pub(crate) ctx: &'a egui::Context,
    pub(crate) input: &'a mut Option<(u16, u8)>,
    pub(crate) inv_view: &'a mut [Option<(u16, u8)>],
    pub(crate) atlas_tex_id: Option<egui::TextureId>,
    pub(crate) icon_uv_resolver: &'a dyn Fn(u16) -> Option<[f32; 4]>,
    pub(crate) open_local: &'a mut bool,
    /// Index into `cuts_for(input)` the player clicked, if any.
    pub(crate) craft_out: &'a mut Option<usize>,
}

/// Build a `SlotIcon` for an item id + count (neutral swatch + the
/// resolved atlas UV + the item name as tooltip).
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

pub(crate) fn render(inp: StonecutterPanelInputs<'_>) {
    if !*inp.open_local {
        return;
    }
    let StonecutterPanelInputs {
        ctx,
        input,
        inv_view,
        atlas_tex_id,
        icon_uv_resolver,
        open_local,
        craft_out,
    } = inp;

    let mut open = true;
    egui::Window::new("Stonecutter")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            // --- input slot -------------------------------------
            ui.horizontal(|ui| {
                ui.label("Input:");
                let icon = input.map(|(id, c)| slot_icon(id, c, icon_uv_resolver));
                let resp = crate::ui::slot_widget::draw_slot_widget(
                    ui,
                    icon,
                    atlas_tex_id,
                    false,
                    egui::Sense::click(),
                );
                // Click the input slot -> return it to the inventory.
                if resp.clicked()
                    && let Some(stack) = input.take()
                {
                    give_to_inv(inv_view, stack);
                }
            });

            ui.separator();

            // --- cut options ------------------------------------
            if let Some((in_id, _)) = *input {
                let cuts = cuts_for(in_id);
                if cuts.is_empty() {
                    ui.label("Not a stonecutter material.");
                } else {
                    ui.label("Cuts:");
                    egui::Grid::new("stonecutter_cuts")
                        .spacing([4.0, 4.0])
                        .show(ui, |ui| {
                            for (i, cut) in cuts.iter().enumerate() {
                                let icon = Some(slot_icon(
                                    cut.output,
                                    cut.count,
                                    icon_uv_resolver,
                                ));
                                let resp = crate::ui::slot_widget::draw_slot_widget(
                                    ui,
                                    icon,
                                    atlas_tex_id,
                                    false,
                                    egui::Sense::click(),
                                );
                                if resp.on_hover_text(cut.name).clicked() {
                                    *craft_out = Some(i);
                                }
                                if (i + 1) % 4 == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                }
            } else {
                ui.label("Click an inventory block to load it.");
            }

            ui.separator();

            // --- player inventory -------------------------------
            ui.label("Inventory:");
            egui::Grid::new("stonecutter_inv")
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    for i in 0..inv_view.len() {
                        let icon =
                            inv_view[i].map(|(id, c)| slot_icon(id, c, icon_uv_resolver));
                        let resp = crate::ui::slot_widget::draw_slot_widget(
                            ui,
                            icon,
                            atlas_tex_id,
                            false,
                            egui::Sense::click(),
                        );
                        // Click an inventory stack -> load it as input
                        // (only cuttable materials; prior input returns).
                        if resp.clicked()
                            && let Some((id, c)) = inv_view[i]
                            && !cuts_for(id).is_empty()
                        {
                            let prior = input.take();
                            *input = Some((id, c));
                            inv_view[i] = None;
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

/// Merge `stack` into the first compatible / empty inventory slot.
/// Overflow is dropped (the stonecutter never overfills; callers size
/// the inventory generously). Mirrors the simple add used elsewhere.
/// `pub(crate)` so `render.rs`'s apply pass reuses it for the crafted
/// output + the on-close input return.
pub(crate) fn give_to_inv(inv: &mut [Option<(u16, u8)>], stack: (u16, u8)) {
    let (id, mut count) = stack;
    let cap = crate::inventory::STACK_MAX;
    // Merge into existing stacks first.
    for slot in inv.iter_mut() {
        if let Some((sid, sc)) = slot
            && *sid == id
            && *sc < cap
        {
            let room = cap - *sc;
            let add = room.min(count);
            *sc += add;
            count = count.saturating_sub(add);
            if count == 0 {
                return;
            }
        }
    }
    // Then fill empty slots.
    for slot in inv.iter_mut() {
        if slot.is_none() {
            let put = count.min(cap);
            *slot = Some((id, put));
            count = count.saturating_sub(put);
            if count == 0 {
                return;
            }
        }
    }
}
