//! A97: the workbench - whichever asset is open on it.
//!
//! A94 gave animations a workspace of their own, on the reasoning that
//! an asset is not part of any one room and editing it over one is the
//! confusion. Screens and fonts are the same kind of thing, so the
//! workbench holds any of the three and the rest of the editor only
//! has to know that the centre is taken.

use crate::asset_editor::{self, AssetEditor};
use crate::font_desk::{self, FontDesk};
use crate::screen_desk::{self, ScreenDesk};

/// What is on the workbench.
#[derive(Debug, Clone, PartialEq)]
pub enum Bench {
    Animation(AssetEditor),
    Screen(ScreenDesk),
    Font(FontDesk),
}

impl Bench {
    /// What is being edited, for the status line.
    pub fn what(&self) -> String {
        match self {
            Bench::Animation(desk) => desk.animation.id.clone(),
            Bench::Screen(desk) => desk.screen.id.clone(),
            Bench::Font(desk) => desk.font.id.clone(),
        }
    }

    /// The image the workspace draws, if it needs one loaded.
    pub fn image(&self) -> Option<&str> {
        match self {
            Bench::Animation(desk) => Some(&desk.animation.image),
            Bench::Font(desk) => Some(&desk.font.image),
            // A screen draws many pictures, asked for one at a time.
            Bench::Screen(_) => None,
        }
    }

    /// Whether it holds unsaved work.
    pub fn dirty(&self) -> bool {
        match self {
            Bench::Animation(desk) => desk.dirty,
            Bench::Screen(desk) => desk.dirty,
            Bench::Font(desk) => desk.dirty,
        }
    }

    /// Advance whatever clock the workspace runs.
    pub fn tick(&mut self, delta: f32) {
        if let Bench::Animation(desk) = self {
            desk.tick(delta);
        }
    }
}

/// What a workspace asked the app to do. The same two answers whatever
/// is on the bench, because the app's side of it is the same job.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct BenchActions {
    pub save: bool,
    pub close: bool,
}

/// Draw whichever workspace is open.
pub fn panel(
    ctx: &egui::Context,
    bench: &mut Bench,
    texture: Option<egui::TextureHandle>,
    rooms: &[String],
    screen_ctx: &screen_desk::Context<'_>,
) -> BenchActions {
    match bench {
        Bench::Animation(desk) => {
            let asked = asset_editor::panel(ctx, desk, texture, rooms);
            BenchActions {
                save: asked.save,
                close: asked.close,
            }
        }
        Bench::Screen(desk) => {
            let asked = screen_desk::panel(ctx, desk, screen_ctx);
            BenchActions {
                save: asked.save,
                close: asked.close,
            }
        }
        Bench::Font(desk) => {
            let asked = font_desk::panel(ctx, desk, texture);
            BenchActions {
                save: asked.save,
                close: asked.close,
            }
        }
    }
}
