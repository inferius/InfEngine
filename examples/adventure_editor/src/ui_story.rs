//! A60.1 / A99: the whole game as a map, in the middle of the window.
//!
//! Everything else in the editor shows one room at a time, and a game
//! is not one room: it is two hundred of them and the ways between.
//! Which rooms a door reaches, which corner of the world a cutscene
//! jumps to, which rooms nothing leads to at all - none of that is
//! visible from inside any single room.
//!
//! It was a floating window of unmoveable boxes and hairline links.
//! A99 rebuilds it on what node editors have agreed on for a decade -
//! Twine's story map and the Yarn Spinner graph in particular:
//!
//! * the arrangement is the author's, dragged by hand and **kept**
//!   (in `story.layout.ron`, beside the rooms), with Arrange as the
//!   rescue rather than the law;
//! * links are arrows, drawn from the edge of one box towards the
//!   other, so a map says which way the door goes;
//! * a door to a room that does not exist ends in a red cross, which
//!   is Twine's broken-link mark and the fastest bug in the list to
//!   find;
//! * the start is badged, unreachable rooms are outlined in warning,
//!   and searching dims everything that does not match rather than
//!   hiding it, so the shape of the game stays visible while you look;
//! * a minimap in the corner, because the point of a map is knowing
//!   where you are in it.

use std::collections::{BTreeMap, HashMap, VecDeque};

use glam::Vec2;
use infengine_adventure::story::{LinkKind, NodeKind, Story};

use crate::lang::t;
use crate::story_layout::Layout;

/// One node's box, in canvas units.
const NODE: egui::Vec2 = egui::vec2(150.0, 26.0);
/// Auto-layout spacing.
const COL_STEP: f32 = 230.0;
const ROW_STEP: f32 = 44.0;
/// The minimap's corner.
const MAP_SIZE: egui::Vec2 = egui::vec2(160.0, 110.0);

/// What the map asked for.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StoryActions {
    /// A room to open in the editor.
    pub open_room: Option<String>,
    /// A room to make the game start in.
    pub set_start: Option<String>,
    /// The arrangement changed and should be written.
    pub save_layout: bool,
    /// Lay every node out again from scratch.
    pub arrange: bool,
}

/// The map's own state.
#[derive(Debug, Clone)]
pub struct StoryUi {
    pan: egui::Vec2,
    zoom: f32,
    filter: String,
    /// What a dragged node lands on. Twine's "snap to grid", and off is
    /// as valid as on.
    pub snap: f32,
    /// The node being dragged, and whether it has moved.
    dragging: Option<String>,
    moved: bool,
    /// The node whose details are showing.
    pub selected: Option<String>,
}

impl Default for StoryUi {
    fn default() -> Self {
        Self {
            pan: egui::vec2(40.0, 40.0),
            zoom: 1.0,
            filter: String::new(),
            snap: 10.0,
            dragging: None,
            moved: false,
            selected: None,
        }
    }
}

/// Where every node goes when nobody has arranged them.
///
/// Breadth-first from the start room, so distance from the start reads
/// left to right - the game literally reads that way. Rooms nothing
/// reaches go in the columns past the furthest reachable one, grouped
/// rather than scattered: a shelf of them at the edge says "these are
/// cut off" at a glance.
pub fn arrange(story: &Story, start: &str) -> BTreeMap<String, Vec2> {
    let mut adjacency: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for link in &story.links {
        adjacency.entry(&link.from).or_default().push(&link.to);
    }

    let mut depth: HashMap<String, usize> = HashMap::new();
    let mut queue = VecDeque::new();
    if story.nodes.iter().any(|node| node.id == start) {
        depth.insert(start.to_string(), 0);
        queue.push_back(start.to_string());
    }
    while let Some(id) = queue.pop_front() {
        let here = depth[&id];
        for next in adjacency.get(id.as_str()).into_iter().flatten() {
            if !depth.contains_key(*next) {
                depth.insert((*next).to_string(), here + 1);
                queue.push_back((*next).to_string());
            }
        }
    }

    let deepest = depth.values().copied().max().unwrap_or(0);
    let mut rows: HashMap<usize, usize> = HashMap::new();
    let mut out = BTreeMap::new();
    for node in &story.nodes {
        let column = depth
            .get(&node.id)
            .copied()
            .unwrap_or_else(|| deepest + if node.kind == NodeKind::Screen { 2 } else { 1 });
        let row = rows.entry(column).or_insert(0);
        out.insert(
            node.id.clone(),
            Vec2::new(column as f32 * COL_STEP, *row as f32 * ROW_STEP),
        );
        *row += 1;
    }
    out
}

