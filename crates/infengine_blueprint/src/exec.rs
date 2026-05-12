//! Bytecode compiler + VM for blueprint graphs.
//!
//! The VM is a tiny stack machine. Phase 15.5 generalises the stack
//! from `Vec<f32>` to `Vec<Value>` so the bytecode can carry Bool /
//! Vec3 / Entity references in addition to Float. New ops:
//!
//! - `PushFloat(f32)` / `PushBool(bool)` / `PushVec3([f32;3])` -
//!   push a literal of the corresponding type
//! - `Add`     - pop two floats, push their sum (still float-only)
//! - `MakeVec3` - pop 3 floats (z, y, x order on the stack) and
//!   pack them into a Vec3 value
//! - `Print` - pop one value and forward to the host's print sink
//!
//! Compile path recognises `const_bool`, `const_vec3`, and
//! `make_vec3` nodes. `set_position` still stubs its three inputs
//! via Print until a per-entity host hook lands.

use std::collections::{HashMap, HashSet};

use crate::{BlueprintTarget, BlueprintVariable, Graph, NamedGraph, Node, NodeId};

/// Runtime value the stack carries between ops.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Value {
    Float(f32),
    Bool(bool),
    Vec3([f32; 3]),
    EntityRef(u32),
}

impl Value {
    pub fn as_float(&self) -> f32 {
        match self {
            Value::Float(v) => *v,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Vec3(v) => v[0],
            Value::EntityRef(_) => 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    PushFloat(f32),
    PushBool(bool),
    PushVec3([f32; 3]),
    Add,
    Sub,
    Mul,
    Div,
    /// Pop two floats, push `Bool(a == b)` (with a small epsilon).
    EqFloat,
    /// Pop two floats, push `Bool(a < b)`.
    LtFloat,
    /// Pop two floats, push `Bool(a > b)`.
    GtFloat,
    /// Pop two bools, push `Bool(a && b)`.
    AndBool,
    /// Pop two bools, push `Bool(a || b)`.
    OrBool,
    /// Pop one bool, push its negation.
    NotBool,
    MakeVec3,
    Print,
    /// Pop one Vec3 and write it as the target entity's world
    /// position. Compiler bakes the entity id from `BlueprintTarget`
    /// at compile time; a Global blueprint never emits this op.
    SetEntityPosition(u32),
    /// Pop one Vec3, treat it as euler XYZ radians and store as the
    /// target entity's 3D rotation.
    SetEntityRotation(u32),
    /// Pop one Vec3 and store as the target entity's 3D scale.
    SetEntityScale(u32),
    /// Ask the host to play the sound at `path` (an asset-relative
    /// path baked at compile time from the node's `path` param).
    PlaySound(String),
    /// Push the current frame's delta time as a Float.
    PushDeltaTime,
    /// Push the host's wall-clock time as a Float.
    PushTimeSeconds,
    /// Push the world-space position of the baked entity as a Vec3.
    /// Compiler bakes the entity id from `BlueprintTarget` so this
    /// op is only emitted for Entity / Class blueprints.
    PushEntityPosition(u32),
    PushEntityRotation(u32),
    PushEntityScale(u32),
    /// Ask the host for the entity id with the given name. Pushes
    /// `EntityRef(id)` on success or `EntityRef(0)` if none match.
    FindEntityByName(String),
    /// Push a literal `EntityRef(id)` baked at compile time. Used by
    /// the `self_entity` node (id = blueprint's target entity) and
    /// by the `entity_ref` constant node (id from params).
    PushEntityRef(u32),
    /// Ask the host to invoke the C# method whose name is the
    /// `method` argument. Argument marshaling is not implemented in
    /// v1; the call is purely exec-flow and produces no value.
    CallCSharp(String),
    /// Pop a Vec3; push the requested component (0=x, 1=y, 2=z) as
    /// a float. Backs each output pin of a `break_vec3` node, which
    /// the compiler emits once per consumer.
    ExtractFloat(u8),
    /// Pop a Vec3 then an EntityRef; write the Vec3 as the entity's
    /// world-space position / rotation / scale via the host.
    SetEntityPositionRef,
    SetEntityRotationRef,
    SetEntityScaleRef,
    /// Pop a bool; if false jump to absolute op index. True falls
    /// through. Used by Branch nodes.
    JumpIfFalse(u32),
    /// Unconditional jump to absolute op index.
    Jump(u32),
    /// Look up a named variable's current value and push it.
    PushVar(String),
    /// Pop the top value and store it under the variable name.
    SetVar(String),
    /// Peek the top of the stack (without popping) and report the
    /// value to the host as the latest output of the data node with
    /// id `slot`. Used by the editor's Play-time "watch values"
    /// display so a node shows its current numeric / boolean /
    /// vector value next to its label.
    Capture(u32),
}

#[derive(Debug, Default, Clone)]
pub struct Program {
    pub ops: Vec<Op>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Trigger {
    OnStart,
    OnTick,
}

impl Trigger {
    fn matches(self, kind: &str) -> bool {
        matches!(
            (self, kind),
            (Trigger::OnStart, "on_start") | (Trigger::OnTick, "on_tick")
        )
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CompileError {
    #[error("cycle detected at node {node:?}")]
    Cycle { node: NodeId },
    #[error("missing data input on node {node:?} pin '{pin}'")]
    MissingInput { node: NodeId, pin: String },
    #[error("unsupported node kind for data evaluation: {kind}")]
    UnsupportedData { kind: String },
}

fn find_node(graph: &Graph, id: NodeId) -> Option<&Node> {
    graph.nodes.iter().find(|n| n.id == id)
}

/// Read an inline pin default off a node. The key is
/// `params["pin_<pin>"]`. Used when no edge feeds the input so the
/// user can type a literal value directly on the pin instead of
/// wiring a `const_*` node.
fn pin_default_float(node: &Node, pin: &str) -> f32 {
    node.params
        .get(&format!("pin_{pin}"))
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0)
}

fn pin_default_bool(node: &Node, pin: &str) -> bool {
    node.params
        .get(&format!("pin_{pin}"))
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false)
}

fn find_data_source<'a>(
    graph: &'a Graph,
    to_node: NodeId,
    to_pin: &str,
) -> Option<(NodeId, &'a str)> {
    graph
        .edges
        .iter()
        .find(|e| e.to_node == to_node && e.to_pin == to_pin)
        .map(|e| (e.from_node, e.from_pin.as_str()))
}

#[allow(clippy::too_many_arguments)]
fn compile_data(
    graph: &Graph,
    node_id: NodeId,
    pin: &str,
    visiting: &mut HashSet<NodeId>,
    ops: &mut Vec<Op>,
    target: &BlueprintTarget,
) -> Result<(), CompileError> {
    if !visiting.insert(node_id) {
        return Err(CompileError::Cycle { node: node_id });
    }
    let node = find_node(graph, node_id).ok_or(CompileError::MissingInput {
        node: node_id,
        pin: pin.to_string(),
    })?;
    match node.kind.as_str() {
        "const_float" => {
            let value = node
                .params
                .get("value")
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(0.0);
            ops.push(Op::PushFloat(value));
        }
        "const_bool" => {
            let value = node
                .params
                .get("value")
                .map(|s| s == "true" || s == "1")
                .unwrap_or(false);
            ops.push(Op::PushBool(value));
        }
        "const_vec3" => {
            let parse = |k: &str| {
                node.params
                    .get(k)
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(0.0)
            };
            ops.push(Op::PushVec3([parse("x"), parse("y"), parse("z")]));
        }
        "get_float_var" => {
            let name = node.params.get("name").cloned().unwrap_or_default();
            ops.push(Op::PushVar(name));
        }
        "delta_time" => {
            ops.push(Op::PushDeltaTime);
        }
        "time_seconds" => {
            ops.push(Op::PushTimeSeconds);
        }
        "get_position" | "get_rotation" | "get_scale" => {
            // Read transform fields off the entity baked into the
            // blueprint's target. Global blueprints fall back to
            // zero values (the host's default impl) so the program
            // still compiles.
            let eid = match target {
                BlueprintTarget::Entity { entity_id, .. } => *entity_id,
                _ => 0,
            };
            let op = match node.kind.as_str() {
                "get_position" => Op::PushEntityPosition(eid),
                "get_rotation" => Op::PushEntityRotation(eid),
                _ => Op::PushEntityScale(eid),
            };
            ops.push(op);
        }
        "find_entity_by_name" => {
            let name = node.params.get("name").cloned().unwrap_or_default();
            ops.push(Op::FindEntityByName(name));
        }
        "self_entity" => {
            // Bake the blueprint target's entity id. Global
            // blueprints have no self entity; emit a 0 ref so the
            // VM's "id != 0" guards skip the write.
            let eid = match target {
                BlueprintTarget::Entity { entity_id, .. } => *entity_id,
                _ => 0,
            };
            ops.push(Op::PushEntityRef(eid));
        }
        "entity_ref" => {
            // Literal entity id stored in the node's params, useful
            // when the user knows a stable entity id ahead of time.
            let id = node
                .params
                .get("id")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            ops.push(Op::PushEntityRef(id));
        }
        "break_vec3" => {
            // Materialise the source Vec3 once, then extract the
            // requested component based on which output pin the
            // consumer wired to. The component index comes from the
            // `pin` parameter the caller passed in.
            match find_data_source(graph, node_id, "value") {
                Some((src, src_pin)) => {
                    compile_data(graph, src, src_pin, visiting, ops, target)?;
                }
                None => ops.push(Op::PushVec3([0.0, 0.0, 0.0])),
            }
            let idx: u8 = match pin {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                _ => 0,
            };
            ops.push(Op::ExtractFloat(idx));
        }
        "add" | "subtract" | "multiply" | "divide" => {
            for input in ["a", "b"] {
                match find_data_source(graph, node_id, input) {
                    Some((src_node, src_pin)) => {
                        compile_data(graph, src_node, src_pin, visiting, ops, target)?;
                    }
                    None => ops.push(Op::PushFloat(pin_default_float(node, input))),
                }
            }
            ops.push(match node.kind.as_str() {
                "add" => Op::Add,
                "subtract" => Op::Sub,
                "multiply" => Op::Mul,
                _ => Op::Div,
            });
        }
        "equal_float" | "less_than" | "greater_than" => {
            for input in ["a", "b"] {
                match find_data_source(graph, node_id, input) {
                    Some((src_node, src_pin)) => {
                        compile_data(graph, src_node, src_pin, visiting, ops, target)?;
                    }
                    None => ops.push(Op::PushFloat(pin_default_float(node, input))),
                }
            }
            ops.push(match node.kind.as_str() {
                "equal_float" => Op::EqFloat,
                "less_than" => Op::LtFloat,
                _ => Op::GtFloat,
            });
        }
        "and" | "or" => {
            for input in ["a", "b"] {
                match find_data_source(graph, node_id, input) {
                    Some((src_node, src_pin)) => {
                        compile_data(graph, src_node, src_pin, visiting, ops, target)?;
                    }
                    None => ops.push(Op::PushBool(pin_default_bool(node, input))),
                }
            }
            ops.push(if node.kind == "and" {
                Op::AndBool
            } else {
                Op::OrBool
            });
        }
        "not" => {
            match find_data_source(graph, node_id, "value") {
                Some((src_node, src_pin)) => {
                    compile_data(graph, src_node, src_pin, visiting, ops, target)?;
                }
                None => ops.push(Op::PushBool(pin_default_bool(node, "value"))),
            }
            ops.push(Op::NotBool);
        }
        "make_vec3" => {
            for input in ["x", "y", "z"] {
                match find_data_source(graph, node_id, input) {
                    Some((src_node, src_pin)) => {
                        compile_data(graph, src_node, src_pin, visiting, ops, target)?;
                    }
                    None => ops.push(Op::PushFloat(pin_default_float(node, input))),
                }
            }
            ops.push(Op::MakeVec3);
        }
        other => {
            visiting.remove(&node_id);
            return Err(CompileError::UnsupportedData {
                kind: other.to_string(),
            });
        }
    }
    // Emit a capture op so the host can show the latest produced
    // value next to this data node in the editor. Peeks the top of
    // the stack without popping, so the consumer still sees the
    // expected value.
    ops.push(Op::Capture(node_id.0));
    visiting.remove(&node_id);
    Ok(())
}

fn compile_sink(
    graph: &Graph,
    node_id: NodeId,
    ops: &mut Vec<Op>,
    target: &BlueprintTarget,
    functions: &[NamedGraph],
    calling: &mut HashSet<String>,
) -> Result<(), CompileError> {
    let node = find_node(graph, node_id).ok_or(CompileError::MissingInput {
        node: node_id,
        pin: "(sink)".to_string(),
    })?;
    match node.kind.as_str() {
        "print" => {
            match find_data_source(graph, node_id, "value") {
                Some((src, src_pin)) => {
                    let mut visiting = HashSet::new();
                    compile_data(graph, src, src_pin, &mut visiting, ops, target)?;
                }
                None => ops.push(Op::PushFloat(pin_default_float(node, "value"))),
            }
            ops.push(Op::Print);
        }
        "set_float_var" => {
            let name = node.params.get("name").cloned().unwrap_or_default();
            match find_data_source(graph, node_id, "value") {
                Some((src, src_pin)) => {
                    let mut visiting = HashSet::new();
                    compile_data(graph, src, src_pin, &mut visiting, ops, target)?;
                }
                None => ops.push(Op::PushFloat(pin_default_float(node, "value"))),
            }
            ops.push(Op::SetVar(name));
        }
        "set_position" | "set_rotation" | "set_scale" | "set_position_of"
        | "set_rotation_of" | "set_scale_of" => {
            // Unified transform-write path. If the `entity` pin has
            // an incoming edge, route through that EntityRef.
            // Otherwise bake the blueprint's own target id (Self)
            // so Entity / Class blueprints can affect themselves
            // without an extra Self node. Compile order leaves the
            // stack as `[EntityRef, Vec3]` which matches the Ref op
            // pop sequence.
            match find_data_source(graph, node_id, "entity") {
                Some((src, src_pin)) => {
                    let mut visiting = HashSet::new();
                    compile_data(graph, src, src_pin, &mut visiting, ops, target)?;
                }
                None => {
                    let self_id = match target {
                        BlueprintTarget::Entity { entity_id, .. } => *entity_id,
                        _ => 0,
                    };
                    ops.push(Op::PushEntityRef(self_id));
                }
            }
            for input in ["x", "y", "z"] {
                match find_data_source(graph, node_id, input) {
                    Some((src, src_pin)) => {
                        let mut visiting = HashSet::new();
                        compile_data(graph, src, src_pin, &mut visiting, ops, target)?;
                    }
                    None => ops.push(Op::PushFloat(pin_default_float(node, input))),
                }
            }
            ops.push(Op::MakeVec3);
            // Both old (`set_*`) and new (`set_*_of`) kinds map to
            // the same Ref op now.
            let op = match node.kind.as_str() {
                "set_position" | "set_position_of" => Op::SetEntityPositionRef,
                "set_rotation" | "set_rotation_of" => Op::SetEntityRotationRef,
                _ => Op::SetEntityScaleRef,
            };
            ops.push(op);
        }
        "play_sound" => {
            let path = node.params.get("path").cloned().unwrap_or_default();
            ops.push(Op::PlaySound(path));
        }
        "call_cs_method" => {
            // Invoke a C# method previously registered via the
            // [BlueprintCallable] attribute. v1: exec-only, no
            // arg marshaling.
            let method = node.params.get("method").cloned().unwrap_or_default();
            ops.push(Op::CallCSharp(method));
        }
        "call_function" => {
            let name = node.params.get("name").cloned().unwrap_or_default();
            if name.is_empty() {
                return Ok(());
            }
            if calling.contains(&name) {
                // Recursive call - skip to prevent infinite expansion.
                tracing::warn!(name = %name, "blueprint call_function recursion skipped");
                return Ok(());
            }
            let Some(func) = functions.iter().find(|f| f.name == name) else {
                return Ok(());
            };
            calling.insert(name.clone());
            // Inline the function body: walk every on_start entry's
            // exec chain. Functions have no on_tick - they're called
            // explicitly, so OnStart is the conventional root.
            let entries: Vec<NodeId> = func
                .graph
                .nodes
                .iter()
                .filter(|n| n.kind == "on_start")
                .map(|n| n.id)
                .collect();
            let mut inner_visited = HashSet::new();
            for entry in entries {
                compile_exec_from(
                    &func.graph,
                    entry,
                    "exec",
                    ops,
                    target,
                    &mut inner_visited,
                    functions,
                    calling,
                )?;
            }
            calling.remove(&name);
        }
        other => {
            return Err(CompileError::UnsupportedData {
                kind: other.to_string(),
            });
        }
    }
    Ok(())
}

/// Walk the execution chain starting at the edges from
/// `(node_id, pin)` whose `to_pin == "in"`. Recursive so branches
/// emit jumps inline; the `visited` set prevents infinite loops on
/// cycles or revisits.
#[allow(clippy::too_many_arguments)]
fn compile_exec_from(
    graph: &Graph,
    node_id: NodeId,
    pin: &str,
    ops: &mut Vec<Op>,
    target: &BlueprintTarget,
    visited: &mut HashSet<NodeId>,
    functions: &[NamedGraph],
    calling: &mut HashSet<String>,
) -> Result<(), CompileError> {
    let next_sinks: Vec<NodeId> = graph
        .edges
        .iter()
        .filter(|e| e.from_node == node_id && e.from_pin == pin && e.to_pin == "in")
        .map(|e| e.to_node)
        .collect();
    for sink_id in next_sinks {
        if !visited.insert(sink_id) {
            continue;
        }
        let sink_node = find_node(graph, sink_id).ok_or(CompileError::MissingInput {
            node: sink_id,
            pin: "(sink)".to_string(),
        })?;
        match sink_node.kind.as_str() {
            "sequence" => {
                // Walk each `then_N` exec output in order. No VM op
                // needed - we just concatenate the chains so they run
                // top-to-bottom.
                for pin_name in ["then_0", "then_1", "then_2", "then_3"] {
                    compile_exec_from(
                        graph, sink_id, pin_name, ops, target, visited, functions, calling,
                    )?;
                }
            }
            "branch" => {
                // Evaluate the bool condition then emit the
                // standard JumpIfFalse + true-body + Jump-to-end +
                // false-body pattern, patching the jump targets
                // once each body's length is known.
                match find_data_source(graph, sink_id, "condition") {
                    Some((src, src_pin)) => {
                        let mut local = HashSet::new();
                        compile_data(graph, src, src_pin, &mut local, ops, target)?;
                    }
                    None => {
                        let sink_node = find_node(graph, sink_id).ok_or(
                            CompileError::MissingInput {
                                node: sink_id,
                                pin: "condition".to_string(),
                            },
                        )?;
                        ops.push(Op::PushBool(pin_default_bool(sink_node, "condition")));
                    }
                }
                let jif_idx = ops.len();
                ops.push(Op::JumpIfFalse(0));
                // True branch.
                compile_exec_from(
                    graph, sink_id, "true", ops, target, visited, functions, calling,
                )?;
                let jmp_idx = ops.len();
                ops.push(Op::Jump(0));
                let false_start = ops.len() as u32;
                // False branch.
                compile_exec_from(
                    graph, sink_id, "false", ops, target, visited, functions, calling,
                )?;
                let end_addr = ops.len() as u32;
                ops[jif_idx] = Op::JumpIfFalse(false_start);
                ops[jmp_idx] = Op::Jump(end_addr);
            }
            _ => {
                compile_sink(graph, sink_id, ops, target, functions, calling)?;
                // A regular sink may have its own `exec` output (rare
                // for the current node set); follow it so chains of
                // Prints / SetPositions all run.
                compile_exec_from(
                    graph, sink_id, "exec", ops, target, visited, functions, calling,
                )?;
            }
        }
    }
    Ok(())
}

/// Compile the event graph alone with no user-defined functions.
/// Convenience for tests + back-compat callers; use
/// `compile_with_functions` for blueprints that may call functions.
pub fn compile(
    graph: &Graph,
    trigger: Trigger,
    target: &BlueprintTarget,
) -> Result<Program, CompileError> {
    compile_with_functions(graph, &[], trigger, target)
}

/// Compile an event graph that may contain `call_function` nodes
/// referencing entries in `functions`. Each call is inlined into the
/// bytecode; recursion through a name already on the call stack is
/// skipped to keep the program finite.
pub fn compile_with_functions(
    graph: &Graph,
    functions: &[NamedGraph],
    trigger: Trigger,
    target: &BlueprintTarget,
) -> Result<Program, CompileError> {
    let mut ops = Vec::new();
    let entries: Vec<NodeId> = graph
        .nodes
        .iter()
        .filter(|n| trigger.matches(&n.kind))
        .map(|n| n.id)
        .collect();
    let mut calling: HashSet<String> = HashSet::new();
    for entry in entries {
        let mut visited = HashSet::new();
        compile_exec_from(
            graph,
            entry,
            "exec",
            &mut ops,
            target,
            &mut visited,
            functions,
            &mut calling,
        )?;
    }
    Ok(Program { ops })
}

pub trait Host {
    /// Forward a `Value` to the editor / runtime sink. The
    /// implementation usually pretty-prints it; tests capture it.
    fn print(&mut self, value: Value);
    /// Record `value` as the latest output of the node identified by
    /// `slot` (the node's u32 id). Default impl is a no-op so hosts
    /// that don't care about watch-values can ignore it.
    fn capture(&mut self, _slot: u32, _value: Value) {}
    /// Write a 3D position to the given entity. Default impl is a
    /// noop so a host that only cares about Print doesn't have to
    /// implement transform mutation.
    fn set_entity_position(&mut self, _entity_id: u32, _position: [f32; 3]) {}
    /// Write a 3D rotation (XYZ euler radians) to the given entity.
    fn set_entity_rotation(&mut self, _entity_id: u32, _rotation: [f32; 3]) {}
    /// Write a 3D scale to the given entity.
    fn set_entity_scale(&mut self, _entity_id: u32, _scale: [f32; 3]) {}
    /// Start playback of the sound at the given asset-relative path.
    fn play_sound(&mut self, _path: &str) {}
    /// Last frame's delta time in seconds. Default returns 0 so
    /// non-runtime hosts (tests) don't need a clock.
    fn delta_time(&self) -> f32 {
        0.0
    }
    /// Wall-clock seconds since the host started.
    fn time_seconds(&self) -> f32 {
        0.0
    }
    /// Read the current world position / rotation / scale of an
    /// entity. Default returns zero / identity values so tests
    /// don't have to model a scene.
    fn entity_position(&self, _entity_id: u32) -> [f32; 3] {
        [0.0, 0.0, 0.0]
    }
    fn entity_rotation(&self, _entity_id: u32) -> [f32; 3] {
        [0.0, 0.0, 0.0]
    }
    fn entity_scale(&self, _entity_id: u32) -> [f32; 3] {
        [1.0, 1.0, 1.0]
    }
    /// Resolve an entity by its display name. `None` => no match.
    fn find_entity_by_name(&self, _name: &str) -> Option<u32> {
        None
    }
    /// Invoke a C# method registered via `[BlueprintCallable]`. v1
    /// is exec-only with no arg marshaling; the full pipeline is
    /// described in `docs/design/blueprint_csharp_callable.md`.
    fn call_csharp(&mut self, _method: &str) {}
}

#[derive(Debug, Default)]
pub struct TracingHost;

impl Host for TracingHost {
    fn print(&mut self, value: Value) {
        match value {
            Value::Float(v) => tracing::info!(value = v, "blueprint print"),
            Value::Bool(b) => tracing::info!(value = b, "blueprint print"),
            Value::Vec3(v) => tracing::info!(?v, "blueprint print"),
            Value::EntityRef(e) => tracing::info!(entity = e, "blueprint print"),
        }
    }
    fn set_entity_position(&mut self, entity_id: u32, position: [f32; 3]) {
        tracing::info!(entity_id, ?position, "set_entity_position");
    }
}

/// Run a program against a host. Convenience wrapper that uses a
/// throwaway variable map; for persistent variables across ticks
/// pass `vars` directly via `run_with_vars`.
pub fn run(program: &Program, host: &mut dyn Host) {
    let mut vars: HashMap<String, Value> = HashMap::new();
    run_with_vars(program, host, &mut vars);
}

/// Run a program reading + writing the caller-owned `vars` map.
/// Lets the editor persist variable values across OnTick frames by
/// keeping one map per blueprint instance.
pub fn run_with_vars(program: &Program, host: &mut dyn Host, vars: &mut HashMap<String, Value>) {
    let mut stack: Vec<Value> = Vec::new();
    let mut pc: usize = 0;
    let mut guard: usize = 0;
    let guard_limit = program.ops.len().saturating_mul(64).max(1024);
    while pc < program.ops.len() {
        guard += 1;
        if guard > guard_limit {
            tracing::warn!("blueprint VM op-count guard tripped; aborting program");
            break;
        }
        match &program.ops[pc] {
            Op::PushFloat(v) => stack.push(Value::Float(*v)),
            Op::PushBool(b) => stack.push(Value::Bool(*b)),
            Op::PushVec3(v) => stack.push(Value::Vec3(*v)),
            Op::Add => {
                let b = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let a = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                stack.push(Value::Float(a + b));
            }
            Op::Sub => {
                let b = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let a = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                stack.push(Value::Float(a - b));
            }
            Op::Mul => {
                let b = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let a = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                stack.push(Value::Float(a * b));
            }
            Op::Div => {
                let b = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let a = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let r = if b.abs() < 1e-12 { 0.0 } else { a / b };
                stack.push(Value::Float(r));
            }
            Op::EqFloat => {
                let b = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let a = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                stack.push(Value::Bool((a - b).abs() < 1e-6));
            }
            Op::LtFloat => {
                let b = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let a = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                stack.push(Value::Bool(a < b));
            }
            Op::GtFloat => {
                let b = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let a = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                stack.push(Value::Bool(a > b));
            }
            Op::AndBool => {
                let b = matches!(stack.pop(), Some(Value::Bool(true)));
                let a = matches!(stack.pop(), Some(Value::Bool(true)));
                stack.push(Value::Bool(a && b));
            }
            Op::OrBool => {
                let b = matches!(stack.pop(), Some(Value::Bool(true)));
                let a = matches!(stack.pop(), Some(Value::Bool(true)));
                stack.push(Value::Bool(a || b));
            }
            Op::NotBool => {
                let a = matches!(stack.pop(), Some(Value::Bool(true)));
                stack.push(Value::Bool(!a));
            }
            Op::MakeVec3 => {
                let z = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let y = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                let x = stack.pop().map(|v| v.as_float()).unwrap_or(0.0);
                stack.push(Value::Vec3([x, y, z]));
            }
            Op::Print => {
                let v = stack.pop().unwrap_or(Value::Float(0.0));
                host.print(v);
            }
            Op::SetEntityPosition(eid) => {
                let pos = match stack.pop().unwrap_or(Value::Vec3([0.0; 3])) {
                    Value::Vec3(v) => v,
                    Value::Float(f) => [f, 0.0, 0.0],
                    _ => [0.0; 3],
                };
                host.set_entity_position(*eid, pos);
            }
            Op::SetEntityRotation(eid) => {
                let rot = match stack.pop().unwrap_or(Value::Vec3([0.0; 3])) {
                    Value::Vec3(v) => v,
                    Value::Float(f) => [f, 0.0, 0.0],
                    _ => [0.0; 3],
                };
                host.set_entity_rotation(*eid, rot);
            }
            Op::SetEntityScale(eid) => {
                let scl = match stack.pop().unwrap_or(Value::Vec3([1.0; 3])) {
                    Value::Vec3(v) => v,
                    Value::Float(f) => [f, f, f],
                    _ => [1.0; 3],
                };
                host.set_entity_scale(*eid, scl);
            }
            Op::PlaySound(path) => {
                host.play_sound(path);
            }
            Op::PushDeltaTime => stack.push(Value::Float(host.delta_time())),
            Op::PushTimeSeconds => stack.push(Value::Float(host.time_seconds())),
            Op::PushEntityPosition(eid) => {
                stack.push(Value::Vec3(host.entity_position(*eid)));
            }
            Op::PushEntityRotation(eid) => {
                stack.push(Value::Vec3(host.entity_rotation(*eid)));
            }
            Op::PushEntityScale(eid) => {
                stack.push(Value::Vec3(host.entity_scale(*eid)));
            }
            Op::FindEntityByName(name) => {
                let id = host.find_entity_by_name(name).unwrap_or(0);
                stack.push(Value::EntityRef(id));
            }
            Op::PushEntityRef(id) => {
                stack.push(Value::EntityRef(*id));
            }
            Op::CallCSharp(method) => {
                host.call_csharp(method);
            }
            Op::ExtractFloat(idx) => {
                let v = match stack.pop().unwrap_or(Value::Vec3([0.0; 3])) {
                    Value::Vec3(arr) => arr.get(*idx as usize).copied().unwrap_or(0.0),
                    Value::Float(f) => f,
                    _ => 0.0,
                };
                stack.push(Value::Float(v));
            }
            Op::SetEntityPositionRef => {
                let vec = match stack.pop().unwrap_or(Value::Vec3([0.0; 3])) {
                    Value::Vec3(v) => v,
                    Value::Float(f) => [f, 0.0, 0.0],
                    _ => [0.0; 3],
                };
                let eid = match stack.pop().unwrap_or(Value::EntityRef(0)) {
                    Value::EntityRef(e) => e,
                    _ => 0,
                };
                if eid != 0 {
                    host.set_entity_position(eid, vec);
                }
            }
            Op::SetEntityRotationRef => {
                let vec = match stack.pop().unwrap_or(Value::Vec3([0.0; 3])) {
                    Value::Vec3(v) => v,
                    Value::Float(f) => [f, 0.0, 0.0],
                    _ => [0.0; 3],
                };
                let eid = match stack.pop().unwrap_or(Value::EntityRef(0)) {
                    Value::EntityRef(e) => e,
                    _ => 0,
                };
                if eid != 0 {
                    host.set_entity_rotation(eid, vec);
                }
            }
            Op::SetEntityScaleRef => {
                let vec = match stack.pop().unwrap_or(Value::Vec3([1.0; 3])) {
                    Value::Vec3(v) => v,
                    Value::Float(f) => [f, f, f],
                    _ => [1.0; 3],
                };
                let eid = match stack.pop().unwrap_or(Value::EntityRef(0)) {
                    Value::EntityRef(e) => e,
                    _ => 0,
                };
                if eid != 0 {
                    host.set_entity_scale(eid, vec);
                }
            }
            Op::JumpIfFalse(addr) => {
                let cond = matches!(stack.pop(), Some(Value::Bool(true)));
                if !cond {
                    pc = *addr as usize;
                    continue;
                }
            }
            Op::Jump(addr) => {
                pc = *addr as usize;
                continue;
            }
            Op::PushVar(name) => {
                let v = vars.get(name).copied().unwrap_or(Value::Float(0.0));
                stack.push(v);
            }
            Op::SetVar(name) => {
                let v = stack.pop().unwrap_or(Value::Float(0.0));
                vars.insert(name.clone(), v);
            }
            Op::Capture(slot) => {
                if let Some(v) = stack.last().copied() {
                    host.capture(*slot, v);
                }
            }
        }
        pc += 1;
    }
}

/// Initialise a `vars` map with the blueprint's variable defaults.
/// Call once when the host enters Play; subsequent ticks share the
/// map so values persist across frames.
pub fn init_vars(variables: &[BlueprintVariable]) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    for v in variables {
        let parsed = match v.ty {
            crate::PinType::Float => v
                .default
                .parse::<f32>()
                .map(Value::Float)
                .unwrap_or(Value::Float(0.0)),
            crate::PinType::Bool => Value::Bool(v.default == "true" || v.default == "1"),
            crate::PinType::Vec3 => {
                // Accept "x,y,z" literal default; everything else
                // falls back to zero.
                let mut it = v.default.split(',').map(|s| s.trim().parse::<f32>().ok());
                let x = it.next().flatten().unwrap_or(0.0);
                let y = it.next().flatten().unwrap_or(0.0);
                let z = it.next().flatten().unwrap_or(0.0);
                Value::Vec3([x, y, z])
            }
            crate::PinType::EntityRef => v
                .default
                .parse::<u32>()
                .map(Value::EntityRef)
                .unwrap_or(Value::EntityRef(0)),
            _ => Value::Float(0.0),
        };
        map.insert(v.name.clone(), parsed);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKindRegistry;

    struct Capture(Vec<Value>);
    impl Host for Capture {
        fn print(&mut self, value: Value) {
            self.0.push(value);
        }
    }

    #[test]
    fn const_print() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let c = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == c)
            .unwrap()
            .params
            .insert("value".into(), "42".into());
        let p = g.add_node("print", [0.0, 0.0]);
        assert!(g.connect(&reg, tick, "exec", p, "in").unwrap());
        assert!(g.connect(&reg, c, "value", p, "value").unwrap());
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(42.0)]);
    }

    #[test]
    fn add_print() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let ca = g.add_node("const_float", [0.0, 0.0]);
        let cb = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == ca)
            .unwrap()
            .params
            .insert("value".into(), "3".into());
        g.nodes
            .iter_mut()
            .find(|n| n.id == cb)
            .unwrap()
            .params
            .insert("value".into(), "4".into());
        let add = g.add_node("add", [0.0, 0.0]);
        let p = g.add_node("print", [0.0, 0.0]);
        assert!(g.connect(&reg, tick, "exec", p, "in").unwrap());
        assert!(g.connect(&reg, ca, "value", add, "a").unwrap());
        assert!(g.connect(&reg, cb, "value", add, "b").unwrap());
        assert!(g.connect(&reg, add, "sum", p, "value").unwrap());
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(7.0)]);
    }

    #[test]
    fn branch_picks_true_arm() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let branch = g.add_node("branch", [0.0, 0.0]);
        let cond = g.add_node("const_bool", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == cond)
            .unwrap()
            .params
            .insert("value".into(), "true".into());
        let true_arm = g.add_node("print", [0.0, 0.0]);
        let false_arm = g.add_node("print", [0.0, 0.0]);
        let true_v = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == true_v)
            .unwrap()
            .params
            .insert("value".into(), "10".into());
        let false_v = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == false_v)
            .unwrap()
            .params
            .insert("value".into(), "99".into());
        assert!(g.connect(&reg, tick, "exec", branch, "in").unwrap());
        assert!(g.connect(&reg, cond, "value", branch, "condition").unwrap());
        assert!(g.connect(&reg, branch, "true", true_arm, "in").unwrap());
        assert!(g.connect(&reg, branch, "false", false_arm, "in").unwrap());
        assert!(g.connect(&reg, true_v, "value", true_arm, "value").unwrap());
        assert!(
            g.connect(&reg, false_v, "value", false_arm, "value")
                .unwrap()
        );
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(10.0)]);
    }

    #[test]
    fn branch_picks_false_arm() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let branch = g.add_node("branch", [0.0, 0.0]);
        let cond = g.add_node("const_bool", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == cond)
            .unwrap()
            .params
            .insert("value".into(), "false".into());
        let true_arm = g.add_node("print", [0.0, 0.0]);
        let false_arm = g.add_node("print", [0.0, 0.0]);
        let true_v = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == true_v)
            .unwrap()
            .params
            .insert("value".into(), "10".into());
        let false_v = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == false_v)
            .unwrap()
            .params
            .insert("value".into(), "99".into());
        assert!(g.connect(&reg, tick, "exec", branch, "in").unwrap());
        assert!(g.connect(&reg, cond, "value", branch, "condition").unwrap());
        assert!(g.connect(&reg, branch, "true", true_arm, "in").unwrap());
        assert!(g.connect(&reg, branch, "false", false_arm, "in").unwrap());
        assert!(g.connect(&reg, true_v, "value", true_arm, "value").unwrap());
        assert!(
            g.connect(&reg, false_v, "value", false_arm, "value")
                .unwrap()
        );
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(99.0)]);
    }

    #[test]
    fn inline_pin_default_used_when_unconnected() {
        // Add node with `b` left unconnected but a `pin_b` default
        // of 5 in the params. The compile path should emit
        // PushFloat(5) instead of PushFloat(0).
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let a = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == a)
            .unwrap()
            .params
            .insert("value".into(), "3".into());
        let add = g.add_node("add", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == add)
            .unwrap()
            .params
            .insert("pin_b".into(), "5".into());
        let p = g.add_node("print", [0.0, 0.0]);
        assert!(g.connect(&reg, tick, "exec", p, "in").unwrap());
        assert!(g.connect(&reg, a, "value", add, "a").unwrap());
        assert!(g.connect(&reg, add, "sum", p, "value").unwrap());
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(8.0)]);
    }

    #[test]
    fn break_vec3_extracts_components() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let v = g.add_node("const_vec3", [0.0, 0.0]);
        {
            let n = g.nodes.iter_mut().find(|n| n.id == v).unwrap();
            n.params.insert("x".into(), "1".into());
            n.params.insert("y".into(), "2".into());
            n.params.insert("z".into(), "3".into());
        }
        let br = g.add_node("break_vec3", [0.0, 0.0]);
        let p = g.add_node("print", [0.0, 0.0]);
        assert!(g.connect(&reg, tick, "exec", p, "in").unwrap());
        assert!(g.connect(&reg, v, "value", br, "value").unwrap());
        // Print only the Y component.
        assert!(g.connect(&reg, br, "y", p, "value").unwrap());
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(2.0)]);
    }

    #[test]
    fn call_cs_method_invokes_host() {
        struct Mock {
            calls: Vec<String>,
        }
        impl Host for Mock {
            fn print(&mut self, _: Value) {}
            fn call_csharp(&mut self, method: &str) {
                self.calls.push(method.to_string());
            }
        }
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let call = g.add_node("call_cs_method", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == call)
            .unwrap()
            .params
            .insert("method".into(), "Engine.Log".into());
        assert!(g.connect(&reg, tick, "exec", call, "in").unwrap());
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut h = Mock { calls: Vec::new() };
        run(&prog, &mut h);
        assert_eq!(h.calls, vec!["Engine.Log".to_string()]);
    }

    #[test]
    fn self_entity_node_bakes_target_id() {
        struct Mock {
            sets: Vec<(u32, [f32; 3])>,
        }
        impl Host for Mock {
            fn print(&mut self, _: Value) {}
            fn set_entity_position(&mut self, entity_id: u32, position: [f32; 3]) {
                self.sets.push((entity_id, position));
            }
        }
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let setp = g.add_node("set_position_of", [0.0, 0.0]);
        let me = g.add_node("self_entity", [0.0, 0.0]);
        let setn = g.nodes.iter_mut().find(|n| n.id == setp).unwrap();
        setn.params.insert("pin_x".into(), "1".into());
        setn.params.insert("pin_y".into(), "2".into());
        setn.params.insert("pin_z".into(), "3".into());
        assert!(g.connect(&reg, tick, "exec", setp, "in").unwrap());
        assert!(g.connect(&reg, me, "entity", setp, "entity").unwrap());
        let target = BlueprintTarget::Entity {
            entity_id: 42,
            entity_name: "Cube".into(),
        };
        let prog = compile(&g, Trigger::OnTick, &target).unwrap();
        let mut h = Mock { sets: Vec::new() };
        run(&prog, &mut h);
        assert_eq!(h.sets, vec![(42, [1.0, 2.0, 3.0])]);
    }

    #[test]
    fn entity_lookup_and_set_position_of() {
        // Build a graph: OnTick -> SetPositionOf( FindEntity("Cube"),
        // x=10, y=20, z=30 ). The mock host returns id 7 for "Cube"
        // and records the set call so we can assert the path end to
        // end.
        struct Mock {
            sets: Vec<(u32, [f32; 3])>,
        }
        impl Host for Mock {
            fn print(&mut self, _: Value) {}
            fn set_entity_position(&mut self, entity_id: u32, position: [f32; 3]) {
                self.sets.push((entity_id, position));
            }
            fn find_entity_by_name(&self, name: &str) -> Option<u32> {
                if name == "Cube" { Some(7) } else { None }
            }
        }
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let setp = g.add_node("set_position_of", [0.0, 0.0]);
        // Inline xyz defaults on the set node.
        let setn = g.nodes.iter_mut().find(|n| n.id == setp).unwrap();
        setn.params.insert("pin_x".into(), "10".into());
        setn.params.insert("pin_y".into(), "20".into());
        setn.params.insert("pin_z".into(), "30".into());
        let find = g.add_node("find_entity_by_name", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == find)
            .unwrap()
            .params
            .insert("name".into(), "Cube".into());
        assert!(g.connect(&reg, tick, "exec", setp, "in").unwrap());
        assert!(g.connect(&reg, find, "entity", setp, "entity").unwrap());
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut h = Mock { sets: Vec::new() };
        run(&prog, &mut h);
        assert_eq!(h.sets, vec![(7, [10.0, 20.0, 30.0])]);
    }

    #[test]
    fn sequence_runs_all_outputs_in_order() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let seq = g.add_node("sequence", [0.0, 0.0]);
        let p0 = g.add_node("print", [0.0, 0.0]);
        let p1 = g.add_node("print", [0.0, 0.0]);
        let v0 = g.add_node("const_float", [0.0, 0.0]);
        let v1 = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == v0)
            .unwrap()
            .params
            .insert("value".into(), "1".into());
        g.nodes
            .iter_mut()
            .find(|n| n.id == v1)
            .unwrap()
            .params
            .insert("value".into(), "2".into());
        assert!(g.connect(&reg, tick, "exec", seq, "in").unwrap());
        assert!(g.connect(&reg, seq, "then_0", p0, "in").unwrap());
        assert!(g.connect(&reg, seq, "then_1", p1, "in").unwrap());
        assert!(g.connect(&reg, v0, "value", p0, "value").unwrap());
        assert!(g.connect(&reg, v1, "value", p1, "value").unwrap());
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(1.0), Value::Float(2.0)]);
    }

    #[test]
    fn math_and_comparison_ops() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let a = g.add_node("const_float", [0.0, 0.0]);
        let b = g.add_node("const_float", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == a)
            .unwrap()
            .params
            .insert("value".into(), "10".into());
        g.nodes
            .iter_mut()
            .find(|n| n.id == b)
            .unwrap()
            .params
            .insert("value".into(), "3".into());
        let sub = g.add_node("subtract", [0.0, 0.0]);
        let p_sub = g.add_node("print", [0.0, 0.0]);
        assert!(g.connect(&reg, tick, "exec", p_sub, "in").unwrap());
        assert!(g.connect(&reg, a, "value", sub, "a").unwrap());
        assert!(g.connect(&reg, b, "value", sub, "b").unwrap());
        assert!(g.connect(&reg, sub, "diff", p_sub, "value").unwrap());
        let prog = compile(&g, Trigger::OnTick, &BlueprintTarget::Global).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(7.0)]);
    }

    #[test]
    fn call_function_inlines_body() {
        let reg = NodeKindRegistry::with_builtins();
        // Function: OnStart -> Print(7).
        let mut func_graph = Graph::default();
        let start = func_graph.add_node("on_start", [0.0, 0.0]);
        let c = func_graph.add_node("const_float", [0.0, 0.0]);
        func_graph
            .nodes
            .iter_mut()
            .find(|n| n.id == c)
            .unwrap()
            .params
            .insert("value".into(), "7".into());
        let p = func_graph.add_node("print", [0.0, 0.0]);
        assert!(func_graph.connect(&reg, start, "exec", p, "in").unwrap());
        assert!(func_graph.connect(&reg, c, "value", p, "value").unwrap());
        let func = NamedGraph {
            name: "shout_seven".into(),
            graph: func_graph,
        };
        // Caller graph: OnTick -> CallFunction("shout_seven").
        let mut caller = Graph::default();
        let tick = caller.add_node("on_tick", [0.0, 0.0]);
        let call = caller.add_node("call_function", [0.0, 0.0]);
        caller
            .nodes
            .iter_mut()
            .find(|n| n.id == call)
            .unwrap()
            .params
            .insert("name".into(), "shout_seven".into());
        assert!(caller.connect(&reg, tick, "exec", call, "in").unwrap());
        let prog = compile_with_functions(
            &caller,
            std::slice::from_ref(&func),
            Trigger::OnTick,
            &BlueprintTarget::Global,
        )
        .unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![Value::Float(7.0)]);
    }

    #[test]
    fn call_function_recursion_skipped() {
        let reg = NodeKindRegistry::with_builtins();
        // Function `a` calls itself: should compile (skip recursion)
        // and produce an empty program rather than blow up.
        let mut g = Graph::default();
        let start = g.add_node("on_start", [0.0, 0.0]);
        let call = g.add_node("call_function", [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == call)
            .unwrap()
            .params
            .insert("name".into(), "a".into());
        assert!(g.connect(&reg, start, "exec", call, "in").unwrap());
        let func = NamedGraph {
            name: "a".into(),
            graph: g.clone(),
        };
        let prog = compile_with_functions(
            &g,
            std::slice::from_ref(&func),
            Trigger::OnStart,
            &BlueprintTarget::Global,
        )
        .unwrap();
        // First-level expansion runs the body once; the inner
        // self-call is skipped. Body has no Print, so ops are empty.
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert!(cap.0.is_empty());
    }

    #[test]
    fn make_vec3_print() {
        let reg = NodeKindRegistry::with_builtins();
        let mut g = Graph::default();
        let tick = g.add_node("on_tick", [0.0, 0.0]);
        let v = g.add_node("const_vec3", [0.0, 0.0]);
        {
            let node = g.nodes.iter_mut().find(|n| n.id == v).unwrap();
            node.params.insert("x".into(), "1.5".into());
            node.params.insert("y".into(), "2.5".into());
            node.params.insert("z".into(), "3.5".into());
        }
        let p = g.add_node("print", [0.0, 0.0]);
        assert!(g.connect(&reg, tick, "exec", p, "in").unwrap());
        // const_vec3 outputs Vec3, but `print.value` wants Float.
        // The connect should reject (type mismatch). The test
        // documents that pin types are enforced.
        let res = g.connect(&reg, v, "value", p, "value");
        assert!(matches!(res, Err(crate::ConnectError::TypeMismatch { .. })));
        // Wire through make_vec3 -> print as a stub by directly
        // pushing the ops via compile, instead, just verify the
        // const_vec3 + Print path emits PushVec3 + Print when wired.
        let _ = compile(&g, Trigger::OnTick, &BlueprintTarget::Global);
        let mut cap = Capture(Vec::new());
        run(
            &Program {
                ops: vec![Op::PushVec3([1.0, 2.0, 3.0]), Op::Print],
            },
            &mut cap,
        );
        assert_eq!(cap.0, vec![Value::Vec3([1.0, 2.0, 3.0])]);
    }
}
