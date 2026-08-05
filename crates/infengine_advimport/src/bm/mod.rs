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
//! Art comes out too: the tail of each `.dat` holds the scene's textures
//! as tiled DDS blobs, which [`resource`] stitches back together and this
//! module writes out as PNG.

pub mod dds;
pub mod entries;
pub mod meta;
pub mod resource;
pub mod scene;
pub mod speech;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use glam::Vec2;
use infengine_adventure::loca::{self, Line, Locale};
use infengine_adventure::{Exit, Hotspot, Layer, LayerKind, Polygon, Room, Shape};
use tracing::{debug, warn};

use crate::encoding;
use crate::ImportError;
use crate::report::{ImportReport, Skipped};

/// Subdirectory of a retail install holding one `.dat` per screen.
const SCENES_DIR: &str = "Scenes";
/// Where room files are written.
const ROOM_DIR: &str = "rooms";

/// A layer with `scroll` at this value moves with the camera one for
/// one, which is not parallax at all.
const NO_PARALLAX: f32 = 1.0;

/// The hotspot table and translations that describe a whole game.
///
/// Both are optional: without them rooms still import, just with raw
/// labels for names and every clickable region filed as a hotspot.
#[derive(Debug, Default)]
pub struct GameMeta {
    pub hotspots: meta::HotSpotTable,
    pub loca: Option<meta::Localisation>,
    /// Room ids that exist, used to resolve exit targets.
    pub rooms: Vec<String>,
}

/// Load `Scripts/HotSpots.xml` and a translation file if either is
/// present. Czech is preferred, then English, then whatever is there.
fn load_meta(source: &Path, rooms: Vec<String>) -> GameMeta {
    let hotspots = read_text(&source.join("Scripts").join("HotSpots.xml"))
        .map(|xml| meta::parse_hotspots(&xml))
        .unwrap_or_default();

    let loca_dir = source.join("loca");
    let loca = ["czech", "english"]
        .iter()
        .find_map(|lang| read_text(&loca_dir.join(format!("loca_{lang}.xml"))))
        .map(|xml| meta::parse_localisation(&xml));

    if hotspots.is_empty() {
        warn!("no Scripts/HotSpots.xml; hotspots will keep their raw labels");
    }
    GameMeta {
        hotspots,
        loca,
        rooms,
    }
}

/// Convert every language the install ships into our own locale files,
/// extracting the voice archive that goes with each.
///
/// Text and audio already share one key in the source data, so the
/// conversion only has to preserve that: each line keeps its speaker,
/// and gains the paths to the recording and lip sync written beside it.
fn export_languages(source: &Path, out_dir: &Path, report: &mut ImportReport) {
    let loca_dir = source.join("loca");
    let Ok(entries) = fs::read_dir(&loca_dir) else {
        warn!("no loca directory; the import will have no text");
        return;
    };

    let mut languages: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_prefix("loca_")
                .and_then(|rest| rest.strip_suffix(".xml"))
                .map(str::to_string)
        })
        .collect();
    languages.sort();

    for language in languages {
        match export_language(source, out_dir, &language, report) {
            Ok(count) => {
                report.languages += 1;
                report.lines += count;
            }
            Err(err) => {
                warn!(%language, %err, "language not imported");
                report.skipped.push(Skipped {
                    name: format!("loca_{language}.xml"),
                    reason: err.to_string(),
                });
            }
        }
    }
}

/// Language tag and the name to show in a language menu.
///
/// The menu name is written in the language itself, which is what a
/// player scanning the list actually recognises. An unrecognised
/// language keeps its directory name rather than being dropped.
fn language_info(name: &str) -> (String, String) {
    const KNOWN: &[(&str, &str, &str)] = &[
        ("czech", "cs", "Čeština"),
        ("english", "en", "English"),
        ("german", "de", "Deutsch"),
        ("french", "fr", "Français"),
        ("spanish", "es", "Español"),
        ("italian", "it", "Italiano"),
        ("polish", "pl", "Polski"),
        ("russian", "ru", "Русский"),
    ];
    KNOWN
        .iter()
        .find(|(dir, _, _)| *dir == name)
        .map(|(_, tag, display)| ((*tag).to_string(), (*display).to_string()))
        .unwrap_or_else(|| (name.to_ascii_lowercase(), name.to_string()))
}

