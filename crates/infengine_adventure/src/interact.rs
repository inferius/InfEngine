//! A2.5: the things in a room a player can click on.
//!
//! A hotspot is a shape over the background plus the metadata the game
//! needs when the player clicks it: what to call it in the status line,
//! where the actor should stand to reach it, which way to face, and
//! which script runs per verb. Exits are the same idea specialised to
//! room changes, which are common enough to deserve their own type
//! instead of a script that every room has to repeat.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::geom;
use crate::walk::Polygon;

/// Which way the actor faces. Four directions is what a hand-drawn
/// sprite sheet realistically covers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    Left,
    Right,
    /// Away from the camera.
    Back,
    /// Toward the camera.
    #[default]
    Front,
}

/// The verb a click carries.
///
/// A one-click interface still needs verbs internally: the same hotspot
/// says something different when examined than when used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Verb {
    /// Just walk there; the default for empty floor.
    #[default]
    Walk,
    Look,
    Use,
    Talk,
    Take,
}

impl Verb {
    /// All verbs, for editor dropdowns and cursor cycling.
    pub const ALL: [Verb; 5] = [Verb::Walk, Verb::Look, Verb::Use, Verb::Talk, Verb::Take];

    /// Lowercase name, used as the RON key and in the status line.
    pub fn name(self) -> &'static str {
        match self {
            Verb::Walk => "walk",
            Verb::Look => "look",
            Verb::Use => "use",
            Verb::Talk => "talk",
            Verb::Take => "take",
        }
    }
}

/// A clickable region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// Axis-aligned rectangle, given as opposite corners.
    Rect { min: Vec2, max: Vec2 },
    /// Arbitrary polygon, for anything a rectangle would over-claim.
    Poly(Polygon),
}

impl Default for Shape {
    fn default() -> Self {
        Shape::Rect {
            min: Vec2::ZERO,
            max: Vec2::ZERO,
        }
    }
}

impl Shape {
    pub fn contains(&self, p: Vec2) -> bool {
        match self {
            Shape::Rect { min, max } => {
                let (lo, hi) = (min.min(*max), min.max(*max));
                p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y
            }
            Shape::Poly(poly) => poly.contains_or_boundary(p),
        }
    }

    /// Centre of the shape's bounding box - the default place to anchor
    /// a label or a walk-to point.
    pub fn center(&self) -> Vec2 {
        match self {
            Shape::Rect { min, max } => (*min + *max) * 0.5,
            Shape::Poly(poly) => geom::bounds(&poly.points)
                .map_or(Vec2::ZERO, |(min, max)| (min + max) * 0.5),
        }
    }

    /// Area, used to pick the tightest hotspot when several overlap.
    pub fn area(&self) -> f32 {
        match self {
            Shape::Rect { min, max } => {
                let d = (*max - *min).abs();
                d.x * d.y
            }
            Shape::Poly(poly) => geom::area(&poly.points),
        }
    }
}

/// Something in the room the player can click.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Hotspot {
    /// Stable identifier referenced by scripts and save files.
    pub id: String,
    /// Display name for the status line.
    pub name: String,
    pub shape: Shape,
    /// Where the actor stands to interact. `None` means interact from
    /// wherever the actor already is.
    #[serde(default)]
    pub walk_to: Option<Vec2>,
    #[serde(default)]
    pub facing: Facing,
    /// Verb used when the player clicks without choosing one.
    #[serde(default)]
    pub default_verb: Verb,
    /// Script to run per verb, as `(verb, script id)` pairs. A verb with
    /// no entry falls back to the game's generic response.
    #[serde(default)]
    pub actions: Vec<(Verb, String)>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Hotspot {
    /// Script bound to `verb`, if any.
    pub fn action(&self, verb: Verb) -> Option<&str> {
        self.actions
            .iter()
            .find(|(v, _)| *v == verb)
            .map(|(_, id)| id.as_str())
    }

    /// Where the actor should stand: the explicit walk-to point, else
    /// the shape's centre.
    pub fn approach_point(&self) -> Vec2 {
        self.walk_to.unwrap_or_else(|| self.shape.center())
    }
}

