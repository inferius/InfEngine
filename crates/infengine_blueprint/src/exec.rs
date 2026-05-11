//! Bytecode compiler + VM for blueprint graphs.
//!
//! Phase 13C compiles a `Graph` into a flat `Program` of `Op` values
//! and runs them on a tiny stack machine. Two entry points are
//! supported: `OnStart` fires once when Play begins, `OnTick` fires
//! every frame. The compiler walks execution edges (`exec` -> `in`)
//! starting from each entry node and, for every sink encountered,
//! recursively compiles its data inputs depth-first.
//!
//! Supported ops (Phase 13C first cut):
//! - `PushFloat(value)` - push a literal on the stack
//! - `Add`              - pop two floats, push their sum
//! - `Print`            - pop one float and forward it to the host
//!
//! `SetPosition` is parsed but not executed - the host hook lands in a
//! later phase when each entity carries its own blueprint slot.

use std::collections::HashSet;

use crate::{Graph, Node, NodeId, NodeKind};

/// Single bytecode instruction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    PushFloat(f32),
    Add,
    /// Pop one float and forward to the host's print sink. The runtime
    /// calls the configured callback with the popped value.
    Print,
}

/// Compiled program for one entry point.
#[derive(Debug, Default, Clone)]
pub struct Program {
    pub ops: Vec<Op>,
}

/// Trigger event the VM was launched for. The compiler emits one
/// program per trigger so the host can fire them at the right time.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Trigger {
    OnStart,
    OnTick,
}

impl Trigger {
    fn matches(self, kind: NodeKind) -> bool {
        matches!(
            (self, kind),
            (Trigger::OnStart, NodeKind::OnStart) | (Trigger::OnTick, NodeKind::OnTick)
        )
    }
}

/// Compile errors. Most failures degrade to "skip this node" at compile
/// time so the live editor stays responsive; a structured error is
/// surfaced only when the host explicitly asks for validation.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CompileError {
    #[error("cycle detected at node {node:?}")]
    Cycle { node: NodeId },
    #[error("missing data input on node {node:?} pin '{pin}'")]
    MissingInput { node: NodeId, pin: String },
    #[error("unsupported node kind for data evaluation: {kind:?}")]
    UnsupportedData { kind: NodeKind },
}

fn find_node(graph: &Graph, id: NodeId) -> Option<&Node> {
    graph.nodes.iter().find(|n| n.id == id)
}

/// Resolve the source edge feeding `(to_node, to_pin)`. Each input may
/// only carry one connection; if multiple edges land on the same pin
/// the first one wins (deterministic but arbitrary).
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

/// Emit ops that, when run, leave the value of `(node, pin)` on the
/// top of the stack.
fn compile_data(
    graph: &Graph,
    node_id: NodeId,
    pin: &str,
    visiting: &mut HashSet<NodeId>,
    ops: &mut Vec<Op>,
) -> Result<(), CompileError> {
    if !visiting.insert(node_id) {
        return Err(CompileError::Cycle { node: node_id });
    }
    let node = find_node(graph, node_id).ok_or(CompileError::MissingInput {
        node: node_id,
        pin: pin.to_string(),
    })?;
    match node.kind {
        NodeKind::ConstFloat => {
            let value = node
                .params
                .get("value")
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(0.0);
            ops.push(Op::PushFloat(value));
        }
        NodeKind::Add => {
            // Compile both inputs, then emit Add.
            for input in node.kind.input_pins() {
                match find_data_source(graph, node_id, input) {
                    Some((src_node, src_pin)) => {
                        compile_data(graph, src_node, src_pin, visiting, ops)?;
                    }
                    None => {
                        ops.push(Op::PushFloat(0.0));
                    }
                }
            }
            ops.push(Op::Add);
        }
        other => {
            visiting.remove(&node_id);
            return Err(CompileError::UnsupportedData { kind: other });
        }
    }
    visiting.remove(&node_id);
    Ok(())
}

/// Compile one sink (Print / SetPosition). Pushes its data inputs onto
/// the stack and emits the action op. `SetPosition` is parsed but not
/// emitted yet (no runtime host hook for per-entity transforms in this
/// phase); it is treated as a no-op so graphs containing it still
/// compile.
fn compile_sink(graph: &Graph, node_id: NodeId, ops: &mut Vec<Op>) -> Result<(), CompileError> {
    let node = find_node(graph, node_id).ok_or(CompileError::MissingInput {
        node: node_id,
        pin: "(sink)".to_string(),
    })?;
    let data_inputs: Vec<&'static str> = node
        .kind
        .input_pins()
        .iter()
        .filter(|p| **p != "in")
        .copied()
        .collect();
    for input in &data_inputs {
        match find_data_source(graph, node_id, input) {
            Some((src, src_pin)) => {
                let mut visiting = HashSet::new();
                compile_data(graph, src, src_pin, &mut visiting, ops)?;
            }
            None => {
                ops.push(Op::PushFloat(0.0));
            }
        }
    }
    match node.kind {
        NodeKind::Print => ops.push(Op::Print),
        NodeKind::SetPosition => {
            // Drop the three pushed floats so the stack stays balanced
            // until a per-entity runtime hook arrives.
            for _ in 0..data_inputs.len() {
                ops.push(Op::Print); // poor man's pop with side-effect
            }
        }
        other => {
            return Err(CompileError::UnsupportedData { kind: other });
        }
    }
    Ok(())
}

