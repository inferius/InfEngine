//! Standalone editor binary for InfEngine projects.
//!
//! Usage:
//!
//! ```sh
//! editor_app                  # opens ./project.ron
//! editor_app path/to/project.ron
//! ```
//!
//! Phase 9.1 ships project loading, the scene picker, and the
//! per-entity inspector. Multi-window editing, gizmos, undo/redo, and
//! a Play mode arrive in later sub-phases.

use std::path::{Path, PathBuf};

use glam::{Mat4, Quat, Vec3, Vec4};
use infengine_assets::AssetServer;
use infengine_blueprint::{
    Blueprint, BlueprintTarget, BlueprintWorkspace, NodeKindRegistry,
    exec::{Program, TracingHost, Trigger, run as run_program},
};
use infengine_core::project::{Project, ProjectError, ProjectScene};
use std::collections::HashMap;

use infengine_audio::AudioSystem;
use infengine_core::scene::{
    AtlasRegion, AudioComponent, BodyKind3D, ColliderShape3D, Entity, EntityId, MeshComponent,
    Physics3DComponent, Scene, SpriteComponent, Transform2D, Transform3D,
};
use infengine_editor::EditorUi;
use infengine_physics::{
    BodyDesc, BodyId as PhysicsBodyId, BodyKind3D as PhysicsBodyKind3D,
    ColliderShape3D as PhysicsColliderShape3D, PhysicsWorld, PhysicsWorld3D,
};
use infengine_render::{Camera3D, MeshHandle, Renderer, SpriteInstance};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use tracing::{error, info, warn};

const DEFAULT_PROJECT_FILE: &str = "project.ron";

const UNDO_LIMIT: usize = 64;

struct Editor {
    project_root: PathBuf,
    project_file: PathBuf,
    project: Project,
    current_scene_index: Option<usize>,
    scene: Scene,
    selected: Option<EntityId>,
    status: String,
    undo_stack: Vec<Scene>,
    redo_stack: Vec<Scene>,
    ctrl_held: bool,
    shift_held: bool,
    z_key_prev: bool,
    y_key_prev: bool,

