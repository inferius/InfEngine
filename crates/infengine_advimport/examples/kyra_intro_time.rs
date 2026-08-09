//! A115: the intro's animations timed exactly as the original played them.
//!
//! A WSA holds frames; the order and speed they play at live in the
//! executable's sequence bytecode - including loops that replay a span
//! of frames, which a linear sheet cannot express. So this interprets
//! each sequence (loops and all), collects for every animation the
//! frames in the order they were shown and the ticks that passed, and
//! re-bakes the sheet in played order with the exact rate. The engine's
//! plain "frame after frame" playback then reproduces the original
//! frame for frame.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_intro_time -- <game dir> <project>
//! ```

#![allow(clippy::print_stdout)]

use std::path::PathBuf;

use glam::Vec2;
use infengine_advimport::kyra::palette::Palette;
use infengine_advimport::kyra::{cps, format80, wsa};

/// Argument bytes per opcode, a `2` meaning a little-endian word.
const SIZES: &[&[u8]] = &[
    &[1, 1], &[1], &[1, 1, 2, 1], &[1], &[1], &[1, 2, 1], &[2], &[2], &[], &[],
    &[1], &[1, 2], &[1], &[1], &[], &[1], &[1, 2, 1, 1], &[], &[], &[],
    &[], &[1, 1], &[1], &[2, 1, 2, 1, 1, 1], &[1], &[1], &[], &[], &[], &[],
];

/// The sequences and the pieces they animate: our animation id, the
/// wsa file list index the stream uses, and the stream's byte range.
struct Scene {
    at: usize,
    end: usize,
    pieces: &'static [(&'static str, usize, &'static str, &'static str)],
}

/// WSA list order in the executable: kallak, tree1, tree2, westwood,
/// mal-kal, destruct, kyrandia, shore, ...
const SCENES: &[Scene] = &[
    Scene { at: 0x2dcfe, end: 0x2e4f3, pieces: &[("intro3_kallak", 0, "KALLAK.WSA", "KALLAK.COL")] },
    Scene {
        at: 0x2e4f3,
        end: 0x2e709,
        pieces: &[
            ("intro2_tree1", 1, "TREE1.WSA", "TREE_EXP.COL"),
            ("intro2_tree2", 2, "TREE2.WSA", "TREE_EXP.COL"),
        ],
    },
    Scene { at: 0x2e709, end: 0x2e754, pieces: &[("intro1_westwood", 3, "WESTWOOD.WSA", "WW.COL")] },
    Scene { at: 0x2e754, end: 0x2e7b0, pieces: &[("intro1_kyrandia", 6, "KYRANDIA.WSA", "WW.COL")] },
    Scene { at: 0x2e7b0, end: 0x2ea17, pieces: &[("intro4_mal-kal", 4, "MAL-KAL.WSA", "MAL-KAL.COL")] },
];