/// Compile the given trigger's program. Walks `exec` edges from every
/// matching entry node, dispatches each downstream sink, and returns
/// the concatenated ops. Returns an empty program if no entries match.
pub fn compile(graph: &Graph, trigger: Trigger) -> Result<Program, CompileError> {
    let mut ops = Vec::new();
    let entries: Vec<NodeId> = graph
        .nodes
        .iter()
        .filter(|n| trigger.matches(n.kind))
        .map(|n| n.id)
        .collect();
    for entry in entries {
        // Sinks fed by this entry's exec output.
        let mut visited_sinks = HashSet::new();
        let mut frontier: Vec<NodeId> = graph
            .edges
            .iter()
            .filter(|e| e.from_node == entry && e.from_pin == "exec" && e.to_pin == "in")
            .map(|e| e.to_node)
            .collect();
        while let Some(sink_id) = frontier.pop() {
            if !visited_sinks.insert(sink_id) {
                continue;
            }
            compile_sink(graph, sink_id, &mut ops)?;
            // Chained exec edges: a sink could carry its own exec
            // output in the future. For Phase 13C only OnStart / OnTick
            // produce exec; nothing chains.
            for edge in &graph.edges {
                if edge.from_node == sink_id && edge.from_pin == "exec" && edge.to_pin == "in" {
                    frontier.push(edge.to_node);
                }
            }
        }
    }
    Ok(Program { ops })
}

/// Host hook for the `Print` op. Lets the editor route VM output to its
/// log panel without coupling the VM to tracing.
pub trait Host {
    fn print(&mut self, value: f32);
}

/// Default host: forward to `tracing::info!`. Useful for headless
/// tests and the standalone runtime.
#[derive(Debug, Default)]
pub struct TracingHost;

impl Host for TracingHost {
    fn print(&mut self, value: f32) {
        tracing::info!(value, "blueprint print");
    }
}

/// Run a compiled program against the given host. Stack underflow
/// pushes 0.0 rather than aborting so a partially-wired graph still
/// produces stable output.
pub fn run(program: &Program, host: &mut dyn Host) {
    let mut stack: Vec<f32> = Vec::new();
    for op in &program.ops {
        match op {
            Op::PushFloat(v) => stack.push(*v),
            Op::Add => {
                let b = stack.pop().unwrap_or(0.0);
                let a = stack.pop().unwrap_or(0.0);
                stack.push(a + b);
            }
            Op::Print => {
                let v = stack.pop().unwrap_or(0.0);
                host.print(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    struct Capture(Vec<f32>);
    impl Host for Capture {
        fn print(&mut self, value: f32) {
            self.0.push(value);
        }
    }

    #[test]
    fn const_print() {
        let mut g = Graph::default();
        let tick = g.add_node(NodeKind::OnTick, [0.0, 0.0]);
        let c = g.add_node(NodeKind::ConstFloat, [0.0, 0.0]);
        g.nodes
            .iter_mut()
            .find(|n| n.id == c)
            .unwrap()
            .params
            .insert("value".into(), "42".into());
        let p = g.add_node(NodeKind::Print, [0.0, 0.0]);
        assert!(g.connect(tick, "exec", p, "in"));
        assert!(g.connect(c, "value", p, "value"));
        let prog = compile(&g, Trigger::OnTick).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![42.0]);
    }

    #[test]
    fn add_print() {
        let mut g = Graph::default();
        let tick = g.add_node(NodeKind::OnTick, [0.0, 0.0]);
        let ca = g.add_node(NodeKind::ConstFloat, [0.0, 0.0]);
        let cb = g.add_node(NodeKind::ConstFloat, [0.0, 0.0]);
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
        let add = g.add_node(NodeKind::Add, [0.0, 0.0]);
        let p = g.add_node(NodeKind::Print, [0.0, 0.0]);
        assert!(g.connect(tick, "exec", p, "in"));
        assert!(g.connect(ca, "value", add, "a"));
        assert!(g.connect(cb, "value", add, "b"));
        assert!(g.connect(add, "sum", p, "value"));
        let prog = compile(&g, Trigger::OnTick).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![7.0]);
    }

    #[test]
    fn empty_input_zero() {
        let mut g = Graph::default();
        let tick = g.add_node(NodeKind::OnTick, [0.0, 0.0]);
        let p = g.add_node(NodeKind::Print, [0.0, 0.0]);
        assert!(g.connect(tick, "exec", p, "in"));
        let prog = compile(&g, Trigger::OnTick).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![0.0]);
    }
}
