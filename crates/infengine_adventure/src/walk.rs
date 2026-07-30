//! A2.2: the walkable floor of a room.
//!
//! A room's floor is one or more *solid* polygons the actor may stand
//! on, minus any number of *hole* polygons punched out of them (a table
//! in the middle of the floor, a fountain, a pit). This is the classic
//! adventure-game walkbox, generalised from convex boxes to arbitrary
//! simple polygons so the editor can trace an irregular floor by hand
//! instead of tiling it with rectangles.
//!
//! Boundaries are walkable. An actor is allowed to stand exactly on the
//! edge of the floor or hug the rim of a hole; only the strict interior
//! of a hole is off limits. That keeps [`WalkArea::clamp`] total: the
//! point it snaps to is always itself walkable.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::geom;

/// A closed polygon. The final point implicitly connects back to the
/// first, so a triangle stores three points, not four.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub points: Vec<Vec2>,
}

impl Polygon {
    pub fn new(points: Vec<Vec2>) -> Self {
        Self { points }
    }

    /// Axis-aligned rectangle from opposite corners.
    pub fn rect(min: Vec2, max: Vec2) -> Self {
        Self::new(geom::rect_points(min, max))
    }

    /// A polygon needs three points to enclose any area.
    pub fn is_degenerate(&self) -> bool {
        self.points.len() < 3 || geom::area(&self.points) <= geom::EPSILON
    }

    pub fn contains(&self, p: Vec2) -> bool {
        geom::contains(&self.points, p)
    }

    pub fn contains_or_boundary(&self, p: Vec2) -> bool {
        geom::contains_or_boundary(&self.points, p)
    }

    /// Iterate the polygon's edges as `(start, end)` pairs.
    pub fn edges(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        let n = self.points.len();
        (0..n).map(move |i| (self.points[i], self.points[(i + 1) % n]))
    }
}

/// The floor of a room: solids minus holes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WalkArea {
    /// Regions the actor may stand on.
    #[serde(default)]
    pub solids: Vec<Polygon>,
    /// Regions punched out of the solids.
    #[serde(default)]
    pub holes: Vec<Polygon>,
}

impl WalkArea {
    pub fn new(solids: Vec<Polygon>, holes: Vec<Polygon>) -> Self {
        Self { solids, holes }
    }

    /// A room with no floor at all: every point is unwalkable.
    pub fn is_empty(&self) -> bool {
        self.solids.iter().all(Polygon::is_degenerate)
    }

    /// May the actor stand at `p`?
    ///
    /// Inside (or on the rim of) a solid, and not strictly inside a hole.
    pub fn contains(&self, p: Vec2) -> bool {
        if !self.solids.iter().any(|s| s.contains_or_boundary(p)) {
            return false;
        }
        // Strictly inside, so the rim itself stays walkable - an actor
        // may stand against the table. This is also what keeps
        // `clamp` total: the rim it snaps to is a legal destination.
        !self.holes.iter().any(|h| {
            h.contains(p) && !geom::on_boundary(&h.points, p, geom::EPSILON)
        })
    }

