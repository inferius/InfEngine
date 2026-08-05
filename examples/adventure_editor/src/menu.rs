//! A42.1 / A83: the top bar - menus, the mode pills, and the controls
//! that never move.
//!
//! The bar is where the editor says what it contains. The menus list
//! every part by name; the four mode pills say what the editor is being
//! used for right now; the right-hand cluster keeps the three things a
//! session always needs - whether the room is saved, the button that
//! saves it, and the button that plays the game - in the corner where
//! every tool keeps them.
//!
//! Two rules it keeps:
//!
//! * nothing is offered that does not exist. An item for a feature that
//!   is not built teaches the menu cannot be trusted.
//! * an item that cannot act right now is disabled with a reason on
//!   hover, not hidden. Disappearing items make a menu unlearnable.

use crate::draw::Overlays;
use crate::mode::Mode;

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
    pub open_story: bool,
    /// Take the last canvas edit back.
    pub undo: bool,
    pub open_help: bool,
    /// Switch the editor to a mode.
    pub mode: Option<Mode>,
    pub start_play: bool,
    pub stop_play: bool,
    /// Run the game as its own process, in its own window.
    pub play_in_window: bool,
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
        use crate::lang::t;
        match self {
            Addable::Hotspot => t("Clickable thing", "Klikací věc"),
            Addable::Exit => t("Way out", "Východ"),
            Addable::Entry => t("Arrival point", "Místo příchodu"),
        }
    }

    /// What it is for, on hover. Adding something you cannot name is how
    /// a room fills up with `hotspot_4`.
    pub fn hint(self) -> &'static str {
        use crate::lang::t;
        match self {
            Addable::Hotspot => t(
                "A region the player can click - scenery to look at, an object to take.",
                "Oblast, na kterou jde kliknout - kulisa k prohlédnutí, předmět k sebrání.",
            ),
            Addable::Exit => t(
                "A region that sends the player to another room.",
                "Oblast, která pošle hráče do jiné místnosti.",
            ),
            Addable::Entry => t(
                "Where the actor stands when a door leads into this room.",
                "Kde postava stojí, když sem vedou dveře.",
            ),
        }
    }
}

/// How big the interface is drawn, as a multiple of its natural size.
///
/// A 4K monitor renders this at a size that is legible in principle and
/// unreadable in practice, and the fix is not per-widget font sizes but
/// one number the whole interface is scaled by.
///
/// Bounded rather than free: below 0.6 the mode pills stop being
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

/// State the bar reads: whether a room is open, whether it is dirty,
/// and the name of the place being edited.
#[derive(Debug, Clone)]
pub struct MenuContext {
    pub has_room: bool,
    pub dirty: bool,
    pub playing: bool,
    /// "project · room", shown beside the menus so the window always
    /// says where you are.
    pub crumb: String,
}