    renderer: Option<Renderer>,
    editor_ui: Option<EditorUi>,
    assets: AssetServer,
    asset_listing: Vec<PathBuf>,
    viewport: (u32, u32),
    zoom: f32,
    fps_ema: f32,
    cursor_pos: [f32; 2],
    left_button: bool,
    left_button_prev: bool,
    dragging: Option<EntityId>,
    drag_offset: [f32; 2],
    drag_axis: DragAxis,
    middle_button: bool,
    middle_button_prev: bool,
    scroll_accum: f32,
    prev_cursor_for_camera: [f32; 2],
    /// Previously selected entity. When the selection changes the 3D
    /// camera snaps its target to the new entity; otherwise the user's
    /// orbit / pan edits are preserved.
    prev_selected: Option<EntityId>,
    f_key_prev: bool,
    /// In-progress 3D translate-gizmo drag. Stores the original entity
    /// position + cursor pixel at drag start so the drag delta is
    /// computed against a stable origin each frame.
    drag_3d: Option<Drag3DState>,
    /// Play-mode lifecycle. Editing is the authoritative state; entering
    /// Playing snapshots the scene and starts ticking physics on the
    /// live copy, Paused freezes the live copy, Stopping reverts to the
    /// snapshot.
    play_state: PlayState,
    /// Authoritative scene preserved across Play -> Stop. Populated only
    /// while `play_state != Editing`.
    play_snapshot: Option<Scene>,
    play_physics: Option<PhysicsWorld>,
    play_physics_3d: Option<PhysicsWorld3D>,
    play_body_map_3d: HashMap<EntityId, PhysicsBodyId>,
    /// Visual scripting editor. The graph is in-memory only at the
    /// scaffold stage; persistence + node-graph execution come later.
    blueprint: BlueprintWorkspace,
    blueprint_open: bool,
    /// The entity whose blueprint is currently loaded in `blueprint`.
    /// `None` = an unbound / global blueprint editor session.
    blueprint_active_entity: Option<EntityId>,
    node_registry: NodeKindRegistry,
    /// Name -> mesh handle map for glTF-imported meshes. Populated by
    /// the "Import glTF" panel; consulted when drawing 3D entities
    /// whose `MeshComponent::mesh` doesn't match a built-in name.
    imported_meshes: HashMap<String, MeshHandle>,
    gltf_path_input: String,
    /// Mesh-name -> last applied texture path. Used to skip re-loading
    /// the same texture frame after frame.
    loaded_mesh_textures: HashMap<String, String>,
    /// Compiled OnTick programs - one per entity script discovered
    /// when Play started, plus the workspace's active tab. The tick
    /// loop runs all of them every frame.
    play_blueprint_ticks: Vec<Program>,
    // Wall-clock timestamp of the previous viewport left-mouse press
    // plus the screen-space pixel position at that time. Used to
    // spot double-clicks in the 3D viewport, which winit's raw mouse
    // events don't surface on their own.
    last_left_click_time: Option<std::time::Instant>,
    last_left_click_pos: [f32; 2],
    /// Audio backend; lazily initialised on first sound request.
    /// `None` when the host has no audio device.
    audio_system: Option<AudioSystem>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PlayState {
    Editing,
    Playing,
    Paused,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DragAxis {
    Free,
    X,
    Y,
}

#[derive(Debug, Copy, Clone)]
struct Drag3DState {
    entity: EntityId,
    axis: Axis3,
    start_pos: [f32; 3],
    start_cursor: [f32; 2],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Axis3 {
    X,
    Y,
    Z,
}

impl Axis3 {
    fn unit(self) -> Vec3 {
        match self {
            Axis3::X => Vec3::X,
            Axis3::Y => Vec3::Y,
            Axis3::Z => Vec3::Z,
        }
    }
}

const GIZMO_AXIS_LEN: f32 = 80.0;
const GIZMO_HANDLE_HALF: f32 = 8.0;
const GIZMO_PICK_THICKNESS: f32 = 14.0;

/// World-space length of one 3D gizmo arm. Drawn at the selected
/// entity's position pointing along +X/+Y/+Z.
const GIZMO_3D_AXIS_LEN: f32 = 1.5;
/// Screen-pixel half-thickness used when hit-testing a 3D gizmo arm.
const GIZMO_3D_PICK_PIXELS: f32 = 10.0;

impl Editor {
    fn new(project_root: PathBuf, project_file: PathBuf, project: Project) -> Self {
        let asset_root = project.asset_root_full(&project_root);
        let assets = AssetServer::new(asset_root);
        let mut me = Self {
            project_root,
            project_file,
            project,
            current_scene_index: None,
            scene: Scene::default(),
            selected: None,
            status: String::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            ctrl_held: false,
            shift_held: false,
            z_key_prev: false,
            y_key_prev: false,
            renderer: None,
            editor_ui: None,
            assets,
            asset_listing: Vec::new(),
            viewport: (1, 1),
            zoom: 1.0,
            fps_ema: 60.0,
            cursor_pos: [0.0, 0.0],
            left_button: false,
            left_button_prev: false,
            dragging: None,
            drag_offset: [0.0, 0.0],
            drag_axis: DragAxis::Free,
            middle_button: false,
            middle_button_prev: false,
            scroll_accum: 0.0,
            prev_cursor_for_camera: [0.0, 0.0],
            prev_selected: None,
            f_key_prev: false,
            drag_3d: None,
            play_state: PlayState::Editing,
            play_snapshot: None,
            play_physics: None,
            play_physics_3d: None,
            play_body_map_3d: HashMap::new(),
            blueprint: BlueprintWorkspace::new(),
            blueprint_open: false,
            blueprint_active_entity: None,
            node_registry: NodeKindRegistry::with_builtins(),
            imported_meshes: HashMap::new(),
            gltf_path_input: String::new(),
            loaded_mesh_textures: HashMap::new(),
            play_blueprint_ticks: Vec::new(),
            audio_system: None,
            last_left_click_time: None,
            last_left_click_pos: [0.0, 0.0],
        };
        if me.project.scenes.is_empty() {
            // A fresh project ships with an empty scenes list and
            // would otherwise present a blank viewport with no hint
            // about what to do next. Seed a starter scene so the user
            // sees the demo cube + sprite the moment the window opens.
            me.add_scene();
            if let Some(idx) = me.current_scene_index {
                let s = &mut me.scene;
                let i = s.add_entity("cube_demo");
                s.entities[i].transform_3d = Some(Transform3D::default());
                s.entities[i].mesh = Some(MeshComponent::cube([0.85, 0.65, 0.4, 1.0]));
                me.selected = Some(me.scene.entities[i].id);
                me.save_current();
                me.status = format!("Created starter scene #{}", idx + 1);
            }
        } else {
            me.open_scene(0);
        }
        me.refresh_assets();
        me
    }

    fn refresh_assets(&mut self) {
        match self.assets.list_files() {
            Ok(list) => self.asset_listing = list,
            Err(err) => warn!(error = %err, "asset listing failed"),
        }
    }

    fn open_scene(&mut self, index: usize) {
        let Some(entry) = self.project.scenes.get(index).cloned() else {
            return;
        };
        let full = self.project.scene_full_path(&self.project_root, &entry);
        match Scene::load(&full) {
            Ok(s) => {
                self.scene = s;
                self.selected = self
                    .scene
                    .entities
                    .iter()
                    .find(|e| e.sprite.is_some())
                    .map(|e| e.id);
                self.current_scene_index = Some(index);
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.status = format!("Loaded {}", entry.name);
                info!(scene = entry.name, "scene opened");
            }
            Err(err) => {
                warn!(error = %err, path = %full.display(), "scene load failed");
                self.status = format!("Load failed: {err}");
            }
        }
    }

    fn save_current(&mut self) {
        let Some(idx) = self.current_scene_index else {
            self.status = "No scene selected".into();
            return;
        };
        let entry = self.project.scenes[idx].clone();
        let full = self.project.scene_full_path(&self.project_root, &entry);
        match self.scene.save(&full) {
            Ok(()) => self.status = format!("Saved {}", entry.name),
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn save_project(&mut self) {
        match self.project.save(&self.project_file) {
            Ok(()) => self.status = format!("Project saved -> {}", self.project_file.display()),
            Err(err) => self.status = format!("Project save failed: {err}"),
        }
    }

    /// Copy the project, every referenced scene file, and the asset
    /// directory into a `dist/` folder next to the project file. The
    /// resulting layout is what `infengine_runtime` expects.
    /// Copy the project manifest, every scene, the asset directory,
    /// and the per-entity blueprint files into `dist/`. The runtime
    /// binary is added by `build_standalone`; `export` only ships the
    /// data.
    fn export(&mut self) {
        let dist = self.project_root.join("dist");
        if let Err(err) = std::fs::create_dir_all(&dist) {
            self.status = format!("Export failed (create dist): {err}");
            return;
        }
        if let Err(err) = std::fs::copy(&self.project_file, dist.join("project.ron")) {
            self.status = format!("Export failed (project.ron): {err}");
            return;
        }
        for entry in &self.project.scenes {
            let src = self.project.scene_full_path(&self.project_root, entry);
            let dst = dist.join(&entry.path);
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(err) = std::fs::copy(&src, &dst) {
                self.status = format!("Export failed (scene {}): {err}", entry.name);
                return;
            }
        }
        let asset_src = self.project.asset_root_full(&self.project_root);
        if asset_src.exists() {
            let asset_dst = dist.join(&self.project.asset_root);
            if let Err(err) = copy_dir_all(&asset_src, &asset_dst) {
                self.status = format!("Export failed (assets): {err}");
                return;
            }
        }
        // Blueprints dir (per-entity .ron files written by the
        // editor). Optional - skip silently when it doesn't exist.
        let blueprints_src = self.project_root.join("blueprints");
        if blueprints_src.exists() {
            let blueprints_dst = dist.join("blueprints");
            if let Err(err) = copy_dir_all(&blueprints_src, &blueprints_dst) {
                self.status = format!("Export failed (blueprints): {err}");
                return;
            }
        }
        let readme = dist.join("README.txt");
        let _ = std::fs::write(
            &readme,
            "InfEngine runtime export.\n\n\
             Drop a built `infengine_runtime` (or `infengine_runtime.exe` on Windows)\n\
             next to this file, then run it. The runtime opens `project.ron` and\n\
             plays the first scene.\n\n\
             Use the editor's 'Build standalone' button to do this in one step,\n\
             or build the runtime with `cargo build --release -p infengine_runtime`\n\
             and copy the binary from `target/release/`.\n",
        );
        self.status = format!("Export OK -> {}", dist.display());
        info!(dist = %dist.display(), "project exported");
    }

    /// Run a release build of `infengine_runtime`, copy the resulting
    /// binary next to `dist/project.ron`, and ensure all the data is
    /// already there via `export`. The result is a self-contained
    /// folder that double-clicks into a playable build.
    fn build_standalone(&mut self) {
        self.export();
        if self.status.starts_with("Export failed") {
            return;
        }
        self.status = "Building runtime (release) ...".into();
        let output = std::process::Command::new("cargo")
            .args(["build", "--release", "-p", "infengine_runtime"])
            .current_dir(workspace_root())
            .output();
        let dist = self.project_root.join("dist");
        match output {
            Ok(out) if out.status.success() => {
                let exe = if cfg!(windows) {
                    "infengine_runtime.exe"
                } else {
                    "infengine_runtime"
                };
                let src = workspace_root().join("target").join("release").join(exe);
                let dst = dist.join(exe);
                if let Err(err) = std::fs::copy(&src, &dst) {
                    self.status = format!("Standalone build copy failed: {err}");
                    return;
                }
                self.status = format!("Standalone build OK -> {}", dst.display());
                info!(out = %dst.display(), "standalone build complete");
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let snippet = stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
                self.status = format!("cargo build failed: {snippet}");
            }
            Err(err) => {
                self.status = format!("Failed to launch cargo: {err}");
            }
        }
    }

    fn add_scene(&mut self) {
        let suffix = self.project.scenes.len() + 1;
        let name = format!("scene_{suffix}");
        let file = format!("{name}.ron");
        let scene_path = PathBuf::from(&file);
        let full = self.project_root.join(&scene_path);

        let new_scene = Scene {
            name: name.clone(),
            ..Scene::default()
        };
        if let Err(err) = new_scene.save(&full) {
            self.status = format!("Failed to write {name}: {err}");
            return;
        }
        self.project.scenes.push(ProjectScene {
            name: name.clone(),
            path: scene_path,
        });
        let idx = self.project.scenes.len() - 1;
        let _ = self.project.save(&self.project_file);
        self.open_scene(idx);
        self.status = format!("Created {name}");
    }

    fn push_snapshot(&mut self) {
        if self.undo_stack.len() >= UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.scene.clone());
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack
                .push(std::mem::replace(&mut self.scene, prev));
            self.selected = self
                .scene
                .entities
                .iter()
                .find(|e| e.sprite.is_some())
                .map(|e| e.id);
            self.status = format!("Undo ({} left)", self.undo_stack.len());
        } else {
            self.status = "Nothing to undo".into();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack
                .push(std::mem::replace(&mut self.scene, next));
            self.selected = self
                .scene
                .entities
                .iter()
                .find(|e| e.sprite.is_some())
                .map(|e| e.id);
            self.status = format!("Redo ({} left)", self.redo_stack.len());
        } else {
            self.status = "Nothing to redo".into();
        }
    }

    /// Bind the active blueprint editor session to a specific entity.
    /// If the entity already has a `blueprint` path, load it from
    /// disk; otherwise create a fresh blueprint, assign a default
    /// path under `blueprints/`, and persist it so the next session
    /// reopens the same file.
    /// Open (or create) a scene-level "global" blueprint. Lives at
    /// `blueprints/scene_<name>.ron` so each scene has its own; the
    /// target is `Global` so set_position and other entity-scoped
    /// nodes are filtered out of the Add menu. Used for one-shot
    /// scene setup (autoplay clips, initial spawns, debug toggles).
    fn open_scene_blueprint(&mut self) {
        self.save_active_blueprint();
        let scene_slug = self
            .current_scene_index
            .and_then(|i| self.project.scenes.get(i))
            .map(|s| {
                s.name
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>()
            })
            .unwrap_or_else(|| "default".into());
        let rel = format!("blueprints/scene_{scene_slug}.ron");
        let full = self.project_root.join(&rel);
        let blueprint = match Blueprint::load(&full) {
            Ok(mut b) => {
                b.target = BlueprintTarget::Global;
                b
            }
            Err(_) => Blueprint {
                name: format!("Scene: {scene_slug}"),
                target: BlueprintTarget::Global,
                graph: infengine_blueprint::Graph::default(),
                variables: Vec::new(),
                functions: Vec::new(),
                macros: Vec::new(),
            },
        };
        if let Some(parent) = full.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = blueprint.save(&full) {
            self.status = format!("Blueprint save failed: {err}");
        }
        self.blueprint.open(blueprint, Some(full));
        self.blueprint_active_entity = None;
        self.blueprint_open = true;
    }

    /// Open (or create) a project-wide global blueprint. Independent
    /// of the active scene; persists at `blueprints/global.ron` so it
    /// stays the same across scene switches.
    fn open_global_blueprint(&mut self) {
        self.save_active_blueprint();
        let rel = "blueprints/global.ron";
        let full = self.project_root.join(rel);
        let blueprint = match Blueprint::load(&full) {
            Ok(mut b) => {
                b.target = BlueprintTarget::Global;
                b
            }
            Err(_) => Blueprint {
                name: "Global".into(),
                target: BlueprintTarget::Global,
                graph: infengine_blueprint::Graph::default(),
                variables: Vec::new(),
                functions: Vec::new(),
                macros: Vec::new(),
            },
        };
        if let Some(parent) = full.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = blueprint.save(&full) {
            self.status = format!("Blueprint save failed: {err}");
        }
        self.blueprint.open(blueprint, Some(full));
        self.blueprint_active_entity = None;
        self.blueprint_open = true;
    }

    fn open_entity_blueprint(&mut self, eid: EntityId) {
        // Save whatever was being edited before swapping.
        self.save_active_blueprint();
        let (rel_path, entity_name) = {
            let Some(entity) = self.scene.get(eid) else {
                return;
            };
            // Prefer the first script slot; fall back to the legacy
            // single-blueprint field; otherwise pick a fresh default
            // path under blueprints/.
            let rel = entity
                .blueprints
                .first()
                .cloned()
                .or_else(|| entity.blueprint.clone())
                .unwrap_or_else(|| format!("blueprints/entity_{}_script_0.ron", eid.0));
            (rel, entity.name.clone())
        };
        let full = self.project_root.join(&rel_path);
        let blueprint = match Blueprint::load(&full) {
            Ok(mut b) => {
                // Refresh target so a renamed entity shows current
                // name in the blueprint header.
                b.target = BlueprintTarget::Entity {
                    entity_id: eid.0,
                    entity_name: entity_name.clone(),
                };
                b
            }
            Err(_) => Blueprint {
                name: format!("Blueprint for {entity_name}"),
                target: BlueprintTarget::Entity {
                    entity_id: eid.0,
                    entity_name: entity_name.clone(),
                },
                graph: infengine_blueprint::Graph::default(),
                variables: Vec::new(),
                functions: Vec::new(),
                macros: Vec::new(),
            },
        };
        // Ensure the blueprints/ dir exists so first-save succeeds.
        if let Some(parent) = full.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Write the new blueprint (or re-save loaded one) immediately
        // so the entity binding survives across closes.
        if let Err(err) = blueprint.save(&full) {
            self.status = format!("Blueprint save failed: {err}");
        }
        if let Some(entity) = self.scene.get_mut(eid) {
            // Ensure the path appears in the scripts list. The legacy
            // `blueprint` slot is left in place for back-compat with
            // older scenes; new content drives off `blueprints`.
            if !entity.blueprints.contains(&rel_path) {
                entity.blueprints.push(rel_path.clone());
            }
            entity.blueprint = Some(rel_path);
        }
        self.blueprint.open(blueprint, Some(full));
        self.blueprint_active_entity = Some(eid);
        self.blueprint_open = true;
    }

    /// Save the currently-active tab's blueprint to disk.
    fn save_active_blueprint(&mut self) {
        let Some(tab) = self.blueprint.active() else {
            return;
        };
        let Some(path) = tab.path.clone() else {
            return;
        };
        let bp = tab.blueprint.clone();
        if let Err(err) = bp.save(&path) {
            self.status = format!("Blueprint save failed: {err}");
        }
    }

    fn start_play(&mut self) {
        if self.play_state != PlayState::Editing {
            return;
        }
        self.play_snapshot = Some(self.scene.clone());
        let mut world = PhysicsWorld::new();
        world.rebuild(&self.scene);
        self.play_physics = Some(world);
        // Build the 3D physics world from any entity that carries both
        // a Transform3D and a Physics3DComponent.
        let mut world_3d = PhysicsWorld3D::new();
        let mut map_3d: HashMap<EntityId, PhysicsBodyId> = HashMap::new();
        for entity in &self.scene.entities {
            if let (Some(t), Some(p)) = (&entity.transform_3d, &entity.physics_3d) {
                let kind = match p.body {
                    BodyKind3D::Dynamic => PhysicsBodyKind3D::Dynamic,
                    BodyKind3D::Static => PhysicsBodyKind3D::Static,
                    BodyKind3D::Kinematic => PhysicsBodyKind3D::Kinematic,
                };
                let shape = match p.shape {
                    ColliderShape3D::Box { half_extents } => {
                        PhysicsColliderShape3D::Box { half_extents }
                    }
                    ColliderShape3D::Sphere { radius } => PhysicsColliderShape3D::Sphere { radius },
                    ColliderShape3D::Capsule {
                        half_height,
                        radius,
                    } => PhysicsColliderShape3D::Capsule {
                        half_height,
                        radius,
                    },
                };
                let rotation = Quat::from_euler(
                    glam::EulerRot::XYZ,
                    t.rotation[0],
                    t.rotation[1],
                    t.rotation[2],
                );
                let id = world_3d.create_body(BodyDesc {
                    kind,
                    shape,
                    position: Vec3::from_array(t.position),
                    rotation,
                    linear_damping: p.linear_damping,
                    angular_damping: p.angular_damping,
                    restitution: p.restitution,
                    friction: p.friction,
                });
                map_3d.insert(entity.id, id);
            }
        }
        self.play_physics_3d = Some(world_3d);
        self.play_body_map_3d = map_3d;

        // Multi-blueprint runtime: gather every script attached to
        // every entity (plus the workspace's active tab so live edits
        // are previewable), compile their OnStart + OnTick programs,
        // fire OnStart once, cache OnTick for the tick loop.
        self.play_blueprint_ticks.clear();
        let mut scripts_to_run: Vec<(infengine_blueprint::Blueprint, std::path::PathBuf)> =
            Vec::new();
        for entity in &self.scene.entities {
            for rel in &entity.blueprints {
                let full = self.project_root.join(rel);
                if let Ok(mut bp) = infengine_blueprint::Blueprint::load(&full) {
                    // Refresh target so the right entity_id is used
                    // by compile_sink for SetEntityPosition.
                    bp.target = infengine_blueprint::BlueprintTarget::Entity {
                        entity_id: entity.id.0,
                        entity_name: entity.name.clone(),
                    };
                    scripts_to_run.push((bp, full));
                }
            }
        }
        // Also include the active workspace tab so live-edited
        // blueprints (without a script-list binding) preview in Play.
        if let Some(active) = self.blueprint.active() {
            scripts_to_run.push((
                active.blueprint.clone(),
                active.path.clone().unwrap_or_default(),
            ));
        }
        for (bp, _path) in &scripts_to_run {
            match infengine_blueprint::exec::compile_with_functions(
                &bp.graph,
                &bp.functions,
                Trigger::OnStart,
                &bp.target,
            ) {
                Ok(prog) => {
                    let mut host = TracingHost;
                    run_program(&prog, &mut host);
                }
                Err(err) => {
                    tracing::warn!(error = %err, "blueprint OnStart compile failed");
                }
            }
            if let Ok(prog) = infengine_blueprint::exec::compile_with_functions(
                &bp.graph,
                &bp.functions,
                Trigger::OnTick,
                &bp.target,
            ) {
                self.play_blueprint_ticks.push(prog);
            }
        }
        // Autoplay sounds attached to entities.
        let auto_clips: Vec<(String, f32)> = self
            .scene
            .entities
            .iter()
            .filter_map(|e| {
                let a = e.audio.as_ref()?;
                if a.autoplay {
                    Some((a.sound_path.clone(), a.volume))
                } else {
                    None
                }
            })
            .collect();
        if !auto_clips.is_empty() {
            if self.audio_system.is_none() {
                match AudioSystem::new() {
                    Ok(s) => self.audio_system = Some(s),
                    Err(err) => warn!(error = %err, "audio init failed"),
                }
            }
            if let Some(audio) = self.audio_system.as_mut() {
                for (path, volume) in auto_clips {
                    let full = self.project_root.join(&path);
                    match audio.load_sound(&full) {
                        Ok(sound) => {
                            // Volume is a simple beep tone hint until
                            // kira's per-instance settings are wired
                            // through `play_sound`. Ignore for now.
                            let _ = volume;
                            let _ = audio.play_sound(&sound);
                        }
                        Err(err) => {
                            warn!(error = %err, path = %full.display(), "audio load failed")
                        }
                    }
                }
            }
        }
        self.play_state = PlayState::Playing;
        self.status = "Play mode".into();
    }

    fn pause_play(&mut self) {
        if self.play_state == PlayState::Playing {
            self.play_state = PlayState::Paused;
            self.status = "Paused".into();
        } else if self.play_state == PlayState::Paused {
            self.play_state = PlayState::Playing;
            self.status = "Resumed".into();
        }
    }

    fn stop_play(&mut self) {
        if self.play_state == PlayState::Editing {
            return;
        }
        if let Some(snap) = self.play_snapshot.take() {
            self.scene = snap;
        }
        self.play_physics = None;
        self.play_physics_3d = None;
        self.play_body_map_3d.clear();
        self.play_blueprint_ticks.clear();
        self.play_state = PlayState::Editing;
        self.status = "Stopped".into();
    }

    fn tick_play(&mut self, dt: f32) {
        if self.play_state != PlayState::Playing {
            return;
        }
        if let Some(world) = self.play_physics.as_mut() {
            world.step(dt);
            world.write_back(&mut self.scene);
        }
        if let Some(world) = self.play_physics_3d.as_mut() {
            world.step(dt);
            for (entity_id, body_id) in &self.play_body_map_3d {
                if let Some(pos) = world.translation(*body_id)
                    && let Some(entity) = self.scene.get_mut(*entity_id)
                    && let Some(t) = entity.transform_3d.as_mut()
                {
                    t.position = pos.to_array();
                }
            }
        }
        // Run every cached blueprint tick. A custom Host captures any
        // SetEntityPosition writes (and Print logs) so we can apply
        // them to the scene afterwards without holding a self.scene
        // borrow during the VM loop.
        if !self.play_blueprint_ticks.is_empty() {
            let mut host = PlayHost::default();
            for prog in &self.play_blueprint_ticks {
                run_program(prog, &mut host);
            }
            for (eid, pos) in host.pending_position_writes {
                if let Some(entity) = self.scene.get_mut(EntityId(eid))
                    && let Some(t) = entity.transform_3d.as_mut()
                {
                    t.position = pos;
                }
            }
        }
    }

    fn camera_pos(&self) -> [f32; 2] {
        if let Some(id) = self.selected
            && let Some(e) = self.scene.get(id)
        {
            e.transform.position
        } else {
            [0.0, 0.0]
        }
    }
}

fn cursor_to_world(
    cursor: [f32; 2],
    viewport: (u32, u32),
    camera_pos: [f32; 2],
    zoom: f32,
) -> [f32; 2] {
    let half_w = viewport.0 as f32 * 0.5;
    let half_h = viewport.1 as f32 * 0.5;
    [
        camera_pos[0] + (cursor[0] - half_w) / zoom,
        camera_pos[1] - (cursor[1] - half_h) / zoom,
    ]
}

/// Project a 3D world point through the camera's view-projection matrix
/// to a physical-pixel position on the screen. Returns `None` for
/// points behind the camera or outside `[0, 1]` clip depth.
fn project_3d_to_screen(camera: &Camera3D, viewport: (u32, u32), world: Vec3) -> Option<[f32; 2]> {
    let clip = camera.view_projection() * Vec4::new(world.x, world.y, world.z, 1.0);
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z < 0.0 || ndc.z > 1.0 {
        return None;
    }
    Some([
        (ndc.x * 0.5 + 0.5) * viewport.0 as f32,
        (-ndc.y * 0.5 + 0.5) * viewport.1 as f32,
    ])
}

/// Distance from `p` to segment `a..b`, all in 2D pixels.
fn dist_point_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let len_sq = ab[0] * ab[0] + ab[1] * ab[1];
    if len_sq < 1e-6 {
        return (ap[0] * ap[0] + ap[1] * ap[1]).sqrt();
    }
    let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / len_sq).clamp(0.0, 1.0);
    let proj = [a[0] + ab[0] * t, a[1] + ab[1] * t];
    let d = [p[0] - proj[0], p[1] - proj[1]];
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

/// Hit-test the 3D translate gizmo of the selected 3D entity. Returns
/// the axis the cursor is closest to (within the pick threshold), or
/// `None` if no arm was hit.
fn pick_gizmo_3d(
    scene: &Scene,
    selected: Option<EntityId>,
    camera_3d: &Camera3D,
    viewport: (u32, u32),
    cursor: [f32; 2],
) -> Option<Axis3> {
    let id = selected?;
    let entity = scene.get(id)?;
    let t = entity.transform_3d.as_ref()?;
    let origin = Vec3::from_array(t.position);
    let s_origin = project_3d_to_screen(camera_3d, viewport, origin)?;
    let mut best = (f32::INFINITY, Axis3::X);
    for axis in [Axis3::X, Axis3::Y, Axis3::Z] {
        let end = origin + axis.unit() * GIZMO_3D_AXIS_LEN;
        let Some(s_end) = project_3d_to_screen(camera_3d, viewport, end) else {
            continue;
        };
        let d = dist_point_to_segment(cursor, s_origin, s_end);
        if d < best.0 {
            best = (d, axis);
        }
    }
    if best.0 <= GIZMO_3D_PICK_PIXELS {
        Some(best.1)
    } else {
        None
    }
}

/// Blueprint VM host the editor uses during Play. Captures
/// SetEntityPosition writes so the editor can apply them after the
/// VM relinquishes its &mut borrow on the host.
#[derive(Debug, Default)]
struct PlayHost {
    pending_position_writes: Vec<(u32, [f32; 3])>,
}

impl infengine_blueprint::exec::Host for PlayHost {
    fn print(&mut self, value: infengine_blueprint::exec::Value) {
        use infengine_blueprint::exec::Value;
        match value {
            Value::Float(v) => tracing::info!(value = v, "blueprint print"),
            Value::Bool(b) => tracing::info!(value = b, "blueprint print"),
            Value::Vec3(v) => tracing::info!(?v, "blueprint print"),
            Value::EntityRef(e) => tracing::info!(entity = e, "blueprint print"),
        }
    }
    fn set_entity_position(&mut self, entity_id: u32, position: [f32; 3]) {
        self.pending_position_writes.push((entity_id, position));
    }
}

fn transform_3d_to_matrix(t: &Transform3D) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from_array(t.scale),
        Quat::from_euler(
            glam::EulerRot::XYZ,
            t.rotation[0],
            t.rotation[1],
            t.rotation[2],
        ),
        Vec3::from_array(t.position),
    )
}

