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
pub mod decompile;
pub mod emc;
pub mod format40;
pub mod format80;
pub mod frames;
pub mod msc;
pub mod pak;
pub mod palette;
pub mod wsa;
pub mod roomdat;
pub mod vm;
pub mod voc;
pub mod xmi;

use std::fs;
use std::path::{Path, PathBuf};

use glam::Vec2;
use infengine_adventure::loca::{self, Line, Locale};
use infengine_adventure::{EntryPoint, Layer, Room};
use tracing::{debug, warn};

use crate::report::{ImportReport, Skipped};
use crate::encoding::Encoding;
use crate::ImportError;
use palette::Palette;

/// Subdirectory of the output that receives decoded images.
const IMAGE_DIR: &str = "images";
/// Where images that are not room backgrounds go: title cards, cutscene
/// stills, item close-ups.
const SPRITE_DIR: &str = "sprites";
/// Where animation sheets go.
const ANIM_DIR: &str = "animations";
/// Where recorded dialogue goes.
const VOICE_DIR: &str = "voice";
/// Where music goes.
const MUSIC_DIR: &str = "music";
/// Where decompiled room logic goes.
const SCRIPT_DIR: &str = "scripts";

/// Height of the playable area of a screen.
///
/// A background decodes to 320x200, but only the top 144 rows are the
/// room - the same height as its walk mask. Kyrandia parks sprite frames
/// in the rows below, so drawing the whole image puts a strip of
/// animation frames under the scene and makes the room 56 pixels taller
/// than the floor it contains.
const PLAY_HEIGHT: u32 = msc::MASK_HEIGHT as u32;

/// Shortest a room can plausibly be.
///
/// The frame table is read heuristically and a few rooms report a frame
/// near the top of the picture, which would crop the room to nothing.
/// A room that short is the reader being wrong rather than the game
/// storing frames over its own scenery, so the play area's height wins.
const MIN_ROOM_HEIGHT: u32 = 96;
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
/// Text encoding to assume for a language whose files are not UTF-8.
///
/// The Czech release writes its dialogue in CP1250; the original is
/// ASCII. Guessing per byte would be unreliable on short lines, so the
/// language decides.
pub fn encoding_for(language: &str) -> Encoding {
    match language {
        "cs" | "sk" | "pl" | "hu" | "sl" | "hr" | "ro" => Encoding::Cp1250,
        _ => Encoding::Latin1,
    }
}

