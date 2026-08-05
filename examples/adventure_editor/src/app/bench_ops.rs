//! A97: what the editor does with the workbench.
//!
//! Opening an asset, and writing it back. Split out of the glue file
//! because the workbench now holds three kinds of thing and the three
//! save paths are half a screen each - `mod.rs` should read as state
//! plus small verbs, not as the place every feature accumulates.

use glam::Vec2;

use tracing::warn;

use crate::sheet::SheetDraft;

use super::Editor;

impl Editor {
    /// Put an animation on the workbench, in its own workspace.
    ///
    /// The sheet has to be measured first: every sensible frame size
    /// depends on how big the picture is, and a cut drawn against a
    /// guessed size would be a cut drawn against nothing.
    pub(super) fn open_asset(&mut self, id: &str) {
        let Some(animation) = self.project.animations.get(id).cloned() else {
            self.status = format!("no animation called {id}");
            return;
        };
        let Some(size) = self.sheet_size(&animation.image) else {
            self.status = format!("{} did not load", animation.image);
            return;
        };
        self.bench = Some(crate::bench::Bench::Animation(
            crate::asset_editor::AssetEditor::open(animation, size),
        ));
        self.status = format!("editing {id} - Close gives the room back");
    }

    /// Open a loose picture as a brand-new animation, cut by guess.
    pub(super) fn cut_new_asset(&mut self, image: &str) {
        let Some(size) = self.sheet_size(image) else {
            self.status = format!("{image} did not load");
            return;
        };
        let draft = SheetDraft::guess(image, size);
        let mut animation = draft.to_animation();
        // A new id, distinct from anything already indexed, so saving
        // adds rather than overwrites.
        if self.project.animations.get(&animation.id).is_some() {
            animation.id = format!("{}_2", animation.id);
        }
        let id = animation.id.clone();
        let mut editor = crate::asset_editor::AssetEditor::open(animation, size);
        // Nothing has been written yet, so it starts unsaved.
        editor.dirty = true;
        self.bench = Some(crate::bench::Bench::Animation(editor));
        self.status = format!("cutting {image} as {id}");
    }

    /// Upload a sheet and report its size in pixels.
    pub(super) fn sheet_size(&mut self, image: &str) -> Option<Vec2> {
        let handle = self.animation_texture(image)?;
        let (w, h) = self.renderer.as_ref()?.texture_size(handle)?;
        Some(Vec2::new(w as f32, h as f32))
    }

    /// Put a screen on the workbench, laid out at its design size.
    pub(super) fn open_screen_desk(&mut self, id: &str) {
        let Some(screen) = self.screen_ui.screens.iter().find(|s| s.id == id).cloned() else {
            self.status = format!("no screen called {id}");
            return;
        };
        self.bench = Some(crate::bench::Bench::Screen(
            crate::screen_desk::ScreenDesk::open(screen),
        ));
        self.status = format!("laying out {id}");
    }

    /// Put a font on the workbench.
    pub(super) fn open_font_desk(&mut self, id: &str) {
        let Some(font) = self.project.fonts.iter().find(|f| f.id == id).cloned() else {
            self.status = format!("no font called {id}");
            return;
        };
        self.bench = Some(crate::bench::Bench::Font(
            crate::font_desk::FontDesk::open(font),
        ));
        self.status = format!("looking at {id}");
    }

    /// Write whatever is on the workbench back to its file.
    pub(super) fn save_bench(&mut self) {
        match self.bench.as_mut() {
            Some(crate::bench::Bench::Animation(_)) => self.save_asset(),
            Some(crate::bench::Bench::Screen(_)) => self.save_screen_desk(),
            Some(crate::bench::Bench::Font(_)) => self.save_font_desk(),
            None => {}
        }
    }

    /// Write the workbench's screen, and keep the editor's copy in step.
    pub(super) fn save_screen_desk(&mut self) {
        use infengine_adventure::screen;
        let Some(crate::bench::Bench::Screen(desk)) = self.bench.as_mut() else {
            return;
        };
        let screen = desk.screen.clone();
        let id = screen.id.trim().to_string();
        if id.is_empty() {
            return;
        }
        let dir = self.project.root.join(screen::SCREEN_DIR);
        // A rename leaves the old file behind, so it goes first.
        if desk.original_id != id {
            let old = dir.join(format!("{}{}", desk.original_id, screen::SCREEN_SUFFIX));
            let _ = std::fs::remove_file(old);
        }
        match screen::save(&screen, &dir) {
            Ok(()) => {
                let original = desk.original_id.clone();
                desk.dirty = false;
                desk.original_id = id.clone();
                self.screen_ui.screens.retain(|s| s.id != original && s.id != id);
                self.screen_ui.screens.push(screen);
                self.screen_ui.screens.sort_by(|a, b| a.id.cmp(&b.id));
                self.status = format!("saved {id}");
            }
            Err(err) => {
                warn!(%err, "could not save the screen");
                self.status = err.to_string();
            }
        }
    }

    /// Write the workbench's font metrics back beside its atlas.
    pub(super) fn save_font_desk(&mut self) {
        use infengine_adventure::pixfont;
        let Some(crate::bench::Bench::Font(desk)) = self.bench.as_mut() else {
            return;
        };
        let font = desk.font.clone();
        let id = font.id.trim().to_string();
        if id.is_empty() {
            return;
        }
        let dir = self.project.root.join(pixfont::FONT_DIR);
        match pixfont::save(&font, &dir) {
            Ok(()) => {
                let original = desk.original_id.clone();
                desk.dirty = false;
                desk.original_id = id.clone();
                self.project.fonts.retain(|f| f.id != original && f.id != id);
                self.project.fonts.push(font);
                self.project.fonts.sort_by(|a, b| a.id.cmp(&b.id));
                self.status = format!("saved {id}");
            }
            Err(err) => {
                warn!(%err, "could not save the font");
                self.status = err.to_string();
            }
        }
    }

    /// Write the workbench's animation into the project's index.
    pub(super) fn save_asset(&mut self) {
        let Some(crate::bench::Bench::Animation(editor)) = self.bench.as_mut() else {
            return;
        };
        let animation = editor.animation.clone();
        let id = animation.id.trim().to_string();
        if id.is_empty() {
            return;
        }
        // A rename replaces rather than duplicates: the old entry is
        // the thing being renamed, not a sibling of it.
        if editor.original_id != id {
            self.project.animations.remove(&editor.original_id);
        }
        self.project.animations.insert(animation);
        let dir = self.project.root.join(infengine_adventure::anim::ANIMATION_DIR);
        match infengine_adventure::anim::save_index(&self.project.animations, &dir) {
            Ok(()) => {
                if let Some(crate::bench::Bench::Animation(editor)) = self.bench.as_mut() {
                    editor.dirty = false;
                    editor.original_id = id.clone();
                }
                self.collect_animations();
                self.status = format!("saved {id}");
            }
            Err(err) => {
                warn!(%err, "could not write the animation index");
                self.status = err.to_string();
            }
        }
    }

}
