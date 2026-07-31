//! A24.3: carrying out what the screen editor and its preview ask for.

use glam::Vec2;

use crate::ui_screen;

use super::Editor;

impl Editor {
    /// Apply what the screen editor asked for.
    pub(super) fn apply_screen_actions(&mut self, actions: ui_screen::ScreenActions) {
        use infengine_adventure::screen;

        let dir = self.project.root.join(screen::SCREEN_DIR);
        if let Some(id) = actions.create {
            let size = if self.room.size.x > 0.0 {
                self.room.size
            } else {
                Vec2::new(320.0, 200.0)
            };
            self.screen_ui.screens.push(screen::Screen::new(id, size));
            self.screen_ui.screens.sort_by(|a, b| a.id.cmp(&b.id));
            self.screen_ui.selected_widget = None;
            self.screen_ui.dirty = true;
        }
        if actions.save
            && let Some(current) = self.screen_ui.current().cloned()
        {
            self.screen_ui.status = match screen::save(&current, &dir) {
                Ok(()) => {
                    self.screen_ui.dirty = false;
                    format!("saved {}", current.id)
                }
                Err(err) => err.to_string(),
            };
        }
        if actions.reload {
            let (screens, errors) = screen::load_dir(&dir);
            self.screen_ui.selected = (!screens.is_empty()).then_some(0);
            self.screen_ui.screens = screens;
            self.screen_ui.selected_widget = None;
            self.screen_ui.dirty = false;
            self.screen_ui.status = if errors.is_empty() {
                "reloaded".into()
            } else {
                errors[0].to_string()
            };
        }
        if let Some(fired) = actions.fired {
            self.fire_screen_action(fired);
        }
    }

    /// Carry out what a previewed button does.
    pub(super) fn fire_screen_action(&mut self, action: infengine_adventure::screen::ScreenAction) {
        use infengine_adventure::screen::ScreenAction;

        match action {
            ScreenAction::NewGame => {
                let start = self.game_ui.manifest.start_room.clone();
                self.screen_ui.preview = None;
                if start.is_empty() {
                    self.status = "no starting room set - see the game window".into();
                    return;
                }
                self.play.game.state = Default::default();
                self.enter_room_by_id(&start, None);
                self.start_play();
            }
            ScreenAction::Resume | ScreenAction::Close => {
                self.screen_ui.preview = None;
            }
            ScreenAction::Open { screen } => {
                if !self.screen_ui.show(&screen) {
                    self.status = format!("no screen called {screen}");
                }
            }
            // The editor is not the thing that quits; saying so is more
            // use than doing nothing silently.
            ScreenAction::Quit => self.status = "quit (ignored in the editor)".into(),
            ScreenAction::SetTextLanguage { language } => self.language.text = language,
            ScreenAction::SetVoiceLanguage { language } => self.language.voice = language,
            ScreenAction::RunScript { script } => {
                if !self.play.game.run_script(&script) {
                    self.status = format!("no script called {script}");
                }
            }
            ScreenAction::None => {}
        }
    }
}