/// Draw the map in the centre of the window.
pub fn panel(
    ctx: &egui::Context,
    state: &mut StoryUi,
    story: &Story,
    layout: &mut Layout,
    start: &str,
) -> StoryActions {
    let mut actions = StoryActions::default();
    egui::CentralPanel::default().show(ctx, |ui| {
        header(ui, state, story, start, &mut actions);
        ui.separator();
        canvas(ui, state, story, layout, start, &mut actions);
    });
    actions
}

fn header(
    ui: &mut egui::Ui,
    state: &mut StoryUi,
    story: &Story,
    start: &str,
    actions: &mut StoryActions,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!("{} míst, {} cest mezi nimi", story.nodes.len(), story.links.len())
            } else {
                format!(
                    "{} places, {} ways between them",
                    story.nodes.len(),
                    story.links.len()
                )
            })
            .small()
            .weak(),
        );
        ui.separator();
        if ui
            .button(t("Arrange", "Uspořádat"))
            .on_hover_text(t(
                "Lay every place out again, left to right from the start",
                "Rozmísti všechna místa znovu, zleva doprava od startu",
            ))
            .clicked()
        {
            actions.arrange = true;
        }
        ui.add(
            egui::Slider::new(&mut state.zoom, 0.25..=2.0)
                .show_value(false)
                .text(t("zoom", "lupa")),
        );
        let mut snapping = state.snap > 0.0;
        if ui
            .checkbox(&mut snapping, t("snap", "přichytávat"))
            .on_hover_text(t(
                "Dragged places land on a grid",
                "Tažená místa dosednou na mřížku",
            ))
            .changed()
        {
            state.snap = if snapping { 10.0 } else { 0.0 };
        }
        ui.label(t("find", "hledat"));
        ui.add(egui::TextEdit::singleline(&mut state.filter).desired_width(120.0));
        if !state.filter.is_empty() && ui.small_button("✕").clicked() {
            state.filter.clear();
        }
    });
    ui.horizontal(|ui| {
        legend(ui, egui::Color32::from_rgb(255, 210, 90), t("door", "dveře"));
        legend(ui, egui::Color32::from_rgb(120, 200, 255), t("script", "skript"));
        legend(ui, egui::Color32::from_rgb(150, 230, 150), t("button", "tlačítko"));
        let lost = story.unreachable(start).len();
        if lost > 0 {
            ui.label(
                egui::RichText::new(if crate::lang::is_czech() {
                    format!("{lost} míst, kam nic nevede")
                } else {
                    format!("{lost} places nothing leads to")
                })
                .small()
                .color(egui::Color32::from_rgb(255, 160, 140)),
            );
        }
        if !story.missing.is_empty() {
            ui.label(
                egui::RichText::new(if crate::lang::is_czech() {
                    format!("{} dveří vede nikam", story.missing.len())
                } else {
                    format!("{} doors lead nowhere", story.missing.len())
                })
                .small()
                .color(egui::Color32::from_rgb(255, 130, 130)),
            );
        }
        ui.label(
            egui::RichText::new(t(
                "drag a place to move it · double-click to open it · right-click for more",
                "místo přesuneš tažením · dvojklikem ho otevřeš · pravým tlačítkem víc",
            ))
            .small()
            .weak(),
        );
    });
}

