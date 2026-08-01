//! A2.6: a room - one screen of the game.
//!
//! Everything the engine needs to draw and run one screen: the painted
//! layers, the floor, the depth ramps, and the clickable things. Rooms
//! reference images by relative path rather than embedding them, so the
//! importer can regenerate art without touching hand-authored data.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::interact::{EntryPoint, Exit, Hotspot};
use crate::scale::ScaleRamp;
use crate::walk::WalkArea;

/// How a layer behaves relative to the actor and the camera.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum LayerKind {
    /// Painted behind everything. The usual case.
    #[default]
    Background,
    /// Drawn in front of the actor once the actor's feet pass `depth_y`,
    /// behind otherwise. This is the walk-behind trick that puts a
    /// pillar or a table edge in front of a character.
    Overlay { depth_y: f32 },
    /// Scrolls at a fraction of the camera's movement, for rooms wider
    /// than the window.
    Parallax { factor: Vec2 },
}

/// One painted plane of a room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    /// Image path relative to the room file's directory.
    pub image: String,
    /// Top-left corner of the image in room space.
    #[serde(default)]
    pub offset: Vec2,
    #[serde(default)]
    pub kind: LayerKind,
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            name: String::new(),
            image: String::new(),
            offset: Vec2::ZERO,
            kind: LayerKind::default(),
            visible: true,
        }
    }
}

impl Layer {
    /// Does this layer draw in front of an actor whose feet are at `y`?
    pub fn occludes_actor_at(&self, feet_y: f32) -> bool {
        match self.kind {
            LayerKind::Overlay { depth_y } => feet_y < depth_y,
            _ => false,
        }
    }
}

/// One screen of the game.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Room {
    /// Stable identifier, referenced by exits and save files.
    pub id: String,
    /// Author-facing name; also the default status-line room title.
    #[serde(default)]
    pub name: String,
    /// Prefix its scripts are named with, when that is not the id.
    ///
    /// Rooms usually own their logic and this stays empty. It is for
    /// the case where several rooms run the same script: Kyrandia's
    /// forest is thirteen distinct rooms drawn on one background and
    /// driven by one `FORESTA.EMC`, so all thirteen carry the prefix
    /// `foresta` while keeping ids of their own.
    #[serde(default)]
    pub script: String,
    /// Room extent in logical pixels. Usually the background's size.
    pub size: Vec2,
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub walk: WalkArea,
    #[serde(default)]
    pub scale: ScaleRamp,
    #[serde(default)]
    pub hotspots: Vec<Hotspot>,
    #[serde(default)]
    pub exits: Vec<Exit>,
    #[serde(default)]
    pub entries: Vec<EntryPoint>,
}

impl Room {
    /// What this room's scripts are named after: its own id, unless it
    /// shares them with other rooms.
    pub fn script_prefix(&self) -> &str {
        if self.script.is_empty() {
            &self.id
        } else {
            &self.script
        }
    }

    pub fn new(id: impl Into<String>, size: Vec2) -> Self {
        Self {
            id: id.into(),
            size,
            ..Default::default()
        }
    }

    /// Topmost enabled hotspot under `p`.
    ///
    /// When shapes overlap the smallest one wins: a doorknob drawn
    /// inside a door hotspot should be reachable, and it is always the
    /// tighter shape.
    pub fn hotspot_at(&self, p: Vec2) -> Option<&Hotspot> {
        self.hotspots
            .iter()
            .filter(|h| h.enabled && h.shape.contains(p))
            .min_by(|a, b| {
                a.shape
                    .area()
                    .partial_cmp(&b.shape.area())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Enabled exit under `p`.
    pub fn exit_at(&self, p: Vec2) -> Option<&Exit> {
        self.exits
            .iter()
            .find(|e| e.enabled && e.shape.contains(p))
    }

    /// Look up an entry point by id, falling back to the first one when
    /// `id` is empty or unknown. Returns `None` only for a room that
    /// declares no entry points at all.
    pub fn entry(&self, id: &str) -> Option<&EntryPoint> {
        if !id.is_empty()
            && let Some(found) = self.entries.iter().find(|e| e.id == id)
        {
            return Some(found);
        }
        self.entries.first()
    }

    /// Layers that draw behind the actor, in painting order.
    pub fn layers_behind(&self, feet_y: f32) -> impl Iterator<Item = &Layer> {
        self.layers
            .iter()
            .filter(move |l| l.visible && !l.occludes_actor_at(feet_y))
    }

    /// Layers that draw in front of the actor, in painting order.
    pub fn layers_in_front(&self, feet_y: f32) -> impl Iterator<Item = &Layer> {
        self.layers
            .iter()
            .filter(move |l| l.visible && l.occludes_actor_at(feet_y))
    }

    /// Problems that would make the room misbehave at runtime. Empty
    /// means the room is sound; the editor surfaces these as warnings
    /// rather than refusing to save a work in progress.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.id.is_empty() {
            issues.push("room has no id".to_string());
        }
        if self.size.x <= 0.0 || self.size.y <= 0.0 {
            issues.push(format!("room size {:?} is not positive", self.size));
        }
        if self.walk.is_empty() {
            issues.push("room has no walkable floor".to_string());
        }
        if self.entries.is_empty() {
            issues.push("room has no entry point".to_string());
        }
        for entry in &self.entries {
            if !self.walk.contains(entry.position) {
                issues.push(format!(
                    "entry point {:?} at {:?} is not on the floor",
                    entry.id, entry.position
                ));
            }
        }
        for exit in &self.exits {
            if exit.target_room.is_empty() {
                issues.push(format!("exit {:?} has no target room", exit.id));
            }
        }
        for hotspot in &self.hotspots {
            if hotspot.id.is_empty() {
                issues.push("a hotspot has no id".to_string());
            }
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interact::{Shape, Verb};
    use crate::walk::Polygon;

    fn rect_shape(x0: f32, y0: f32, x1: f32, y1: f32) -> Shape {
        Shape::Rect {
            min: Vec2::new(x0, y0),
            max: Vec2::new(x1, y1),
        }
    }

    fn sample_room() -> Room {
        let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
        room.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 120.0),
            Vec2::new(320.0, 200.0),
        ));
        room.entries.push(EntryPoint {
            id: "west".into(),
            position: Vec2::new(20.0, 180.0),
            ..Default::default()
        });
        room.entries.push(EntryPoint {
            id: "east".into(),
            position: Vec2::new(300.0, 180.0),
            ..Default::default()
        });
        room.hotspots.push(Hotspot {
            id: "door".into(),
            name: "door".into(),
            shape: rect_shape(100.0, 20.0, 180.0, 140.0),
            default_verb: Verb::Use,
            enabled: true,
            ..Default::default()
        });
        room.hotspots.push(Hotspot {
            id: "knob".into(),
            name: "doorknob".into(),
            shape: rect_shape(165.0, 80.0, 175.0, 90.0),
            enabled: true,
            ..Default::default()
        });
        room
    }