/// Import a retail directory as one language.
///
/// A translated release ships the same archives with the text rewritten,
/// so importing a second language means pointing this at that copy with
/// a different tag - the rooms and art come out identical and the lines
/// land under the same ids.
pub fn import_dir(
    source: &Path,
    out_dir: &Path,
    language: &str,
) -> Result<ImportReport, ImportError> {
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

    export_music(source, out_dir, &mut report);

    // One scan for the game palette before anything decodes, so every
    // room can borrow it.
    let global = find_global_palette(source);
    if global.is_none() {
        warn!("no palette found; images will be written as greyscale");
    }

    let mut locale = Locale::new(language, language);
    // The CD release keeps extra animations in .APK and its spoken
    // dialogue in .VRM, both of them ordinary PAK archives beside the
    // rooms. Reading them costs nothing beyond opening the files.
    for archive in &archives {
        let stem = archive
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        import_extra_animations(&archive.with_extension("APK"), &stem, global.as_ref(), out_dir, &mut report);

        let outcome = import_pak(archive, out_dir, global.as_ref(), &mut locale, language, &mut report);

        // Voices are linked by line index, so the room's lines have to
        // be in the locale before the recordings are matched against
        // them - otherwise every recording lands unattached.
        import_voices(&archive.with_extension("VRM"), &stem, out_dir, &mut locale, &mut report);

        match outcome {
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

    if !locale.is_empty() {
        report.languages = 1;
        report.lines = locale.len();
        loca::save_locale(&locale, &out_dir.join(loca::LOCALE_DIR)).map_err(|err| {
            ImportError::WriteRoom {
                path: format!("{}/{}", loca::LOCALE_DIR, language),
                detail: err.to_string(),
            }
        })?;
    }

    Ok(report)
}

/// Import one archive, appending to `report`.
pub fn import_pak(
    archive: &Path,
    out_dir: &Path,
    global_palette: Option<&Palette>,
    locale: &mut Locale,
    language: &str,
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

    let names: Vec<String> = pak
        .entries()
        .iter()
        .map(|e| e.name.to_ascii_uppercase())
        .collect();

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

        // A3.9: a room recolours a 28-entry window of the game palette
        // from its own .DAT. Without it the image decodes perfectly but
        // wears the previous room's colours.
        let stem_of_entry = entry.name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&entry.name);
        let room_palette = room_override(&pak, stem_of_entry, fallback);
        let rgba = image.to_rgba(room_palette.as_ref().or(fallback));
        let name = entry.name.to_ascii_lowercase().replace(".cps", "");

        // A9.3: only a real room gets a room file. The game marks one by
        // shipping the whole set beside the picture - the .DAT with its
        // palette and mask name, the .MSC walk mask, the .EMC script.
        // Title cards, cutscene stills and item close-ups are pictures
        // with none of those, and turning them into rooms filled the
        // editor's list with things nobody can walk around in.
        let is_room = [".DAT", ".MSC", ".EMC"]
            .iter()
            .all(|ext| names.iter().any(|n| *n == format!("{stem_of_entry}{ext}")));

        if !is_room {
            let image_rel = format!("{SPRITE_DIR}/{name}.png");
            write_png(&out_dir.join(&image_rel), image.width, image.height, &rgba)?;
            report.images += 1;
            report.sprites += 1;
            debug!(archive = %stem, entry = %entry.name, "imported picture, not a room");
            continue;
        }

        // A13.2: the room ends where its frame storage begins. The
        // room's own table says where that is, which is better than a
        // fixed line - the bridge stores its river frames from row 136,
        // eight rows above the play area's nominal bottom.
        let stored = room_frames(&pak, stem_of_entry, image.width, image.height);
        let tallest = image.height.clamp(1, PLAY_HEIGHT);
        let room_height = frames::storage_top(&stored)
            .map(u32::from)
            .filter(|top| *top >= MIN_ROOM_HEIGHT)
            .unwrap_or(PLAY_HEIGHT)
            .clamp(1, tallest);
        let image_rel = format!("{IMAGE_DIR}/{name}.png");
        write_png(
            &out_dir.join(&image_rel),
            image.width,
            room_height,
            &crop_rows(&rgba, image.width, room_height),
        )?;
        report.images += 1;

        // Each animation the room stores comes out as its own sheet,
        // frames left to right, rather than as one undifferentiated
        // strip: a room commonly holds two or three separate loops.
        for (index, group) in frames::group_by_size(&stored).iter().enumerate() {
            let Some(sheet) = cut_sheet(&rgba, image.width, group) else {
                continue;
            };
            let rel = format!("{ANIM_DIR}/{name}_{index}.png");
            write_png(&out_dir.join(&rel), sheet.0, sheet.1, &sheet.2)?;
            report.images += 1;
            report.animations += 1;
            report.frames += group.len();
        }

        let mut room = background_room(&name, image.width, room_height, &image_rel);
        // A5.2: the game's own walkable region, so an imported room
        // arrives with a floor instead of an empty one to trace by hand.
        if let Some(area) = walk_area_for(&pak, stem_of_entry) {
            room.walk = area;
            report.masks += 1;
        }
        add_entry_points(&pak, stem_of_entry, &mut room);
        let room_path = out_dir.join(ROOM_DIR).join(format!("{name}.room.ron"));
        infengine_adventure::save(&room, &room_path).map_err(|source| ImportError::WriteRoom {
            path: room_path.display().to_string(),
            detail: source.to_string(),
        })?;

        report.rooms += 1;
        collect_lines(&pak, stem_of_entry, &name, language, locale);
        export_scripts(&pak, stem_of_entry, &name, language, out_dir, report)?;
        debug!(archive = %stem, entry = %entry.name, "imported background");
    }

    export_animations(&pak, &stem, fallback, out_dir, report)?;

    Ok(())
}

