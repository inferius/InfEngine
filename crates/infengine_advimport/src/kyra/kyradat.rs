//! A40.1: the room graph, which is not in the game's data files.
//!
//! Kyrandia's room connections live in the executable, not in the PAKs -
//! a table the interpreter indexes rather than a file it opens. That is
//! why importing every archive yields rooms with no way out of them.
//!
//! ScummVM's build tools lift that table out of each supported release
//! and store it in `kyra.dat`, an index followed by raw blobs. Reading
//! it here keeps the extraction offline and optional: hand the importer
//! a `kyra.dat` and the rooms arrive connected, leave it out and they
//! arrive as before. Nothing from it is embedded in this crate.
//!
//! Container layout, read off the file:
//!
//! ```text
//!   repeated, until the first blob:
//!     u32   offset of the blob, little-endian
//!     8     identifier in hexadecimal
//!     1     NUL
//! ```
//!
//! A blob's length is the distance to the next offset, so the index has
//! to be sorted by offset before anything can be read. The identifier is
//! four bytes - game, platform, variant, resource - and the two tables
//! wanted here are `..0120` (room file names) and `..0130` (the graph),
//! which is the order the extractor's own enum puts them in.
//!
//! The graph is big-endian inside a little-endian file:
//!
//! ```text
//!   u32   number of rooms
//!   per room, 9 bytes:
//!     u8    index into the file-name table
//!     u16   room reached by walking north
//!     u16   east, u16 south, u16 west   (0xFFFF for no exit)
//! ```
//!
//! The counts that come out are the reason this was worth chasing: 246
//! rooms drawn from 107 backgrounds. The forest is thirteen rooms
//! sharing one picture and the caves are seventy-six sharing two - so a
//! room is not a background, and an importer that assumes it is collapses
//! the game's two mazes into two screens.

use std::collections::HashMap;
use std::fmt;

/// Identifier of the room file-name table for Kyrandia 1, DOS, English.
pub const ID_ROOM_FILENAMES: &str = "00100120";
/// Identifier of the room graph for the same release.
pub const ID_ROOM_LIST: &str = "00100130";

/// Length of an identifier in the index, in hexadecimal characters.
const ID_LEN: usize = 8;
/// Bytes one room occupies in the graph.
const ROOM_BYTES: usize = 9;
/// Link value meaning there is no exit that way.
const NO_EXIT: u16 = 0xFFFF;

/// Which way an exit leads. The table stores four, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    North,
    East,
    South,
    West,
}

impl Direction {
    /// All four, in the order the table stores them.
    pub const ALL: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    /// Lower-case name, used to build exit identifiers.
    pub fn name(self) -> &'static str {
        match self {
            Direction::North => "north",
            Direction::East => "east",
            Direction::South => "south",
            Direction::West => "west",
        }
    }

    /// The way back.
    pub fn opposite(self) -> Direction {
        match self {
            Direction::North => Direction::South,
            Direction::East => Direction::West,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
        }
    }
}

/// One room of the game: a background and up to four ways out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomLink {
    /// Index into [`RoomGraph::names`].
    pub name_index: u8,
    /// North, east, south, west. `None` where the table said `0xFFFF`.
    pub exits: [Option<u16>; 4],
}

impl RoomLink {
    /// The exits that exist, paired with their direction.
    pub fn present(&self) -> impl Iterator<Item = (Direction, u16)> + '_ {
        Direction::ALL
            .into_iter()
            .zip(self.exits)
            .filter_map(|(dir, to)| to.map(|to| (dir, to)))
    }

    /// Can you leave this room at all? Rooms that reach nothing are the
    /// unused tail of the table rather than real dead ends.
    pub fn is_linked(&self) -> bool {
        self.exits.iter().any(Option::is_some)
    }
}

/// The whole graph: backgrounds by index, and the rooms drawn on them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomGraph {
    /// Background file stems, as the game spells them.
    pub names: Vec<String>,
    /// Rooms, in table order. The index *is* the room's identity.
    pub rooms: Vec<RoomLink>,
}

impl RoomGraph {
    /// Background stem of a room, if its index is in range.
    pub fn name_of(&self, room: usize) -> Option<&str> {
        let link = self.rooms.get(room)?;
        self.names.get(link.name_index as usize).map(String::as_str)
    }

