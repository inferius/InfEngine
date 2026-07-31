//! A7.2: finding textures inside a Black Mirror scene and putting them
//! back together.
//!
//! After the scene graph, a `.dat` holds its art as a run of resources.
//! Each one opens with a length-prefixed path such as
//! `Scenes/LighthouseDay/s097_LighthouseDay.tga`, followed by a block of
//! metadata this importer does not need, followed by the texture's tiles
//! as consecutive DDS blobs.
//!
//! The metadata between the name and the first tile has no known layout,
//! so the reader does not try to parse it: it takes the name, then scans
//! forward to the first DDS blob and follows the `[u32 size][DDS]` chain
//! until it stops matching. The chain naturally ends where the next
//! resource begins, which is what makes the shortcut safe.
//!
//! Tiles arrive row by row with widths descending across a row - 1024,
//! 256, 32 - so a new row starts wherever a tile is not narrower than
//! the one before it. Splitting on a change of height instead looks
//! right until a texture ends with two rows of equal height, and then
//! the last strip of the image is assembled out of order.

use std::fmt;

use super::dds::{self, DdsError};

/// Longest resource path worth believing.
const MAX_NAME_LEN: u32 = 256;
/// Shortest string that can be a resource path.
const MIN_NAME_LEN: u32 = 5;
/// Refuse a tile chain longer than this; a scene has tens of tiles per
/// texture, not thousands.
const MAX_TILES: usize = 4096;
/// Smallest a blob can be and still hold a DDS preamble.
const MIN_BLOB: usize = 128;
/// Largest blob worth believing. A length field read from the wrong
/// place would otherwise send the reader off the end of the file.
const MAX_BLOB: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceError {
    /// No DDS blob followed the resource name.
    NoTiles,
    /// Tiles were found but none decoded.
    NoDecodableTiles { first: DdsError },
    /// Rows did not form a rectangle.
    RaggedGrid,
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTiles => write!(f, "resource has no DDS tiles"),
            Self::NoDecodableTiles { first } => {
                write!(f, "no tile decoded, first failure: {first}")
            }
            Self::RaggedGrid => write!(f, "tile rows are not the same total width"),
        }
    }
}

impl std::error::Error for ResourceError {}

/// A texture named inside a scene file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRef {
    /// Path as stored, e.g. `Scenes/LighthouseDay/s097.tga`.
    pub path: String,
    /// Byte offset just past the name.
    pub after_name: usize,
}

impl ResourceRef {
    /// Just the file name, lowercased - what a `Layer` refers to.
    pub fn file_name(&self) -> String {
        self.path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.path)
            .to_ascii_lowercase()
    }
}

/// Is this byte plausible inside a resource path?
fn path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'/' | b'\\' | b'.' | b'_' | b'-' | b' ')
}

/// Find every length-prefixed resource path in `data`.
///
/// A candidate must be length-prefixed, entirely made of path bytes, and
/// contain a dot - which is enough to separate real paths from the
/// occasional readable run inside compressed pixels.
pub fn find_resources(data: &[u8], from: usize) -> Vec<ResourceRef> {
    let mut out = Vec::new();
    let mut i = from;
    while i + 4 < data.len() {
        let len = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        if !(MIN_NAME_LEN..=MAX_NAME_LEN).contains(&len) {
            i += 1;
            continue;
        }
        let start = i + 4;
        let end = start + len as usize;
        let Some(bytes) = data.get(start..end) else {
            i += 1;
            continue;
        };
        if !bytes.iter().all(|b| path_byte(*b)) || !bytes.contains(&b'.') {
            i += 1;
            continue;
        }
        let Ok(path) = std::str::from_utf8(bytes) else {
            i += 1;
            continue;
        };
        out.push(ResourceRef {
            path: path.to_string(),
            after_name: end,
        });
        i = end;
    }
    out
}

/// One tile plus where it sat in the chain.
#[derive(Debug, Clone)]
struct RawTile {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Follow the `[u32 size][DDS]` chain that starts after `from`.
fn read_tiles(data: &[u8], from: usize) -> Result<Vec<RawTile>, ResourceError> {
    let Some(first) = find_magic(data, from) else {
        return Err(ResourceError::NoTiles);
    };
    let mut pos = first.saturating_sub(4);
    let mut tiles = Vec::new();
    let mut first_error = None;

    while pos + 8 < data.len() && tiles.len() < MAX_TILES {
        let size =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        if data.get(pos + 4..pos + 8) != Some(b"DDS ")
            || !(MIN_BLOB..=MAX_BLOB).contains(&size)
        {
            break;
        }
        let Some(blob) = data.get(pos + 4..pos + 4 + size) else {
            break;
        };
        match dds::decode(blob) {
            Ok(tile) => tiles.push(RawTile {
                width: tile.width,
                height: tile.height,
                rgba: tile.rgba,
            }),
            Err(err) => {
                first_error.get_or_insert(err);
            }
        }
        pos += 4 + size;
    }

    if tiles.is_empty() {
        return match first_error {
            Some(first) => Err(ResourceError::NoDecodableTiles { first }),
            None => Err(ResourceError::NoTiles),
        };
    }
    Ok(tiles)
}

fn find_magic(data: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(4)
        .position(|w| w == b"DDS ")
        .map(|p| p + from)
}

/// A reassembled texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major from the top.
    pub rgba: Vec<u8>,
}

