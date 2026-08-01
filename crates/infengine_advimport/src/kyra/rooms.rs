//! A43.1: turning backgrounds into rooms.
//!
//! The import writes one room per background, which is the obvious
//! reading and the wrong one. Kyrandia draws 246 rooms out of 107
//! pictures: the forest is thirteen places sharing one painting and the
//! caves are seventy-six sharing two. One room per background collapses
//! both mazes into a couple of screens, which is why the imported game
//! had eighty-seven rooms and no way through any of them.
//!
//! This runs afterwards, over the files the import just wrote. It reads
//! each background room, copies it once per place the graph says exists,
//! and gives each copy the exits the graph lists. A background used by
//! exactly one room keeps its own name; one used by thirteen gets
//! thirteen numbered rooms and the plain name disappears, because
//! leaving it behind would leave an unreachable duplicate in the list.
//!
//! Every copy keeps the background's name as its script prefix. The
//! logic is per-picture in the original - one `FORESTA.EMC` for all
//! thirteen - and splitting it thirteen ways would be inventing
//! something the source does not have.
//!
//! What this cannot do is place the exits truthfully. Where in the
//! picture a doorway is remains in the bytecode, so each exit is put as
//! a strip along the edge it leads off. That makes the game walkable and
//! makes every doorway visible in the editor, which is where the real
//! shape gets drawn.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use glam::Vec2;
use infengine_adventure::interact::{Exit, Shape};
use infengine_adventure::Room;

use super::kyradat::{Direction, RoomGraph};

/// How deep an exit strip reaches into the room, in pixels.
///
/// Wide enough to walk into without aiming, narrow enough that the four
/// of them leave the middle of the room clear.
const EDGE_DEPTH: f32 = 14.0;
/// How much of an edge's length the strip covers, as a fraction.
///
/// Not the whole edge: two strips meeting in a corner would make the
/// corner ambiguous, and the first one found would win at random.
const EDGE_SPAN: f32 = 0.6;

/// What an expansion did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expansion {
    /// Rooms written.
    pub written: usize,
    /// Exits given out.
    pub exits: usize,
    /// Background room files removed because they became numbered rooms.
    pub replaced: usize,
    /// Backgrounds the graph names that the import never produced, in
    /// the game's own spelling. Usually a picture that did not decode.
    pub missing: Vec<String>,
}

/// Room ids the graph implies, indexed by graph room number.
///
/// A background carrying one room keeps its plain name so the common
/// case stays readable; one carrying several gets numbered, and the
/// number is the game's own room id rather than a running count, so the
/// name survives re-importing and can be checked against the source.
pub fn room_ids(graph: &RoomGraph) -> HashMap<usize, String> {
    let uses = graph.uses();
    graph
        .linked()
        .filter_map(|(index, link)| {
            let name = graph.names.get(link.name_index as usize)?.to_ascii_lowercase();
            let id = if uses.get(&link.name_index).copied().unwrap_or(0) > 1 {
                format!("{name}_{index}")
            } else {
                name
            };
            Some((index, id))
        })
        .collect()
}

/// A strip along one edge of a room.
fn edge_shape(size: Vec2, dir: Direction) -> Shape {
    let span = size * EDGE_SPAN;
    let inset = (size - span) * 0.5;
    let (min, max) = match dir {
        Direction::North => (
            Vec2::new(inset.x, 0.0),
            Vec2::new(inset.x + span.x, EDGE_DEPTH),
        ),
        Direction::South => (
            Vec2::new(inset.x, size.y - EDGE_DEPTH),
            Vec2::new(inset.x + span.x, size.y),
        ),
        Direction::West => (
            Vec2::new(0.0, inset.y),
            Vec2::new(EDGE_DEPTH, inset.y + span.y),
        ),
        Direction::East => (
            Vec2::new(size.x - EDGE_DEPTH, inset.y),
            Vec2::new(size.x, inset.y + span.y),
        ),
    };
    Shape::Rect { min, max }
}

/// Build one room of the graph from its background.
pub fn instance(base: &Room, id: &str, prefix: &str, index: usize, graph: &RoomGraph) -> Room {
    let mut room = base.clone();
    room.id = id.to_string();
    room.script = prefix.to_string();
    // The background's name is what the place looks like, not what it
    // is, so the numbered rooms keep it as a hint rather than a title.
    if room.name.is_empty() {
        room.name = prefix.to_string();
    }
    room.exits.clear();

    let ids = room_ids(graph);
    let Some(link) = graph.rooms.get(index) else {
        return room;
    };
    for (dir, to) in link.present() {
        let Some(target) = ids.get(&(to as usize)) else {
            continue;
        };
        let shape = edge_shape(room.size, dir);
        room.exits.push(Exit {
            id: dir.name().to_string(),
            name: format!("{} to {target}", dir.name()),
            walk_to: Some(shape.center()),
            shape,
            target_room: target.clone(),
            target_entry: String::new(),
            enabled: true,
        });
    }
    room
}

