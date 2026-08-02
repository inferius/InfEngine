//! A60.1: the spider - the whole game as a map.
//!
//! Everything else in the editor shows one room at a time, and a game is
//! not one room: it is two hundred of them and the ways between. Which
//! rooms a door reaches, which corner of the world a cutscene jumps to,
//! which rooms nothing leads to at all - none of that is visible from
//! inside any single room, and it is exactly what an author needs to see
//! to understand what they have.
//!
//! The data side has existed for a while ([`infengine_adventure::story`]
//! builds the graph and finds the unreachable rooms); this is the
//! picture of it. Rooms are laid out in columns by how far they are from
//! the start - the game literally reads left to right - and clicking one
//! opens it, because a map you cannot travel by is a poster.

use std::collections::{BTreeMap, HashMap, VecDeque};

use infengine_adventure::story::{LinkKind, NodeKind, Story};

/// Grid spacing, in canvas units.
const COL_STEP: f32 = 170.0;
const ROW_STEP: f32 = 26.0;
/// One node's box.
const NODE: egui::Vec2 = egui::vec2(150.0, 20.0);

/// What the map asked for.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StoryActions {
    /// A room the user clicked, to open.
    pub open_room: Option<String>,
}

/// The window's own state.
#[derive(Debug, Clone)]
pub struct StoryUi {
    pub open: bool,
    pan: egui::Vec2,
    zoom: f32,
    filter: String,
}

impl Default for StoryUi {
    fn default() -> Self {
        Self {
            open: false,
            pan: egui::vec2(20.0, 20.0),
            zoom: 1.0,
            filter: String::new(),
        }
    }
}

/// Column and row for every node: breadth-first from the start room, so
/// distance from the start reads left to right.
///
/// Rooms nothing reaches go in the columns after the furthest reachable
/// one, grouped rather than scattered - a shelf of them at the edge of
/// the map says "these are cut off" at a glance.
fn grid(story: &Story, start: &str) -> HashMap<String, (usize, usize)> {
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
    let mut out = HashMap::new();
    // Node order is the story's own, which is sorted - so the layout is
    // stable between openings instead of shuffling with a hash map.
    for node in &story.nodes {
        let column = depth
            .get(&node.id)
            .copied()
            .unwrap_or_else(|| deepest + if node.kind == NodeKind::Screen { 2 } else { 1 });
        let row = rows.entry(column).or_insert(0);
        out.insert(node.id.clone(), (column, *row));
        *row += 1;
    }
    out
}

/// Draw the map. Returns what was clicked.
pub fn window(
    ctx: &egui::Context,
    state: &mut StoryUi,
    story: &Story,
    start: &str,
) -> StoryActions {
    let mut actions = StoryActions::default();
    let mut open = state.open;
    egui::Window::new("Story map")
        .open(&mut open)
        .default_size(egui::vec2(900.0, 600.0))
        .show(ctx, |ui| {
            header(ui, state, story, start);
            ui.separator();
            canvas(ui, state, story, start, &mut actions);
        });
    state.open = open;
    actions
}

