//! A4.2: the Black Mirror 2 / 3 import backend.
//!
//! Both games use the same scene format, so one backend covers them.
//! Each `Scenes/*.dat` holds one screen, and its node graph lines up
//! almost exactly with our own room model:
//!
//! | theirs | ours |
//! |--------|------|
//! | `Project` w/h | [`Room::size`] |
//! | `Layer` x/y/w/h/file | [`Layer`] |
//! | `Layer` ysort | walk-behind [`LayerKind::Overlay`] |
//! | `Layer` scroll | [`LayerKind::Parallax`] |
//! | `HotSpot` + `Point` list | [`Hotspot`] with a polygon shape |
//!
//! What this backend does *not* yet produce: pixels. Layer images are
//! named as `.tga` but stored as DDS inside the same `.dat`, and pulling
//! them out is a separate step. Rooms therefore import with correct
//! geometry and references, and the images resolve once that lands.

pub mod scene;

use std::fs;
use std::path::{Path, PathBuf};

use glam::Vec2;
use infengine_adventure::{Hotspot, Layer, LayerKind, Polygon, Room, Shape};
use tracing::{debug, warn};

use crate::ImportError;
use crate::report::{ImportReport, Skipped};

/// Subdirectory of a retail install holding one `.dat` per screen.
const SCENES_DIR: &str = "Scenes";
/// Where room files are written.
const ROOM_DIR: &str = "rooms";

/// A layer with `scroll` at this value moves with the camera one for
/// one, which is not parallax at all.
const NO_PARALLAX: f32 = 1.0;

/// Import every scene under `source`.
///
/// `source` is the game directory; the scenes live in `Scenes`. A scene
/// that fails to parse is recorded and the run continues.
pub fn import_dir(source: &Path, out_dir: &Path) -> Result<ImportReport, ImportError> {
    let scenes_dir = if source.join(SCENES_DIR).is_dir() {
        source.join(SCENES_DIR)
    } else {
        source.to_path_buf()
    };

    let mut scenes: Vec<PathBuf> = fs::read_dir(&scenes_dir)
        .map_err(|err| ImportError::ReadDir {
            path: scenes_dir.display().to_string(),
            source: err,
        })?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dat")))
        .collect();
    scenes.sort();

    if scenes.is_empty() {
        return Err(ImportError::NothingToImport {
            path: scenes_dir.display().to_string(),
            looking_for: "Scenes/*.dat".into(),
        });
    }

    let mut report = ImportReport::default();
    for path in &scenes {
        match import_scene(path, out_dir) {
            Ok(()) => {
                report.archives += 1;
                report.rooms += 1;
            }
            Err(err) => {
                debug!(scene = %path.display(), %err, "scene skipped");
                report.skipped.push(Skipped {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                    reason: err.to_string(),
                });
            }
        }
    }

    if report.rooms == 0 {
        warn!("no scene parsed; the format may differ in this release");
    }
    Ok(report)
}

/// Convert one scene file into a room.
pub fn import_scene(path: &Path, out_dir: &Path) -> Result<(), ImportError> {
    let bytes = fs::read(path).map_err(|err| ImportError::ReadFile {
        path: path.display().to_string(),
        source: err,
    })?;
    let parsed = scene::parse(&bytes).map_err(|err| ImportError::Format {
        path: path.display().to_string(),
        detail: err.to_string(),
    })?;

    let room = to_room(&parsed);
    let file_name = format!("{}.room.ron", room.id);
    let room_path = out_dir.join(ROOM_DIR).join(file_name);
    infengine_adventure::save(&room, &room_path).map_err(|err| ImportError::WriteRoom {
        path: room_path.display().to_string(),
        detail: err.to_string(),
    })
}

/// Map a parsed scene onto our room model.
pub fn to_room(parsed: &scene::Scene) -> Room {
    let id = parsed.name.to_ascii_lowercase();
    let project = parsed.first_of_kind("Project");
    let size = Vec2::new(
        project.and_then(|p| p.number("w")).unwrap_or(0.0),
        project.and_then(|p| p.number("h")).unwrap_or(0.0),
    );

    let mut room = Room::new(id, size);
    room.name = parsed.name.clone();

    for element in parsed.of_kind("Layer") {
        let Some(image) = element.text("file") else {
            // A layer with no image is a marker for something the engine
            // draws itself; nothing for us to reference.
            continue;
        };
        room.layers.push(Layer {
            name: element.label().to_string(),
            image: image.to_string(),
            offset: Vec2::new(
                element.number("x").unwrap_or(0.0),
                element.number("y").unwrap_or(0.0),
            ),
            kind: layer_kind(element),
            visible: true,
        });
    }

    for element in parsed.of_kind("HotSpot") {
        let Some(shape) = hotspot_shape(&element.points) else {
            continue;
        };
        room.hotspots.push(Hotspot {
            id: element.label().to_string(),
            name: element.label().to_string(),
            shape,
            enabled: true,
            ..Default::default()
        });
    }

    room
}

/// Decide how a layer behaves from the attributes it carries.
///
/// `ysort` is the walk-behind threshold: the layer draws in front of an
/// actor standing above that line. `scroll` other than 1 means the layer
/// moves at a fraction of the camera.
fn layer_kind(element: &scene::Element) -> LayerKind {
    if let Some(depth_y) = element.number("ysort").filter(|y| *y > 0.0) {
        return LayerKind::Overlay { depth_y };
    }
    if let Some(scroll) = element
        .number("scroll")
        .filter(|s| (*s - NO_PARALLAX).abs() > f32::EPSILON)
    {
        return LayerKind::Parallax {
            factor: Vec2::splat(scroll),
        };
    }
    LayerKind::Background
}