    #[test]
    fn smallest_overlapping_hotspot_wins() {
        let room = sample_room();
        // Inside both the door and the knob.
        let hit = room.hotspot_at(Vec2::new(170.0, 85.0)).expect("hit");
        assert_eq!(hit.id, "knob");
        // Only inside the door.
        let hit = room.hotspot_at(Vec2::new(110.0, 85.0)).expect("hit");
        assert_eq!(hit.id, "door");
    }

    #[test]
    fn disabled_hotspots_are_skipped() {
        let mut room = sample_room();
        room.hotspots[1].enabled = false;
        let hit = room.hotspot_at(Vec2::new(170.0, 85.0)).expect("hit");
        assert_eq!(hit.id, "door");
    }

    #[test]
    fn click_on_empty_background_hits_nothing() {
        let room = sample_room();
        assert!(room.hotspot_at(Vec2::new(5.0, 5.0)).is_none());
    }

    #[test]
    fn entry_lookup_falls_back_to_the_first() {
        let room = sample_room();
        assert_eq!(room.entry("east").expect("found").id, "east");
        assert_eq!(room.entry("").expect("fallback").id, "west");
        assert_eq!(room.entry("nonexistent").expect("fallback").id, "west");
        assert!(Room::default().entry("west").is_none());
    }

    #[test]
    fn overlay_splits_layers_by_actor_depth() {
        let mut room = sample_room();
        room.layers.push(Layer {
            name: "bg".into(),
            image: "hall.png".into(),
            ..Default::default()
        });
        room.layers.push(Layer {
            name: "pillar".into(),
            image: "pillar.png".into(),
            kind: LayerKind::Overlay { depth_y: 160.0 },
            ..Default::default()
        });
        // Actor deep in the room (small y): the pillar is in front.
        assert_eq!(room.layers_behind(140.0).count(), 1);
        assert_eq!(room.layers_in_front(140.0).count(), 1);
        // Actor at the front of the room: the pillar is behind.
        assert_eq!(room.layers_behind(180.0).count(), 2);
        assert_eq!(room.layers_in_front(180.0).count(), 0);
    }

    #[test]
    fn invisible_layers_never_draw() {
        let mut room = sample_room();
        room.layers.push(Layer {
            name: "bg".into(),
            image: "hall.png".into(),
            visible: false,
            ..Default::default()
        });
        assert_eq!(room.layers_behind(180.0).count(), 0);
        assert_eq!(room.layers_in_front(180.0).count(), 0);
    }

    #[test]
    fn validate_accepts_a_sound_room() {
        assert!(sample_room().validate().is_empty());
    }

    #[test]
    fn validate_flags_an_entry_off_the_floor() {
        let mut room = sample_room();
        room.entries[0].position = Vec2::new(20.0, 10.0);
        let issues = room.validate();
        assert!(
            issues.iter().any(|i| i.contains("not on the floor")),
            "got {issues:?}"
        );
    }

    #[test]
    fn validate_flags_an_empty_room() {
        let issues = Room::default().validate();
        assert!(issues.iter().any(|i| i.contains("no id")));
        assert!(issues.iter().any(|i| i.contains("no walkable floor")));
        assert!(issues.iter().any(|i| i.contains("no entry point")));
    }

    #[test]
    fn validate_flags_a_dangling_exit() {
        let mut room = sample_room();
        room.exits.push(Exit {
            id: "north".into(),
            shape: rect_shape(0.0, 0.0, 10.0, 10.0),
            enabled: true,
            ..Default::default()
        });
        let issues = room.validate();
        assert!(issues.iter().any(|i| i.contains("no target room")));
    }
}
