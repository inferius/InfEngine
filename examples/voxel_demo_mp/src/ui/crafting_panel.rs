//! W50.3 + W54.2 - 3x3 crafting-table panel.
//!
//! Renders the egui::Window for the crafting_table block. The
//! 3x3 grid lives on `Voxel::craft_grid_3x3`; the panel handles
//! LMB / RMB / Shift+click / drag-distribute against it, computes
//! the output via `recipes::matcher::match_3x3`, and reports back
//! to the caller whether the player clicked the output slot.
//!
//! W469: fully stack-native. The grid, the cursor and the player
//! inventory view all hold `Option<ItemStack>` - components ride
//! through every click, merges gate on `same_stackable` (a worn
//! tool never count-merges into a fresh one), a full place onto a
//! non-stackable slot SWAPS (the W467 semantic), and splits clone
//! components to both halves. Recipe matching stays id + count
//! based.
//!
//! Mouse handling (W470: every rule is the SHARED ported doClick
//! core in `container::click` - this file only tracks input):
//! - LMB / RMB click -> `pickup_click` (take all / place all /
//!   merge / swap; RMB takes the CEIL half / places one).
//! - Shift+LMB -> `quick_move` with `CraftingMenu.quickMoveStack`
//!   routing (grid -> player main-then-hotbar; player -> grid,
//!   falling back to a hotbar<->main move).
//! - LMB / RMB drag-paint -> visit tracking with MC add-slot
//!   eligibility, applied ON RELEASE via `quick_craft_apply`
//!   (floor split / one per slot, remainder to the cursor).
//! - Double-click -> `pickup_all` collect across grid + inventory.
//! - Number keys over a hovered slot -> `swap_click` with the
//!   hotbar; Q / Ctrl+Q -> `throw_from_slot` (spawned by render).
//! - Creative middle-click -> `clone_click`.
//! - Held stack always painted as a floating sprite at the mouse
//!   pointer so the player sees what's on the cursor.

use crate::container;
use crate::container::click::{self, SectionMut};
use crate::item::ItemStack;
use crate::item::stack::{drop_into_first_free_stacks, take_n_stacks};
use crate::recipes::matcher;

