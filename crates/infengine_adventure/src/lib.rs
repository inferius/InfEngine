//! Point-and-click adventure runtime for InfEngine.
//!
//! This crate is the *data model and rules* of a classic adventure game:
//! rooms made of painted layers, a walkable floor, depth-based actor
//! scaling, and clickable hotspots and exits. It deliberately knows
//! nothing about rendering, windowing, or any particular game's file
//! formats - it is what both the runtime and the editor agree on.
//!
//! Room space runs x right, y down, origin top-left, one unit = one
//! logical pixel, matching how a background image is authored.
//!
//! ```
//! use glam::Vec2;
//! use infengine_adventure::{Polygon, Room, WalkArea, find_path};
//!
//! let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
//! room.walk.solids.push(Polygon::rect(
//!     Vec2::new(0.0, 120.0),
//!     Vec2::new(320.0, 200.0),
//! ));
//!
//! // A click above the floor walks to the nearest reachable spot.
//! let path = find_path(&room.walk, Vec2::new(20.0, 180.0), Vec2::new(300.0, 10.0))
//!     .expect("the room has a floor");
//! assert!(room.walk.contains(*path.last().expect("non-empty")));
//! ```

#![forbid(unsafe_code)]

pub mod action;
pub mod anim;
pub mod game;
pub mod geom;
pub mod host;
pub mod interact;
pub mod io;
pub mod mask;
/// Localisation, re-exported from [`infengine_loca`].
///
/// It lives in its own crate because a menu or a quest log needs the
/// same thing and should not have to depend on an adventure runtime.
/// The path stays here so nothing that already writes
/// `infengine_adventure::loca` has to change.
pub use infengine_loca as loca;

pub mod pack;
pub mod path;
pub mod project;
pub mod room;
pub mod save;
pub mod scale;
pub mod sound;
pub mod story;
pub mod screen;
pub mod script_io;
pub mod walk;

pub use action::{Action, Script};
pub use anim::{Animation, AnimationSet};
pub use game::{Actor, Click, Game, GameState, Runner};
pub use host::Host;
pub use interact::{EntryPoint, Exit, Facing, Hotspot, Shape, Verb};
pub use io::{RoomIoError, load, save};
pub use loca::{LanguageChoice, Line, Locale, Locales, Spoken};
pub use infengine_loca::plural::Category as PluralCategory;
pub use mask::trace as trace_walk_mask;
pub use path::{PathError, find_path, path_length, point_along};
pub use project::Project;
pub use room::{Layer, LayerKind, Room};
pub use save::Save;
pub use story::Story;
pub use screen::{Screen, Widget, WidgetKind};
pub use scale::{ScaleRamp, ScaleZone};
pub use walk::{Polygon, WalkArea};
