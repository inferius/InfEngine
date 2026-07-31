//! A24.1: menus and the rest of the game's own screens.
//!
//! A title screen, an options page, the inventory bar - all of them are
//! a picture with things placed on it that the player can click. That is
//! also what a room is, so a screen is deliberately built the same way:
//! rectangles over an image, with an id, and text referenced by
//! localisation id rather than written in.
//!
//! Keeping the text as ids is what makes a menu translatable at all. A
//! button labelled "New game" in the file is a button that says "New
//! game" in every language, and finding out at release time is the usual
//! way that happens.
//!
//! A screen knows nothing about how it is drawn. It says a widget covers
//! this rectangle and means this; whether the host renders it with the
//! room camera or straight to the window is the host's business.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Directory holding screen files, relative to the project root.
pub const SCREEN_DIR: &str = "screens";
/// Suffix identifying one.
pub const SCREEN_SUFFIX: &str = ".screen.ron";

/// Where a label sits inside its rectangle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

/// What pressing a widget does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenAction {
    /// Start the game at the manifest's starting room.
    NewGame,
    /// Resume whatever was being played.
    Resume,
    /// Show another screen.
    Open { screen: String },
    /// Close this screen and go back to what was underneath.
    Close,
    Quit,
    /// Pick the language subtitles are read in.
    SetTextLanguage { language: String },
    /// Pick the language recordings are played in. Separate from the
    /// text one on purpose - see `LanguageChoice`.
    SetVoiceLanguage { language: String },
    /// Write the game to a slot.
    SaveGame { slot: u32 },
    /// Read a slot back.
    LoadGame { slot: u32 },
    /// Run a script, for anything the list above does not cover.
    RunScript { script: String },
    /// Nothing. A decorative widget that happens to be clickable-shaped.
    #[default]
    None,
}

impl ScreenAction {
    pub fn label(&self) -> String {
        match self {
            ScreenAction::NewGame => "new game".into(),
            ScreenAction::Resume => "resume".into(),
            ScreenAction::Open { screen } => format!("open {screen}"),
            ScreenAction::Close => "close".into(),
            ScreenAction::Quit => "quit".into(),
            ScreenAction::SetTextLanguage { language } => format!("text = {language}"),
            ScreenAction::SetVoiceLanguage { language } => format!("voice = {language}"),
            ScreenAction::SaveGame { slot } => format!("save to {slot}"),
            ScreenAction::LoadGame { slot } => format!("load {slot}"),
            ScreenAction::RunScript { script } => format!("run {script}"),
            ScreenAction::None => "nothing".into(),
        }
    }
}

/// What a widget is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WidgetKind {
    /// A picture, project-relative.
    Image { image: String },
    /// Text, by localisation id.
    Label {
        line: String,
        #[serde(default = "default_text_size")]
        size: f32,
        #[serde(default)]
        align: Align,
    },
    /// Text you can press.
    Button {
        line: String,
        #[serde(default = "default_text_size")]
        size: f32,
        #[serde(default)]
        align: Align,
        #[serde(default)]
        action: ScreenAction,
    },
}

fn default_text_size() -> f32 {
    16.0
}

impl WidgetKind {
    /// Localisation id this widget shows, if any.
    pub fn line(&self) -> Option<&str> {
        match self {
            WidgetKind::Image { .. } => None,
            WidgetKind::Label { line, .. } | WidgetKind::Button { line, .. } => Some(line),
        }
    }

    /// Image this widget draws, if any.
    pub fn image(&self) -> Option<&str> {
        match self {
            WidgetKind::Image { image } => Some(image),
            _ => None,
        }
    }

    pub fn action(&self) -> Option<&ScreenAction> {
        match self {
            WidgetKind::Button { action, .. } => Some(action),
            _ => None,
        }
    }
}

/// One thing on a screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    /// Top-left and bottom-right, in screen units.
    pub min: Vec2,
    pub max: Vec2,
    pub kind: WidgetKind,
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool {
    true
}

impl Widget {
    pub fn contains(&self, p: Vec2) -> bool {
        let (lo, hi) = (self.min.min(self.max), self.min.max(self.max));
        p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y
    }

    pub fn area(&self) -> f32 {
        let d = (self.max - self.min).abs();
        d.x * d.y
    }

    /// Move the whole widget.
    pub fn translate(&mut self, delta: Vec2) {
        self.min += delta;
        self.max += delta;
    }
}

/// A menu, an options page, a heads-up bar.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub id: String,
    /// Design size in screen units. The host scales this to the window.
    pub size: Vec2,
    #[serde(default)]
    pub widgets: Vec<Widget>,
    /// Whether what is behind it keeps being drawn - true for a pause
    /// menu over a room, false for a title screen.
    #[serde(default)]
    pub overlay: bool,
}