fn legend(ui: &mut egui::Ui, colour: egui::Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 3.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, colour);
    ui.label(egui::RichText::new(label).small().weak());
}

fn canvas(
    ui: &mut egui::Ui,
    state: &mut StoryUi,
    story: &Story,
    layout: &mut Layout,
    start: &str,
    actions: &mut StoryActions,
) {
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    // Dragging the background pans, unless a node took the drag first.
    if state.dragging.is_none() && response.dragged() {
        state.pan += response.drag_delta();
    }
    // The wheel zooms about the pointer, so the place being looked at
    // stays under the cursor.
    if let Some(pointer) = response.hover_pos() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 {
            let before = (pointer - rect.min - state.pan) / state.zoom;
            state.zoom = (state.zoom * (scroll * 0.002).exp()).clamp(0.25, 2.0);
            let after = (pointer - rect.min - state.pan) / state.zoom;
            state.pan += (after - before) * state.zoom;
        }
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(18));

    // Anything the layout does not place yet gets the automatic spot,
    // so a project that has never been arranged still reads.
    // Worked out once per frame rather than asked of the layout as it
    // is being dragged: a closure borrowing the layout would forbid
    // writing to it, which is the one thing a drag has to do.
    let auto = arrange(story, start);
    let places: BTreeMap<String, Vec2> = story
        .nodes
        .iter()
        .map(|node| {
            let at = layout
                .get(&node.id)
                .or_else(|| auto.get(&node.id).copied())
                .unwrap_or(Vec2::ZERO);
            (node.id.clone(), at)
        })
        .collect();
    let at = |id: &str| -> Vec2 { places.get(id).copied().unwrap_or(Vec2::ZERO) };
    let zoom = state.zoom;
    let to_screen = |p: Vec2| rect.min + (egui::vec2(p.x, p.y) + state.pan) * zoom;
    let box_of = |id: &str| egui::Rect::from_min_size(to_screen(at(id)), NODE * zoom);

    let filter = state.filter.to_ascii_lowercase();
    let dim = |id: &str| !filter.is_empty() && !id.to_ascii_lowercase().contains(&filter);

    // Links first, boxes over them.
    for link in &story.links {
        if story.missing.contains(&link.to) {
            continue;
        }
        let (from, to) = (box_of(&link.from), box_of(&link.to));
        let faded = dim(&link.from) && dim(&link.to);
        let colour = match link.kind {
            LinkKind::Exit => egui::Color32::from_rgb(255, 210, 90),
            LinkKind::Script => egui::Color32::from_rgb(120, 200, 255),
            LinkKind::Button => egui::Color32::from_rgb(150, 230, 150),
        };
        let colour = colour.gamma_multiply(if faded { 0.15 } else { 0.65 });
        arrow(&painter, from, to, colour, zoom);
    }

    // A door to a room that is not there: Twine's broken link, and the
    // fastest bug in the game to find once it is drawn. The story knows
    // which places are missing, so the cross hangs off whoever names
    // one.
    let red = egui::Color32::from_rgb(255, 90, 90);
    for link in &story.links {
        if !story.missing.contains(&link.to) {
            continue;
        }
        let source = box_of(&link.from);
        let end = source.right_center() + egui::vec2(46.0 * zoom, 0.0);
        painter.line_segment([source.right_center(), end], egui::Stroke::new(1.5, red));
        let r = 4.0 * zoom;
        painter.line_segment(
            [end + egui::vec2(-r, -r), end + egui::vec2(r, r)],
            egui::Stroke::new(1.5, red),
        );
        painter.line_segment(
            [end + egui::vec2(r, -r), end + egui::vec2(-r, r)],
            egui::Stroke::new(1.5, red),
        );
        painter.text(
            end + egui::vec2(8.0 * zoom, 0.0),
            egui::Align2::LEFT_CENTER,
            &link.to,
            egui::FontId::proportional((10.0 * zoom).max(7.0)),
            red,
        );
    }

    let unreachable = story.unreachable(start);
    for node in &story.nodes {
        let box_ = box_of(&node.id);
        if !rect.intersects(box_) {
            continue;
        }
        let faded = dim(&node.id);
        let is_start = node.id == start;
        let fill = if is_start {
            egui::Color32::from_rgb(70, 120, 80)
        } else if node.kind == NodeKind::Screen {
            egui::Color32::from_rgb(95, 75, 120)
        } else {
            egui::Color32::from_rgb(50, 62, 85)
        };
        let fill = if faded { fill.gamma_multiply(0.3) } else { fill };
        painter.rect_filled(box_, 4.0, fill);
        if unreachable.iter().any(|lost| lost.id == node.id) {
            painter.rect_stroke(
                box_,
                4.0,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 160, 140)),
                egui::StrokeKind::Inside,
            );
        }
        if state.selected.as_deref() == Some(node.id.as_str()) {
            painter.rect_stroke(
                box_.expand(2.0),
                5.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 235, 140)),
                egui::StrokeKind::Outside,
            );
        }
        let label = if is_start {
            format!("▶ {}", node.id)
        } else {
            node.id.clone()
        };
        painter.text(
            box_.left_center() + egui::vec2(6.0 * zoom, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional((12.0 * zoom).max(7.0)),
            if faded {
                egui::Color32::from_gray(110)
            } else {
                egui::Color32::from_gray(235)
            },
        );

        let hit = ui.interact(
            box_,
            egui::Id::new(("story_node", &node.id)),
            egui::Sense::click_and_drag(),
        );
        if hit.drag_started() {
            state.dragging = Some(node.id.clone());
            state.selected = Some(node.id.clone());
        }
        if hit.dragged() && state.dragging.as_deref() == Some(node.id.as_str()) {
            let delta = hit.drag_delta() / zoom;
            let mut moved = at(&node.id) + Vec2::new(delta.x, delta.y);
            if state.snap > 0.0 {
                moved = Vec2::new(
                    (moved.x / state.snap).round() * state.snap,
                    (moved.y / state.snap).round() * state.snap,
                );
            }
            layout.set(&node.id, moved);
            state.moved = true;
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if hit.clicked() {
            state.selected = Some(node.id.clone());
        }
        if hit.double_clicked() && node.kind == NodeKind::Room {
            actions.open_room = Some(node.id.clone());
        }
        hit.context_menu(|ui| {
            if node.kind == NodeKind::Room {
                if ui.button(t("Open this room", "Otevřít místnost")).clicked() {
                    actions.open_room = Some(node.id.clone());
                    ui.close();
                }
                if ui
                    .button(t("Start the game here", "Odsud hru začínat"))
                    .clicked()
                {
                    actions.set_start = Some(node.id.clone());
                    ui.close();
                }
            }
            if ui.button(t("Send to the middle", "Poslat doprostřed")).clicked() {
                layout.set(&node.id, Vec2::ZERO);
                state.moved = true;
                ui.close();
            }
        });
        if hit.hovered() {
            let ways: Vec<String> = story
                .out_of(&node.id)
                .iter()
                .map(|link| format!("{} \u{2192} {}", link.kind.label(), link.to))
                .collect();
            hit.on_hover_text(if ways.is_empty() {
                t("leads nowhere", "nikam nevede").to_string()
            } else {
                ways.join("\n")
            });
        }
    }

    // Letting go of a dragged node is when the arrangement is worth
    // writing: saving on every frame of a drag would be a file write
    // per pixel.
    if state.dragging.is_some() && ui.input(|i| i.pointer.any_released()) {
        state.dragging = None;
        if state.moved {
            state.moved = false;
            actions.save_layout = true;
        }
    }

    minimap(&painter, rect, story, &places, start);
}