fn header(ui: &mut egui::Ui, state: &mut StoryUi, story: &Story, start: &str) {
    ui.horizontal(|ui| {
        ui.label(format!(
            "{} places, {} ways between them",
            story.nodes.len(),
            story.links.len()
        ));
        ui.add(
            egui::Slider::new(&mut state.zoom, 0.3..=2.0)
                .show_value(false)
                .text("zoom"),
        );
        ui.label("find");
        ui.add(egui::TextEdit::singleline(&mut state.filter).desired_width(120.0));
        if !state.filter.is_empty() && ui.small_button("clear").clicked() {
            state.filter.clear();
        }
    });
    ui.horizontal(|ui| {
        legend(ui, egui::Color32::from_rgb(255, 210, 90), "door");
        legend(ui, egui::Color32::from_rgb(120, 200, 255), "script");
        legend(ui, egui::Color32::from_rgb(150, 230, 150), "button");
        let lost = story.unreachable(start).len();
        if lost > 0 {
            ui.label(
                egui::RichText::new(format!("{lost} places nothing leads to"))
                    .small()
                    .color(egui::Color32::from_rgb(255, 160, 140)),
            );
        }
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
    start: &str,
    actions: &mut StoryActions,
) {
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::drag());
    // Dragging the background is the pan; the nodes take their clicks
    // first, so the two do not fight.
    state.pan += response.drag_delta();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(18));

    let layout = grid(story, start);
    let zoom = state.zoom;
    let place = |id: &str| -> Option<egui::Rect> {
        let (column, row) = layout.get(id)?;
        let at = egui::vec2(*column as f32 * COL_STEP, *row as f32 * ROW_STEP);
        Some(egui::Rect::from_min_size(
            rect.min + (at + state.pan) * zoom,
            NODE * zoom,
        ))
    };

    // Edges first, nodes on top of them.
    for link in &story.links {
        let (Some(from), Some(to)) = (place(&link.from), place(&link.to)) else {
            continue;
        };
        let colour = match link.kind {
            LinkKind::Exit => egui::Color32::from_rgba_unmultiplied(255, 210, 90, 70),
            LinkKind::Script => egui::Color32::from_rgba_unmultiplied(120, 200, 255, 70),
            LinkKind::Button => egui::Color32::from_rgba_unmultiplied(150, 230, 150, 70),
        };
        painter.line_segment(
            [from.right_center(), to.left_center()],
            egui::Stroke::new(1.0, colour),
        );
    }

    let filter = state.filter.to_ascii_lowercase();
    for node in &story.nodes {
        let Some(box_) = place(&node.id) else { continue };
        if !rect.intersects(box_) {
            continue;
        }
        let dimmed = !filter.is_empty() && !node.id.to_ascii_lowercase().contains(&filter);
        let is_start = node.id == start;
        let fill = if is_start {
            egui::Color32::from_rgb(90, 140, 90)
        } else if node.kind == NodeKind::Screen {
            egui::Color32::from_rgb(105, 80, 130)
        } else {
            egui::Color32::from_rgb(55, 70, 95)
        };
        let fill = if dimmed { fill.gamma_multiply(0.25) } else { fill };
        painter.rect_filled(box_, 3.0, fill);
        painter.text(
            box_.left_center() + egui::vec2(4.0 * zoom, 0.0),
            egui::Align2::LEFT_CENTER,
            &node.id,
            egui::FontId::proportional((12.0 * zoom).max(7.0)),
            if dimmed {
                egui::Color32::from_gray(120)
            } else {
                egui::Color32::from_gray(230)
            },
        );

        let hit = ui.interact(
            box_,
            egui::Id::new(("story_node", &node.id)),
            egui::Sense::click(),
        );
        if hit.clicked() && node.kind == NodeKind::Room {
            actions.open_room = Some(node.id.clone());
        }
        if hit.hovered() {
            // Where you can go from here, which is the question a map
            // node gets pointed at.
            let ways: Vec<String> = story
                .out_of(&node.id)
                .iter()
                .map(|link| format!("{} \u{2192} {}", link.kind.label(), link.to))
                .collect();
            let text = if ways.is_empty() {
                "leads nowhere".to_string()
            } else {
                ways.join("\n")
            };
            hit.on_hover_text(text);
        }
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
        let layout = grid(&story(), "a");
        assert_eq!(layout["a"].0, 0);
        assert_eq!(layout["b"].0, 1);
    }

    #[test]
    fn a_room_nothing_reaches_sits_past_the_furthest_reachable_one() {
        // Grouped at the edge rather than scattered: a shelf of cut-off
        // rooms says what it is at a glance.
        let layout = grid(&story(), "a");
        assert!(layout["island"].0 > layout["b"].0);
    }

    #[test]
    fn a_start_the_story_does_not_have_still_lays_everything_out() {
        let layout = grid(&story(), "nowhere");
        assert_eq!(layout.len(), 3, "every node still gets a place");
    }
}
