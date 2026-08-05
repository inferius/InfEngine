//! A98: opening a game without going through a terminal.
//!
//! The editor could only be pointed at a project on the command line,
//! which makes it a tool for whoever launched it rather than a tool
//! anyone can open. So it starts on a page instead: the projects you
//! had open recently, a folder browser that says which folders are
//! projects, and a New button that writes a fresh one.
//!
//! What marks a folder as a project is `game.ron` beside a `rooms/`
//! directory - the manifest was already the project file in everything
//! but name, and inventing a second one would only mean two files to
//! keep in step. A folder holding `rooms/` and no manifest still opens,
//! because that is what an import produces before anyone saves the
//! game's settings.

use std::path::{Path, PathBuf};

use infengine_adventure::pack::MANIFEST_FILE;
use infengine_adventure::project::ROOMS_DIR;

use crate::lang::t;

/// The page's own state.
#[derive(Debug, Clone)]
pub struct Browser {
    /// Whether the page is showing.
    pub open: bool,
    /// Folder being looked at.
    pub at: PathBuf,
    /// What a new project would be called.
    pub new_name: String,
    pub status: String,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            open: false,
            at: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            new_name: String::new(),
            status: String::new(),
        }
    }
}

/// What the page asked for.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct BrowserActions {
    /// A project to open, by its folder.
    pub open: Option<PathBuf>,
    /// A project to create: where it goes.
    pub create: Option<PathBuf>,
    /// A recent entry that no longer exists, to forget.
    pub forget: Option<PathBuf>,
}

/// Whether a folder holds a game.
///
/// The manifest is what the editor writes for a project it knows
/// about; `rooms/` is what an importer leaves behind before anyone has
/// saved settings. Either is enough to open.
pub fn is_project(path: &Path) -> bool {
    path.join(MANIFEST_FILE).is_file() || path.join(ROOMS_DIR).is_dir()
}

/// How a project describes itself in a list: its title if it has one.
pub fn title_of(path: &Path) -> Option<String> {
    let manifest = infengine_adventure::pack::GameManifest::load(path)?;
    (!manifest.title.trim().is_empty()).then_some(manifest.title)
}

/// Draw the page. Returns what was asked for.
pub fn panel(
    ctx: &egui::Context,
    state: &mut Browser,
    recent: &[PathBuf],
) -> BrowserActions {
    let mut actions = BrowserActions::default();
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(12.0);
        ui.heading(t("Open a game", "Otevřít hru"));
        ui.label(
            egui::RichText::new(t(
                "A game is a folder holding rooms/ and the rest of its parts.",
                "Hra je složka, ve které je rooms/ a zbytek jejích částí.",
            ))
            .small()
            .weak(),
        );
        ui.add_space(10.0);

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                recent_list(ui, recent, &mut actions);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 12.0).max(240.0));
                folder_list(ui, state, &mut actions);
            });
        });

        if !state.status.is_empty() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(&state.status)
                    .small()
                    .color(egui::Color32::from_rgb(255, 200, 120)),
            );
        }
    });
    actions
}

/// The projects this editor has had open, newest first.
fn recent_list(ui: &mut egui::Ui, recent: &[PathBuf], actions: &mut BrowserActions) {
    ui.label(egui::RichText::new(t("RECENT", "NAPOSLEDY")).small().weak());
    if recent.is_empty() {
        ui.label(
            egui::RichText::new(t(
                "Nothing yet. Find a folder on the right.",
                "Zatím nic. Najdi složku vpravo.",
            ))
            .small()
            .weak(),
        );
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("recent_projects")
        .max_height(320.0)
        .show(ui, |ui| {
            for path in recent {
                let gone = !path.is_dir();
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                let mut text = egui::text::LayoutJob::default();
                text.append(
                    &match title_of(path) {
                        Some(title) => format!("{title}  ({name})"),
                        None => name,
                    },
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(14.0),
                        color: if gone {
                            egui::Color32::from_gray(120)
                        } else {
                            ui.visuals().strong_text_color()
                        },
                        ..Default::default()
                    },
                );
                text.append(
                    &format!("\n{}", path.display()),
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(10.0),
                        color: ui.visuals().weak_text_color(),
                        ..Default::default()
                    },
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!gone, egui::Button::new(text))
                        .on_disabled_hover_text(t(
                            "This folder is not there any more",
                            "Tahle složka už neexistuje",
                        ))
                        .clicked()
                    {
                        actions.open = Some(path.clone());
                    }
                    if gone && ui.small_button("✕").clicked() {
                        actions.forget = Some(path.clone());
                    }
                });
            }
        });
}