fn export_language(
    source: &Path,
    out_dir: &Path,
    language: &str,
    report: &mut ImportReport,
) -> Result<usize, ImportError> {
    let xml = read_text(&source.join("loca").join(format!("loca_{language}.xml"))).ok_or(
        ImportError::NothingToImport {
            path: source.display().to_string(),
            looking_for: format!("loca_{language}.xml"),
        },
    )?;
    let parsed = meta::parse_localisation(&xml);
    let (tag, display_name) = language_info(language);

    // Pull the recordings out first so each line can point at its file.
    let archive = source.join("loca").join(language).join("speech.dat");
    let prefix = format!("voice/{tag}");
    let mut voice: HashMap<String, (String, Option<String>)> = HashMap::new();
    if archive.is_file() {
        match speech::open_index(&archive) {
            Ok(entries) => {
                match speech::extract(&archive, &entries, out_dir, &prefix) {
                    Ok(stats) => {
                        report.voice_lines += stats.audio;
                        report.voice_bytes += stats.bytes;
                        for entry in &entries {
                            let Some(speaker) = entry.speaker() else {
                                continue;
                            };
                            let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
                            let rel = format!("{prefix}/{speaker}/{name}");
                            let slot = voice
                                .entry(entry.label().to_string())
                                .or_insert_with(|| (String::new(), None));
                            if entry.is_audio() {
                                slot.0 = rel;
                            } else if entry.is_lipsync() {
                                slot.1 = Some(rel);
                            }
                        }
                    }
                    Err(err) => warn!(%language, %err, "voice archive not extracted"),
                }
            }
            Err(err) => warn!(%language, %err, "voice index not read"),
        }
    }

    let mut locale = Locale::new(&tag, &display_name);
    for (key, line) in parsed.iter() {
        let mut out = Line::text(&line.text);
        out.speaker = line.speaker.clone();
        if let Some((audio, lipsync)) = voice.get(key) {
            if !audio.is_empty() {
                out.voice = Some(audio.clone());
            }
            out.lipsync = lipsync.clone();
        }
        locale.insert(key, out);
    }

    let count = locale.len();
    loca::save_locale(&locale, &out_dir.join(loca::LOCALE_DIR)).map_err(|err| {
        ImportError::WriteRoom {
            path: format!("{}/{}", loca::LOCALE_DIR, tag),
            detail: err.to_string(),
        }
    })?;
    Ok(count)
}

