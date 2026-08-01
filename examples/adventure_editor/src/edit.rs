//! A6.2: what the mouse does to a room.
//!
//! Editing operations are free functions over the room and a small
//! selection state, so they can be unit tested without a window, a GPU,
//! or an event loop. The app layer decides *when* to call them; this
//! module decides what they do.

use glam::Vec2;
use infengine_adventure::{Hotspot, Polygon, Room, Shape};

/// What the left mouse button acts on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    /// Drag existing vertices and shapes.
    #[default]
    Select,
    /// Click to append vertices to the floor outline.
    DrawFloor,
    /// Drag out a rectangular hotspot.
    DrawHotspot,
    /// Click to preview the actor's route.
    WalkPreview,
}

impl Tool {
    /// Every tool, for the tests that keep the inspector's tab lists
    /// and its wording complete. Nothing at runtime wants all of them:
    /// each tab offers the ones its own job uses.
    #[cfg(test)]
    pub const ALL: [Tool; 4] = [
        Tool::Select,
        Tool::DrawFloor,
        Tool::DrawHotspot,
        Tool::WalkPreview,
    ];
}

/// Which polygon list a vertex belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolyKind {
    Solid,
    Hole,
}

/// A single draggable vertex of the walkable area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexRef {
    pub kind: PolyKind,
    pub polygon: usize,
    pub vertex: usize,
}

/// What the user currently has hold of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Selection {
    #[default]
    None,
    Vertex(VertexRef),
    Hotspot(usize),
}

/// Find the walk-area vertex within `radius` of `p`, nearest first.
pub fn vertex_at(room: &Room, p: Vec2, radius: f32) -> Option<VertexRef> {
    let mut best: Option<(VertexRef, f32)> = None;
    let lists = [
        (PolyKind::Solid, &room.walk.solids),
        (PolyKind::Hole, &room.walk.holes),
    ];
    for (kind, polys) in lists {
        for (polygon, poly) in polys.iter().enumerate() {
            for (vertex, point) in poly.points.iter().enumerate() {
                let d = point.distance(p);
                if d <= radius && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((
                        VertexRef {
                            kind,
                            polygon,
                            vertex,
                        },
                        d,
                    ));
                }
            }
        }
    }
    best.map(|(v, _)| v)
}

fn polygon_mut(room: &mut Room, at: VertexRef) -> Option<&mut Polygon> {
    let list = match at.kind {
        PolyKind::Solid => &mut room.walk.solids,
        PolyKind::Hole => &mut room.walk.holes,
    };
    list.get_mut(at.polygon)
}

/// Move a vertex to `p`. Returns false for a stale reference.
pub fn move_vertex(room: &mut Room, at: VertexRef, p: Vec2) -> bool {
    let Some(poly) = polygon_mut(room, at) else {
        return false;
    };
    let Some(point) = poly.points.get_mut(at.vertex) else {
        return false;
    };
    *point = p;
    true
}

/// Delete a vertex.
///
/// A polygon that would drop below three vertices is removed entirely
/// rather than left as a degenerate sliver that renders as nothing and
/// silently breaks pathfinding.
pub fn delete_vertex(room: &mut Room, at: VertexRef) -> bool {
    let Some(poly) = polygon_mut(room, at) else {
        return false;
    };
    if at.vertex >= poly.points.len() {
        return false;
    }
    if poly.points.len() <= 3 {
        let list = match at.kind {
            PolyKind::Solid => &mut room.walk.solids,
            PolyKind::Hole => &mut room.walk.holes,
        };
        list.remove(at.polygon);
        return true;
    }
    poly.points.remove(at.vertex);
    true
}

