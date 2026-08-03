//! A21.2: turning a loose image into an animation.
//!
//! An imported game hands over sprite sheets as flat pictures. Nothing
//! in the file says where one frame ends and the next begins - that
//! lived in the original's code, and for the pages this importer cannot
//! decode yet it is not coming back. So the editor has to be the place
//! where a sheet is cut up, and this is the state of that cut.
//!
//! The guess below is a starting point and nothing more. It is honest
//! about that: it never invents a frame count a sheet cannot hold, and
//! anything it proposes is meant to be dragged into place by eye.

use glam::Vec2;
use infengine_adventure::Animation;

/// A sheet being cut into frames.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetDraft {
    /// Project-relative path of the image.
    pub image: String,
    /// Id the animation will get.
    pub id: String,
    pub frame_size: Vec2,
    /// Pixels between neighbouring frames, both axes.
    pub gap: Vec2,
    /// Frames per row; zero means one long row.
    pub columns: u32,
    pub frames: u32,
    pub fps: f32,
    pub looping: bool,
    /// Room to tie it to, or none for a sheet usable anywhere.
    pub room: Option<String>,
    /// Size of the image, so the frame count can be kept believable.
    pub sheet_size: Vec2,
}

impl SheetDraft {
    /// A first cut at an unseen sheet.
    pub fn guess(image: &str, sheet_size: Vec2) -> Self {
        let (frame_size, frames) = guess_frames(sheet_size);
        Self {
            id: default_id(image),
            image: image.to_string(),
            frame_size,
            gap: Vec2::ZERO,
            columns: 0,
            frames,
            fps: 8.0,
            looping: true,
            room: None,
            sheet_size,
        }
    }

    /// How many frames of this size the sheet can actually hold.
    ///
    /// The editor clamps to it rather than letting a slider promise
    /// frames that would sample past the right edge and come out as
    /// stretched garbage.
    pub fn max_frames(&self) -> u32 {
        if self.frame_size.x <= 0.0 || self.frame_size.y <= 0.0 {
            return 1;
        }
        // With a gap, N frames span N sizes and N-1 gaps - the last
        // frame does not need a gap after it.
        let fit = |space: f32, size: f32, gap: f32| {
            (((space + gap) / (size + gap)).floor() as u32).max(1)
        };
        let per_row = fit(self.sheet_size.x, self.frame_size.x, self.gap.x);
        if self.columns == 0 {
            return per_row;
        }
        let columns = self.columns.min(per_row);
        let rows = fit(self.sheet_size.y, self.frame_size.y, self.gap.y);
        (columns * rows).max(1)
    }

    /// The animation this draft describes.
    pub fn to_animation(&self) -> Animation {
        let mut animation = Animation::new(
            self.id.trim(),
            &self.image,
            self.frames.clamp(1, self.max_frames()),
            self.frame_size,
        );
        animation.gap = self.gap;
        animation.columns = self.columns;
        animation.fps = self.fps;
        animation.looping = self.looping;
        animation.room = self.room.clone();
        animation
    }

    /// Is this ready to be added to the project?
    pub fn is_valid(&self) -> bool {
        !self.id.trim().is_empty()
            && self.frame_size.x >= 1.0
            && self.frame_size.y >= 1.0
            && self.frame_size.x <= self.sheet_size.x
            && self.frame_size.y <= self.sheet_size.y
    }
}

/// Id for a sheet, from its file name.
fn default_id(image: &str) -> String {
    image
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(image)
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(image)
        .to_string()
}

/// Narrowest frame the guess will propose, in pixels.
///
/// Without a floor, a sheet whose width is prime picks the divisor 1 and
/// proposes as many one-pixel frames as it is wide, which is arithmetic
/// rather than a suggestion.
const MIN_FRAME_WIDTH: u32 = 8;

