//! A17.2: playing an animation in the editor.
//!
//! State is a clock, not a frame counter. Which frame shows is derived
//! from elapsed time by [`infengine_adventure::Animation`], so
//! scrubbing, pausing and stepping all ask the same question the game
//! will ask, and the editor cannot drift out of agreement with it.

use glam::Vec2;
use infengine_adventure::Animation;

/// Where a previewed animation sits, until placing them is a thing the
/// editor does properly.
pub const DEFAULT_PREVIEW: Vec2 = Vec2::new(160.0, 72.0);

/// What the editor is previewing.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    /// Id of the animation being shown, if any.
    pub selected: Option<String>,
    pub playing: bool,
    /// Seconds since the animation started.
    pub time: f32,
    /// Speed multiplier, so a slow animation can be inspected quickly
    /// and a fast one slowly.
    pub speed: f32,
    /// Where the preview is drawn, in room coordinates.
    pub position: Vec2,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            selected: None,
            playing: true,
            time: 0.0,
            speed: 1.0,
            position: DEFAULT_PREVIEW,
        }
    }
}

impl Playback {
    /// Show a different animation, from its first frame.
    pub fn select(&mut self, id: impl Into<String>) {
        let id = id.into();
        if self.selected.as_deref() != Some(id.as_str()) {
            self.time = 0.0;
        }
        self.selected = Some(id);
        self.playing = true;
    }

    pub fn clear(&mut self) {
        self.selected = None;
        self.time = 0.0;
    }

    /// Advance the clock.
    ///
    /// A one-shot stops itself at the end rather than sitting at its
    /// last frame with the clock still running, so the play button
    /// reads correctly.
    pub fn tick(&mut self, delta: f32, animation: Option<&Animation>) {
        if !self.playing {
            return;
        }
        self.time += delta * self.speed.max(0.0);
        if let Some(animation) = animation
            && animation.finished_at(self.time)
        {
            self.time = animation.duration();
            self.playing = false;
        }
    }

    /// Which frame to draw.
    pub fn frame(&self, animation: &Animation) -> u32 {
        animation.frame_at(self.time)
    }

    /// Jump to a frame, which also pauses - dragging the slider while
    /// it plays would fight the clock.
    pub fn scrub_to(&mut self, frame: u32, animation: &Animation) {
        self.playing = false;
        self.time = if animation.fps > 0.0 {
            frame as f32 / animation.fps
        } else {
            0.0
        };
    }

    /// Step one frame, wrapping at the end of a loop.
    pub fn step(&mut self, delta: i32, animation: &Animation) {
        let frames = animation.frames.max(1) as i32;
        let current = self.frame(animation) as i32;
        let next = (current + delta).rem_euclid(frames);
        self.scrub_to(next as u32, animation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn river() -> Animation {
        Animation::new("bridge_0", "a.png", 3, Vec2::new(56.0, 32.0))
    }

    #[test]
    fn ticking_advances_through_the_frames() {
        let a = river(); // 3 frames at 8 fps
        let mut p = Playback::default();
        p.tick(0.13, Some(&a));
        assert_eq!(p.frame(&a), 1);
        p.tick(0.13, Some(&a));
        assert_eq!(p.frame(&a), 2);
    }

    #[test]
    fn a_paused_preview_does_not_move() {
        let a = river();
        let mut p = Playback {
            playing: false,
            ..Default::default()
        };
        p.tick(10.0, Some(&a));
        assert_eq!(p.time, 0.0);
    }

    #[test]
    fn speed_scales_the_clock() {
        let a = river();
        let mut p = Playback {
            speed: 2.0,
            ..Default::default()
        };
        p.tick(0.065, Some(&a));
        assert_eq!(p.frame(&a), 1, "twice as fast reaches frame 1 twice as soon");
    }

    #[test]
    fn a_negative_speed_does_not_run_the_clock_backwards() {
        let a = river();
        let mut p = Playback {
            speed: -4.0,
            ..Default::default()
        };
        p.tick(1.0, Some(&a));
        assert!(p.time >= 0.0);
    }

    #[test]
    fn a_one_shot_stops_itself_at_the_end() {
        // Otherwise the play button keeps claiming it is running.
        let mut a = river();
        a.looping = false;
        let mut p = Playback::default();
        p.tick(10.0, Some(&a));
        assert!(!p.playing);
        assert_eq!(p.frame(&a), 2);
    }

    #[test]
    fn a_loop_keeps_going() {
        let a = river();
        let mut p = Playback::default();
        p.tick(100.0, Some(&a));
        assert!(p.playing);
    }

    #[test]
    fn selecting_a_different_animation_restarts_it() {
        let a = river();
        let mut p = Playback::default();
        p.tick(0.3, Some(&a));
        p.select("other");
        assert_eq!(p.time, 0.0);
    }

    #[test]
    fn reselecting_the_same_animation_does_not_restart_it() {
        let a = river();
        let mut p = Playback::default();
        p.select("bridge_0");
        p.tick(0.2, Some(&a));
        let was = p.time;
        p.select("bridge_0");
        assert_eq!(p.time, was);
    }

    #[test]
    fn scrubbing_pauses_so_the_clock_does_not_fight_the_slider() {
        let a = river();
        let mut p = Playback::default();
        p.scrub_to(2, &a);
        assert!(!p.playing);
        assert_eq!(p.frame(&a), 2);
    }

    #[test]
    fn stepping_wraps_at_both_ends() {
        let a = river();
        let mut p = Playback::default();
        p.step(-1, &a);
        assert_eq!(p.frame(&a), 2, "stepping back from the first frame wraps");
        p.step(1, &a);
        assert_eq!(p.frame(&a), 0);
    }

    #[test]
    fn clearing_forgets_the_selection() {
        let mut p = Playback::default();
        p.select("x");
        p.clear();
        assert!(p.selected.is_none());
        assert_eq!(p.time, 0.0);
    }
}