/// Insert a vertex on the outline edge nearest to `p`, splitting it.
///
/// This is how a floor gains detail: click on an edge rather than having
/// to rebuild the polygon.
pub fn insert_vertex_on_edge(room: &mut Room, p: Vec2, radius: f32) -> Option<VertexRef> {
    let mut best: Option<(PolyKind, usize, usize, f32)> = None;
    let lists = [
        (PolyKind::Solid, &room.walk.solids),
        (PolyKind::Hole, &room.walk.holes),
    ];
    for (kind, polys) in lists {
        for (pi, poly) in polys.iter().enumerate() {
            let n = poly.points.len();
            for i in 0..n {
                let a = poly.points[i];
                let b = poly.points[(i + 1) % n];
                let d = infengine_adventure::geom::distance_to_segment(a, b, p);
                if d <= radius && best.is_none_or(|(_, _, _, bd)| d < bd) {
                    best = Some((kind, pi, i, d));
                }
            }
        }
    }
    let (kind, polygon, edge, _) = best?;
    let at = VertexRef {
        kind,
        polygon,
        vertex: edge + 1,
    };
    let poly = polygon_mut(room, at)?;
    poly.points.insert(edge + 1, p);
    Some(at)
}

/// Append a point to the floor outline being drawn.
///
/// The first three points create a polygon; later ones extend it. The
/// caller keeps `active` between clicks and clears it to finish.
pub fn extend_floor(room: &mut Room, active: &mut Vec<Vec2>, p: Vec2) {
    active.push(p);
    if active.len() < 3 {
        return;
    }
    if active.len() == 3 {
        room.walk.solids.push(Polygon::new(active.clone()));
        return;
    }
    if let Some(last) = room.walk.solids.last_mut() {
        last.points = active.clone();
    }
}

/// Turn a drag into a rectangular hotspot.
///
/// A drag smaller than `min_size` on either axis is rejected: it is
/// almost always a click that slipped, and an invisible hotspot is worse
/// than none.
pub fn finish_hotspot_drag(
    room: &mut Room,
    from: Vec2,
    to: Vec2,
    min_size: f32,
) -> Option<usize> {
    let min = from.min(to);
    let max = from.max(to);
    if (max.x - min.x) < min_size || (max.y - min.y) < min_size {
        return None;
    }
    let id = unique_hotspot_id(room, "hotspot");
    room.hotspots.push(Hotspot {
        name: id.clone(),
        id,
        shape: Shape::Rect { min, max },
        enabled: true,
        ..Default::default()
    });
    Some(room.hotspots.len() - 1)
}

/// A hotspot id not already used in this room.
pub fn unique_hotspot_id(room: &Room, base: &str) -> String {
    if !room.hotspots.iter().any(|h| h.id == base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base}_{n}")).find(|candidate| {
        !room.hotspots.iter().any(|h| h.id == *candidate)
    }).unwrap_or_else(|| base.to_string())
}

