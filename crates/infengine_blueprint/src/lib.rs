//! Blueprint (visual scripting) graph for InfEngine.
//!
//! Phase 15 reworks the original enum-based node kinds into a runtime
//! registry of typed node definitions. Each pin carries a `PinType`
//! that the graph validates at edge-create time, so the editor
//! visually distinguishes execution flow from data and refuses to
//! plug a Float into an Exec slot. Node kinds are addressed by
//! string ids registered in a `NodeKindRegistry`, which the host
//! application can extend at startup to add domain-specific nodes
//! without touching this crate.
//!
//! `Blueprint` wraps a `Graph` together with metadata describing what
//! the script applies to (`BlueprintTarget`) so the editor can show
//! "Editing: Entity 'cube_demo'" instead of presenting an anonymous
//! canvas.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod exec;

/// Stable numeric id of a node inside a graph.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// Kind of value flowing through a pin. Two pins may be connected only
/// when their types match exactly. `Exec` is the special "execution
/// flow" type that drives node firing (Unreal-style white wires).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Exec,
    Float,
    Bool,
    Vec3,
    /// Reference to a scene entity by stable id - resolved at runtime.
    EntityRef,
    /// Plain UTF-8 string.
    Text,
}

impl PinType {
    /// Rough RGB used by the editor to colour-code pins / edges.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            PinType::Exec => [240, 240, 240],
            PinType::Float => [120, 220, 120],
            PinType::Bool => [220, 60, 60],
            PinType::Vec3 => [240, 200, 80],
            PinType::EntityRef => [80, 160, 255],
            PinType::Text => [200, 130, 255],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PinType::Exec => "exec",
            PinType::Float => "float",
            PinType::Bool => "bool",
            PinType::Vec3 => "vec3",
            PinType::EntityRef => "entity",
            PinType::Text => "text",
        }
    }
}

/// Where a node is meaningful. The editor filters its "Add node"
/// catalogue by the active blueprint's target so a global blueprint
/// doesn't offer entity-only ops like SetPosition. An empty `contexts`
/// list on the def means "available everywhere".
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintContext {
    Global,
    Entity,
    Class,
}

/// Pin metadata: pin name + type. Stored on node-kind definitions, not
/// per-node, because every node of a kind has the same pin layout.
#[derive(Debug, Clone)]
pub struct PinSpec {
    pub name: &'static str,
    pub ty: PinType,
}

impl PinSpec {
    pub const fn new(name: &'static str, ty: PinType) -> Self {
        Self { name, ty }
    }
}

/// Definition of one node kind. `id` is the stable string key serialised
/// in saved graphs (e.g. `"on_start"`). `inputs` / `outputs` describe
/// the pins in display order. `category` is the menu label the editor
/// uses to group related nodes.
#[derive(Debug, Clone)]
pub struct NodeKindDef {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub inputs: &'static [PinSpec],
    pub outputs: &'static [PinSpec],
    /// Contexts in which the editor offers this node. Empty = any.
    pub contexts: &'static [BlueprintContext],
}

impl NodeKindDef {
    pub fn input(&self, name: &str) -> Option<&'static PinSpec> {
        self.inputs.iter().find(|p| p.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&'static PinSpec> {
        self.outputs.iter().find(|p| p.name == name)
    }

    /// True when this node is available in `ctx`. Empty contexts list
    /// = "available everywhere".
    pub fn available_in(&self, ctx: BlueprintContext) -> bool {
        self.contexts.is_empty() || self.contexts.contains(&ctx)
    }
}

/// Registry mapping string kind ids to definitions. The host
/// application can register custom node kinds at startup; built-in
/// engine nodes are pre-registered via `with_builtins`.
#[derive(Debug, Default)]
pub struct NodeKindRegistry {
    defs: HashMap<&'static str, NodeKindDef>,
}

impl NodeKindRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-populated registry with the engine's built-in node kinds.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        for def in BUILTIN_NODES.iter() {
            r.register(def.clone());
        }
        r
    }

    pub fn register(&mut self, def: NodeKindDef) {
        self.defs.insert(def.id, def);
    }

    pub fn get(&self, id: &str) -> Option<&NodeKindDef> {
        self.defs.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &NodeKindDef> {
        self.defs.values()
    }
}

/// Built-in node catalogue. Static so the registry can pre-populate
/// itself in `with_builtins` without runtime allocation.
const BUILTIN_NODES: &[NodeKindDef] = &[
    NodeKindDef {
        id: "on_start",
        label: "On Start",
        category: "Events",
        inputs: &[],
        outputs: &[PinSpec::new("exec", PinType::Exec)],
        contexts: &[],
    },
    NodeKindDef {
        id: "on_tick",
        label: "On Tick",
        category: "Events",
        inputs: &[],
        outputs: &[PinSpec::new("exec", PinType::Exec)],
        contexts: &[],
    },
    NodeKindDef {
        id: "const_float",
        label: "Const Float",
        category: "Values",
        inputs: &[],
        outputs: &[PinSpec::new("value", PinType::Float)],
        contexts: &[],
    },
    NodeKindDef {
        id: "const_bool",
        label: "Const Bool",
        category: "Values",
        inputs: &[],
        outputs: &[PinSpec::new("value", PinType::Bool)],
        contexts: &[],
    },
    NodeKindDef {
        id: "const_vec3",
        label: "Const Vec3",
        category: "Values",
        inputs: &[],
        outputs: &[PinSpec::new("value", PinType::Vec3)],
        contexts: &[],
    },
    NodeKindDef {
        id: "make_vec3",
        label: "Make Vec3",
        category: "Math",
        inputs: &[
            PinSpec::new("x", PinType::Float),
            PinSpec::new("y", PinType::Float),
            PinSpec::new("z", PinType::Float),
        ],
        outputs: &[PinSpec::new("value", PinType::Vec3)],
        contexts: &[],
    },
    NodeKindDef {
        id: "add",
        label: "Add",
        category: "Math",
        inputs: &[
            PinSpec::new("a", PinType::Float),
            PinSpec::new("b", PinType::Float),
        ],
        outputs: &[PinSpec::new("sum", PinType::Float)],
        contexts: &[],
    },
    NodeKindDef {
        id: "subtract",
        label: "Subtract",
        category: "Math",
        inputs: &[
            PinSpec::new("a", PinType::Float),
            PinSpec::new("b", PinType::Float),
        ],
        outputs: &[PinSpec::new("diff", PinType::Float)],
        contexts: &[],
    },
    NodeKindDef {
        id: "multiply",
        label: "Multiply",
        category: "Math",
        inputs: &[
            PinSpec::new("a", PinType::Float),
            PinSpec::new("b", PinType::Float),
        ],
        outputs: &[PinSpec::new("product", PinType::Float)],
        contexts: &[],
    },
    NodeKindDef {
        id: "divide",
        label: "Divide",
        category: "Math",
        inputs: &[
            PinSpec::new("a", PinType::Float),
            PinSpec::new("b", PinType::Float),
        ],
        outputs: &[PinSpec::new("quotient", PinType::Float)],
        contexts: &[],
    },
    NodeKindDef {
        id: "equal_float",
        label: "Equal (float)",
        category: "Comparison",
        inputs: &[
            PinSpec::new("a", PinType::Float),
            PinSpec::new("b", PinType::Float),
        ],
        outputs: &[PinSpec::new("value", PinType::Bool)],
        contexts: &[],
    },
    NodeKindDef {
        id: "less_than",
        label: "Less Than (float)",
        category: "Comparison",
        inputs: &[
            PinSpec::new("a", PinType::Float),
            PinSpec::new("b", PinType::Float),
        ],
        outputs: &[PinSpec::new("value", PinType::Bool)],
        contexts: &[],
    },
    NodeKindDef {
        id: "greater_than",
        label: "Greater Than (float)",
        category: "Comparison",
        inputs: &[
            PinSpec::new("a", PinType::Float),
            PinSpec::new("b", PinType::Float),
        ],
        outputs: &[PinSpec::new("value", PinType::Bool)],
        contexts: &[],
    },
    NodeKindDef {
        id: "and",
        label: "AND",
        category: "Logic",
        inputs: &[
            PinSpec::new("a", PinType::Bool),
            PinSpec::new("b", PinType::Bool),
        ],
        outputs: &[PinSpec::new("value", PinType::Bool)],
        contexts: &[],
    },
    NodeKindDef {
        id: "or",
        label: "OR",
        category: "Logic",
        inputs: &[
            PinSpec::new("a", PinType::Bool),
            PinSpec::new("b", PinType::Bool),
        ],
        outputs: &[PinSpec::new("value", PinType::Bool)],
        contexts: &[],
    },
    NodeKindDef {
        id: "not",
        label: "NOT",
        category: "Logic",
        inputs: &[PinSpec::new("value", PinType::Bool)],
        outputs: &[PinSpec::new("value", PinType::Bool)],
        contexts: &[],
    },
    NodeKindDef {
        // Multi-output exec node. The compiler walks `then_0` through
        // `then_3` in order so chained sinks run top-to-bottom from a
        // single trigger.
        id: "sequence",
        label: "Sequence",
        category: "Control",
        inputs: &[PinSpec::new("in", PinType::Exec)],
        outputs: &[
            PinSpec::new("then_0", PinType::Exec),
            PinSpec::new("then_1", PinType::Exec),
            PinSpec::new("then_2", PinType::Exec),
            PinSpec::new("then_3", PinType::Exec),
        ],
        contexts: &[],
    },
    NodeKindDef {
        id: "print",
        label: "Print",
        category: "Debug",
        inputs: &[
            PinSpec::new("in", PinType::Exec),
            PinSpec::new("value", PinType::Float),
        ],
        outputs: &[],
        contexts: &[],
    },
    NodeKindDef {
        id: "get_float_var",
        label: "Get Float Variable",
        category: "Variables",
        inputs: &[],
        outputs: &[PinSpec::new("value", PinType::Float)],
        contexts: &[],
    },
    NodeKindDef {
        id: "set_float_var",
        label: "Set Float Variable",
        category: "Variables",
        inputs: &[
            PinSpec::new("in", PinType::Exec),
            PinSpec::new("value", PinType::Float),
        ],
        outputs: &[],
        contexts: &[],
    },
    NodeKindDef {
        id: "branch",
        label: "Branch",
        category: "Control",
        inputs: &[
            PinSpec::new("in", PinType::Exec),
            PinSpec::new("condition", PinType::Bool),
        ],
        outputs: &[
            PinSpec::new("true", PinType::Exec),
            PinSpec::new("false", PinType::Exec),
        ],
        contexts: &[],
    },
    NodeKindDef {
        id: "set_position",
        label: "Set Position",
        category: "Transform",
        inputs: &[
            PinSpec::new("in", PinType::Exec),
            PinSpec::new("x", PinType::Float),
            PinSpec::new("y", PinType::Float),
            PinSpec::new("z", PinType::Float),
        ],
        outputs: &[],
        // Entity-only - rejects in a Global blueprint.
        contexts: &[BlueprintContext::Entity, BlueprintContext::Class],
    },
    NodeKindDef {
        id: "set_rotation",
        label: "Set Rotation",
        category: "Transform",
        inputs: &[
            PinSpec::new("in", PinType::Exec),
            PinSpec::new("x", PinType::Float),
            PinSpec::new("y", PinType::Float),
            PinSpec::new("z", PinType::Float),
        ],
        outputs: &[],
        contexts: &[BlueprintContext::Entity, BlueprintContext::Class],
    },
    NodeKindDef {
        id: "set_scale",
        label: "Set Scale",
        category: "Transform",
        inputs: &[
            PinSpec::new("in", PinType::Exec),
            PinSpec::new("x", PinType::Float),
            PinSpec::new("y", PinType::Float),
            PinSpec::new("z", PinType::Float),
        ],
        outputs: &[],
        contexts: &[BlueprintContext::Entity, BlueprintContext::Class],
    },
    NodeKindDef {
        // Visual-only annotation. Has no pins; the renderer special-
        // cases this kind to paint a larger translucent box with a
        // text label stored in `params["text"]`. Never participates
        // in compile.
        id: "comment",
        label: "Comment",
        category: "Annotations",
        inputs: &[],
        outputs: &[],
        contexts: &[],
    },
    NodeKindDef {
        // `params["path"]` carries the asset path; the host resolves
        // it against the project root and feeds it to the audio
        // backend.
        id: "play_sound",
        label: "Play Sound",
        category: "Audio",
        inputs: &[PinSpec::new("in", PinType::Exec)],
        outputs: &[],
        contexts: &[],
    },
    NodeKindDef {
        id: "delta_time",
        label: "Delta Time",
        category: "Time",
        inputs: &[],
        outputs: &[PinSpec::new("value", PinType::Float)],
        contexts: &[],
    },
    NodeKindDef {
        id: "time_seconds",
        label: "Time Seconds",
        category: "Time",
        inputs: &[],
        outputs: &[PinSpec::new("value", PinType::Float)],
        contexts: &[],
    },
    NodeKindDef {
        // The function name is stored in `params["name"]`; the
        // compiler inlines the named function's body (matched against
        // `Blueprint::functions`) and skips recursion to keep the
        // emitted program finite.
        id: "call_function",
        label: "Call Function",
        category: "Functions",
        inputs: &[PinSpec::new("in", PinType::Exec)],
        outputs: &[PinSpec::new("exec", PinType::Exec)],
        contexts: &[],
    },
];