/// All inputs the 3x3 panel needs in one struct so render.rs
/// doesn't have to grow another 10-arg call site.
pub(crate) struct CraftingPanelInputs<'a> {
    pub(crate) ctx: &'a egui::Context,
    pub(crate) grid: &'a mut [Option<ItemStack>; 9],
    pub(crate) held: &'a mut Option<ItemStack>,
    pub(crate) inv_view: &'a mut [Option<ItemStack>],
    pub(crate) atlas_tex_id: Option<egui::TextureId>,
    pub(crate) icon_uv_resolver: &'a dyn Fn(u16) -> Option<[f32; 4]>,
    pub(crate) open_local: &'a mut bool,
    pub(crate) take_output_out: &'a mut bool,
    /// W470: creative flag for the CLONE arm.
    pub(crate) creative: bool,
    /// W470: stacks thrown by Q / Ctrl+Q this frame - render.rs
    /// spawns them as drops with MC toss velocity.
    pub(crate) thrown_out: &'a mut Vec<ItemStack>,
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
        creative,
        thrown_out,
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
                            let slot = grid[idx]
                                .as_ref()
                                .map(|st| (st.id, st.count));
                            let resp = draw_slot(
                                ui,
                                slot,
                                36.0,
                                atlas_tex_id,
                                icon_uv_resolver,
                            );
                            track_slot(TrackCtx {
                                drag: &mut drag,
                                clicks: &mut clicks,
                                resp,
                                sref: SlotRef::Grid(idx),
                                slot: grid[idx].as_ref(),
                                held: held.as_ref(),
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
                        let slot = inv_view
                            .get(idx)
                            .and_then(|s| s.as_ref())
                            .map(|st| (st.id, st.count));
                        let resp = draw_slot(
                            ui,
                            slot,
                            36.0,
                            atlas_tex_id,
                            icon_uv_resolver,
                        );
                        track_slot(TrackCtx {
                            drag: &mut drag,
                            clicks: &mut clicks,
                            resp,
                            sref: SlotRef::Inv(idx),
                            slot: inv_view.get(idx).and_then(|s| s.as_ref()),
                            held: held.as_ref(),
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

    // W54.5: Recipe book lives in its own egui::Window so the
    // player can dock it next to the crafting panel and search
    // recipes without losing the grid view.
    if book_open {
        render_recipe_book_window(
            ctx,
            &mut book_open,
            atlas_tex_id,
            icon_uv_resolver,
            inv_view,
            grid,
        );
    }
    ctx.memory_mut(|m| m.data.insert_temp(book_id, book_open));

    // ---- Drag state machine: react to release (W470: MC
    // QUICK_CRAFT phase 2 - apply-on-release via the shared
    // `quick_craft_apply`; the old live-updating distribute fns
    // are gone). A single-slot visit stays a plain click.
    let mut drag_committed = false;
    if lmb_released
        && drag.button == Some(DragButton::Left)
        && drag.slots.len() >= 2
    {
        let gis: Vec<usize> = drag.slots.iter().map(|s| gi_of(*s)).collect();
        let mut sections = sections_of(grid, inv_view);
        click::quick_craft_apply(&mut sections, held, &gis, false);
        drag_committed = true;
    } else if rmb_released
        && drag.button == Some(DragButton::Right)
        && drag.slots.len() >= 2
    {
        let gis: Vec<usize> = drag.slots.iter().map(|s| gi_of(*s)).collect();
        let mut sections = sections_of(grid, inv_view);
        click::quick_craft_apply(&mut sections, held, &gis, true);
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
            if let Some(st) =
                apply_click(grid, inv_view, held, sref, kind, creative)
            {
                thrown_out.push(st);
            }
        }
    }
    // Persist drag state for the next frame.
    ctx.memory_mut(|m| m.data.insert_temp(drag_id, drag));

    // ---- Floating cursor sprite. ------------------------------
    if let (Some(pos), Some(st)) = (cursor_pos, held.as_ref()) {
        paint_cursor(
            ctx,
            pos,
            st.id,
            st.count,
            atlas_tex_id,
            icon_uv_resolver,
        );
    }
}

#[derive(Debug, Clone, Copy)]
enum ClickKind {
    Lmb,
    Rmb,
    ShiftLmb,
    /// W470: double-click collect (MC PICKUP_ALL).
    Collect,
    /// W470: number key over the hovered slot (hotbar index 0..8).
    SwapHotbar(u8),
    /// W470: Q / Ctrl+Q throw.
    ThrowOne,
    ThrowAll,
    /// W470: creative middle-click clone.
    CloneStack,
}

/// Per-slot tracking inputs (W470: struct per the 5+ args rule).
struct TrackCtx<'a> {
    drag: &'a mut DragState,
    clicks: &'a mut Vec<(SlotRef, ClickKind)>,
    resp: egui::Response,
    sref: SlotRef,
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
        sref,
        slot,
        held,
        lmb_pressed,
        lmb_down,
        rmb_pressed,
        rmb_down,
        shift_down,
        ui,
    } = t;
    // W54.4: use `contains_pointer()` not `hovered()`. egui's
    // `hovered()` returns false on widgets the cursor passes over
    // during an already-active drag started on a sibling widget;
    // `contains_pointer()` reports the raw geometric hit which is
    // what we want for "did the drag visit this slot".
    let pointer_over = resp.contains_pointer();
    if let Some(cur) = held
        && pointer_over
    {
        // W470: MC add-slot eligibility (source lines 354-358) -
        // quick-replaceable + cursor holds more items than slots
        // already visited (no accept filters on this screen).
        let eligible =
            click::can_add_drag_slot(slot, true, cur, drag.slots.len());
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
    // W470: hovered-slot keyboard ops (suppressed while a text
    // field owns the keyboard, e.g. the recipe-book search).
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
                    clicks.push((sref, ClickKind::SwapHotbar(n as u8)));
                    break;
                }
            }
            if slot.is_some() && i.key_pressed(egui::Key::Q) {
                clicks.push((
                    sref,
                    if i.modifiers.command {
                        ClickKind::ThrowAll
                    } else {
                        ClickKind::ThrowOne
                    },
                ));
            }
        });
    }
    // Standard click handling: fires on release inside the widget.
    // Double-click first: on the second click of a double,
    // `clicked()` is also true (MC: click #1 picked the stack up
    // the previous frame, click #2 collects).
    if resp.double_clicked() {
        if held.is_some() {
            clicks.push((sref, ClickKind::Collect));
        }
    } else if resp.clicked() {
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
    } else if resp.middle_clicked() && slot.is_some() {
        clicks.push((sref, ClickKind::CloneStack));
    }
}