/// Topmost hotspot under `p`, matching the runtime's own hit rule.
pub fn hotspot_at(room: &Room, p: Vec2) -> Option<usize> {
    room.hotspots
        .iter()
        .enumerate()
        .filter(|(_, h)| h.shape.contains(p))
        .min_by(|(_, a), (_, b)| {
            a.shape
                .area()
                .partial_cmp(&b.shape.area())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
}

/// Move a hotspot by a delta, keeping its shape.
pub fn move_hotspot(room: &mut Room, index: usize, delta: Vec2) -> bool {
    let Some(hotspot) = room.hotspots.get_mut(index) else {
        return false;
    };
    match &mut hotspot.shape {
        Shape::Rect { min, max } => {
            *min += delta;
            *max += delta;
        }
        Shape::Poly(poly) => {
            for point in &mut poly.points {
                *point += delta;
            }
        }
    }
    if let Some(walk_to) = hotspot.walk_to.as_mut() {
        *walk_to += delta;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room_with_floor() -> Room {
        let mut room = Room::new("test", Vec2::new(320.0, 200.0));
        room.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        room
    }

    #[test]
    fn a_vertex_is_found_within_the_grab_radius() {
        let room = room_with_floor();
        let hit = vertex_at(&room, Vec2::new(2.0, 102.0), 8.0).expect("near a corner");
        assert_eq!(hit.kind, PolyKind::Solid);
        assert_eq!(hit.polygon, 0);
        assert_eq!(hit.vertex, 0);
        assert!(vertex_at(&room, Vec2::new(160.0, 150.0), 8.0).is_none());
    }

    #[test]
    fn the_nearest_vertex_wins_when_two_are_in_range() {
        let mut room = room_with_floor();
        room.walk.solids[0].points = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(6.0, 0.0),
            Vec2::new(6.0, 6.0),
            Vec2::new(0.0, 6.0),
        ];
        let hit = vertex_at(&room, Vec2::new(5.0, 0.5), 10.0).expect("in range");
        assert_eq!(hit.vertex, 1);
    }

    #[test]
    fn moving_a_vertex_updates_the_polygon() {
        let mut room = room_with_floor();
        let at = VertexRef {
            kind: PolyKind::Solid,
            polygon: 0,
            vertex: 0,
        };
        assert!(move_vertex(&mut room, at, Vec2::new(20.0, 120.0)));
        assert_eq!(room.walk.solids[0].points[0], Vec2::new(20.0, 120.0));
    }

    #[test]
    fn a_stale_reference_is_rejected_rather_than_panicking() {
        let mut room = room_with_floor();
        let bad = VertexRef {
            kind: PolyKind::Solid,
            polygon: 9,
            vertex: 0,
        };
        assert!(!move_vertex(&mut room, bad, Vec2::ZERO));
        assert!(!delete_vertex(&mut room, bad));
        let bad_vertex = VertexRef {
            kind: PolyKind::Solid,
            polygon: 0,
            vertex: 99,
        };
        assert!(!move_vertex(&mut room, bad_vertex, Vec2::ZERO));
    }

    #[test]
    fn deleting_from_a_triangle_removes_the_whole_polygon() {
        let mut room = Room::new("t", Vec2::new(10.0, 10.0));
        room.walk.solids.push(Polygon::new(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(5.0, 10.0),
        ]));
        let at = VertexRef {
            kind: PolyKind::Solid,
            polygon: 0,
            vertex: 1,
        };
        assert!(delete_vertex(&mut room, at));
        assert!(
            room.walk.solids.is_empty(),
            "a two-point sliver would break pathfinding silently"
        );
    }

    #[test]
    fn deleting_from_a_larger_polygon_keeps_it() {
        let mut room = room_with_floor();
        let at = VertexRef {
            kind: PolyKind::Solid,
            polygon: 0,
            vertex: 2,
        };
        assert!(delete_vertex(&mut room, at));
        assert_eq!(room.walk.solids[0].points.len(), 3);
    }

    #[test]
    fn a_vertex_can_be_inserted_on_an_edge() {
        let mut room = room_with_floor();
        let before = room.walk.solids[0].points.len();
        let at = insert_vertex_on_edge(&mut room, Vec2::new(160.0, 101.0), 8.0)
            .expect("near the top edge");
        assert_eq!(room.walk.solids[0].points.len(), before + 1);
        assert_eq!(room.walk.solids[0].points[at.vertex], Vec2::new(160.0, 101.0));
    }

    #[test]
    fn inserting_far_from_any_edge_does_nothing() {
        let mut room = room_with_floor();
        assert!(insert_vertex_on_edge(&mut room, Vec2::new(160.0, 150.0), 4.0).is_none());
        assert_eq!(room.walk.solids[0].points.len(), 4);
    }

    #[test]
    fn drawing_a_floor_creates_a_polygon_on_the_third_point() {
        let mut room = Room::new("t", Vec2::new(100.0, 100.0));
        let mut active = Vec::new();
        extend_floor(&mut room, &mut active, Vec2::new(0.0, 0.0));
        extend_floor(&mut room, &mut active, Vec2::new(10.0, 0.0));
        assert!(room.walk.solids.is_empty(), "two points enclose nothing");
        extend_floor(&mut room, &mut active, Vec2::new(10.0, 10.0));
        assert_eq!(room.walk.solids.len(), 1);
        extend_floor(&mut room, &mut active, Vec2::new(0.0, 10.0));
        assert_eq!(room.walk.solids.len(), 1, "still one polygon");
        assert_eq!(room.walk.solids[0].points.len(), 4);
    }

    #[test]
    fn a_hotspot_drag_becomes_a_rectangle() {
        let mut room = room_with_floor();
        let index = finish_hotspot_drag(
            &mut room,
            Vec2::new(50.0, 120.0),
            Vec2::new(90.0, 160.0),
            4.0,
        )
        .expect("large enough");
        assert_eq!(index, 0);
        assert!(room.hotspots[0].shape.contains(Vec2::new(70.0, 140.0)));
        assert!(room.hotspots[0].enabled);
    }

    #[test]
    fn a_drag_backwards_still_produces_the_same_rectangle() {
        let mut room = room_with_floor();
        finish_hotspot_drag(&mut room, Vec2::new(90.0, 160.0), Vec2::new(50.0, 120.0), 4.0)
            .expect("large enough");
        assert!(room.hotspots[0].shape.contains(Vec2::new(70.0, 140.0)));
    }

    #[test]
    fn a_slipped_click_does_not_create_an_invisible_hotspot() {
        let mut room = room_with_floor();
        assert!(
            finish_hotspot_drag(&mut room, Vec2::new(50.0, 50.0), Vec2::new(51.0, 80.0), 4.0)
                .is_none()
        );
        assert!(room.hotspots.is_empty());
    }

    #[test]
    fn hotspot_ids_do_not_collide() {
        let mut room = room_with_floor();
        for _ in 0..3 {
            finish_hotspot_drag(&mut room, Vec2::ZERO, Vec2::new(20.0, 20.0), 4.0)
                .expect("large enough");
        }
        let ids: Vec<_> = room.hotspots.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, vec!["hotspot", "hotspot_2", "hotspot_3"]);
    }

    #[test]
    fn the_smallest_hotspot_under_the_cursor_is_picked() {
        let mut room = room_with_floor();
        finish_hotspot_drag(&mut room, Vec2::ZERO, Vec2::new(100.0, 100.0), 4.0).expect("big");
        finish_hotspot_drag(&mut room, Vec2::new(40.0, 40.0), Vec2::new(60.0, 60.0), 4.0)
            .expect("small");
        assert_eq!(hotspot_at(&room, Vec2::new(50.0, 50.0)), Some(1));
        assert_eq!(hotspot_at(&room, Vec2::new(10.0, 10.0)), Some(0));
        assert_eq!(hotspot_at(&room, Vec2::new(200.0, 200.0)), None);
    }

    #[test]
    fn moving_a_hotspot_carries_its_walk_to_point() {
        let mut room = room_with_floor();
        finish_hotspot_drag(&mut room, Vec2::ZERO, Vec2::new(20.0, 20.0), 4.0).expect("big");
        room.hotspots[0].walk_to = Some(Vec2::new(10.0, 30.0));
        assert!(move_hotspot(&mut room, 0, Vec2::new(5.0, 5.0)));
        assert!(room.hotspots[0].shape.contains(Vec2::new(6.0, 6.0)));
        assert_eq!(room.hotspots[0].walk_to, Some(Vec2::new(15.0, 35.0)));
        assert!(!move_hotspot(&mut room, 9, Vec2::ZERO));
    }

    #[test]
    fn moving_a_polygon_hotspot_moves_every_vertex() {
        let mut room = room_with_floor();
        room.hotspots.push(Hotspot {
            id: "poly".into(),
            shape: Shape::Poly(Polygon::rect(Vec2::ZERO, Vec2::new(10.0, 10.0))),
            enabled: true,
            ..Default::default()
        });
        assert!(move_hotspot(&mut room, 0, Vec2::new(100.0, 0.0)));
        assert!(room.hotspots[0].shape.contains(Vec2::new(105.0, 5.0)));
        assert!(!room.hotspots[0].shape.contains(Vec2::new(5.0, 5.0)));
    }

    #[test]
    fn the_tool_list_is_complete() {
        assert_eq!(Tool::ALL.len(), 4);
        assert_eq!(Tool::default(), Tool::Select);
    }
}