/// A named spot an actor can be placed at when entering a room.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntryPoint {
    pub id: String,
    pub position: Vec2,
    #[serde(default)]
    pub facing: Facing,
}

/// A region that moves the actor to another room.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Exit {
    pub id: String,
    /// Display name for the status line, e.g. "to the courtyard".
    #[serde(default)]
    pub name: String,
    pub shape: Shape,
    /// Room id to load.
    pub target_room: String,
    /// [`EntryPoint::id`] in the target room. Empty means the target's
    /// first entry point.
    #[serde(default)]
    pub target_entry: String,
    /// Where the actor walks before the transition fires.
    #[serde(default)]
    pub walk_to: Option<Vec2>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Exit {
    pub fn approach_point(&self) -> Vec2 {
        self.walk_to.unwrap_or_else(|| self.shape.center())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Shape {
        Shape::Rect {
            min: Vec2::new(x0, y0),
            max: Vec2::new(x1, y1),
        }
    }

    #[test]
    fn rect_contains_edges_and_interior() {
        let s = rect(10.0, 10.0, 20.0, 30.0);
        assert!(s.contains(Vec2::new(15.0, 20.0)));
        assert!(s.contains(Vec2::new(10.0, 10.0)));
        assert!(s.contains(Vec2::new(20.0, 30.0)));
        assert!(!s.contains(Vec2::new(9.0, 20.0)));
        assert!(!s.contains(Vec2::new(15.0, 31.0)));
    }

    #[test]
    fn rect_with_swapped_corners_still_works() {
        // The editor lets you drag a rectangle in any direction.
        let s = rect(20.0, 30.0, 10.0, 10.0);
        assert!(s.contains(Vec2::new(15.0, 20.0)));
        assert!(!s.contains(Vec2::new(25.0, 20.0)));
    }

    #[test]
    fn rect_center_and_area() {
        let s = rect(10.0, 10.0, 20.0, 30.0);
        assert_eq!(s.center(), Vec2::new(15.0, 20.0));
        assert!((s.area() - 200.0).abs() < 1e-3);
    }

    #[test]
    fn poly_shape_delegates_to_the_polygon() {
        let s = Shape::Poly(Polygon::rect(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
        ));
        assert!(s.contains(Vec2::new(5.0, 5.0)));
        assert!(!s.contains(Vec2::new(15.0, 5.0)));
        assert_eq!(s.center(), Vec2::new(5.0, 5.0));
        assert!((s.area() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn hotspot_action_lookup() {
        let hs = Hotspot {
            id: "door".into(),
            name: "heavy door".into(),
            shape: rect(0.0, 0.0, 10.0, 10.0),
            actions: vec![
                (Verb::Look, "look_door".into()),
                (Verb::Use, "open_door".into()),
            ],
            ..Default::default()
        };
        assert_eq!(hs.action(Verb::Look), Some("look_door"));
        assert_eq!(hs.action(Verb::Use), Some("open_door"));
        assert_eq!(hs.action(Verb::Talk), None);
    }

    #[test]
    fn approach_point_prefers_the_explicit_walk_to() {
        let shape = rect(0.0, 0.0, 100.0, 100.0);
        let mut hs = Hotspot {
            shape: shape.clone(),
            ..Default::default()
        };
        // Without a walk-to, the actor would try to stand in the middle
        // of the object; with one, it stands where the designer said.
        assert_eq!(hs.approach_point(), Vec2::new(50.0, 50.0));
        hs.walk_to = Some(Vec2::new(50.0, 120.0));
        assert_eq!(hs.approach_point(), Vec2::new(50.0, 120.0));
    }

    #[test]
    fn verb_names_are_stable() {
        assert_eq!(Verb::ALL.len(), 5);
        assert_eq!(Verb::default(), Verb::Walk);
        assert_eq!(Verb::Take.name(), "take");
    }
}
