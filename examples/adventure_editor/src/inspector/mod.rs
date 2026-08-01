//! A41.1: the inspector, organised by what you are doing.
//!
//! It used to be one column with nine headings stacked down it - tool,
//! overlays, room, layers, animations, sheets, lines, selection,
//! problems - all present at once whatever you were working on. Nothing
//! said which parts went together, so the editor read as a list of
//! fields rather than as a tool. The "Draw floor" button and the
//! "Walkable floor" checkbox are two halves of one job and sat five
//! sections apart.
//!
//! So the panel is split by job instead. Each [`Task`] answers one
//! question about the room, and choosing it does the setting-up: it
//! turns on the overlay that job needs and picks up the tool that job
//! uses. That is the whole point - the connection between a tool and
//! what it draws on is made by the editor rather than left to be
//! learned.
//!
//! An empty tab says what to do about being empty. "This room has none"
//! is a fact; "no floor yet - draw one with Outline, or the actor cannot
//! walk here" is an instruction, and the difference is most of what
//! makes a panel usable the first time.

pub mod floor;
pub mod motion;
pub mod room;
pub mod talk;
pub mod things;

use crate::draw::Overlays;
use crate::edit::Tool;
use crate::ui::{UiActions, UiState};

/// One job you can be doing in a room.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Task {
    /// What this screen is.
    #[default]
    Room,
    /// Where the actor may stand.
    Floor,
    /// What the player can click.
    Things,
    /// What moves.
    Motion,
    /// What is said.
    Talk,
}

impl Task {
    pub const ALL: [Task; 5] = [
        Task::Room,
        Task::Floor,
        Task::Things,
        Task::Motion,
        Task::Talk,
    ];

    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Task::Room => "Room",
            Task::Floor => "Floor",
            Task::Things => "Things",
            Task::Motion => "Motion",
            Task::Talk => "Talk",
        }
    }

    /// The question this tab answers, shown under the tabs.
    ///
    /// Phrased as what the job is rather than what the fields are: the
    /// fields are visible already, and what is not visible is why they
    /// are together.
    pub fn question(self) -> &'static str {
        match self {
            Task::Room => "What this screen is - its name, its size, the pictures it is painted from.",
            Task::Floor => "Where the actor may stand, and where they arrive from each door.",
            Task::Things => "What the player can click - the scenery, and the ways out.",
            Task::Motion => "What moves here, and the sheets the frames are cut from.",
            Task::Talk => "What is said here, and how much of it has been recorded.",
        }
    }

    /// The tools this job uses. The first is picked when the tab opens.
    ///
    /// A tab with no tools of its own leaves the current one alone: the
    /// dialogue list does not care what the mouse is doing.
    pub fn tools(self) -> &'static [Tool] {
        match self {
            Task::Room => &[],
            Task::Floor => &[Tool::DrawFloor, Tool::Select, Tool::WalkPreview],
            Task::Things => &[Tool::DrawHotspot, Tool::Select],
            Task::Motion | Task::Talk => &[],
        }
    }

    /// Turn on the overlays this job needs to see.
    ///
    /// Only applied when the tab changes, so a deliberate toggle
    /// afterwards is not undone on the next frame.
    pub fn reveal(self, overlays: &mut Overlays) {
        match self {
            Task::Room => overlays.bounds = true,
            Task::Floor => {
                overlays.floor = true;
                overlays.entries = true;
            }
            Task::Things => {
                overlays.hotspots = true;
                overlays.exits = true;
            }
            Task::Motion | Task::Talk => {}
        }
    }

    /// How many things this tab has to show, for the tab label.
    ///
    /// A count on the tab means you can tell an empty room from a full
    /// one without opening all five.
    pub fn count(self, state: &UiState<'_>) -> usize {
        match self {
            Task::Room => state.room.layers.len(),
            Task::Floor => state.room.walk.solids.len() + state.room.entries.len(),
            Task::Things => state.room.hotspots.len() + state.room.exits.len(),
            Task::Motion => state.animations.len(),
            Task::Talk => state.lines.len(),
        }
    }
}

/// Draw the right-hand panel. Returns where its left edge ended up.
pub fn panel(ctx: &egui::Context, state: &mut UiState<'_>, actions: &mut UiActions) -> f32 {
    let response = egui::SidePanel::right("inspector")
        .default_width(340.0)
        .show(ctx, |ui| {
            toolbar(ui, state, actions);
            ui.separator();
            tabs(ui, state);
            ui.label(
                egui::RichText::new(state.task.question())
                    .small()
                    .weak(),
            );
            ui.separator();
            tools(ui, state);

            egui::ScrollArea::vertical()
                .id_salt("inspector_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match *state.task {
                        Task::Room => room::show(ui, state),
                        Task::Floor => floor::show(ui, state),
                        Task::Things => things::show(ui, state, actions),
                        Task::Motion => motion::show(ui, state, actions),
                        Task::Talk => talk::show(ui, state, actions),
                    }
                    ui.add_space(12.0);
                    problems(ui, state);
                });
        });
    response.response.rect.left()
}