/// One node in the graph. `kind` references a `NodeKindDef::id` in
/// the registry, so saved graphs survive across renames / refactors
/// of the static enum used by old phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: String,
    pub position: [f32; 2],
    /// Stringly-typed parameter slots so node kinds can carry literal
    /// values (e.g. the constant for `const_float`) without per-kind
    /// fields in the saved file.
    #[serde(default)]
    pub params: HashMap<String, String>,
}

impl Node {
    pub fn new(id: NodeId, kind: impl Into<String>, position: [f32; 2]) -> Self {
        Self {
            id,
            kind: kind.into(),
            position,
            params: HashMap::new(),
        }
    }
}

/// Edge from `(from_node, from_pin)` to `(to_node, to_pin)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from_node: NodeId,
    pub from_pin: String,
    pub to_node: NodeId,
    pub to_pin: String,
}

/// Container of nodes + edges + the running next-id counter. Pure
/// data; connection validation lives in `Graph::connect`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub next_id: u32,
}

/// Errors raised by graph mutation / IO.
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

/// Result of `Graph::connect`. `Ok(true)` = edge added; `Ok(false)` =
/// edge rejected for benign reasons (duplicate, self-loop).
/// `Err(ConnectError)` = type mismatch or unknown pin.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConnectError {
    #[error("unknown node {node:?}")]
    UnknownNode { node: NodeId },
    #[error("unknown node kind '{kind}'")]
    UnknownKind { kind: String },
    #[error("unknown pin '{pin}' on node {node:?}")]
    UnknownPin { node: NodeId, pin: String },
    #[error("pin type mismatch: {from_ty} -> {to_ty}")]
    TypeMismatch {
        from_ty: &'static str,
        to_ty: &'static str,
    },
}

impl Graph {
    pub fn add_node(&mut self, kind: impl Into<String>, position: [f32; 2]) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(Node::new(id, kind, position));
        id
    }

    pub fn remove_node(&mut self, id: NodeId) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from_node != id && e.to_node != id);
    }

    fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Attempt to add an edge. Returns `Ok(true)` on success,
    /// `Ok(false)` for benign rejections (duplicate, self-loop), or
    /// `Err(ConnectError)` when the registry-validated pin types do
    /// not match.
    pub fn connect(
        &mut self,
        registry: &NodeKindRegistry,
        from_node: NodeId,
        from_pin: &str,
        to_node: NodeId,
        to_pin: &str,
    ) -> Result<bool, ConnectError> {
        if from_node == to_node {
            return Ok(false);
        }
        let from = self
            .node(from_node)
            .ok_or(ConnectError::UnknownNode { node: from_node })?;
        let to = self
            .node(to_node)
            .ok_or(ConnectError::UnknownNode { node: to_node })?;
        let from_def = registry
            .get(&from.kind)
            .ok_or_else(|| ConnectError::UnknownKind {
                kind: from.kind.clone(),
            })?;
        let to_def = registry
            .get(&to.kind)
            .ok_or_else(|| ConnectError::UnknownKind {
                kind: to.kind.clone(),
            })?;
        let from_spec = from_def
            .output(from_pin)
            .ok_or_else(|| ConnectError::UnknownPin {
                node: from_node,
                pin: from_pin.to_string(),
            })?;
        let to_spec = to_def
            .input(to_pin)
            .ok_or_else(|| ConnectError::UnknownPin {
                node: to_node,
                pin: to_pin.to_string(),
            })?;
        if from_spec.ty != to_spec.ty {
            return Err(ConnectError::TypeMismatch {
                from_ty: from_spec.ty.label(),
                to_ty: to_spec.ty.label(),
            });
        }
        // Prevent duplicate edges on the same source/target pin pair.
        if self.edges.iter().any(|e| {
            e.from_node == from_node
                && e.from_pin == from_pin
                && e.to_node == to_node
                && e.to_pin == to_pin
        }) {
            return Ok(false);
        }
        self.edges.push(Edge {
            from_node,
            from_pin: from_pin.to_owned(),
            to_node,
            to_pin: to_pin.to_owned(),
        });
        Ok(true)
    }
}

/// What a blueprint applies to. Drives the editor's header label and
/// runtime resolution of references like `SetPosition` ("position of
/// what?").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintTarget {
    /// Global script - lives outside the scene, runs in the host loop.
    #[default]
    Global,
    /// Bound to a specific scene entity by stable id.
    Entity { entity_id: u32, entity_name: String },
    /// Bound to a named class / archetype - resolved per-instance at
    /// runtime.
    Class(String),
}

/// Map a `BlueprintTarget` to the equivalent `BlueprintContext` used
/// for node-catalogue filtering.
pub fn blueprint_context(target: &BlueprintTarget) -> BlueprintContext {
    match target {
        BlueprintTarget::Global => BlueprintContext::Global,
        BlueprintTarget::Entity { .. } => BlueprintContext::Entity,
        BlueprintTarget::Class(_) => BlueprintContext::Class,
    }
}