    /// Nearest walkable point to `p`.
    ///
    /// Returns `p` unchanged when it is already walkable, and `None` only
    /// when the room has no floor to snap to. Used for click targets that
    /// land on a wall: the actor walks to the closest reachable spot
    /// rather than refusing to move.
    pub fn clamp(&self, p: Vec2) -> Option<Vec2> {
        if self.contains(p) {
            return Some(p);
        }
        let mut best: Option<(Vec2, f32)> = None;
        let candidates = self.solids.iter().chain(self.holes.iter());
        for poly in candidates {
            let Some((q, d)) = geom::closest_boundary_point(&poly.points, p) else {
                continue;
            };
            // A hole's rim borders both the hole and the floor around it;
            // only keep it if it really is standable.
            if !self.contains(q) {
                continue;
            }
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((q, d));
            }
        }
        best.map(|(q, _)| q)
    }

    /// Every edge of every polygon, solids and holes alike. Pathfinding
    /// treats both as walls.
    pub fn edges(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        self.solids
            .iter()
            .chain(self.holes.iter())
            .flat_map(Polygon::edges)
    }

    /// Every vertex of every polygon. These are the turning points a
    /// path may pivot around.
    pub fn vertices(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.solids
            .iter()
            .chain(self.holes.iter())
            .flat_map(|p| p.points.iter().copied())
    }

    /// Can the actor walk the straight line from `a` to `b` without
    /// leaving the floor?
    ///
    /// A segment is clear when it crosses no wall and its midpoint is
    /// walkable. The midpoint check is what rejects a segment that spans
    /// a hole by entering and leaving through shared vertices, where no
    /// *proper* crossing occurs.
    pub fn segment_is_clear(&self, a: Vec2, b: Vec2) -> bool {
        if self.edges().any(|(e0, e1)| {
            geom::segments_properly_intersect(a, b, e0, e1)
        }) {
            return false;
        }
        self.contains((a + b) * 0.5) && self.contains(a) && self.contains(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 100x100 floor with a 20x20 hole in the middle.
    fn floor_with_hole() -> WalkArea {
        WalkArea::new(
            vec![Polygon::rect(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0))],
            vec![Polygon::rect(Vec2::new(40.0, 40.0), Vec2::new(60.0, 60.0))],
        )
    }

    #[test]
    fn interior_is_walkable_and_hole_is_not() {
        let area = floor_with_hole();
        assert!(area.contains(Vec2::new(10.0, 10.0)));
        assert!(!area.contains(Vec2::new(50.0, 50.0)));
        assert!(!area.contains(Vec2::new(150.0, 10.0)));
    }

    #[test]
    fn rims_are_walkable() {
        let area = floor_with_hole();
        // Outer edge of the floor.
        assert!(area.contains(Vec2::new(0.0, 50.0)));
        // Rim of the hole: standing against the table is fine.
        assert!(area.contains(Vec2::new(40.0, 50.0)));
    }

    #[test]
    fn clamp_returns_input_when_already_walkable() {
        let area = floor_with_hole();
        let p = Vec2::new(10.0, 10.0);
        assert_eq!(area.clamp(p), Some(p));
    }

    #[test]
    fn clamp_snaps_outside_click_onto_the_floor() {
        let area = floor_with_hole();
        let q = area.clamp(Vec2::new(-30.0, 50.0)).expect("floor exists");
        assert!(area.contains(q));
        assert!((q.x - 0.0).abs() < 1e-3);
        assert!((q.y - 50.0).abs() < 1e-3);
    }

    #[test]
    fn clamp_pushes_a_click_inside_the_hole_out_to_its_rim() {
        let area = floor_with_hole();
        // Just inside the hole, nearest to its left wall.
        let q = area.clamp(Vec2::new(42.0, 50.0)).expect("floor exists");
        assert!(area.contains(q), "clamped point must itself be walkable");
        assert!((q.x - 40.0).abs() < 1e-3);
    }

    #[test]
    fn clamp_on_a_floorless_room_is_none() {
        let area = WalkArea::default();
        assert!(area.clamp(Vec2::new(5.0, 5.0)).is_none());
        assert!(area.is_empty());
    }

    #[test]
    fn straight_line_across_open_floor_is_clear() {
        let area = floor_with_hole();
        assert!(area.segment_is_clear(Vec2::new(5.0, 5.0), Vec2::new(95.0, 5.0)));
    }

    #[test]
    fn straight_line_through_the_hole_is_blocked() {
        let area = floor_with_hole();
        assert!(!area.segment_is_clear(Vec2::new(10.0, 50.0), Vec2::new(90.0, 50.0)));
    }

    #[test]
    fn line_leaving_the_floor_is_blocked() {
        let area = floor_with_hole();
        assert!(!area.segment_is_clear(Vec2::new(50.0, 10.0), Vec2::new(150.0, 10.0)));
    }

    #[test]
    fn line_grazing_a_hole_corner_diagonally_is_blocked() {
        // Enters at (40,40) and leaves at (60,60): both are hole
        // vertices, so no proper crossing happens and only the midpoint
        // test catches it.
        let area = floor_with_hole();
        assert!(!area.segment_is_clear(Vec2::new(30.0, 30.0), Vec2::new(70.0, 70.0)));
    }

    #[test]
    fn line_running_along_a_wall_is_clear() {
        let area = floor_with_hole();
        assert!(area.segment_is_clear(Vec2::new(0.0, 10.0), Vec2::new(0.0, 90.0)));
    }

    #[test]
    fn degenerate_polygon_detection() {
        assert!(Polygon::default().is_degenerate());
        assert!(Polygon::new(vec![Vec2::ZERO, Vec2::new(1.0, 1.0)]).is_degenerate());
        // Three collinear points enclose no area.
        assert!(Polygon::new(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 0.0),
        ])
        .is_degenerate());
        assert!(!Polygon::rect(Vec2::ZERO, Vec2::new(10.0, 10.0)).is_degenerate());
    }

    #[test]
    fn edges_close_the_loop() {
        let poly = Polygon::rect(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let edges: Vec<_> = poly.edges().collect();
        assert_eq!(edges.len(), 4);
        assert_eq!(edges[3].1, edges[0].0, "last edge returns to the start");
    }
}