/// Convert the game's music into standard MIDI files.
///
/// The score ships three ways: XMIDI for a wavetable card, and AdLib
/// driver data in `.ADL` and `.SND` for an OPL2 chip. Only the XMIDI is
/// convertible to something standard - the AdLib banks would need a
/// synthesiser emulated to produce any sound at all - so those are
/// reported as untouched rather than quietly ignored.
fn export_music(source: &Path, out_dir: &Path, report: &mut ImportReport) {
    let Ok(bytes) = fs::read(source.join("XMI.PAK")) else {
        return;
    };
    let Ok(pak) = pak::Pak::parse(&bytes) else {
        return;
    };
    for (index, entry) in pak.with_extension("XMI") {
        let Some(data) = pak.data_at(index) else {
            continue;
        };
        let sequences = match xmi::to_midi(data) {
            Ok(sequences) => sequences,
            Err(err) => {
                debug!(entry = %entry.name, %err, "XMI did not convert");
                report.skipped.push(Skipped {
                    name: entry.name.clone(),
                    reason: err.to_string(),
                });
                continue;
            }
        };
        let stem = entry
            .name
            .to_ascii_lowercase()
            .replace(".xmi", "");
        for (n, midi) in sequences.iter().enumerate() {
            let relative = if sequences.len() == 1 {
                format!("{MUSIC_DIR}/{stem}.mid")
            } else {
                format!("{MUSIC_DIR}/{stem}_{n}.mid")
            };
            let path = out_dir.join(&relative);
            if let Some(parent) = path.parent()
                && fs::create_dir_all(parent).is_err()
            {
                continue;
            }
            if fs::write(&path, midi).is_ok() {
                report.music += 1;
            }
        }
    }
}

/// Decode the `.APK` beside a room, which holds animations the `.PAK`
/// does not.
fn import_extra_animations(
    path: &Path,
    stem: &str,
    fallback: Option<&Palette>,
    out_dir: &Path,
    report: &mut ImportReport,
) {
    let Ok(bytes) = fs::read(path) else { return };
    let Ok(pak) = pak::Pak::parse(&bytes) else {
        return;
    };
    if let Err(err) = export_animations(&pak, stem, fallback, out_dir, report) {
        warn!(archive = %path.display(), %err, "extra animations not written");
    }
}

/// Decode the `.VRM` beside a room: the CD release's spoken dialogue.
///
/// Recordings are numbered, and the number is the index of the line
/// they belong to - sparsely, since not every line is voiced. Numbers
/// that fall outside the room's line count are still written out but
/// left unlinked rather than attached to the wrong subtitle.
fn import_voices(
    path: &Path,
    stem: &str,
    out_dir: &Path,
    locale: &mut Locale,
    report: &mut ImportReport,
) {
    let Ok(bytes) = fs::read(path) else { return };
    let Ok(pak) = pak::Pak::parse(&bytes) else {
        return;
    };
    for (index, entry) in pak.with_extension("VOC") {
        let Some(data) = pak.data_at(index) else {
            continue;
        };
        let voice = match voc::decode(data) {
            Ok(voice) => voice,
            Err(err) => {
                debug!(entry = %entry.name, %err, "VOC did not decode");
                report.skipped.push(Skipped {
                    name: format!("{stem}:{}", entry.name),
                    reason: err.to_string(),
                });
                continue;
            }
        };
        let number = entry
            .name
            .split('.')
            .next()
            .and_then(|s| s.parse::<usize>().ok());
        let relative = match number {
            Some(n) => format!("{VOICE_DIR}/{stem}/{n:02}.wav"),
            None => format!("{VOICE_DIR}/{stem}/{}.wav", entry.name.to_ascii_lowercase()),
        };
        let path = out_dir.join(&relative);
        if let Some(parent) = path.parent()
            && fs::create_dir_all(parent).is_err()
        {
            continue;
        }
        let wav = voice.to_wav();
        if fs::write(&path, &wav).is_err() {
            continue;
        }
        report.voice_lines += 1;
        report.voice_bytes += wav.len() as u64;

        if let Some(n) = number {
            let id = emc::Script::line_id(stem, n);
            if let Some(line) = locale.lines.get_mut(&id) {
                line.voice = Some(relative);
            } else {
                report.unlinked_voice += 1;
            }
        }
    }
}