/// The three hand-with-quill shapes from WRITING.CPS: a bank of
/// `u16 count`, word offsets, and per shape a small header
/// (flags u16, height u8, width u16, height again, sizes) followed by
/// Format80 pixels. The widths and heights match the rectangles the
/// Mac build cuts: 88x122, 80x117, 117x124.
fn hand_shapes(game: &std::path::Path) -> Vec<(u32, u32, Vec<u8>)> {
    let Some(bank) = std::fs::read(game.join("WRITING.CPS"))
        .ok()
        .and_then(|data| cps::decode(&data).ok())
    else {
        return Vec::new();
    };
    let buffer = &bank.indices;
    let count = u16::from_le_bytes([buffer[0], buffer[1]]) as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let at = u16::from_le_bytes([buffer[2 + i * 2], buffer[3 + i * 2]]) as usize;
        let Some(header) = buffer.get(at..at + 10) else { continue };
        let height = header[2] as u32;
        let width = u16::from_le_bytes([header[3], header[4]]) as u32;
        let want = (width * height) as usize;
        // Format80 unpacks to a run-length stream (header words 8..10
        // say how long), and in that stream a zero means "the next byte
        // counts transparent pixels" - the classic Westwood shape.
        let rle_len = u16::from_le_bytes([header[8], header[9]]) as usize;
        let pixels = buffer
            .get(at + 10..)
            .and_then(|chunk| format80::decompress_up_to(chunk, rle_len + 64).ok())
            .map(|mut rle| {
                rle.truncate(rle_len);
                let mut px = Vec::with_capacity(want);
                let mut i = 0;
                while px.len() < want && i < rle.len() {
                    let b = rle[i];
                    i += 1;
                    if b == 0 {
                        let run = *rle.get(i).unwrap_or(&0) as usize;
                        i += 1;
                        px.extend(std::iter::repeat_n(0u8, run));
                    } else {
                        px.push(b);
                    }
                }
                px.resize(want, 0);
                px
            });
        match pixels {
            Some(px) => out.push((width, height, px)),
            None => println!("hand shape {i}: did not decode"),
        }
    }
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_intro_time <game dir> <project>");
        return;
    };
    let game = PathBuf::from(game);
    let project = PathBuf::from(project);
    let Ok(exe) = std::fs::read(game.join("MAIN.EXE")) else {
        println!("no MAIN.EXE");
        return;
    };

    let anim_dir = project.join(infengine_adventure::anim::ANIMATION_DIR);
    let mut index = match infengine_adventure::anim::load_index(&anim_dir) {
        Ok(index) => index,
        Err(err) => {
            println!("refusing to touch a broken index: {err}");
            return;
        }
    };

    // TREE2 needs TREE1's last frame as its screen; brought over below.
    let mut tree_last: Option<Vec<u8>> = None;
    let hands = hand_shapes(&game);

    for scene in SCENES {
        let plays = played(&exe, scene.at, scene.end);
        for (id, wsa_at, file, col) in scene.pieces {
            let Some((order, ticks, draws)) = plays.get(wsa_at) else {
                println!("{id}: the stream never plays it");
                continue;
            };
            let Ok(data) = std::fs::read(game.join(file)) else { continue };
            let decoded = if *file == "TREE2.WSA" {
                wsa::decode_over(&data, tree_last.as_deref())
            } else {
                wsa::decode(&data)
            };
            let Ok(anim) = decoded else {
                println!("{id}: undecodable");
                continue;
            };
            if *file == "TREE1.WSA" {
                tree_last = anim.frames.last().cloned();
            }

            let palette = std::fs::read(game.join(col))
                .ok()
                .and_then(|data| Palette::from_vga_6bit(&data));
            let mut played: Vec<Vec<u8>> = order
                .iter()
                .filter_map(|frame| anim.frames.get(*frame).cloned())
                .collect();
            if played.is_empty() {
                continue;
            }
            // The hand rides over the frames: each drawShape belongs to
            // the frame just shown, and it rests where it was between
            // strokes - so the last position carries forward. The scene
            // sits at y 16, and shape coordinates are screen ones.
            if !draws.is_empty() && !hands.is_empty() {
                let mut current: Option<(usize, i32, i32)> = None;
                let mut next = draws.iter().peekable();
                for (shown, frame) in played.iter_mut().enumerate() {
                    while let Some((at, shape, x, y)) = next.peek() {
                        if *at > shown {
                            break;
                        }
                        current = Some((*shape, *x, *y - 16));
                        next.next();
                    }
                    if let Some((shape, x, y)) = current
                        && let Some((w, h, pixels)) = hands.get(shape)
                    {
                        stamp(frame, anim.width, anim.height, pixels, *w, *h, x, y);
                    }
                }
            }
            let fps = 60.0 * played.len() as f32 / (*ticks).max(1) as f32;
            let refs: Vec<&Vec<u8>> = played.iter().collect();
            let sheet = bake(&refs, anim.width, anim.height, palette.as_ref());
            let rel = format!("animations/{id}.png");
            if let Err(err) = save(&project.join(&rel), sheet.0, sheet.1, &sheet.2) {
                println!("{id}: {err}");
                continue;
            }
            let old = index.get(id).cloned();
            let mut row = infengine_adventure::anim::Animation::new(
                *id,
                &rel,
                played.len() as u32,
                Vec2::new(anim.width as f32, anim.height as f32),
            );
            row.columns =
                infengine_adventure::Animation::columns_for(anim.width as f32, played.len() as u32);
            row.fps = fps;
            row.looping = false;
            if let Some(old) = old {
                row.room = old.room;
                row.at = old.at;
            }
            index.insert(row);
            println!(
                "{id}: {} frames played over {:.1}s ({:.2} fps)",
                played.len(),
                *ticks as f32 / 60.0,
                fps
            );
        }
    }
    if let Err(err) = infengine_adventure::anim::save_index(&index, &anim_dir) {
        println!("could not write the index: {err}");
    }
}