/// Global index of a `SlotRef` on the [3x3 grid, player inventory]
/// screen.
fn gi_of(sref: SlotRef) -> usize {
    match sref {
        SlotRef::Grid(i) => i,
        SlotRef::Inv(i) => 9 + i,
    }
}

/// The panel's screen as click-core sections. No accept filters -
/// craft cells and inventory slots take anything.
fn sections_of<'a>(
    grid: &'a mut [Option<ItemStack>; 9],
    inv: &'a mut [Option<ItemStack>],
) -> [SectionMut<'a>; 2] {
    [SectionMut::open(&mut grid[..]), SectionMut::open(inv)]
}

/// Apply one click through the SHARED doClick core. Returns a
/// thrown stack for the Q ops (render.rs spawns it).
fn apply_click(
    grid: &mut [Option<ItemStack>; 9],
    inv: &mut [Option<ItemStack>],
    held: &mut Option<ItemStack>,
    sref: SlotRef,
    kind: ClickKind,
    creative: bool,
) -> Option<ItemStack> {
    let inv_len = inv.len();
    let gi = gi_of(sref);
    let mut sections = sections_of(grid, inv);
    match kind {
        ClickKind::Lmb => {
            click::pickup_click(&mut sections, held, gi, false);
        }
        ClickKind::Rmb => {
            click::pickup_click(&mut sections, held, gi, true);
        }
        ClickKind::ShiftLmb => {
            // CraftingMenu.quickMoveStack routing: grid cells move
            // to the player inventory main-first then hotbar
            // (`moveItemStackTo(10, 46, false)`); player slots try
            // the grid first (`moveItemStackTo(1, 10, false)`),
            // then fall back to a hotbar<->main move.
            match sref {
                SlotRef::Grid(_) => {
                    let order: Vec<usize> = (crate::inventory::HOTBAR_SLOTS
                        ..inv_len)
                        .chain(0..crate::inventory::HOTBAR_SLOTS)
                        .map(|i| 9 + i)
                        .collect();
                    click::quick_move(&mut sections, gi, &order);
                }
                SlotRef::Inv(i) => {
                    let grid_order: Vec<usize> = (0..9).collect();
                    if !click::quick_move(&mut sections, gi, &grid_order) {
                        let fallback: Vec<usize> =
                            if i < crate::inventory::HOTBAR_SLOTS {
                                (crate::inventory::HOTBAR_SLOTS..inv_len)
                                    .map(|k| 9 + k)
                                    .collect()
                            } else {
                                (0..crate::inventory::HOTBAR_SLOTS)
                                    .map(|k| 9 + k)
                                    .collect()
                            };
                        click::quick_move(&mut sections, gi, &fallback);
                    }
                }
            }
        }
        ClickKind::Collect => {
            click::pickup_all(&mut sections, held, gi);
        }
        ClickKind::SwapHotbar(h) => {
            if (h as usize) < crate::inventory::HOTBAR_SLOTS {
                click::swap_click(&mut sections, gi, 9 + h as usize);
            }
        }
        ClickKind::ThrowOne => {
            return click::throw_from_slot(&mut sections, held, gi, false);
        }
        ClickKind::ThrowAll => {
            return click::throw_from_slot(&mut sections, held, gi, true);
        }
        ClickKind::CloneStack => {
            click::clone_click(&sections, held, gi, creative);
        }
    }
    None
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
    // W364: item-name hover tooltip (R20 request).
    if let Some((id, _)) = slot {
        return resp.on_hover_text(crate::item::item_display_name(id));
    }
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

/// Project the stack-typed grid to plain id + count cells for the
/// matcher (recipes match on ids + counts; components never gate a
/// recipe).
pub(crate) fn grid_as_u16(
    g: &[Option<ItemStack>; 9],
) -> [Option<(u16, u8)>; 9] {
    std::array::from_fn(|i| g[i].as_ref().map(|st| (st.id, st.count)))
}

/// W54.5 - Recipe book in a standalone egui::Window. Shows every
/// known shaped + shapeless recipe with a search filter and a
/// craftable-only toggle. Each card paints the 3x3 input layout
/// next to an arrow + output icon (MC `RecipeBookComponent` shape).
/// Clicking a card auto-fills the 3x3 grid from the player inv.
fn render_recipe_book_window(
    ctx: &egui::Context,
    open: &mut bool,
    atlas_tex_id: Option<egui::TextureId>,
    icon_uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
    inv_view: &mut [Option<ItemStack>],
    grid: &mut [Option<ItemStack>; 9],
) {
    use crate::recipes::RECIPES;
    use crate::recipes::shaped::SHAPED_RECIPES;

    // Search state persisted via egui memory.
    let search_id = egui::Id::new("crafting_panel_recipe_search");
    let mut search: String = ctx
        .memory(|m| m.data.get_temp::<String>(search_id).unwrap_or_default());
    let only_craftable_id =
        egui::Id::new("crafting_panel_recipe_only_craftable");
    let mut only_craftable: bool = ctx.memory(|m| {
        m.data
            .get_temp::<bool>(only_craftable_id)
            .unwrap_or(false)
    });

    let screen = ctx.content_rect();
    egui::Window::new("Recipe Book")
        .open(open)
        .collapsible(true)
        .resizable(true)
        .default_pos([
            screen.center().x + 60.0,
            screen.center().y - 260.0,
        ])
        .default_size([360.0, 540.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.add_sized(
                    egui::vec2(200.0, 20.0),
                    egui::TextEdit::singleline(&mut search),
                );
                if ui.button("X").on_hover_text("Clear search").clicked() {
                    search.clear();
                }
            });
            ui.checkbox(&mut only_craftable, "Only craftable from inventory");
            ui.separator();

            // Build entry list with search + craftable filter.
            let q = search.trim().to_lowercase();
            let mut entries: Vec<RecipeEntry> = Vec::new();
            for r in RECIPES {
                entries.push(recipe_entry_shapeless(r));
            }
            for r in SHAPED_RECIPES {
                entries.push(recipe_entry_shaped(r));
            }
            let filtered: Vec<&RecipeEntry> = entries
                .iter()
                .filter(|e| {
                    if !q.is_empty() {
                        let name = e.name.to_lowercase();
                        // W124: multilingual haystack search.
                        let out = crate::lang::search_haystack(e.output.0);
                        if !name.contains(&q) && !out.contains(&q) {
                            return false;
                        }
                    }
                    if only_craftable && !is_craftable_from(e, inv_view) {
                        return false;
                    }
                    true
                })
                .collect();
            ui.label(format!("Showing {} recipes", filtered.len()));
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in filtered {
                    draw_recipe_card(
                        ui,
                        atlas_tex_id,
                        icon_uv_resolver,
                        entry,
                        inv_view,
                        grid,
                    );
                    ui.separator();
                }
            });
        });

    ctx.memory_mut(|m| {
        m.data.insert_temp(search_id, search);
        m.data.insert_temp(only_craftable_id, only_craftable);
    });
}

