//! A29.1: working out where the player arrives.
//!
//! Black Mirror's scene files say where the doors are but not where you
//! stand after walking through one. The information is there all the
//! same, one room over: if the hall has a door to the kitchen and the
//! kitchen has one back to the hall, then the kitchen's door *is* where
//! you arrive from the hall. You come out where you would go in.
//!
//! So this is a second pass over rooms that are already imported, which
//! is the only way round it - an entry point is derived from another
//! room's exit, and during the first pass that room may not exist yet.
//!
//! Where a room has no door back, nothing is invented. A room you can
//! only leave is a real thing in these games, and guessing a spot for
//! it would put the player through a wall.

use std::collections::BTreeMap;

use glam::Vec2;
use infengine_adventure::interact::EntryPoint;
use infengine_adventure::room::Room;

/// Add entry points to every room that something leads into.
///
/// Returns how many were added.
pub fn derive(rooms: &mut BTreeMap<String, Room>) -> usize {
    // Who leads where, collected before anything is changed: the pass
    // reads every room while writing to one.
    let mut arrivals: Vec<(String, String)> = Vec::new();
    for (id, room) in rooms.iter() {
        for exit in &room.exits {
            if !exit.target_room.is_empty() && exit.target_room != *id {
                arrivals.push((exit.target_room.clone(), id.clone()));
            }
        }
    }

    let mut added = 0;
    for (target, from) in arrivals {
        let Some(position) = doorway_in(rooms, &target, &from) else {
            continue;
        };
        let Some(room) = rooms.get_mut(&target) else {
            continue;
        };
        if room.entries.iter().any(|entry| entry.id == from) {
            continue;
        }
        room.entries.push(EntryPoint {
            id: from,
            position,
            facing: Default::default(),
        });
        added += 1;
    }

    for room in rooms.values_mut() {
        room.entries.sort_by(|a, b| a.id.cmp(&b.id));
    }
    added
}

/// Where in `room` the door back to `from` is, as a place to stand.
///
/// The bottom of the doorway rather than its middle, because a doorway
/// is drawn as the whole opening and an actor stands on the floor at
/// the foot of it, not halfway up the frame.
fn doorway_in(rooms: &BTreeMap<String, Room>, room: &str, from: &str) -> Option<Vec2> {
    let room = rooms.get(room)?;
    let exit = room
        .exits
        .iter()
        .find(|exit| exit.target_room == from)?;
    let centre = exit.shape.center();
    let bottom = shape_bottom(&exit.shape);
    Some(Vec2::new(centre.x, bottom))
}

fn shape_bottom(shape: &infengine_adventure::interact::Shape) -> f32 {
    use infengine_adventure::interact::Shape;
    match shape {
        Shape::Rect { min, max } => min.y.max(max.y),
        Shape::Poly(poly) => poly
            .points
            .iter()
            .map(|p| p.y)
            .fold(f32::MIN, f32::max),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_adventure::interact::{Exit, Shape};

    fn room_with_exit(id: &str, exit_id: &str, target: &str, at: Vec2) -> Room {
        let mut room = Room::new(id, Vec2::new(1280.0, 800.0));
        room.exits.push(Exit {
            id: exit_id.into(),
            name: String::new(),
            shape: Shape::Rect {
                min: at,
                max: at + Vec2::new(60.0, 120.0),
            },
            target_room: target.into(),
            enabled: true,
            ..Default::default()
        });
        room
    }

    fn pair() -> BTreeMap<String, Room> {
        let mut rooms = BTreeMap::new();
        rooms.insert(
            "hall".to_string(),
            room_with_exit("hall", "FromHallToKitchen", "kitchen", Vec2::new(100.0, 300.0)),
        );
        rooms.insert(
            "kitchen".to_string(),
            room_with_exit("kitchen", "FromKitchenToHall", "hall", Vec2::new(700.0, 400.0)),
        );
        rooms
    }

    #[test]
    fn you_arrive_where_you_would_leave() {
        let mut rooms = pair();
        assert_eq!(derive(&mut rooms), 2);

        // Coming from the hall, you stand at the kitchen's door back to
        // the hall: its middle horizontally, its foot vertically.
        let kitchen = &rooms["kitchen"];
        assert_eq!(kitchen.entries.len(), 1);
        assert_eq!(kitchen.entries[0].id, "hall");
        assert_eq!(kitchen.entries[0].position, Vec2::new(730.0, 520.0));

        let hall = &rooms["hall"];
        assert_eq!(hall.entries[0].id, "kitchen");
        assert_eq!(hall.entries[0].position, Vec2::new(130.0, 420.0));
    }

    #[test]
    fn a_room_with_no_way_back_gets_nothing_invented() {
        let mut rooms = BTreeMap::new();
        rooms.insert(
            "hall".to_string(),
            room_with_exit("hall", "FromHallToCellar", "cellar", Vec2::new(10.0, 20.0)),
        );
        // The cellar has no door back up. A guessed spot would put the
        // player through a wall.
        rooms.insert("cellar".to_string(), Room::new("cellar", Vec2::ZERO));

        assert_eq!(derive(&mut rooms), 0);
        assert!(rooms["cellar"].entries.is_empty());
    }

    #[test]
    fn an_exit_with_no_target_leads_nowhere_and_adds_nothing() {
        let mut rooms = BTreeMap::new();
        rooms.insert(
            "hall".to_string(),
            room_with_exit("hall", "FromHallToNowhere", "", Vec2::ZERO),
        );
        assert_eq!(derive(&mut rooms), 0);
    }

    #[test]
    fn a_door_that_loops_to_its_own_room_is_ignored() {
        let mut rooms = BTreeMap::new();
        rooms.insert(
            "hall".to_string(),
            room_with_exit("hall", "FromHallToHall", "hall", Vec2::ZERO),
        );
        assert_eq!(derive(&mut rooms), 0, "arriving where you already are is not an entry");
    }

    #[test]
    fn running_twice_does_not_double_the_entries() {
        let mut rooms = pair();
        derive(&mut rooms);
        let again = derive(&mut rooms);
        assert_eq!(again, 0);
        assert_eq!(rooms["kitchen"].entries.len(), 1);
    }

    #[test]
    fn several_ways_in_give_several_entry_points_named_after_them() {
        let mut rooms = pair();
        rooms.insert(
            "garden".to_string(),
            room_with_exit("garden", "FromGardenToKitchen", "kitchen", Vec2::ZERO),
        );
        // The kitchen needs a door back to the garden for the arrival
        // point to exist at all.
        rooms.get_mut("kitchen").expect("kitchen").exits.push(Exit {
            id: "FromKitchenToGarden".into(),
            name: String::new(),
            shape: Shape::Rect {
                min: Vec2::new(0.0, 0.0),
                max: Vec2::new(40.0, 90.0),
            },
            target_room: "garden".into(),
            enabled: true,
            ..Default::default()
        });

        derive(&mut rooms);
        let ids: Vec<&str> = rooms["kitchen"]
            .entries
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, vec!["garden", "hall"], "sorted, one per way in");
    }
}
