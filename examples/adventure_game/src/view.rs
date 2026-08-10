//! A36.3: fitting a room to the window.
//!
//! A room is authored at a fixed size - 320x144 for Kyrandia, 1280x800
//! for Black Mirror - and the window is whatever the player made it.
//! The picture is scaled up whole and centred, with bars where the
//! proportions do not match.
//!
//! Whole numbers matter for the older games. A 320-wide background
//! stretched by 2.7 is a smeared background; at 2 or 3 it is the
//! picture the artist drew, larger. So integer scaling is preferred and
//! only given up when the room would not fit at 1:1.

use glam::Vec2;

/// Where the room sits inside the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Window size in logical pixels.
    pub viewport: Vec2,
    /// Room size in its own units.
    pub room: Vec2,
    /// How much of the room is on screen at once.
    ///
    /// The whole room, for a room that fits one screen - which is most
    /// of them. A room bigger than its view is one the camera moves
    /// around inside, and this is the window it moves.
    pub visible: Vec2,
    /// Top-left of that window, in room units.
    pub camera: Vec2,
    /// Pixels per room unit.
    pub scale: f32,
    /// Top-left of the drawn room, in window pixels.
    pub origin: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self {
            viewport: Vec2::new(1280.0, 800.0),
            room: Vec2::new(320.0, 200.0),
            // Zero is "all of it", the same as a room with no camera -
            // so a view that nobody configured shows the whole room.
            visible: Vec2::ZERO,
            camera: Vec2::ZERO,
            scale: 1.0,
            origin: Vec2::ZERO,
        }
    }
}

impl View {
    /// Work out the scale and offset for the current sizes.
    ///
    /// What is scaled to the window is the visible part, not the whole
    /// room. Scaling the room instead would shrink a wide one until all
    /// of it fitted, which is the opposite of what a camera is for.
    pub fn fit(&mut self) {
        let visible = self.visible().max(Vec2::ONE);
        let fit = (self.viewport / visible).min_element();
        // Whole steps while it still fits; below 1:1 there is no whole
        // step left and a fractional one beats cropping.
        self.scale = if fit >= 1.0 { fit.floor() } else { fit.max(0.01) };
        let drawn = visible * self.scale;
        self.origin = ((self.viewport - drawn) * 0.5).max(Vec2::ZERO);
        tracing::debug!(viewport = ?self.viewport, ?visible, scale = self.scale, origin = ?self.origin, "view fit");
    }

    /// The window into the room, with zero meaning all of it.
    pub fn visible(self) -> Vec2 {
        let width = if self.visible.x > 0.0 { self.visible.x } else { self.room.x };
        let height = if self.visible.y > 0.0 { self.visible.y } else { self.room.y };
        Vec2::new(width.min(self.room.x), height.min(self.room.y))
    }

    /// Window pixel to room unit.
    pub fn to_room(self, window: Vec2) -> Vec2 {
        self.camera + (window - self.origin) / self.scale.max(f32::EPSILON)
    }

    /// Room unit to window pixel.
    pub fn to_window(self, room: Vec2) -> Vec2 {
        self.origin + (room - self.camera) * self.scale
    }

    /// Is this window point over the picture at all?
    pub fn contains(self, window: Vec2) -> bool {
        let p = self.to_room(window) - self.camera;
        let visible = self.visible();
        p.x >= 0.0 && p.y >= 0.0 && p.x <= visible.x && p.y <= visible.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(viewport: (f32, f32), room: (f32, f32)) -> View {
        let mut v = View {
            viewport: Vec2::new(viewport.0, viewport.1),
            room: Vec2::new(room.0, room.1),
            ..Default::default()
        };
        v.fit();
        v
    }

    #[test]
    fn a_small_room_is_scaled_by_a_whole_number() {
        // 1280/320 is four, 800/144 is five: the smaller wins, and it
        // stays whole so the pixels stay square and sharp.
        let v = view((1280.0, 800.0), (320.0, 144.0));
        assert_eq!(v.scale, 4.0);
    }

    #[test]
    fn an_awkward_window_still_scales_wholly() {
        // 3.6 across, 5.5 down: three, not three point six.
        let v = view((1160.0, 800.0), (320.0, 144.0));
        assert_eq!(v.scale, 3.0);
    }

    #[test]
    fn a_room_too_big_for_the_window_shrinks_by_a_fraction() {
        // There is no whole step below one, and cropping a room the
        // player is meant to click on would be worse.
        let v = view((640.0, 400.0), (1280.0, 800.0));
        assert_eq!(v.scale, 0.5);
    }

    #[test]
    fn the_picture_is_centred() {
        let v = view((1280.0, 800.0), (320.0, 144.0));
        // 320*4 = 1280 across, so no bars; 144*4 = 576 down, leaving 224.
        assert_eq!(v.origin, Vec2::new(0.0, 112.0));
    }

    #[test]
    fn window_and_room_coordinates_are_inverses() {
        let v = view((1280.0, 800.0), (320.0, 144.0));
        for point in [Vec2::ZERO, Vec2::new(160.0, 72.0), Vec2::new(319.0, 143.0)] {
            let back = v.to_room(v.to_window(point));
            assert!(back.abs_diff_eq(point, 1e-3), "{point} -> {back}");
        }
    }

    #[test]
    fn a_click_in_the_bars_is_not_a_click_in_the_room() {
        let v = view((1280.0, 800.0), (320.0, 144.0));
        assert!(v.contains(Vec2::new(640.0, 400.0)));
        assert!(!v.contains(Vec2::new(640.0, 10.0)), "above the picture");
        assert!(!v.contains(Vec2::new(640.0, 790.0)), "below it");
    }

    #[test]
    fn a_room_with_no_size_does_not_divide_by_zero() {
        let v = view((1280.0, 800.0), (0.0, 0.0));
        assert!(v.scale.is_finite() && v.scale > 0.0);
        assert!(v.to_room(Vec2::new(10.0, 10.0)).is_finite());
    }
}
