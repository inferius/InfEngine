//! A17.1: animations, and how to play one.
//!
//! An animation is a sheet: its frames laid out left to right in one
//! image. A sheet alone is not enough to play, because nothing in a PNG
//! says where one frame ends and the next begins - so every animation
//! is described here, and the descriptions live in one index file
//! beside the images.
//!
//! ```text
//!   animations/
//!     index.ron            what each sheet contains
//!     bridge_0.png         three frames of river, side by side
//! ```
//!
//! Playback is a pure function of elapsed time rather than a stateful
//! ticker. That keeps scrubbing, pausing and stepping in the editor
//! exactly consistent with what the game will do, since both ask the
//! same question: which frame is showing at time t.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Conventional directory and index file.
pub const ANIMATION_DIR: &str = "animations";
pub const ANIMATION_INDEX: &str = "index.ron";

/// Frames per second an animation runs at when it does not say.
pub const DEFAULT_FPS: f32 = 8.0;

/// One animation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    /// Stable id, e.g. `bridge_0`.
    pub id: String,
    /// Sheet image, relative to the project root.
    pub image: String,
    /// How many frames the sheet holds.
    pub frames: u32,
    /// Size of one frame in pixels.
    pub frame_size: Vec2,
    #[serde(default = "default_fps")]
    pub fps: f32,
    #[serde(default = "default_true")]
    pub looping: bool,
    /// Room this belongs to, when it is a room's own animation rather
    /// than a cutscene or a character.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
}

fn default_fps() -> f32 {
    DEFAULT_FPS
}

fn default_true() -> bool {
    true
}

impl Animation {
    pub fn new(id: impl Into<String>, image: impl Into<String>, frames: u32, frame_size: Vec2) -> Self {
        Self {
            id: id.into(),
            image: image.into(),
            frames: frames.max(1),
            frame_size,
            fps: DEFAULT_FPS,
            looping: true,
            room: None,
        }
    }

    pub fn in_room(mut self, room: impl Into<String>) -> Self {
        self.room = Some(room.into());
        self
    }

    /// How long one loop takes, in seconds.
    pub fn duration(&self) -> f32 {
        if self.fps <= 0.0 {
            return 0.0;
        }
        self.frames as f32 / self.fps
    }

    /// Which frame is showing at `time` seconds.
    ///
    /// A looping animation wraps; a one-shot holds its last frame
    /// rather than vanishing, which is what a door that has finished
    /// opening should look like.
    pub fn frame_at(&self, time: f32) -> u32 {
        if self.frames <= 1 || self.fps <= 0.0 {
            return 0;
        }
        if time <= 0.0 {
            return 0;
        }
        let index = (time * self.fps) as u32;
        if self.looping {
            index % self.frames
        } else {
            index.min(self.frames - 1)
        }
    }

    /// Has a one-shot animation finished by `time`?
    ///
    /// A looping animation is never finished.
    pub fn finished_at(&self, time: f32) -> bool {
        !self.looping && time >= self.duration()
    }

    /// Where frame `index` sits in the sheet, as `(min, max)` in pixels.
    pub fn frame_rect(&self, index: u32) -> (Vec2, Vec2) {
        let index = index.min(self.frames.saturating_sub(1));
        let min = Vec2::new(index as f32 * self.frame_size.x, 0.0);
        (min, min + self.frame_size)
    }

    /// The same rectangle as texture coordinates, given the sheet's
    /// pixel size.
    pub fn frame_uv(&self, index: u32, sheet: Vec2) -> (Vec2, Vec2) {
        let (min, max) = self.frame_rect(index);
        let sheet = sheet.max(Vec2::splat(1.0));
        (min / sheet, max / sheet)
    }

    /// Width the whole sheet should be.
    pub fn sheet_width(&self) -> f32 {
        self.frame_size.x * self.frames as f32
    }
}

/// Every animation a project has.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnimationSet {
    #[serde(default)]
    animations: BTreeMap<String, Animation>,
}

impl AnimationSet {
    pub fn insert(&mut self, animation: Animation) {
        self.animations.insert(animation.id.clone(), animation);
    }

    pub fn get(&self, id: &str) -> Option<&Animation> {
        self.animations.get(id)
    }

