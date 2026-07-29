//! W319 - villager trade panel.
//!
//! RMB on a villager opens it. One row per offer of the villager's
//! profession (tiers 1-2 from the generated MC trade table): the
//! wanted stack(s), an arrow, the given stack, and a Trade button
//! enabled only when the inventory can pay. The button reports the
//! offer index; `render.rs` applies the exchange on the inventory
//! snapshot after the egui pass (stonecutter pattern).

use crate::trades::{count_items, sampled_offer_indices, trades_for};
use crate::ui::SlotIcon;

pub(crate) struct TradePanelInputs<'a> {
    pub(crate) ctx: &'a egui::Context,
    pub(crate) profession: &'a str,
    /// Mob id of the open villager - seeds the per-villager offer
    /// sample (W440).
    pub(crate) villager_id: u32,
    /// W458: villager trade level (1..=5) - gates the offer tiers
    /// and names the window (Novice..Master).
    pub(crate) level: u8,
    pub(crate) inv_view: &'a [Option<(u16, u8)>],
    pub(crate) atlas_tex_id: Option<egui::TextureId>,
    pub(crate) icon_uv_resolver: &'a dyn Fn(u16) -> Option<[f32; 4]>,
    pub(crate) open_local: &'a mut bool,
    /// Index into `trades_for(profession)` the player bought.
    pub(crate) trade_out: &'a mut Option<usize>,
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

pub(crate) fn render(inp: TradePanelInputs<'_>) {
    if !*inp.open_local {
        return;
    }
    let TradePanelInputs {
        ctx,
        profession,
        villager_id,
        level,
        inv_view,
        atlas_tex_id,
        icon_uv_resolver,
        open_local,
        trade_out,
    } = inp;

    let title = {
        let mut c = profession.chars();
        match c.next() {
            Some(f) => format!(
                "Trading - {}{} ({})",
                f.to_uppercase(),
                c.as_str(),
                crate::trades::level_name(level),
            ),
            None => "Trading".to_string(),
        }
    };
    let mut open = true;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            // W440: bounded height + scroll - the full pool of some
            // professions (shepherd wool/carpet colours) overflowed
            // the window with no way to reach the lower rows.
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            egui::Grid::new("villager_trades")
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    // W440: per-villager sampled offers (2 per tier,
                    // MC's per-level roll) instead of the whole
                    // profession pool. `i` stays an index into the
                    // trades_for enumeration so the apply site's
                    // `.nth(i)` keeps working.
                    for i in
                        sampled_offer_indices(profession, villager_id, level)
                    {
                        let Some(t) = trades_for(profession).nth(i) else {
                            continue;
                        };
                        let afford = count_items(inv_view, t.want)
                            >= t.want_n as u32
                            && t.want2.is_none_or(|(id2, n2)| {
                                count_items(inv_view, id2) >= n2 as u32
                            });
                        let want =
                            Some(slot_icon(t.want, t.want_n, icon_uv_resolver));
                        crate::ui::slot_widget::draw_slot_widget(
                            ui,
                            want,
                            atlas_tex_id,
                            false,
                            egui::Sense::hover(),
                        );
                        // W440: ALWAYS draw the second cost column
                        // (empty slot when absent) so the arrow +
                        // result columns line up across rows.
                        let w2 = t
                            .want2
                            .map(|(id2, n2)| slot_icon(id2, n2, icon_uv_resolver));
                        crate::ui::slot_widget::draw_slot_widget(
                            ui,
                            w2,
                            atlas_tex_id,
                            false,
                            egui::Sense::hover(),
                        );
                        ui.label("->");
                        let give =
                            Some(slot_icon(t.give, t.give_n, icon_uv_resolver));
                        crate::ui::slot_widget::draw_slot_widget(
                            ui,
                            give,
                            atlas_tex_id,
                            false,
                            egui::Sense::hover(),
                        );
                        if ui
                            .add_enabled(afford, egui::Button::new("Trade"))
                            .clicked()
                        {
                            *trade_out = Some(i);
                        }
                        ui.end_row();
                    }
                });
            });
        });
    if !open {
        *open_local = false;
    }
}