/// Composed world-space 2D transform: walks the parent chain
/// multiplying scale and offsetting position. Rotation is omitted
/// because Transform2D doesn't carry one yet.
fn entity_world_transform_2d(scene: &Scene, entity: &Entity) -> Transform2D {
    let local = entity.transform;
    match entity.parent.and_then(|p| scene.get(p)) {
        Some(parent) => {
            let p = entity_world_transform_2d(scene, parent);
            Transform2D {
                position: [
                    p.position[0] + local.position[0] * p.scale[0],
                    p.position[1] + local.position[1] * p.scale[1],
                ],
                scale: [p.scale[0] * local.scale[0], p.scale[1] * local.scale[1]],
            }
        }
        None => local,
    }
}

/// World-space model matrix for a 3D entity: walks up the parent
/// chain so each entity inherits its ancestors' transforms (Unity-
/// style scene-graph composition). Entities without a `transform_3d`
/// contribute an identity matrix so a 2D-only ancestor doesn't break
/// the chain.
fn entity_world_matrix_3d(scene: &Scene, entity: &Entity) -> Mat4 {
    let local = entity
        .transform_3d
        .as_ref()
        .map(transform_3d_to_matrix)
        .unwrap_or(Mat4::IDENTITY);
    match entity.parent.and_then(|p| scene.get(p)) {
        Some(parent) => entity_world_matrix_3d(scene, parent) * local,
        None => local,
    }
}