impl BlueprintTarget {
    pub fn label(&self) -> String {
        match self {
            BlueprintTarget::Global => "Global".into(),
            BlueprintTarget::Entity {
                entity_id,
                entity_name,
            } => format!("Entity '{entity_name}' (#{entity_id})"),
            BlueprintTarget::Class(c) => format!("Class '{c}'"),
        }
    }
}

/// User-defined typed variable that lives on a blueprint. Backs the
/// side-panel Variables list (Unreal-style). Get/Set nodes for these
/// variables are added by the runtime once execution catches up; the
/// data is stored so the variable persists across editor sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueprintVariable {
    pub name: String,
    pub ty: PinType,
    /// Stringly-typed default value. Parsed at runtime based on `ty`.
    #[serde(default)]
    pub default: String,
}

/// Blueprint = named graph + the thing it targets. Saved files store
/// this whole struct so the target context travels with the script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blueprint {
    pub name: String,
    #[serde(default)]
    pub target: BlueprintTarget,
    pub graph: Graph,
    /// Variables defined on the blueprint. Lives in the side panel
    /// next to the canvas.
    #[serde(default)]
    pub variables: Vec<BlueprintVariable>,
    /// Named function sub-graphs the user authored on the side panel.
    /// Functions are full mini-graphs with their own entry node.
    #[serde(default)]
    pub functions: Vec<NamedGraph>,
    /// Macros - inline-expanded sub-graphs. Same shape as a function
    /// but the compiler splices them into the call site rather than
    /// emitting a CALL op.
    #[serde(default)]
    pub macros: Vec<NamedGraph>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedGraph {
    pub name: String,
    pub graph: Graph,
}

impl Default for Blueprint {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            target: BlueprintTarget::Global,
            graph: Graph::default(),
            variables: Vec::new(),
            functions: Vec::new(),
            macros: Vec::new(),
        }
    }
}

impl Blueprint {
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

// ---------- Editor canvas ---------------------------------------------

const NODE_WIDTH: f32 = 170.0;
const NODE_HEADER_HEIGHT: f32 = 28.0;
const PIN_ROW_HEIGHT: f32 = 18.0;
const PIN_RADIUS: f32 = 5.0;
const PIN_HIT_RADIUS: f32 = 10.0;

fn node_height_def(def: &NodeKindDef) -> f32 {
    let rows = def.inputs.len().max(def.outputs.len()) as f32;
    NODE_HEADER_HEIGHT + rows * PIN_ROW_HEIGHT + 8.0
}

/// Header tint for a node, keyed off its declared category. Mirrors
/// Unreal's coloured-header convention so the user can spot events,
/// math, control flow and variable nodes at a glance.
/// Minimum distance from `point` to the cubic bezier wire that
/// `draw_edge` paints between `a` and `b`. We sample the same 24
/// points and take the closest segment, which is plenty accurate
/// for "is the cursor on this wire" hover detection.
fn bezier_min_distance(a: egui::Pos2, b: egui::Pos2, point: egui::Pos2) -> f32 {
    let dx = (b.x - a.x).abs().max(40.0) * 0.5;
    let c1 = egui::pos2(a.x + dx, a.y);
    let c2 = egui::pos2(b.x - dx, b.y);
    let mut prev = a;
    let mut best = f32::INFINITY;
    for i in 1..=24 {
        let t = i as f32 / 24.0;
        let one = 1.0 - t;
        let x = one * one * one * a.x
            + 3.0 * one * one * t * c1.x
            + 3.0 * one * t * t * c2.x
            + t * t * t * b.x;
        let y = one * one * one * a.y
            + 3.0 * one * one * t * c1.y
            + 3.0 * one * t * t * c2.y
            + t * t * t * b.y;
        let curr = egui::pos2(x, y);
        // Distance from `point` to segment (prev, curr).
        let sx = curr.x - prev.x;
        let sy = curr.y - prev.y;
        let len_sq = sx * sx + sy * sy;
        let t_proj = if len_sq > 0.0 {
            (((point.x - prev.x) * sx + (point.y - prev.y) * sy) / len_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let nx = prev.x + sx * t_proj;
        let ny = prev.y + sy * t_proj;
        let d = ((point.x - nx).powi(2) + (point.y - ny).powi(2)).sqrt();
        if d < best {
            best = d;
        }
        prev = curr;
    }
    best
}

fn category_header_color(category: &str) -> [u8; 3] {
    match category {
        "Events" => [180, 60, 60],
        "Math" => [60, 140, 80],
        "Control" => [200, 140, 50],
        "Comparison" => [110, 150, 90],
        "Logic" => [140, 120, 60],
        "Variables" => [180, 160, 60],
        "Transform" => [80, 110, 180],
        "Audio" => [150, 90, 170],
        "Time" => [80, 140, 170],
        "Functions" => [130, 80, 170],
        "Debug" | "Values" => [110, 110, 110],
        _ => [72, 84, 108],
    }
}

#[derive(Debug, Clone)]
struct PendingEdge {
    node: NodeId,
    pin: String,
    is_output: bool,
    ty: PinType,
}

/// Payload for the side-panel -> canvas drag-and-drop flow. The side
/// panel wraps each variable / function row in a `dnd_drag_source`
/// carrying one of these; the canvas registers a matching drop zone
/// and translates the drop into either a Get/Set variable node or a
/// CallFunction node.
#[derive(Debug, Clone)]
pub enum BpDragPayload {
    /// Drag a named variable. The drop site asks Set vs Get (or, if
    /// the user held Ctrl/Alt, applies the modifier rule directly).
    Variable(String, PinType),
    /// Drag a function. The drop creates a `call_function` node with
    /// the `name` param pre-filled.
    Function(String),
}

/// Which sub-graph the editor's canvas is currently rendering. The
/// blueprint always owns an `EventGraph` (the default trigger graph)
/// plus user-defined functions and macros; the sub-tab strip switches
/// between them. Function / macro body editing lives behind a stub
/// for the moment - the data is persisted, but the canvas only edits
/// the event graph until the full multi-graph refactor lands.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum SubGraphRef {
    #[default]
    Event,
    Function(usize),
    Macro(usize),
}

/// Egui canvas. Owns its own pan / drag / connect state. Renders into
/// any `egui::Ui` via `show`, mutates the supplied blueprint, and
/// validates connections through the registry it was given.
#[derive(Debug, Default)]
pub struct BlueprintEditor {
    pub blueprint: Blueprint,
    /// Filesystem source for this blueprint, used by the workspace to
    /// save dirty changes and to focus an already-open tab when the
    /// user double-clicks the same entity twice.
    pub path: Option<PathBuf>,
    /// Which sub-graph is currently active in the canvas.
    pub active_sub_graph: SubGraphRef,
    view_offset: egui::Vec2,
    dragging: Option<NodeId>,
    drag_node_offset: egui::Vec2,
    pending_edge: Option<PendingEdge>,
    selected: Option<NodeId>,
    context_menu_at: Option<egui::Pos2>,
    /// When a pin drag releases on empty canvas the editor pops the
    /// "Add node" context menu pre-filtered by the source pin's type
    /// and auto-connects whatever node the user picks. This stores the
    /// originating pin until the menu closes.
    context_menu_from_pin: Option<PendingEdge>,
    /// Additional nodes added to the selection via Shift+click.
    /// `selected` is still the "primary" (drives the inspector); the
    /// extras are along for the ride so Delete and drag can act on
    /// them too.
    selected_extra: HashSet<NodeId>,
    /// Live search query for the context menu. Unreal-style: typing
    /// filters the flat list of node kinds by case-insensitive
    /// substring match against label + category.
    context_menu_search: String,
    /// Pending variable drop awaiting Set/Get disambiguation. Stores
    /// the variable name + the canvas-local drop position; the
    /// canvas renders a small popup that resolves into either a
    /// `get_*_var` or `set_*_var` node.
    pending_var_drop: Option<(String, PinType, egui::Pos2)>,
    status: String,
}

impl BlueprintEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_blueprint(blueprint: Blueprint) -> Self {
        Self {
            blueprint,
            ..Self::default()
        }
    }

    fn node_origin(&self, canvas_min: egui::Pos2, node: &Node) -> egui::Pos2 {
        canvas_min + self.view_offset + egui::vec2(node.position[0], node.position[1])
    }

    fn pin_position(
        &self,
        registry: &NodeKindRegistry,
        canvas_min: egui::Pos2,
        node: &Node,
        pin: &str,
        is_output: bool,
    ) -> Option<egui::Pos2> {
        let def = registry.get(&node.kind)?;
        let pins = if is_output { def.outputs } else { def.inputs };
        let idx = pins.iter().position(|p| p.name == pin)?;
        let origin = self.node_origin(canvas_min, node);
        let y = origin.y + NODE_HEADER_HEIGHT + idx as f32 * PIN_ROW_HEIGHT + PIN_ROW_HEIGHT * 0.5;
        let x = if is_output {
            origin.x + NODE_WIDTH
        } else {
            origin.x
        };
        Some(egui::pos2(x, y))
    }

    fn hit_pin(
        &self,
        registry: &NodeKindRegistry,
        canvas_min: egui::Pos2,
        pos: egui::Pos2,
    ) -> Option<(NodeId, String, bool, PinType)> {
        for node in self.blueprint.graph.nodes.iter().rev() {
            let Some(def) = registry.get(&node.kind) else {
                continue;
            };
            for (is_output, pins) in [(true, def.outputs), (false, def.inputs)] {
                for pin in pins {
                    if let Some(p) =
                        self.pin_position(registry, canvas_min, node, pin.name, is_output)
                        && pos.distance(p) <= PIN_HIT_RADIUS
                    {
                        return Some((node.id, pin.name.to_string(), is_output, pin.ty));
                    }
                }
            }
        }
        None
    }

    fn hit_node(
        &self,
        registry: &NodeKindRegistry,
        canvas_min: egui::Pos2,
        pos: egui::Pos2,
    ) -> Option<NodeId> {
        for node in self.blueprint.graph.nodes.iter().rev() {
            let Some(def) = registry.get(&node.kind) else {
                continue;
            };
            let origin = self.node_origin(canvas_min, node);
            // Comment nodes use a wider, fixed-height rect (matches
            // the renderer's layout).
            let size = if node.kind == "comment" {
                egui::vec2(NODE_WIDTH * 2.2, 64.0)
            } else {
                egui::vec2(NODE_WIDTH, node_height_def(def))
            };
            let rect = egui::Rect::from_min_size(origin, size);
            if rect.contains(pos) {
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
        stroke: f32,
    ) {
        let dx = (b.x - a.x).abs().max(40.0) * 0.5;
        let c1 = egui::pos2(a.x + dx, a.y);
        let c2 = egui::pos2(b.x - dx, b.y);
        let mut prev = a;
        for i in 1..=24 {
            let t = i as f32 / 24.0;
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
            painter.line_segment([prev, p], egui::Stroke::new(stroke, color));
            prev = p;
        }
    }

    /// Render the editor. Returns `true` whenever the graph mutated
    /// this frame so the host can persist it.
    pub fn show(&mut self, ui: &mut egui::Ui, registry: &NodeKindRegistry) -> bool {
        let mut dirty = false;
        ui.horizontal_top(|ui| {
            // Left side panel: blueprint metadata + variables /
            // functions / macros lists (Unreal-style).
            let side_w = 240.0;
            ui.allocate_ui_with_layout(
                egui::vec2(side_w, ui.available_height()),
                egui::Layout::top_down_justified(egui::Align::LEFT),
                |ui| {
                    if self.render_side_panel(ui) {
                        dirty = true;
                    }
                },
            );
            ui.separator();
            // Right column: the existing canvas surface.
            ui.vertical(|ui| {
                if self.show_canvas(ui, registry) {
                    dirty = true;
                }
            });
        });
        dirty
    }

    fn render_side_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let mut dirty = false;
        ui.heading("Blueprint");
        ui.label(format!("Name: {}", self.blueprint.name));
        ui.label(format!("Target: {}", self.blueprint.target.label()));
        ui.separator();
        // Variables.
        ui.collapsing("Variables", |ui| {
            let mut remove: Option<usize> = None;
            for (i, var) in self.blueprint.variables.iter_mut().enumerate() {
                // The drag handle (left column) carries the variable
                // name as payload. The rest of the row stays as a
                // regular edit surface so renaming / changing type
                // still works without triggering a drag.
                ui.horizontal(|ui| {
                    let drag_id = egui::Id::new(("bp_var_drag", i));
                    let payload = BpDragPayload::Variable(var.name.clone(), var.ty);
                    ui.dnd_drag_source(drag_id, payload, |ui| {
                        ui.label("::");
                    })
                    .response
                    .on_hover_text("Drag onto canvas to add Get/Set");
                    if ui.text_edit_singleline(&mut var.name).changed() {
                        dirty = true;
                    }
                    egui::ComboBox::from_id_salt(format!("var_ty_{i}"))
                        .selected_text(var.ty.label())
                        .show_ui(ui, |ui| {
                            for ty in [
                                PinType::Float,
                                PinType::Bool,
                                PinType::Vec3,
                                PinType::Text,
                                PinType::EntityRef,
                            ] {
                                if ui.selectable_label(var.ty == ty, ty.label()).clicked() {
                                    var.ty = ty;
                                    dirty = true;
                                }
                            }
                        });
                    if ui.small_button("X").clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Default");
                    if ui.text_edit_singleline(&mut var.default).changed() {
                        dirty = true;
                    }
                });
            }
            if let Some(i) = remove {
                self.blueprint.variables.remove(i);
                dirty = true;
            }
            if ui.button("Add variable").clicked() {
                self.blueprint.variables.push(BlueprintVariable {
                    name: format!("Var{}", self.blueprint.variables.len() + 1),
                    ty: PinType::Float,
                    default: "0".into(),
                });
                dirty = true;
            }
        });
        // Functions (drag-droppable: drop onto canvas -> CallFunction).
        ui.collapsing("Functions", |ui| {
            let mut remove: Option<usize> = None;
            for (i, f) in self.blueprint.functions.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let drag_id = egui::Id::new(("bp_fn_drag", i));
                    let payload = BpDragPayload::Function(f.name.clone());
                    ui.dnd_drag_source(drag_id, payload, |ui| {
                        ui.label("::");
                    })
                    .response
                    .on_hover_text("Drag onto canvas to add Call Function");
                    if ui.text_edit_singleline(&mut f.name).changed() {
                        dirty = true;
                    }
                    if ui.small_button("X").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.blueprint.functions.remove(i);
                dirty = true;
            }
            if ui.button("Add function").clicked() {
                self.blueprint.functions.push(NamedGraph {
                    name: format!("Function{}", self.blueprint.functions.len() + 1),
                    graph: Graph::default(),
                });
                dirty = true;
            }
        });
        // Inspector for the currently selected node. Lives here on
        // the side panel so the canvas stays clean.
        ui.collapsing("Selected node", |ui| {
            if self.render_node_inspector(ui) {
                dirty = true;
            }
        });
        // Macros (scaffold).
        ui.collapsing("Macros", |ui| {
            let mut remove: Option<usize> = None;
            for (i, m) in self.blueprint.macros.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui.text_edit_singleline(&mut m.name).changed() {
                        dirty = true;
                    }
                    if ui.small_button("X").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.blueprint.macros.remove(i);
                dirty = true;
            }
            if ui.button("Add macro").clicked() {
                self.blueprint.macros.push(NamedGraph {
                    name: format!("Macro{}", self.blueprint.macros.len() + 1),
                    graph: Graph::default(),
                });
                dirty = true;
            }
        });
        dirty
    }

