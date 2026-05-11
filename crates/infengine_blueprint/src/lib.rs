//! Blueprint (visual scripting) graph scaffold.
//!
//! Phase 10.6 lays the foundation for an Unreal-style node graph
//! editor: a serialisable data model and a placeholder egui panel so
//! the rest of the engine can route the user to the future editor.
//! Node execution (compiler -> Rust closures, or VM, or C# code-gen)
//! lands in a later phase together with a richer node catalogue.

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
        if !self.nodes.iter().any(|n| n.id == from_node) {
            return false;
        }
        if !self.nodes.iter().any(|n| n.id == to_node) {
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

/// Minimal egui-based blueprint editor. Renders nodes as draggable
/// rectangles with input/output pin labels; clicking "Add node" opens
/// a kind picker. Edge editing is a placeholder list at this stage -
/// the polished spline canvas arrives in a later phase, possibly built
/// on top of `egui-snarl`.
#[derive(Debug, Default)]
pub struct BlueprintEditor {
    pub graph: Graph,
    /// Node currently being dragged on the canvas (egui pointer-state
    /// snapshot).
    dragging: Option<NodeId>,
    drag_offset: [f32; 2],
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

    /// Render the editor into the given `ui`. Returns `true` when the
    /// graph changed this frame so the caller can persist.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        let mut dirty = false;
        ui.horizontal(|ui| {
            ui.label(format!("Nodes: {}", self.graph.nodes.len()));
            ui.label(format!("Edges: {}", self.graph.edges.len()));
            ui.menu_button("Add node", |ui| {
                for kind in [
                    NodeKind::OnStart,
                    NodeKind::OnTick,
                    NodeKind::ConstFloat,
                    NodeKind::Add,
                    NodeKind::Print,
                    NodeKind::SetPosition,
                ] {
                    if ui.button(kind.label()).clicked() {
                        self.graph.add_node(kind, [40.0, 40.0]);
                        dirty = true;
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        let canvas = ui.available_size_before_wrap();
        let (rect, response) = ui.allocate_exact_size(canvas, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(28));

        let pointer = response.interact_pointer_pos();
        if response.drag_started()
            && let Some(pos) = pointer
        {
            for node in self.graph.nodes.iter().rev() {
                let node_rect = egui::Rect::from_min_size(
                    rect.min + egui::vec2(node.position[0], node.position[1]),
                    egui::vec2(140.0, 60.0),
                );
                if node_rect.contains(pos) {
                    self.dragging = Some(node.id);
                    self.drag_offset = [
                        pos.x - rect.min.x - node.position[0],
                        pos.y - rect.min.y - node.position[1],
                    ];
                    break;
                }
            }
        }
        if response.dragged()
            && let (Some(id), Some(pos)) = (self.dragging, pointer)
            && let Some(node) = self.graph.nodes.iter_mut().find(|n| n.id == id)
        {
            node.position[0] = (pos.x - rect.min.x - self.drag_offset[0]).max(0.0);
            node.position[1] = (pos.y - rect.min.y - self.drag_offset[1]).max(0.0);
            dirty = true;
        }
        if response.drag_stopped() {
            self.dragging = None;
        }

        for node in &self.graph.nodes {
            let origin = rect.min + egui::vec2(node.position[0], node.position[1]);
            let node_rect = egui::Rect::from_min_size(origin, egui::vec2(140.0, 60.0));
            painter.rect_filled(node_rect, 4.0, egui::Color32::from_rgb(48, 56, 72));
            painter.rect_stroke(
                node_rect,
                4.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
                egui::StrokeKind::Inside,
            );
            painter.text(
                origin + egui::vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                node.kind.label(),
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
            let inputs = node.kind.input_pins();
            let outputs = node.kind.output_pins();
            for (i, pin) in inputs.iter().enumerate() {
                painter.circle_filled(
                    origin + egui::vec2(0.0, 26.0 + i as f32 * 12.0),
                    3.0,
                    egui::Color32::from_rgb(120, 200, 255),
                );
                painter.text(
                    origin + egui::vec2(6.0, 22.0 + i as f32 * 12.0),
                    egui::Align2::LEFT_TOP,
                    *pin,
                    egui::FontId::proportional(10.0),
                    egui::Color32::LIGHT_BLUE,
                );
            }
            for (i, pin) in outputs.iter().enumerate() {
                painter.circle_filled(
                    origin + egui::vec2(140.0, 26.0 + i as f32 * 12.0),
                    3.0,
                    egui::Color32::from_rgb(255, 200, 120),
                );
                painter.text(
                    origin + egui::vec2(134.0, 22.0 + i as f32 * 12.0),
                    egui::Align2::RIGHT_TOP,
                    *pin,
                    egui::FontId::proportional(10.0),
                    egui::Color32::LIGHT_YELLOW,
                );
            }
        }

        for edge in &self.graph.edges {
            let from = self.graph.nodes.iter().find(|n| n.id == edge.from_node);
            let to = self.graph.nodes.iter().find(|n| n.id == edge.to_node);
            if let (Some(f), Some(t)) = (from, to) {
                let a = rect.min + egui::vec2(f.position[0] + 140.0, f.position[1] + 32.0);
                let b = rect.min + egui::vec2(t.position[0], t.position[1] + 32.0);
                painter.line_segment(
                    [a, b],
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(200, 200, 200)),
                );
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
}
