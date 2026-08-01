//! Cut the player character out of its sprite sheet.
//!
//! Kyrandia keeps Brandon in one 320x200 image with his walk cycles laid
//! out in bands: a side view along the top, then a front view and a back
//! view of six frames each. Nothing in the file says so - the bands are
//! found by looking for rows and columns that hold nothing.
//!
//! ```text
//! cargo run -p infengine_advimport --example kyra_actor -- <project> [sprite]
//! ```
//!
//! Writes three animations into the project and points its manifest at
//! them. The left-facing walk is the right-facing one mirrored, which is
//! what a sheet with one side view means.

#![allow(clippy::print_stdout)]

use std::path::{Path, PathBuf};

use glam::Vec2;
use infengine_adventure::anim::{self, Animation};
use infengine_adventure::pack::{ActorLook, GameManifest};

/// Frames a walk cycle is expected to have.
///
/// The front and back bands hold exactly six. The side band holds more,
/// because it also carries the standing poses, so it is cut to the same
/// six taken from where the walk starts.
const WALK_FRAMES: usize = 6;
/// How much wider than the median a segment has to be before it is read
/// as two frames that touch.
const MERGED: f32 = 1.6;
/// And how much wider before it is not part of this cycle at all.
///
/// The front and back bands each end with a figure six times the width
/// of a walk frame - a different pose sharing the row. Splitting it
/// would make six frames of nonsense, and letting it set the frame box
/// made every frame three times too wide with the character in a corner.
const FOREIGN: f32 = 2.5;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(project) = args.next() else {
        println!("usage: kyra_actor <project> [sprite]");
        return;
    };
    let project = PathBuf::from(project);
    let sprite = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| project.join("sprites/brandon.png"));

    let image = match image::open(&sprite) {
        Ok(image) => image.to_rgba8(),
        Err(err) => {
            println!("could not read {}: {err}", sprite.display());
            return;
        }
    };
    let (width, height) = (image.width() as usize, image.height() as usize);
    let pixels = image.into_raw();

    let bands = bands(&pixels, width, height);
    println!("{} bands of content", bands.len());
    for (top, bottom) in &bands {
        println!("   rows {top}-{bottom} ({} tall)", bottom - top + 1);
    }

    // The walk cycles are the tall bands. Anything short is a row of
    // faces or held items, which this is not about.
    let tall: Vec<&(usize, usize)> = bands.iter().filter(|(t, b)| b - t + 1 >= 24).collect();
    if tall.len() < 3 {
        println!("expected three tall bands, found {}", tall.len());
        return;
    }

    // Top to bottom: the side view, then front, then back. That is the
    // order this sheet uses and the only thing here taken on trust -
    // the pictures are what say which is which, and swapping two lines
    // in the manifest is the fix if a different release differs.
    let named = [
        ("actor_walk_right", tall[0]),
        ("actor_walk_front", tall[1]),
        ("actor_walk_back", tall[2]),
    ];

    let mut written = Vec::new();
    for (id, (top, bottom)) in named {
        let Some(boxes) = frames(&pixels, width, *top, *bottom) else {
            println!("{id}: no frames found");
            continue;
        };
        let taken: Vec<(usize, usize)> = boxes.into_iter().take(WALK_FRAMES).collect();
        // The box is measured from the frames actually taken, not from
        // everything in the row.
        let Some(frame_width) = taken.iter().map(|(a, b)| b - a + 1).max() else {
            continue;
        };
        let frame = (frame_width, bottom - top + 1);
        let sheet = cut(&pixels, width, *top, frame, &taken);
        let rel = format!("animations/{id}.png");
        let path = project.join(&rel);
        if let Err(err) = save(&path, frame.0 * taken.len(), frame.1, &sheet) {
            println!("{id}: {err}");
            continue;
        }
        println!(
            "   {id}: {} frames of {}x{}",
            taken.len(),
            frame.0,
            frame.1
        );
        written.push((id, rel, taken.len(), frame));
    }
    if written.is_empty() {
        return;
    }

    // Register them, keeping whatever the project already had.
    let dir = project.join("animations");
    let mut set = anim::load_index(&dir).unwrap_or_default();
    for (id, rel, frames, (w, h)) in &written {
        set.insert(Animation::new(
            *id,
            rel,
            *frames as u32,
            Vec2::new(*w as f32, *h as f32),
        ));
    }
    if let Err(err) = anim::save_index(&set, &dir) {
        println!("could not write the animation index: {err}");
        return;
    }

    let Some(mut manifest) = GameManifest::load(&project) else {
        println!("no game.ron to point at them");
        return;
    };
    manifest.actor = Some(ActorLook {
        // The sheet's side view faces right, so the left-facing walk is
        // left empty and drawn mirrored. Naming both would draw the
        // figure walking left while facing right.
        walk_left: String::new(),
        walk_right: "actor_walk_right".into(),
        walk_back: "actor_walk_back".into(),
        walk_front: "actor_walk_front".into(),
        idle: String::new(),
    });
    match manifest.save(&project) {
        Ok(()) => println!("manifest now names the actor"),
        Err(err) => println!("could not write game.ron: {err}"),
    }
}

