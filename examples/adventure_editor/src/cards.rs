//! A62: the scene list gets faces.
//!
//! A list of bare names answers "what is this room called" - the wrong
//! question. An author scanning the panel is asking "which one is the
//! bridge with the troll", and the answer to that is a picture. So every
//! scene in the list gets a card: a thumbnail of its background, how
//! much there is to click, and whether anything is scripted to happen.
//!
//! Two hundred rooms means two hundred images, which cannot all be read
//! the first frame without a freeze. The cache loads a few per frame in
//! the order the list asked for them, so the panel fills in over a
//! second while staying responsive - the same trick every photo browser
//! plays.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use infengine_adventure::Project;

/// Thumbnail width in points; height follows the image's own ratio.
pub const THUMB: f32 = 56.0;

/// What one scene shows on its card.
#[derive(Default)]
pub struct Card {
    pub thumb: Option<egui::TextureHandle>,
    pub things: usize,
    pub doors: usize,
    /// Whether an entry script exists for this scene's family.
    pub scripted: bool,
}

/// The cache: cards by room label, and the queue of ones still owed.
#[derive(Default)]
pub struct Cards {
    by_room: HashMap<String, Card>,
    wanted: VecDeque<(String, PathBuf)>,
    queued: HashSet<String>,
}

impl Cards {
    pub fn get(&self, label: &str) -> Option<&Card> {
        self.by_room.get(label)
    }

    /// Ask for a card. Queued once; answered by a later [`Cards::tick`].
    pub fn request(&mut self, label: &str, path: &Path) {
        if self.by_room.contains_key(label) || self.queued.contains(label) {
            return;
        }
        self.queued.insert(label.to_string());
        self.wanted.push_back((label.to_string(), path.to_path_buf()));
    }

    /// Drop a card so it is rebuilt - the room was just saved and its
    /// counts, or even its background, may have changed.
    pub fn forget(&mut self, label: &str) {
        self.by_room.remove(label);
        self.queued.remove(label);
    }

    /// Load up to `budget` queued cards. Called once per frame; the
    /// budget is what keeps the first opening of a big project smooth.
    pub fn tick(&mut self, ctx: &egui::Context, project: &Project, budget: usize) {
        for _ in 0..budget {
            let Some((label, path)) = self.wanted.pop_front() else {
                return;
            };
            self.queued.remove(&label);
            let card = load(ctx, project, &label, &path);
            self.by_room.insert(label, card);
        }
        if !self.wanted.is_empty() {
            // More to do next frame, even if the mouse holds still.
            ctx.request_repaint();
        }
    }
}

/// Build one card from the room file on disk.
fn load(ctx: &egui::Context, project: &Project, label: &str, path: &Path) -> Card {
    let Ok(room) = infengine_adventure::load(path) else {
        // A room that does not parse still deserves a row - the list is
        // where the author goes to find it and fix it.
        return Card::default();
    };
    let scripted = project
        .scripts
        .contains_key(&format!("{}.entry", room.script_prefix()));
    let thumb = room
        .layers
        .first()
        .and_then(|layer| thumbnail(ctx, project, label, &layer.image));
    Card {
        thumb,
        things: room.hotspots.len(),
        doors: room.exits.len(),
        scripted,
    }
}

/// Read an image and shrink it to card size.
///
/// Loaded at twice the display width so it stays crisp on a scaled-up
/// interface, which for a pixel-art background matters.
fn thumbnail(
    ctx: &egui::Context,
    project: &Project,
    label: &str,
    image: &str,
) -> Option<egui::TextureHandle> {
    let path = project.resolve(image);
    let small = image::open(path).ok()?.thumbnail(THUMB as u32 * 2, 128);
    let rgba = small.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(format!("card:{label}"), pixels, egui::TextureOptions::LINEAR))
}

/// The one-line summary under a scene's name.
pub fn caption(card: &Card) -> String {
    let mut out = format!("{} things \u{b7} {} doors", card.things, card.doors);
    if card.scripted {
        out.push_str(" \u{b7} script");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caption_reads_like_a_summary() {
        let card = Card {
            things: 3,
            doors: 2,
            ..Default::default()
        };
        assert_eq!(caption(&card), "3 things \u{b7} 2 doors");
    }

    #[test]
    fn a_scripted_scene_says_so() {
        let card = Card {
            scripted: true,
            ..Default::default()
        };
        assert!(caption(&card).ends_with("script"));
    }
}
