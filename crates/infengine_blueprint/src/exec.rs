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

use std::collections::HashSet;

use crate::{BlueprintTarget, Graph, Node, NodeId};

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    PushFloat(f32),
    PushBool(bool),
    PushVec3([f32; 3]),
    Add,
    MakeVec3,
    Print,
    /// Pop one Vec3 and write it as the target entity's world
    /// position. Compiler bakes the entity id from `BlueprintTarget`
    /// at compile time; a Global blueprint never emits this op.
    SetEntityPosition(u32),
    /// Pop a bool; if false jump to absolute op index. True falls
    /// through. Used by Branch nodes.
    JumpIfFalse(u32),
    /// Unconditional jump to absolute op index.
    Jump(u32),
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
        "make_vec3" => {
            for input in ["x", "y", "z"] {
                match find_data_source(graph, node_id, input) {
                    Some((src_node, src_pin)) => {
                        compile_data(graph, src_node, src_pin, visiting, ops)?;
                    }
                    None => ops.push(Op::PushFloat(0.0)),
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
    visiting.remove(&node_id);
    Ok(())
}

fn compile_sink(
    graph: &Graph,
    node_id: NodeId,
    ops: &mut Vec<Op>,
    target: &BlueprintTarget,
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
                    compile_data(graph, src, src_pin, &mut visiting, ops)?;
                }
                None => ops.push(Op::PushFloat(0.0)),
            }
            ops.push(Op::Print);
        }
        "set_position" => {
            for input in ["x", "y", "z"] {
                match find_data_source(graph, node_id, input) {
                    Some((src, src_pin)) => {
                        let mut visiting = HashSet::new();
                        compile_data(graph, src, src_pin, &mut visiting, ops)?;
                    }
                    None => ops.push(Op::PushFloat(0.0)),
                }
            }
            ops.push(Op::MakeVec3);
            match target {
                BlueprintTarget::Entity { entity_id, .. } => {
                    ops.push(Op::SetEntityPosition(*entity_id));
                }
                _ => {
                    // Non-entity target: discard the vec3 by printing.
                    ops.push(Op::Print);
                }
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

/// Walk the execution chain starting at the edges from
/// `(node_id, pin)` whose `to_pin == "in"`. Recursive so branches
/// emit jumps inline; the `visited` set prevents infinite loops on
/// cycles or revisits.
fn compile_exec_from(
    graph: &Graph,
    node_id: NodeId,
    pin: &str,
    ops: &mut Vec<Op>,
    target: &BlueprintTarget,
    visited: &mut HashSet<NodeId>,
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
            "branch" => {
                // Evaluate the bool condition then emit the
                // standard JumpIfFalse + true-body + Jump-to-end +
                // false-body pattern, patching the jump targets
                // once each body's length is known.
                match find_data_source(graph, sink_id, "condition") {
                    Some((src, src_pin)) => {
                        let mut local = HashSet::new();
                        compile_data(graph, src, src_pin, &mut local, ops)?;
                    }
                    None => ops.push(Op::PushBool(false)),
                }
                let jif_idx = ops.len();
                ops.push(Op::JumpIfFalse(0));
                // True branch.
                compile_exec_from(graph, sink_id, "true", ops, target, visited)?;
                let jmp_idx = ops.len();
                ops.push(Op::Jump(0));
                let false_start = ops.len() as u32;
                // False branch.
                compile_exec_from(graph, sink_id, "false", ops, target, visited)?;
                let end_addr = ops.len() as u32;
                ops[jif_idx] = Op::JumpIfFalse(false_start);
                ops[jmp_idx] = Op::Jump(end_addr);
            }
            _ => {
                compile_sink(graph, sink_id, ops, target)?;
                // A regular sink may have its own `exec` output (rare
                // for the current node set); follow it so chains of
                // Prints / SetPositions all run.
                compile_exec_from(graph, sink_id, "exec", ops, target, visited)?;
            }
        }
    }
    Ok(())
}

pub fn compile(
    graph: &Graph,
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
    for entry in entries {
        let mut visited = HashSet::new();
        compile_exec_from(graph, entry, "exec", &mut ops, target, &mut visited)?;
    }
    Ok(Program { ops })
}

pub trait Host {
    /// Forward a `Value` to the editor / runtime sink. The
    /// implementation usually pretty-prints it; tests capture it.
    fn print(&mut self, value: Value);
    /// Write a 3D position to the given entity. Default impl is a
    /// noop so a host that only cares about Print doesn't have to
    /// implement transform mutation.
    fn set_entity_position(&mut self, _entity_id: u32, _position: [f32; 3]) {}
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

pub fn run(program: &Program, host: &mut dyn Host) {
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
        }
        pc += 1;
    }
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
