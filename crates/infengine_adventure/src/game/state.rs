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

/// The play state of a game in progress.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    /// Id of the room the player is in.
    #[serde(default)]
    pub room: String,
    #[serde(default)]
    pub actor: Actor,
    /// Story flags. Absent means false, so only what has happened is
    /// stored and a saved game stays readable.
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
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
    }

    pub fn flag(&self, flag: &str) -> bool {
        self.flags.get(flag).copied().unwrap_or(false)
    }

    pub fn set_flag(&mut self, flag: impl Into<String>, value: bool) {
        self.flags.insert(flag.into(), value);
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