/// Helper that returns the world-space position of an entity's 3D
/// transform, including parent chain. Convenience for the camera
/// target / 3D gizmo paths.
fn entity_world_position_3d(scene: &Scene, entity: &Entity) -> Option<Vec3> {
    if entity.transform_3d.is_none() && entity.parent.is_none() {
        return None;
    }
    let m = entity_world_matrix_3d(scene, entity);
    Some(m.w_axis.truncate())
}

/// Draw the 3D translate gizmo as screen-space debug lines projected
/// through the renderer's current 3D camera. Assumes the 2D camera has
/// been pinned at (0,0) zoom=1 so 2D world coords match screen pixels
/// minus the viewport center.
fn draw_gizmo_3d(renderer: &mut Renderer, viewport: (u32, u32), origin: Vec3) {
    let cam = renderer.camera_3d().clone();
    let Some(s_origin) = project_3d_to_screen(&cam, viewport, origin) else {
        return;
    };
    let origin_world = cursor_to_world(s_origin, viewport, [0.0, 0.0], 1.0);
    let colors = [
        (Axis3::X, [1.0, 0.3, 0.3, 1.0]),
        (Axis3::Y, [0.3, 1.0, 0.3, 1.0]),
        (Axis3::Z, [0.4, 0.5, 1.0, 1.0]),
    ];
    for (axis, color) in colors {
        let end = origin + axis.unit() * GIZMO_3D_AXIS_LEN;
        let Some(s_end) = project_3d_to_screen(&cam, viewport, end) else {
            continue;
        };
        let end_world = cursor_to_world(s_end, viewport, [0.0, 0.0], 1.0);
        renderer.debug_line(origin_world, end_world, color);
    }
    let yellow = [1.0, 1.0, 0.3, 1.0];
    renderer.debug_rect(
        [
            origin_world[0] - GIZMO_HANDLE_HALF,
            origin_world[1] - GIZMO_HANDLE_HALF,
        ],
        [
            origin_world[0] + GIZMO_HANDLE_HALF,
            origin_world[1] + GIZMO_HANDLE_HALF,
        ],
        yellow,
    );
}

/// Hit-test the translate gizmo of the currently-selected entity.
/// Returns the axis the user clicked on, if any. The gizmo is drawn
/// in world units, so test against world-space coordinates.
fn pick_gizmo(scene: &Scene, selected: Option<EntityId>, world: [f32; 2]) -> Option<DragAxis> {
    let id = selected?;
    let entity = scene.get(id)?;
    let p = entity.transform.position;
    let dx = world[0] - p[0];
    let dy = world[1] - p[1];

    // Center handle (free).
    if dx.abs() <= GIZMO_HANDLE_HALF && dy.abs() <= GIZMO_HANDLE_HALF {
        return Some(DragAxis::Free);
    }
    // X handle: along [+0, +GIZMO_AXIS_LEN] with thickness.
    if (0.0..=GIZMO_AXIS_LEN).contains(&dx) && dy.abs() <= GIZMO_PICK_THICKNESS * 0.5 {
        return Some(DragAxis::X);
    }
    // Y handle.
    if (0.0..=GIZMO_AXIS_LEN).contains(&dy) && dx.abs() <= GIZMO_PICK_THICKNESS * 0.5 {
        return Some(DragAxis::Y);
    }
    None
}

/// Recursive scene-tree row renderer. Walks `children` starting at
/// `parent`, indents each row by `depth`, lets the user select / open
/// the blueprint / delete each entity.
#[allow(clippy::too_many_arguments)]
fn draw_hierarchy(
    ui: &mut egui::Ui,
    scene: &Scene,
    children: &HashMap<Option<EntityId>, Vec<usize>>,
    parent: Option<EntityId>,
    depth: usize,
    selected_id: Option<EntityId>,
    selected_ref: &mut Option<EntityId>,
    delete_entity: &mut Option<EntityId>,
    open_blueprint: &mut Option<EntityId>,
    reparent_request: &mut Option<(EntityId, Option<EntityId>)>,
) {
    let Some(indices) = children.get(&parent) else {
        return;
    };
    for &i in indices {
        let entity = &scene.entities[i];
        let is_sel = Some(entity.id) == selected_id;
        let glyph = if entity.is_3d() {
            "3D "
        } else if entity.sprite.is_some() {
            "S  "
        } else {
            "   "
        };
        let class_suffix = match &entity.class {
            Some(c) => format!(" [{c}]"),
            None => String::new(),
        };
        let label = format!("{glyph}{}{} (#{})", entity.name, class_suffix, entity.id.0);
        // Wrap each row in a drop zone so other rows can be dragged
        // onto it; the row itself is the drag source. egui's
        // `dnd_*` API does the visual feedback for us.
        let drop_id = egui::Id::new(("hierarchy_drop", entity.id.0));
        let drop = ui.dnd_drop_zone::<EntityId, _>(egui::Frame::default(), |ui| {
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 12.0);
                let src_id = egui::Id::new(("hierarchy_drag", entity.id.0));
                let resp = ui
                    .dnd_drag_source(src_id, entity.id, |ui| ui.selectable_label(is_sel, label))
                    .response;
                if resp.clicked() {
                    *selected_ref = Some(entity.id);
                }
                if resp.double_clicked() {
                    *open_blueprint = Some(entity.id);
                    *selected_ref = Some(entity.id);
                }
                if ui
                    .small_button("BP")
                    .on_hover_text("Open blueprint")
                    .clicked()
                {
                    *open_blueprint = Some(entity.id);
                    *selected_ref = Some(entity.id);
                }
                if ui.small_button("X").clicked() {
                    *delete_entity = Some(entity.id);
                }
            });
        });
        let _ = drop_id;
        if let Some(payload) = drop.1
            && *payload != entity.id
            && !is_descendant_of(scene, entity.id, *payload)
        {
            *reparent_request = Some((*payload, Some(entity.id)));
        }
        // Recurse into children.
        draw_hierarchy(
            ui,
            scene,
            children,
            Some(entity.id),
            depth + 1,
            selected_id,
            selected_ref,
            delete_entity,
            open_blueprint,
            reparent_request,
        );
    }
}

/// Slab-method ray vs. axis-aligned box. Returns the entry distance
/// (clamped at 0 so rays starting inside the box still pick it).
fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = Vec3::new(
        if dir.x != 0.0 {
            1.0 / dir.x
        } else {
            f32::INFINITY
        },
        if dir.y != 0.0 {
            1.0 / dir.y
        } else {
            f32::INFINITY
        },
        if dir.z != 0.0 {
            1.0 / dir.z
        } else {
            f32::INFINITY
        },
    );
    let t1 = (min - origin) * inv;
    let t2 = (max - origin) * inv;
    let tmin = t1.min(t2);
    let tmax = t1.max(t2);
    let t_near = tmin.x.max(tmin.y).max(tmin.z);
    let t_far = tmax.x.min(tmax.y).min(tmax.z);
    if t_near <= t_far && t_far >= 0.0 {
        Some(t_near.max(0.0))
    } else {
        None
    }
}

/// Cast a ray from the camera through the cursor and pick the nearest
/// 3D entity whose AABB the ray intersects. The AABB is built from
/// the entity's `transform_3d` (scale + position); rotation is
/// ignored, so heavily rotated meshes pick imprecisely - acceptable
/// for the current cube-only mesh catalogue.
fn pick_3d_entity(
    scene: &Scene,
    camera: &Camera3D,
    viewport: (u32, u32),
    cursor: [f32; 2],
) -> Option<EntityId> {
    let w = viewport.0.max(1) as f32;
    let h = viewport.1.max(1) as f32;
    let ndc_x = (cursor[0] / w) * 2.0 - 1.0;
    let ndc_y = 1.0 - (cursor[1] / h) * 2.0;
    let inv = camera.view_projection().inverse();
    let near = inv * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
    let far = inv * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
    if near.w.abs() < 1e-6 || far.w.abs() < 1e-6 {
        return None;
    }
    let near = near.truncate() / near.w;
    let far = far.truncate() / far.w;
    let dir = (far - near).normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    let origin = camera.position;

    let mut best: Option<(f32, EntityId)> = None;
    for entity in &scene.entities {
        let Some(t) = entity.transform_3d.as_ref() else {
            continue;
        };
        let half = Vec3::new(
            t.scale[0].abs() * 0.5,
            t.scale[1].abs() * 0.5,
            t.scale[2].abs() * 0.5,
        );
        let center = Vec3::from_array(t.position);
        let min = center - half;
        let max = center + half;
        if let Some(t_hit) = ray_aabb(origin, dir, min, max)
            && best.is_none_or(|(b, _)| t_hit < b)
        {
            best = Some((t_hit, entity.id));
        }
    }
    best.map(|(_, id)| id)
}

fn pick_entity(scene: &Scene, world_point: [f32; 2]) -> Option<(EntityId, [f32; 2])> {
    let mut hit = None;
    for entity in &scene.entities {
        let Some(sprite) = &entity.sprite else {
            continue;
        };
        let half_x = sprite.size[0] * entity.transform.scale[0] * 0.5;
        let half_y = sprite.size[1] * entity.transform.scale[1] * 0.5;
        let p = entity.transform.position;
        if world_point[0] >= p[0] - half_x
            && world_point[0] <= p[0] + half_x
            && world_point[1] >= p[1] - half_y
            && world_point[1] <= p[1] + half_y
        {
            hit = Some((entity.id, [world_point[0] - p[0], world_point[1] - p[1]]));
        }
    }
    hit
}

/// Recursively copy `src` into `dst`. Used by the editor's Export step.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn region_label(r: &AtlasRegion) -> &'static str {
    match r {
        AtlasRegion::Full => "Full",
        AtlasRegion::Checker => "Checker",
        AtlasRegion::Dots => "Dots",
        AtlasRegion::Stripes => "Stripes",
        AtlasRegion::Cross => "Cross",
    }
}

