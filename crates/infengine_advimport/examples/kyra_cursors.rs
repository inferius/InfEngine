//! A128: the game's own mouse pointers.
//!
//! `MOUSE.CPS` is not a shape bank but a 320x200 sheet with the
//! pointers laid on it over magenta, each arrow sitting on its own
//! black tile. The rectangles below were measured off that sheet: the
//! plain arrow, and the four directional ones the original shows when
//! the cursor is over a way out - which is how a player sees there is
//! a way down at all.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_cursors -- <game dir> <project>
//! ```

#![allow(clippy::print_stdout)]

use infengine_advimport::kyra::cps;
use infengine_advimport::kyra::palette::Palette;

/// Name, and where it sits on the sheet: x, y, width, height.
const SHAPES: &[(&str, u32, u32, u32, u32)] = &[
    ("pointer", 0, 0, 8, 10),
    ("up", 81, 0, 13, 17),
    ("right", 97, 0, 11, 17),
    ("down", 113, 0, 13, 17),
    ("left", 129, 0, 11, 17),
    ("no", 145, 0, 15, 17),
];

/// The sheet's own background, and the tile black the arrows sit on.
/// Both become transparent; the arrows themselves are white.
const CLEAR: &[[u8; 3]] = &[[170, 0, 170], [0, 0, 0]];

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_cursors <game dir> <project>");
        return;
    };
    let game = std::path::PathBuf::from(game);
    let project = std::path::PathBuf::from(project);

    let Ok(data) = std::fs::read(game.join("MOUSE.CPS")) else {
        println!("no MOUSE.CPS");
        return;
    };
    let Ok(sheet) = cps::decode(&data) else {
        println!("MOUSE.CPS did not decode");
        return;
    };
    let palette = std::fs::read(game.join("PALETTE.COL"))
        .ok()
        .and_then(|data| Palette::from_vga_6bit(&data))
        .or(sheet.palette.clone())
        .unwrap_or_default();
    let rgba = sheet.to_rgba(Some(&palette));

    let dir = project.join("cursors");
    if std::fs::create_dir_all(&dir).is_err() {
        println!("could not make the cursors directory");
        return;
    }
    for (name, x, y, width, height) in SHAPES {
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..*height {
            for column in 0..*width {
                let at = (((y + row) * sheet.width + x + column) * 4) as usize;
                let pixel = &rgba[at..at + 4];
                let colour = [pixel[0], pixel[1], pixel[2]];
                if CLEAR.contains(&colour) {
                    out.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    out.extend_from_slice(pixel);
                }
            }
        }
        let path = dir.join(format!("{name}.png"));
        match image::RgbaImage::from_raw(*width, *height, out)
            .ok_or_else(|| "size mismatch".to_string())
            .and_then(|image| image.save(&path).map_err(|err| err.to_string()))
        {
            Ok(()) => println!("  {name}: {width}x{height} -> cursors/{name}.png"),
            Err(err) => println!("  {name}: {err}"),
        }
    }
}
