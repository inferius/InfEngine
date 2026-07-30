//! A5.1: turning a walkability bitmap into walk polygons.
//!
//! Two things produce a floor as a *bitmap* rather than as polygons: an
//! imported game that stored its walkable region as a mask, and an
//! author painting one in the editor. Both want the same conversion, and
//! [`WalkArea`] wants polygons, so this module traces the boundary
//! between walkable and blocked pixels and hands back closed loops.
//!
//! The method is boundary following on the integer lattice. Every side
//! of a walkable pixel whose neighbour is blocked contributes one unit
//! edge, oriented so the floor is always on the same side. Chaining
//! those edges yields closed loops: outer loops wind one way, loops
//! around holes the other, so a hole in the middle of a floor is
//! separated from the floor itself by the sign of its area - no
//! point-in-polygon testing needed.
//!
//! Raw loops follow every pixel corner, so a diagonal wall becomes a
//! staircase of hundreds of vertices. Since pathfinding cost grows with
//! the square of the vertex count, the loops are simplified before they
//! are returned.

use std::collections::HashMap;

use glam::Vec2;

use crate::geom;
use crate::walk::{Polygon, WalkArea};

/// Default simplification tolerance, in pixels.
///
/// The value has to sit between two specific numbers. A single-pixel
/// staircase step deviates 0.71 px from the diagonal it approximates, so
/// anything above that collapses the staircase. A genuine right-angle
/// corner deviates 1.41 px from the line across it, so anything below
/// that keeps the corner. One pixel splits them with room on both sides;
/// go higher and small square features round themselves away entirely.
pub const DEFAULT_TOLERANCE: f32 = 1.0;

/// Loops shorter than this enclose at most a pixel or two of floor and
/// are noise from a mask's anti-aliased edge.
const MIN_LOOP_AREA: f32 = 2.0;

/// A lattice point, in integer pixel-corner coordinates.
type Corner = (i32, i32);

/// Build a [`WalkArea`] from a walkability bitmap.
///
/// `walkable[y * width + x]` is true where an actor may stand. The
/// resulting polygons are in the same pixel coordinates as the bitmap.
pub fn trace(walkable: &[bool], width: usize, height: usize, tolerance: f32) -> WalkArea {
    let mut area = WalkArea::default();
    if width == 0 || height == 0 || walkable.len() < width * height {
        return area;
    }

    let at = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            return false;
        }
        walkable[y as usize * width + x as usize]
    };

    // One directed unit edge per exposed pixel side, floor on the left.
    let mut edges: HashMap<Corner, Vec<Corner>> = HashMap::new();
    let mut push = |from: Corner, to: Corner| edges.entry(from).or_default().push(to);

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            if !at(x, y) {
                continue;
            }
            if !at(x - 1, y) {
                push((x, y), (x, y + 1));
            }
            if !at(x, y + 1) {
                push((x, y + 1), (x + 1, y + 1));
            }
            if !at(x + 1, y) {
                push((x + 1, y + 1), (x + 1, y));
            }
            if !at(x, y - 1) {
                push((x + 1, y), (x, y));
            }
        }
    }

    for loop_points in chain_loops(&mut edges) {
        let simplified = simplify(&loop_points, tolerance);
        if simplified.len() < 3 {
            continue;
        }
        if geom::area(&simplified) < MIN_LOOP_AREA {
            continue;
        }
        // Outer boundaries and hole boundaries wind opposite ways, which
        // the sign of the area reports directly. With floor-on-the-left
        // edges in a y-down space, an outer boundary comes out negative.
        if geom::signed_area_2(&simplified) < 0.0 {
            area.solids.push(Polygon::new(simplified));
        } else {
            area.holes.push(Polygon::new(simplified));
        }
    }

    area
}

/// Follow the edge set into closed loops, consuming it.
fn chain_loops(edges: &mut HashMap<Corner, Vec<Corner>>) -> Vec<Vec<Vec2>> {
    let mut starts: Vec<Corner> = edges.keys().copied().collect();
    // Deterministic output regardless of hash order, so an import is
    // reproducible and diffs stay stable.
    starts.sort_unstable();

    let mut loops = Vec::new();
    for start in starts {
        while edges.get(&start).is_some_and(|v| !v.is_empty()) {
            let mut points = Vec::new();
            let mut current = start;
            while let Some(next) = edges.get_mut(&current).and_then(Vec::pop) {
                points.push(Vec2::new(current.0 as f32, current.1 as f32));
                current = next;
                if current == start {
                    break;
                }
            }
            if points.len() >= 3 {
                loops.push(points);
            }
        }
    }
    loops
}