/// Read a text file into UTF-8, whatever it was written in.
///
/// These files declare `encoding="utf-8"` and then carry the odd
/// Latin-1 byte in a German label. A strict read rejects the whole file
/// over one of them, which silently costs every hotspot name and every
/// exit in the game. Decoding lossily would keep the file but replace
/// those bytes with `?`, so the fallback is a real code page instead -
/// it maps every byte and cannot lose a letter.
fn read_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let (text, found) = encoding::decode(&bytes, encoding::Encoding::Latin1);
    if found != encoding::Encoding::Utf8 {
        debug!(path = %path.display(), ?found, "file was not UTF-8; converted");
    }
    Some(encoding::strip_bom(&text).to_string())
}

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

    // Room ids are the scene file names, so the whole set is known
    // before anything is parsed - which is what lets an exit resolve to
    // a room that has not been imported yet.
    let room_ids: Vec<String> = scenes
        .iter()
        .filter_map(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .collect();
    let game = load_meta(source, room_ids);

    let mut report = ImportReport::default();
    export_languages(source, out_dir, &mut report);
    for path in &scenes {
        match import_scene(path, out_dir, &game, &mut report) {
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

    report.entry_points = derive_entry_points(out_dir);
    Ok(report)
}

/// Subdirectory that receives decoded images.
const IMAGE_DIR: &str = "images";

/// Extract every texture a scene holds and write it as a PNG.
///
/// Returns a map from the lowercased file name a `Layer` refers to
/// (`s097_lighthouseday.tga`) to the project-relative PNG path we wrote.
/// A texture that does not decode is recorded and skipped: one broken
/// resource must not cost the whole scene.
fn export_textures(
    data: &[u8],
    parsed: &scene::Scene,
    scene_name: &str,
    out_dir: &Path,
    report: &mut ImportReport,
) -> HashMap<String, String> {
    let declared = declared_sizes(parsed);
    let mut written = HashMap::new();
    for found in resource::find_resources(data, parsed.graph_end) {
        let key = found.file_name();
        if written.contains_key(&key) {
            continue;
        }
        let texture = match resource::read_texture(data, found.after_name) {
            Ok(texture) => texture,
            Err(err) => {
                debug!(resource = %found.path, %err, "texture skipped");
                report.skipped.push(Skipped {
                    name: found.path.clone(),
                    reason: err.to_string(),
                });
                continue;
            }
        };
        // Tiles pad the texture out to a sum of powers of two, so the
        // stored image is wider and taller than the art. Crop to what the
        // layer declares; without this every background carries a strip
        // of undefined padding along its right and bottom edges.
        let texture = match declared.get(&key) {
            Some(&(w, h)) => texture.cropped(w, h),
            None => texture,
        };
        let stem = key.rsplit_once('.').map(|(s, _)| s).unwrap_or(&key);
        let relative = format!("{IMAGE_DIR}/{scene_name}/{stem}.png");
        let path = out_dir.join(&relative);
        if let Err(err) = write_png(&path, &texture) {
            warn!(%err, "could not write texture");
            report.skipped.push(Skipped {
                name: relative.clone(),
                reason: err.to_string(),
            });
            continue;
        }
        report.images += 1;
        written.insert(key, relative);
    }
    written
}

/// Size each layer declares for its image, keyed by lowercased file
/// name. A sheet shared by several layers keeps the largest, so nothing
/// is cropped away from a neighbour.
fn declared_sizes(parsed: &scene::Scene) -> HashMap<String, (u32, u32)> {
    let mut out: HashMap<String, (u32, u32)> = HashMap::new();
    for element in parsed.of_kind("Layer") {
        let Some(file) = element.text("file") else {
            continue;
        };
        let (Some(w), Some(h)) = (element.number("w"), element.number("h")) else {
            continue;
        };
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let key = file
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(file)
            .to_ascii_lowercase();
        let size = (w as u32, h as u32);
        out.entry(key)
            .and_modify(|slot| *slot = (slot.0.max(size.0), slot.1.max(size.1)))
            .or_insert(size);
    }
    out
}

fn write_png(path: &Path, texture: &resource::Texture) -> Result<(), ImportError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ImportError::WriteFile {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let buffer =
        image::RgbaImage::from_raw(texture.width, texture.height, texture.rgba.clone())
            .ok_or_else(|| ImportError::Format {
                path: path.display().to_string(),
                detail: format!(
                    "{}x{} does not match {} bytes",
                    texture.width,
                    texture.height,
                    texture.rgba.len()
                ),
            })?;
    buffer.save(path).map_err(|source| ImportError::EncodeImage {
        path: path.display().to_string(),
        detail: source.to_string(),
    })
}

/// Point each layer at the PNG we wrote for it, cropping the padded
/// texture down to the size the layer declares.
fn relink_layers(room: &mut Room, written: &HashMap<String, String>) {
    for layer in &mut room.layers {
        let key = layer
            .image
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&layer.image)
            .to_ascii_lowercase();
        if let Some(relative) = written.get(&key) {
            layer.image = relative.clone();
        }
    }
}

/// Convert one scene file into a room.
/// Second pass: give every room the spots you arrive at.
///
/// Reads the rooms back rather than keeping them all in memory during
/// the first pass. An entry point comes from *another* room's exit, so
/// it cannot be worked out until every room exists - and re-reading a
/// few hundred small files costs less than the restructuring would.
fn derive_entry_points(out_dir: &Path) -> usize {
    use std::collections::BTreeMap;

    let dir = out_dir.join(ROOM_DIR);
    let Ok(listing) = fs::read_dir(&dir) else {
        return 0;
    };
    let mut paths: Vec<PathBuf> = listing
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".room.ron"))
        })
        .collect();
    paths.sort();

    let mut rooms: BTreeMap<String, Room> = BTreeMap::new();
    for path in &paths {
        match infengine_adventure::load(path) {
            Ok(room) => {
                rooms.insert(room.id.clone(), room);
            }
            Err(err) => debug!(path = %path.display(), %err, "room not re-read"),
        }
    }

    let added = entries::derive(&mut rooms);
    for room in rooms.values() {
        let path = dir.join(format!("{}.room.ron", room.id));
        if let Err(err) = infengine_adventure::save(room, &path) {
            warn!(path = %path.display(), %err, "could not write the entry points back");
        }
    }
    added
}

