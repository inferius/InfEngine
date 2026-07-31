//! A19.1: the character the player moves.
//!
//! An actor is a position, a facing, and whatever is left of the path it
//! was told to walk. Pathfinding itself lives in [`crate::path`]; this
//! only consumes the result, one waypoint at a time.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::interact::Facing;
use crate::path::find_path;
use crate::scale::ScaleRamp;
use crate::walk::WalkArea;

/// How fast an actor walks, in room units per second.
///
/// Room units are logical pixels, so this is "a 320-wide screen crossed
/// in four seconds" - about what the games of the era used.
pub const DEFAULT_SPEED: f32 = 80.0;

/// A character on screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    pub position: Vec2,
    #[serde(default)]
    pub facing: Facing,
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Waypoints still to reach, nearest first.
    ///
    /// Not part of a saved game in any meaningful sense, but keeping it
    /// serialisable costs nothing and lets the editor snapshot a scene
    /// mid-walk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    path: Vec<Vec2>,
}

fn default_speed() -> f32 {
    DEFAULT_SPEED
}

impl Default for Actor {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            facing: Facing::default(),
            speed: DEFAULT_SPEED,
            path: Vec::new(),
        }
    }
}

impl Actor {
    pub fn new(position: Vec2) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }

    /// Put the actor somewhere without walking there.
    pub fn place(&mut self, position: Vec2, facing: Facing) {
        self.position = position;
        self.facing = facing;
        self.path.clear();
    }

    /// Plan a walk to `target`, replacing any walk in progress.
    ///
    /// Returns whether there is now something to walk. A click on the
    /// spot the actor already stands on is not a failure, it is simply
    /// nothing to do - the caller must not block on it.
    pub fn walk_to(&mut self, area: &WalkArea, target: Vec2) -> bool {
        self.path.clear();
        let Ok(path) = find_path(area, self.position, target) else {
            return false;
        };
        // find_path includes the starting point; walking to it would be
        // a zero-length first leg that only confuses the facing.
        self.path = path.into_iter().skip(1).collect();
        self.trim_reached();
        !self.path.is_empty()
    }

    /// Follow a path that was worked out elsewhere.
    pub fn follow(&mut self, path: Vec<Vec2>) {
        self.path = path;
        self.trim_reached();
    }

    pub fn is_walking(&self) -> bool {
        !self.path.is_empty()
    }

    /// Waypoints still ahead - what the editor draws as the walk line.
    pub fn path(&self) -> &[Vec2] {
        &self.path
    }

    /// Give up on the current walk where the actor stands.
    pub fn stop(&mut self) {
        self.path.clear();
    }

    /// Move along the path for `dt` seconds.
    ///
    /// Leftover distance carries into the next leg, so a corner never
    /// costs a frame of movement: at 60 Hz a path with many short legs
    /// would otherwise walk visibly slower than a straight one.
    pub fn advance(&mut self, dt: f32) {
        let mut budget = self.speed * dt.max(0.0);
        while budget > 0.0 {
            let Some(next) = self.path.first().copied() else {
                break;
            };
            let delta = next - self.position;
            let distance = delta.length();
            if distance <= budget {
                self.face_along(delta);
                self.position = next;
                self.path.remove(0);
                budget -= distance;
            } else {
                self.face_along(delta);
                self.position += delta / distance * budget;
                break;
            }
        }
    }

    /// How large to draw the actor here, per the room's depth ramp.
    pub fn scale(&self, ramp: &ScaleRamp) -> f32 {
        ramp.scale_at(self.position.y)
    }

    /// Turn to look the way the actor is moving.
    ///
    /// Sideways wins ties because a sprite sheet's side views read as
    /// movement and its front view does not; a diagonal walk drawn
    /// front-on looks like sliding.
    fn face_along(&mut self, delta: Vec2) {
        if delta == Vec2::ZERO {
            return;
        }
        self.facing = if delta.x.abs() >= delta.y.abs() {
            if delta.x < 0.0 { Facing::Left } else { Facing::Right }
        } else if delta.y < 0.0 {
            Facing::Back
        } else {
            Facing::Front
        };
    }

    /// Drop leading waypoints the actor is already standing on.
    fn trim_reached(&mut self) {
        while self
            .path
            .first()
            .is_some_and(|p| p.distance_squared(self.position) < 1e-6)
        {
            self.path.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::Polygon;

    fn floor() -> WalkArea {
        let mut area = WalkArea::default();
        area.solids.push(Polygon::rect(
            Vec2::new(0.0, 100.0),
            Vec2::new(320.0, 200.0),
        ));
        area
    }

    #[test]
    fn an_actor_reaches_its_target_and_stops() {
        let mut actor = Actor::new(Vec2::new(10.0, 150.0));
        assert!(actor.walk_to(&floor(), Vec2::new(90.0, 150.0)));
        assert!(actor.is_walking());

        // Exactly one second at the default speed covers eighty units.
        actor.advance(1.0);
        assert!(actor.position.abs_diff_eq(Vec2::new(90.0, 150.0), 1e-3));
        assert!(!actor.is_walking(), "arriving ends the walk");
    }

    #[test]
    fn walking_to_where_you_stand_is_nothing_to_do() {
        let mut actor = Actor::new(Vec2::new(10.0, 150.0));
        // Not "false because it failed" - false because there is no walk
        // to wait for. A script blocking on this would hang forever.
        assert!(!actor.walk_to(&floor(), Vec2::new(10.0, 150.0)));
        assert!(!actor.is_walking());
    }

    #[test]
    fn leftover_distance_carries_across_a_corner() {
        let mut actor = Actor::new(Vec2::ZERO);
        actor.follow(vec![Vec2::new(40.0, 0.0), Vec2::new(40.0, 40.0)]);
        actor.advance(1.0);
        // Eighty units of budget, forty of them spent on the first leg:
        // the corner must not swallow the rest.
        assert!(actor.position.abs_diff_eq(Vec2::new(40.0, 40.0), 1e-3));
    }

    #[test]
    fn facing_follows_the_leg_being_walked() {
        let mut actor = Actor::new(Vec2::ZERO);
        actor.follow(vec![Vec2::new(-10.0, 0.0)]);
        actor.advance(0.01);
        assert_eq!(actor.facing, Facing::Left);

        actor.place(Vec2::ZERO, Facing::Front);
        actor.follow(vec![Vec2::new(0.0, -10.0)]);
        actor.advance(0.01);
        assert_eq!(actor.facing, Facing::Back, "up the screen is away");

        actor.place(Vec2::ZERO, Facing::Back);
        actor.follow(vec![Vec2::new(0.0, 10.0)]);
        actor.advance(0.01);
        assert_eq!(actor.facing, Facing::Front);
    }

    #[test]
    fn a_diagonal_walk_faces_sideways() {
        let mut actor = Actor::new(Vec2::ZERO);
        actor.follow(vec![Vec2::new(10.0, 10.0)]);
        actor.advance(0.01);
        assert_eq!(actor.facing, Facing::Right);
    }

    #[test]
    fn placing_an_actor_cancels_the_walk() {
        let mut actor = Actor::new(Vec2::new(10.0, 150.0));
        actor.walk_to(&floor(), Vec2::new(300.0, 150.0));
        actor.place(Vec2::new(5.0, 120.0), Facing::Back);
        assert!(!actor.is_walking());
        assert_eq!(actor.position, Vec2::new(5.0, 120.0));
    }

    #[test]
    fn a_click_off_the_floor_walks_to_the_nearest_reachable_spot() {
        let mut actor = Actor::new(Vec2::new(10.0, 150.0));
        let area = floor();
        assert!(actor.walk_to(&area, Vec2::new(160.0, 10.0)));
        actor.advance(10.0);
        assert!(area.contains(actor.position));
    }
}
