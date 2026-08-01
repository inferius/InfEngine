//! A42.3: putting something new in a room.
//!
//! Until now the only thing that could be added at all was a hotspot,
//! and only by knowing to pick a tool and drag with it. Exits and
//! arrival points could be read and edited but never created, so a room
//! imported without a door stayed without one.
//!
//! Everything added here lands in the middle of what the user is looking
//! at, at a size they can see and grab. The alternatives are worse:
//! at the room's origin it may be off screen, and at the cursor it lands
//! wherever the mouse happened to be when the menu was clicked.
//!
//! Ids are made unique across the whole room rather than per list.
//! A hotspot and an exit that share an id are two different things a
//! script cannot tell apart, and the failure surfaces much later.

use glam::Vec2;
use infengine_adventure::interact::{EntryPoint, Exit, Hotspot, Shape};
use infengine_adventure::Room;

/// Side of a freshly added region, in room units.
///
/// Big enough to see and to grab a corner of on a 320-wide screen, small
/// enough not to swallow the picture on a 1280-wide one.
const NEW_SIZE: f32 = 48.0;

/// An id not used by any hotspot or exit in this room.
pub fn unique_id(room: &Room, base: &str) -> String {
    let taken = |candidate: &str| {
        room.hotspots.iter().any(|h| h.id == candidate)
            || room.exits.iter().any(|e| e.id == candidate)
            || room.entries.iter().any(|e| e.id == candidate)
    };
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| base.to_string())
}

/// A square of [`NEW_SIZE`] centred on `at`, kept inside the room.
fn box_at(room: &Room, at: Vec2) -> (Vec2, Vec2) {
    let half = Vec2::splat(NEW_SIZE * 0.5);
    // Clamping the centre rather than the corners keeps the region
    // square in a room smaller than the region itself.
    let low = half.min(room.size * 0.5);
    let high = (room.size - half).max(low);
    let centre = at.clamp(low, high);
    (centre - half, centre + half)
}

/// Add a clickable region. Returns its index.
pub fn hotspot(room: &mut Room, at: Vec2) -> usize {
    let (min, max) = box_at(room, at);
    let id = unique_id(room, "thing");
    room.hotspots.push(Hotspot {
        name: id.clone(),
        id,
        shape: Shape::Rect { min, max },
        enabled: true,
        ..Default::default()
    });
    room.hotspots.len() - 1
}

/// Add a way out. It leads nowhere until a room is picked for it, which
/// the Things tab shows in red.
pub fn exit(room: &mut Room, at: Vec2) -> usize {
    let (min, max) = box_at(room, at);
    let id = unique_id(room, "way_out");
    room.exits.push(Exit {
        name: String::new(),
        id,
        shape: Shape::Rect { min, max },
        target_room: String::new(),
        target_entry: String::new(),
        walk_to: None,
        enabled: true,
    });
    room.exits.len() - 1
}

/// Add an arrival point.
///
/// Placed on the floor when there is one: an arrival point off the floor
/// strands the actor, and dropping it at the view's centre would do that
/// about as often as not.
pub fn entry(room: &mut Room, at: Vec2) -> usize {
    let position = if room.walk.is_empty() || room.walk.contains(at) {
        at
    } else {
        nearest_floor_point(room, at).unwrap_or(at)
    };
    let id = unique_id(room, "arrival");
    room.entries.push(EntryPoint {
        id,
        position,
        facing: Default::default(),
    });
    room.entries.len() - 1
}

/// The floor corner nearest `at`.
///
/// A corner rather than the true nearest point on the outline: it is a
/// position the author placed themselves, so it is certainly reachable,
/// and it is going to be dragged into place anyway.
fn nearest_floor_point(room: &Room, at: Vec2) -> Option<Vec2> {
    room.walk
        .solids
        .iter()
        .flat_map(|poly| poly.points.iter().copied())
        .min_by(|a, b| {
            a.distance_squared(at)
                .partial_cmp(&b.distance_squared(at))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Room {
        Room::new("hall", Vec2::new(320.0, 144.0))
    }

    #[test]
    fn a_new_thing_lands_where_you_are_looking() {
        let mut r = room();
        let index = hotspot(&mut r, Vec2::new(160.0, 72.0));
        assert!(r.hotspots[index].shape.contains(Vec2::new(160.0, 72.0)));
    }

    #[test]
    fn a_new_thing_is_kept_inside_the_room() {
        // Added while scrolled off the edge, it would otherwise be
        // created somewhere it can never be clicked.
        let mut r = room();
        let index = hotspot(&mut r, Vec2::new(-500.0, 900.0));
        let Shape::Rect { min, max } = r.hotspots[index].shape else {
            panic!("a new region is a rectangle");
        };
        assert!(min.x >= 0.0 && min.y >= 0.0, "{min}");
        assert!(max.x <= 320.0 && max.y <= 144.0, "{max}");
    }

    #[test]
    fn a_region_stays_square_in_a_room_smaller_than_itself() {
        let mut r = Room::new("tiny", Vec2::new(20.0, 20.0));
        let index = hotspot(&mut r, Vec2::new(10.0, 10.0));
        let Shape::Rect { min, max } = r.hotspots[index].shape else {
            panic!("rect");
        };
        assert_eq!(max - min, Vec2::splat(NEW_SIZE));
    }

    #[test]
    fn ids_do_not_collide_across_the_room() {
        // A hotspot and an exit sharing an id are two things a script
        // cannot tell apart, and the failure shows up much later.
        let mut r = room();
        hotspot(&mut r, Vec2::ZERO);
        exit(&mut r, Vec2::ZERO);
        entry(&mut r, Vec2::ZERO);
        hotspot(&mut r, Vec2::ZERO);

        let mut ids: Vec<&str> = r
            .hotspots
            .iter()
            .map(|h| h.id.as_str())
            .chain(r.exits.iter().map(|e| e.id.as_str()))
            .chain(r.entries.iter().map(|e| e.id.as_str()))
            .collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "{ids:?}");
    }

    #[test]
    fn a_new_way_out_leads_nowhere_rather_than_somewhere_wrong() {
        // Defaulting it to the first room in the project would make a
        // door that silently works and goes to the wrong place.
        let mut r = room();
        let index = exit(&mut r, Vec2::new(10.0, 10.0));
        assert!(r.exits[index].target_room.is_empty());
        assert!(r.exits[index].enabled);
    }

    #[test]
    fn an_arrival_point_is_put_on_the_floor() {
        let mut r = room();
        r.walk.solids.push(infengine_adventure::walk::Polygon::new(vec![
            Vec2::new(0.0, 100.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(100.0, 140.0),
            Vec2::new(0.0, 140.0),
        ]));
        // Asked for somewhere well off the floor.
        let index = entry(&mut r, Vec2::new(300.0, 10.0));
        let placed = r.entries[index].position;
        assert!(
            r.walk.contains(placed) || r.walk.solids[0].points.contains(&placed),
            "{placed} is not on the floor"
        );
    }

    #[test]
    fn an_arrival_point_in_a_room_with_no_floor_lands_where_asked() {
        let mut r = room();
        let index = entry(&mut r, Vec2::new(42.0, 43.0));
        assert_eq!(r.entries[index].position, Vec2::new(42.0, 43.0));
    }
}