/// Predicate: does the player inv hold every input the recipe
/// needs, in the right quantity? Id-keyed - components never gate
/// a recipe.
fn is_craftable_from(
    entry: &RecipeEntry,
    inv: &[Option<ItemStack>],
) -> bool {
    let mut needed: Vec<(u16, u32)> = Vec::new();
    for id in entry.cells.iter().flatten() {
        if let Some(t) = needed.iter_mut().find(|(b, _)| *b == *id) {
            t.1 += 1;
        } else {
            needed.push((*id, 1));
        }
    }
    for (id, count) in &needed {
        let avail: u32 = inv
            .iter()
            .flatten()
            .filter(|st| st.id == *id)
            .map(|st| st.count as u32)
            .sum();
        if avail < *count {
            return false;
        }
    }
    true
}

/// W54.5: one recipe card. Layout (left -> right):
///   3x3 input grid  ->  output tile + count badge
/// Each row of the grid is 24px tall so the card fits a few per
/// scroll page. Click anywhere on the card auto-fills the 3x3
/// crafting grid.
fn draw_recipe_card(
    ui: &mut egui::Ui,
    atlas_tex_id: Option<egui::TextureId>,
    icon_uv_resolver: &dyn Fn(u16) -> Option<[f32; 4]>,
    entry: &RecipeEntry,
    inv_view: &mut [Option<ItemStack>],
    grid: &mut [Option<ItemStack>; 9],
) {
    let mut card_clicked = false;
    ui.horizontal(|ui| {
        // 3x3 input grid.
        ui.vertical(|ui| {
            for r in 0..3 {
                ui.horizontal(|ui| {
                    for c in 0..3 {
                        let id_opt = entry.cells[r * 3 + c];
                        let slot = id_opt.map(|id| (id, 1u8));
                        let resp = draw_slot(
                            ui,
                            slot,
                            22.0,
                            atlas_tex_id,
                            icon_uv_resolver,
                        );
                        if resp.clicked() {
                            card_clicked = true;
                        }
                    }
                });
            }
        });
        ui.label("  ->  ");
        let out_resp = draw_slot(
            ui,
            Some(entry.output),
            32.0,
            atlas_tex_id,
            icon_uv_resolver,
        );
        if out_resp.clicked() {
            card_clicked = true;
        }
        ui.vertical(|ui| {
            ui.label(entry.name);
            ui.label(
                egui::RichText::new(format!(
                    "-> {} x{}",
                    crate::lang::localized_item_name(entry.output.0),
                    entry.output.1,
                ))
                .small(),
            );
            let craftable = is_craftable_from(entry, inv_view);
            if craftable {
                ui.label(
                    egui::RichText::new("Craftable")
                        .small()
                        .color(egui::Color32::from_rgb(120, 220, 120)),
                );
            } else {
                ui.label(
                    egui::RichText::new("Missing items")
                        .small()
                        .color(egui::Color32::from_gray(180)),
                );
            }
            if ui.small_button("Fill grid").clicked() {
                card_clicked = true;
            }
        });
    });
    if card_clicked {
        autofill_grid_from_inv(entry, inv_view, grid);
    }
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

/// W54.3 - autofill: for each non-empty cell in the recipe, take
/// 1 of the matching item from the player inventory into the
/// matching grid cell (the taken single clones the source stack's
/// components). Bails (leaves grid + inv untouched) if any
/// required input is missing.
fn autofill_grid_from_inv(
    entry: &RecipeEntry,
    inv_view: &mut [Option<ItemStack>],
    grid: &mut [Option<ItemStack>; 9],
) {
    // First sweep: check feasibility against a tally. Mismatch ->
    // bail without side effects.
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
            .flatten()
            .filter(|st| st.id == *id)
            .map(|st| st.count as u32)
            .sum();
        if available < *count {
            return; // not enough; abort.
        }
    }
    // Return whatever is already on the grid to the inventory so
    // the autofill starts from a clean grid (components ride back).
    for cell in grid.iter_mut() {
        if let Some(st) = cell.take() {
            let _ = drop_into_first_free_stacks(inv_view, st);
        }
    }
    // Second sweep: actually move 1 of each required input into
    // the matching cell. Inventory mutated in place.
    for (idx, maybe_id) in entry.cells.iter().enumerate() {
        let Some(id) = maybe_id else { continue };
        if let Some(slot) = inv_view.iter_mut().find(|s| {
            s.as_ref().is_some_and(|st| st.id == *id && st.count > 0)
        }) {
            grid[idx] = take_n_stacks(slot, 1);
        }
    }
}

// Keep the `container` import quiet about unused submodule.
const _: fn() = || {
    let _ = container::Container::with_size(0);
};
