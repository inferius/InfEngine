#![allow(clippy::print_stdout)]
//! Generates the InfEngine showcase project on disk.
//!
//! Outputs (relative to repo root):
//!
//! ```text
//! examples/showcase/project/
//! |- project.ron                  Project manifest (scene list, classes, libraries).
//! |- scenes/arena.ron             3D arena scene with floor, player, pickups, lights.
//! |- blueprints/lib_math.ron      Library blueprint: `double` helper function.
//! |- blueprints/scene_arena.ron   Global scene blueprint; prints on Play start.
//! |- blueprints/pickup_spin.ron   Class blueprint shared by every pickup; spins them.
//! |- blueprints/player_bp.ron     Entity blueprint on the player cube; calls C# tick.
//! `- blueprints/light_pulse.ron   Light pulses intensity over time (variable + math).
//! ```
//!
//! Run once:
//!
//! ```sh
//! cargo run -p showcase --bin showcase_setup
//! ```
//!
//! then open the project in the editor:
//!
//! ```sh
//! cargo run -p editor_app -- examples/showcase/project/project.ron
//! ```
//!
//! Hit Play. The pickups rotate via blueprint variables, the global
//! blueprint logs at start, the C# `Engine.Log` fires via Call C#
//! Method, the point lights light the arena. Everything covered by
//! the engine's MVP feature set in one project.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use infengine_blueprint::{
    Blueprint, BlueprintTarget, BlueprintVariable, Graph, NamedGraph, NodeKindRegistry, PinType,
};
use infengine_core::project::{Class, Project, ProjectScene};
use infengine_core::scene::{
    AudioComponent, Entity, EntityId, LightComponent, MeshComponent, Physics3DComponent, Scene,
    Transform3D,
};

fn main() -> Result<()> {
    let root = repo_root()?;
    let proj_dir = root.join("examples/showcase/project");
    let scenes_dir = proj_dir.join("scenes");
    let bp_dir = proj_dir.join("blueprints");
    std::fs::create_dir_all(&scenes_dir).context("create scenes dir")?;
    std::fs::create_dir_all(&bp_dir).context("create blueprints dir")?;

    let registry = NodeKindRegistry::with_builtins();

    // --- Project manifest -------------------------------------------------
    let project = Project {
        name: "Arena Showcase".into(),
        asset_root: PathBuf::from("assets"),
        scenes: vec![ProjectScene {
            name: "Arena".into(),
            path: PathBuf::from("scenes/arena.ron"),
        }],
        classes: vec![Class {
            name: "Pickup".into(),
            blueprint: Some("blueprints/pickup_spin.ron".into()),
            default_components: vec![],
        }],
        blueprint_libraries: vec!["blueprints/lib_math.ron".into()],
        network: Default::default(),
    };
    project
        .save(proj_dir.join("project.ron"))
        .context("save project")?;

    // --- Scene ------------------------------------------------------------
    let scene = build_scene();
    scene
        .save(scenes_dir.join("arena.ron"))
        .context("save scene")?;

    // --- Blueprints -------------------------------------------------------
    build_lib_math().save(bp_dir.join("lib_math.ron"))?;
    build_scene_arena().save(bp_dir.join("scene_arena.ron"))?;
    build_pickup_spin(&registry).save(bp_dir.join("pickup_spin.ron"))?;
    build_player_bp(&registry).save(bp_dir.join("player_bp.ron"))?;
    build_light_pulse(&registry).save(bp_dir.join("light_pulse.ron"))?;

    println!("Showcase project written to {}", proj_dir.display());
    println!("Open with: cargo run -p editor_app -- {}/project.ron", proj_dir.display());
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    // The bin is invoked from the repo root via `cargo run -p showcase`.
    // CARGO_MANIFEST_DIR points at examples/showcase, so go up two.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest
        .parent()
        .and_then(Path::parent)
        .context("derive repo root from CARGO_MANIFEST_DIR")?
        .to_path_buf())
}