impl AppHandler for Editor {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, project = %self.project.name, "editor_app started");
        match Renderer::new(window) {
            Ok(r) => {
                let editor = EditorUi::new(r.wgpu_device(), r.surface_format(), window.raw(), 1);
                self.editor_ui = Some(editor);
                self.renderer = Some(r);
            }
            Err(err) => {
                error!(error = %err, "renderer init failed");
                std::process::exit(2);
            }
        }
    }

    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(editor) = self.editor_ui.as_mut() {
            editor.handle_engine_event(window, event);
        }
        let egui_wants = self
            .editor_ui
            .as_ref()
            .is_some_and(|e| e.wants_pointer_input());
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = [position.x as f32, position.y as f32];
            }
            WindowEvent::MouseInput { state, button, .. } if *button == MouseButton::Left => {
                if !egui_wants {
                    self.left_button = matches!(state, ElementState::Pressed);
                } else if matches!(state, ElementState::Released) {
                    self.left_button = false;
                }
            }
            WindowEvent::MouseInput { state, button, .. } if *button == MouseButton::Middle => {
                if !egui_wants {
                    self.middle_button = matches!(state, ElementState::Pressed);
                } else if matches!(state, ElementState::Released) {
                    self.middle_button = false;
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !egui_wants => {
                use infengine_window::raw_winit::event::MouseScrollDelta;
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 50.0,
                };
                self.scroll_accum += amount;
            }
            _ => {}
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        let dt = ctx.delta_time.max(1e-4);
        self.fps_ema = self.fps_ema * 0.9 + (1.0 / dt) * 0.1;

        // Modifier + Z / Y edge-triggered undo / redo.
        self.ctrl_held = ctx
            .input
            .any_pressed(&[Key::ControlLeft, Key::ControlRight]);
        self.shift_held = ctx.input.any_pressed(&[Key::ShiftLeft, Key::ShiftRight]);
        let z_now = ctx.input.is_pressed(Key::Z);
        let y_now = ctx.input.is_pressed(Key::Y);
        let z_just = z_now && !self.z_key_prev;
        let y_just = y_now && !self.y_key_prev;
        self.z_key_prev = z_now;
        self.y_key_prev = y_now;
        if self.ctrl_held && z_just {
            if self.shift_held {
                self.redo();
            } else {
                self.undo();
            }
        }
        if self.ctrl_held && y_just {
            self.redo();
        }

        let mut zoom_step = 0.0_f32;
        if ctx.input.is_pressed(Key::Q) {
            zoom_step -= 1.0;
        }
        if ctx.input.is_pressed(Key::E) {
            zoom_step += 1.0;
        }
        if zoom_step != 0.0 {
            let factor = (zoom_step * 1.6 * dt).exp();
            self.zoom = (self.zoom * factor).clamp(0.25, 4.0);
        }

        let cursor_world =
            cursor_to_world(self.cursor_pos, self.viewport, self.camera_pos(), self.zoom);
        let left_just_pressed = self.left_button && !self.left_button_prev;
        let left_just_released = !self.left_button && self.left_button_prev;
        self.left_button_prev = self.left_button;
        let egui_pointer = self
            .editor_ui
            .as_ref()
            .is_some_and(|e| e.wants_pointer_input());
        let selected_is_3d = self
            .selected
            .and_then(|id| self.scene.get(id))
            .is_some_and(|e| e.is_3d());
        let camera_3d_snapshot = self.renderer.as_ref().map(|r| r.camera_3d().clone());

        // Detect viewport double-clicks ourselves: winit doesn't
        // expose a click-count, so compare against the previous press
        // (time + position). 400 ms / 8 px is the standard threshold.
        let mut viewport_double_click = false;
        if left_just_pressed && !egui_pointer {
            let now = std::time::Instant::now();
            if let Some(prev) = self.last_left_click_time {
                let dt_ms = now.duration_since(prev).as_millis();
                let dx = self.cursor_pos[0] - self.last_left_click_pos[0];
                let dy = self.cursor_pos[1] - self.last_left_click_pos[1];
                if dt_ms < 400 && (dx * dx + dy * dy) < 64.0 {
                    viewport_double_click = true;
                }
            }
            self.last_left_click_time = Some(now);
            self.last_left_click_pos = self.cursor_pos;
        }
        // Entity to open in the blueprint editor once viewport pick
        // resolves. Filled in below; deferred so the call doesn't
        // overlap with the outstanding `&mut self.scene` borrow.
        let mut open_bp_after_pick: Option<EntityId> = None;
        if left_just_pressed && !egui_pointer {
            let mut handled_3d = false;
            if selected_is_3d
                && let Some(cam) = camera_3d_snapshot.as_ref()
                && let Some(axis) = pick_gizmo_3d(
                    &self.scene,
                    self.selected,
                    cam,
                    self.viewport,
                    self.cursor_pos,
                )
                && let Some(id) = self.selected
                && let Some(start_pos) = self
                    .scene
                    .get(id)
                    .and_then(|e| e.transform_3d.as_ref().map(|t| t.position))
            {
                self.push_snapshot();
                self.drag_3d = Some(Drag3DState {
                    entity: id,
                    axis,
                    start_pos,
                    start_cursor: self.cursor_pos,
                });
                handled_3d = true;
            }
            if !handled_3d {
                // Gizmo on the selected entity takes priority over body pick.
                let gizmo_hit = pick_gizmo(&self.scene, self.selected, cursor_world);
                if let Some(axis) = gizmo_hit
                    && let Some(id) = self.selected
                {
                    let p = self
                        .scene
                        .get(id)
                        .map(|e| e.transform.position)
                        .unwrap_or([0.0, 0.0]);
                    self.push_snapshot();
                    self.dragging = Some(id);
                    self.drag_axis = axis;
                    self.drag_offset = [cursor_world[0] - p[0], cursor_world[1] - p[1]];
                } else if let Some((id, ofs)) = pick_entity(&self.scene, cursor_world) {
                    self.push_snapshot();
                    self.selected = Some(id);
                    self.dragging = Some(id);
                    self.drag_axis = DragAxis::Free;
                    self.drag_offset = ofs;
                    if viewport_double_click {
                        open_bp_after_pick = Some(id);
                    }
                } else if let Some(cam) = camera_3d_snapshot.as_ref()
                    && let Some(id) =
                        pick_3d_entity(&self.scene, cam, self.viewport, self.cursor_pos)
                {
                    // 3D viewport pick: aim a ray from the camera
                    // through the cursor and select the nearest 3D
                    // entity its AABB hits. Doesn't start a drag -
                    // the 3D translate gizmo is responsible for that
                    // on the next click.
                    self.selected = Some(id);
                    if viewport_double_click {
                        open_bp_after_pick = Some(id);
                    }
                } else if viewport_double_click && let Some(sel) = self.selected {
                    // Double-click on the 2D body that was already
                    // selected this frame also opens the blueprint.
                    open_bp_after_pick = Some(sel);
                }
            }
        }
        if let Some(eid) = open_bp_after_pick {
            self.open_entity_blueprint(eid);
        }
        if left_just_released {
            self.dragging = None;
            self.drag_3d = None;
        }
        if self.left_button
            && let Some(id) = self.dragging
            && let Some(entity) = self.scene.get_mut(id)
        {
            let new_x = cursor_world[0] - self.drag_offset[0];
            let new_y = cursor_world[1] - self.drag_offset[1];
            match self.drag_axis {
                DragAxis::Free => {
                    entity.transform.position[0] = new_x;
                    entity.transform.position[1] = new_y;
                }
                DragAxis::X => {
                    entity.transform.position[0] = new_x;
                }
                DragAxis::Y => {
                    entity.transform.position[1] = new_y;
                }
            }
        }
        // 3D translate-gizmo drag: convert the mouse pixel delta into a
        // world-space delta along the projected axis line.
        if self.left_button
            && let Some(state) = self.drag_3d
            && let Some(cam) = camera_3d_snapshot.as_ref()
        {
            let origin = Vec3::from_array(state.start_pos);
            let unit_end = origin + state.axis.unit();
            if let (Some(s_origin), Some(s_unit_end)) = (
                project_3d_to_screen(cam, self.viewport, origin),
                project_3d_to_screen(cam, self.viewport, unit_end),
            ) {
                let screen_axis = [s_unit_end[0] - s_origin[0], s_unit_end[1] - s_origin[1]];
                let len_sq = screen_axis[0] * screen_axis[0] + screen_axis[1] * screen_axis[1];
                if len_sq > 1e-4 {
                    let mouse_delta = [
                        self.cursor_pos[0] - state.start_cursor[0],
                        self.cursor_pos[1] - state.start_cursor[1],
                    ];
                    let dot = mouse_delta[0] * screen_axis[0] + mouse_delta[1] * screen_axis[1];
                    let delta_world = dot / len_sq;
                    let new_pos = origin + state.axis.unit() * delta_world;
                    if let Some(entity) = self.scene.get_mut(state.entity)
                        && let Some(t) = entity.transform_3d.as_mut()
                    {
                        t.position = new_pos.to_array();
                    }
                }
            }
        }

        self.tick_play(dt);

        let fps = self.fps_ema;
        let mut action_save = false;
        let mut action_save_all = false;
        let mut action_add_scene = false;
        let mut action_add_entity_sprite = false;
        let mut action_add_entity_empty = false;
        let mut action_add_entity_cube = false;
        let mut action_open_blueprint_for: Option<EntityId> = None;
        let mut action_open_scene_blueprint = false;
        let mut action_open_global_blueprint = false;
        let mut blueprint_dirty = false;
        let mut action_new_class = false;
        let mut action_spawn_from_class: Option<String> = None;
        let mut reparent_request: Option<(EntityId, Option<EntityId>)> = None;
        let mut action_undo = false;
        let mut action_redo = false;
        let mut action_play = false;
        let mut action_pause = false;
        let mut action_stop = false;
        let mut action_export = false;
        let mut action_build_standalone = false;
        let mut action_refresh_assets = false;
        let mut action_import_gltf: Option<String> = None;
        let mut switch_to: Option<usize> = None;
        let mut delete_entity: Option<EntityId> = None;
        let undo_count = self.undo_stack.len();
        let redo_count = self.redo_stack.len();

        if let Some(editor_ui) = self.editor_ui.as_mut() {
            let project_name = self.project.name.clone();
            let scenes = self.project.scenes.clone();
            let classes = self.project.classes.clone();
            let current = self.current_scene_index;
            let selected_id = self.selected;
            let status = self.status.clone();
            let scene_ref = &mut self.scene;
            let selected_ref = &mut self.selected;
            let asset_list = self.asset_listing.clone();
            let play_state = self.play_state;
            let blueprint = &mut self.blueprint;
            let blueprint_open = &mut self.blueprint_open;

            let node_registry = &self.node_registry;
            let gltf_path = &mut self.gltf_path_input;
            let imported_mesh_names: Vec<String> = self.imported_meshes.keys().cloned().collect();
            let mut grid_visible = self
                .renderer
                .as_ref()
                .map(|r| r.show_grid())
                .unwrap_or(true);
            // Inspector UI.
            editor_ui.run(window.raw(), |ctx| {
                egui::TopBottomPanel::top("play_toolbar").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let play_label = match play_state {
                            PlayState::Editing => "Play",
                            PlayState::Playing => "Playing...",
                            PlayState::Paused => "Resume",
                        };
                        if ui
                            .add_enabled(
                                play_state != PlayState::Playing,
                                egui::Button::new(play_label),
                            )
                            .clicked()
                        {
                            match play_state {
                                PlayState::Editing => action_play = true,
                                PlayState::Paused => action_pause = true,
                                PlayState::Playing => {}
                            }
                        }
                        if ui
                            .add_enabled(
                                play_state == PlayState::Playing,
                                egui::Button::new("Pause"),
                            )
                            .clicked()
                        {
                            action_pause = true;
                        }
                        if ui
                            .add_enabled(
                                play_state != PlayState::Editing,
                                egui::Button::new("Stop"),
                            )
                            .clicked()
                        {
                            action_stop = true;
                        }
                        if play_state != PlayState::Editing {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 96, 96),
                                match play_state {
                                    PlayState::Playing => "[PLAY]",
                                    PlayState::Paused => "[PAUSED]",
                                    PlayState::Editing => "",
                                },
                            );
                        }
                        ui.separator();
                        ui.toggle_value(blueprint_open, "Blueprint");
                        ui.separator();
                        ui.checkbox(&mut grid_visible, "Grid");
                    });
                });

                if *blueprint_open {
                    let action_scene = &mut action_open_scene_blueprint;
                    let action_global = &mut action_open_global_blueprint;
                    egui::Window::new("Blueprint editor")
                        .default_size([720.0, 480.0])
                        .open(blueprint_open)
                        .show(ctx, |ui| {
                            // Quick access bar so the user can spin up
                            // a Scene / Global blueprint without
                            // having to double-click any entity.
                            ui.horizontal(|ui| {
                                if ui.button("Open Scene Blueprint").clicked() {
                                    *action_scene = true;
                                }
                                if ui.button("Open Global Blueprint").clicked() {
                                    *action_global = true;
                                }
                            });
                            ui.separator();
                            if blueprint.show(ui, node_registry) {
                                blueprint_dirty = true;
                            }
                        });
                }

                egui::SidePanel::left("inspector")
                    .resizable(true)
                    .default_width(320.0)
                    .show(ctx, |ui| {
                        ui.heading(format!("Project: {project_name}"));
                        ui.label(format!("FPS: {fps:.0}"));
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    undo_count > 0,
                                    egui::Button::new(format!("Undo ({undo_count})")),
                                )
                                .clicked()
                            {
                                action_undo = true;
                            }
                            if ui
                                .add_enabled(
                                    redo_count > 0,
                                    egui::Button::new(format!("Redo ({redo_count})")),
                                )
                                .clicked()
                            {
                                action_redo = true;
                            }
                        });
                        if !status.is_empty() {
                            ui.small(status.clone());
                        }
                        ui.separator();

                        ui.collapsing("Scenes", |ui| {
                            for (i, entry) in scenes.iter().enumerate() {
                                let selected = current == Some(i);
                                if ui.selectable_label(selected, &entry.name).clicked() {
                                    switch_to = Some(i);
                                }
                            }
                            ui.horizontal(|ui| {
                                if ui.button("Save scene").clicked() {
                                    action_save = true;
                                }
                                if ui.button("Save project").clicked() {
                                    action_save_all = true;
                                }
                                if ui.button("New scene").clicked() {
                                    action_add_scene = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.button("Export -> dist/").clicked() {
                                    action_export = true;
                                }
                                if ui.button("Build standalone").clicked() {
                                    action_build_standalone = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Background");
                                ui.color_edit_button_rgb(&mut scene_ref.clear_color);
                            });
                        });

                        ui.collapsing("Hierarchy", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    // Pre-compute the parent -> children
                                    // index map so the tree walk doesn't
                                    // re-scan all entities per node.
                                    let mut children: HashMap<Option<EntityId>, Vec<usize>> =
                                        HashMap::new();
                                    for (i, e) in scene_ref.entities.iter().enumerate() {
                                        children.entry(e.parent).or_default().push(i);
                                    }
                                    // Top-level drop zone -> drop here to
                                    // promote an entity to root.
                                    let root_drop = ui.dnd_drop_zone::<EntityId, _>(
                                        egui::Frame::default(),
                                        |ui| {
                                            ui.small("(drop here to make root)");
                                        },
                                    );
                                    if let Some(payload) = root_drop.1 {
                                        reparent_request = Some((*payload, None));
                                    }
                                    draw_hierarchy(
                                        ui,
                                        scene_ref,
                                        &children,
                                        None,
                                        0,
                                        selected_id,
                                        selected_ref,
                                        &mut delete_entity,
                                        &mut action_open_blueprint_for,
                                        &mut reparent_request,
                                    );
                                });
                            ui.horizontal(|ui| {
                                if ui.button("Add sprite").clicked() {
                                    action_add_entity_sprite = true;
                                }
                                if ui.button("Add empty").clicked() {
                                    action_add_entity_empty = true;
                                }
                                if ui.button("Add 3D cube").clicked() {
                                    action_add_entity_cube = true;
                                }
                            });
                        });

                        ui.collapsing("Assets", |ui| {
                            if asset_list.is_empty() {
                                ui.label("(no files under assets/)");
                            } else {
                                egui::ScrollArea::vertical()
                                    .max_height(180.0)
                                    .show(ui, |ui| {
                                        for entry in &asset_list {
                                            ui.label(entry.display().to_string());
                                        }
                                    });
                            }
                            if ui.button("Refresh").clicked() {
                                action_refresh_assets = true;
                            }
                        });

                        ui.collapsing("Classes", |ui| {
                            if classes.is_empty() {
                                ui.label("(no classes registered)");
                            } else {
                                for c in &classes {
                                    ui.horizontal(|ui| {
                                        ui.label(&c.name);
                                        if ui.small_button("Spawn").clicked() {
                                            action_spawn_from_class = Some(c.name.clone());
                                        }
                                    });
                                }
                            }
                            if ui.button("New class").clicked() {
                                action_new_class = true;
                            }
                        });

                        ui.collapsing("Import glTF", |ui| {
                            ui.label("Path to .gltf or .glb");
                            ui.text_edit_singleline(gltf_path);
                            if ui.button("Import").clicked() && !gltf_path.is_empty() {
                                action_import_gltf = Some(gltf_path.clone());
                            }
                            ui.separator();
                            ui.label(format!("{} imported mesh(es)", imported_mesh_names.len()));
                            for name in &imported_mesh_names {
                                ui.small(name);
                            }
                        });

                        ui.collapsing("Selection", |ui| {
                            inspector_for_selection(ui, scene_ref, selected_ref);
                        });

                        ui.separator();
                        ui.label("Q / E = zoom; click = pick + drag; Esc = quit");
                        ui.label("Ctrl+Z = undo; Ctrl+Y or Ctrl+Shift+Z = redo");
                    });
            });
            if let Some(r) = self.renderer.as_mut() {
                r.set_show_grid(grid_visible);
            }
        }

        if action_undo {
            self.undo();
        }
        if action_redo {
            self.redo();
        }
        if action_play {
            self.start_play();
        }
        if action_pause {
            self.pause_play();
        }
        if action_stop {
            self.stop_play();
        }
        // Apply deferred actions outside the immutable closure context.
        if let Some(id) = delete_entity {
            self.push_snapshot();
            self.scene.remove(id);
            if self.selected == Some(id) {
                self.selected = self
                    .scene
                    .entities
                    .iter()
                    .find(|e| e.sprite.is_some())
                    .map(|e| e.id);
            }
        }
        if let Some(idx) = switch_to {
            self.open_scene(idx);
        }
        if action_save {
            self.save_current();
        }
        if action_save_all {
            self.save_project();
        }
        if action_add_scene {
            self.add_scene();
        }
        if action_export {
            self.export();
        }
        if action_build_standalone {
            self.build_standalone();
        }
        if let Some((child, new_parent)) = reparent_request
            && let Some(entity) = self.scene.get_mut(child)
        {
            entity.parent = new_parent;
        }
        if action_new_class {
            let suffix = self.project.classes.len() + 1;
            self.project.classes.push(infengine_core::project::Class {
                name: format!("Class{suffix}"),
                blueprint: None,
                default_components: vec![],
            });
            let _ = self.project.save(&self.project_file);
            self.status = format!("Added class #{suffix}");
        }
        if let Some(class_name) = action_spawn_from_class {
            self.push_snapshot();
            let i = self
                .scene
                .add_entity(format!("{}_{}", class_name, self.scene.next_id));
            self.scene.entities[i].class = Some(class_name.clone());
            // Class instances default to 3D entities with a cube mesh
            // so they appear in the viewport immediately. The class's
            // `default_components` field will replace this hardcoded
            // bundle once components are first-class.
            self.scene.entities[i].transform_3d = Some(Transform3D::default());
            self.scene.entities[i].mesh = Some(MeshComponent::cube([0.6, 0.7, 0.85, 1.0]));
            // Inherit the class's shared blueprint reference so all
            // instances point at the same .ron file.
            if let Some(class) = self.project.classes.iter().find(|c| c.name == class_name)
                && let Some(bp) = class.blueprint.clone()
            {
                self.scene.entities[i].blueprint = Some(bp);
            }
            self.selected = Some(self.scene.entities[i].id);
            self.status = format!("Spawned instance of class '{class_name}'");
        }
        if let Some(eid) = action_open_blueprint_for {
            self.open_entity_blueprint(eid);
        }
        if action_open_scene_blueprint {
            self.open_scene_blueprint();
        }
        if action_open_global_blueprint {
            self.open_global_blueprint();
        }
        if blueprint_dirty {
            self.save_active_blueprint();
        }
        if let Some(path) = action_import_gltf
            && let Some(r) = self.renderer.as_mut()
        {
            match r.import_gltf_meshes(&path) {
                Ok(out) => {
                    let count = out.len();
                    for (name, handle) in out {
                        self.imported_meshes.insert(name, handle);
                    }
                    self.status = format!("Imported {count} mesh(es) from {path}");
                }
                Err(err) => {
                    self.status = format!("glTF import failed: {err}");
                }
            }
        }
        if action_refresh_assets {
            self.refresh_assets();
        }
        if action_add_entity_sprite {
            self.push_snapshot();
            let i = self
                .scene
                .add_entity(format!("entity_{}", self.scene.next_id));
            self.scene.entities[i].sprite =
                Some(SpriteComponent::new([64.0, 64.0], AtlasRegion::Checker));
            self.selected = Some(self.scene.entities[i].id);
        }
        if action_add_entity_empty {
            self.push_snapshot();
            let i = self
                .scene
                .add_entity(format!("entity_{}", self.scene.next_id));
            self.selected = Some(self.scene.entities[i].id);
        }
        if action_add_entity_cube {
            self.push_snapshot();
            let i = self
                .scene
                .add_entity(format!("cube_{}", self.scene.next_id));
            self.scene.entities[i].transform_3d = Some(Transform3D::default());
            self.scene.entities[i].mesh = Some(MeshComponent::cube([0.8, 0.8, 0.85, 1.0]));
            self.selected = Some(self.scene.entities[i].id);
        }

        // 3. Render the world now that scene state is final.
        let imported_meshes_snapshot = self.imported_meshes.clone();
        // Apply any pending mesh-texture loads. We compare against
        // `loaded_mesh_textures` so paths that haven't changed don't
        // re-decode the file every frame.
        let texture_requests: Vec<(String, String)> = self
            .scene
            .entities
            .iter()
            .filter_map(|e| {
                let m = e.mesh.as_ref()?;
                let t = m.texture.as_ref()?;
                if self.loaded_mesh_textures.get(&m.mesh) == Some(t) {
                    None
                } else {
                    Some((m.mesh.clone(), t.clone()))
                }
            })
            .collect();
        for (mesh_name, tex_path) in texture_requests {
            let handle = if mesh_name == "cube" {
                self.renderer.as_ref().map(|r| r.cube_handle())
            } else {
                imported_meshes_snapshot.get(&mesh_name).copied()
            };
            let Some(handle) = handle else {
                continue;
            };
            let full = self.project_root.join(&tex_path);
            if let Some(r) = self.renderer.as_mut() {
                match r.load_mesh_albedo_for(handle, &full) {
                    Ok(()) => {
                        self.loaded_mesh_textures.insert(mesh_name, tex_path);
                    }
                    Err(err) => {
                        self.status = format!("Texture load failed: {err}");
                    }
                }
            }
        }
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let viewport = self.viewport;
        // Set up cameras. When a 3D entity is selected the 2D camera is
        // pinned at (0,0) so the screen-space gizmo overlay maps 1:1
        // with viewport pixels; otherwise the 2D camera follows the
        // selected sprite as before.
        {
            let camera = renderer.camera_mut();
            if selected_is_3d {
                camera.position = [0.0, 0.0];
                camera.zoom = 1.0;
            } else if let Some(id) = self.selected
                && let Some(e) = self.scene.get(id)
            {
                camera.position = e.transform.position;
                camera.zoom = self.zoom;
            } else {
                camera.position = [0.0, 0.0];
                camera.zoom = self.zoom;
            }
        }
        // Blender-style 3D camera:
        //   middle drag           orbit around target
        //   shift + middle drag   pan target + position
        //   scroll                zoom (move along view dir)
        //   F                     focus target on selection
        // Selection changes also snap the target.
        let selection_changed = self.selected != self.prev_selected;
        self.prev_selected = self.selected;
        let f_now = ctx.input.is_pressed(Key::F);
        let f_just = f_now && !self.f_key_prev;
        self.f_key_prev = f_now;
        let middle_just_pressed = self.middle_button && !self.middle_button_prev;
        self.middle_button_prev = self.middle_button;
        if middle_just_pressed {
            self.prev_cursor_for_camera = self.cursor_pos;
        }
        let cursor_dx = self.cursor_pos[0] - self.prev_cursor_for_camera[0];
        let cursor_dy = self.cursor_pos[1] - self.prev_cursor_for_camera[1];
        self.prev_cursor_for_camera = self.cursor_pos;

        let cam = renderer.camera_3d_mut();

        if (selection_changed || f_just)
            && let Some(id) = self.selected
            && let Some(e) = self.scene.get(id)
            && let Some(target) = entity_world_position_3d(&self.scene, e)
        {
            let offset = cam.position - cam.target;
            cam.target = target;
            cam.position = target + offset;
        }

        if self.middle_button && (cursor_dx.abs() > 0.0 || cursor_dy.abs() > 0.0) {
            let offset = cam.position - cam.target;
            let dist = offset.length().max(0.01);
            if self.shift_held {
                let forward = (cam.target - cam.position).normalize_or_zero();
                let right = forward.cross(Vec3::Y).normalize_or_zero();
                let up_axis = right.cross(forward).normalize_or_zero();
                let pan_speed = dist * 0.0015;
                let delta = right * (-cursor_dx) * pan_speed + up_axis * cursor_dy * pan_speed;
                cam.target += delta;
                cam.position += delta;
            } else {
                let yaw = -cursor_dx * 0.005;
                let pitch = -cursor_dy * 0.005;
                let yaw_q = Quat::from_rotation_y(yaw);
                let forward = (cam.target - cam.position).normalize_or_zero();
                let right = forward.cross(Vec3::Y).normalize_or_zero();
                let pitch_q = Quat::from_axis_angle(right, pitch);
                let new_offset = pitch_q * yaw_q * offset;
                cam.position = cam.target + new_offset;
            }
        }

        if self.scroll_accum.abs() > 0.0 {
            let offset = cam.position - cam.target;
            let dist = offset.length().max(0.01);
            let factor = (1.0 - self.scroll_accum * 0.1).clamp(0.1, 10.0);
            let new_dist = (dist * factor).clamp(0.1, 1000.0);
            cam.position = cam.target + offset.normalize_or_zero() * new_dist;
            self.scroll_accum = 0.0;
        }
        renderer.set_clear_color(
            self.scene.clear_color[0] as f64,
            self.scene.clear_color[1] as f64,
            self.scene.clear_color[2] as f64,
            1.0,
        );
        let imported_meshes = &imported_meshes_snapshot;
        let scene_ref_for_draw = &self.scene;
        for entity in &self.scene.entities {
            if let Some(sprite) = &entity.sprite {
                let world_t = entity_world_transform_2d(scene_ref_for_draw, entity);
                let size = [
                    sprite.size[0] * world_t.scale[0],
                    sprite.size[1] * world_t.scale[1],
                ];
                let (uv_min, uv_max) = sprite.region.uv_rect();
                renderer.draw_sprite(SpriteInstance {
                    position: world_t.position,
                    size,
                    color: sprite.color,
                    uv_min,
                    uv_max,
                });
            }
            if entity.transform_3d.is_some()
                && let Some(mesh) = &entity.mesh
            {
                // Resolve the mesh slot: imported glTF mesh by name,
                // else the built-in unit cube. World matrix walks the
                // parent chain so children inherit their parent's
                // transform.
                let handle = imported_meshes
                    .get(&mesh.mesh)
                    .copied()
                    .unwrap_or_else(|| renderer.cube_handle());
                let world = entity_world_matrix_3d(scene_ref_for_draw, entity);
                renderer.draw_mesh_pbr(
                    handle,
                    world.to_cols_array(),
                    mesh.color,
                    mesh.metallic,
                    mesh.roughness,
                );
            }
            // Push entity's LightComponent into the renderer's per-
            // frame light queue. Uses the composed world position so
            // parented lights move with their parent.
            if let Some(light) = entity.light.as_ref()
                && let Some(pos) = entity_world_position_3d(scene_ref_for_draw, entity)
            {
                renderer.queue_point_light(
                    pos.to_array(),
                    light.color,
                    light.intensity,
                    light.range,
                );
            }
        }
        if let Some(id) = self.selected
            && let Some(entity) = self.scene.get(id)
        {
            if entity.is_3d() {
                if let Some(pos) = entity_world_position_3d(scene_ref_for_draw, entity) {
                    draw_gizmo_3d(renderer, viewport, pos);
                }
            } else {
                let p = entity.transform.position;
                if let Some(sprite) = &entity.sprite {
                    let hx = sprite.size[0] * entity.transform.scale[0] * 0.5;
                    let hy = sprite.size[1] * entity.transform.scale[1] * 0.5;
                    renderer.debug_rect(
                        [p[0] - hx, p[1] - hy],
                        [p[0] + hx, p[1] + hy],
                        [1.0, 0.9, 0.2, 1.0],
                    );
                }
                // Translate gizmo arms.
                let red = [1.0, 0.3, 0.3, 1.0];
                let green = [0.3, 1.0, 0.3, 1.0];
                let yellow = [1.0, 1.0, 0.3, 1.0];
                renderer.debug_line(p, [p[0] + GIZMO_AXIS_LEN, p[1]], red);
                renderer.debug_line(p, [p[0], p[1] + GIZMO_AXIS_LEN], green);
                let tip_x = [p[0] + GIZMO_AXIS_LEN, p[1]];
                renderer.debug_line(tip_x, [tip_x[0] - 10.0, tip_x[1] + 6.0], red);
                renderer.debug_line(tip_x, [tip_x[0] - 10.0, tip_x[1] - 6.0], red);
                let tip_y = [p[0], p[1] + GIZMO_AXIS_LEN];
                renderer.debug_line(tip_y, [tip_y[0] - 6.0, tip_y[1] - 10.0], green);
                renderer.debug_line(tip_y, [tip_y[0] + 6.0, tip_y[1] - 10.0], green);
                renderer.debug_rect(
                    [p[0] - GIZMO_HANDLE_HALF, p[1] - GIZMO_HANDLE_HALF],
                    [p[0] + GIZMO_HANDLE_HALF, p[1] + GIZMO_HANDLE_HALF],
                    yellow,
                );
            }
        }

        let render_result = if let Some(editor_ui) = self.editor_ui.as_mut() {
            renderer.render(|d, q, e, v, s| editor_ui.render(d, q, e, v, s))
        } else {
            renderer.render(|_, _, _, _, _| {})
        };
        if let Err(err) = render_result {
            warn!(error = ?err, "frame render failed");
        }
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        self.viewport = (width, height);
        if let Some(r) = self.renderer.as_mut() {
            r.resize(width, height);
        }
    }
}

