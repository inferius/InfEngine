//! A19.1: everything the game remembers.
//!
//! Which room is open, where the actor stands, what the player carries,
//! and which story flags are set. It is deliberately small and fully
//! serialisable: the editor's test harness is nothing more than a way to
//! author one of these by hand and drop the game into it.
//!
//! Hotspot enabling lives here as an *override* rather than being
//! written back into the room. A room file describes the room as the
//! author drew it; whether the loose brick is still there at this point
//! in the story is play state, and mixing the two would mean testing a
//! scene silently rewrites the project.

use std::collections::{BTreeMap, BTreeSet};

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::interact::Facing;
use crate::room::Room;

use super::actor::Actor;

/// One item lying on a floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundItem {
    pub item: String,
    /// Top-left of its sprite, in room pixels.
    pub at: Vec2,
}

/// One script-made change to a room's floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloorChange {
    pub min: Vec2,
    pub max: Vec2,
    /// Opened up to walking, or blocked off.
    pub open: bool,
}

/// The play state of a game in progress.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    /// Id of the room the player is in.
    #[serde(default)]
    pub room: String,
    #[serde(default)]
    pub actor: Actor,
    /// Top-left of the view in the room, and where it is heading.
    ///
    /// Part of the saved game because a game saved mid-pan should come
    /// back looking at the same thing, not snap to a corner.
    #[serde(default)]
    pub camera: Vec2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera_target: Option<(Vec2, f32)>,
    /// Story flags. Absent means false, so only what has happened is
    /// stored and a saved game stays readable.
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
    /// The one item held against the world - the thing on the cursor.
    /// Not the inventory; scripts test it with `Condition::Holding`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<String>,
    /// Where the last click landed, while its actions run.
    ///
    /// Scripts branch on where the player pointed relative to the
    /// actor; the point has to survive from the click into the script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click: Option<Vec2>,
    /// Items lying on floors, by room. World state: they stay where
    /// they were left when the actor walks away and comes back.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ground: BTreeMap<String, Vec<GroundItem>>,
    /// Floor changed by scripts, by room: bridges lowered, pits
    /// flooded. Applied over the authored floor, in order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub floor: BTreeMap<String, Vec<FloorChange>>,
    /// What the player carries, in the order it was picked up.
    #[serde(default)]
    pub inventory: Vec<String>,
    /// Hotspots a script has switched, keyed by id. Anything not listed
    /// is however the room file left it.
    #[serde(default)]
    pub hotspots: BTreeMap<String, bool>,
    /// Rooms the player has been in. The editor uses it to show what a
    /// test run has actually covered.
    #[serde(default)]
    pub visited: BTreeSet<String>,
}

impl GameState {
    /// Start in a room, at an entry point.
    pub fn start(room: &Room, entry: Option<&str>) -> Self {
        let mut state = Self::default();
        state.enter(room, entry);
        state
    }

    /// Move into a room and stand the actor at an entry point.
    ///
    /// An unnamed or unknown entry falls back to the room's first one,
    /// and a room with no entry points at all puts the actor in the
    /// middle of the floor. Neither is an error worth stopping a test
    /// run for - a half-authored room should still be enterable.
    pub fn enter(&mut self, room: &Room, entry: Option<&str>) {
        self.room = room.id.clone();
        self.visited.insert(room.id.clone());

        let point = entry
            .and_then(|id| room.entries.iter().find(|e| e.id == id))
            .or_else(|| room.entries.first());
        match point {
            Some(point) => self.actor.place(point.position, point.facing),
            None => {
                let middle = Vec2::new(room.size.x * 0.5, room.size.y * 0.75);
                let position = room.walk.clamp(middle).unwrap_or(middle);
                self.actor.place(position, Facing::Front);
            }
        }

        // The view arrives with the actor rather than at the room's
        // corner: walking into a wide room and seeing the far end of it
        // is disorienting, and a pan to catch up would be a pan nobody
        // asked for.
        self.camera_target = None;
        self.camera = if room.camera.follow {
            room.camera.centred_on(room.size, self.actor.position)
        } else {
            room.camera.clamp(room.size, Vec2::ZERO)
        };
    }

