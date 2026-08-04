//! A83: the inspector, driven by the mode.
//!
//! The right-hand column answers a different question in each mode:
//!
//! * **Room** - what this screen is: identity, pictures, floor, the
//!   things to click, the shot. Everything about the room in one
//!   scrollable column, sections with counts, problems at the foot.
//! * **Animate** - what moves and what is said: animations, the sheet
//!   cutter, the room's lines.
//! * **Script** - the room's scripts and the one on the canvas spelled
//!   out as readable steps.
//! * **Test** - the tester's controls: verb, flags, inventory, saves,
//!   and the log of what the scripts actually did.
//!
//! An empty section says what to do about being empty. "This room has
//! none" is a fact; "no floor yet - draw one with Outline" is an
//! instruction, and the difference is most of what makes a panel usable
//! the first time.

pub mod camera;
pub mod floor;
pub mod motion;
pub mod room;
pub mod scripts;
pub mod talk;
pub mod things;

use crate::edit::Tool;
use crate::mode::Mode;
use crate::ui::{UiActions, UiState};

/// State the inspector needs that lives outside [`UiState`]: the game
/// being tested and the script on the graph canvas.
pub struct Extras<'a> {
    pub play: &'a mut crate::play::PlayMode,
    pub play_draft: &'a mut crate::ui_play::PlayDraft,
    pub slots: &'a [infengine_adventure::save::SlotInfo],
    /// The open room's id, cloned so the panel can read it while the
    /// room itself is borrowed elsewhere.
    pub room_id: String,
    /// The script on the graph canvas, if any.
    pub on_canvas: Option<String>,
}

/// Draw the right-hand panel. Returns what the Test controls asked for.
pub fn panel(
    ctx: &egui::Context,
    state: &mut UiState<'_>,
    actions: &mut UiActions,
    extras: &mut Extras<'_>,
) -> crate::ui_play::PlayActions {
    let mut play_actions = crate::ui_play::PlayActions::default();
    egui::SidePanel::right("inspector")
        .default_width(340.0)
        .show(ctx, |ui| {
            header(ui, state, extras);
            ui.separator();
            match state.mode {
                Mode::Room => room_mode(ui, state, actions),
                Mode::Animate => animate_mode(ui, state, actions),
                Mode::Script => scripts::show(ui, state, actions, extras.on_canvas.as_deref()),
                Mode::Test => {
                    egui::ScrollArea::vertical()
                        .id_salt("inspector_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            play_actions = crate::ui_play::contents(
                                ui,
                                extras.play,
                                extras.play_draft,
                                &extras.room_id,
                                extras.slots,
                            );
                        });
                }
            }
        });
    play_actions
}

/// What the column is about right now: the mode, the thing, one fact.
fn header(ui: &mut egui::Ui, state: &UiState<'_>, extras: &Extras<'_>) {
    let (title, sub) = match state.mode {
        Mode::Room => (
            state.room.id.clone(),
            format!(
                "{} x {} · {} picture{}",
                state.room.size.x as i32,
                state.room.size.y as i32,
                state.room.layers.len(),
                plural(state.room.layers.len()),
            ),
        ),
        Mode::Animate => (
            state.room.id.clone(),
            format!(
                "{} animation{} · {} line{}",
                state.animations.len(),
                plural(state.animations.len()),
                state.lines.len(),
                plural(state.lines.len()),
            ),
        ),
        Mode::Script => (
            extras
                .on_canvas
                .clone()
                .unwrap_or_else(|| "nothing on the canvas".into()),
            "boxes and wires in the centre, the same steps as text below".into(),
        ),
        Mode::Test => (
            format!("running {}", extras.room_id),
            "flags and inventory are kept between runs".into(),
        ),
    };
    ui.label(
        egui::RichText::new(state.mode.label().to_uppercase())
            .small()
            .weak(),
    );
    ui.heading(title);
    ui.label(egui::RichText::new(sub).small().weak());
}

/// Everything about the room, in sections with counts.
fn room_mode(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    egui::ScrollArea::vertical()
        .id_salt("inspector_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // The selection first, always visible: clicking a thing on
            // the picture is the way this column is usually reached.
            things::selected(ui, state, actions);

            section(ui, "Room", state.room.layers.len(), true, |ui| {
                room::show(ui, state)
            });
            let floors = state.room.walk.solids.len() + state.room.entries.len();
            section(ui, "Floor", floors, true, |ui| floor::show(ui, state));
            let clickable = state.room.hotspots.len() + state.room.exits.len();
            section(ui, "Things", clickable, true, |ui| things::show(ui, state));
            section(ui, "Camera", 0, false, |ui| camera::show(ui, state));

            ui.add_space(12.0);
            problems(ui, state);
        });
}

/// What moves and what is said.
fn animate_mode(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    egui::ScrollArea::vertical()
        .id_salt("inspector_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            motion::show(ui, state, actions);
            ui.add_space(10.0);
            section(ui, "Talk", state.lines.len(), false, |ui| {
                talk::show(ui, state, actions)
            });
            ui.add_space(12.0);
            problems(ui, state);
        });
}

/// One collapsible section, its count on the header.
fn section(
    ui: &mut egui::Ui,
    title: &str,
    count: usize,
    open: bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    let label = if count > 0 {
        format!("{title} · {count}")
    } else {
        title.to_string()
    };
    egui::CollapsingHeader::new(label)
        .id_salt(("inspector_section", title))
        .default_open(open)
        .show(ui, body);
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
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

/// Shown at the foot of Room and Animate, because a problem you cannot
/// see is worse than one in the wrong place.
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

/// What a section shows when it has nothing: what that means, and what
/// to do.
pub fn nothing_yet(ui: &mut egui::Ui, meaning: &str, next: &str) {
    ui.label(egui::RichText::new(meaning).weak());
    ui.label(egui::RichText::new(next).small().weak());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_name_and_a_hint() {
        for tool in Tool::ALL {
            assert!(!tool_label(tool).is_empty(), "{tool:?} has no name");
            assert!(!tool_hint(tool).is_empty(), "{tool:?} has no hint");
        }
    }

    #[test]
    fn plurals_read_naturally() {
        assert_eq!(plural(1), "");
        assert_eq!(plural(0), "s");
        assert_eq!(plural(2), "s");
    }
}