/// Turn a hotspot's vertex list into a shape.
///
/// Three or more points enclose an area and become a polygon. Two points
/// describe opposite corners of a rectangle. A single point has no
/// extent, so it is dropped rather than invented into a box of some
/// arbitrary size that would silently mis-place the click target.
fn hotspot_shape(points: &[Vec2]) -> Option<Shape> {
    match points.len() {
        0 | 1 => None,
        2 => Some(Shape::Rect {
            min: points[0].min(points[1]),
            max: points[0].max(points[1]),
        }),
        _ => Some(Shape::Poly(Polygon::new(points.to_vec()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scene::{Element, Scene, Value};

    fn element(kind: &str, attrs: &[(&str, Value)], points: &[Vec2]) -> Element {
        Element {
            kind: kind.into(),
            attrs: attrs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
            points: points.to_vec(),
        }
    }

    fn sample_scene() -> Scene {
        Scene {
            name: "LighthouseDay".into(),
            elements: vec![
                element(
                    "Project",
                    &[
                        ("w", Value::Number(1280.0)),
                        ("h", Value::Number(800.0)),
                    ],
                    &[],
                ),
                element(
                    "Layer",
                    &[
                        ("label", Value::Text("bg".into())),
                        ("file", Value::Text("s097.tga".into())),
                        ("x", Value::Number(0.0)),
                        ("y", Value::Number(0.0)),
                    ],
                    &[],
                ),
                element(
                    "Layer",
                    &[
                        ("label", Value::Text("pillar".into())),
                        ("file", Value::Text("pillar.tga".into())),
                        ("ysort", Value::Number(644.0)),
                    ],
                    &[],
                ),
                element(
                    "Layer",
                    &[
                        ("label", Value::Text("clouds".into())),
                        ("file", Value::Text("clouds.tga".into())),
                        ("scroll", Value::Number(0.1)),
                    ],
                    &[],
                ),
                element(
                    "HotSpot",
                    &[("label", Value::Text("LighthouseNote".into()))],
                    &[
                        Vec2::new(10.0, 10.0),
                        Vec2::new(30.0, 10.0),
                        Vec2::new(30.0, 40.0),
                    ],
                ),
            ],
        }
    }

    #[test]
    fn room_takes_its_size_from_the_project_node() {
        let room = to_room(&sample_scene());
        assert_eq!(room.size, Vec2::new(1280.0, 800.0));
        assert_eq!(room.id, "lighthouseday");
        assert_eq!(room.name, "LighthouseDay");
    }

    #[test]
    fn a_scene_without_a_project_node_still_imports() {
        let scene = Scene {
            name: "Bare".into(),
            elements: Vec::new(),
        };
        let room = to_room(&scene);
        assert_eq!(room.size, Vec2::ZERO);
        // validate() should flag the missing size rather than the
        // importer inventing one.
        assert!(room.validate().iter().any(|i| i.contains("not positive")));
    }

    #[test]
    fn layers_keep_their_order_and_images() {
        let room = to_room(&sample_scene());
        assert_eq!(room.layers.len(), 3);
        assert_eq!(room.layers[0].name, "bg");
        assert_eq!(room.layers[0].image, "s097.tga");
    }

    #[test]
    fn ysort_becomes_a_walk_behind_overlay() {
        let room = to_room(&sample_scene());
        assert_eq!(
            room.layers[1].kind,
            LayerKind::Overlay { depth_y: 644.0 }
        );
        // An actor deep in the room is behind it, one at the front is not.
        assert!(room.layers[1].occludes_actor_at(500.0));
        assert!(!room.layers[1].occludes_actor_at(700.0));
    }

    #[test]
    fn scroll_becomes_parallax() {
        let room = to_room(&sample_scene());
        assert_eq!(
            room.layers[2].kind,
            LayerKind::Parallax {
                factor: Vec2::splat(0.1)
            }
        );
    }

    #[test]
    fn scroll_of_one_is_not_parallax() {
        let e = element("Layer", &[("scroll", Value::Number(1.0))], &[]);
        assert_eq!(layer_kind(&e), LayerKind::Background);
    }

    #[test]
    fn a_layer_without_an_image_is_dropped() {
        let scene = Scene {
            name: "S".into(),
            elements: vec![element(
                "Layer",
                &[("label", Value::Text("marker".into()))],
                &[],
            )],
        };
        assert!(to_room(&scene).layers.is_empty());
    }

    #[test]
    fn a_hotspot_outline_becomes_a_polygon() {
        let room = to_room(&sample_scene());
        assert_eq!(room.hotspots.len(), 1);
        let hotspot = &room.hotspots[0];
        assert_eq!(hotspot.id, "LighthouseNote");
        assert!(hotspot.shape.contains(Vec2::new(25.0, 15.0)));
        assert!(!hotspot.shape.contains(Vec2::new(5.0, 5.0)));
    }

    #[test]
    fn two_points_become_a_rectangle_in_either_drag_direction() {
        let shape = hotspot_shape(&[Vec2::new(30.0, 40.0), Vec2::new(10.0, 10.0)])
            .expect("two points make a rect");
        assert!(shape.contains(Vec2::new(20.0, 20.0)));
        assert!(!shape.contains(Vec2::new(35.0, 20.0)));
    }

    #[test]
    fn a_hotspot_with_too_few_points_is_dropped_not_invented() {
        assert!(hotspot_shape(&[]).is_none());
        assert!(hotspot_shape(&[Vec2::new(1.0, 1.0)]).is_none());
        let scene = Scene {
            name: "S".into(),
            elements: vec![element(
                "HotSpot",
                &[("label", Value::Text("dot".into()))],
                &[Vec2::new(1.0, 1.0)],
            )],
        };
        assert!(to_room(&scene).hotspots.is_empty());
    }
}