fn build_scene() -> Scene {
    let mut s = Scene {
        name: "Arena".into(),
        entities: Vec::new(),
        next_id: 1,
        clear_color: [0.04, 0.05, 0.08],
        gravity: [0.0, -1200.0],
    };

    // Floor.
    let floor = make_3d_entity(
        &mut s,
        "Floor",
        [0.0, -0.5, 0.0],
        [0.0; 3],
        [20.0, 0.5, 20.0],
        [0.35, 0.35, 0.4, 1.0],
        Some(Physics3DComponent::static_box()),
    );
    s.entities.push(floor);

    // Player. Entity blueprint runs every frame (`player_bp.ron`).
    let mut player = make_3d_entity(
        &mut s,
        "Player",
        [0.0, 1.0, 0.0],
        [0.0; 3],
        [1.0, 1.0, 1.0],
        [0.85, 0.20, 0.20, 1.0],
        Some(Physics3DComponent::dynamic_box()),
    );
    player.blueprints.push("blueprints/player_bp.ron".into());
    player.blueprint = Some("blueprints/player_bp.ron".into());
    s.entities.push(player);

    // Four pickups at corners. All share the "Pickup" class so the
    // shared class blueprint (`pickup_spin.ron`) drives their rotation.
    let pickup_positions = [
        ("Pickup_NE", [4.0, 0.6, 4.0]),
        ("Pickup_NW", [-4.0, 0.6, 4.0]),
        ("Pickup_SE", [4.0, 0.6, -4.0]),
        ("Pickup_SW", [-4.0, 0.6, -4.0]),
    ];
    for (name, pos) in pickup_positions {
        let mut pickup = make_3d_entity(
            &mut s,
            name,
            pos,
            [0.0; 3],
            [0.6, 0.6, 0.6],
            [0.20, 0.85, 0.30, 1.0],
            None,
        );
        pickup.class = Some("Pickup".into());
        pickup
            .blueprints
            .push("blueprints/pickup_spin.ron".into());
        pickup.blueprint = Some("blueprints/pickup_spin.ron".into());
        s.entities.push(pickup);
    }

    // Two point lights flanking the arena, one pulsing via blueprint.
    let mut light_a = make_3d_entity(
        &mut s,
        "Light_Warm",
        [3.0, 3.5, 3.0],
        [0.0; 3],
        [0.2; 3],
        [1.0, 0.65, 0.30, 1.0],
        None,
    );
    light_a.light = Some(LightComponent::point([1.0, 0.65, 0.30], 12.0, 14.0));
    light_a
        .blueprints
        .push("blueprints/light_pulse.ron".into());
    light_a.blueprint = Some("blueprints/light_pulse.ron".into());
    s.entities.push(light_a);

    let mut light_b = make_3d_entity(
        &mut s,
        "Light_Cool",
        [-3.0, 3.5, -3.0],
        [0.0; 3],
        [0.2; 3],
        [0.35, 0.60, 1.0, 1.0],
        None,
    );
    light_b.light = Some(LightComponent::point([0.35, 0.60, 1.0], 10.0, 14.0));
    s.entities.push(light_b);

    // Ambient audio on the player; auto-plays at Play start.
    // Path points at a placeholder file the user can drop in later;
    // missing file logs a warning instead of crashing.
    if let Some(player) = s.entities.iter_mut().find(|e| e.name == "Player") {
        player.audio = Some(AudioComponent {
            sound_path: "assets/audio/ambient.ogg".into(),
            volume: 0.35,
            autoplay: true,
            looping: true,
        });
    }

    s
}

fn make_3d_entity(
    scene: &mut Scene,
    name: &str,
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    color: [f32; 4],
    physics: Option<Physics3DComponent>,
) -> Entity {
    let id = EntityId(scene.next_id);
    scene.next_id += 1;
    let mut e = Entity::new(id, name);
    e.transform_3d = Some(Transform3D {
        position,
        rotation,
        scale,
    });
    e.mesh = Some(MeshComponent {
        mesh: "cube".into(),
        color,
        metallic: 0.0,
        roughness: 0.6,
        texture: None,
    });
    e.physics_3d = physics;
    e
}

// ---------------------------- Blueprints --------------------------------

