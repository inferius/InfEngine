//! Blueprint (visual scripting) graph scaffold.
//!
//! Phase 10.6 introduced the data model + a minimal egui canvas. Phase
//! 13B grows the editor into a usable surface: canvas panning,
//! drag-pin-to-pin edge creation, bezier-spline rendering, right-click
//! context menus, and Delete-key cleanup. Execution (bytecode VM)
//! lands in 13C.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Stable numeric id of a node inside a graph.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// Built-in node catalogue. Each variant carries the data needed at
/// edit time; runtime parameters live on the node's `params` map so
/// future kinds can grow without breaking saved graphs.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Fires once when the owning entity enters Play mode.
    OnStart,
    /// Fires every frame while the entity is alive.
    OnTick,
    /// Constant float value exposed on the `value` output pin.
    ConstFloat,
    /// Two-input addition (`a`, `b`) producing one output (`sum`).
    Add,
    /// Sink: writes its input to the engine log.
    Print,
    /// Moves the owning entity by the input vector each tick.
    SetPosition,
}

impl NodeKind {
    pub fn label(self) -> &'static str {
        match self {
            NodeKind::OnStart => "On Start",
            NodeKind::OnTick => "On Tick",
            NodeKind::ConstFloat => "Const Float",
            NodeKind::Add => "Add",
            NodeKind::Print => "Print",
            NodeKind::SetPosition => "Set Position",
        }
    }

    pub fn all() -> &'static [NodeKind] {
        &[
            NodeKind::OnStart,
            NodeKind::OnTick,
            NodeKind::ConstFloat,
            NodeKind::Add,
            NodeKind::Print,
            NodeKind::SetPosition,
        ]
    }

    pub fn input_pins(self) -> &'static [&'static str] {
        match self {
            NodeKind::OnStart | NodeKind::OnTick | NodeKind::ConstFloat => &[],
            NodeKind::Add => &["a", "b"],
            NodeKind::Print => &["value"],
            NodeKind::SetPosition => &["x", "y", "z"],
        }
    }

    pub fn output_pins(self) -> &'static [&'static str] {
        match self {
            NodeKind::OnStart | NodeKind::OnTick => &["exec"],
            NodeKind::ConstFloat => &["value"],
            NodeKind::Add => &["sum"],
            NodeKind::Print | NodeKind::SetPosition => &[],
        }
    }
}

/// One node in the graph. `position` keeps the edit-time canvas layout
/// stable across saves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub position: [f32; 2],
    /// Stringly-typed parameter slots so node kinds can carry literal
    /// values (e.g. the constant for `ConstFloat`) without per-kind
    /// fields in the saved file.
    #[serde(default)]
    pub params: HashMap<String, String>,
}

impl Node {
    pub fn new(id: NodeId, kind: NodeKind, position: [f32; 2]) -> Self {
        Self {
            id,
            kind,
            position,
            params: HashMap::new(),
        }
    }
}

/// Edge from `(from_node, from_pin)` to `(to_node, to_pin)`. Pin names
/// must match the entries in `NodeKind::input_pins`/`output_pins`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from_node: NodeId,
    pub from_pin: String,
    pub to_node: NodeId,
    pub to_pin: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub next_id: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("graph file {path} not found: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("graph file {path} parse error: {source}")]
    Parse {
        path: PathBuf,
        source: ron::error::SpannedError,
    },
    #[error("graph file {path} serialize error: {source}")]
    Serialize {
        path: PathBuf,
        source: ron::error::Error,
    },
}