/// Drop vertices that add no shape, within `tolerance`.
///
/// Collinear runs go first, cheaply, then Douglas-Peucker handles the
/// staircases that a pixel grid produces on any non-axis-aligned edge.
pub fn simplify(points: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let coarse = drop_collinear(points);
    if coarse.len() < 4 || tolerance <= 0.0 {
        return coarse;
    }

    // A closed loop has no natural endpoints, so split it at the two
    // most distant vertices and simplify each half. Anchoring on an
    // arbitrary vertex instead would let the algorithm smooth away a
    // genuine corner that happened to land at the seam.
    let (a, b) = extreme_pair(&coarse);
    let (first, second) = if a < b { (a, b) } else { (b, a) };

    let half_one: Vec<Vec2> = coarse[first..=second].to_vec();
    let mut half_two: Vec<Vec2> = coarse[second..].to_vec();
    half_two.extend_from_slice(&coarse[..=first]);

    let mut out = douglas_peucker(&half_one, tolerance);
    out.pop();
    let mut tail = douglas_peucker(&half_two, tolerance);
    tail.pop();
    out.append(&mut tail);
    out
}

/// Indices of the two vertices furthest apart.
fn extreme_pair(points: &[Vec2]) -> (usize, usize) {
    let mut best = (0usize, 0usize);
    let mut best_dist = -1.0;
    // The loop is closed, so comparing every vertex against the first
    // one's furthest partner is enough to find a stable diameter.
    let anchor = points[0];
    let mut far = 0;
    for (i, p) in points.iter().enumerate() {
        let d = anchor.distance_squared(*p);
        if d > best_dist {
            best_dist = d;
            far = i;
        }
    }
    best_dist = -1.0;
    for (i, p) in points.iter().enumerate() {
        let d = points[far].distance_squared(*p);
        if d > best_dist {
            best_dist = d;
            best = (far, i);
        }
    }
    best
}

fn drop_collinear(points: &[Vec2]) -> Vec<Vec2> {
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let prev = points[(i + n - 1) % n];
        let cur = points[i];
        let next = points[(i + 1) % n];
        if geom::distance_to_segment(prev, next, cur) > geom::EPSILON {
            out.push(cur);
        }
    }
    if out.len() < 3 { points.to_vec() } else { out }
}

