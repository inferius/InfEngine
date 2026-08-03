//! A67: pull the rooms' ambient animations out of the `.DAT` scripts.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_scene_anims -- <game dir> <project>
//! ```
//!
//! For every room PAK: decode the background with the room's palette,
//! read the `.DAT` animation section, and cut each animation's frames
//! out of the image into a sheet under `animations/`. The ids are
//! `<room>_<slot>`, which is exactly what the decompiled
//! `sceneAnimOn(slot)` calls already point at - so re-running
//! `kyra_spots` afterwards turns hundreds of untranslated steps into
//! playing animations.

#![allow(clippy::print_stdout)]

use infengine_advimport::kyra::{cps, pak::Pak, palette::Palette, roomdat, scene_anim};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_scene_anims <dir of .PAK files> <project dir>");
        return;
    };
    let game = std::path::PathBuf::from(game);
    let project = std::path::PathBuf::from(project);

    let Some(base) = global_palette(&game) else {
        println!("no PALETTE.COL found under {}", game.display());
        return;
    };

    let anim_dir = project.join(infengine_adventure::anim::ANIMATION_DIR);
    let mut set = infengine_adventure::anim::load_index(&anim_dir).unwrap_or_default();

    let mut paks: Vec<_> = std::fs::read_dir(&game)
        .expect("readable game dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("pak"))
        })
        .collect();
    paks.sort();

    let (mut rooms_with, mut sheets, mut skipped) = (0usize, 0usize, 0usize);
    for path in &paks {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(pak) = Pak::parse(&bytes) else { continue };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        // The room's pieces, all by the archive's own name.
        let want = |ext: &str| {
            pak.entries()
                .iter()
                .position(|e| e.name.eq_ignore_ascii_case(&format!("{stem}.{ext}")))
                .and_then(|index| pak.data_at(index))
        };
        let (Some(dat), Some(cps_data)) = (want("dat"), want("cps")) else {
            continue;
        };
        let Ok(room) = roomdat::parse(dat) else { continue };
        let Ok(image) = cps::decode(cps_data) else { continue };
        let palette = room.apply(&base);

        let anims = scene_anim::parse(dat, room.after_palette);
        let mut any = false;
        for anim in &anims.anims {
            match sheet_of(&project, &stem, anim, &anims, &image, &palette) {
                Some(entry) => {
                    set.insert(entry);
                    sheets += 1;
                    any = true;
                }
                None => skipped += 1,
            }
        }
        if any {
            rooms_with += 1;
        }
    }

    if let Err(err) = infengine_adventure::anim::save_index(&set, &anim_dir) {
        println!("index not written: {err}");
        return;
    }
    println!(
        "{rooms_with} rooms gained ambient animations: {sheets} sheets, {skipped} scripts skipped"
    );
}

/// `PALETTE.COL`, loose or inside any archive.
fn global_palette(game: &std::path::Path) -> Option<Palette> {
    if let Ok(bytes) = std::fs::read(game.join("PALETTE.COL"))
        && let Some(palette) = Palette::from_vga_6bit(&bytes)
    {
        return Some(palette);
    }
    for entry in std::fs::read_dir(game).ok()?.flatten() {
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let Ok(pak) = Pak::parse(&bytes) else { continue };
        if let Some(index) = pak
            .entries()
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case("PALETTE.COL"))
            && let Some(data) = pak.data_at(index)
            && let Some(palette) = Palette::from_vga_6bit(data)
        {
            return Some(palette);
        }
    }
    None
}

/// Cut one animation's frames into a sheet and describe it.
///
/// The cell is the bounding box of every frame's rectangle, so a
/// script that nudges its shape around still fits - the movement is
/// baked into where each frame sits inside the cell.
fn sheet_of(
    project: &std::path::Path,
    room: &str,
    anim: &scene_anim::SceneAnim,
    anims: &scene_anim::SceneAnims,
    image: &cps::CpsImage,
    palette: &Palette,
) -> Option<infengine_adventure::Animation> {
    if anim.frames.is_empty() {
        return None;
    }
    // The box every frame fits in, in room space.
    let (mut min_x, mut min_y) = (i32::MAX, i32::MAX);
    let (mut max_x, mut max_y) = (i32::MIN, i32::MIN);
    for frame in &anim.frames {
        let shape = anims.shape(frame.shape)?;
        min_x = min_x.min(frame.x);
        min_y = min_y.min(frame.y);
        max_x = max_x.max(frame.x + shape.w as i32);
        max_y = max_y.max(frame.y + shape.h as i32);
    }
    let (cell_w, cell_h) = ((max_x - min_x).max(1) as u32, (max_y - min_y).max(1) as u32);
    if cell_w > 320 || cell_h > 200 {
        return None;
    }

    let frames = anim.frames.len() as u32;
    let columns = infengine_adventure::Animation::columns_for(cell_w as f32, frames);
    let rows = frames.div_ceil(columns);
    let mut sheet = image::RgbaImage::new(cell_w * columns, cell_h * rows);

    for (index, frame) in anim.frames.iter().enumerate() {
        let shape = anims.shape(frame.shape)?;
        let cell_x = (index as u32 % columns) * cell_w;
        let cell_y = (index as u32 / columns) * cell_h;
        for dy in 0..shape.h {
            for dx in 0..shape.w {
                let sx = shape.x + if frame.flipped { shape.w - 1 - dx } else { dx };
                let sy = shape.y + dy;
                if sx >= image.width || sy >= image.height {
                    continue;
                }
                let at = sy as usize * image.width as usize + sx as usize;
                let Some(&index_color) = image.indices.get(at) else {
                    continue;
                };
                let [r, g, b] = palette.color(index_color);
                let px = cell_x + (frame.x - min_x) as u32 + dx;
                let py = cell_y + (frame.y - min_y) as u32 + dy;
                sheet.put_pixel(px, py, image::Rgba([r, g, b, 255]));
            }
        }
    }

    let id = format!("{room}_{}", anim.index);
    let relative = format!("{}/{id}.png", infengine_adventure::anim::ANIMATION_DIR);
    let path = project.join(&relative);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    sheet.save(&path).ok()?;

    Some(infengine_adventure::Animation {
        id,
        image: relative,
        frames,
        frame_size: glam::Vec2::new(cell_w as f32, cell_h as f32),
        columns,
        gap: glam::Vec2::ZERO,
        fps: scene_anim::fps(&anim.frames),
        looping: anim.looping,
        room: Some(room.to_string()),
        at: Some(glam::Vec2::new(min_x as f32, min_y as f32)),
    })
}