/// Rows that hold something, grouped into bands.
fn bands(pixels: &[u8], width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for y in 0..=height {
        let full = y < height && (0..width).any(|x| solid(pixels, width, x, y));
        match (full, start) {
            (true, None) => start = Some(y),
            (false, Some(top)) => {
                out.push((top, y - 1));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Frame size and the left edge of each frame in a band.
///
/// Frames are cropped tight in the sheet and are not the same width, so
/// the box is the widest of them and each frame is centred in it.
/// Anything much wider than the rest is two figures that touch, and is
/// split down the middle rather than kept as one enormous frame.
fn frames(
    pixels: &[u8],
    width: usize,
    top: usize,
    bottom: usize,
) -> Option<Vec<(usize, usize)>> {
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for x in 0..=width {
        let full = x < width && (top..=bottom).any(|y| solid(pixels, width, x, y));
        match (full, start) {
            (true, None) => start = Some(x),
            (false, Some(left)) => {
                segments.push((left, x - 1));
                start = None;
            }
            _ => {}
        }
    }
    if segments.is_empty() {
        return None;
    }

    let mut widths: Vec<usize> = segments.iter().map(|(a, b)| b - a + 1).collect();
    widths.sort_unstable();
    let median = widths[widths.len() / 2] as f32;

    let mut split: Vec<(usize, usize)> = Vec::new();
    for (left, right) in segments {
        let span = (right - left + 1) as f32;
        if span > median * FOREIGN {
            continue;
        }
        if span > median * MERGED {
            let mid = left + (right - left).div_ceil(2);
            split.push((left, mid));
            split.push((mid + 1, right));
        } else {
            split.push((left, right));
        }
    }
    (!split.is_empty()).then_some(split)
}

/// Lay the frames out left to right in one sheet, each centred in its box.
fn cut(
    pixels: &[u8],
    width: usize,
    top: usize,
    frame: (usize, usize),
    boxes: &[(usize, usize)],
) -> Vec<u8> {
    let (fw, fh) = frame;
    let sheet_width = fw * boxes.len();
    let mut out = vec![0u8; sheet_width * fh * 4];
    for (index, (left, right)) in boxes.iter().enumerate() {
        let span = right - left + 1;
        let pad = (fw - span) / 2;
        for y in 0..fh {
            for x in 0..span {
                let src = ((top + y) * width + left + x) * 4;
                if !solid(pixels, width, left + x, top + y) {
                    continue;
                }
                let dst = (y * sheet_width + index * fw + pad + x) * 4;
                out[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
        }
    }
    out
}

/// Is this pixel part of the figure?
///
/// The sheet's background is opaque black rather than transparent, so
/// alpha alone does not answer it. Black is also a colour the figure
/// uses, but only inside its outline, and taking the outline's own black
/// as background would only shave the edge - whereas keeping the
/// background would draw a rectangle around the character.
fn solid(pixels: &[u8], width: usize, x: usize, y: usize) -> bool {
    let at = (y * width + x) * 4;
    let Some(pixel) = pixels.get(at..at + 4) else {
        return false;
    };
    pixel[3] > 0 && (pixel[0], pixel[1], pixel[2]) != (0, 0, 0)
}

fn save(path: &Path, width: usize, height: usize, rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    image::RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec())
        .ok_or_else(|| "frame data does not match its size".to_string())?
        .save(path)
        .map_err(|err| err.to_string())
}
