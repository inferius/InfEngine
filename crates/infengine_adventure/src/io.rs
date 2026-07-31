//! A2.7: reading and writing room files.
//!
//! Rooms are RON, pretty-printed, and meant to survive being opened in a
//! text editor and diffed in git. The importer writes them, the editor
//! rewrites them, and a human occasionally fixes one by hand - so field
//! order and indentation are stable rather than whatever serde emits by
//! default.

use std::fs;
use std::path::Path;

use crate::room::Room;

/// Conventional extension for room files.
pub const ROOM_EXTENSION: &str = "room.ron";

#[derive(Debug, thiserror::Error)]
pub enum RoomIoError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: ron::error::SpannedError,
    },
    #[error("failed to serialize room {id}: {source}")]
    Serialize {
        id: String,
        #[source]
        source: ron::Error,
    },
}

fn pretty() -> ron::ser::PrettyConfig {
    ron::ser::PrettyConfig::new()
        .indentor("  ")
        .struct_names(true)
        .separate_tuple_members(false)
        .enumerate_arrays(false)
}

/// Serialize a room to a RON string.
/// Directory holding room files, relative to the project root.
pub const ROOM_DIR: &str = "rooms";
/// Suffix identifying a room file.
pub const ROOM_SUFFIX: &str = ".room.ron";

pub fn to_string(room: &Room) -> Result<String, RoomIoError> {
    ron::ser::to_string_pretty(room, pretty()).map_err(|source| RoomIoError::Serialize {
        id: room.id.clone(),
        source,
    })
}

/// Parse a room from a RON string. `origin` only labels error messages.
pub fn from_string(text: &str, origin: &str) -> Result<Room, RoomIoError> {
    ron::from_str(text).map_err(|source| RoomIoError::Parse {
        path: origin.to_string(),
        source,
    })
}

/// Read a room file from disk.
pub fn load(path: &Path) -> Result<Room, RoomIoError> {
    let text = fs::read_to_string(path).map_err(|source| RoomIoError::Read {
        path: path.display().to_string(),
        source,
    })?;
    from_string(&text, &path.display().to_string())
}

/// Write a room file, creating parent directories as needed.
pub fn save(room: &Room, path: &Path) -> Result<(), RoomIoError> {
    let text = to_string(room)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| RoomIoError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    fs::write(path, text).map_err(|source| RoomIoError::Write {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interact::{EntryPoint, Exit, Hotspot, Shape, Verb};
    use crate::room::{Layer, LayerKind};
    use crate::scale::{ScaleRamp, ScaleZone};
    use crate::walk::Polygon;
    use glam::Vec2;

    fn rich_room() -> Room {
        let mut room = Room::new("crypt", Vec2::new(320.0, 200.0));
        room.name = "The Crypt".into();
        room.layers.push(Layer {
            name: "bg".into(),
            image: "crypt.png".into(),
            ..Default::default()
        });
        room.layers.push(Layer {
            name: "sarcophagus".into(),
            image: "crypt_overlay.png".into(),
            kind: LayerKind::Overlay { depth_y: 150.0 },
            ..Default::default()
        });
        room.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 120.0),
            Vec2::new(320.0, 200.0),
        ));
        room.walk.holes.push(Polygon::rect(
            Vec2::new(140.0, 140.0),
            Vec2::new(200.0, 170.0),
        ));
        room.scale = ScaleRamp::new(vec![ScaleZone {
            y_near: 200.0,
            y_far: 120.0,
            scale_near: 1.0,
            scale_far: 0.5,
        }]);
        room.hotspots.push(Hotspot {
            id: "urn".into(),
            name: "cracked urn".into(),
            shape: Shape::Rect {
                min: Vec2::new(40.0, 100.0),
                max: Vec2::new(70.0, 140.0),
            },
            walk_to: Some(Vec2::new(55.0, 150.0)),
            default_verb: Verb::Take,
            actions: vec![(Verb::Look, "look_urn".into())],
            ..Default::default()
        });
        room.exits.push(Exit {
            id: "stairs".into(),
            name: "up the stairs".into(),
            shape: Shape::Poly(Polygon::rect(
                Vec2::new(280.0, 100.0),
                Vec2::new(320.0, 180.0),
            )),
            target_room: "chapel".into(),
            target_entry: "down".into(),
            enabled: true,
            ..Default::default()
        });
        room.entries.push(EntryPoint {
            id: "down".into(),
            position: Vec2::new(300.0, 175.0),
            ..Default::default()
        });
        room
    }

    #[test]
    fn round_trip_preserves_everything() {
        let room = rich_room();
        let text = to_string(&room).expect("serialize");
        let back = from_string(&text, "<test>").expect("parse");
        assert_eq!(room, back);
    }

    #[test]
    fn serialized_form_is_human_readable() {
        let text = to_string(&rich_room()).expect("serialize");
        assert!(text.contains("id: \"crypt\""));
        assert!(text.contains("cracked urn"));
        // Multi-line, indented, not a single blob.
        assert!(text.lines().count() > 20);
    }

    #[test]
    fn missing_optional_fields_get_defaults() {
        // The minimum a hand-written room can be.
        // glam serializes Vec2 as a plain pair, not named fields.
        let text = r#"Room(id: "bare", size: (320.0, 200.0))"#;
        let room = from_string(text, "<test>").expect("parse");
        assert_eq!(room.id, "bare");
        assert!(room.layers.is_empty());
        assert!(room.walk.solids.is_empty());
        assert!(room.scale.is_flat());
        assert_eq!(room.name, "");
    }

    #[test]
    fn parse_error_names_its_origin() {
        let err = from_string("Room(id: ", "rooms/broken.room.ron").expect_err("should fail");
        assert!(err.to_string().contains("rooms/broken.room.ron"));
    }

    #[test]
    fn save_and_load_round_trip_through_a_file() {
        let dir = std::env::temp_dir().join("infengine_adventure_io_test");
        let path = dir.join("nested").join("crypt.room.ron");
        let _ = fs::remove_dir_all(&dir);

        let room = rich_room();
        save(&room, &path).expect("save creates parent dirs");
        let back = load(&path).expect("load");
        assert_eq!(room, back);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_of_a_missing_file_reports_the_path() {
        let err = load(Path::new("definitely/not/here.room.ron")).expect_err("should fail");
        assert!(err.to_string().contains("definitely"));
    }
}
