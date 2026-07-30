//! A3.5: the Kyrandia import backend.
//!
//! Walks a retail `DAT` directory, opens every `.PAK`, decodes the `.CPS`
//! backgrounds inside, and writes them out as PNGs paired with a stub
//! room file. The stub carries the background layer and the room size;
//! the floor, hotspots and exits are drawn afterwards in the editor,
//! because Kyrandia keeps those in script bytecode rather than in
//! anything worth reverse-engineering for a first pass.
//!
//! Nothing here is a runtime dependency: this crate converts foreign
//! data into our own format once, offline.

pub mod cps;
pub mod format80;
pub mod pak;
pub mod palette;

use std::fs;
use std::path::{Path, PathBuf};

use glam::Vec2;
use infengine_adventure::{Layer, Room};
use tracing::{debug, warn};

use crate::report::{ImportReport, Skipped};
use crate::ImportError;
use palette::Palette;

/// Subdirectory of the output that receives decoded images.
const IMAGE_DIR: &str = "images";
/// Subdirectory that receives room stubs.
const ROOM_DIR: &str = "rooms";

/// The game-wide palette. Room backgrounds ship without one of their own
/// and take their colours from here, so without it every screen decodes
/// as a correct but grey image.
const GLOBAL_PALETTE: &str = "PALETTE.COL";

/// Find the game-wide palette by scanning archives for a `.COL` entry.
///
/// Kyrandia keeps several palettes (title screen, Westwood logo, a few
/// cutscene-specific ones); `PALETTE.COL` is the one the rooms use, so
/// it wins, and any other `.COL` is only a fallback for an install laid
/// out differently.
pub fn find_global_palette(source: &Path) -> Option<Palette> {
    let mut fallback = None;
    let mut archives: Vec<PathBuf> = fs::read_dir(source)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pak")))
        .collect();
    archives.sort();

    for path in archives {
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(pak) = pak::Pak::parse(&bytes) else {
            continue;
        };
        for (index, entry) in pak.with_extension("COL") {
            let Some(data) = pak.data_at(index) else {
                continue;
            };
            let Some(palette) = Palette::from_vga_6bit(data) else {
                continue;
            };
            if entry.name.eq_ignore_ascii_case(GLOBAL_PALETTE) {
                debug!(archive = %path.display(), "found the game palette");
                return Some(palette);
            }
            fallback.get_or_insert(palette);
        }
    }
    if fallback.is_some() {
        warn!("{GLOBAL_PALETTE} not found; falling back to another .COL");
    }
    fallback
}

/// Import every `.PAK` found under `source` into `out_dir`.
///
/// `source` is the directory holding the retail data (a GOG install puts
/// it in `DAT`). Missing or unreadable individual archives are recorded
/// in the report rather than aborting the run: a partial import is more
/// useful than none.
pub fn import_dir(source: &Path, out_dir: &Path) -> Result<ImportReport, ImportError> {
    let mut report = ImportReport::default();

    let mut archives: Vec<PathBuf> = fs::read_dir(source)
        .map_err(|source_err| ImportError::ReadDir {
            path: source.display().to_string(),
            source: source_err,
        })?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pak"))
        })
        .collect();
    archives.sort();

    if archives.is_empty() {
        return Err(ImportError::NothingToImport {
            path: source.display().to_string(),
            looking_for: "*.PAK".into(),
        });
    }

    // One scan for the game palette before anything decodes, so every
    // room can borrow it.
    let global = find_global_palette(source);
    if global.is_none() {
        warn!("no palette found; images will be written as greyscale");
    }

    for archive in &archives {
        match import_pak(archive, out_dir, global.as_ref(), &mut report) {
            Ok(()) => report.archives += 1,
            Err(err) => {
                warn!(archive = %archive.display(), %err, "skipping unreadable archive");
                report.skipped.push(Skipped {
                    name: archive.display().to_string(),
                    reason: err.to_string(),
                });
            }
        }
    }

    Ok(report)
}

