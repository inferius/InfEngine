//! A6.1: mapping between room space and the window.
//!
//! Rooms are authored in pixels with the origin at the top-left and y
//! growing downwards, which is how a background image is laid out. The
//! renderer's 2D camera has the origin at the centre of the screen and y
//! growing upwards. Every click and every drawn line crosses that
//! boundary, so the conversion lives in one place rather than being
//! re-derived at each call site with a sign error waiting to happen.

use glam::Vec2;

/// Zoom limits. Below the minimum a room is unreadable; above the
/// maximum a single pixel fills the window and dragging a vertex becomes
/// guesswork.
pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 8.0;

/// How the room is framed in the window.
#[derive(Debug, Clone)]
pub struct View {
    /// Room-space point shown at the centre of the window.
    pub center: Vec2,
    pub zoom: f32,
    /// Window size in logical pixels.
    pub viewport: Vec2,
    /// Room extent, used to frame it on load.
    pub room_size: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
            viewport: Vec2::new(1280.0, 720.0),
            room_size: Vec2::new(320.0, 200.0),
        }
    }
}

impl View {
    /// Frame the whole room, leaving a small margin.
    pub fn fit(&mut self) {
        self.center = self.room_size * 0.5;
        if self.room_size.x <= 0.0 || self.room_size.y <= 0.0 {
            self.zoom = 1.0;
            return;
        }
        let fit = (self.viewport / self.room_size) * 0.92;
        self.zoom = fit.x.min(fit.y).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Room space to renderer world space.
    ///
    /// The renderer's camera sits at the origin looking at a y-up world,
    /// so this only has to flip y; the camera's own zoom and position
    /// handle the rest.
    pub fn room_to_world(&self, room: Vec2) -> Vec2 {
        Vec2::new(room.x, -room.y)
    }

    /// Window position (logical pixels, origin top-left) to room space.
    pub fn window_to_room(&self, window: Vec2) -> Vec2 {
        let from_center = window - self.viewport * 0.5;
        self.center + Vec2::new(from_center.x, from_center.y) / self.zoom
    }

    /// Room space to window position.
    ///
    /// The inverse of [`Self::window_to_room`]. Nothing in the editor
    /// needs it yet - drawing goes through the renderer camera - but it
    /// is what the round-trip test checks the conversion against, and it
    /// is the function any future screen-space UI will reach for.
    #[allow(dead_code)]
    pub fn room_to_window(&self, room: Vec2) -> Vec2 {
        (room - self.center) * self.zoom + self.viewport * 0.5
    }

    /// Zoom about a fixed window position, so the room point under the
    /// cursor stays under the cursor.
    pub fn zoom_at(&mut self, window_pos: Vec2, factor: f32) {
        let anchor = self.window_to_room(window_pos);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.window_to_room(window_pos);
        self.center += anchor - after;
    }

    /// Drag the view by a window-space delta.
    pub fn pan(&mut self, window_delta: Vec2) {
        self.center -= window_delta / self.zoom;
    }

    /// Radius in room units that corresponds to `pixels` on screen, so a
    /// grab handle stays the same size however far you have zoomed in.
    pub fn screen_pixels_in_room(&self, pixels: f32) -> f32 {
        pixels / self.zoom.max(f32::EPSILON)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        View {
            center: Vec2::new(160.0, 100.0),
            zoom: 2.0,
            viewport: Vec2::new(800.0, 600.0),
            room_size: Vec2::new(320.0, 200.0),
        }
    }

    #[test]
    fn window_and_room_conversions_are_inverses() {
        let v = view();
        for probe in [
            Vec2::new(0.0, 0.0),
            Vec2::new(400.0, 300.0),
            Vec2::new(799.0, 12.0),
        ] {
            let room = v.window_to_room(probe);
            let back = v.room_to_window(room);
            assert!(
                back.distance(probe) < 1e-3,
                "{probe:?} -> {room:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn the_view_centre_maps_to_the_window_centre() {
        let v = view();
        let center = v.room_to_window(v.center);
        assert!(center.distance(v.viewport * 0.5) < 1e-3);
    }

    #[test]
    fn room_space_is_y_down_and_world_space_is_y_up() {
        let v = view();
        assert_eq!(v.room_to_world(Vec2::new(10.0, 20.0)), Vec2::new(10.0, -20.0));
    }

    #[test]
    fn fit_frames_the_whole_room() {
        let mut v = view();
        v.fit();
        assert_eq!(v.center, Vec2::new(160.0, 100.0));
        // 800/320 = 2.5, 600/200 = 3.0; the tighter axis wins.
        assert!((v.zoom - 2.5 * 0.92).abs() < 1e-3);
    }

    #[test]
    fn fit_clamps_instead_of_dividing_by_a_zero_room() {
        let mut v = View {
            room_size: Vec2::ZERO,
            ..view()
        };
        v.fit();
        assert_eq!(v.zoom, 1.0, "a zero-sized room must not produce infinity");
    }

    #[test]
    fn fit_respects_the_zoom_limits() {
        let mut v = View {
            room_size: Vec2::new(4.0, 4.0),
            ..view()
        };
        v.fit();
        assert!(v.zoom <= MAX_ZOOM);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor() {
        let mut v = view();
        let cursor = Vec2::new(600.0, 200.0);
        let before = v.window_to_room(cursor);
        v.zoom_at(cursor, 1.7);
        let after = v.window_to_room(cursor);
        assert!(
            before.distance(after) < 1e-3,
            "the room point under the cursor drifted: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn zoom_is_clamped_at_both_ends() {
        let mut v = view();
        v.zoom_at(Vec2::ZERO, 1000.0);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_at(Vec2::ZERO, 0.0001);
        assert_eq!(v.zoom, MIN_ZOOM);
    }

    #[test]
    fn panning_moves_the_room_with_the_cursor() {
        let mut v = view();
        let anchor = v.window_to_room(Vec2::new(400.0, 300.0));
        v.pan(Vec2::new(100.0, 0.0));
        // Dragging right by 100 screen pixels should bring room content
        // that was to the left into view: the centre moves left.
        assert!(v.center.x < anchor.x);
        assert!((v.center.x - (anchor.x - 50.0)).abs() < 1e-3);
    }

    #[test]
    fn grab_handles_shrink_in_room_units_as_you_zoom_in() {
        let mut v = view();
        let at_2x = v.screen_pixels_in_room(8.0);
        v.zoom = 4.0;
        let at_4x = v.screen_pixels_in_room(8.0);
        assert!(at_4x < at_2x);
        assert!((at_2x - 4.0).abs() < 1e-3);
    }
}
