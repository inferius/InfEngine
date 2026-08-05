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

/// A layer that appears only in a certain state of the world.
///
/// A93: this is how one room wears its variants - the table with the
/// letter and the table without are two layers over one background,
/// each shown while its flag agrees. The alternative was one image per
/// state of the room, which multiplies pictures by every flag that
/// touches them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerCondition {
    /// The flag that decides.
    pub flag: String,
    /// Show the layer when the flag has this value. An unset flag
    /// counts as `false`, so `value: false` shows the layer until a
    /// script first sets the flag.
    #[serde(default = "default_true")]
    pub value: bool,
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
    /// Drawn only while this condition holds, if given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown_if: Option<LayerCondition>,
}

impl Layer {
    /// Whether the layer draws, given the game's flags.
    ///
    /// `visible` is the author's own switch; the condition is the
    /// game's. Both must agree.
    pub fn shown(&self, flag_of: impl Fn(&str) -> bool) -> bool {
        if !self.visible {
            return false;
        }
        match &self.shown_if {
            Some(condition) => flag_of(&condition.flag) == condition.value,
            None => true,
        }
    }
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
            shown_if: None,
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

/// How much of a room is on screen, and how the view moves.
///
/// A room the size of the window needs none of this, and most are. It
/// exists for the ones that are not: a corridor twice as wide as the
/// screen, a cliff the camera climbs, an intro that scrolls from the sky
/// down to the island. Without it a room can only ever be one screen,
/// which is a limit on the games that can be made rather than on the
/// engine's difficulty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// How much of the room is visible at once.
    ///
    /// Zero on either axis means the whole room that way, which is what
    /// a room without a camera is and what every room file written
    /// before this says.
    #[serde(default)]
    pub view: Vec2,
    /// Whether the view keeps the actor in frame as they walk.
    ///
    /// Off by default: a cutscene room has no actor to follow, and a
    /// camera that lurches after one during a pan is worse than one that
    /// does not move at all.
    #[serde(default)]
    pub follow: bool,
}

impl Camera {
    /// What the view actually covers in a room of this size.
    pub fn view_in(&self, room: Vec2) -> Vec2 {
        let width = if self.view.x > 0.0 { self.view.x } else { room.x };
        let height = if self.view.y > 0.0 { self.view.y } else { room.y };
        Vec2::new(width.min(room.x), height.min(room.y))
    }

    /// Does the room move under the view at all?
    pub fn scrolls(&self, room: Vec2) -> bool {
        let view = self.view_in(room);
        view.x < room.x || view.y < room.y
    }

    /// Keep a view position inside the room.
    ///
    /// The position is the view's top-left, so the far edge is the
    /// room's size less the view's - clamping to the room's size instead
    /// would let the camera run off it by a screenful.
    pub fn clamp(&self, room: Vec2, at: Vec2) -> Vec2 {
        let limit = (room - self.view_in(room)).max(Vec2::ZERO);
        at.clamp(Vec2::ZERO, limit)
    }

    /// Where the view sits to put `point` in the middle of it.
    pub fn centred_on(&self, room: Vec2, point: Vec2) -> Vec2 {
        self.clamp(room, point - self.view_in(room) * 0.5)
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
    /// How much of it is on screen, and whether the view follows.
    #[serde(default)]
    pub camera: Camera,
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

    #[test]
    fn a_layer_with_no_condition_draws_whenever_it_is_visible() {
        let layer = Layer {
            image: "bg.png".into(),
            ..Default::default()
        };
        assert!(layer.shown(|_| false));
        let hidden = Layer {
            visible: false,
            ..layer.clone()
        };
        assert!(!hidden.shown(|_| true), "the author's switch wins");
    }

    #[test]
    fn a_conditional_layer_follows_its_flag() {
        // The table with the letter and the table without: two layers
        // over one background, opposite conditions on one flag.
        let with_letter = Layer {
            image: "table_letter.png".into(),
            shown_if: Some(LayerCondition {
                flag: "took_letter".into(),
                value: false,
            }),
            ..Default::default()
        };
        let without = Layer {
            image: "table_bare.png".into(),
            shown_if: Some(LayerCondition {
                flag: "took_letter".into(),
                value: true,
            }),
            ..Default::default()
        };
        // Before the letter is taken the flag is unset, which reads as
        // false: the letter is on the table.
        assert!(with_letter.shown(|_| false));
        assert!(!without.shown(|_| false));
        // After.
        assert!(!with_letter.shown(|_| true));
        assert!(without.shown(|_| true));
    }

    #[test]
    fn a_condition_only_answers_for_its_own_flag() {
        let layer = Layer {
            shown_if: Some(LayerCondition {
                flag: "saw_wizard".into(),
                value: true,
            }),
            ..Default::default()
        };
        assert!(layer.shown(|flag| flag == "saw_wizard"));
        assert!(!layer.shown(|flag| flag == "something_else"));
    }

    #[test]
    fn a_room_file_written_before_conditions_still_loads() {
        // No shown_if in the text at all: the layer draws, as it did.
        let layer: Layer = ron::from_str(
            r#"(name: "bg", image: "hall.png", offset: (0.0, 0.0), visible: true)"#,
        )
        .expect("an older layer still parses");
        assert!(layer.shown_if.is_none());
        assert!(layer.shown(|_| false));
    }

    #[test]
    fn a_room_without_a_camera_is_all_on_screen() {
        // Every room file written before the camera existed says
        // nothing about one, and has to keep meaning what it meant.
        let camera = Camera::default();
        let room = Vec2::new(320.0, 200.0);
        assert_eq!(camera.view_in(room), room);
        assert!(!camera.scrolls(room));
        assert_eq!(camera.clamp(room, Vec2::new(500.0, 500.0)), Vec2::ZERO);
    }

    #[test]
    fn the_view_stops_at_the_far_edge_rather_than_at_the_corner() {
        // The position is the view's top-left, so clamping to the room's
        // own size would let the camera run a whole screen off the end.
        let camera = Camera {
            view: Vec2::new(320.0, 200.0),
            follow: false,
        };
        let room = Vec2::new(320.0, 400.0);
        assert!(camera.scrolls(room));
        assert_eq!(camera.clamp(room, Vec2::new(0.0, 999.0)), Vec2::new(0.0, 200.0));
        assert_eq!(camera.clamp(room, Vec2::new(0.0, -50.0)), Vec2::ZERO);
    }

    #[test]
    fn centring_on_a_point_near_an_edge_still_fills_the_screen() {
        // Otherwise following someone into a corner shows a band of
        // nothing beside them.
        let camera = Camera {
            view: Vec2::new(320.0, 200.0),
            follow: true,
        };
        let room = Vec2::new(640.0, 200.0);
        assert_eq!(camera.centred_on(room, Vec2::new(320.0, 100.0)).x, 160.0);
        assert_eq!(camera.centred_on(room, Vec2::new(10.0, 100.0)).x, 0.0);
        assert_eq!(camera.centred_on(room, Vec2::new(630.0, 100.0)).x, 320.0);
    }

    #[test]
    fn a_view_bigger_than_its_room_is_the_room() {
        let camera = Camera {
            view: Vec2::new(800.0, 600.0),
            follow: false,
        };
        let room = Vec2::new(320.0, 200.0);
        assert_eq!(camera.view_in(room), room);
        assert!(!camera.scrolls(room));
    }
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