/// Returns `true` if `candidate` sits anywhere in the parent chain
/// of `of`. Used to filter the parent picker so the user cannot make
/// the scene tree cyclic.
fn is_descendant_of(scene: &Scene, candidate: EntityId, of: EntityId) -> bool {
    if candidate == of {
        return false;
    }
    let mut cur = scene.get(candidate).and_then(|e| e.parent);
    while let Some(c) = cur {
        if c == of {
            return true;
        }
        cur = scene.get(c).and_then(|e| e.parent);
    }
    false
}

fn inspector_for_selection(ui: &mut egui::Ui, scene: &mut Scene, selected: &mut Option<EntityId>) {
    let Some(id) = *selected else {
        ui.label("(no entity selected)");
        return;
    };
    // Snapshot candidate parents up front so we can show the picker
    // without holding a mutable borrow over the scene.
    let candidates: Vec<(EntityId, String)> = scene
        .entities
        .iter()
        .filter(|e| e.id != id && !is_descendant_of(scene, e.id, id))
        .map(|e| (e.id, e.name.clone()))
        .collect();

    let Some(entity) = scene.get_mut(id) else {
        ui.label("(selection lost)");
        return;
    };
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut entity.name);
    });
    ui.label(format!("Id: {}", id.0));

    // Parent picker (Unity-style scene tree reparent).
    let current_parent_label = match entity.parent {
        Some(p) => candidates
            .iter()
            .find(|(c, _)| *c == p)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| format!("#{}", p.0)),
        None => "(root)".into(),
    };
    ui.horizontal(|ui| {
        ui.label("Parent");
        egui::ComboBox::from_id_salt("parent_picker")
            .selected_text(current_parent_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut entity.parent, None, "(root)");
                for (cid, cname) in &candidates {
                    ui.selectable_value(&mut entity.parent, Some(*cid), cname);
                }
            });
    });

    // Class field (free-form tag for shared blueprint membership).
    let mut class_str = entity.class.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Class");
        if ui.text_edit_singleline(&mut class_str).changed() {
            entity.class = if class_str.is_empty() {
                None
            } else {
                Some(class_str)
            };
        }
    });

    if entity.is_3d() {
        if let Some(t) = entity.transform_3d.as_mut() {
            transform_3d_editor(ui, t);
        }
        mesh_editor(ui, entity);
        physics_3d_editor(ui, entity);
    } else {
        transform_editor(ui, &mut entity.transform);
        sprite_editor(ui, entity);
    }
    scripts_editor(ui, entity, id);
    audio_editor(ui, entity);
    light_editor(ui, entity);
}