pub fn import_scene(
    path: &Path,
    out_dir: &Path,
    game: &GameMeta,
    report: &mut ImportReport,
) -> Result<(), ImportError> {
    let bytes = fs::read(path).map_err(|err| ImportError::ReadFile {
        path: path.display().to_string(),
        source: err,
    })?;
    let parsed = scene::parse(&bytes).map_err(|err| ImportError::Format {
        path: path.display().to_string(),
        detail: err.to_string(),
    })?;

    let mut room = to_room(&parsed, game);
    let written = export_textures(&bytes, &parsed, &room.id, out_dir, report);
    relink_layers(&mut room, &written);
    let file_name = format!("{}.room.ron", room.id);
    let room_path = out_dir.join(ROOM_DIR).join(file_name);
    infengine_adventure::save(&room, &room_path).map_err(|err| ImportError::WriteRoom {
        path: room_path.display().to_string(),
        detail: err.to_string(),
    })
}

/// Map a parsed scene onto our room model.
pub fn to_room(parsed: &scene::Scene, game: &GameMeta) -> Room {
    let id = parsed.name.to_ascii_lowercase();
    let project = parsed.first_of_kind("Project");
    let size = Vec2::new(
        project.and_then(|p| p.number("w")).unwrap_or(0.0),
        project.and_then(|p| p.number("h")).unwrap_or(0.0),
    );

    let mut room = Room::new(id.clone(), size);
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
            shown_if: None,
        });
    }

    for element in parsed.of_kind("HotSpot") {
        let Some(shape) = hotspot_shape(&element.points) else {
            continue;
        };
        let label = element.label();
        let found = game.hotspots.get(label);
        let name = found.map_or_else(
            || label.to_string(),
            |m| m.display_name(game.loca.as_ref()),
        );

        // The scene graph files doorways and scenery under the same node
        // type; only the hotspot table tells them apart.
        let is_exit = found.is_some_and(|m| m.kind == meta::HotSpotKind::Exit);
        if is_exit {
            let target = meta::exit_target(label)
                .and_then(|t| meta::resolve_room(&t, &game.rooms, &id))
                .unwrap_or_default();
            room.exits.push(Exit {
                id: label.to_string(),
                name,
                shape,
                target_room: target,
                enabled: true,
                ..Default::default()
            });
            continue;
        }

        room.hotspots.push(Hotspot {
            id: label.to_string(),
            name,
            shape,
            default_verb: found.map(|m| m.verb).unwrap_or_default(),
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
    use infengine_adventure::Verb;
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
            graph_end: 0,
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
        let room = to_room(&sample_scene(), &GameMeta::default());
        assert_eq!(room.size, Vec2::new(1280.0, 800.0));
        assert_eq!(room.id, "lighthouseday");
        assert_eq!(room.name, "LighthouseDay");
    }

    #[test]
    fn a_scene_without_a_project_node_still_imports() {
        let scene = Scene {
            graph_end: 0,
            name: "Bare".into(),
            elements: Vec::new(),
        };
        let room = to_room(&scene, &GameMeta::default());
        assert_eq!(room.size, Vec2::ZERO);
        // validate() should flag the missing size rather than the
        // importer inventing one.
        assert!(room.validate().iter().any(|i| i.contains("not positive")));
    }

    #[test]
    fn layers_keep_their_order_and_images() {
        let room = to_room(&sample_scene(), &GameMeta::default());
        assert_eq!(room.layers.len(), 3);
        assert_eq!(room.layers[0].name, "bg");
        assert_eq!(room.layers[0].image, "s097.tga");
    }

    #[test]
    fn ysort_becomes_a_walk_behind_overlay() {
        let room = to_room(&sample_scene(), &GameMeta::default());
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
        let room = to_room(&sample_scene(), &GameMeta::default());
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
            graph_end: 0,
            name: "S".into(),
            elements: vec![element(
                "Layer",
                &[("label", Value::Text("marker".into()))],
                &[],
            )],
        };
        assert!(to_room(&scene, &GameMeta::default()).layers.is_empty());
    }

    #[test]
    fn a_hotspot_outline_becomes_a_polygon() {
        let room = to_room(&sample_scene(), &GameMeta::default());
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

    /// A scene with one doorway and one object, both filed as HotSpot
    /// nodes exactly as the scene graph stores them.
    fn scene_with_a_doorway() -> Scene {
        Scene {
            graph_end: 0,
            name: "Beach".into(),
            elements: vec![
                element(
                    "HotSpot",
                    &[("label", Value::Text("FromBeachToPromenade".into()))],
                    &[
                        Vec2::new(0.0, 0.0),
                        Vec2::new(10.0, 0.0),
                        Vec2::new(10.0, 10.0),
                    ],
                ),
                element(
                    "HotSpot",
                    &[("label", Value::Text("Note".into()))],
                    &[
                        Vec2::new(20.0, 20.0),
                        Vec2::new(30.0, 20.0),
                        Vec2::new(30.0, 30.0),
                    ],
                ),
            ],
        }
    }

    fn game_meta() -> GameMeta {
        let xml = concat!(
            "<HotSpots>",
            "<HotSpot label=\"FromBeachToPromenade\" display_name=\"#k1 Weg\" cursor=\"exit\" type=\"exit\"/>",
            "<HotSpot label=\"Note\" display_name=\"#k2 Zettel\" cursor=\"pick_up\" type=\"game\"/>",
            "</HotSpots>"
        );
        GameMeta {
            hotspots: meta::parse_hotspots(xml),
            loca: Some(meta::parse_localisation(
                "<TextRoot><Label value=\"k2\"><Text value=\"listek\"/></Label></TextRoot>",
            )),
            rooms: vec!["promenadeday".into(), "beach".into()],
        }
    }

    #[test]
    fn the_hotspot_table_splits_doorways_out_into_exits() {
        // Without the table both of these are hotspots, because the
        // scene graph files them under the same node type.
        let plain = to_room(&scene_with_a_doorway(), &GameMeta::default());
        assert_eq!(plain.hotspots.len(), 2);
        assert!(plain.exits.is_empty());

        let room = to_room(&scene_with_a_doorway(), &game_meta());
        assert_eq!(room.hotspots.len(), 1);
        assert_eq!(room.exits.len(), 1);
        assert_eq!(room.exits[0].id, "FromBeachToPromenade");
    }

    #[test]
    fn an_exit_target_is_resolved_against_the_rooms_that_exist() {
        let room = to_room(&scene_with_a_doorway(), &game_meta());
        assert_eq!(room.exits[0].target_room, "promenadeday");
        // A resolved target means validate() has nothing to complain
        // about on this exit.
        assert!(!room.validate().iter().any(|i| i.contains("no target room")));
    }

    #[test]
    fn hotspots_take_their_verb_and_translated_name_from_the_table() {
        let room = to_room(&scene_with_a_doorway(), &game_meta());
        let note = &room.hotspots[0];
        assert_eq!(note.id, "Note");
        assert_eq!(note.default_verb, Verb::Take);
        assert_eq!(note.name, "listek", "the translation wins over the label");
    }

    #[test]
    fn an_unresolvable_exit_target_is_left_empty_for_the_editor() {
        let mut game = game_meta();
        game.rooms = vec!["somewhereelse".into()];
        let room = to_room(&scene_with_a_doorway(), &game);
        assert_eq!(room.exits.len(), 1);
        assert!(room.exits[0].target_room.is_empty());
        assert!(room.validate().iter().any(|i| i.contains("no target room")));
    }

    #[test]
    fn a_hotspot_with_too_few_points_is_dropped_not_invented() {
        assert!(hotspot_shape(&[]).is_none());
        assert!(hotspot_shape(&[Vec2::new(1.0, 1.0)]).is_none());
        let scene = Scene {
            graph_end: 0,
            name: "S".into(),
            elements: vec![element(
                "HotSpot",
                &[("label", Value::Text("dot".into()))],
                &[Vec2::new(1.0, 1.0)],
            )],
        };
        assert!(to_room(&scene, &GameMeta::default()).hotspots.is_empty());
    }
}