    /// Rooms that have at least one exit.
    pub fn linked(&self) -> impl Iterator<Item = (usize, &RoomLink)> {
        self.rooms
            .iter()
            .enumerate()
            .filter(|(_, link)| link.is_linked())
    }

    /// How many linked rooms share each background.
    ///
    /// The importer needs this to name rooms: a background used once can
    /// keep its own name, and one used thirteen times cannot.
    pub fn uses(&self) -> HashMap<u8, usize> {
        let mut uses = HashMap::new();
        for (_, link) in self.linked() {
            *uses.entry(link.name_index).or_insert(0) += 1;
        }
        uses
    }

    /// How many links have a matching link back the other way.
    ///
    /// Not a validity check - Kyrandia has genuine one-way drops - but a
    /// reading of the table that is wrong tends to score near zero here,
    /// and a right one scores in the nineties.
    pub fn symmetry(&self) -> (usize, usize) {
        let mut agree = 0;
        let mut total = 0;
        for (from, link) in self.linked() {
            for (dir, to) in link.present() {
                let Some(other) = self.rooms.get(to as usize) else {
                    continue;
                };
                total += 1;
                let back = other.exits[dir.opposite() as usize];
                if back == Some(from as u16) {
                    agree += 1;
                }
            }
        }
        (agree, total)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KyraDatError {
    /// The file does not begin with a readable index.
    NoIndex,
    /// A wanted table is not in this file.
    MissingEntry(String),
    /// A table is there but is not the length its own count implies.
    BadRoomList { count: usize, bytes: usize },
    /// A room points at a background the name table does not have.
    NameOutOfRange { room: usize, index: u8 },
}

impl fmt::Display for KyraDatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoIndex => write!(f, "kyra.dat does not start with an index"),
            Self::MissingEntry(id) => write!(f, "kyra.dat has no entry {id}"),
            Self::BadRoomList { count, bytes } => write!(
                f,
                "kyra.dat room list claims {count} rooms but holds {bytes} bytes"
            ),
            Self::NameOutOfRange { room, index } => write!(
                f,
                "kyra.dat room {room} names background {index}, which does not exist"
            ),
        }
    }
}

impl std::error::Error for KyraDatError {}

/// A parsed `kyra.dat`: identifiers to blob ranges.
#[derive(Debug, Clone)]
pub struct KyraDat<'a> {
    data: &'a [u8],
    entries: Vec<(String, usize, usize)>,
}

impl<'a> KyraDat<'a> {
    /// Read the index. Blobs are borrowed, not copied.
    pub fn parse(data: &'a [u8]) -> Result<Self, KyraDatError> {
        let mut raw: Vec<(String, usize)> = Vec::new();
        let mut p = 0usize;
        // The index runs until it would reach the first blob, which is
        // whatever the lowest offset seen so far is. Reading past that
        // would parse blob bytes as index entries.
        let mut first_blob = usize::MAX;
        // One index entry is the offset, the identifier and its NUL.
        const ENTRY: usize = 4 + ID_LEN + 1;
        while p + ENTRY <= data.len() && p + ENTRY <= first_blob {
            let offset = u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]);
            let id = &data[p + 4..p + 4 + ID_LEN];
            if data[p + 4 + ID_LEN] != 0 || !id.iter().all(|b| b.is_ascii_hexdigit()) {
                break;
            }
            let offset = offset as usize;
            if offset > data.len() {
                break;
            }
            first_blob = first_blob.min(offset);
            raw.push((String::from_utf8_lossy(id).into_owned(), offset));
            p += ENTRY;
        }
        if raw.is_empty() {
            return Err(KyraDatError::NoIndex);
        }

        // A blob ends where the next one starts, so the index has to be
        // in offset order before any length is known.
        raw.sort_by_key(|(_, offset)| *offset);
        let mut entries = Vec::with_capacity(raw.len());
        for (i, (id, offset)) in raw.iter().enumerate() {
            let end = raw.get(i + 1).map_or(data.len(), |(_, next)| *next);
            entries.push((id.clone(), *offset, end.max(*offset)));
        }
        Ok(Self { data, entries })
    }

    /// Every identifier present, in offset order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(id, ..)| id.as_str())
    }

    /// One blob.
    pub fn entry(&self, id: &str) -> Option<&'a [u8]> {
        let (_, start, end) = self.entries.iter().find(|(name, ..)| name == id)?;
        self.data.get(*start..*end)
    }

    /// Read the room graph of one release.
    pub fn room_graph(&self, names_id: &str, list_id: &str) -> Result<RoomGraph, KyraDatError> {
        let names = self
            .entry(names_id)
            .ok_or_else(|| KyraDatError::MissingEntry(names_id.into()))?;
        let list = self
            .entry(list_id)
            .ok_or_else(|| KyraDatError::MissingEntry(list_id.into()))?;
        parse_room_graph(names, list)
    }

    /// Read the graph of Kyrandia 1, DOS, English.
    pub fn kyrandia1(&self) -> Result<RoomGraph, KyraDatError> {
        self.room_graph(ID_ROOM_FILENAMES, ID_ROOM_LIST)
    }
}