    /// Spawn a variable Get / Set node at the given canvas-local
    /// position with the variable name + type baked into the node's
    /// params. Only Float vars have a typed Get/Set node today; other
    /// types fall back to `get_float_var` / `set_float_var` until the
    /// generalised variable ops land (the value still flows through
    /// the VM, just narrowed to float at the boundary).
    fn spawn_var_node(
        &mut self,
        name: &str,
        _ty: PinType,
        is_setter: bool,
        canvas_pos: egui::Vec2,
    ) -> NodeId {
        let kind = if is_setter {
            "set_float_var"
        } else {
            "get_float_var"
        };
        let id = self
            .blueprint
            .graph
            .add_node(kind, [canvas_pos.x, canvas_pos.y]);
        if let Some(n) = self.blueprint.graph.nodes.iter_mut().find(|n| n.id == id) {
            n.params.insert("name".into(), name.to_string());
        }
        self.selected = Some(id);
        id
    }

    /// Show editor fields for `params` on the currently selected node
    /// in the currently visible sub-graph. Specialises a handful of
    /// known node kinds (constants, variable get/set, set_position,
    /// call_function) and falls back to a generic key/value list for
    /// everything else so user-registered nodes still expose their
    /// params.
    fn render_node_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(sel) = self.selected else {
            ui.label("(none)");
            return false;
        };
        // Function-name candidate list (read before borrowing the
        // active graph mutably, since we may need to render a
        // ComboBox of them while editing the node's `name` param).
        let function_names: Vec<String> = self
            .blueprint
            .functions
            .iter()
            .map(|f| f.name.clone())
            .collect();
        // Resolve the visible graph based on the active sub-graph.
        let graph: &mut Graph = match self.active_sub_graph {
            SubGraphRef::Event => &mut self.blueprint.graph,
            SubGraphRef::Function(i) => match self.blueprint.functions.get_mut(i) {
                Some(f) => &mut f.graph,
                None => return false,
            },
            SubGraphRef::Macro(i) => match self.blueprint.macros.get_mut(i) {
                Some(m) => &mut m.graph,
                None => return false,
            },
        };
        let Some(node) = graph.nodes.iter_mut().find(|n| n.id == sel) else {
            ui.label("(stale selection)");
            return false;
        };
        ui.label(format!("Kind: {}", node.kind));
        let mut dirty = false;
        match node.kind.as_str() {
            "const_float" => {
                let mut v = node
                    .params
                    .get("value")
                    .cloned()
                    .unwrap_or_else(|| "0".into());
                ui.horizontal(|ui| {
                    ui.label("value");
                    if ui.text_edit_singleline(&mut v).changed() {
                        node.params.insert("value".into(), v);
                        dirty = true;
                    }
                });
            }
            "const_bool" => {
                let cur = node
                    .params
                    .get("value")
                    .map(|s| s == "true" || s == "1")
                    .unwrap_or(false);
                let mut next = cur;
                if ui.checkbox(&mut next, "value").changed() {
                    node.params.insert(
                        "value".into(),
                        if next { "true".into() } else { "false".into() },
                    );
                    dirty = true;
                }
            }
            "const_vec3" | "make_vec3" => {
                for axis in ["x", "y", "z"] {
                    let mut s = node
                        .params
                        .get(axis)
                        .cloned()
                        .unwrap_or_else(|| "0".into());
                    ui.horizontal(|ui| {
                        ui.label(axis);
                        if ui.text_edit_singleline(&mut s).changed() {
                            node.params.insert(axis.into(), s);
                            dirty = true;
                        }
                    });
                }
            }
            "get_float_var" | "set_float_var" => {
                let mut name = node
                    .params
                    .get("name")
                    .cloned()
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label("variable");
                    if ui.text_edit_singleline(&mut name).changed() {
                        node.params.insert("name".into(), name);
                        dirty = true;
                    }
                });
            }
            "play_sound" => {
                let mut path = node.params.get("path").cloned().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label("path");
                    if ui.text_edit_singleline(&mut path).changed() {
                        node.params.insert("path".into(), path);
                        dirty = true;
                    }
                });
            }
            "call_function" => {
                let mut name = node
                    .params
                    .get("name")
                    .cloned()
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label("function");
                    let display = if name.is_empty() {
                        "(none)".to_string()
                    } else {
                        name.clone()
                    };
                    egui::ComboBox::from_id_salt(format!("call_fn_{}", node.id.0))
                        .selected_text(display)
                        .show_ui(ui, |ui| {
                            for fname in &function_names {
                                if ui
                                    .selectable_label(fname == &name, fname)
                                    .clicked()
                                {
                                    name = fname.clone();
                                    node.params.insert("name".into(), name.clone());
                                    dirty = true;
                                }
                            }
                        });
                });
            }
            _ => {
                // Generic fallback: list whatever params already exist
                // so user-registered node kinds can be tweaked. New
                // keys can't be added here - those come from a custom
                // inspector the host registers later.
                if node.params.is_empty() {
                    ui.label("(no params)");
                } else {
                    let keys: Vec<String> = node.params.keys().cloned().collect();
                    for k in keys {
                        let mut v = node.params.get(&k).cloned().unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(&k);
                            if ui.text_edit_singleline(&mut v).changed() {
                                node.params.insert(k.clone(), v);
                                dirty = true;
                            }
                        });
                    }
                }
            }
        }
        dirty
    }

    fn show_canvas(&mut self, ui: &mut egui::Ui, registry: &NodeKindRegistry) -> bool {
        let mut dirty = false;

        // Clamp active sub-graph index if the user deleted the
        // function / macro it was pointing at.
        let valid = match self.active_sub_graph {
            SubGraphRef::Event => true,
            SubGraphRef::Function(i) => i < self.blueprint.functions.len(),
            SubGraphRef::Macro(i) => i < self.blueprint.macros.len(),
        };
        if !valid {
            self.active_sub_graph = SubGraphRef::Event;
        }

        // Sub-tab strip: EventGraph + every function + every macro.
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(
                    matches!(self.active_sub_graph, SubGraphRef::Event),
                    "EventGraph",
                )
                .clicked()
            {
                self.active_sub_graph = SubGraphRef::Event;
            }
            for (i, f) in self.blueprint.functions.iter().enumerate() {
                let active = matches!(self.active_sub_graph, SubGraphRef::Function(j) if j == i);
                let label = format!("fn {}", f.name);
                if ui.selectable_label(active, label).clicked() {
                    self.active_sub_graph = SubGraphRef::Function(i);
                }
            }
            for (i, m) in self.blueprint.macros.iter().enumerate() {
                let active = matches!(self.active_sub_graph, SubGraphRef::Macro(j) if j == i);
                let label = format!("macro {}", m.name);
                if ui.selectable_label(active, label).clicked() {
                    self.active_sub_graph = SubGraphRef::Macro(i);
                }
            }
        });
        ui.separator();

        // Editing a function / macro body reuses the same canvas
        // code, which is hard-coded to talk to `self.blueprint.graph`.
        // The cleanest local workaround is to temporarily swap the
        // active sub-graph into the event slot for the duration of
        // the render, then swap the edited graph back when we're
        // done. The swap is a no-op for the Event sub-graph.
        let active_sub = self.active_sub_graph;
        let swapped: Option<Graph> = match active_sub {
            SubGraphRef::Event => None,
            SubGraphRef::Function(i) => self
                .blueprint
                .functions
                .get_mut(i)
                .map(|f| std::mem::take(&mut f.graph)),
            SubGraphRef::Macro(i) => self
                .blueprint
                .macros
                .get_mut(i)
                .map(|m| std::mem::take(&mut m.graph)),
        };
        let saved_event = swapped.map(|g| std::mem::replace(&mut self.blueprint.graph, g));

        let inner_dirty = self.show_canvas_body(ui, registry);
        dirty |= inner_dirty;

        // Restore: put the (possibly edited) sub-graph back where it
        // came from and reinstate the original event graph.
        if let Some(orig_event) = saved_event {
            let edited_sub = std::mem::replace(&mut self.blueprint.graph, orig_event);
            match active_sub {
                SubGraphRef::Function(i) => {
                    if let Some(f) = self.blueprint.functions.get_mut(i) {
                        f.graph = edited_sub;
                    }
                }
                SubGraphRef::Macro(i) => {
                    if let Some(m) = self.blueprint.macros.get_mut(i) {
                        m.graph = edited_sub;
                    }
                }
                SubGraphRef::Event => {}
            }
        }
        dirty
    }

    /// Canvas body. Always reads + writes `self.blueprint.graph`; the
    /// caller in `show_canvas` is responsible for swapping the active
    /// sub-graph into that slot before invoking this and restoring it
    /// afterwards. Returns `true` when the active graph mutated.
    fn show_canvas_body(&mut self, ui: &mut egui::Ui, registry: &NodeKindRegistry) -> bool {
        let mut dirty = false;

        // Header: blueprint name + target + nodes/edges counts.
        ui.horizontal(|ui| {
            ui.label(format!("Blueprint: {}", self.blueprint.name));
            ui.separator();
            ui.label(format!("Target: {}", self.blueprint.target.label()));
            ui.separator();
            ui.label(format!("Nodes: {}", self.blueprint.graph.nodes.len()));
            ui.label(format!("Edges: {}", self.blueprint.graph.edges.len()));
            ui.menu_button("Add node", |ui| {
                let ctx = blueprint_context(&self.blueprint.target);
                // Group nodes by category, filtered by the active
                // target's context so e.g. a Global blueprint never
                // offers SetPosition.
                let mut by_cat: HashMap<&'static str, Vec<&NodeKindDef>> = HashMap::new();
                for def in registry.iter() {
                    if !def.available_in(ctx) {
                        continue;
                    }
                    by_cat.entry(def.category).or_default().push(def);
                }
                let mut cats: Vec<_> = by_cat.keys().copied().collect();
                cats.sort_unstable();
                for cat in cats {
                    ui.menu_button(cat, |ui| {
                        let mut defs = by_cat[cat].clone();
                        defs.sort_by_key(|d| d.label);
                        for def in defs {
                            if ui.button(def.label).clicked() {
                                let pos = -self.view_offset + egui::vec2(40.0, 40.0);
                                self.blueprint.graph.add_node(def.id, [pos.x, pos.y]);
                                dirty = true;
                                ui.close();
                            }
                        }
                    });
                }
            });
            if ui.button("Reset view").clicked() {
                self.view_offset = egui::Vec2::ZERO;
            }
        });
        if !self.status.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(255, 200, 80), &self.status);
        }

        // Blueprint settings: name + target picker.
        ui.collapsing("Blueprint settings", |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                if ui.text_edit_singleline(&mut self.blueprint.name).changed() {
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Target");
                let current = self.blueprint.target.label();
                egui::ComboBox::from_id_salt("blueprint_target")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                matches!(self.blueprint.target, BlueprintTarget::Global),
                                "Global",
                            )
                            .clicked()
                        {
                            self.blueprint.target = BlueprintTarget::Global;
                            dirty = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(self.blueprint.target, BlueprintTarget::Entity { .. }),
                                "Entity",
                            )
                            .clicked()
                        {
                            self.blueprint.target = BlueprintTarget::Entity {
                                entity_id: 0,
                                entity_name: "unnamed".into(),
                            };
                            dirty = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(self.blueprint.target, BlueprintTarget::Class(_)),
                                "Class",
                            )
                            .clicked()
                        {
                            self.blueprint.target = BlueprintTarget::Class("Player".into());
                            dirty = true;
                        }
                    });
            });
            match &mut self.blueprint.target {
                BlueprintTarget::Global => {}
                BlueprintTarget::Entity {
                    entity_id,
                    entity_name,
                } => {
                    ui.horizontal(|ui| {
                        ui.label("Entity id");
                        if ui.add(egui::DragValue::new(entity_id)).changed() {
                            dirty = true;
                        }
                        ui.label("Name");
                        if ui.text_edit_singleline(entity_name).changed() {
                            dirty = true;
                        }
                    });
                }
                BlueprintTarget::Class(name) => {
                    ui.horizontal(|ui| {
                        ui.label("Class");
                        if ui.text_edit_singleline(name).changed() {
                            dirty = true;
                        }
                    });
                }
            }
        });
        ui.separator();

        let canvas = ui.available_size_before_wrap();
        let (rect, response) = ui.allocate_exact_size(canvas, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(28));

        // Side-panel -> canvas drop: if the user releases a var /
        // function payload over the canvas, materialise it into a
        // node at the drop position.
        let drop_payload = response.dnd_release_payload::<BpDragPayload>();
        if let Some(payload) = drop_payload {
            // Hover position is the cursor when the release happened;
            // fall back to canvas top-left if egui didn't capture one.
            let drop_pos = response.hover_pos().unwrap_or(rect.min);
            let canvas_pos = drop_pos - rect.min - self.view_offset;
            match payload.as_ref() {
                BpDragPayload::Function(name) => {
                    let new_id = self
                        .blueprint
                        .graph
                        .add_node("call_function", [canvas_pos.x, canvas_pos.y]);
                    if let Some(n) = self
                        .blueprint
                        .graph
                        .nodes
                        .iter_mut()
                        .find(|n| n.id == new_id)
                    {
                        n.params.insert("name".into(), name.clone());
                    }
                    self.selected = Some(new_id);
                    dirty = true;
                }
                BpDragPayload::Variable(name, ty) => {
                    // Unreal-style modifiers: Alt = Get, Ctrl = Set,
                    // otherwise ask via a small popup so the user
                    // explicitly picks Set vs Get.
                    let mods = ui.input(|i| i.modifiers);
                    if mods.alt {
                        self.spawn_var_node(name, *ty, false, canvas_pos);
                        dirty = true;
                    } else if mods.ctrl {
                        self.spawn_var_node(name, *ty, true, canvas_pos);
                        dirty = true;
                    } else {
                        // Defer until the user picks via the popup
                        // (rendered further down once the borrow
                        // checker is happy).
                        self.pending_var_drop =
                            Some((name.clone(), *ty, egui::pos2(canvas_pos.x, canvas_pos.y)));
                    }
                }
            }
        }

        // Grid backdrop.
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

        // Pan with middle / right drag.
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            self.view_offset += response.drag_delta();
        }

        if primary_just_pressed && let Some(pos) = pointer {
            if let Some((node_id, pin, is_output, ty)) = self.hit_pin(registry, rect.min, pos) {
                self.pending_edge = Some(PendingEdge {
                    node: node_id,
                    pin,
                    is_output,
                    ty,
                });
                self.dragging = None;
            } else if let Some(node_id) = self.hit_node(registry, rect.min, pos) {
                self.dragging = Some(node_id);
                let shift = ui.input(|i| i.modifiers.shift);
                if shift {
                    // Toggle this node's membership in the extra set
                    // without disturbing the primary `selected`.
                    if self.selected_extra.contains(&node_id) {
                        self.selected_extra.remove(&node_id);
                    } else {
                        self.selected_extra.insert(node_id);
                    }
                } else {
                    // Plain click clears multi-selection and picks
                    // this node as the new primary.
                    self.selected_extra.clear();
                    self.selected = Some(node_id);
                }
                if let Some(node) = self.blueprint.graph.nodes.iter().find(|n| n.id == node_id) {
                    let origin = self.node_origin(rect.min, node);
                    self.drag_node_offset = pos - origin;
                }
            } else {
                self.selected = None;
                self.selected_extra.clear();
            }
        }

        if primary_down
            && let Some(id) = self.dragging
            && let Some(pos) = pointer
            && let Some(node) = self.blueprint.graph.nodes.iter_mut().find(|n| n.id == id)
        {
            let local = pos - rect.min - self.view_offset - self.drag_node_offset;
            node.position[0] = local.x;
            node.position[1] = local.y;
            dirty = true;
        }

        if primary_released {
            self.dragging = None;
            if let (Some(pending), Some(pos)) = (self.pending_edge.take(), pointer) {
                if let Some((target_node, target_pin, target_is_output, _target_ty)) =
                    self.hit_pin(registry, rect.min, pos)
                {
                    if target_is_output != pending.is_output {
                        let (from_node, from_pin, to_node, to_pin) = if pending.is_output {
                            (pending.node, pending.pin.clone(), target_node, target_pin)
                        } else {
                            (target_node, target_pin, pending.node, pending.pin.clone())
                        };
                        match self
                            .blueprint
                            .graph
                            .connect(registry, from_node, &from_pin, to_node, &to_pin)
                        {
                            Ok(true) => {
                                dirty = true;
                                self.status.clear();
                            }
                            Ok(false) => {}
                            Err(err) => {
                                self.status = format!("Connect: {err}");
                            }
                        }
                    }
                } else {
                    // Dropped on empty canvas - open the contextual
                    // "Add node" menu pre-filtered by the source pin's
                    // type so the user can spawn + auto-connect a
                    // compatible node in one stroke.
                    self.context_menu_at = Some(pos);
                    self.context_menu_from_pin = Some(pending);
                }
            }
        }

        let delete_pressed = ui.input(|i| i.key_pressed(egui::Key::Delete));
        if delete_pressed {
            let mut to_remove: Vec<NodeId> = self.selected_extra.drain().collect();
            if let Some(id) = self.selected.take() {
                to_remove.push(id);
            }
            for id in to_remove {
                self.blueprint.graph.remove_node(id);
                dirty = true;
            }
        }

        // Find which edge (if any) the cursor is hovering. We
        // approximate the bezier with the same 24-sample polyline
        // draw_edge uses, then take the minimum point-to-segment
        // distance across all samples.
        let mut hovered_edge: Option<usize> = None;
        if let Some(p) = hover
            && self.pending_edge.is_none()
        {
            let mut best = 8.0_f32;
            for (idx, edge) in self.blueprint.graph.edges.iter().enumerate() {
                let from = self
                    .blueprint
                    .graph
                    .nodes
                    .iter()
                    .find(|n| n.id == edge.from_node);
                let to = self
                    .blueprint
                    .graph
                    .nodes
                    .iter()
                    .find(|n| n.id == edge.to_node);
                if let (Some(f), Some(t)) = (from, to)
                    && let (Some(a), Some(b)) = (
                        self.pin_position(registry, rect.min, f, &edge.from_pin, true),
                        self.pin_position(registry, rect.min, t, &edge.to_pin, false),
                    )
                {
                    let d = bezier_min_distance(a, b, p);
                    if d < best {
                        best = d;
                        hovered_edge = Some(idx);
                    }
                }
            }
        }

        // Right-click on a hovered wire -> break the link. Bypasses
        // the empty-canvas context menu trigger below.
        let mut wire_break_consumed = false;
        if let Some(idx) = hovered_edge
            && response.clicked_by(egui::PointerButton::Secondary)
        {
            self.blueprint.graph.edges.remove(idx);
            wire_break_consumed = true;
            dirty = true;
        }
        // Empty-canvas right-click: open the Add-node context menu.
        // Skipped if a wire was just broken.
        if !wire_break_consumed
            && response.clicked_by(egui::PointerButton::Secondary)
            && let Some(pos) = hover
            && self.hit_pin(registry, rect.min, pos).is_none()
            && self.hit_node(registry, rect.min, pos).is_none()
        {
            self.context_menu_at = Some(pos);
        }

        // Edges first.
        for (idx, edge) in self.blueprint.graph.edges.iter().enumerate() {
            let from = self
                .blueprint
                .graph
                .nodes
                .iter()
                .find(|n| n.id == edge.from_node);
            let to = self
                .blueprint
                .graph
                .nodes
                .iter()
                .find(|n| n.id == edge.to_node);
            if let (Some(f), Some(t)) = (from, to)
                && let (Some(a), Some(b)) = (
                    self.pin_position(registry, rect.min, f, &edge.from_pin, true),
                    self.pin_position(registry, rect.min, t, &edge.to_pin, false),
                )
            {
                // Colour edges by the source pin type.
                let color = registry
                    .get(&f.kind)
                    .and_then(|d| d.output(&edge.from_pin))
                    .map(|p| {
                        let [r, g, b] = p.ty.rgb();
                        egui::Color32::from_rgb(r, g, b)
                    })
                    .unwrap_or(egui::Color32::from_gray(200));
                let stroke_w = if hovered_edge == Some(idx) { 4.0 } else { 2.0 };
                Self::draw_edge(&painter, a, b, color, stroke_w);
            }
        }

        // In-flight pin drag preview.
        if let (Some(pending), Some(pos)) = (self.pending_edge.as_ref(), hover)
            && let Some(node) = self
                .blueprint
                .graph
                .nodes
                .iter()
                .find(|n| n.id == pending.node)
            && let Some(anchor) =
                self.pin_position(registry, rect.min, node, &pending.pin, pending.is_output)
        {
            let (a, b) = if pending.is_output {
                (anchor, pos)
            } else {
                (pos, anchor)
            };
            let [r, g, b_col] = pending.ty.rgb();
            Self::draw_edge(&painter, a, b, egui::Color32::from_rgb(r, g, b_col), 2.0);
        }

        // Nodes. Snapshot view + selection state so we can borrow
        // the graph nodes mutably (needed for inline pin defaults).
        let view_offset = self.view_offset;
        let selected_id = self.selected;
        let multi_selected = self.selected_extra.clone();
        // Build a set of pins that have an incoming edge so inline
        // editors can hide themselves while a wire is connected.
        let connected_in_pins: HashSet<(NodeId, String)> = self
            .blueprint
            .graph
            .edges
            .iter()
            .map(|e| (e.to_node, e.to_pin.clone()))
            .collect();
        for node in &mut self.blueprint.graph.nodes {
            let Some(def) = registry.get(&node.kind) else {
                continue;
            };
            let origin = rect.min
                + view_offset
                + egui::vec2(node.position[0], node.position[1]);
            // Comment nodes are an annotation overlay - a wide,
            // translucent rect with an editable text caption and no
            // pins. They never compile and never participate in
            // wire routing.
            if node.kind == "comment" {
                let size = egui::vec2(NODE_WIDTH * 2.2, 64.0);
                let cmt_rect = egui::Rect::from_min_size(origin, size);
                painter.rect_filled(
                    cmt_rect,
                    6.0,
                    egui::Color32::from_rgba_unmultiplied(80, 60, 30, 110),
                );
                let stroke_col = if selected_id == Some(node.id) {
                    egui::Color32::from_rgb(255, 200, 80)
                } else {
                    egui::Color32::from_rgba_unmultiplied(200, 160, 70, 200)
                };
                painter.rect_stroke(
                    cmt_rect,
                    6.0,
                    egui::Stroke::new(1.5, stroke_col),
                    egui::StrokeKind::Inside,
                );
                let mut text = node.params.get("text").cloned().unwrap_or_default();
                let edit_rect = cmt_rect.shrink(8.0);
                let resp = ui.put(
                    edit_rect,
                    egui::TextEdit::multiline(&mut text)
                        .desired_width(edit_rect.width())
                        .font(egui::TextStyle::Body)
                        .hint_text("comment..."),
                );
                if resp.changed() {
                    node.params.insert("text".into(), text);
                    dirty = true;
                }
                continue;
            }
            let rect_node =
                egui::Rect::from_min_size(origin, egui::vec2(NODE_WIDTH, node_height_def(def)));
            painter.rect_filled(rect_node, 4.0, egui::Color32::from_rgb(48, 56, 72));
            let selected = selected_id == Some(node.id) || multi_selected.contains(&node.id);
            let border = if selected {
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 200, 80))
            } else {
                egui::Stroke::new(1.0, egui::Color32::from_gray(120))
            };
            painter.rect_stroke(rect_node, 4.0, border, egui::StrokeKind::Inside);
            // Selection glow: a second outer rect a few pixels out
            // from the node so the selected node visually pops.
            if selected {
                let glow_rect = rect_node.expand(3.0);
                painter.rect_stroke(
                    glow_rect,
                    6.0,
                    egui::Stroke::new(
                        1.5,
                        egui::Color32::from_rgba_unmultiplied(255, 200, 80, 90),
                    ),
                    egui::StrokeKind::Outside,
                );
            }
            // Header colour keyed off the node's category so the user
            // can tell Events / Math / Control / Variables apart at
            // a glance, Unreal-style.
            let header_rgb = category_header_color(def.category);
            let header_rect =
                egui::Rect::from_min_size(origin, egui::vec2(NODE_WIDTH, NODE_HEADER_HEIGHT));
            painter.rect_filled(
                header_rect,
                4.0,
                egui::Color32::from_rgb(header_rgb[0], header_rgb[1], header_rgb[2]),
            );
            painter.text(
                origin + egui::vec2(8.0, 6.0),
                egui::Align2::LEFT_TOP,
                def.label,
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
            for (i, pin) in def.inputs.iter().enumerate() {
                let y = origin.y
                    + NODE_HEADER_HEIGHT
                    + i as f32 * PIN_ROW_HEIGHT
                    + PIN_ROW_HEIGHT * 0.5;
                let [r, g, b] = pin.ty.rgb();
                let connected =
                    connected_in_pins.contains(&(node.id, pin.name.to_string()));
                let pin_center = egui::pos2(origin.x, y);
                let hover_rect = egui::Rect::from_center_size(
                    pin_center,
                    egui::vec2(PIN_RADIUS * 3.0, PIN_RADIUS * 3.0),
                );
                let hover_resp = ui.interact(
                    hover_rect,
                    ui.id().with(("bp_pin_in", node.id.0, i as u32)),
                    egui::Sense::hover(),
                );
                let radius = if hover_resp.hovered() {
                    PIN_RADIUS + 2.0
                } else {
                    PIN_RADIUS
                };
                painter.circle_filled(
                    pin_center,
                    radius,
                    egui::Color32::from_rgb(r, g, b),
                );
                hover_resp.on_hover_text(format!(
                    "{} : {}\n{}",
                    pin.name,
                    pin.ty.label(),
                    if connected { "connected" } else { "default literal" }
                ));
                painter.text(
                    egui::pos2(origin.x + 10.0, y - 7.0),
                    egui::Align2::LEFT_TOP,
                    pin.name,
                    egui::FontId::proportional(11.0),
                    egui::Color32::LIGHT_GRAY,
                );
                // Inline pin-default editor: only for non-exec inputs
                // that have no incoming edge. Stored under
                // `params["pin_<name>"]`.
                if pin.ty != PinType::Exec
                    && !connected_in_pins.contains(&(node.id, pin.name.to_string()))
                {
                    let label_w = (pin.name.len() as f32) * 6.5;
                    let edit_x = origin.x + 14.0 + label_w + 4.0;
                    let edit_w = (origin.x + NODE_WIDTH - 8.0) - edit_x;
                    if edit_w > 24.0 {
                        let key = format!("pin_{}", pin.name);
                        let edit_rect = egui::Rect::from_min_size(
                            egui::pos2(edit_x, y - 9.0),
                            egui::vec2(edit_w, 16.0),
                        );
                        match pin.ty {
                            PinType::Bool => {
                                let mut cur = node
                                    .params
                                    .get(&key)
                                    .map(|s| s == "true" || s == "1")
                                    .unwrap_or(false);
                                let resp =
                                    ui.put(edit_rect, egui::Checkbox::new(&mut cur, ""));
                                if resp.changed() {
                                    node.params.insert(
                                        key,
                                        if cur { "true".into() } else { "false".into() },
                                    );
                                    dirty = true;
                                }
                            }
                            _ => {
                                let mut buf =
                                    node.params.get(&key).cloned().unwrap_or_default();
                                let resp = ui.put(
                                    edit_rect,
                                    egui::TextEdit::singleline(&mut buf)
                                        .desired_width(edit_w)
                                        .font(egui::TextStyle::Monospace)
                                        .hint_text("0"),
                                );
                                if resp.changed() {
                                    node.params.insert(key, buf);
                                    dirty = true;
                                }
                            }
                        }
                    }
                }
            }
            for (i, pin) in def.outputs.iter().enumerate() {
                let y = origin.y
                    + NODE_HEADER_HEIGHT
                    + i as f32 * PIN_ROW_HEIGHT
                    + PIN_ROW_HEIGHT * 0.5;
                let [r, g, b] = pin.ty.rgb();
                let pin_center = egui::pos2(origin.x + NODE_WIDTH, y);
                let hover_rect = egui::Rect::from_center_size(
                    pin_center,
                    egui::vec2(PIN_RADIUS * 3.0, PIN_RADIUS * 3.0),
                );
                let hover_resp = ui.interact(
                    hover_rect,
                    ui.id().with(("bp_pin_out", node.id.0, i as u32)),
                    egui::Sense::hover(),
                );
                let radius = if hover_resp.hovered() {
                    PIN_RADIUS + 2.0
                } else {
                    PIN_RADIUS
                };
                painter.circle_filled(
                    pin_center,
                    radius,
                    egui::Color32::from_rgb(r, g, b),
                );
                hover_resp
                    .on_hover_text(format!("{} : {}", pin.name, pin.ty.label()));
                painter.text(
                    egui::pos2(origin.x + NODE_WIDTH - 10.0, y - 7.0),
                    egui::Align2::RIGHT_TOP,
                    pin.name,
                    egui::FontId::proportional(11.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        }

        // Context menu popup (Unreal-style: search field + flat
        // results when typing, categorised tree otherwise).
        if let Some(anchor) = self.context_menu_at {
            let mut close = false;
            let mut add_kind: Option<&'static str> = None;
            let pin_filter = self.context_menu_from_pin.clone();
            egui::Area::new(egui::Id::new("blueprint_context_menu"))
                .fixed_pos(anchor)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(260.0);
                        ui.set_max_height(420.0);
                        if let Some(p) = &pin_filter {
                            let [r, g, b] = p.ty.rgb();
                            ui.colored_label(
                                egui::Color32::from_rgb(r, g, b),
                                format!("Connecting {} ({})", p.pin, p.ty.label()),
                            );
                            ui.separator();
                        }
                        // Search field + clear button.
                        ui.horizontal(|ui| {
                            ui.label("Search:");
                            let resp = ui.text_edit_singleline(&mut self.context_menu_search);
                            resp.request_focus();
                            if ui.small_button("X").clicked() {
                                self.context_menu_search.clear();
                            }
                        });
                        ui.separator();
                        let ctx = blueprint_context(&self.blueprint.target);
                        let query = self.context_menu_search.to_lowercase();
                        // Collect candidates pre-filtered by pin
                        // compatibility + context.
                        let mut candidates: Vec<&NodeKindDef> = Vec::new();
                        for def in registry.iter() {
                            if !def.available_in(ctx) {
                                continue;
                            }
                            if let Some(p) = &pin_filter {
                                let target_pins =
                                    if p.is_output { def.inputs } else { def.outputs };
                                if !target_pins.iter().any(|tp| tp.ty == p.ty) {
                                    continue;
                                }
                            }
                            candidates.push(def);
                        }
                        // Scoring + filter pass for the query.
                        let matches: Vec<&NodeKindDef> = if query.is_empty() {
                            candidates.clone()
                        } else {
                            candidates
                                .iter()
                                .copied()
                                .filter(|d| {
                                    d.label.to_lowercase().contains(&query)
                                        || d.category.to_lowercase().contains(&query)
                                        || d.id.to_lowercase().contains(&query)
                                })
                                .collect()
                        };
                        egui::ScrollArea::vertical()
                            .max_height(340.0)
                            .show(ui, |ui| {
                                if query.is_empty() {
                                    // Tree view by category.
                                    let mut by_cat: HashMap<
                                        &'static str,
                                        Vec<&NodeKindDef>,
                                    > = HashMap::new();
                                    for def in &matches {
                                        by_cat.entry(def.category).or_default().push(def);
                                    }
                                    let mut cats: Vec<_> = by_cat.keys().copied().collect();
                                    cats.sort_unstable();
                                    for cat in cats {
                                        ui.collapsing(cat, |ui| {
                                            let mut defs = by_cat[cat].clone();
                                            defs.sort_by_key(|d| d.label);
                                            for def in defs {
                                                if ui
                                                    .button(def.label)
                                                    .on_hover_text(def.id)
                                                    .clicked()
                                                {
                                                    add_kind = Some(def.id);
                                                    close = true;
                                                }
                                            }
                                        });
                                    }
                                } else {
                                    // Flat ranked list while searching.
                                    if matches.is_empty() {
                                        ui.label("(no matches)");
                                    }
                                    for def in &matches {
                                        let label = format!("{} - {}", def.category, def.label);
                                        if ui.button(label).on_hover_text(def.id).clicked() {
                                            add_kind = Some(def.id);
                                            close = true;
                                        }
                                    }
                                }
                            });
                        ui.separator();
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            if let Some(kind) = add_kind {
                let canvas_pos = anchor - rect.min - self.view_offset;
                let new_id = self
                    .blueprint
                    .graph
                    .add_node(kind, [canvas_pos.x, canvas_pos.y]);
                dirty = true;
                // Auto-connect when the menu came from a pin drag.
                if let Some(p) = self.context_menu_from_pin.take()
                    && let Some(def) = registry.get(kind)
                {
                    let target_pins = if p.is_output { def.inputs } else { def.outputs };
                    if let Some(target_pin) = target_pins.iter().find(|tp| tp.ty == p.ty) {
                        let (from_node, from_pin, to_node, to_pin) = if p.is_output {
                            (p.node, p.pin.clone(), new_id, target_pin.name.to_string())
                        } else {
                            (new_id, target_pin.name.to_string(), p.node, p.pin.clone())
                        };
                        let _ = self
                            .blueprint
                            .graph
                            .connect(registry, from_node, &from_pin, to_node, &to_pin);
                    }
                }
            }
            if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.context_menu_at = None;
                self.context_menu_from_pin = None;
                self.context_menu_search.clear();
            }
        }

        // Pending variable drop -> Set / Get picker popup.
        if let Some((name, ty, canvas_pos)) = self.pending_var_drop.clone() {
            let screen_anchor = rect.min + self.view_offset + canvas_pos.to_vec2();
            let mut consumed = false;
            egui::Area::new(egui::Id::new(("bp_var_drop_popup", name.clone())))
                .fixed_pos(screen_anchor)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(140.0);
                        ui.label(format!("{name} : {}", ty.label()));
                        ui.separator();
                        if ui.button("Get").clicked() {
                            self.spawn_var_node(
                                &name,
                                ty,
                                false,
                                egui::vec2(canvas_pos.x, canvas_pos.y),
                            );
                            dirty = true;
                            consumed = true;
                        }
                        if ui.button("Set").clicked() {
                            self.spawn_var_node(
                                &name,
                                ty,
                                true,
                                egui::vec2(canvas_pos.x, canvas_pos.y),
                            );
                            dirty = true;
                            consumed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            consumed = true;
                        }
                    });
                });
            if consumed || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.pending_var_drop = None;
            }
        }

        dirty
    }
}