/// Import one archive, appending to `report`.
pub fn import_pak(
    archive: &Path,
    out_dir: &Path,
    global_palette: Option<&Palette>,
    report: &mut ImportReport,
) -> Result<(), ImportError> {
    let bytes = fs::read(archive).map_err(|source| ImportError::ReadFile {
        path: archive.display().to_string(),
        source,
    })?;
    let pak = pak::Pak::parse(&bytes).map_err(|source| ImportError::Format {
        path: archive.display().to_string(),
        detail: source.to_string(),
    })?;

    // Colour priority: whatever the image embedded, else a palette
    // shipped alongside it in the same archive, else the game palette.
    let shared = archive_palette(&pak);
    let fallback = shared.as_ref().or(global_palette);

    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "unnamed".into());

    for (index, entry) in pak.with_extension("CPS") {
        let Some(data) = pak.data_at(index) else {
            continue;
        };
        let image = match cps::decode(data) {
            Ok(image) => image,
            Err(err) => {
                debug!(entry = %entry.name, %err, "CPS did not decode");
                report.skipped.push(Skipped {
                    name: format!("{}:{}", archive.display(), entry.name),
                    reason: err.to_string(),
                });
                continue;
            }
        };

        // Only full-screen images are backgrounds; the odd-sized ones
        // are palettes, fonts, or UI strips that need their own pass.
        if image.height <= 1 {
            report.skipped.push(Skipped {
                name: entry.name.clone(),
                reason: format!("not a full screen ({} pixels)", image.indices.len()),
            });
            continue;
        }

        let rgba = image.to_rgba(fallback);
        let name = entry.name.to_ascii_lowercase().replace(".cps", "");
        let image_rel = format!("{IMAGE_DIR}/{name}.png");
        let image_path = out_dir.join(&image_rel);
        write_png(&image_path, image.width, image.height, &rgba)?;

        let room = background_room(&name, image.width, image.height, &image_rel);
        let room_path = out_dir.join(ROOM_DIR).join(format!("{name}.room.ron"));
        infengine_adventure::save(&room, &room_path).map_err(|source| ImportError::WriteRoom {
            path: room_path.display().to_string(),
            detail: source.to_string(),
        })?;

        report.images += 1;
        report.rooms += 1;
        debug!(archive = %stem, entry = %entry.name, "imported background");
    }

    Ok(())
}

/// Palette shipped inside this archive, if any: an explicit `.COL`
/// entry first, otherwise one embedded in a sibling image.
fn archive_palette(pak: &pak::Pak<'_>) -> Option<Palette> {
    for (index, _) in pak.with_extension("COL") {
        if let Some(data) = pak.data_at(index)
            && let Some(p) = Palette::from_vga_6bit(data)
        {
            return Some(p);
        }
    }
    for (index, _) in pak.with_extension("CPS") {
        if let Some(data) = pak.data_at(index)
            && let Ok(image) = cps::decode(data)
            && let Some(p) = image.palette
        {
            return Some(p);
        }
    }
    None
}

/// A room stub: correct size, background wired up, everything else left
/// for the editor.
fn background_room(id: &str, width: u32, height: u32, image_rel: &str) -> Room {
    let mut room = Room::new(id, Vec2::new(width as f32, height as f32));
    room.name = id.to_string();
    room.layers.push(Layer {
        name: "background".into(),
        image: image_rel.to_string(),
        ..Default::default()
    });
    room
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), ImportError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ImportError::WriteFile {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let buffer = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        ImportError::Format {
            path: path.display().to_string(),
            detail: format!("{width}x{height} does not match {} bytes", rgba.len()),
        }
    })?;
    buffer
        .save(path)
        .map_err(|source| ImportError::EncodeImage {
            path: path.display().to_string(),
            detail: source.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_room_stub_is_valid_apart_from_authoring() {
        let room = background_room("alchemy", 320, 200, "images/alchemy.png");
        assert_eq!(room.id, "alchemy");
        assert_eq!(room.size, Vec2::new(320.0, 200.0));
        assert_eq!(room.layers.len(), 1);
        assert_eq!(room.layers[0].image, "images/alchemy.png");

        // The stub deliberately has no floor or entry point yet, and
        // validate() should say exactly that rather than pass silently.
        let issues = room.validate();
        assert!(issues.iter().any(|i| i.contains("no walkable floor")));
        assert!(issues.iter().any(|i| i.contains("no entry point")));
        assert!(
            !issues.iter().any(|i| i.contains("no id")),
            "the id and size are filled in: {issues:?}"
        );
    }
}