/// The frame table a room stores, if it has one.
fn room_frames(
    pak: &pak::Pak<'_>,
    stem: &str,
    width: u32,
    height: u32,
) -> Vec<frames::Frame> {
    let Some(data) = pak.get(&format!("{stem}.DAT")) else {
        return Vec::new();
    };
    let Ok(parsed) = roomdat::parse(data) else {
        return Vec::new();
    };
    let Some(table) = frames::table_bytes(data, parsed.after_palette) else {
        return Vec::new();
    };
    frames::read(table, width as u16, height as u16)
}

/// Cut a group of frames out of the image and lay them out left to
/// right as one sheet.
fn cut_sheet(
    rgba: &[u8],
    image_width: u32,
    group: &[frames::Frame],
) -> Option<(u32, u32, Vec<u8>)> {
    let first = group.first()?;
    let (fw, fh) = (first.width as u32, first.height as u32);
    if fw == 0 || fh == 0 {
        return None;
    }
    let sheet_w = fw * group.len() as u32;
    let mut out = vec![0u8; sheet_w as usize * fh as usize * 4];
    for (index, frame) in group.iter().enumerate() {
        let ox = index as u32 * fw;
        for row in 0..fh {
            let src = ((frame.y as u32 + row) as usize * image_width as usize
                + frame.x as usize)
                * 4;
            let dst = ((row * sheet_w) + ox) as usize * 4;
            let span = fw as usize * 4;
            if src + span > rgba.len() {
                continue;
            }
            out[dst..dst + span].copy_from_slice(&rgba[src..src + span]);
        }
    }
    Some((sheet_w, fh, out))
}

/// Keep the first `rows` rows of an RGBA image.
fn crop_rows(rgba: &[u8], width: u32, rows: u32) -> Vec<u8> {
    let span = width as usize * 4;
    rgba[..(rows as usize * span).min(rgba.len())].to_vec()
}

/// Give the room the positions the game starts the actor at.
///
/// The four stored points are often the same position repeated, so
/// duplicates collapse into one entry. Each is snapped onto the floor:
/// a handful sit a pixel outside the traced outline, and an entry point
/// off the floor would strand the actor with nowhere to walk.
fn add_entry_points(pak: &pak::Pak<'_>, stem: &str, room: &mut Room) {
    let Some(data) = pak.get(&format!("{stem}.DAT")) else {
        return;
    };
    let Ok(parsed) = roomdat::parse(data) else {
        return;
    };
    for (index, point) in parsed.entry_points.iter().enumerate() {
        let placed = room.walk.clamp(*point).unwrap_or(*point);
        if room
            .entries
            .iter()
            .any(|e| e.position.distance(placed) < 1.0)
        {
            continue;
        }
        room.entries.push(EntryPoint {
            id: format!("entry{index}"),
            position: placed,
            ..Default::default()
        });
    }
}

/// Decompile a room's bytecode into our own actions and write it out.
///
/// The result is deliberately not a runnable copy of Kyrandia's logic:
/// most calls come through as `Unknown` because the engine's function
/// table is largely unidentified. Counting them is the point - the
/// editor can show how much of a room is still to be worked out instead
/// of presenting a half-translated script as finished.
fn export_scripts(
    pak: &pak::Pak<'_>,
    stem: &str,
    room_id: &str,
    language: &str,
    out_dir: &Path,
    report: &mut ImportReport,
) -> Result<(), ImportError> {
    let Some(data) = pak.get(&format!("{stem}.EMC")) else {
        return Ok(());
    };
    let Ok(parsed) = emc::decode(data, encoding_for(language)) else {
        return Ok(());
    };
    let decompiled = decompile::decompile(room_id, &parsed);
    if decompiled.is_empty() {
        return Ok(());
    }

    let text = ron::ser::to_string_pretty(
        &decompiled.scripts,
        ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
    )
    .map_err(|err| ImportError::WriteRoom {
        path: format!("{SCRIPT_DIR}/{room_id}"),
        detail: err.to_string(),
    })?;
    let path = out_dir.join(SCRIPT_DIR).join(format!("{room_id}.script.ron"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ImportError::WriteFile {
            path: parent.display().to_string(),
            source,
        })?;
    }
    fs::write(&path, text).map_err(|source| ImportError::WriteFile {
        path: path.display().to_string(),
        source,
    })?;

    report.scripts += decompiled.scripts.len();
    report.unknown_actions += decompiled.unknown_count();
    report.scene_links += decompiled.scene_targets.len();
    report.plumbing_dropped += decompiled.plumbing_dropped;
    Ok(())
}