/// Expand the rooms an import wrote into the rooms the game has.
///
/// `dir` is the import's `rooms/` directory.
pub fn expand(dir: &Path, graph: &RoomGraph) -> std::io::Result<Expansion> {
    let mut report = Expansion::default();
    let ids = room_ids(graph);

    // Load each background once rather than once per room that uses it:
    // the caves would otherwise read the same file seventy-six times.
    let mut bases: HashMap<String, Option<Room>> = HashMap::new();
    let load = |prefix: &str, bases: &mut HashMap<String, Option<Room>>| -> Option<Room> {
        bases
            .entry(prefix.to_string())
            .or_insert_with(|| {
                let path = dir.join(format!("{prefix}.room.ron"));
                infengine_adventure::load(&path).ok()
            })
            .clone()
    };

    // Backgrounds whose plain-named file is superseded by numbered
    // rooms. Collected first and deleted at the end, because the file is
    // still being read while the copies are made.
    let mut superseded: Vec<String> = Vec::new();

    for (index, id) in &ids {
        let Some(link) = graph.rooms.get(*index) else {
            continue;
        };
        let Some(prefix) = graph
            .names
            .get(link.name_index as usize)
            .map(|n| n.to_ascii_lowercase())
        else {
            continue;
        };
        let Some(base) = load(&prefix, &mut bases) else {
            continue;
        };
        if *id != prefix {
            superseded.push(prefix.clone());
        }

        let room = instance(&base, id, &prefix, *index, graph);
        report.exits += room.exits.len();
        let path = dir.join(format!("{id}.room.ron"));
        infengine_adventure::save(&room, &path)
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        report.written += 1;
    }

    for (prefix, base) in &bases {
        if base.is_none() {
            report.missing.push(prefix.to_ascii_uppercase());
        }
    }
    report.missing.sort();

    superseded.sort();
    superseded.dedup();
    for prefix in superseded {
        let path = dir.join(format!("{prefix}.room.ron"));
        if path.exists() && fs::remove_file(&path).is_ok() {
            report.replaced += 1;
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kyra::kyradat::parse_room_graph;

    const NO_EXIT: u16 = 0xFFFF;

    fn names(list: &[&str]) -> Vec<u8> {
        let mut v = Vec::new();
        for name in list {
            v.extend_from_slice(name.as_bytes());
            v.push(0);
        }
        v
    }

    fn list(rooms: &[(u8, u16, u16, u16, u16)]) -> Vec<u8> {
        let mut v = (rooms.len() as u32).to_be_bytes().to_vec();
        for (ni, n, e, s, w) in rooms {
            v.push(*ni);
            for link in [n, e, s, w] {
                v.extend_from_slice(&link.to_be_bytes());
            }
        }
        v
    }

    /// Two forest rooms on one picture, plus a cave on its own.
    fn graph() -> RoomGraph {
        parse_room_graph(
            &names(&["FORESTA", "CAVE"]),
            &list(&[
                (0, NO_EXIT, 1, NO_EXIT, NO_EXIT),
                (0, NO_EXIT, 2, NO_EXIT, 0),
                (1, NO_EXIT, NO_EXIT, NO_EXIT, 1),
            ]),
        )
        .expect("graph")
    }

    #[test]
    fn a_shared_background_gets_one_numbered_room_each() {
        let ids = room_ids(&graph());
        assert_eq!(ids.get(&0).map(String::as_str), Some("foresta_0"));
        assert_eq!(ids.get(&1).map(String::as_str), Some("foresta_1"));
        // Used once, so it keeps its own name.
        assert_eq!(ids.get(&2).map(String::as_str), Some("cave"));
    }

    #[test]
    fn every_link_becomes_an_exit_naming_the_room_it_reaches() {
        let g = graph();
        let base = Room::new("foresta", Vec2::new(320.0, 144.0));
        let room = instance(&base, "foresta_1", "foresta", 1, &g);
        let targets: Vec<&str> = room.exits.iter().map(|e| e.target_room.as_str()).collect();
        assert_eq!(targets, vec!["cave", "foresta_0"], "east then west");
        assert_eq!(room.script, "foresta", "shared logic keeps one name");
    }

    #[test]
    fn the_exits_of_a_room_do_not_overlap() {
        // Four strips meeting in the corners would make a corner click
        // land on whichever exit happened to be found first.
        let g = parse_room_graph(&names(&["X"]), &list(&[(0, 0, 0, 0, 0)])).expect("graph");
        let base = Room::new("x", Vec2::new(320.0, 144.0));
        let room = instance(&base, "x", "x", 0, &g);
        assert_eq!(room.exits.len(), 4);
        for (i, a) in room.exits.iter().enumerate() {
            for b in &room.exits[i + 1..] {
                let (Shape::Rect { min: amin, max: amax }, Shape::Rect { min: bmin, max: bmax }) =
                    (&a.shape, &b.shape)
                else {
                    panic!("edge strips are rectangles");
                };
                let overlap =
                    amin.x < bmax.x && bmin.x < amax.x && amin.y < bmax.y && bmin.y < amax.y;
                assert!(!overlap, "{} overlaps {}", a.id, b.id);
            }
        }
    }

    #[test]
    fn an_exit_is_walked_to_rather_than_teleported_through() {
        let g = graph();
        let base = Room::new("cave", Vec2::new(320.0, 144.0));
        let room = instance(&base, "cave", "cave", 2, &g);
        let exit = room.exits.first().expect("one exit west");
        let target = exit.walk_to.expect("somewhere to walk");
        assert!(exit.shape.contains(target));
    }

    #[test]
    fn a_room_keeps_what_the_background_gave_it() {
        let g = graph();
        let mut base = Room::new("foresta", Vec2::new(320.0, 144.0));
        base.layers.push(infengine_adventure::Layer {
            name: "bg".into(),
            image: "images/foresta.png".into(),
            ..Default::default()
        });
        base.entries.push(infengine_adventure::EntryPoint {
            id: "entry0".into(),
            position: Vec2::new(100.0, 120.0),
            facing: Default::default(),
        });
        let room = instance(&base, "foresta_0", "foresta", 0, &g);
        assert_eq!(room.layers.len(), 1, "the picture comes along");
        assert_eq!(room.entries.len(), 1, "so do the arrival points");
        assert_eq!(room.size, base.size);
    }

    #[test]
    fn a_link_to_a_room_with_no_exits_of_its_own_is_dropped() {
        // Rooms the table lists but never links are its unused tail;
        // an exit into one is a door into nothing.
        let g = parse_room_graph(
            &names(&["A", "B"]),
            &list(&[(0, NO_EXIT, 1, NO_EXIT, NO_EXIT), (1, NO_EXIT, NO_EXIT, NO_EXIT, NO_EXIT)]),
        )
        .expect("graph");
        let base = Room::new("a", Vec2::new(320.0, 144.0));
        let room = instance(&base, "a", "a", 0, &g);
        assert!(room.exits.is_empty(), "room 1 is not in the graph's rooms");
    }

    #[test]
    fn expanding_replaces_a_shared_background_and_keeps_a_lone_one() {
        let dir = std::env::temp_dir().join("infengine_expand_rooms");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        for name in ["foresta", "cave"] {
            let room = Room::new(name, Vec2::new(320.0, 144.0));
            infengine_adventure::save(&room, &dir.join(format!("{name}.room.ron"))).expect("save");
        }

        let report = expand(&dir, &graph()).expect("expand");
        assert_eq!(report.written, 3);
        assert_eq!(report.replaced, 1, "foresta became two numbered rooms");
        assert!(report.missing.is_empty());
        assert!(dir.join("foresta_0.room.ron").exists());
        assert!(dir.join("foresta_1.room.ron").exists());
        assert!(dir.join("cave.room.ron").exists());
        assert!(
            !dir.join("foresta.room.ron").exists(),
            "leaving it would leave an unreachable duplicate in the list"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_background_the_import_never_produced_is_named_rather_than_skipped() {
        let dir = std::env::temp_dir().join("infengine_expand_missing");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        // Only the cave decoded; the forest picture did not.
        let room = Room::new("cave", Vec2::new(320.0, 144.0));
        infengine_adventure::save(&room, &dir.join("cave.room.ron")).expect("save");

        let report = expand(&dir, &graph()).expect("expand");
        assert_eq!(report.written, 1);
        assert_eq!(report.missing, vec!["FORESTA"]);

        let _ = fs::remove_dir_all(&dir);
    }
}
