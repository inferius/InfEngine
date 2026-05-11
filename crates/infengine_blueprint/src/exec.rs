//! Bytecode compiler + VM for blueprint graphs.
//!
//! Phase 13C compiles a `Graph` into a flat `Program` of `Op` values
//! and runs them on a tiny stack machine. Two entry points are
//! supported: `OnStart` fires once when Play begins, `OnTick` fires
//! every frame. The compiler walks execution edges (`exec` -> `in`)
//! starting from each entry node and, for every sink encountered,
//! recursively compiles its data inputs depth-first.
//!
//! Supported builtin node kinds:
//! - `on_start`, `on_tick`  - exec source
//! - `const_float`          - push parameter "value"
//! - `add`                  - pop 2 floats, push sum
//! - `print`                - pop one float, forward to host
//! - `set_position`         - pop 3 floats (stubbed - no host hook yet)

use std::collections::HashSet;

use crate::{Graph, Node, NodeId};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    PushFloat(f32),
    Add,
    Print,
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
    match node.kind.as_str() {
        "const_float" => {
            let value = node
                .params
                .get("value")
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(0.0);
            ops.push(Op::PushFloat(value));
        }
        "add" => {
            for input in ["a", "b"] {
                match find_data_source(graph, node_id, input) {
                    Some((src_node, src_pin)) => {
                        compile_data(graph, src_node, src_pin, visiting, ops)?;
                    }
                    None => ops.push(Op::PushFloat(0.0)),
                }
            }
            ops.push(Op::Add);
        }
        other => {
            visiting.remove(&node_id);
            return Err(CompileError::UnsupportedData {
                kind: other.to_string(),
            });
        }
    }
    visiting.remove(&node_id);
    Ok(())
}

fn compile_sink(graph: &Graph, node_id: NodeId, ops: &mut Vec<Op>) -> Result<(), CompileError> {
    let node = find_node(graph, node_id).ok_or(CompileError::MissingInput {
        node: node_id,
        pin: "(sink)".to_string(),
    })?;
    match node.kind.as_str() {
        "print" => {
            match find_data_source(graph, node_id, "value") {
                Some((src, src_pin)) => {
                    let mut visiting = HashSet::new();
                    compile_data(graph, src, src_pin, &mut visiting, ops)?;
                }
                None => ops.push(Op::PushFloat(0.0)),
            }
            ops.push(Op::Print);
        }
        "set_position" => {
            // Stub: evaluate x/y/z to keep the stack balanced and to
            // surface errors early, then discard the values via Print
            // until a host hook arrives.
            for input in ["x", "y", "z"] {
                match find_data_source(graph, node_id, input) {
                    Some((src, src_pin)) => {
                        let mut visiting = HashSet::new();
                        compile_data(graph, src, src_pin, &mut visiting, ops)?;
                    }
                    None => ops.push(Op::PushFloat(0.0)),
                }
                ops.push(Op::Print);
            }
        }
        other => {
            return Err(CompileError::UnsupportedData {
                kind: other.to_string(),
            });
        }
    }
    Ok(())
}

pub fn compile(graph: &Graph, trigger: Trigger) -> Result<Program, CompileError> {
    let mut ops = Vec::new();
    let entries: Vec<NodeId> = graph
        .nodes
        .iter()
        .filter(|n| trigger.matches(&n.kind))
        .map(|n| n.id)
        .collect();
    for entry in entries {
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
            for edge in &graph.edges {
                if edge.from_node == sink_id && edge.from_pin == "exec" && edge.to_pin == "in" {
                    frontier.push(edge.to_node);
                }
            }
        }
    }
    Ok(Program { ops })
}

pub trait Host {
    fn print(&mut self, value: f32);
}

#[derive(Debug, Default)]
pub struct TracingHost;

impl Host for TracingHost {
    fn print(&mut self, value: f32) {
        tracing::info!(value, "blueprint print");
    }
}

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
    use crate::NodeKindRegistry;

    struct Capture(Vec<f32>);
    impl Host for Capture {
        fn print(&mut self, value: f32) {
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
        let prog = compile(&g, Trigger::OnTick).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![42.0]);
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
        let prog = compile(&g, Trigger::OnTick).unwrap();
        let mut cap = Capture(Vec::new());
        run(&prog, &mut cap);
        assert_eq!(cap.0, vec![7.0]);
    }
}