impl Graph {
    pub fn add_node(&mut self, kind: NodeKind, position: [f32; 2]) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(Node::new(id, kind, position));
        id
    }

    pub fn remove_node(&mut self, id: NodeId) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from_node != id && e.to_node != id);
    }

    pub fn connect(
        &mut self,
        from_node: NodeId,
        from_pin: &str,
        to_node: NodeId,
        to_pin: &str,
    ) -> bool {
        if from_node == to_node {
            return false;
        }
        if !self.nodes.iter().any(|n| n.id == from_node) {
            return false;
        }
        if !self.nodes.iter().any(|n| n.id == to_node) {
            return false;
        }
        // Prevent duplicate edges on the same source/target pin pair.
        if self.edges.iter().any(|e| {
            e.from_node == from_node
                && e.from_pin == from_pin
                && e.to_node == to_node
                && e.to_pin == to_pin
        }) {
            return false;
        }
        self.edges.push(Edge {
            from_node,
            from_pin: from_pin.to_owned(),
            to_node,
            to_pin: to_pin.to_owned(),
        });
        true
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, GraphError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| GraphError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        ron::de::from_str(&text).map_err(|source| GraphError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), GraphError> {
        let path = path.as_ref();
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(
            |source| GraphError::Serialize {
                path: path.to_path_buf(),
                source,
            },
        )?;
        std::fs::write(path, text).map_err(|source| GraphError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

const NODE_WIDTH: f32 = 160.0;
const NODE_HEADER_HEIGHT: f32 = 26.0;
const PIN_ROW_HEIGHT: f32 = 16.0;
const PIN_RADIUS: f32 = 5.0;
const PIN_HIT_RADIUS: f32 = 9.0;

fn node_height(node: &Node) -> f32 {
    let rows = node
        .kind
        .input_pins()
        .len()
        .max(node.kind.output_pins().len()) as f32;
    NODE_HEADER_HEIGHT + rows * PIN_ROW_HEIGHT + 8.0
}

/// Drag of a pin in progress. `is_output` distinguishes the side the
/// drag started on so we know whether to look for an input pin (when
/// `is_output == true`) or an output pin (when false) on release.
#[derive(Debug, Clone)]
struct PendingEdge {
    node: NodeId,
    pin: String,
    is_output: bool,
}

/// Egui-based blueprint editor. Phase 13B adds pan + drag-connect +
/// context menu + bezier edge rendering on top of the original
/// scaffold.
#[derive(Debug, Default)]
pub struct BlueprintEditor {
    pub graph: Graph,
    /// Canvas pan offset in screen pixels.
    view_offset: egui::Vec2,
    /// Currently-dragged node (left mouse on node body).
    dragging: Option<NodeId>,
    drag_node_offset: egui::Vec2,
    /// In-flight pin connection drag.
    pending_edge: Option<PendingEdge>,
    /// Currently-selected node for Delete-key removal.
    selected: Option<NodeId>,
    /// Right-click context menu spawn position (canvas-space) or `None`
    /// when no menu is open.
    context_menu_at: Option<egui::Pos2>,
}

impl BlueprintEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_graph(graph: Graph) -> Self {
        Self {
            graph,
            ..Self::default()
        }
    }

    fn node_origin(&self, canvas_min: egui::Pos2, node: &Node) -> egui::Pos2 {
        canvas_min + self.view_offset + egui::vec2(node.position[0], node.position[1])
    }

    fn node_rect(&self, canvas_min: egui::Pos2, node: &Node) -> egui::Rect {
        let origin = self.node_origin(canvas_min, node);
        egui::Rect::from_min_size(origin, egui::vec2(NODE_WIDTH, node_height(node)))
    }

    fn pin_position(
        &self,
        canvas_min: egui::Pos2,
        node: &Node,
        pin: &str,
        is_output: bool,
    ) -> Option<egui::Pos2> {
        let origin = self.node_origin(canvas_min, node);
        let pins = if is_output {
            node.kind.output_pins()
        } else {
            node.kind.input_pins()
        };
        let idx = pins.iter().position(|p| *p == pin)?;
        let y = origin.y + NODE_HEADER_HEIGHT + idx as f32 * PIN_ROW_HEIGHT + PIN_ROW_HEIGHT * 0.5;
        let x = if is_output {
            origin.x + NODE_WIDTH
        } else {
            origin.x
        };
        Some(egui::pos2(x, y))
    }

    fn hit_pin(&self, canvas_min: egui::Pos2, pos: egui::Pos2) -> Option<(NodeId, String, bool)> {
        for node in self.graph.nodes.iter().rev() {
            for is_output in [true, false] {
                let pins = if is_output {
                    node.kind.output_pins()
                } else {
                    node.kind.input_pins()
                };
                for pin in pins {
                    if let Some(p) = self.pin_position(canvas_min, node, pin, is_output)
                        && pos.distance(p) <= PIN_HIT_RADIUS
                    {
                        return Some((node.id, (*pin).to_string(), is_output));
                    }
                }
            }
        }
        None
    }

    fn hit_node(&self, canvas_min: egui::Pos2, pos: egui::Pos2) -> Option<NodeId> {
        for node in self.graph.nodes.iter().rev() {
            if self.node_rect(canvas_min, node).contains(pos) {
                return Some(node.id);
            }
        }
        None
    }

    fn draw_edge(
        painter: &egui::Painter,
        a: egui::Pos2,
        b: egui::Pos2,
        color: egui::Color32,
        stroke_width: f32,
    ) {
        // Cubic bezier with horizontal tangents - mimics Unreal /
        // Blender node graph cosmetics. Sample 16 segments for a smooth
        // curve without burning the painter.
        let dx = (b.x - a.x).abs().max(40.0) * 0.5;
        let c1 = egui::pos2(a.x + dx, a.y);
        let c2 = egui::pos2(b.x - dx, b.y);
        let mut prev = a;
        let steps = 24;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let one = 1.0 - t;
            let x = one * one * one * a.x
                + 3.0 * one * one * t * c1.x
                + 3.0 * one * t * t * c2.x
                + t * t * t * b.x;
            let y = one * one * one * a.y
                + 3.0 * one * one * t * c1.y
                + 3.0 * one * t * t * c2.y
                + t * t * t * b.y;
            let p = egui::pos2(x, y);
            painter.line_segment([prev, p], egui::Stroke::new(stroke_width, color));
            prev = p;
        }
    }

    /// Render the editor into the given `ui`. Returns `true` when the
    /// graph changed this frame so the caller can persist.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        let mut dirty = false;

        // Top bar: stats + "Add node" menu (kept for accessibility, the
        // canvas right-click menu duplicates it).
        ui.horizontal(|ui| {
            ui.label(format!("Nodes: {}", self.graph.nodes.len()));
            ui.label(format!("Edges: {}", self.graph.edges.len()));
            ui.menu_button("Add node", |ui| {
                for kind in NodeKind::all() {
                    if ui.button(kind.label()).clicked() {
                        let canvas_pos = -self.view_offset + egui::vec2(40.0, 40.0);
                        self.graph.add_node(*kind, [canvas_pos.x, canvas_pos.y]);
                        dirty = true;
                        ui.close();
                    }
                }
            });
            if ui.button("Reset view").clicked() {
                self.view_offset = egui::Vec2::ZERO;
            }
            ui.label("LMB drag pin -> create edge. MMB / RMB drag = pan. Delete to remove.");
        });
        ui.separator();

        let canvas = ui.available_size_before_wrap();
        let (rect, response) = ui.allocate_exact_size(canvas, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(28));

        // Grid lines for visual feedback when panning.
        let grid_step = 40.0;
        let grid_color = egui::Color32::from_gray(40);
        let ox = self.view_offset.x.rem_euclid(grid_step);
        let oy = self.view_offset.y.rem_euclid(grid_step);
        let mut x = rect.min.x + ox;
        while x < rect.max.x {
            painter.line_segment(
                [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                egui::Stroke::new(1.0, grid_color),
            );
            x += grid_step;
        }
        let mut y = rect.min.y + oy;
        while y < rect.max.y {
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                egui::Stroke::new(1.0, grid_color),
            );
            y += grid_step;
        }

        let pointer = response.interact_pointer_pos();
        let hover = response.hover_pos();
        let primary_down = ui.input(|i| i.pointer.primary_down());
        let primary_just_pressed = response.drag_started_by(egui::PointerButton::Primary)
            || (response.clicked_by(egui::PointerButton::Primary) && pointer.is_some());
        let primary_released = response.drag_stopped_by(egui::PointerButton::Primary)
            || response.clicked_by(egui::PointerButton::Primary);

        // Pan with middle or secondary mouse button drag.
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            self.view_offset += response.drag_delta();
        }

        // Start interaction on left-click: prefer pin > node body.
        if primary_just_pressed && let Some(pos) = pointer {
            if let Some((node_id, pin, is_output)) = self.hit_pin(rect.min, pos) {
                self.pending_edge = Some(PendingEdge {
                    node: node_id,
                    pin,
                    is_output,
                });
                self.dragging = None;
            } else if let Some(node_id) = self.hit_node(rect.min, pos) {
                self.dragging = Some(node_id);
                self.selected = Some(node_id);
                if let Some(node) = self.graph.nodes.iter().find(|n| n.id == node_id) {
                    let origin = self.node_origin(rect.min, node);
                    self.drag_node_offset = pos - origin;
                }
            } else {
                self.selected = None;
            }
        }

        // Drag node body.
        if primary_down
            && let Some(id) = self.dragging
            && let Some(pos) = pointer
            && let Some(node) = self.graph.nodes.iter_mut().find(|n| n.id == id)
        {
            let local = pos - rect.min - self.view_offset - self.drag_node_offset;
            node.position[0] = local.x;
            node.position[1] = local.y;
            dirty = true;
        }

        // Drop pin -> attempt to create edge.
        if primary_released {
            self.dragging = None;
            if let (Some(pending), Some(pos)) = (self.pending_edge.take(), pointer)
                && let Some((target_node, target_pin, target_is_output)) =
                    self.hit_pin(rect.min, pos)
                && target_is_output != pending.is_output
            {
                let (from_node, from_pin, to_node, to_pin) = if pending.is_output {
                    (pending.node, pending.pin.clone(), target_node, target_pin)
                } else {
                    (target_node, target_pin, pending.node, pending.pin.clone())
                };
                if self.graph.connect(from_node, &from_pin, to_node, &to_pin) {
                    dirty = true;
                }
            }
        }

        // Context menu (right-click on empty canvas only - right-drag is
        // pan above, so this only fires when the user releases without
        // dragging).
        if response.clicked_by(egui::PointerButton::Secondary)
            && let Some(pos) = hover
            && self.hit_pin(rect.min, pos).is_none()
            && self.hit_node(rect.min, pos).is_none()
        {
            self.context_menu_at = Some(pos);
        }

        // Delete key removes the selection.
        let delete_pressed = ui.input(|i| i.key_pressed(egui::Key::Delete));
        if delete_pressed && let Some(id) = self.selected.take() {
            self.graph.remove_node(id);
            dirty = true;
        }

        // Draw edges first so nodes paint on top.
        for edge in &self.graph.edges {
            let from = self.graph.nodes.iter().find(|n| n.id == edge.from_node);
            let to = self.graph.nodes.iter().find(|n| n.id == edge.to_node);
            if let (Some(f), Some(t)) = (from, to)
                && let (Some(a), Some(b)) = (
                    self.pin_position(rect.min, f, &edge.from_pin, true),
                    self.pin_position(rect.min, t, &edge.to_pin, false),
                )
            {
                Self::draw_edge(&painter, a, b, egui::Color32::from_rgb(200, 200, 200), 2.0);
            }
        }

        // Draw the in-progress pin drag as a ghost edge.
        if let (Some(pending), Some(pos)) = (self.pending_edge.as_ref(), hover)
            && let Some(node) = self.graph.nodes.iter().find(|n| n.id == pending.node)
            && let Some(anchor) = self.pin_position(rect.min, node, &pending.pin, pending.is_output)
        {
            let (a, b) = if pending.is_output {
                (anchor, pos)
            } else {
                (pos, anchor)
            };
            Self::draw_edge(&painter, a, b, egui::Color32::from_rgb(255, 220, 80), 2.0);
        }

        // Draw nodes.
        for node in &self.graph.nodes {
            let origin = self.node_origin(rect.min, node);
            let rect_node =
                egui::Rect::from_min_size(origin, egui::vec2(NODE_WIDTH, node_height(node)));
            painter.rect_filled(rect_node, 4.0, egui::Color32::from_rgb(48, 56, 72));
            let selected = self.selected == Some(node.id);
            let border = if selected {
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 200, 80))
            } else {
                egui::Stroke::new(1.0, egui::Color32::from_gray(120))
            };
            painter.rect_stroke(rect_node, 4.0, border, egui::StrokeKind::Inside);
            // Header strip.
            let header_rect =
                egui::Rect::from_min_size(origin, egui::vec2(NODE_WIDTH, NODE_HEADER_HEIGHT));
            painter.rect_filled(header_rect, 4.0, egui::Color32::from_rgb(72, 84, 108));
            painter.text(
                origin + egui::vec2(8.0, 6.0),
                egui::Align2::LEFT_TOP,
                node.kind.label(),
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
            for (i, pin) in node.kind.input_pins().iter().enumerate() {
                let y = origin.y
                    + NODE_HEADER_HEIGHT
                    + i as f32 * PIN_ROW_HEIGHT
                    + PIN_ROW_HEIGHT * 0.5;
                painter.circle_filled(
                    egui::pos2(origin.x, y),
                    PIN_RADIUS,
                    egui::Color32::from_rgb(120, 200, 255),
                );
                painter.text(
                    egui::pos2(origin.x + 10.0, y - 7.0),
                    egui::Align2::LEFT_TOP,
                    *pin,
                    egui::FontId::proportional(11.0),
                    egui::Color32::LIGHT_BLUE,
                );
            }
            for (i, pin) in node.kind.output_pins().iter().enumerate() {
                let y = origin.y
                    + NODE_HEADER_HEIGHT
                    + i as f32 * PIN_ROW_HEIGHT
                    + PIN_ROW_HEIGHT * 0.5;
                painter.circle_filled(
                    egui::pos2(origin.x + NODE_WIDTH, y),
                    PIN_RADIUS,
                    egui::Color32::from_rgb(255, 200, 120),
                );
                painter.text(
                    egui::pos2(origin.x + NODE_WIDTH - 10.0, y - 7.0),
                    egui::Align2::RIGHT_TOP,
                    *pin,
                    egui::FontId::proportional(11.0),
                    egui::Color32::LIGHT_YELLOW,
                );
            }
        }

        // Context-menu popup. egui's `Area` lets us float a small
        // chooser above the canvas at the click position.
        if let Some(anchor) = self.context_menu_at {
            let mut close = false;
            let mut add_kind: Option<NodeKind> = None;
            egui::Area::new(egui::Id::new("blueprint_context_menu"))
                .fixed_pos(anchor)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(140.0);
                        ui.label("Add node");
                        for kind in NodeKind::all() {
                            if ui.button(kind.label()).clicked() {
                                add_kind = Some(*kind);
                                close = true;
                            }
                        }
                        ui.separator();
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            if let Some(kind) = add_kind {
                let canvas_pos = anchor - rect.min - self.view_offset;
                self.graph.add_node(kind, [canvas_pos.x, canvas_pos.y]);
                dirty = true;
            }
            if close
                || ui.input(|i| i.key_pressed(egui::Key::Escape))
                || (response.clicked_by(egui::PointerButton::Primary)
                    && response.hovered()
                    && self.context_menu_at.is_some())
            {
                self.context_menu_at = None;
            }
        }

        dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_serde() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::OnTick, [10.0, 10.0]);
        let b = g.add_node(NodeKind::Print, [200.0, 10.0]);
        assert!(g.connect(a, "exec", b, "value"));
        let s = ron::ser::to_string(&g).unwrap();
        let back: Graph = ron::de::from_str(&s).unwrap();
        assert_eq!(back.nodes.len(), 2);
        assert_eq!(back.edges.len(), 1);
    }

    #[test]
    fn duplicate_connect_rejected() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::OnTick, [0.0, 0.0]);
        let b = g.add_node(NodeKind::Print, [0.0, 0.0]);
        assert!(g.connect(a, "exec", b, "value"));
        assert!(!g.connect(a, "exec", b, "value"));
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn self_connect_rejected() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::Add, [0.0, 0.0]);
        assert!(!g.connect(a, "sum", a, "a"));
    }
}
