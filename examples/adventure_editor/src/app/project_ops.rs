//! A98: opening and making games, from inside the editor.
//!
//! Loading a project is not one assignment: it is the rooms, the
//! scripts, the screens, the manifest, the languages, and the pile of
//! per-room state that has to be thrown away with the old game. Doing
//! it in one place means the second project of a session opens exactly
//! like the first, which is not true of an editor that only ever
//! loaded at startup.

use std::path::{Path, PathBuf};

use infengine_adventure::pack::GameManifest;
use infengine_adventure::{Project, Room};
use tracing::warn;

use crate::play::PlayMode;
use crate::ui_game::GameUi;
use crate::ui_screen::ScreenUi;

use super::Editor;

impl Editor {
    /// Open a game folder, replacing whatever was open.
    pub(super) fn open_project(&mut self, root: &Path) {
        let project = Project::open(root);
        let root = project.root.clone();
        self.play = PlayMode::with_scripts(project.scripts.clone());
        self.game_ui = GameUi {
            manifest: GameManifest::load(&root).unwrap_or_default(),
            ..Default::default()
        };
        let (screens, errors) = infengine_adventure::screen::load_dir(
            &root.join(infengine_adventure::screen::SCREEN_DIR),
        );
        for err in &errors {
            warn!(%err, "a screen did not load");
        }
        self.screen_ui = ScreenUi {
            selected: (!screens.is_empty()).then_some(0),
            screens,
            ..Default::default()
        };

        // Everything that described the last game has to go, or the new
        // one opens wearing its predecessor's rooms and languages.
        self.project = project;
        self.current_room = None;
        self.room = Room::default();
        self.room_path = None;
        self.dirty = false;
        self.scripts_dirty = false;
        self.textures.clear();
        self.anim_textures.clear();
        self.missing_images.clear();
        self.undo.clear();
        self.bench = None;
        self.graph_ui = Default::default();
        self.story_cache = None;
        self.story_layout = crate::story_layout::Layout::load(&root);
        self.cards = Default::default();
        self.languages.clear();
        self.lines.clear();
        self.playback.clear();
        self.room_animations.clear();
        self.scene_script = None;
        self.timeline_selected = None;
        self.browser.open = false;
        self.browser.at = root.clone();

        self.remember_recent(&root);
        self.refresh_pictures();
        self.refresh_slots();
        if !self.project.is_empty() {
            self.open_room(0);
        }
        self.status = if self.project.is_empty() {
            format!("no rooms under {}", root.display())
        } else {
            format!("{} - {} rooms", root.display(), self.project.rooms.len())
        };
    }

    /// Make a game folder with everything a game needs, and open it.
    ///
    /// The skeleton is what the editor itself expects to find: the
    /// directories the library's shelves read from, and a manifest, so
    /// the new project is a project rather than an empty folder that
    /// becomes one after enough saves.
    pub(super) fn create_project(&mut self, at: &Path) {
        use infengine_adventure::{anim, pixfont, project, screen};

        if at.exists() && at.read_dir().into_iter().flatten().next().is_some() {
            self.browser.status = format!("{} is not empty", at.display());
            return;
        }
        let dirs = [
            project::ROOMS_DIR,
            "pictures",
            "sprites",
            anim::ANIMATION_DIR,
            screen::SCREEN_DIR,
            pixfont::FONT_DIR,
            "sounds",
            "scripts",
        ];
        for dir in dirs {
            if let Err(err) = std::fs::create_dir_all(at.join(dir)) {
                warn!(%err, "could not make the project folder");
                self.browser.status = err.to_string();
                return;
            }
        }
        let title = at
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("New game")
            .to_string();
        let manifest = GameManifest {
            title,
            ..Default::default()
        };
        if let Err(err) = manifest.save(at) {
            warn!(%err, "could not write the manifest");
            self.browser.status = err.to_string();
            return;
        }
        self.open_project(at);
        self.status = "new game - make a room on the left to start".into();
    }

    /// Put a project at the top of the recent list, on disk.
    ///
    /// Written at once rather than on exit: an editor that crashes
    /// should still remember what it had open, and that is exactly the
    /// session where remembering matters.
    fn remember_recent(&mut self, root: &Path) {
        let mut settings = crate::settings::Settings::load();
        settings.remember_project(root);
        self.recent = settings.recent.clone();
        settings.save();
    }

    /// Drop a recent entry whose folder is gone.
    pub(super) fn forget_recent(&mut self, root: &Path) {
        let mut settings = crate::settings::Settings::load();
        settings.forget_project(root);
        self.recent = settings.recent.clone();
        settings.save();
    }

    /// Show the page that opens games.
    pub(super) fn show_browser(&mut self) {
        self.browser.at = if self.project.root.is_dir() {
            self.project.root.clone()
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };
        self.browser.status.clear();
        self.browser.open = true;
    }
}