    /// Send the view somewhere, over `seconds` or at once.
    pub fn camera_to(&mut self, to: Vec2, seconds: f32) {
        if seconds > 0.0 {
            self.camera_target = Some((to, seconds));
        } else {
            self.camera = to;
            self.camera_target = None;
        }
    }

    /// Is the view still on its way somewhere?
    pub fn camera_moving(&self) -> bool {
        self.camera_target.is_some()
    }

    /// Advance the view by `dt`.
    ///
    /// A pan runs at whatever speed reaches its target in the time it
    /// was given, recomputed each step rather than fixed at the start -
    /// so a pan interrupted and redirected mid-way still lands on time
    /// instead of overshooting at the old speed.
    pub fn step_camera(&mut self, room: &Room, dt: f32) {
        if room.camera.follow && self.camera_target.is_none() {
            self.camera = room.camera.centred_on(room.size, self.actor.position);
            return;
        }
        let Some((to, left)) = self.camera_target else {
            return;
        };
        let dt = dt.max(0.0);
        if dt >= left || left <= 0.0 {
            self.camera = room.camera.clamp(room.size, to);
            self.camera_target = None;
            return;
        }
        let step = (to - self.camera) * (dt / left);
        self.camera = room.camera.clamp(room.size, self.camera + step);
        self.camera_target = Some((to, left - dt));
    }

    pub fn flag(&self, flag: &str) -> bool {
        self.flags.get(flag).copied().unwrap_or(false)
    }

    pub fn set_flag(&mut self, flag: impl Into<String>, value: bool) {
        self.flags.insert(flag.into(), value);
    }

    /// Lay an item on a room's floor.
    pub fn place(&mut self, room: impl Into<String>, item: impl Into<String>, at: Vec2) {
        self.ground
            .entry(room.into())
            .or_default()
            .push(GroundItem {
                item: item.into(),
                at,
            });
    }

    /// Take the first matching item off a room's floor.
    pub fn take_ground(&mut self, room: &str, item: &str) -> Option<GroundItem> {
        let items = self.ground.get_mut(room)?;
        let index = items.iter().position(|g| g.item == item)?;
        Some(items.remove(index))
    }