/// Classic Douglas-Peucker on an open polyline.
fn douglas_peucker(points: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let first = points[0];
    let last = points[points.len() - 1];

    let mut worst = 0.0;
    let mut worst_at = 0;
    for (i, p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let d = geom::distance_to_segment(first, last, *p);
        if d > worst {
            worst = d;
            worst_at = i;
        }
    }

    if worst <= tolerance {
        return vec![first, last];
    }
    let mut left = douglas_peucker(&points[..=worst_at], tolerance);
    let right = douglas_peucker(&points[worst_at..], tolerance);
    left.pop();
    left.extend(right);
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a bitmap from an ASCII picture: `#` walkable, `.` blocked.
    fn bitmap(rows: &[&str]) -> (Vec<bool>, usize, usize) {
        let height = rows.len();
        let width = rows[0].len();
        let mut cells = Vec::with_capacity(width * height);
        for row in rows {
            assert_eq!(row.len(), width, "rows must be the same length");
            cells.extend(row.chars().map(|c| c == '#'));
        }
        (cells, width, height)
    }

    #[test]
    fn an_empty_mask_yields_no_floor() {
        let (cells, w, h) = bitmap(&["....", "....", "...."]);
        let area = trace(&cells, w, h, DEFAULT_TOLERANCE);
        assert!(area.is_empty());
    }

    #[test]
    fn zero_sized_input_is_handled() {
        assert!(trace(&[], 0, 0, 1.0).is_empty());
        assert!(trace(&[true], 4, 4, 1.0).is_empty(), "short buffer");
    }

    #[test]
    fn a_solid_rectangle_becomes_one_polygon_of_four_corners() {
        let (cells, w, h) = bitmap(&[
            "......",
            ".####.",
            ".####.",
            ".####.",
            "......",
        ]);
        let area = trace(&cells, w, h, DEFAULT_TOLERANCE);
        assert_eq!(area.solids.len(), 1);
        assert_eq!(area.holes.len(), 0);
        assert_eq!(area.solids[0].points.len(), 4);
        // The polygon covers the painted pixels and nothing else.
        assert!(area.contains(Vec2::new(2.5, 2.5)));
        assert!(!area.contains(Vec2::new(0.5, 2.5)));
    }

    #[test]
    fn a_hole_is_separated_from_the_floor_around_it() {
        let (cells, w, h) = bitmap(&[
            "........",
            ".######.",
            ".#....#.",
            ".#....#.",
            ".######.",
            "........",
        ]);
        let area = trace(&cells, w, h, DEFAULT_TOLERANCE);
        assert_eq!(area.solids.len(), 1, "one floor");
        assert_eq!(area.holes.len(), 1, "one hole");
        // Standing on the ring is fine, standing in the hole is not.
        assert!(area.contains(Vec2::new(1.5, 1.5)));
        assert!(!area.contains(Vec2::new(3.5, 2.5)));
    }

    #[test]
    fn two_separate_regions_become_two_polygons() {
        let (cells, w, h) = bitmap(&[
            "..........",
            ".##....##.",
            ".##....##.",
            "..........",
        ]);
        let area = trace(&cells, w, h, DEFAULT_TOLERANCE);
        assert_eq!(area.solids.len(), 2);
        assert!(area.contains(Vec2::new(1.5, 1.5)));
        assert!(area.contains(Vec2::new(7.5, 1.5)));
        assert!(!area.contains(Vec2::new(4.5, 1.5)));
    }

    #[test]
    fn a_floor_touching_the_edge_of_the_mask_still_closes() {
        // No blocked border to trace against: the loop has to close on
        // the bitmap bounds.
        let (cells, w, h) = bitmap(&["####", "####"]);
        let area = trace(&cells, w, h, DEFAULT_TOLERANCE);
        assert_eq!(area.solids.len(), 1);
        assert!(area.contains(Vec2::new(2.0, 1.0)));
    }

    #[test]
    fn a_staircase_edge_is_simplified_to_a_diagonal() {
        // A triangular floor: traced raw this is one vertex per pixel
        // step, and pathfinding cost grows with the square of that.
        let (cells, w, h) = bitmap(&[
            "#.........",
            "##........",
            "###.......",
            "####......",
            "#####.....",
            "######....",
            "#######...",
            "########..",
        ]);
        let raw = trace(&cells, w, h, 0.0);
        let simplified = trace(&cells, w, h, DEFAULT_TOLERANCE);
        let raw_points = raw.solids[0].points.len();
        let simple_points = simplified.solids[0].points.len();
        assert!(
            simple_points < raw_points / 2,
            "expected real reduction, got {simple_points} from {raw_points}"
        );
        // The floor is still where it was.
        assert!(simplified.contains(Vec2::new(1.0, 6.0)));
    }

    #[test]
    fn simplification_keeps_the_shape_walkable_where_it_was() {
        let (cells, w, h) = bitmap(&[
            "..........",
            "..######..",
            ".#######..",
            ".########.",
            ".########.",
            "..######..",
            "..........",
        ]);
        let area = trace(&cells, w, h, DEFAULT_TOLERANCE);
        assert_eq!(area.solids.len(), 1);
        // A point well inside stays inside after simplification.
        assert!(area.contains(Vec2::new(4.5, 3.5)));
        assert!(!area.contains(Vec2::new(0.5, 0.5)));
    }

    #[test]
    fn single_pixel_noise_is_dropped() {
        let (cells, w, h) = bitmap(&[
            ".........",
            ".#.......",
            ".........",
            "...####..",
            "...####..",
            ".........",
        ]);
        let area = trace(&cells, w, h, DEFAULT_TOLERANCE);
        assert_eq!(area.solids.len(), 1, "the stray pixel is not a floor");
        assert!(area.contains(Vec2::new(4.5, 3.5)));
    }

    #[test]
    fn tracing_is_deterministic() {
        let (cells, w, h) = bitmap(&[
            "..........",
            ".##....##.",
            ".##....##.",
            "..........",
        ]);
        let a = trace(&cells, w, h, DEFAULT_TOLERANCE);
        let b = trace(&cells, w, h, DEFAULT_TOLERANCE);
        assert_eq!(a, b, "hash iteration order must not leak into output");
    }

    #[test]
    fn simplify_leaves_short_inputs_alone() {
        let pts = vec![Vec2::ZERO, Vec2::new(1.0, 0.0)];
        assert_eq!(simplify(&pts, 1.0), pts);
    }

    #[test]
    fn collinear_points_are_dropped() {
        let pts = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let out = simplify(&pts, DEFAULT_TOLERANCE);
        assert_eq!(out.len(), 4, "the midpoint of a straight edge adds nothing");
    }
}