/// One link, drawn as an arrow from box to box.
fn arrow(
    painter: &egui::Painter,
    from: egui::Rect,
    to: egui::Rect,
    colour: egui::Color32,
    zoom: f32,
) {
    // Edge to edge rather than centre to centre, so the line does not
    // disappear under the boxes it connects.
    let dir = (to.center() - from.center()).normalized();
    let start = from.center() + dir * (from.size().max_elem() * 0.42);
    let end = to.center() - dir * (to.size().max_elem() * 0.42);
    painter.line_segment([start, end], egui::Stroke::new(1.5, colour));
    // The head: which way the door goes, which is the question.
    let head = 7.0 * zoom;
    let left = egui::vec2(-dir.y, dir.x);
    painter.line_segment(
        [end, end - dir * head + left * head * 0.5],
        egui::Stroke::new(1.5, colour),
    );
    painter.line_segment(
        [end, end - dir * head - left * head * 0.5],
        egui::Stroke::new(1.5, colour),
    );
}

/// The corner map: the whole game at once, however far the view is in.
fn minimap(
    painter: &egui::Painter,
    rect: egui::Rect,
    story: &Story,
    places: &BTreeMap<String, Vec2>,
    start: &str,
) {
    if story.nodes.len() < 2 {
        return;
    }
    let frame = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - MAP_SIZE.x - 10.0, rect.max.y - MAP_SIZE.y - 10.0),
        MAP_SIZE,
    );
    painter.rect_filled(frame, 3.0, egui::Color32::from_black_alpha(180));

    let dots: Vec<Vec2> = story
        .nodes
        .iter()
        .map(|node| places.get(&node.id).copied().unwrap_or(Vec2::ZERO))
        .collect();
    let (mut lo, mut hi) = (dots[0], dots[0]);
    for p in &dots {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let span = (hi - lo).max(Vec2::splat(1.0));
    let scale = ((frame.width() - 12.0) / span.x).min((frame.height() - 12.0) / span.y);
    for (node, p) in story.nodes.iter().zip(dots) {
        let dot = frame.min + egui::vec2(6.0, 6.0) + egui::vec2((p.x - lo.x) * scale, (p.y - lo.y) * scale);
        painter.circle_filled(
            dot,
            if node.id == start { 3.0 } else { 2.0 },
            if node.id == start {
                egui::Color32::from_rgb(150, 230, 150)
            } else {
                egui::Color32::from_gray(170)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_adventure::story::{Link, Node};

    fn story() -> Story {
        Story {
            nodes: vec![
                Node {
                    id: "a".into(),
                    kind: NodeKind::Room,
                },
                Node {
                    id: "b".into(),
                    kind: NodeKind::Room,
                },
                Node {
                    id: "island".into(),
                    kind: NodeKind::Room,
                },
            ],
            links: vec![Link {
                from: "a".into(),
                to: "b".into(),
                kind: LinkKind::Exit,
                via: "door".into(),
            }],
            missing: Default::default(),
        }
    }

    #[test]
    fn distance_from_the_start_reads_left_to_right() {
        let places = arrange(&story(), "a");
        assert!(places["b"].x > places["a"].x);
    }

    #[test]
    fn a_room_nothing_reaches_sits_past_the_furthest_reachable_one() {
        let places = arrange(&story(), "a");
        assert!(places["island"].x > places["b"].x);
    }

    #[test]
    fn a_start_the_story_does_not_have_still_lays_everything_out() {
        let places = arrange(&story(), "nowhere");
        assert_eq!(places.len(), 3, "every node still gets a place");
    }

    #[test]
    fn a_hand_placed_node_keeps_its_place_over_the_automatic_one() {
        // The arrangement is the author's work; auto-layout is the
        // rescue, not the law.
        let mut layout = Layout::default();
        layout.set("b", Vec2::new(-500.0, 12.0));
        assert_eq!(layout.get("b"), Some(Vec2::new(-500.0, 12.0)));
        assert!(layout.get("a").is_none(), "the rest fall back to automatic");
    }

    #[test]
    fn a_fresh_map_snaps_and_has_nothing_selected() {
        let state = StoryUi::default();
        assert!(state.snap > 0.0);
        assert!(state.selected.is_none() && state.dragging.is_none());
    }
}