/// Multi-tab blueprint workspace. Each tab is a full
/// `BlueprintEditor` instance (with its own canvas pan / drag /
/// selection state); the workspace renders a tab strip at the top
/// and delegates the canvas to the active tab. Mirrors Unreal's BP
/// Editor where every opened object lives in its own tab.
#[derive(Debug, Default)]
pub struct BlueprintWorkspace {
    pub tabs: Vec<BlueprintEditor>,
    pub active: usize,
}

impl BlueprintWorkspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a blueprint in a fresh tab. If a tab already exists for
    /// the same `path`, focus it instead of opening a duplicate.
    pub fn open(&mut self, blueprint: Blueprint, path: Option<PathBuf>) -> usize {
        if let Some(p) = &path
            && let Some(i) = self.tabs.iter().position(|t| t.path.as_ref() == Some(p))
        {
            self.active = i;
            return i;
        }
        self.tabs.push(BlueprintEditor {
            blueprint,
            path,
            ..Default::default()
        });
        self.active = self.tabs.len() - 1;
        self.active
    }

    pub fn active(&self) -> Option<&BlueprintEditor> {
        self.tabs.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut BlueprintEditor> {
        self.tabs.get_mut(self.active)
    }

    pub fn close(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
    }

    /// Render the tab strip + active blueprint. Returns `true` when
    /// the active blueprint mutated this frame.
    pub fn show(&mut self, ui: &mut egui::Ui, registry: &NodeKindRegistry) -> bool {
        let mut dirty = false;
        let mut close_idx: Option<usize> = None;
        ui.horizontal_wrapped(|ui| {
            for (i, tab) in self.tabs.iter().enumerate() {
                let label = match &tab.blueprint.target {
                    BlueprintTarget::Entity { entity_name, .. } => {
                        format!("{} [{}]", tab.blueprint.name, entity_name)
                    }
                    BlueprintTarget::Class(c) => {
                        format!("{} (class {})", tab.blueprint.name, c)
                    }
                    BlueprintTarget::Global => tab.blueprint.name.clone(),
                };
                if ui.selectable_label(i == self.active, label).clicked() {
                    self.active = i;
                }
                if ui.small_button("x").clicked() {
                    close_idx = Some(i);
                }
                ui.separator();
            }
        });
        if let Some(i) = close_idx {
            self.close(i);
        }
        ui.separator();
        if self.tabs.is_empty() {
            ui.label(
                "(no blueprint open - double-click an entity in the scene hierarchy to start)",
            );
            return dirty;
        }
        if let Some(tab) = self.tabs.get_mut(self.active)
            && tab.show(ui, registry)
        {
            dirty = true;
        }
        dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_connect_accepts_matching() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let p = g.add_node("print", [0.0, 0.0]);
        let ok = g.connect(&reg, tick, "exec", p, "in").unwrap();
        assert!(ok);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn typed_connect_rejects_mismatch() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let c = g.add_node("const_float", [0.0, 0.0]);
        let p = g.add_node("print", [0.0, 0.0]);
        // Float -> Exec is a type mismatch.
        let res = g.connect(&reg, c, "value", p, "in");
        assert!(matches!(res, Err(ConnectError::TypeMismatch { .. })));
        assert_eq!(g.edges.len(), 0);
    }

    #[test]
    fn duplicate_connect_rejected() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let a = g.add_node("on_tick", [0.0, 0.0]);
        let b = g.add_node("print", [0.0, 0.0]);
        assert!(g.connect(&reg, a, "exec", b, "in").unwrap());
        assert!(!g.connect(&reg, a, "exec", b, "in").unwrap());
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn blueprint_round_trip() {
        let bp = Blueprint {
            name: "TestBP".into(),
            target: BlueprintTarget::Entity {
                entity_id: 7,
                entity_name: "cube".into(),
            },
            graph: Graph::default(),
            variables: Vec::new(),
            functions: Vec::new(),
            macros: Vec::new(),
        };
        let s = ron::ser::to_string(&bp).unwrap();
        let back: Blueprint = ron::de::from_str(&s).unwrap();
        assert_eq!(back.name, "TestBP");
        match back.target {
            BlueprintTarget::Entity {
                entity_id,
                entity_name,
            } => {
                assert_eq!(entity_id, 7);
                assert_eq!(entity_name, "cube");
            }
            _ => panic!("wrong target"),
        }
    }

    #[test]
    fn registry_register_and_get() {
        let mut reg = NodeKindRegistry::new();
        const PINS: &[PinSpec] = &[PinSpec::new("v", PinType::Float)];
        reg.register(NodeKindDef {
            id: "test_custom",
            label: "Test Custom",
            category: "Custom",
            inputs: PINS,
            outputs: &[],
            contexts: &[],
        });
        assert!(reg.get("test_custom").is_some());
    }
}
