//! A2.3: walking a straight-ish path across a room floor.
//!
//! Adventure games do not need general navmesh machinery. The floor is a
//! handful of polygons, the actor is a point, and players expect the
//! shortest visually sensible route - the same thing SCUMM and Sierra
//! games did with walkboxes.
//!
//! The approach here is a visibility graph: the only places a path can
//! usefully turn are the corners of the floor and of its holes, so the
//! nodes are the start, the goal, and every polygon vertex. Two nodes
//! are connected when the straight line between them stays on the floor.
//! A* over that graph gives the true shortest path, not an approximation
//! snapped to a grid.
//!
//! Rooms have tens of vertices, not thousands, so building the graph
//! per request costs less than keeping one in sync with an editor that
//! is dragging those vertices around.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use glam::Vec2;

use crate::walk::WalkArea;

/// Why a walk request produced no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// The room has no walkable floor at all.
    NoFloor,
    /// Start and goal are both on the floor but no route connects them,
    /// e.g. two solids that do not touch.
    Unreachable,
}

/// Node priority for the A* frontier. `BinaryHeap` is a max-heap, so the
/// ordering is deliberately reversed to pop the cheapest node first.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frontier {
    estimate: f32,
    node: usize,
}

impl Eq for Frontier {}

impl Ord for Frontier {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed, and NaN-safe: an unorderable estimate sorts last
        // rather than panicking inside the heap.
        other
            .estimate
            .partial_cmp(&self.estimate)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Frontier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Index of the start and goal nodes in the graph built by [`find_path`].
const START: usize = 0;
const GOAL: usize = 1;

/// Find a walkable route from `start` to `goal`.
///
/// Both endpoints are snapped onto the floor first, so a click on a wall
/// walks to the nearest reachable spot instead of failing. The returned
/// path always begins with the snapped start and ends with the snapped
/// goal; a clear line of sight yields just those two points.
pub fn find_path(area: &WalkArea, start: Vec2, goal: Vec2) -> Result<Vec<Vec2>, PathError> {
    let (Some(start), Some(goal)) = (area.clamp(start), area.clamp(goal)) else {
        return Err(PathError::NoFloor);
    };

    if area.segment_is_clear(start, goal) {
        return Ok(vec![start, goal]);
    }

    // Node 0 is the start, node 1 the goal, the rest are floor corners.
    let mut nodes = Vec::with_capacity(16);
    nodes.push(start);
    nodes.push(goal);
    nodes.extend(area.vertices());

    let n = nodes.len();
    // Adjacency as a flat matrix of edge costs; f32::INFINITY means the
    // straight line between those two corners leaves the floor.
    let mut cost = vec![f32::INFINITY; n * n];
    for i in 0..n {
        for j in (i + 1)..n {
            if area.segment_is_clear(nodes[i], nodes[j]) {
                let d = nodes[i].distance(nodes[j]);
                cost[i * n + j] = d;
                cost[j * n + i] = d;
            }
        }
    }

    let mut g = vec![f32::INFINITY; n];
    let mut came_from = vec![usize::MAX; n];
    let mut closed = vec![false; n];
    g[START] = 0.0;

    let mut open = BinaryHeap::new();
    open.push(Frontier {
        estimate: start.distance(goal),
        node: START,
    });

    while let Some(Frontier { node, .. }) = open.pop() {
        if node == GOAL {
            return Ok(reconstruct(&nodes, &came_from, GOAL));
        }
        if closed[node] {
            continue;
        }
        closed[node] = true;

        for next in 0..n {
            let step = cost[node * n + next];
            if !step.is_finite() || closed[next] {
                continue;
            }
            let tentative = g[node] + step;
            if tentative < g[next] {
                g[next] = tentative;
                came_from[next] = node;
                open.push(Frontier {
                    estimate: tentative + nodes[next].distance(goal),
                    node: next,
                });
            }
        }
    }

    Err(PathError::Unreachable)
}

fn reconstruct(nodes: &[Vec2], came_from: &[usize], mut node: usize) -> Vec<Vec2> {
    let mut path = vec![nodes[node]];
    while came_from[node] != usize::MAX {
        node = came_from[node];
        path.push(nodes[node]);
    }
    path.reverse();
    path
}

/// Total walking distance of a path, for pacing an actor's animation.
pub fn path_length(path: &[Vec2]) -> f32 {
    path.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Position reached after walking `distance` along `path`.
///
/// Returns the final point once the path runs out, so an actor that has
/// arrived simply stays put.
pub fn point_along(path: &[Vec2], distance: f32) -> Option<Vec2> {
    let last = *path.last()?;
    if distance <= 0.0 {
        return path.first().copied();
    }
    let mut remaining = distance;
    for w in path.windows(2) {
        let seg = w[0].distance(w[1]);
        if remaining <= seg {
            let t = if seg > 0.0 { remaining / seg } else { 0.0 };
            return Some(w[0].lerp(w[1], t));
        }
        remaining -= seg;
    }
    Some(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::{Polygon, WalkArea};

    fn open_floor() -> WalkArea {
        WalkArea::new(
            vec![Polygon::rect(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0))],
            Vec::new(),
        )
    }

    fn floor_with_hole() -> WalkArea {
        WalkArea::new(
            vec![Polygon::rect(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0))],
            vec![Polygon::rect(Vec2::new(40.0, 20.0), Vec2::new(60.0, 80.0))],
        )
    }

    fn assert_path_is_walkable(area: &WalkArea, path: &[Vec2]) {
        for w in path.windows(2) {
            assert!(
                area.segment_is_clear(w[0], w[1]),
                "leg {:?} -> {:?} leaves the floor",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn clear_line_of_sight_is_two_points() {
        let area = open_floor();
        let path = find_path(&area, Vec2::new(10.0, 10.0), Vec2::new(90.0, 90.0)).expect("path");
        assert_eq!(path.len(), 2);
        assert_eq!(path[0], Vec2::new(10.0, 10.0));
        assert_eq!(path[1], Vec2::new(90.0, 90.0));
    }

    #[test]
    fn path_routes_around_an_obstacle() {
        let area = floor_with_hole();
        let start = Vec2::new(10.0, 50.0);
        let goal = Vec2::new(90.0, 50.0);
        let path = find_path(&area, start, goal).expect("path");
        assert!(path.len() > 2, "a straight line would cross the obstacle");
        assert_eq!(path[0], start);
        assert_eq!(*path.last().expect("non-empty"), goal);
        assert_path_is_walkable(&area, &path);
    }

    #[test]
    fn path_around_obstacle_takes_the_shorter_side() {
        // The hole spans y 20..80 in a 0..100 room, so going over the top
        // (20 units of clearance) is shorter than under the bottom (20
        // as well) - make it asymmetric to have a definite answer.
        let area = WalkArea::new(
            vec![Polygon::rect(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0))],
            vec![Polygon::rect(Vec2::new(40.0, 10.0), Vec2::new(60.0, 90.0))],
        );
        let path =
            find_path(&area, Vec2::new(10.0, 95.0), Vec2::new(90.0, 95.0)).expect("path");
        // Every waypoint should stay in the lower band, not detour over
        // the top of the obstacle.
        assert!(
            path.iter().all(|p| p.y > 50.0),
            "expected the short way round, got {path:?}"
        );
    }

    #[test]
    fn goal_off_the_floor_is_snapped_not_rejected() {
        let area = open_floor();
        let path = find_path(&area, Vec2::new(50.0, 50.0), Vec2::new(500.0, 50.0)).expect("path");
        let end = *path.last().expect("non-empty");
        assert!(area.contains(end));
        assert!((end.x - 100.0).abs() < 1e-3);
    }

    #[test]
    fn start_inside_an_obstacle_is_snapped_out() {
        let area = floor_with_hole();
        let path = find_path(&area, Vec2::new(50.0, 50.0), Vec2::new(10.0, 50.0)).expect("path");
        assert!(area.contains(path[0]));
        assert_path_is_walkable(&area, &path);
    }

    #[test]
    fn room_without_a_floor_reports_no_floor() {
        let area = WalkArea::default();
        assert_eq!(
            find_path(&area, Vec2::ZERO, Vec2::new(10.0, 10.0)),
            Err(PathError::NoFloor)
        );
    }

    #[test]
    fn disconnected_solids_report_unreachable() {
        let area = WalkArea::new(
            vec![
                Polygon::rect(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)),
                Polygon::rect(Vec2::new(50.0, 0.0), Vec2::new(60.0, 10.0)),
            ],
            Vec::new(),
        );
        assert_eq!(
            find_path(&area, Vec2::new(5.0, 5.0), Vec2::new(55.0, 5.0)),
            Err(PathError::Unreachable)
        );
    }

    #[test]
    fn path_length_sums_the_legs() {
        let path = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 4.0),
            Vec2::new(3.0, 14.0),
        ];
        assert!((path_length(&path) - 15.0).abs() < 1e-3);
    }

    #[test]
    fn point_along_interpolates_and_saturates() {
        let path = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        assert_eq!(point_along(&path, 0.0), Some(Vec2::new(0.0, 0.0)));
        assert_eq!(point_along(&path, 2.5), Some(Vec2::new(2.5, 0.0)));
        assert_eq!(point_along(&path, 999.0), Some(Vec2::new(10.0, 0.0)));
        assert_eq!(point_along(&[], 1.0), None);
    }

    #[test]
    fn point_along_crosses_a_corner() {
        let path = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
        ];
        assert_eq!(point_along(&path, 15.0), Some(Vec2::new(10.0, 5.0)));
    }
}
