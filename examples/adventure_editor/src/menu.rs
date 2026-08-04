//! A42.1: the menu bar, which is where the editor says what it contains.
//!
//! Before this there was no top-level anything. The game settings, the
//! screen designer and the script graph were reached by three buttons
//! labelled `game...`, `screens...` and `graph...`, sitting inside the
//! play panel next to the verb picker - so the only way to find out the
//! editor had a screen designer was to click a button that did not
//! mention screens, in a panel about testing.
//!
//! A menu bar is not decoration here. It is the one place that lists the
//! editor's parts by name, and the place a reader looks first when they
//! do not know what a program can do. Everything reachable is named in
//! it, including the things that also have a button elsewhere.
//!
//! Two rules it keeps:
//!
//! * nothing is offered that does not exist. An item for a feature that
//!   is not built teaches the menu cannot be trusted.
//! * an item that cannot act right now is disabled with a reason on
//!   hover, not hidden. Disappearing items make a menu unlearnable.

use crate::draw::Overlays;
use crate::inspector::Task;

/// What the menu bar asks the app to do.
///
/// Separate from the panels' own actions: a menu item and a button that
/// do the same thing should arrive at the same code, but the menu can
/// also ask for things no panel offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuActions {
    pub save: bool,
    pub reload: bool,
    pub fit_view: bool,
    /// Add something to the room, placed in the middle of the view.
    pub add: Option<Addable>,
    pub open_game: bool,
    pub open_screens: bool,
    pub open_graph: bool,
    pub open_story: bool,
    pub open_library: bool,
    /// Take the last canvas edit back.
    pub undo: bool,
    pub open_help: bool,
    /// Switch the inspector to a job.
    pub task: Option<Task>,
    pub start_play: bool,
    pub stop_play: bool,
}

/// Things the Room menu can add. Each lands in the middle of the room so
/// it is on screen and can be dragged from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addable {
    Hotspot,
    Exit,
    Entry,
}

impl Addable {
    pub fn label(self) -> &'static str {
        match self {
            Addable::Hotspot => "Clickable thing",
            Addable::Exit => "Way out",
            Addable::Entry => "Arrival point",
        }
    }

    /// What it is for, on hover. Adding something you cannot name is how
    /// a room fills up with `hotspot_4`.
    pub fn hint(self) -> &'static str {
        match self {
            Addable::Hotspot => {
                "A region the player can click - scenery to look at, an object to take."
            }
            Addable::Exit => "A region that sends the player to another room.",
            Addable::Entry => "Where the actor stands when a door leads into this room.",
        }
    }

    /// The tab that edits it, opened along with the thing itself.
    pub fn task(self) -> Task {
        match self {
            Addable::Hotspot | Addable::Exit => Task::Things,
            Addable::Entry => Task::Floor,
        }
    }
}

/// How big the interface is drawn, as a multiple of its natural size.
///
/// A 4K monitor renders this at a size that is legible in principle and
/// unreadable in practice, and the fix is not per-widget font sizes but
/// one number the whole interface is scaled by.
///
/// Bounded rather than free: below 0.6 the tab strip stops being
/// clickable, and above 2.5 the inspector is wider than the picture it
/// is inspecting. Both are states a user cannot get back out of, since
/// the control that would undo them has scaled too.
pub const MIN_UI_SCALE: f32 = 0.6;
pub const MAX_UI_SCALE: f32 = 2.5;
/// What one press of the shortcut changes it by.
pub const UI_SCALE_STEP: f32 = 0.1;

/// Clamp a scale into the range the interface stays usable at.
pub fn clamp_ui_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE)
    } else {
        1.0
    }
}

/// State the bar keeps: whether a room is open, whether it is dirty.
#[derive(Debug, Clone, Copy)]
pub struct MenuContext {
    pub has_room: bool,
    pub dirty: bool,
    pub playing: bool,
}

