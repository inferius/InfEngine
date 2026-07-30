//! A2.4: perspective scaling of the actor by depth.
//!
//! A point-and-click room is a flat painting, so depth is faked the way
//! the genre always faked it: the further up the screen the actor's feet
//! are, the smaller the actor is drawn. A room defines one or more
//! vertical ramps mapping feet-y to a scale factor, which lets a single
//! room have a corridor receding at one rate and a staircase at another.

use serde::{Deserialize, Serialize};

/// A linear depth ramp over a band of the room.
///
/// `y_near` is the bottom of the band (closest to the camera, largest
/// scale) and `y_far` the top. The two y values may be given in either
/// order; [`ScaleRamp`] normalises them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScaleZone {
    pub y_near: f32,
    pub y_far: f32,
    pub scale_near: f32,
    pub scale_far: f32,
}

impl Default for ScaleZone {
    fn default() -> Self {
        Self {
            y_near: 0.0,
            y_far: 0.0,
            scale_near: 1.0,
            scale_far: 1.0,
        }
    }
}

impl ScaleZone {
    /// Normalised band as `(top_y, bottom_y)`.
    fn band(&self) -> (f32, f32) {
        (self.y_far.min(self.y_near), self.y_far.max(self.y_near))
    }

    fn contains(&self, y: f32) -> bool {
        let (top, bottom) = self.band();
        y >= top && y <= bottom
    }

    /// Interpolate within the band. `y` outside the band extrapolates,
    /// so callers clamp first.
    fn scale_at(&self, y: f32) -> f32 {
        let span = self.y_near - self.y_far;
        if span.abs() <= f32::EPSILON {
            return self.scale_near;
        }
        let t = (y - self.y_far) / span;
        self.scale_far + (self.scale_near - self.scale_far) * t
    }
}

/// All the depth ramps in a room.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScaleRamp {
    #[serde(default)]
    pub zones: Vec<ScaleZone>,
}

impl ScaleRamp {
    pub fn new(zones: Vec<ScaleZone>) -> Self {
        Self { zones }
    }

    /// A room with no ramps draws the actor at native size everywhere.
    pub fn is_flat(&self) -> bool {
        self.zones.is_empty()
    }

    /// Scale factor for an actor whose feet are at `y`.
    ///
    /// Inside a zone, interpolate. Outside every zone, use the nearest
    /// zone's nearest endpoint rather than extrapolating off to zero or
    /// to a giant - an actor stepping one pixel past the ramp should not
    /// change size discontinuously.
    pub fn scale_at(&self, y: f32) -> f32 {
        if let Some(zone) = self.zones.iter().find(|z| z.contains(y)) {
            return zone.scale_at(y).max(0.0);
        }
        let mut best: Option<(f32, f32)> = None;
        for zone in &self.zones {
            let (top, bottom) = zone.band();
            let (edge, scale) = if y < top {
                (top, zone.scale_at(top))
            } else {
                (bottom, zone.scale_at(bottom))
            };
            let d = (y - edge).abs();
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, scale));
            }
        }
        best.map_or(1.0, |(_, s)| s.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corridor() -> ScaleRamp {
        // Feet at y=400 (front of the room) draw at full size, feet at
        // y=100 (back wall) at 40%.
        ScaleRamp::new(vec![ScaleZone {
            y_near: 400.0,
            y_far: 100.0,
            scale_near: 1.0,
            scale_far: 0.4,
        }])
    }

    #[test]
    fn flat_room_scales_everything_at_one() {
        let ramp = ScaleRamp::default();
        assert!(ramp.is_flat());
        assert_eq!(ramp.scale_at(0.0), 1.0);
        assert_eq!(ramp.scale_at(9999.0), 1.0);
    }

    #[test]
    fn endpoints_hit_their_declared_scales() {
        let ramp = corridor();
        assert!((ramp.scale_at(400.0) - 1.0).abs() < 1e-4);
        assert!((ramp.scale_at(100.0) - 0.4).abs() < 1e-4);
    }

    #[test]
    fn midpoint_interpolates_linearly() {
        let ramp = corridor();
        assert!((ramp.scale_at(250.0) - 0.7).abs() < 1e-4);
    }

    #[test]
    fn outside_the_band_clamps_instead_of_extrapolating() {
        let ramp = corridor();
        // Well past the front of the ramp: stays at the near scale
        // rather than growing without bound.
        assert!((ramp.scale_at(900.0) - 1.0).abs() < 1e-4);
        // Above the back wall: stays at the far scale, never negative.
        assert!((ramp.scale_at(-500.0) - 0.4).abs() < 1e-4);
    }

    #[test]
    fn reversed_y_order_is_accepted() {
        let ramp = ScaleRamp::new(vec![ScaleZone {
            y_near: 100.0,
            y_far: 400.0,
            scale_near: 0.4,
            scale_far: 1.0,
        }]);
        assert!((ramp.scale_at(100.0) - 0.4).abs() < 1e-4);
        assert!((ramp.scale_at(400.0) - 1.0).abs() < 1e-4);
        assert!((ramp.scale_at(250.0) - 0.7).abs() < 1e-4);
    }

    #[test]
    fn zero_height_zone_uses_its_near_scale() {
        let ramp = ScaleRamp::new(vec![ScaleZone {
            y_near: 200.0,
            y_far: 200.0,
            scale_near: 0.6,
            scale_far: 0.9,
        }]);
        assert!((ramp.scale_at(200.0) - 0.6).abs() < 1e-4);
    }

    #[test]
    fn the_zone_containing_y_wins_over_a_nearer_edge() {
        // A staircase ramp overlapping the floor ramp: the containing
        // zone must be picked, not whichever edge happens to be closest.
        let ramp = ScaleRamp::new(vec![
            ScaleZone {
                y_near: 400.0,
                y_far: 300.0,
                scale_near: 1.0,
                scale_far: 0.8,
            },
            ScaleZone {
                y_near: 300.0,
                y_far: 100.0,
                scale_near: 0.8,
                scale_far: 0.2,
            },
        ]);
        assert!((ramp.scale_at(200.0) - 0.5).abs() < 1e-4);
    }
}