fn light_editor(ui: &mut egui::Ui, entity: &mut Entity) {
    ui.separator();
    let Some(light) = entity.light.as_mut() else {
        if ui.button("Add point light").clicked() {
            entity.light = Some(infengine_core::scene::LightComponent::point(
                [1.0, 0.95, 0.85],
                3.0,
                10.0,
            ));
        }
        return;
    };
    ui.label("Point light");
    ui.horizontal(|ui| {
        ui.label("Color");
        ui.color_edit_button_rgb(&mut light.color);
    });
    ui.add(egui::Slider::new(&mut light.intensity, 0.0..=20.0).text("Intensity"));
    ui.add(egui::Slider::new(&mut light.range, 0.1..=100.0).text("Range"));
    if ui.button("Remove light").clicked() {
        entity.light = None;
    }
}

fn audio_editor(ui: &mut egui::Ui, entity: &mut Entity) {
    ui.separator();
    let Some(audio) = entity.audio.as_mut() else {
        if ui.button("Add audio component").clicked() {
            entity.audio = Some(AudioComponent::new("assets/sound.wav"));
        }
        return;
    };
    ui.label("Audio");
    ui.horizontal(|ui| {
        ui.label("Path");
        ui.text_edit_singleline(&mut audio.sound_path);
    });
    ui.add(egui::Slider::new(&mut audio.volume, 0.0..=1.0).text("Volume"));
    ui.checkbox(&mut audio.autoplay, "Autoplay on Play");
    ui.checkbox(&mut audio.looping, "Loop (not yet honoured)");
    if ui.button("Remove audio component").clicked() {
        entity.audio = None;
    }
}