/// Library blueprint exposing one helper function, `double`.
/// Demonstrates `Project.blueprint_libraries` + cross-blueprint calls.
fn build_lib_math() -> Blueprint {
    let mut bp = Blueprint {
        name: "lib_math".into(),
        target: BlueprintTarget::Global,
        graph: Graph::default(),
        variables: vec![BlueprintVariable {
            name: "Input".into(),
            ty: PinType::Float,
            default: "0".into(),
        }],
        functions: Vec::new(),
        macros: Vec::new(),
    };

    // Function `double`: prints Input * 2.
    let reg = NodeKindRegistry::with_builtins();
    let mut g = Graph::default();
    let entry = g.add_node("on_start", [40.0, 40.0]);
    let get = g.add_node("get_float_var", [40.0, 140.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == get)
        .unwrap()
        .params
        .insert("name".into(), "Input".into());
    let mul = g.add_node("multiply", [240.0, 140.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == mul)
        .unwrap()
        .params
        .insert("pin_b".into(), "2".into());
    let print = g.add_node("print", [440.0, 40.0]);
    let _ = g.connect(&reg, entry, "exec", print, "in");
    let _ = g.connect(&reg, get, "value", mul, "a");
    let _ = g.connect(&reg, mul, "product", print, "value");
    bp.functions.push(NamedGraph {
        name: "double".into(),
        graph: g,
    });

    bp
}

/// Global scene blueprint. Prints once at Play start and announces
/// every tick's elapsed time at 1 Hz via comparison + branch.
fn build_scene_arena() -> Blueprint {
    let mut bp = Blueprint {
        name: "Scene Arena".into(),
        target: BlueprintTarget::Global,
        graph: Graph::default(),
        variables: vec![BlueprintVariable {
            name: "LastBeat".into(),
            ty: PinType::Float,
            default: "0".into(),
        }],
        functions: Vec::new(),
        macros: Vec::new(),
    };
    let reg = NodeKindRegistry::with_builtins();
    let g = &mut bp.graph;
    let on_start = g.add_node("on_start", [40.0, 40.0]);
    // Sequence the OnStart so we can both reset the C#-side score
    // counter AND emit the sentinel "started" print.
    let start_seq = g.add_node("sequence", [240.0, 40.0]);
    let reset_cs = g.add_node("call_cs_method", [440.0, 40.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == reset_cs)
        .unwrap()
        .params
        .insert("method".into(), "Showcase.ResetScore".into());
    let start_print = g.add_node("print", [440.0, 120.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == start_print)
        .unwrap()
        .params
        .insert("pin_value".into(), "1".into()); // Float marker = "1.0 = started"
    let _ = g.connect(&reg, on_start, "exec", start_seq, "in");
    let _ = g.connect(&reg, start_seq, "then_0", reset_cs, "in");
    let _ = g.connect(&reg, start_seq, "then_1", start_print, "in");

    let on_tick = g.add_node("on_tick", [40.0, 200.0]);
    let time = g.add_node("time_seconds", [40.0, 300.0]);
    let cmp = g.add_node("greater_than", [240.0, 300.0]);
    let last = g.add_node("get_float_var", [40.0, 400.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == last)
        .unwrap()
        .params
        .insert("name".into(), "LastBeat".into());
    let add = g.add_node("add", [240.0, 400.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == add)
        .unwrap()
        .params
        .insert("pin_b".into(), "1".into()); // beat every +1s
    let branch = g.add_node("branch", [460.0, 240.0]);
    let beat_print = g.add_node("print", [680.0, 200.0]);
    let store = g.add_node("set_float_var", [680.0, 320.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == store)
        .unwrap()
        .params
        .insert("name".into(), "LastBeat".into());
    let _ = g.connect(&reg, on_tick, "exec", branch, "in");
    let _ = g.connect(&reg, time, "value", cmp, "a");
    let _ = g.connect(&reg, last, "value", add, "a");
    let _ = g.connect(&reg, add, "sum", cmp, "b");
    let _ = g.connect(&reg, cmp, "value", branch, "condition");
    let _ = g.connect(&reg, branch, "true", beat_print, "in");
    let _ = g.connect(&reg, branch, "true", store, "in");
    let _ = g.connect(&reg, time, "value", beat_print, "value");
    let _ = g.connect(&reg, time, "value", store, "value");
    bp
}

/// Pickup spin blueprint - class-level. Increments a float variable
/// every tick by `dt * 2` and writes it to the entity's Y rotation.
fn build_pickup_spin(reg: &NodeKindRegistry) -> Blueprint {
    let mut bp = Blueprint {
        name: "Pickup Spin".into(),
        target: BlueprintTarget::Class("Pickup".into()),
        graph: Graph::default(),
        variables: vec![BlueprintVariable {
            name: "Angle".into(),
            ty: PinType::Float,
            default: "0".into(),
        }],
        functions: Vec::new(),
        macros: Vec::new(),
    };
    let g = &mut bp.graph;
    let tick = g.add_node("on_tick", [40.0, 40.0]);
    // Sequence so we can fire both set_float_var AND set_rotation from
    // a single OnTick exec, since set_float_var has no exec output.
    let seq = g.add_node("sequence", [240.0, 40.0]);
    let get = g.add_node("get_float_var", [40.0, 160.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == get)
        .unwrap()
        .params
        .insert("name".into(), "Angle".into());
    let dt = g.add_node("delta_time", [40.0, 260.0]);
    let mul = g.add_node("multiply", [240.0, 260.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == mul)
        .unwrap()
        .params
        .insert("pin_b".into(), "2".into());
    let add = g.add_node("add", [440.0, 160.0]);
    let set_var = g.add_node("set_float_var", [640.0, 40.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == set_var)
        .unwrap()
        .params
        .insert("name".into(), "Angle".into());
    let set_rot = g.add_node("set_rotation", [860.0, 40.0]);
    let _ = g.connect(reg, tick, "exec", seq, "in");
    let _ = g.connect(reg, seq, "then_0", set_var, "in");
    let _ = g.connect(reg, seq, "then_1", set_rot, "in");
    let _ = g.connect(reg, dt, "value", mul, "a");
    let _ = g.connect(reg, get, "value", add, "a");
    let _ = g.connect(reg, mul, "product", add, "b");
    let _ = g.connect(reg, add, "sum", set_var, "value");
    let _ = g.connect(reg, add, "sum", set_rot, "y");
    bp
}

/// Player blueprint - logs once on start, calls a registered C#
/// method every tick to demonstrate the Scripts category.
fn build_player_bp(reg: &NodeKindRegistry) -> Blueprint {
    let mut bp = Blueprint {
        name: "Player".into(),
        target: BlueprintTarget::Entity {
            entity_id: 2, // Player id assigned in build_scene
            entity_name: "Player".into(),
        },
        graph: Graph::default(),
        variables: Vec::new(),
        functions: Vec::new(),
        macros: Vec::new(),
    };
    let g = &mut bp.graph;
    let on_start = g.add_node("on_start", [40.0, 40.0]);
    let hello = g.add_node("print", [240.0, 40.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == hello)
        .unwrap()
        .params
        .insert("pin_value".into(), "777".into()); // sentinel value in the log
    let _ = g.connect(reg, on_start, "exec", hello, "in");

    let on_tick = g.add_node("on_tick", [40.0, 200.0]);
    // Bridge into the Game assembly's Showcase.OnPickupCollected via
    // the [BlueprintCallable] surface defined in
    // scripts/InfEngine.Scripts.Game/Showcase.cs. v1 logs through
    // the stub host (see docs/design/blueprint_csharp_callable.md);
    // the same node graph picks up the real reverse-call once the
    // CoreCLR manifest pipeline lands.
    let cs = g.add_node("call_cs_method", [240.0, 200.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == cs)
        .unwrap()
        .params
        .insert("method".into(), "Showcase.OnPickupCollected".into());
    let _ = g.connect(reg, on_tick, "exec", cs, "in");
    bp
}

/// Light pulse blueprint. Sets the light entity's Y position to
/// `3.5 + sin-ish( time )` approximated via `(time*2)`-mod arithmetic
/// using only the math nodes available in v1 (no sine yet). Result is
/// a triangle-wave bob.
fn build_light_pulse(reg: &NodeKindRegistry) -> Blueprint {
    let mut bp = Blueprint {
        name: "Light Pulse".into(),
        target: BlueprintTarget::Entity {
            entity_id: 7, // Light_Warm
            entity_name: "Light_Warm".into(),
        },
        graph: Graph::default(),
        variables: vec![BlueprintVariable {
            name: "Phase".into(),
            ty: PinType::Float,
            default: "0".into(),
        }],
        functions: Vec::new(),
        macros: Vec::new(),
    };
    let g = &mut bp.graph;
    let tick = g.add_node("on_tick", [40.0, 40.0]);
    let phase = g.add_node("get_float_var", [40.0, 160.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == phase)
        .unwrap()
        .params
        .insert("name".into(), "Phase".into());
    let dt = g.add_node("delta_time", [40.0, 260.0]);
    let advance = g.add_node("add", [240.0, 160.0]);
    let store = g.add_node("set_float_var", [440.0, 40.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == store)
        .unwrap()
        .params
        .insert("name".into(), "Phase".into());
    let set_pos = g.add_node("set_position", [660.0, 40.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == set_pos)
        .unwrap()
        .params
        .insert("pin_x".into(), "3".into());
    g.nodes
        .iter_mut()
        .find(|n| n.id == set_pos)
        .unwrap()
        .params
        .insert("pin_z".into(), "3".into());
    // Same sequence trick: set_float_var has no exec output, so use a
    // sequence to fire both the variable write and the set_position.
    let seq = g.add_node("sequence", [240.0, 40.0]);
    let _ = g.connect(reg, tick, "exec", seq, "in");
    let _ = g.connect(reg, seq, "then_0", store, "in");
    let _ = g.connect(reg, seq, "then_1", set_pos, "in");
    let _ = g.connect(reg, phase, "value", advance, "a");
    let _ = g.connect(reg, dt, "value", advance, "b");
    let _ = g.connect(reg, advance, "sum", store, "value");
    // Y position oscillates between 3.0 and 4.0 by feeding (phase + 3)
    // straight in; for a proper sine we'd need a builtin sin node.
    let bias = g.add_node("add", [440.0, 200.0]);
    g.nodes
        .iter_mut()
        .find(|n| n.id == bias)
        .unwrap()
        .params
        .insert("pin_b".into(), "3".into());
    let _ = g.connect(reg, advance, "sum", bias, "a");
    let _ = g.connect(reg, bias, "sum", set_pos, "y");
    bp
}
