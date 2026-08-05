//! A25.2: editing a script as a node graph.
//!
//! Hosts the engine's blueprint canvas with the adventure node
//! catalogue registered, so an imported Kyrandia script opens as a
//! graph, gets rewired, and goes back out as our own script file.
//!
//! The conversion happens at the edges - open converts, save converts
//! back - rather than the canvas editing actions directly. That keeps
//! the graph free to hold a half-finished edit that is not yet a valid
//! script, which is what editing is.

use infengine_adventure::action::Script;
use infengine_blueprint::{Blueprint, BlueprintEditor, NodeKindRegistry};

/// The script canvas's state.
pub struct GraphUi {
    /// Script currently on the canvas.
    pub script_id: Option<String>,
    pub editor: BlueprintEditor,
    pub registry: NodeKindRegistry,
    pub dirty: bool,
    pub status: String,
}

impl std::fmt::Debug for GraphUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphUi")
            .field("script", &self.script_id)
            .finish()
    }
}

impl Default for GraphUi {
    fn default() -> Self {
        Self {
            script_id: None,
            editor: BlueprintEditor::new(),
            registry: infengine_advgraph::registry(),
            dirty: false,
            status: String::new(),
        }
    }
}

impl GraphUi {
    /// Put a script on the canvas.
    pub fn load(&mut self, script: &Script) {
        let blueprint = Blueprint {
            graph: infengine_advgraph::to_graph(script),
            ..Default::default()
        };
        self.editor = BlueprintEditor::with_blueprint(blueprint);
        self.script_id = Some(script.id.clone());
        self.dirty = false;
        self.status = format!("{} nodes", self.editor.blueprint.graph.nodes.len());
    }

    /// The script the canvas currently describes.
    pub fn to_script(&self) -> Option<Script> {
        let id = self.script_id.clone()?;
        let mut script = Script::new(id);
        script.actions = infengine_advgraph::from_graph(&self.editor.blueprint.graph);
        Some(script)
    }
}

#[derive(Debug, Default)]
pub struct GraphActions {
    /// A script the user asked to open on the canvas.
    pub open_script: Option<String>,
    pub save: bool,
    /// Run what is on the canvas, without saving it.
    pub run: bool,
    /// An animation a node names, to open on the workbench.
    pub edit_animation: Option<String>,
}

/// What a node's params may be, so the graph offers pickers rather
/// than blank boxes.
#[derive(Debug, Clone, Copy, Default)]
pub struct GraphOptions<'a> {
    pub animations: &'a [String],
    pub sounds: &'a [String],
    pub rooms: &'a [String],
    pub images: &'a [String],
}

/// The script canvas, filling the centre of the window in Script mode.
///
/// A floating window once; now the mode owns the middle of the screen,
/// the way the graph in the design does. Must be added after every side
/// and bottom panel, because a central panel takes whatever is left.
pub fn panel(
    ctx: &egui::Context,
    state: &mut GraphUi,
    scripts: &[String],
    options: GraphOptions<'_>,
) -> GraphActions {
    let mut actions = GraphActions::default();

    // What every param may be, refreshed each frame: a project gains
    // animations while the graph is open, and a picker listing what
    // the project held a minute ago is a picker that lies.
    state.editor.param_options.clear();
    for (key, values) in [
        ("animation", options.animations),
        ("sound", options.sounds),
        ("music", options.sounds),
        ("room", options.rooms),
        ("image", options.images),
    ] {
        if !values.is_empty() {
            state
                .editor
                .param_options
                .insert(key.to_string(), values.to_vec());
        }
    }
    // The one param that names something with a workbench of its own.
    state.editor.param_editable.insert("animation".to_string());

    egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("graph_script")
                    .selected_text(
                        state
                            .script_id
                            .clone()
                            .unwrap_or_else(|| "pick a script".into()),
                    )
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for id in scripts {
                            let picked = state.script_id.as_deref() == Some(id.as_str());
                            if ui.selectable_label(picked, id).clicked() {
                                actions.open_script = Some(id.clone());
                            }
                        }
                    });

                if ui
                    .add_enabled(state.script_id.is_some(), egui::Button::new("save"))
                    .clicked()
                {
                    actions.save = true;
                }
                if ui
                    .add_enabled(state.script_id.is_some(), egui::Button::new("run"))
                    .on_hover_text("Run what is on the canvas, without saving")
                    .clicked()
                {
                    actions.run = true;
                }

                // The count of untranslated steps, live. Rewiring a
                // graph is how they get fixed, so it belongs where the
                // rewiring happens.
                if let Some(script) = state.to_script() {
                    let unknown = script.unknown_count();
                    let text = format!("{} steps, {unknown} untranslated", script.actions.len());
                    ui.label(if unknown == 0 {
                        egui::RichText::new(text).small().weak()
                    } else {
                        egui::RichText::new(text)
                            .small()
                            .color(egui::Color32::from_rgb(230, 150, 60))
                    });
                }
            });

            if !state.status.is_empty() {
                ui.label(egui::RichText::new(&state.status).small().weak());
            }
            ui.separator();

            if state.script_id.is_none() {
                ui.label(
                    egui::RichText::new(
                        "Pick a script. Imported ones open as a graph you can rewire.",
                    )
                    .weak(),
                );
                return;
            }
            state.dirty |= state.editor.show(ui, &state.registry);
        });

    if let Some((_, id)) = state.editor.take_open_asset() {
        actions.edit_animation = Some(id);
    }
    actions
}