/// Draw the bar. Returns what was asked for.
pub fn bar(
    ctx: &egui::Context,
    menu: &MenuContext,
    overlays: &mut Overlays,
    mode: Mode,
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
            view_menu(ui, overlays, ui_scale);
            help_menu(ui, &mut actions);
            ui.label(egui::RichText::new(&menu.crumb).small().weak());

            ui.separator();
            for option in Mode::ALL {
                let selected = mode == option;
                if ui
                    .selectable_label(selected, option.label())
                    .on_hover_text(option.hint())
                    .clicked()
                    && !selected
                {
                    actions.mode = Some(option);
                }
            }

            // The right-hand cluster: saved-or-not, Save, Play. Laid
            // right-to-left, so the first added is the rightmost.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                use crate::lang::t;
                if menu.playing {
                    if ui.button(t("⏹ Stop", "⏹ Zastavit")).clicked() {
                        actions.stop_play = true;
                    }
                } else if ui
                    .add_enabled(
                        menu.has_room,
                        egui::Button::new(t("▶ Play game", "▶ Spustit hru")),
                    )
                    .on_hover_text(t("Run the game from this room", "Spusť hru od této místnosti"))
                    .on_disabled_hover_text(t("Open a room first", "Nejdřív otevři místnost"))
                    .clicked()
                {
                    actions.start_play = true;
                }
                if ui
                    .add_enabled(menu.dirty, egui::Button::new(t("Save", "Uložit")))
                    .on_hover_text(t(
                        "Write this room and its scripts back to their files",
                        "Zapiš místnost a její skripty zpět do souborů",
                    ))
                    .on_disabled_hover_text(t("Nothing has changed", "Nic se nezměnilo"))
                    .clicked()
                {
                    actions.save = true;
                }
                if menu.dirty {
                    ui.label(
                        egui::RichText::new(t("● unsaved", "● neuloženo"))
                            .small()
                            .color(egui::Color32::from_rgb(255, 200, 120)),
                    );
                }
                if ui
                    .button("?")
                    .on_hover_text(t("How this editor works", "Jak tento editor funguje"))
                    .clicked()
                {
                    actions.open_help = true;
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

fn project_menu(ui: &mut egui::Ui, menu: &MenuContext, actions: &mut MenuActions) {
    use crate::lang::t;
    ui.menu_button(t("Project", "Projekt"), |ui| {
        if ui
            .add_enabled(menu.dirty, egui::Button::new(t("Save room", "Uložit místnost")))
            .on_disabled_hover_text(t("Nothing has changed", "Nic se nezměnilo"))
            .clicked()
        {
            actions.save = true;
            ui.close();
        }
        if ui
            .add_enabled(
                menu.has_room,
                egui::Button::new(t("Reload room", "Znovu načíst místnost")),
            )
            .on_hover_text(t(
                "Throw away unsaved changes and read the file again",
                "Zahoď neuložené změny a načti soubor znovu",
            ))
            .clicked()
        {
            actions.reload = true;
            ui.close();
        }
        ui.separator();
        if ui
            .button(t("Play in its own window", "Spustit ve vlastním okně"))
            .on_hover_text(t(
                "Run the game as the player will see it - a separate window, no editor around it",
                "Spusť hru, jak ji uvidí hráč - samostatné okno, žádný editor okolo",
            ))
            .clicked()
        {
            actions.play_in_window = true;
            ui.close();
        }
        ui.separator();
        if ui
            .button(t("Game settings...", "Nastavení hry..."))
            .on_hover_text(t(
                "Title, which room the game starts in, languages, export",
                "Název, startovní místnost, jazyky, export",
            ))
            .clicked()
        {
            actions.open_game = true;
            ui.close();
        }
        if ui
            .button(t("Screens...", "Obrazovky..."))
            .on_hover_text(t(
                "Menus and title cards: the parts of the game that are not rooms",
                "Menu a titulky: části hry, které nejsou místnosti",
            ))
            .clicked()
        {
            actions.open_screens = true;
            ui.close();
        }
        if ui
            .button(t("Story map...", "Mapa příběhu..."))
            .on_hover_text(t(
                "Every place in the game and the ways between them",
                "Všechna místa ve hře a cesty mezi nimi",
            ))
            .clicked()
        {
            actions.open_story = true;
            ui.close();
        }
    });
}

fn room_menu(ui: &mut egui::Ui, menu: &MenuContext, actions: &mut MenuActions) {
    use crate::lang::t;
    ui.menu_button(t("Room", "Místnost"), |ui| {
        if !menu.has_room {
            ui.label(egui::RichText::new(t("No room open.", "Žádná místnost.")).weak());
            return;
        }
        ui.label(egui::RichText::new(t("Add", "Přidat")).small().weak());
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
            .button(t("Fit in view", "Zarámovat do okna"))
            .on_hover_text(t("Frame the whole room", "Ukázat celou místnost"))
            .clicked()
        {
            actions.fit_view = true;
            ui.close();
        }
    });
}

fn view_menu(ui: &mut egui::Ui, overlays: &mut Overlays, ui_scale: &mut f32) {
    use crate::lang::t;
    ui.menu_button(t("View", "Zobrazení"), |ui| {
        ui.label(
            egui::RichText::new(t("Show over the picture", "Ukázat přes obrázek"))
                .small()
                .weak(),
        );
        ui.checkbox(&mut overlays.floor, t("Walkable floor", "Pochozí podlaha"));
        ui.checkbox(&mut overlays.hotspots, t("Clickable things", "Klikací věci"));
        ui.checkbox(&mut overlays.exits, t("Ways out", "Východy"));
        ui.checkbox(&mut overlays.entries, t("Arrival points", "Místa příchodu"));
        ui.checkbox(&mut overlays.bounds, t("Room edge", "Okraj místnosti"));
        ui.checkbox(&mut overlays.camera, t("Camera frame", "Rám kamery"));
        ui.separator();
        // The editor's own language, apart from the game's: what the
        // panels say, not what the characters do.
        ui.label(egui::RichText::new(t("Language", "Jazyk")).small().weak());
        ui.horizontal(|ui| {
            let mut czech = crate::lang::is_czech();
            if ui.selectable_label(!czech, "English").clicked() {
                czech = false;
            }
            if ui.selectable_label(czech, "Čeština").clicked() {
                czech = true;
            }
            crate::lang::set_czech(czech);
        });
        ui.separator();
        ui.label(
            egui::RichText::new(t("Interface size", "Velikost rozhraní"))
                .small()
                .weak(),
        );
        ui.horizontal(|ui| {
            if ui.small_button("-").clicked() {
                *ui_scale = clamp_ui_scale(*ui_scale - UI_SCALE_STEP);
            }
            // Shown as a percentage: 1.2 means nothing, 120% is a size.
            ui.label(format!("{:.0}%", *ui_scale * 100.0));
            if ui.small_button("+").clicked() {
                *ui_scale = clamp_ui_scale(*ui_scale + UI_SCALE_STEP);
            }
            if ui.small_button(t("reset", "výchozí")).clicked() {
                *ui_scale = 1.0;
            }
        });
        let mut slider = *ui_scale;
        if ui
            .add(
                egui::Slider::new(&mut slider, MIN_UI_SCALE..=MAX_UI_SCALE)
                    .show_value(false),
            )
            .on_hover_text(t(
                "Scales every panel, menu and label. Ctrl+ and Ctrl- do the same.",
                "Zvětší každý panel, menu i popisek. Ctrl+ a Ctrl- dělají totéž.",
            ))
            .changed()
        {
            *ui_scale = clamp_ui_scale(slider);
        }
    });
}

fn help_menu(ui: &mut egui::Ui, actions: &mut MenuActions) {
    use crate::lang::t;
    ui.menu_button(t("Help", "Nápověda"), |ui| {
        if ui
            .button(t("How this editor works", "Jak tento editor funguje"))
            .on_hover_text(t(
                "What each part of the window is for",
                "K čemu je která část okna",
            ))
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
    fn everything_addable_says_what_it_is_for() {
        for what in [Addable::Hotspot, Addable::Exit, Addable::Entry] {
            assert!(!what.label().is_empty(), "{what:?} has no label");
            assert!(
                what.hint().len() > 20,
                "{what:?} needs a sentence saying what it is for"
            );
        }
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
        assert!(actions.add.is_none() && actions.mode.is_none());
        assert!(!actions.save && !actions.start_play && !actions.open_help);
    }
}