/// The folder browser: where you are, what is under it, what is a game.
fn folder_list(ui: &mut egui::Ui, state: &mut Browser, actions: &mut BrowserActions) {
    ui.horizontal(|ui| {
        if ui
            .small_button("⬆")
            .on_hover_text(t("Up one folder", "O složku výš"))
            .clicked()
            && let Some(parent) = state.at.parent().map(Path::to_path_buf)
        {
            state.at = parent;
        }
        ui.label(
            egui::RichText::new(state.at.display().to_string())
                .small()
                .weak(),
        );
    });

    // The folder you are standing in, when it is itself a game.
    if is_project(&state.at) {
        ui.horizontal(|ui| {
            if ui
                .button(t("Open this folder", "Otevřít tuhle složku"))
                .clicked()
            {
                actions.open = Some(state.at.clone());
            }
            ui.label(
                egui::RichText::new(t("this folder is a game", "tahle složka je hra"))
                    .small()
                    .color(egui::Color32::from_rgb(150, 220, 150)),
            );
        });
    }

    ui.separator();
    let mut folders: Vec<PathBuf> = std::fs::read_dir(&state.at)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();

    egui::ScrollArea::vertical()
        .id_salt("folder_browser")
        .max_height(280.0)
        .show(ui, |ui| {
            if folders.is_empty() {
                ui.label(
                    egui::RichText::new(t("No folders here.", "Žádné složky."))
                        .small()
                        .weak(),
                );
            }
            for path in folders {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                let game = is_project(&path);
                ui.horizontal(|ui| {
                    let label = if game {
                        egui::RichText::new(format!("🎮 {name}"))
                            .color(egui::Color32::from_rgb(150, 220, 150))
                    } else {
                        egui::RichText::new(format!("📁 {name}"))
                    };
                    if ui.selectable_label(false, label).clicked() {
                        state.at = path.clone();
                    }
                    if game
                        && ui
                            .small_button(t("open", "otevřít"))
                            .clicked()
                    {
                        actions.open = Some(path.clone());
                    }
                });
            }
        });

    ui.separator();
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.new_name)
                .hint_text(t("new game name", "jméno nové hry"))
                .desired_width(160.0),
        );
        let name = state.new_name.trim().to_string();
        if ui
            .add_enabled(!name.is_empty(), egui::Button::new(t("+ New game", "+ Nová hra")))
            .on_hover_text(t(
                "Make a folder here with everything a game needs",
                "Založ tady složku se vším, co hra potřebuje",
            ))
            .clicked()
        {
            actions.create = Some(state.at.join(name));
            state.new_name.clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_with_rooms_is_a_game_even_before_it_has_a_manifest() {
        // What an importer leaves behind, before anyone saves settings.
        let dir = std::env::temp_dir().join("infengine_browser_rooms");
        let _ = std::fs::create_dir_all(dir.join(ROOMS_DIR));
        assert!(is_project(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_with_a_manifest_is_a_game() {
        let dir = std::env::temp_dir().join("infengine_browser_manifest");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(MANIFEST_FILE), "(title: \"x\")");
        assert!(is_project(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ordinary_folder_is_not_a_game() {
        let dir = std::env::temp_dir().join("infengine_browser_plain");
        let _ = std::fs::create_dir_all(&dir);
        assert!(!is_project(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn actions_default_to_doing_nothing() {
        let actions = BrowserActions::default();
        assert!(actions.open.is_none() && actions.create.is_none());
    }
}
