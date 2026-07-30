//! A3.3: VGA palettes.
//!
//! DOS-era art stores colour components as 6-bit values, because that is
//! what the VGA DAC accepted. Widening them to 8 bits by shifting left
//! two would cap white at 252 and leave every image slightly dark, so the
//! top two bits are replicated into the bottom - `v << 2 | v >> 4` maps
//! 63 to 255 exactly and keeps the ramp even.

/// Bytes in a 256-colour VGA palette: three components per entry.
pub const VGA_PALETTE_BYTES: usize = 768;

/// A 256-entry RGB palette, already widened to 8 bits per component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    entries: [[u8; 3]; 256],
}

impl Default for Palette {
    /// A greyscale ramp, so an image decoded without its palette is
    /// still legible rather than uniformly black.
    fn default() -> Self {
        let mut entries = [[0u8; 3]; 256];
        for (i, e) in entries.iter_mut().enumerate() {
            *e = [i as u8; 3];
        }
        Self { entries }
    }
}

/// Widen one 6-bit VGA component to 8 bits.
fn widen(v: u8) -> u8 {
    let v = v & 0x3F;
    (v << 2) | (v >> 4)
}

impl Palette {
    /// Parse a 768-byte 6-bit VGA palette.
    ///
    /// Returns `None` when the slice is the wrong length.
    pub fn from_vga_6bit(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != VGA_PALETTE_BYTES {
            return None;
        }
        let mut entries = [[0u8; 3]; 256];
        for (i, e) in entries.iter_mut().enumerate() {
            let base = i * 3;
            *e = [
                widen(bytes[base]),
                widen(bytes[base + 1]),
                widen(bytes[base + 2]),
            ];
        }
        Some(Self { entries })
    }

    pub fn color(&self, index: u8) -> [u8; 3] {
        self.entries[index as usize]
    }

    /// Copy of this palette with a run of 6-bit RGB triples written over
    /// it, starting at `start`.
    ///
    /// This is how a Kyrandia room recolours itself: the game-wide
    /// palette is the base and each room replaces a window near the top
    /// of it. Triples that would run past index 255 are dropped rather
    /// than wrapping around to index 0 and corrupting the low colours.
    pub fn with_override(&self, start: u8, six_bit_rgb: &[u8]) -> Self {
        let mut out = self.clone();
        for (i, triple) in six_bit_rgb.chunks_exact(3).enumerate() {
            let Some(index) = (start as usize).checked_add(i).filter(|i| *i < 256) else {
                break;
            };
            out.entries[index] = [widen(triple[0]), widen(triple[1]), widen(triple[2])];
        }
        out
    }

    /// Expand an indexed image to RGBA8.
    ///
    /// `transparent` names an index to write as fully transparent, which
    /// is how sprite sheets mark their background; pass `None` for an
    /// opaque background image.
    pub fn expand_rgba(&self, indices: &[u8], transparent: Option<u8>) -> Vec<u8> {
        let mut out = Vec::with_capacity(indices.len() * 4);
        for &i in indices {
            let [r, g, b] = self.color(i);
            let a = if transparent == Some(i) { 0 } else { 255 };
            out.extend_from_slice(&[r, g, b, a]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_bit_white_widens_to_full_white() {
        // The whole point of the replicate: 63 must reach 255, not 252.
        assert_eq!(widen(63), 255);
    }

    #[test]
    fn six_bit_black_stays_black() {
        assert_eq!(widen(0), 0);
    }

    #[test]
    fn widening_is_monotonic_and_spans_the_range() {
        let mut prev = 0;
        for v in 0..=63u8 {
            let w = widen(v);
            assert!(w >= prev, "widen must not go backwards at {v}");
            prev = w;
        }
        assert_eq!(widen(32), 130);
    }

    #[test]
    fn high_bits_above_six_are_ignored() {
        // Real palettes only ever use the low six bits; a stray high bit
        // must not shift the colour.
        assert_eq!(widen(0xC0 | 63), widen(63));
    }

    #[test]
    fn palette_parses_and_indexes() {
        let mut bytes = vec![0u8; VGA_PALETTE_BYTES];
        bytes[3] = 63; // colour 1 = pure red
        bytes[4] = 0;
        bytes[5] = 0;
        let pal = Palette::from_vga_6bit(&bytes).expect("768 bytes");
        assert_eq!(pal.color(0), [0, 0, 0]);
        assert_eq!(pal.color(1), [255, 0, 0]);
    }

    #[test]
    fn wrong_length_palette_is_rejected() {
        assert!(Palette::from_vga_6bit(&[0u8; 100]).is_none());
        assert!(Palette::from_vga_6bit(&[]).is_none());
    }

    #[test]
    fn default_palette_is_a_grey_ramp() {
        let pal = Palette::default();
        assert_eq!(pal.color(0), [0, 0, 0]);
        assert_eq!(pal.color(128), [128, 128, 128]);
        assert_eq!(pal.color(255), [255, 255, 255]);
    }

    #[test]
    fn expand_rgba_is_opaque_without_a_transparent_index() {
        let pal = Palette::default();
        let rgba = pal.expand_rgba(&[0, 255], None);
        assert_eq!(rgba, vec![0, 0, 0, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn expand_rgba_zeroes_alpha_on_the_transparent_index() {
        let pal = Palette::default();
        let rgba = pal.expand_rgba(&[0, 255], Some(0));
        assert_eq!(rgba[3], 0, "index 0 should be transparent");
        assert_eq!(rgba[7], 255, "index 255 should stay opaque");
    }
}