/// Turn the two raw tables into a graph.
pub fn parse_room_graph(names: &[u8], list: &[u8]) -> Result<RoomGraph, KyraDatError> {
    let names: Vec<String> = names
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();

    if list.len() < 4 {
        return Err(KyraDatError::BadRoomList {
            count: 0,
            bytes: list.len(),
        });
    }
    // Big-endian, inside a file whose own index is little-endian. The
    // extractor writes its tables network order regardless of host.
    let count = u32::from_be_bytes([list[0], list[1], list[2], list[3]]) as usize;
    let body = &list[4..];
    if count.checked_mul(ROOM_BYTES) != Some(body.len()) {
        return Err(KyraDatError::BadRoomList {
            count,
            bytes: body.len(),
        });
    }

    let mut rooms = Vec::with_capacity(count);
    for i in 0..count {
        let rec = &body[i * ROOM_BYTES..(i + 1) * ROOM_BYTES];
        let name_index = rec[0];
        if name_index as usize >= names.len() {
            return Err(KyraDatError::NameOutOfRange {
                room: i,
                index: name_index,
            });
        }
        let mut exits = [None; 4];
        for (slot, exit) in exits.iter_mut().enumerate() {
            let value = u16::from_be_bytes([rec[1 + slot * 2], rec[2 + slot * 2]]);
            *exit = (value != NO_EXIT).then_some(value);
        }
        rooms.push(RoomLink { name_index, exits });
    }

    Ok(RoomGraph { names, rooms })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a container the way the real file is laid out.
    fn container(blobs: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let header = blobs.len() * (4 + ID_LEN + 1);
        let mut index = Vec::new();
        let mut body = Vec::new();
        for (id, data) in blobs {
            index.extend_from_slice(&((header + body.len()) as u32).to_le_bytes());
            index.extend_from_slice(id.as_bytes());
            index.push(0);
            body.extend_from_slice(data);
        }
        index.extend_from_slice(&body);
        index
    }

    fn names(list: &[&str]) -> Vec<u8> {
        let mut v = Vec::new();
        for name in list {
            v.extend_from_slice(name.as_bytes());
            v.push(0);
        }
        v
    }

    /// Rooms as (name index, north, east, south, west).
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

    fn sample() -> RoomGraph {
        parse_room_graph(
            &names(&["FORESTA", "FORESTB", "CAVE"]),
            &list(&[
                (0, NO_EXIT, 1, NO_EXIT, NO_EXIT),
                (1, NO_EXIT, 2, NO_EXIT, 0),
                (0, NO_EXIT, NO_EXIT, NO_EXIT, 1),
                (2, NO_EXIT, NO_EXIT, NO_EXIT, NO_EXIT),
            ]),
        )
        .expect("graph")
    }

    #[test]
    fn a_graph_reads_names_and_links() {
        let graph = sample();
        assert_eq!(graph.names.len(), 3);
        assert_eq!(graph.rooms.len(), 4);
        assert_eq!(graph.name_of(0), Some("FORESTA"));
        assert_eq!(graph.rooms[0].exits[Direction::East as usize], Some(1));
        assert_eq!(graph.rooms[0].exits[Direction::North as usize], None);
    }

    #[test]
    fn one_background_can_carry_several_rooms() {
        // The whole reason for reading this table: room 0 and room 2 are
        // different places that happen to look the same.
        let graph = sample();
        assert_eq!(graph.name_of(0), graph.name_of(2));
        assert_ne!(graph.rooms[0].exits, graph.rooms[2].exits);
        assert_eq!(graph.uses().get(&0), Some(&2));
    }

    #[test]
    fn a_room_with_no_exits_is_not_counted_as_linked() {
        let graph = sample();
        let linked: Vec<usize> = graph.linked().map(|(i, _)| i).collect();
        assert_eq!(linked, vec![0, 1, 2], "room 3 leads nowhere");
    }

    #[test]
    fn symmetry_counts_links_that_lead_back() {
        let graph = sample();
        // 0->E->1 and 1->W->0 agree; 1->E->2 and 2->W->1 agree.
        assert_eq!(graph.symmetry(), (4, 4));
    }

    #[test]
    fn a_one_way_drop_lowers_symmetry_without_being_an_error() {
        let graph = parse_room_graph(
            &names(&["LEDGE", "PIT"]),
            &list(&[
                (0, NO_EXIT, NO_EXIT, 1, NO_EXIT),
                (1, NO_EXIT, NO_EXIT, NO_EXIT, NO_EXIT),
            ]),
        )
        .expect("graph");
        assert_eq!(graph.symmetry(), (0, 1));
    }

    #[test]
    fn the_container_finds_its_entries() {
        let raw = container(&[
            (ID_ROOM_FILENAMES, names(&["FORESTA", "FORESTB", "CAVE"])),
            (
                ID_ROOM_LIST,
                list(&[(0, NO_EXIT, 1, NO_EXIT, NO_EXIT), (1, NO_EXIT, NO_EXIT, NO_EXIT, 0)]),
            ),
        ]);
        let dat = KyraDat::parse(&raw).expect("index");
        assert_eq!(dat.ids().count(), 2);
        assert!(dat.entry(ID_ROOM_LIST).is_some());
        assert_eq!(dat.entry("deadbeef"), None);

        let graph = dat.kyrandia1().expect("graph");
        assert_eq!(graph.rooms.len(), 2);
        assert_eq!(graph.name_of(1), Some("FORESTB"));
    }

    #[test]
    fn the_index_stops_at_the_first_blob() {
        // Blob bytes that would parse as another index entry must not be
        // read as one; the lowest offset seen is where the index ends.
        let mut blob = 0u32.to_le_bytes().to_vec();
        blob.extend_from_slice(b"00000000\0");
        let raw = container(&[("00100120", blob)]);
        let dat = KyraDat::parse(&raw).expect("index");
        assert_eq!(dat.ids().count(), 1);
    }

    #[test]
    fn a_missing_table_is_named_rather_than_guessed() {
        let raw = container(&[(ID_ROOM_FILENAMES, names(&["A"]))]);
        let dat = KyraDat::parse(&raw).expect("index");
        assert_eq!(
            dat.kyrandia1(),
            Err(KyraDatError::MissingEntry(ID_ROOM_LIST.into()))
        );
    }

    #[test]
    fn a_count_that_does_not_match_the_length_is_rejected() {
        // Reading the count little-endian gives a huge number; that has
        // to fail loudly rather than truncate to something plausible.
        let mut bad = list(&[(0, NO_EXIT, NO_EXIT, NO_EXIT, NO_EXIT)]);
        bad[0..4].copy_from_slice(&7u32.to_be_bytes());
        assert!(matches!(
            parse_room_graph(&names(&["A"]), &bad),
            Err(KyraDatError::BadRoomList { count: 7, .. })
        ));
    }

    #[test]
    fn a_link_to_a_background_that_does_not_exist_is_rejected() {
        assert_eq!(
            parse_room_graph(&names(&["A"]), &list(&[(9, 0, 0, 0, 0)])),
            Err(KyraDatError::NameOutOfRange { room: 0, index: 9 })
        );
    }

    #[test]
    fn a_file_that_is_not_a_container_is_rejected() {
        assert_eq!(KyraDat::parse(&[]).err(), Some(KyraDatError::NoIndex));
        assert_eq!(
            KyraDat::parse(b"not an index at all, just text").err(),
            Some(KyraDatError::NoIndex)
        );
    }
}