/// Draw the bar. Returns what was asked for.
pub fn bar(
    ctx: &egui::Context,
    menu: MenuContext,
    overlays: &mut Overlays,
    task: Task,
    ui_scale: &mut f32,
) -> MenuActions {
    let mut actions = MenuActions::default();
    scale_shortcuts(ctx, ui_scale);
    // The undo chord, read here because this is the one place with the
    // frame's input and the actions in the same hands.
    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
        actions.undo = true;
    }
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            project_menu(ui, menu, &mut actions);
            room_menu(ui, menu, &mut actions);
            view_menu(ui, menu, overlays, task, ui_scale, &mut actions);
            help_menu(ui, &mut actions);

            // The play control sits at the right where a transport
            // usually is, away from the menus so it is not read as one.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if menu.playing {
                    if ui.button("Stop").clicked() {
                        actions.stop_play = true;
                    }
                } else if ui
                    .add_enabled(menu.has_room, egui::Button::new("Play"))
                    .on_hover_text("Run the game from this room")
                    .on_disabled_hover_text("Open a room first")
                    .clicked()
                {
                    actions.start_play = true;
                }
                if ui
                    .button("?")
                    .on_hover_text("How this editor works")
                    .clicked()
                {
                    actions.open_help = true;
                }
                if menu.dirty {
                    ui.label(
                        egui::RichText::new("unsaved")
                            .small()
                            .color(egui::Color32::from_rgb(255, 200, 120)),
                    );
                }
            });
        });
    });
    actions
}

/// Ctrl+, Ctrl- and Ctrl-0, done here rather than left to egui.
///
/// egui has its own keyboard zoom, but it writes the zoom factor
/// directly, and this editor writes it every frame from `ui_scale` -
/// so egui's would be undone before it was ever drawn. Turning its
/// version off and driving the same number from one place keeps the
/// slider and the keys in agreement.
fn scale_shortcuts(ctx: &egui::Context, ui_scale: &mut f32) {
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    ctx.input_mut(|input| {
        let cmd = egui::Modifiers::COMMAND;
        // Plus is Shift+Equals on most layouts, so both count.
        if input.consume_key(cmd, egui::Key::Plus)
            || input.consume_key(cmd, egui::Key::Equals)
        {
            *ui_scale = clamp_ui_scale(*ui_scale + UI_SCALE_STEP);
        }
        if input.consume_key(cmd, egui::Key::Minus) {
            *ui_scale = clamp_ui_scale(*ui_scale - UI_SCALE_STEP);
        }
        if input.consume_key(cmd, egui::Key::Num0) {
            *ui_scale = 1.0;
        }
    });
}

fn project_menu(ui: &mut egui::Ui, menu: MenuContext, actions: &mut MenuActions) {
    ui.menu_button("Project", |ui| {
        if ui
            .add_enabled(menu.dirty, egui::Button::new("Save room"))
            .on_disabled_hover_text("Nothing has changed")
            .clicked()
        {
            actions.save = true;
            ui.close();
        }
        if ui
            .add_enabled(menu.has_room, egui::Button::new("Reload room"))
            .on_hover_text("Throw away unsaved changes and read the file again")
            .clicked()
        {
            actions.reload = true;
            ui.close();
        }
        ui.separator();
        if ui
            .button("Game settings...")
            .on_hover_text("Title, which room the game starts in, languages, export")
            .clicked()
        {
            actions.open_game = true;
            ui.close();
        }
        if ui
            .button("Screens...")
            .on_hover_text("Menus and title cards: the parts of the game that are not rooms")
            .clicked()
        {
            actions.open_screens = true;
            ui.close();
        }
        if ui
            .button("Library...")
            .on_hover_text("Every picture, animation, and sound the project holds")
            .clicked()
        {
            actions.open_library = true;
            ui.close();
        }
        if ui
            .button("Story map...")
            .on_hover_text("Every place in the game and the ways between them")
            .clicked()
        {
            actions.open_story = true;
            ui.close();
        }
        if ui
            .button("Script graph...")
            .on_hover_text("What happens when something is clicked, as a wiring diagram")
            .clicked()
        {
            actions.open_graph = true;
            ui.close();
        }
    });
}

fn room_menu(ui: &mut egui::Ui, menu: MenuContext, actions: &mut MenuActions) {
    ui.menu_button("Room", |ui| {
        if !menu.has_room {
            ui.label(egui::RichText::new("No room open.").weak());
            return;
        }
        ui.label(egui::RichText::new("Add").small().weak());
        for what in [Addable::Hotspot, Addable::Exit, Addable::Entry] {
            if ui
                .button(what.label())
                .on_hover_text(what.hint())
                .clicked()
            {
                actions.add = Some(what);
                ui.close();
            }
        }
        ui.separator();
        if ui
            .button("Fit in view")
            .on_hover_text("Frame the whole room")
            .clicked()
        {
            actions.fit_view = true;
            ui.close();
        }
    });
}