/// Interpret one stream: which frames each WSA showed, and the ticks
/// its span of the scene took. Loops replay their body.
fn played(
    data: &[u8],
    at: usize,
    end: usize,
) -> std::collections::HashMap<usize, (Vec<usize>, u32, Vec<(usize, usize, i32, i32)>)> {
    use std::collections::HashMap;
    let mut current: HashMap<usize, usize> = HashMap::new();
    let mut order: HashMap<usize, (Vec<usize>, u32, Vec<(usize, usize, i32, i32)>)> = HashMap::new();
    let mut last_play: Option<usize> = None;
    let mut spans: HashMap<usize, (u32, u32)> = HashMap::new();
    let mut ticks = 0u32;
    let mut p = at;
    let mut loop_from: HashMap<i64, usize> = HashMap::new();
    let mut loops_left: HashMap<i64, u16> = HashMap::new();
    while p < end {
        let op = data[p] as usize;
        let Some(sizes) = SIZES.get(op) else { break };
        p += 1;
        let mut args = Vec::new();
        for &s in *sizes {
            if s == 1 {
                args.push(data[p] as i64);
                p += 1;
            } else {
                args.push(u16::from_le_bytes([data[p], data[p + 1]]) as i64);
                p += 2;
            }
        }
        match op {
            0x06 | 0x07 => ticks += args[0] as u32,
            0x02 => {
                let (wsa, frame) = (args[0] as usize, args[1] as usize);
                current.insert(wsa, frame);
                note(&mut order, &mut spans, wsa, frame, ticks);
                last_play = Some(wsa);
            }
            0x03 => {
                let wsa = args[0] as usize;
                let frame = current.get(&wsa).map(|f| f + 1).unwrap_or(0);
                current.insert(wsa, frame);
                note(&mut order, &mut spans, wsa, frame, ticks);
                last_play = Some(wsa);
            }
            0x04 => {
                let wsa = args[0] as usize;
                let frame = current.get(&wsa).map(|f| f.saturating_sub(1)).unwrap_or(0);
                current.insert(wsa, frame);
                note(&mut order, &mut spans, wsa, frame, ticks);
                last_play = Some(wsa);
            }
            // drawShape rides on the frame that was just shown - the
            // writing hand is drawn over the WSA every step.
            0x05 => {
                if let Some(wsa) = last_play
                    && let Some(entry) = order.get_mut(&wsa)
                {
                    let shown = entry.0.len() - 1;
                    entry.2.push((shown, args[0] as usize, args[1] as i32, args[2] as i32));
                }
            }
            0x0A => {
                loop_from.insert(args[0], p);
            }
            0x0B => {
                let (slot, count) = (args[0], args[1] as u16);
                let left = loops_left.remove(&slot);
                let again = match left {
                    None => count > 1,
                    Some(left) => left > 1,
                };
                if again {
                    loops_left.insert(slot, left.map(|l| l - 1).unwrap_or(count - 1));
                    if let Some(from) = loop_from.get(&slot) {
                        p = *from;
                    }
                }
            }
            _ => {}
        }
    }
    for (wsa, (first, last)) in spans {
        if let Some(entry) = order.get_mut(&wsa) {
            entry.1 = (last - first).max(entry.0.len() as u32);
        }
    }
    order
}

fn note(
    order: &mut std::collections::HashMap<usize, (Vec<usize>, u32, Vec<(usize, usize, i32, i32)>)>,
    spans: &mut std::collections::HashMap<usize, (u32, u32)>,
    wsa: usize,
    frame: usize,
    ticks: u32,
) {
    order.entry(wsa).or_default().0.push(frame);
    let span = spans.entry(wsa).or_insert((ticks, ticks));
    span.1 = ticks;
}

/// Draw a shape's non-zero pixels onto a frame at the given spot.
fn stamp(frame: &mut [u8], fw: u32, fh: u32, pixels: &[u8], w: u32, h: u32, x: i32, y: i32) {
    for row in 0..h as i32 {
        let ty = y + row;
        if ty < 0 || ty >= fh as i32 {
            continue;
        }
        for col in 0..w as i32 {
            let tx = x + col;
            if tx < 0 || tx >= fw as i32 {
                continue;
            }
            let ink = pixels[(row * w as i32 + col) as usize];
            if ink != 0 {
                frame[(ty * fw as i32 + tx) as usize] = ink;
            }
        }
    }
}

/// Frames in played order, laid out as a grid, coloured by the scene's
/// palette with index 0 transparent.
fn bake(
    frames: &[&Vec<u8>],
    width: u32,
    height: u32,
    palette: Option<&Palette>,
) -> (u32, u32, Vec<u8>) {
    let owned;
    let palette = match palette {
        Some(p) => p,
        None => {
            owned = Palette::default();
            &owned
        }
    };
    let count = frames.len() as u32;
    let columns = infengine_adventure::Animation::columns_for(width as f32, count);
    let rows = count.div_ceil(columns);
    let (sw, sh) = (width * columns, height * rows);
    let mut rgba = vec![0u8; sw as usize * sh as usize * 4];
    for (i, frame) in frames.iter().enumerate() {
        let expanded = palette.expand_rgba(frame, Some(0));
        let (ox, oy) = ((i as u32 % columns) * width, (i as u32 / columns) * height);
        for y in 0..height as usize {
            let src = y * width as usize * 4;
            let dst = ((oy as usize + y) * sw as usize + ox as usize) * 4;
            let span = width as usize * 4;
            rgba[dst..dst + span].copy_from_slice(&expanded[src..src + span]);
        }
    }
    (sw, sh, rgba)
}

fn save(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "pixel data does not match its size".to_string())?
        .save(path)
        .map_err(|err| err.to_string())
}