impl Screen {
    pub fn new(id: impl Into<String>, size: Vec2) -> Self {
        Self {
            id: id.into(),
            size,
            ..Default::default()
        }
    }

    /// The topmost visible widget under `p`.
    ///
    /// Smallest wins where they overlap, the same rule rooms use: a
    /// button drawn on top of a panel has to be reachable, and it is
    /// always the tighter rectangle.
    pub fn widget_at(&self, p: Vec2) -> Option<&Widget> {
        self.widgets
            .iter()
            .filter(|widget| widget.visible && widget.contains(p))
            .min_by(|a, b| {
                a.area()
                    .partial_cmp(&b.area())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Every localisation id the screen shows.
    pub fn lines(&self) -> BTreeSet<String> {
        self.widgets
            .iter()
            .filter_map(|widget| widget.kind.line())
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Every image the screen draws.
    pub fn images(&self) -> BTreeSet<String> {
        self.widgets
            .iter()
            .filter_map(|widget| widget.kind.image())
            .filter(|image| !image.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Screens this one can lead to.
    pub fn destinations(&self) -> BTreeSet<String> {
        self.widgets
            .iter()
            .filter_map(|widget| widget.kind.action())
            .filter_map(|action| match action {
                ScreenAction::Open { screen } => Some(screen.clone()),
                _ => None,
            })
            .collect()
    }

    /// Complaints worth showing in the editor.
    ///
    /// Not errors: a screen being built is allowed to be half finished.
    /// These are the things that would be silently wrong at run time -
    /// a button with no action does nothing, and a widget with no id
    /// cannot be referred to.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();

        for widget in &self.widgets {
            if widget.id.trim().is_empty() {
                problems.push("a widget has no id".to_string());
            } else if !seen.insert(widget.id.as_str()) {
                problems.push(format!("two widgets share the id {}", widget.id));
            }
            if widget.area() <= 0.0 {
                problems.push(format!("{} has no area", widget.id));
            }
            if let WidgetKind::Button { action, line, .. } = &widget.kind {
                if *action == ScreenAction::None {
                    problems.push(format!("{} is a button that does nothing", widget.id));
                }
                if line.trim().is_empty() {
                    problems.push(format!("{} has no text", widget.id));
                }
            }
        }
        problems
    }
}

#[derive(Debug)]
pub enum ScreenIoError {
    Read { path: String, source: std::io::Error },
    Parse { path: String, detail: String },
}

impl std::fmt::Display for ScreenIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScreenIoError::Read { path, source } => write!(f, "{path}: {source}"),
            ScreenIoError::Parse { path, detail } => write!(f, "{path}: {detail}"),
        }
    }
}

impl std::error::Error for ScreenIoError {}

pub fn save(screen: &Screen, dir: &Path) -> Result<(), ScreenIoError> {
    let text = ron::ser::to_string_pretty(
        screen,
        ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
    )
    .map_err(|err| ScreenIoError::Parse {
        path: screen.id.clone(),
        detail: err.to_string(),
    })?;
    fs::create_dir_all(dir).map_err(|source| ScreenIoError::Read {
        path: dir.display().to_string(),
        source,
    })?;
    let path = dir.join(format!("{}{SCREEN_SUFFIX}", screen.id));
    fs::write(&path, text).map_err(|source| ScreenIoError::Read {
        path: path.display().to_string(),
        source,
    })
}

/// Load every screen in a directory, sorted by id.
///
/// A file that will not parse is reported and the rest are kept, for
/// the same reason scripts work that way: one broken menu must not cost
/// the editor every other one.
pub fn load_dir(dir: &Path) -> (Vec<Screen>, Vec<ScreenIoError>) {
    let mut screens = Vec::new();
    let mut errors = Vec::new();

    let Ok(entries) = fs::read_dir(dir) else {
        return (screens, errors);
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(SCREEN_SUFFIX))
        })
        .collect();
    paths.sort();