fn view_menu(
    ui: &mut egui::Ui,
    menu: MenuContext,
    overlays: &mut Overlays,
    task: Task,
    ui_scale: &mut f32,
    actions: &mut MenuActions,
) {
    ui.menu_button("View", |ui| {
        ui.label(egui::RichText::new("Show over the picture").small().weak());
        ui.checkbox(&mut overlays.floor, "Walkable floor");
        ui.checkbox(&mut overlays.hotspots, "Clickable things");
        ui.checkbox(&mut overlays.exits, "Ways out");
        ui.checkbox(&mut overlays.entries, "Arrival points");
        ui.checkbox(&mut overlays.bounds, "Room edge");
        ui.separator();
        // Listing the tabs here too: a reader who has not noticed the
        // tab strip still finds out the editor has five jobs in it.
        ui.label(egui::RichText::new("Work on").small().weak());
        for other in Task::ALL {
            let mut chosen = task == other;
            if ui
                .checkbox(&mut chosen, other.label())
                .on_hover_text(other.question())
                .clicked()
            {
                actions.task = Some(other);
                ui.close();
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Interface size").small().weak());
        ui.horizontal(|ui| {
            if ui.small_button("-").clicked() {
                *ui_scale = clamp_ui_scale(*ui_scale - UI_SCALE_STEP);
            }
            // Shown as a percentage: 1.2 means nothing, 120% is a size.
            ui.label(format!("{:.0}%", *ui_scale * 100.0));
            if ui.small_button("+").clicked() {
                *ui_scale = clamp_ui_scale(*ui_scale + UI_SCALE_STEP);
            }
            if ui.small_button("reset").clicked() {
                *ui_scale = 1.0;
            }
        });
        let mut slider = *ui_scale;
        if ui
            .add(
                egui::Slider::new(&mut slider, MIN_UI_SCALE..=MAX_UI_SCALE)
                    .show_value(false),
            )
            .on_hover_text("Scales every panel, menu and label. Ctrl+ and Ctrl- do the same.")
            .changed()
        {
            *ui_scale = clamp_ui_scale(slider);
        }
        let _ = menu;
    });
}

fn help_menu(ui: &mut egui::Ui, actions: &mut MenuActions) {
    ui.menu_button("Help", |ui| {
        if ui
            .button("How this editor works")
            .on_hover_text("What each part of the window is for")
            .clicked()
        {
            actions.open_help = true;
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_addable_says_what_it_is_and_where_it_is_edited() {
        for what in [Addable::Hotspot, Addable::Exit, Addable::Entry] {
            assert!(!what.label().is_empty(), "{what:?} has no label");
            assert!(
                what.hint().len() > 20,
                "{what:?} needs a sentence saying what it is for"
            );
        }
        // Adding a thing and being left on a tab that does not show it
        // is the worst kind of nothing-happened.
        assert_eq!(Addable::Hotspot.task(), Task::Things);
        assert_eq!(Addable::Exit.task(), Task::Things);
        assert_eq!(Addable::Entry.task(), Task::Floor);
    }

    #[test]
    fn the_interface_scale_cannot_be_driven_somewhere_it_cannot_return_from() {
        // The control that would undo a bad scale has scaled too, so the
        // range has to be enforced rather than suggested.
        assert_eq!(clamp_ui_scale(0.01), MIN_UI_SCALE);
        assert_eq!(clamp_ui_scale(99.0), MAX_UI_SCALE);
        assert_eq!(clamp_ui_scale(1.0), 1.0);
        assert_eq!(clamp_ui_scale(f32::NAN), 1.0, "a NaN scale draws nothing");
        assert_eq!(clamp_ui_scale(f32::INFINITY), 1.0);
    }

    #[test]
    fn stepping_from_either_end_stays_in_range() {
        let mut scale = MIN_UI_SCALE;
        for _ in 0..40 {
            scale = clamp_ui_scale(scale - UI_SCALE_STEP);
        }
        assert_eq!(scale, MIN_UI_SCALE);
        for _ in 0..80 {
            scale = clamp_ui_scale(scale + UI_SCALE_STEP);
        }
        assert_eq!(scale, MAX_UI_SCALE);
    }

    #[test]
    fn a_fresh_bar_asks_for_nothing() {
        let actions = MenuActions::default();
        assert_eq!(actions, MenuActions::default());
        assert!(actions.add.is_none() && actions.task.is_none());
        assert!(!actions.save && !actions.start_play && !actions.open_help);
    }
}