fn toolbar(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    ui.horizontal(|ui| {
        let save = egui::Button::new(if state.dirty { "Save *" } else { "Save" });
        if ui
            .add_enabled(state.dirty, save)
            .on_hover_text("Write this room back to its file")
            .clicked()
        {
            actions.save = true;
        }
        if ui
            .button("Reload")
            .on_hover_text("Throw away unsaved changes and read the file again")
            .clicked()
        {
            actions.reload = true;
        }
        if ui
            .button("Fit")
            .on_hover_text("Frame the whole room in the view")
            .clicked()
        {
            actions.fit_view = true;
        }
    });
}

/// The tab strip. Switching tabs is what sets up the tool and overlays.
fn tabs(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    ui.horizontal_wrapped(|ui| {
        for task in Task::ALL {
            let count = task.count(state);
            let label = if count > 0 {
                format!("{} {count}", task.label())
            } else {
                task.label().to_string()
            };
            let selected = *state.task == task;
            if ui
                .selectable_label(selected, label)
                .on_hover_text(task.question())
                .clicked()
                && !selected
            {
                *state.task = task;
                task.reveal(state.overlays);
                if let Some(first) = task.tools().first() {
                    *state.tool = *first;
                }
            }
        }
    });
}

/// Only the tools this job uses, with what they do written next to them.
fn tools(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    let available = state.task.tools();
    if available.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for tool in available {
            if ui
                .selectable_label(*state.tool == *tool, tool_label(*tool))
                .clicked()
            {
                *state.tool = *tool;
            }
        }
    });
    // The hint describes the tool actually in force, which may be one
    // this tab does not offer if another tab set it.
    ui.label(egui::RichText::new(tool_hint(*state.tool)).small().weak());
    ui.separator();
}

/// Tool names as a job describes them rather than as the enum spells
/// them: you draw an outline, you do not "draw floor".
pub fn tool_label(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "Adjust",
        Tool::DrawFloor => "Outline",
        Tool::DrawHotspot => "Box out",
        Tool::WalkPreview => "Test walk",
    }
}

pub fn tool_hint(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "Drag a corner or a box. Right-click an edge to add a corner.",
        Tool::DrawFloor => "Click to place corners. Right-click to close the outline.",
        Tool::DrawHotspot => "Drag a rectangle over the thing the player should be able to click.",
        Tool::WalkPreview => "Click where the actor should go; the route is drawn to there.",
    }
}

/// Shown at the foot of every tab, because a problem you cannot see is
/// worse than one in the wrong place.
fn problems(ui: &mut egui::Ui, state: &UiState<'_>) {
    let issues = state.room.validate();
    if issues.is_empty() && state.missing_images.is_empty() {
        return;
    }
    ui.separator();
    ui.heading("Problems");
    for image in state.missing_images {
        ui.label(
            egui::RichText::new(format!("image not found: {image}"))
                .color(egui::Color32::from_rgb(255, 140, 120)),
        );
    }
    for issue in issues {
        ui.label(egui::RichText::new(issue).color(egui::Color32::from_rgb(255, 200, 120)));
    }
}

/// What a tab shows when it has nothing: what that means, and what to do.
pub fn nothing_yet(ui: &mut egui::Ui, meaning: &str, next: &str) {
    ui.label(egui::RichText::new(meaning).weak());
    ui.label(egui::RichText::new(next).small().weak());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_task_says_what_it_is_for() {
        for task in Task::ALL {
            assert!(!task.label().is_empty(), "{task:?} has no label");
            assert!(
                task.question().len() > 20,
                "{task:?} needs a real sentence, not a word"
            );
        }
    }

    #[test]
    fn every_tool_has_a_name_and_a_hint() {
        for tool in Tool::ALL {
            assert!(!tool_label(tool).is_empty(), "{tool:?} has no name");
            assert!(!tool_hint(tool).is_empty(), "{tool:?} has no hint");
        }
    }

    #[test]
    fn a_tab_that_offers_a_tool_offers_it_first_by_default() {
        // Opening Floor with Select in force would leave the user
        // clicking on the picture and nothing happening.
        assert_eq!(Task::Floor.tools().first(), Some(&Tool::DrawFloor));
        assert_eq!(Task::Things.tools().first(), Some(&Tool::DrawHotspot));
    }

    #[test]
    fn opening_a_tab_reveals_what_that_job_needs_to_see() {
        let mut overlays = Overlays {
            floor: false,
            hotspots: false,
            exits: false,
            entries: false,
            bounds: false,
        };
        Task::Floor.reveal(&mut overlays);
        assert!(overlays.floor && overlays.entries);
        assert!(!overlays.hotspots, "floor work does not need hotspots on");

        Task::Things.reveal(&mut overlays);
        assert!(overlays.hotspots && overlays.exits);
    }

    #[test]
    fn revealing_never_turns_an_overlay_off() {
        // Switching tabs must not undo a deliberate toggle, so reveal
        // only ever sets.
        let full = Overlays::default();
        for task in Task::ALL {
            let mut overlays = full;
            task.reveal(&mut overlays);
            assert_eq!(overlays, full, "{task:?} turned something off");
        }
    }

    #[test]
    fn every_tool_belongs_to_some_tab() {
        // A tool no tab offers is a tool nobody can reach.
        for tool in Tool::ALL {
            assert!(
                Task::ALL.iter().any(|t| t.tools().contains(&tool)),
                "{tool:?} is offered by no tab"
            );
        }
    }
}
