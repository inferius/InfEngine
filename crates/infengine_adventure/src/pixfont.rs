//! A73: bitmap fonts - text drawn from a picture.
//!
//! A game's lettering is part of its art. The engine's interface text
//! comes from system fonts, but subtitles inside the picture want the
//! game's own: chunky, palette-coloured, pixel-exact. A bitmap font is
//! an atlas image and a table saying where each character sits in it
//! and how wide it draws - nothing about HOW to draw, so the same file
//! serves the game, the editor, and any tool that can blit a rectangle.
//!
//! Glyph rectangles bake the character's vertical offset in: a cell is
//! a full line-height tall with the ink already sitting at its proper
//! depth, so a renderer places cells on a baseline-less pen line and
//! advances - no per-glyph metrics arithmetic at draw time.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Directory holding fonts, relative to the project root.
pub const FONT_DIR: &str = "fonts";
/// Suffix identifying one.
pub const FONT_SUFFIX: &str = ".font.ron";

/// One character's place in the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixGlyph {
    /// Top-left of its cell in the atlas, in pixels.
    pub x: u32,
    pub y: u32,
    /// Ink width, which is also the advance.
    pub w: u32,
}

/// A bitmap font: an atlas and where every character lives in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PixFont {
    pub id: String,
    /// Atlas image, project-relative. White ink with alpha, so drawing
    /// tints it to any colour with a multiply.
    pub image: String,
    /// Every cell is this tall.
    pub line_height: u32,
    /// Pixels between characters.
    #[serde(default)]
    pub letter_spacing: u32,
    /// Width of a space. Zero falls back to the widest glyph's half.
    #[serde(default)]
    pub space_width: u32,
    pub glyphs: BTreeMap<char, PixGlyph>,
}

/// One character, placed: where its cell sits in the atlas and where
/// it goes relative to the text's origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// Cell rectangle in the atlas, in pixels.
    pub atlas_min: Vec2,
    pub atlas_max: Vec2,
    /// Offset from the text origin, in pixels.
    pub offset: Vec2,
}

impl PixFont {
    /// Width of a space, resolved.
    fn space(&self) -> u32 {
        if self.space_width > 0 {
            return self.space_width;
        }
        let widest = self.glyphs.values().map(|g| g.w).max().unwrap_or(4);
        (widest / 2).max(2)
    }

    /// The glyph for `c`, falling back through case to `?`.
    ///
    /// A missing letter still occupies the line: swallowing it would
    /// silently shorten words, and a text full of `?` names exactly
    /// which characters the font is missing.
    fn glyph(&self, c: char) -> Option<&PixGlyph> {
        self.glyphs
            .get(&c)
            .or_else(|| self.glyphs.get(&c.to_ascii_uppercase()))
            .or_else(|| self.glyphs.get(&c.to_ascii_lowercase()))
            .or_else(|| self.glyphs.get(&'?'))
    }

    /// Lay a line of text out. Newlines stack lines.
    pub fn layout(&self, text: &str) -> Vec<Placed> {
        let mut out = Vec::new();
        let mut pen = Vec2::ZERO;
        for c in text.chars() {
            if c == '\n' {
                pen.x = 0.0;
                pen.y += self.line_height as f32 + 1.0;
                continue;
            }
            if c == ' ' {
                pen.x += (self.space() + self.letter_spacing) as f32;
                continue;
            }
            let Some(glyph) = self.glyph(c) else { continue };
            out.push(Placed {
                atlas_min: Vec2::new(glyph.x as f32, glyph.y as f32),
                atlas_max: Vec2::new(
                    (glyph.x + glyph.w) as f32,
                    (glyph.y + self.line_height) as f32,
                ),
                offset: pen,
            });
            pen.x += (glyph.w + self.letter_spacing) as f32;
        }
        out
    }

    /// Size the laid-out text covers.
    pub fn measure(&self, text: &str) -> Vec2 {
        let mut widest = 0f32;
        let mut pen = Vec2::new(0.0, self.line_height as f32);
        for c in text.chars() {
            match c {
                '\n' => {
                    widest = widest.max(pen.x);
                    pen.x = 0.0;
                    pen.y += self.line_height as f32 + 1.0;
                }
                ' ' => pen.x += (self.space() + self.letter_spacing) as f32,
                other => {
                    if let Some(glyph) = self.glyph(other) {
                        pen.x += (glyph.w + self.letter_spacing) as f32;
                    }
                }
            }
        }
        Vec2::new(widest.max(pen.x), pen.y)
    }
}

/// Load every font in a directory, sorted by id.
pub fn load_dir(dir: &Path) -> Vec<PixFont> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_font = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(FONT_SUFFIX));
        if !is_font {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path)
            && let Ok(font) = ron::from_str::<PixFont>(&text)
        {
            out.push(font);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Write one font next to its atlas.
pub fn save(font: &PixFont, dir: &Path) -> std::io::Result<()> {
    let text = ron::ser::to_string_pretty(
        font,
        ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
    )
    .map_err(std::io::Error::other)?;
    fs::create_dir_all(dir)?;
    fs::write(dir.join(format!("{}{FONT_SUFFIX}", font.id)), text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> PixFont {
        let mut glyphs = BTreeMap::new();
        glyphs.insert('A', PixGlyph { x: 0, y: 0, w: 6 });
        glyphs.insert('b', PixGlyph { x: 8, y: 0, w: 4 });
        glyphs.insert('?', PixGlyph { x: 16, y: 0, w: 5 });
        PixFont {
            id: "test".into(),
            image: "fonts/test.png".into(),
            line_height: 8,
            letter_spacing: 1,
            space_width: 3,
            glyphs,
        }
    }

    #[test]
    fn text_lays_out_left_to_right_with_spacing() {
        let placed = font().layout("Ab");
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].offset, Vec2::ZERO);
        // A is 6 wide plus 1 spacing.
        assert_eq!(placed[1].offset, Vec2::new(7.0, 0.0));
        assert_eq!(placed[1].atlas_min, Vec2::new(8.0, 0.0));
        assert_eq!(placed[1].atlas_max, Vec2::new(12.0, 8.0));
    }

    #[test]
    fn a_missing_letter_shows_as_the_question_mark() {
        let placed = font().layout("A#");
        assert_eq!(placed.len(), 2, "the unknown character still lands");
        assert_eq!(placed[1].atlas_min.x, 16.0);
    }

    #[test]
    fn case_falls_back_before_the_question_mark() {
        // Only 'b' exists; 'B' should find it rather than punt.
        let placed = font().layout("B");
        assert_eq!(placed[0].atlas_min.x, 8.0);
    }

    #[test]
    fn newlines_stack_and_measure_matches_layout() {
        let f = font();
        let placed = f.layout("A\nAb");
        assert_eq!(placed[0].offset.y, 0.0);
        assert_eq!(placed[1].offset, Vec2::new(0.0, 9.0));
        let size = f.measure("A\nAb");
        // Second line: A(6)+1+b(4)+1 = 12 wide; two lines of 8 with a
        // 1-pixel gap between.
        assert_eq!(size, Vec2::new(12.0, 17.0));
    }

    #[test]
    fn a_font_round_trips_through_a_directory() {
        let dir = std::env::temp_dir().join("infengine_pixfont_round_trip");
        let _ = fs::remove_dir_all(&dir);
        save(&font(), &dir).expect("save");
        let loaded = load_dir(&dir);
        assert_eq!(loaded, vec![font()]);
        let _ = fs::remove_dir_all(&dir);
    }
}