    for path in paths {
        let origin = path.display().to_string();
        match fs::read_to_string(&path) {
            Ok(text) => match ron::from_str::<Screen>(&text) {
                Ok(screen) => screens.push(screen),
                Err(err) => errors.push(ScreenIoError::Parse {
                    path: origin,
                    detail: err.to_string(),
                }),
            },
            Err(source) => errors.push(ScreenIoError::Read { path: origin, source }),
        }
    }
    screens.sort_by(|a, b| a.id.cmp(&b.id));
    (screens, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(id: &str, min: Vec2, max: Vec2, line: &str, action: ScreenAction) -> Widget {
        Widget {
            id: id.into(),
            min,
            max,
            kind: WidgetKind::Button {
                line: line.into(),
                size: 16.0,
                align: Align::Center,
                action,
            },
            visible: true,
        }
    }

    fn title_screen() -> Screen {
        let mut screen = Screen::new("title", Vec2::new(320.0, 200.0));
        screen.widgets.push(Widget {
            id: "backdrop".into(),
            min: Vec2::ZERO,
            max: Vec2::new(320.0, 200.0),
            kind: WidgetKind::Image {
                image: "images/title.png".into(),
            },
            visible: true,
        });
        screen.widgets.push(button(
            "new",
            Vec2::new(100.0, 120.0),
            Vec2::new(220.0, 140.0),
            "menu.new_game",
            ScreenAction::NewGame,
        ));
        screen.widgets.push(button(
            "options",
            Vec2::new(100.0, 150.0),
            Vec2::new(220.0, 170.0),
            "menu.options",
            ScreenAction::Open {
                screen: "options".into(),
            },
        ));
        screen
    }

    #[test]
    fn the_tighter_widget_wins_where_they_overlap() {
        let screen = title_screen();
        // The button sits on top of the full-screen backdrop.
        let hit = screen.widget_at(Vec2::new(160.0, 130.0)).expect("a hit");
        assert_eq!(hit.id, "new");
        // And off the buttons, the backdrop is still there.
        assert_eq!(
            screen.widget_at(Vec2::new(10.0, 10.0)).map(|w| w.id.as_str()),
            Some("backdrop")
        );
    }

    #[test]
    fn a_hidden_widget_is_not_clickable() {
        let mut screen = title_screen();
        screen.widgets[1].visible = false;
        assert_eq!(
            screen.widget_at(Vec2::new(160.0, 130.0)).map(|w| w.id.as_str()),
            Some("backdrop")
        );
    }

    #[test]
    fn a_screen_names_its_text_its_art_and_where_it_leads() {
        let screen = title_screen();
        assert!(screen.lines().contains("menu.new_game"));
        assert!(screen.lines().contains("menu.options"));
        assert!(screen.images().contains("images/title.png"));
        assert_eq!(screen.destinations().len(), 1);
        assert!(screen.destinations().contains("options"));
    }

    #[test]
    fn a_button_that_does_nothing_is_a_complaint_not_an_error() {
        let mut screen = title_screen();
        screen.widgets.push(button(
            "dead",
            Vec2::new(0.0, 0.0),
            Vec2::new(50.0, 20.0),
            "menu.whatever",
            ScreenAction::None,
        ));
        let problems = screen.problems();
        assert!(
            problems.iter().any(|p| p.contains("dead")),
            "{problems:?}"
        );
        // And the screen is still perfectly loadable and usable.
        assert!(screen.widget_at(Vec2::new(10.0, 10.0)).is_some());
    }

    #[test]
    fn duplicate_ids_and_zero_sized_widgets_are_reported() {
        let mut screen = Screen::new("broken", Vec2::new(320.0, 200.0));
        screen.widgets.push(button(
            "same",
            Vec2::ZERO,
            Vec2::new(10.0, 10.0),
            "a",
            ScreenAction::Close,
        ));
        screen.widgets.push(button(
            "same",
            Vec2::ZERO,
            Vec2::ZERO,
            "b",
            ScreenAction::Close,
        ));
        let problems = screen.problems();
        assert!(problems.iter().any(|p| p.contains("share the id")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("no area")), "{problems:?}");
    }

    #[test]
    fn a_screen_round_trips_through_a_directory() {
        let dir = std::env::temp_dir().join("infengine_screen_round_trip");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");

        let screen = title_screen();
        save(&screen, &dir).expect("save");
        let (loaded, errors) = load_dir(&dir);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(loaded, vec![screen]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_broken_screen_does_not_cost_the_others() {
        let dir = std::env::temp_dir().join("infengine_screen_broken");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");

        save(&title_screen(), &dir).expect("save");
        fs::write(dir.join("bad.screen.ron"), "not ron").expect("write");

        let (loaded, errors) = load_dir(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("bad.screen.ron"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rectangle_dragged_backwards_still_contains_its_middle() {
        let widget = button(
            "backwards",
            Vec2::new(100.0, 100.0),
            Vec2::new(20.0, 20.0),
            "x",
            ScreenAction::Close,
        );
        assert!(widget.contains(Vec2::new(60.0, 60.0)));
        assert!(!widget.contains(Vec2::new(120.0, 60.0)));
    }
}