    /// What lies on a room's floor.
    pub fn ground_in(&self, room: &str) -> &[GroundItem] {
        self.ground.get(room).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The room's floor with every script-made change applied.
    ///
    /// Borrowed untouched in the common case of no changes, so walking
    /// costs nothing extra in rooms no script has remodelled.
    pub fn walk_area<'a>(&self, room: &'a Room) -> std::borrow::Cow<'a, crate::walk::WalkArea> {
        let Some(changes) = self.floor.get(&room.id).filter(|c| !c.is_empty()) else {
            return std::borrow::Cow::Borrowed(&room.walk);
        };
        let mut walk = room.walk.clone();
        for change in changes {
            let rect = crate::walk::Polygon::rect(change.min, change.max);
            if change.open {
                walk.solids.push(rect);
            } else {
                walk.holes.push(rect);
            }
        }
        std::borrow::Cow::Owned(walk)
    }

    pub fn has(&self, item: &str) -> bool {
        self.inventory.iter().any(|held| held == item)
    }

    /// Pick something up. Carrying two of the same thing is not a state
    /// any adventure puzzle wants, so a repeat is ignored.
    pub fn give(&mut self, item: impl Into<String>) {
        let item = item.into();
        if !self.has(&item) {
            self.inventory.push(item);
        }
    }

    pub fn take(&mut self, item: &str) {
        self.inventory.retain(|held| held != item);
    }

    /// Is this hotspot clickable right now?
    pub fn hotspot_enabled(&self, room: &Room, id: &str) -> bool {
        if let Some(state) = self.hotspots.get(id) {
            return *state;
        }
        room.hotspots
            .iter()
            .find(|h| h.id == id)
            .is_some_and(|h| h.enabled)
    }

    pub fn set_hotspot(&mut self, id: impl Into<String>, enabled: bool) {
        self.hotspots.insert(id.into(), enabled);
    }

    /// The hotspot under `p`, honouring the overrides.
    ///
    /// [`Room::hotspot_at`] cannot do this itself: it only sees the room
    /// file, and a script may since have switched something on or off.
    pub fn hotspot_at<'r>(
        &self,
        room: &'r Room,
        p: Vec2,
    ) -> Option<&'r crate::interact::Hotspot> {
        room.hotspots
            .iter()
            .filter(|h| self.hotspot_enabled(room, &h.id) && h.shape.contains(p))
            .min_by(|a, b| {
                a.shape
                    .area()
                    .partial_cmp(&b.shape.area())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room twice as tall as its view.
    fn tall_room() -> Room {
        let mut room = Room::new("cliff", Vec2::new(320.0, 400.0));
        room.camera = crate::room::Camera {
            view: Vec2::new(320.0, 200.0),
            follow: false,
        };
        room
    }

    #[test]
    fn a_cut_arrives_at_once_and_a_pan_takes_the_time_it_was_given() {
        let room = tall_room();
        let mut state = GameState::default();

        state.camera_to(Vec2::new(0.0, 200.0), 0.0);
        assert_eq!(state.camera, Vec2::new(0.0, 200.0));
        assert!(!state.camera_moving());

        state.camera = Vec2::ZERO;
        state.camera_to(Vec2::new(0.0, 200.0), 1.0);
        state.step_camera(&room, 0.5);
        assert!(state.camera_moving(), "half way is not there");
        assert!((state.camera.y - 100.0).abs() < 1.0, "{}", state.camera.y);
        state.step_camera(&room, 0.5);
        assert_eq!(state.camera, Vec2::new(0.0, 200.0));
        assert!(!state.camera_moving());
    }

    #[test]
    fn a_pan_redirected_half_way_still_lands_on_time() {
        // The speed is worked out afresh each step rather than fixed
        // when the pan starts, so changing its mind does not overshoot.
        let room = tall_room();
        let mut state = GameState::default();
        state.camera_to(Vec2::new(0.0, 200.0), 1.0);
        state.step_camera(&room, 0.5);
        state.camera_to(Vec2::new(0.0, 0.0), 0.5);
        state.step_camera(&room, 0.5);
        assert_eq!(state.camera, Vec2::ZERO);
    }

    #[test]
    fn a_pan_never_leaves_the_room() {
        let room = tall_room();
        let mut state = GameState::default();
        state.camera_to(Vec2::new(0.0, 9999.0), 1.0);
        for _ in 0..120 {
            state.step_camera(&room, 1.0 / 60.0);
        }
        assert_eq!(state.camera, Vec2::new(0.0, 200.0), "the far edge, not past it");
    }

    #[test]
    fn a_following_camera_keeps_up_but_yields_to_a_pan() {
        // A camera that lurched after the actor mid-pan would fight the
        // script for the shot.
        let mut room = tall_room();
        room.camera.follow = true;
        let mut state = GameState::default();
        state.actor.place(Vec2::new(160.0, 300.0), Default::default());

        state.step_camera(&room, 1.0 / 60.0);
        assert_eq!(state.camera.y, 200.0, "centred on the actor, clamped");

        state.camera_to(Vec2::ZERO, 1.0);
        state.step_camera(&room, 1.0);
        assert_eq!(state.camera, Vec2::ZERO, "the script won");
    }

    #[test]
    fn entering_a_room_puts_the_view_where_the_actor_is() {
        // Walking into a tall room and being shown its ceiling is
        // disorienting, and a pan to catch up is a pan nobody asked for.
        let mut room = tall_room();
        room.camera.follow = true;
        room.entries.push(crate::interact::EntryPoint {
            id: "bottom".into(),
            position: Vec2::new(160.0, 380.0),
            facing: Default::default(),
        });
        let mut state = GameState::default();
        state.enter(&room, None);
        assert_eq!(state.camera.y, 200.0);
        assert!(!state.camera_moving());
    }
    use crate::interact::{EntryPoint, Hotspot, Shape};
    use crate::walk::Polygon;

    fn room() -> Room {
        let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
        room.walk.solids.push(Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        room.entries.push(EntryPoint {
            id: "door".into(),
            position: Vec2::new(20.0, 150.0),
            facing: Facing::Right,
        });
        room.entries.push(EntryPoint {
            id: "stairs".into(),
            position: Vec2::new(300.0, 150.0),
            facing: Facing::Left,
        });
        room
    }

    #[test]
    fn entering_stands_the_actor_at_the_named_point() {
        let state = GameState::start(&room(), Some("stairs"));
        assert_eq!(state.actor.position, Vec2::new(300.0, 150.0));
        assert_eq!(state.actor.facing, Facing::Left);
        assert!(state.visited.contains("hall"));
    }

    #[test]
    fn an_unknown_entry_falls_back_rather_than_failing() {
        let state = GameState::start(&room(), Some("cellar"));
        assert_eq!(state.actor.position, Vec2::new(20.0, 150.0));
    }

    #[test]
    fn a_room_without_entries_still_puts_the_actor_on_the_floor() {
        let mut room = room();
        room.entries.clear();
        let state = GameState::start(&room, None);
        assert!(
            room.walk.contains(state.actor.position),
            "a half-authored room must still be enterable"
        );
    }

    #[test]
    fn an_override_beats_the_room_file_in_both_directions() {
        let mut room = room();
        room.hotspots.push(Hotspot {
            id: "brick".into(),
            shape: Shape::Rect {
                min: Vec2::new(0.0, 0.0),
                max: Vec2::new(50.0, 50.0),
            },
            enabled: true,
            ..Default::default()
        });
        let mut state = GameState::start(&room, None);
        assert!(state.hotspot_enabled(&room, "brick"));

        state.set_hotspot("brick", false);
        assert!(!state.hotspot_enabled(&room, "brick"));
        assert!(
            state.hotspot_at(&room, Vec2::new(10.0, 10.0)).is_none(),
            "a disabled hotspot is not clickable"
        );

        state.set_hotspot("brick", true);
        assert!(state.hotspot_at(&room, Vec2::new(10.0, 10.0)).is_some());
        assert!(
            room.hotspots[0].enabled,
            "and the room file was never touched"
        );
    }

    #[test]
    fn the_inventory_holds_one_of_each() {
        let mut state = GameState::default();
        state.give("key");
        state.give("key");
        assert_eq!(state.inventory, vec!["key"]);
        state.take("key");
        assert!(!state.has("key"));
    }

    #[test]
    fn a_state_survives_a_round_trip() {
        let mut state = GameState::start(&room(), Some("door"));
        state.set_flag("met_brynn", true);
        state.give("amulet");
        state.set_hotspot("brick", false);

        let text = ron::ser::to_string(&state).expect("serialise");
        let back: GameState = ron::from_str(&text).expect("deserialise");
        assert_eq!(back, state);
    }
}
#[cfg(test)]
mod floor_tests {
    use super::*;

    #[test]
    fn a_script_made_bridge_is_walkable_and_a_blocked_pit_is_not() {
        let mut room = Room::new("chasm", Vec2::new(320.0, 200.0));
        room.walk.solids.push(crate::walk::Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(100.0, 200.0),
        ));
        let mut state = GameState::default();
        state.enter(&room, None);

        // Beyond the authored floor, until the bridge lowers.
        assert!(!state.walk_area(&room).contains(Vec2::new(150.0, 150.0)));
        state.floor.entry("chasm".into()).or_default().push(FloorChange {
            min: Vec2::new(100.0, 140.0),
            max: Vec2::new(200.0, 160.0),
            open: true,
        });
        assert!(state.walk_area(&room).contains(Vec2::new(150.0, 150.0)));

        // And the reverse: the authored floor, blocked off.
        state.floor.entry("chasm".into()).or_default().push(FloorChange {
            min: Vec2::new(20.0, 120.0),
            max: Vec2::new(40.0, 140.0),
            open: false,
        });
        assert!(!state.walk_area(&room).contains(Vec2::new(30.0, 130.0)));
        // A room nothing touched borrows the floor as it was authored.
        let other = Room::new("hall", Vec2::new(320.0, 200.0));
        assert!(matches!(
            state.walk_area(&other),
            std::borrow::Cow::Borrowed(_)
        ));
    }
}