fn scripts_editor(ui: &mut egui::Ui, entity: &mut Entity, eid: EntityId) {
    ui.separator();
    ui.label("Scripts");
    let mut remove: Option<usize> = None;
    for (i, path) in entity.blueprints.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.small(format!("[{i}]"));
            ui.text_edit_singleline(path);
            if ui.small_button("X").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        entity.blueprints.remove(i);
    }
    if ui.button("Add script").clicked() {
        let idx = entity.blueprints.len();
        entity
            .blueprints
            .push(format!("blueprints/entity_{}_script_{idx}.ron", eid.0));
    }
}

fn transform_3d_editor(ui: &mut egui::Ui, t: &mut Transform3D) {
    ui.label("Position");
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut t.position[0])
                .prefix("X: ")
                .speed(0.05),
        );
        ui.add(
            egui::DragValue::new(&mut t.position[1])
                .prefix("Y: ")
                .speed(0.05),
        );
        ui.add(
            egui::DragValue::new(&mut t.position[2])
                .prefix("Z: ")
                .speed(0.05),
        );
    });
    ui.label("Rotation (rad)");
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut t.rotation[0])
                .prefix("X: ")
                .speed(0.02),
        );
        ui.add(
            egui::DragValue::new(&mut t.rotation[1])
                .prefix("Y: ")
                .speed(0.02),
        );
        ui.add(
            egui::DragValue::new(&mut t.rotation[2])
                .prefix("Z: ")
                .speed(0.02),
        );
    });
    ui.label("Scale");
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut t.scale[0])
                .prefix("X: ")
                .speed(0.05)
                .range(0.01..=100.0),
        );
        ui.add(
            egui::DragValue::new(&mut t.scale[1])
                .prefix("Y: ")
                .speed(0.05)
                .range(0.01..=100.0),
        );
        ui.add(
            egui::DragValue::new(&mut t.scale[2])
                .prefix("Z: ")
                .speed(0.05)
                .range(0.01..=100.0),
        );
    });
}

fn mesh_editor(ui: &mut egui::Ui, entity: &mut Entity) {
    let Some(mesh) = entity.mesh.as_mut() else {
        if ui.button("Add mesh component").clicked() {
            entity.mesh = Some(MeshComponent::cube([0.8, 0.8, 0.85, 1.0]));
        }
        return;
    };
    ui.horizontal(|ui| {
        ui.label("Mesh");
        ui.text_edit_singleline(&mut mesh.mesh);
    });
    ui.horizontal(|ui| {
        ui.label("Albedo");
        ui.color_edit_button_rgba_unmultiplied(&mut mesh.color);
    });
    ui.add(egui::Slider::new(&mut mesh.metallic, 0.0..=1.0).text("Metallic"));
    ui.add(egui::Slider::new(&mut mesh.roughness, 0.04..=1.0).text("Roughness"));
    let mut texture_str = mesh.texture.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Texture");
        if ui.text_edit_singleline(&mut texture_str).changed() {
            mesh.texture = if texture_str.is_empty() {
                None
            } else {
                Some(texture_str)
            };
        }
    });
}

fn physics_3d_editor(ui: &mut egui::Ui, entity: &mut Entity) {
    ui.separator();
    let Some(p) = entity.physics_3d.as_mut() else {
        ui.horizontal(|ui| {
            if ui.button("Add dynamic 3D body").clicked() {
                entity.physics_3d = Some(Physics3DComponent::dynamic_box());
            }
            if ui.button("Add static 3D body").clicked() {
                entity.physics_3d = Some(Physics3DComponent::static_box());
            }
        });
        return;
    };
    ui.label("3D physics");
    egui::ComboBox::from_id_salt("body_3d_kind")
        .selected_text(match p.body {
            BodyKind3D::Dynamic => "Dynamic",
            BodyKind3D::Static => "Static",
            BodyKind3D::Kinematic => "Kinematic",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut p.body, BodyKind3D::Dynamic, "Dynamic");
            ui.selectable_value(&mut p.body, BodyKind3D::Static, "Static");
            ui.selectable_value(&mut p.body, BodyKind3D::Kinematic, "Kinematic");
        });
    match &mut p.shape {
        ColliderShape3D::Box { half_extents } => {
            ui.label("Box half-extents");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut half_extents[0])
                        .prefix("X: ")
                        .speed(0.05),
                );
                ui.add(
                    egui::DragValue::new(&mut half_extents[1])
                        .prefix("Y: ")
                        .speed(0.05),
                );
                ui.add(
                    egui::DragValue::new(&mut half_extents[2])
                        .prefix("Z: ")
                        .speed(0.05),
                );
            });
        }
        ColliderShape3D::Sphere { radius } => {
            ui.add(egui::Slider::new(radius, 0.05..=10.0).text("Sphere radius"));
        }
        ColliderShape3D::Capsule {
            half_height,
            radius,
        } => {
            ui.add(egui::Slider::new(half_height, 0.05..=10.0).text("Capsule half-height"));
            ui.add(egui::Slider::new(radius, 0.05..=10.0).text("Capsule radius"));
        }
    }
    ui.add(egui::Slider::new(&mut p.friction, 0.0..=2.0).text("Friction"));
    ui.add(egui::Slider::new(&mut p.restitution, 0.0..=1.0).text("Restitution"));
    ui.add(egui::Slider::new(&mut p.linear_damping, 0.0..=5.0).text("Linear damping"));
    ui.add(egui::Slider::new(&mut p.angular_damping, 0.0..=5.0).text("Angular damping"));
    if ui.button("Remove 3D body").clicked() {
        entity.physics_3d = None;
    }
}

fn transform_editor(ui: &mut egui::Ui, t: &mut Transform2D) {
    ui.add(
        egui::DragValue::new(&mut t.position[0])
            .prefix("X: ")
            .speed(1.0),
    );
    ui.add(
        egui::DragValue::new(&mut t.position[1])
            .prefix("Y: ")
            .speed(1.0),
    );
    ui.add(egui::Slider::new(&mut t.scale[0], 0.25..=4.0).text("Scale X"));
    ui.add(egui::Slider::new(&mut t.scale[1], 0.25..=4.0).text("Scale Y"));
}

fn sprite_editor(ui: &mut egui::Ui, entity: &mut Entity) {
    let has_sprite = entity.sprite.is_some();
    let mut keep = has_sprite;
    if has_sprite {
        ui.checkbox(&mut keep, "Sprite component");
    } else if ui.button("Add sprite component").clicked() {
        entity.sprite = Some(SpriteComponent::new([64.0, 64.0], AtlasRegion::Checker));
        keep = true;
    }
    if has_sprite && !keep {
        entity.sprite = None;
        return;
    }
    let Some(sprite) = entity.sprite.as_mut() else {
        return;
    };
    ui.add(egui::Slider::new(&mut sprite.size[0], 8.0..=512.0).text("Width"));
    ui.add(egui::Slider::new(&mut sprite.size[1], 8.0..=512.0).text("Height"));
    ui.horizontal(|ui| {
        ui.label("Color");
        ui.color_edit_button_rgba_unmultiplied(&mut sprite.color);
    });
    egui::ComboBox::from_label("Region")
        .selected_text(region_label(&sprite.region))
        .show_ui(ui, |ui| {
            for r in [
                AtlasRegion::Checker,
                AtlasRegion::Dots,
                AtlasRegion::Stripes,
                AtlasRegion::Cross,
                AtlasRegion::Full,
            ] {
                let label = region_label(&r);
                let is_selected =
                    std::mem::discriminant(&sprite.region) == std::mem::discriminant(&r);
                if ui.selectable_label(is_selected, label).clicked() {
                    sprite.region = r;
                }
            }
        });
}

/// Best-effort workspace-root lookup. We walk upwards from the
/// running editor binary looking for a `Cargo.toml` that contains a
/// `[workspace]` table. Falls back to `.` so the cargo invocation
/// still runs (and fails informatively) when launched outside the
/// dev tree.
fn workspace_root() -> PathBuf {
    let mut cur = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        let candidate = cur.join("Cargo.toml");
        if candidate.exists()
            && let Ok(text) = std::fs::read_to_string(&candidate)
            && text.contains("[workspace]")
        {
            return cur;
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => return PathBuf::from("."),
        }
    }
}

fn load_or_create_project(path: &Path) -> Result<Project, ProjectError> {
    if path.exists() {
        Project::load(path)
    } else {
        let project = Project {
            name: "Untitled".to_owned(),
            asset_root: PathBuf::from("assets"),
            scenes: Vec::new(),
            classes: Vec::new(),
        };
        project.save(path)?;
        info!(path = %path.display(), "created blank project");
        Ok(project)
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let arg = std::env::args().nth(1);
    let project_file = PathBuf::from(arg.unwrap_or_else(|| DEFAULT_PROJECT_FILE.to_string()));
    let project_root = project_file
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let project = match load_or_create_project(&project_file) {
        Ok(p) => p,
        Err(err) => {
            error!(error = %err, "failed to open project");
            std::process::exit(1);
        }
    };
    info!(
        project = %project.name,
        scenes = project.scenes.len(),
        file = %project_file.display(),
        "project loaded"
    );

    let editor = Editor::new(project_root, project_file, project);
    let config = WindowConfig {
        title: format!("InfEngine Editor - {}", editor.project.name),
        width: 1440,
        height: 900,
        resizable: true,
    };
    if let Err(err) = run(config, editor) {
        error!(error = %err, "event loop failed");
        std::process::exit(1);
    }
}