/// Pull a room's dialogue out of its script into the locale.
///
/// Lines are keyed by room and position, which the translated releases
/// preserve - that is what makes the Czech text land on the same ids as
/// the English.
fn collect_lines(
    pak: &pak::Pak<'_>,
    stem: &str,
    room_id: &str,
    language: &str,
    locale: &mut Locale,
) {
    let Some(data) = pak.get(&format!("{stem}.EMC")) else {
        return;
    };
    let script = match emc::decode(data, encoding_for(language)) {
        Ok(script) => script,
        Err(err) => {
            debug!(stem, %err, "room script did not decode");
            return;
        }
    };
    for (index, text) in script.texts.iter().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        locale.insert(emc::Script::line_id(room_id, index), Line::text(text));
    }
}

/// Decode every `.WSA` in the archive and write it as a sprite sheet.
///
/// Frames are laid out left to right in one PNG, which is what both an
/// editor and a sprite renderer want; palette index 0 becomes
/// transparent, since that is what the animations key their background
/// on.
fn export_animations(
    pak: &pak::Pak<'_>,
    archive: &str,
    fallback: Option<&Palette>,
    out_dir: &Path,
    report: &mut ImportReport,
) -> Result<(), ImportError> {
    for (index, entry) in pak.with_extension("WSA") {
        let Some(data) = pak.data_at(index) else {
            continue;
        };
        let animation = match wsa::decode(data) {
            Ok(animation) => animation,
            Err(err) => {
                debug!(entry = %entry.name, %err, "WSA did not decode");
                report.skipped.push(Skipped {
                    name: entry.name.clone(),
                    reason: err.to_string(),
                });
                continue;
            }
        };
        let (width, height, rgba) = animation.to_sheet_rgba(fallback);
        let name = entry.name.to_ascii_lowercase().replace(".wsa", "");
        let path = out_dir.join(format!("{ANIM_DIR}/{archive}_{name}.png"));
        write_png(&path, width, height, &rgba)?;
        report.images += 1;
        report.animations += 1;
        report.frames += animation.frame_count();
    }
    Ok(())
}

/// Walkable region for `stem`, traced from its `.MSC` mask.
///
/// Returns `None` when the archive has no mask or it does not decode -
/// the room still imports, just without a floor.
fn walk_area_for(pak: &pak::Pak<'_>, stem: &str) -> Option<infengine_adventure::WalkArea> {
    let data = pak.get(&format!("{stem}.MSC"))?;
    match msc::decode(data) {
        Ok(mask) => {
            let area = mask.to_walk_area();
            if area.is_empty() { None } else { Some(area) }
        }
        Err(err) => {
            debug!(stem, %err, "room mask did not decode");
            None
        }
    }
}

/// The room-specific palette for `stem`, built by applying the override
/// in `<stem>.DAT` on top of `base`.
///
/// Returns `None` when the archive has no matching `.DAT`, or when that
/// file does not parse - a miscoloured room beats no room at all, so the
/// caller falls back to the base palette.
fn room_override(pak: &pak::Pak<'_>, stem: &str, base: Option<&Palette>) -> Option<Palette> {
    let base = base?;
    let data = pak.get(&format!("{stem}.DAT"))?;
    match roomdat::parse(data) {
        Ok(room) => Some(room.apply(base)),
        Err(err) => {
            debug!(stem, %err, "room .DAT has no usable palette override");
            None
        }
    }
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