/// Propose a frame width for a sheet.
///
/// Frames on these sheets sit in a row, so a frame width has to divide
/// the sheet width exactly - anything else drifts and the last frame
/// runs off the edge. Among the exact divisors the one nearest the
/// sheet's height wins, because a frame of a walking character is
/// roughly as wide as it is tall.
fn guess_frames(sheet: Vec2) -> (Vec2, u32) {
    let width = sheet.x.max(1.0) as u32;
    let height = sheet.y.max(1.0) as u32;

    let best = (1..=width)
        .filter(|candidate| width.is_multiple_of(*candidate))
        .filter(|candidate| *candidate >= MIN_FRAME_WIDTH.min(width))
        .min_by_key(|candidate| candidate.abs_diff(height));

    match best {
        Some(frame_width) => (
            Vec2::new(frame_width as f32, height as f32),
            (width / frame_width).max(1),
        ),
        None => (sheet, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gap_and_a_grid_change_how_many_frames_fit() {
        let mut draft = SheetDraft::guess("s.png", Vec2::new(100.0, 50.0));
        draft.frame_size = Vec2::new(20.0, 20.0);
        draft.gap = Vec2::new(5.0, 5.0);
        // One row: 4 frames of 20 with 5 between span 95 <= 100.
        assert_eq!(draft.max_frames(), 4);
        draft.columns = 4;
        // Two rows of 20 with 5 between span 45 <= 50.
        assert_eq!(draft.max_frames(), 8);
        let animation = draft.to_animation();
        assert_eq!(animation.gap, Vec2::new(5.0, 5.0));
        assert_eq!(animation.columns, 4);
    }

    #[test]
    fn a_square_sheet_is_one_square_frame() {
        let (size, frames) = guess_frames(Vec2::new(32.0, 32.0));
        assert_eq!(size, Vec2::new(32.0, 32.0));
        assert_eq!(frames, 1);
    }

    #[test]
    fn a_strip_is_cut_into_frames_about_as_wide_as_it_is_tall() {
        let (size, frames) = guess_frames(Vec2::new(160.0, 32.0));
        assert_eq!(size, Vec2::new(32.0, 32.0));
        assert_eq!(frames, 5);
    }

    #[test]
    fn the_guess_always_divides_the_sheet_exactly() {
        // A width that is prime has no cut but the whole thing, and the
        // guess must say so rather than proposing ninety-seven frames
        // one pixel wide.
        let (size, frames) = guess_frames(Vec2::new(97.0, 40.0));
        assert_eq!(frames, 1);
        assert_eq!(size.x, 97.0);
    }

    #[test]
    fn an_id_comes_from_the_file_name() {
        assert_eq!(default_id("sprites/brandon.png"), "brandon");
        assert_eq!(default_id("brandon.walk.png"), "brandon.walk");
        assert_eq!(default_id("noextension"), "noextension");
    }

    #[test]
    fn a_draft_cannot_promise_more_frames_than_the_sheet_holds() {
        let mut draft = SheetDraft::guess("sprites/strip.png", Vec2::new(160.0, 32.0));
        draft.frames = 99;
        assert_eq!(draft.max_frames(), 5);
        assert_eq!(draft.to_animation().frames, 5, "clamped, not trusted");
    }

    #[test]
    fn a_frame_larger_than_its_sheet_is_not_valid() {
        let mut draft = SheetDraft::guess("sprites/strip.png", Vec2::new(160.0, 32.0));
        assert!(draft.is_valid());
        draft.frame_size = Vec2::new(200.0, 32.0);
        assert!(!draft.is_valid());
        draft.frame_size = Vec2::new(32.0, 32.0);
        draft.id = "  ".into();
        assert!(!draft.is_valid(), "a nameless animation cannot be indexed");
    }

    #[test]
    fn a_draft_becomes_the_animation_it_describes() {
        let mut draft = SheetDraft::guess("sprites/strip.png", Vec2::new(160.0, 32.0));
        draft.fps = 12.0;
        draft.looping = false;
        draft.room = Some("hall".into());
        let animation = draft.to_animation();
        assert_eq!(animation.id, "strip");
        assert_eq!(animation.image, "sprites/strip.png");
        assert_eq!(animation.frames, 5);
        assert_eq!(animation.fps, 12.0);
        assert!(!animation.looping);
        assert_eq!(animation.room.as_deref(), Some("hall"));
    }
}