impl Texture {
    /// Crop to `width` x `height`, dropping the power-of-two padding a
    /// tile grid adds. A request larger than the texture is clamped.
    pub fn cropped(&self, width: u32, height: u32) -> Texture {
        let w = width.min(self.width).max(1);
        let h = height.min(self.height).max(1);
        if w == self.width && h == self.height {
            return self.clone();
        }
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h as usize {
            let start = (y * self.width as usize) * 4;
            rgba.extend_from_slice(&self.rgba[start..start + w as usize * 4]);
        }
        Texture {
            width: w,
            height: h,
            rgba,
        }
    }
}

/// Split a tile chain into rows.
///
/// Widths descend across a row, so a tile that is not narrower than its
/// predecessor opens a new row.
fn split_rows(tiles: Vec<RawTile>) -> Vec<Vec<RawTile>> {
    let mut rows: Vec<Vec<RawTile>> = Vec::new();
    for tile in tiles {
        let starts_row = rows
            .last()
            .and_then(|r| r.last())
            .is_none_or(|prev| tile.width >= prev.width || tile.height != prev.height);
        if starts_row {
            rows.push(Vec::new());
        }
        if let Some(row) = rows.last_mut() {
            row.push(tile);
        }
    }
    rows
}

/// Read a texture: locate its tiles and stitch them into one image.
pub fn read_texture(data: &[u8], after_name: usize) -> Result<Texture, ResourceError> {
    let rows = split_rows(read_tiles(data, after_name)?);
    let width: u32 = rows[0].iter().map(|t| t.width).sum();
    let height: u32 = rows.iter().map(|r| r[0].height).sum();
    if rows
        .iter()
        .any(|r| r.iter().map(|t| t.width).sum::<u32>() != width)
    {
        return Err(ResourceError::RaggedGrid);
    }

    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let mut oy = 0usize;
    for row in &rows {
        let mut ox = 0usize;
        for tile in row {
            for y in 0..tile.height as usize {
                let src = y * tile.width as usize * 4;
                let dst = ((oy + y) * width as usize + ox) * 4;
                let span = tile.width as usize * 4;
                rgba[dst..dst + span].copy_from_slice(&tile.rgba[src..src + span]);
            }
            ox += tile.width as usize;
        }
        oy += row[0].height as usize;
    }

    Ok(Texture {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a DDS blob of one solid colour, uncompressed BGRA32.
    fn tile_blob(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut header = vec![0u8; 128];
        header[..4].copy_from_slice(b"DDS ");
        header[4..8].copy_from_slice(&124u32.to_le_bytes());
        header[12..16].copy_from_slice(&height.to_le_bytes());
        header[16..20].copy_from_slice(&width.to_le_bytes());
        // fourcc stays zero: uncompressed.
        header[88..92].copy_from_slice(&32u32.to_le_bytes()); // dwRGBBitCount
        for (i, mask) in [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]
            .iter()
            .enumerate()
        {
            let at = 92 + i * 4; // dwRBitMask onwards
            header[at..at + 4].copy_from_slice(&mask.to_le_bytes());
        }
        // Stored B G R A.
        let pixel = [color[2], color[1], color[0], color[3]];
        for _ in 0..(width * height) {
            header.extend_from_slice(&pixel);
        }
        header
    }

    fn chained(tiles: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for blob in tiles {
            out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
            out.extend_from_slice(blob);
        }
        out
    }

    fn named(path: &str) -> Vec<u8> {
        let mut out = (path.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(path.as_bytes());
        out
    }

    #[test]
    fn a_resource_path_is_found_and_its_file_name_extracted() {
        let data = named("Scenes/LighthouseDay/s097_LighthouseDay.tga");
        let found = find_resources(&data, 0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "Scenes/LighthouseDay/s097_LighthouseDay.tga");
        assert_eq!(found[0].file_name(), "s097_lighthouseday.tga");
    }

    #[test]
    fn strings_without_a_dot_are_not_resources() {
        // Plenty of readable runs appear inside pixel data.
        let data = named("NotAPathJustText");
        assert!(find_resources(&data, 0).is_empty());
    }

    #[test]
    fn a_length_prefix_that_overruns_the_buffer_is_ignored() {
        let mut data = 200u32.to_le_bytes().to_vec();
        data.extend_from_slice(b"short.tga");
        // The 200-byte claim cannot be satisfied, but the scan must keep
        // going rather than give up or panic.
        assert!(find_resources(&data, 0).is_empty());
    }

    #[test]
    fn two_resources_in_a_row_are_both_found() {
        let mut data = named("a/one.tga");
        data.extend_from_slice(&named("b/two.tst"));
        let found = find_resources(&data, 0);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].file_name(), "two.tst");
    }

    #[test]
    fn a_single_tile_texture_reassembles() {
        let mut data = named("x/one.tga");
        let at = data.len();
        data.extend_from_slice(&chained(&[tile_blob(4, 2, [10, 20, 30, 255])]));
        let tex = read_texture(&data, at).expect("texture");
        assert_eq!((tex.width, tex.height), (4, 2));
        assert_eq!(&tex.rgba[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn tiles_stitch_into_a_grid_in_row_major_order() {
        // Two rows of two tiles: widths descend within a row.
        let mut data = named("x/grid.tga");
        let at = data.len();
        data.extend_from_slice(&chained(&[
            tile_blob(4, 2, [1, 0, 0, 255]),
            tile_blob(2, 2, [2, 0, 0, 255]),
            tile_blob(4, 2, [3, 0, 0, 255]),
            tile_blob(2, 2, [4, 0, 0, 255]),
        ]));
        let tex = read_texture(&data, at).expect("texture");
        assert_eq!((tex.width, tex.height), (6, 4));
        let px = |x: usize, y: usize| tex.rgba[(y * 6 + x) * 4];
        assert_eq!(px(0, 0), 1, "top-left tile");
        assert_eq!(px(5, 0), 2, "top-right tile");
        assert_eq!(px(0, 3), 3, "bottom-left tile");
        assert_eq!(px(5, 3), 4, "bottom-right tile");
    }

    #[test]
    fn two_rows_of_equal_height_are_kept_apart() {
        // This is the case a split-on-height-change rule gets wrong: it
        // merges the rows and the last strip of the image comes out
        // scrambled.
        let rows = split_rows(vec![
            RawTile { width: 4, height: 2, rgba: vec![0; 32] },
            RawTile { width: 2, height: 2, rgba: vec![0; 16] },
            RawTile { width: 4, height: 2, rgba: vec![0; 32] },
            RawTile { width: 2, height: 2, rgba: vec![0; 16] },
        ]);
        assert_eq!(rows.len(), 2, "equal heights must still be two rows");
        assert_eq!(rows[0].len(), 2);
    }

    #[test]
    fn a_change_of_height_also_opens_a_row() {
        let rows = split_rows(vec![
            RawTile { width: 4, height: 4, rgba: vec![0; 64] },
            RawTile { width: 2, height: 4, rgba: vec![0; 32] },
            RawTile { width: 4, height: 1, rgba: vec![0; 16] },
            RawTile { width: 2, height: 1, rgba: vec![0; 8] },
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][0].height, 1);
    }

    #[test]
    fn a_ragged_grid_is_rejected_rather_than_stitched_wrong() {
        let mut data = named("x/bad.tga");
        let at = data.len();
        data.extend_from_slice(&chained(&[
            tile_blob(4, 2, [1, 0, 0, 255]),
            tile_blob(2, 2, [2, 0, 0, 255]),
            tile_blob(4, 2, [3, 0, 0, 255]),
        ]));
        assert_eq!(read_texture(&data, at), Err(ResourceError::RaggedGrid));
    }

    #[test]
    fn a_resource_with_no_tiles_is_reported() {
        let data = named("x/empty.tga");
        assert_eq!(read_texture(&data, data.len()), Err(ResourceError::NoTiles));
    }

    #[test]
    fn cropping_removes_power_of_two_padding() {
        let tex = Texture {
            width: 4,
            height: 4,
            rgba: (0..64).map(|i| i as u8).collect(),
        };
        let cropped = tex.cropped(2, 2);
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(cropped.rgba.len(), 2 * 2 * 4);
        assert_eq!(&cropped.rgba[..4], &tex.rgba[..4]);
        // Second row of the crop starts at the second row of the source.
        assert_eq!(&cropped.rgba[8..12], &tex.rgba[16..20]);
    }

    #[test]
    fn cropping_larger_than_the_texture_is_clamped() {
        let tex = Texture {
            width: 2,
            height: 2,
            rgba: vec![7u8; 16],
        };
        let cropped = tex.cropped(99, 99);
        assert_eq!((cropped.width, cropped.height), (2, 2));
    }
}