    pub fn len(&self) -> usize {
        self.animations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.animations.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Animation> {
        self.animations.values()
    }

    /// Animations belonging to one room, in id order.
    pub fn for_room<'a>(&'a self, room: &'a str) -> impl Iterator<Item = &'a Animation> {
        self.animations
            .values()
            .filter(move |a| a.room.as_deref() == Some(room))
    }

    /// Animations not tied to a room: cutscenes, characters, titles.
    pub fn loose(&self) -> impl Iterator<Item = &Animation> {
        self.animations.values().filter(|a| a.room.is_none())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AnimIoError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: ron::error::SpannedError,
    },
    #[error("failed to serialize the animation index: {source}")]
    Serialize {
        #[source]
        source: ron::Error,
    },
}

/// Write the index into `dir`.
pub fn save_index(set: &AnimationSet, dir: &Path) -> Result<(), AnimIoError> {
    let text = ron::ser::to_string_pretty(
        set,
        ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
    )
    .map_err(|source| AnimIoError::Serialize { source })?;
    fs::create_dir_all(dir).map_err(|source| AnimIoError::Write {
        path: dir.display().to_string(),
        source,
    })?;
    let path = dir.join(ANIMATION_INDEX);
    fs::write(&path, text).map_err(|source| AnimIoError::Write {
        path: path.display().to_string(),
        source,
    })
}

/// Read the index from `dir`.
///
/// A project with no index yields an empty set rather than an error:
/// having no animations yet is a normal state.
pub fn load_index(dir: &Path) -> Result<AnimationSet, AnimIoError> {
    let path = dir.join(ANIMATION_INDEX);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(AnimationSet::default());
    };
    ron::from_str(&text).map_err(|source| AnimIoError::Parse {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn river() -> Animation {
        Animation::new("bridge_0", "animations/bridge_0.png", 3, Vec2::new(56.0, 32.0))
            .in_room("bridge")
    }

    #[test]
    fn a_looping_animation_wraps() {
        let a = river(); // 3 frames at 8 fps
        assert_eq!(a.frame_at(0.0), 0);
        assert_eq!(a.frame_at(0.13), 1);
        assert_eq!(a.frame_at(0.26), 2);
        assert_eq!(a.frame_at(0.38), 0, "back to the start");
        assert!(!a.finished_at(100.0), "a loop never finishes");
    }

    #[test]
    fn a_one_shot_holds_its_last_frame() {
        // A door that has finished opening should stay open, not vanish.
        let mut a = river();
        a.looping = false;
        assert_eq!(a.frame_at(10.0), 2);
        assert!(a.finished_at(10.0));
        assert!(!a.finished_at(0.1));
    }

    #[test]
    fn time_before_the_start_shows_the_first_frame() {
        assert_eq!(river().frame_at(-5.0), 0);
    }

    #[test]
    fn a_single_frame_animation_never_moves() {
        let a = Animation::new("still", "s.png", 1, Vec2::splat(16.0));
        assert_eq!(a.frame_at(0.0), 0);
        assert_eq!(a.frame_at(99.0), 0);
    }

    #[test]
    fn a_zero_frame_rate_does_not_divide_by_zero() {
        let mut a = river();
        a.fps = 0.0;
        assert_eq!(a.frame_at(5.0), 0);
        assert_eq!(a.duration(), 0.0);
    }

    #[test]
    fn frames_are_laid_out_left_to_right() {
        let a = river();
        assert_eq!(a.frame_rect(0), (Vec2::ZERO, Vec2::new(56.0, 32.0)));
        assert_eq!(
            a.frame_rect(2),
            (Vec2::new(112.0, 0.0), Vec2::new(168.0, 32.0))
        );
        assert_eq!(a.sheet_width(), 168.0);
    }

    #[test]
    fn a_frame_past_the_end_clamps_instead_of_reading_off_the_sheet() {
        let a = river();
        assert_eq!(a.frame_rect(99), a.frame_rect(2));
    }

    #[test]
    fn texture_coordinates_span_the_frame() {
        let a = river();
        let (min, max) = a.frame_uv(1, Vec2::new(168.0, 32.0));
        assert!((min.x - 1.0 / 3.0).abs() < 1e-5);
        assert!((max.x - 2.0 / 3.0).abs() < 1e-5);
        assert_eq!(min.y, 0.0);
        assert_eq!(max.y, 1.0);
    }

    #[test]
    fn a_zero_sized_sheet_does_not_divide_by_zero() {
        let (min, max) = river().frame_uv(0, Vec2::ZERO);
        assert!(min.x.is_finite() && max.x.is_finite());
    }

    #[test]
    fn animations_are_grouped_by_room() {
        let mut set = AnimationSet::default();
        set.insert(river());
        set.insert(
            Animation::new("bridge_1", "animations/bridge_1.png", 3, Vec2::splat(40.0))
                .in_room("bridge"),
        );
        set.insert(Animation::new(
            "intro1_westwood",
            "animations/intro1_westwood.png",
            23,
            Vec2::new(320.0, 56.0),
        ));

        assert_eq!(set.len(), 3);
        assert_eq!(set.for_room("bridge").count(), 2);
        assert_eq!(set.loose().count(), 1);
        assert_eq!(set.for_room("nowhere").count(), 0);
    }

    #[test]
    fn the_index_round_trips_through_a_file() {
        let dir = std::env::temp_dir().join("infengine_anim_test");
        let _ = fs::remove_dir_all(&dir);

        let mut set = AnimationSet::default();
        set.insert(river());
        save_index(&set, &dir).expect("save");
        let back = load_index(&dir).expect("load");
        assert_eq!(set, back);
        assert_eq!(back.get("bridge_0").expect("entry").frames, 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_index_loads_empty() {
        let loaded = load_index(Path::new("definitely/not/here")).expect("no error");
        assert!(loaded.is_empty());
    }

    #[test]
    fn a_frame_count_of_zero_is_corrected_to_one() {
        // A sheet always holds at least the frame it shows.
        let a = Animation::new("x", "x.png", 0, Vec2::splat(8.0));
        assert_eq!(a.frames, 1);
    }
}
