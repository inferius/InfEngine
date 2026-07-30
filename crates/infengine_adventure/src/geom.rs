//! A2.1: polygon primitives shared by walkable areas, hotspots, and
//! exits.
//!
//! Everything here works in *room space*: x grows right, y grows down,
//! origin at the room's top-left corner, one unit = one logical pixel.
//! That matches how a background image is authored, so a hotspot drawn
//! over a pixel in the editor keeps the same coordinates in the file.

use glam::Vec2;

/// Points closer than this are treated as the same point. Room space is
/// in pixels, so a thousandth of a pixel is comfortably below anything
/// an editor can produce.
pub const EPSILON: f32 = 1e-3;

/// Twice the signed area of a polygon. Positive means clockwise winding
/// in room space (y-down), negative means counter-clockwise.
pub fn signed_area_2(points: &[Vec2]) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut acc = 0.0;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        acc += a.x * b.y - b.x * a.y;
    }
    acc
}

/// Area of a polygon, winding-independent.
pub fn area(points: &[Vec2]) -> f32 {
    signed_area_2(points).abs() * 0.5
}

/// Cross product of `b - a` and `c - a`. Sign tells which side of the
/// line `a -> b` the point `c` falls on.
fn cross(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Closest point to `p` on the segment `a -> b`.
pub fn closest_point_on_segment(a: Vec2, b: Vec2, p: Vec2) -> Vec2 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq <= EPSILON * EPSILON {
        return a;
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}

/// Distance from `p` to the segment `a -> b`.
pub fn distance_to_segment(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    closest_point_on_segment(a, b, p).distance(p)
}

/// Is `p` within `tolerance` of the polygon's outline?
///
/// Ray casting is undefined exactly on an edge, so callers that need a
/// stable answer for boundary points check this first.
pub fn on_boundary(points: &[Vec2], p: Vec2, tolerance: f32) -> bool {
    if points.len() < 2 {
        return points.first().is_some_and(|v| v.distance(p) <= tolerance);
    }
    (0..points.len()).any(|i| {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        distance_to_segment(a, b, p) <= tolerance
    })
}

/// Is `p` strictly inside the polygon?
///
/// Crossing-number test. Points exactly on an edge give an arbitrary but
/// deterministic answer; use [`contains_or_boundary`] when the boundary
/// should count as inside.
pub fn contains(points: &[Vec2], p: Vec2) -> bool {
    if points.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[j];
        // Half-open edge rule: an edge counts when it spans p.y from
        // above-inclusive to below-exclusive. This makes a vertex touched
        // by two edges count exactly once.
        if (a.y > p.y) != (b.y > p.y) {
            let t = (p.y - a.y) / (b.y - a.y);
            if p.x < a.x + t * (b.x - a.x) {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// [`contains`], but a point on the outline counts as inside.
pub fn contains_or_boundary(points: &[Vec2], p: Vec2) -> bool {
    contains(points, p) || on_boundary(points, p, EPSILON)
}

/// Closest point on the polygon's outline, and its distance.
///
/// Returns `None` for a degenerate polygon with fewer than two points.
pub fn closest_boundary_point(points: &[Vec2], p: Vec2) -> Option<(Vec2, f32)> {
    if points.len() < 2 {
        return points.first().map(|&v| (v, v.distance(p)));
    }
    let mut best: Option<(Vec2, f32)> = None;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let q = closest_point_on_segment(a, b, p);
        let d = q.distance(p);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((q, d));
        }
    }
    best
}

/// Do segments `a1 -> a2` and `b1 -> b2` cross at a point interior to
/// both?
///
/// Touching endpoints and collinear overlap deliberately return `false`:
/// a path that runs along a wall or pivots around a corner vertex is
/// legal, only a segment that cuts *through* an edge is not.
pub fn segments_properly_intersect(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> bool {
    let d1 = cross(b1, b2, a1);
    let d2 = cross(b1, b2, a2);
    let d3 = cross(a1, a2, b1);
    let d4 = cross(a1, a2, b2);
    // Any near-zero term means an endpoint lies on the other segment's
    // line: a touch or an overlap, not a proper crossing.
    if d1.abs() <= EPSILON || d2.abs() <= EPSILON || d3.abs() <= EPSILON || d4.abs() <= EPSILON {
        return false;
    }
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// Axis-aligned bounds of a point set, as `(min, max)`.
pub fn bounds(points: &[Vec2]) -> Option<(Vec2, Vec2)> {
    let mut iter = points.iter().copied();
    let first = iter.next()?;
    let mut min = first;
    let mut max = first;
    for p in iter {
        min = min.min(p);
        max = max.max(p);
    }
    Some((min, max))
}

/// Build the four corners of an axis-aligned rectangle, clockwise in
/// room space.
pub fn rect_points(min: Vec2, max: Vec2) -> Vec<Vec2> {
    vec![
        min,
        Vec2::new(max.x, min.y),
        max,
        Vec2::new(min.x, max.y),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Vec2> {
        rect_points(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0))
    }

    #[test]
    fn area_is_winding_independent() {
        let cw = square();
        let mut ccw = square();
        ccw.reverse();
        assert!((area(&cw) - 100.0).abs() < 1e-3);
        assert!((area(&ccw) - 100.0).abs() < 1e-3);
        // ...but the signed version flips.
        assert!(signed_area_2(&cw) * signed_area_2(&ccw) < 0.0);
    }

    #[test]
    fn contains_interior_and_rejects_exterior() {
        let sq = square();
        assert!(contains(&sq, Vec2::new(5.0, 5.0)));
        assert!(!contains(&sq, Vec2::new(-1.0, 5.0)));
        assert!(!contains(&sq, Vec2::new(11.0, 5.0)));
        assert!(!contains(&sq, Vec2::new(5.0, -0.5)));
    }

    #[test]
    fn vertex_ray_hit_counts_once() {
        // A ray leaving (5, 0) grazes the top-left and top-right
        // vertices. Without the half-open edge rule this double-counts
        // and reports the point as outside.
        let tri = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(5.0, 10.0),
        ];
        assert!(contains(&tri, Vec2::new(5.0, 5.0)));
        assert!(!contains(&tri, Vec2::new(0.5, 9.0)));
    }

    #[test]
    fn boundary_points_are_detected() {
        let sq = square();
        assert!(on_boundary(&sq, Vec2::new(0.0, 5.0), EPSILON));
        assert!(on_boundary(&sq, Vec2::new(10.0, 10.0), EPSILON));
        assert!(!on_boundary(&sq, Vec2::new(5.0, 5.0), EPSILON));
        assert!(contains_or_boundary(&sq, Vec2::new(0.0, 5.0)));
    }

    #[test]
    fn degenerate_polygons_contain_nothing() {
        assert!(!contains(&[], Vec2::ZERO));
        assert!(!contains(&[Vec2::ZERO], Vec2::ZERO));
        assert!(!contains(&[Vec2::ZERO, Vec2::new(1.0, 1.0)], Vec2::ZERO));
    }

    #[test]
    fn closest_point_on_segment_clamps_to_ends() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(10.0, 0.0);
        assert_eq!(closest_point_on_segment(a, b, Vec2::new(5.0, 5.0)).x, 5.0);
        assert_eq!(closest_point_on_segment(a, b, Vec2::new(-5.0, 0.0)), a);
        assert_eq!(closest_point_on_segment(a, b, Vec2::new(50.0, 0.0)), b);
    }

    #[test]
    fn zero_length_segment_returns_its_own_point() {
        let a = Vec2::new(3.0, 4.0);
        assert_eq!(closest_point_on_segment(a, a, Vec2::new(9.0, 9.0)), a);
    }

    #[test]
    fn closest_boundary_point_projects_onto_nearest_edge() {
        let sq = square();
        let (q, d) = closest_boundary_point(&sq, Vec2::new(5.0, 12.0)).expect("non-degenerate");
        assert!((q.y - 10.0).abs() < 1e-3);
        assert!((d - 2.0).abs() < 1e-3);
    }

    #[test]
    fn proper_intersection_detected() {
        assert!(segments_properly_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
            Vec2::new(10.0, 0.0),
        ));
    }

    #[test]
    fn shared_endpoint_is_not_a_proper_intersection() {
        // Two walk-area edges meeting at a corner: a path pivoting around
        // that corner must stay legal.
        assert!(!segments_properly_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
        ));
    }

    #[test]
    fn collinear_overlap_is_not_a_proper_intersection() {
        // A path running along a wall is legal.
        assert!(!segments_properly_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(8.0, 0.0),
        ));
    }

    #[test]
    fn disjoint_segments_do_not_intersect() {
        assert!(!segments_properly_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(5.0, 5.0),
            Vec2::new(6.0, 5.0),
        ));
    }

    #[test]
    fn bounds_of_empty_set_is_none() {
        assert!(bounds(&[]).is_none());
        let (min, max) = bounds(&square()).expect("non-empty");
        assert_eq!(min, Vec2::new(0.0, 0.0));
        assert_eq!(max, Vec2::new(10.0, 10.0));
    }
}
