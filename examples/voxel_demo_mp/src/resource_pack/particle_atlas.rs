// Particle sprite atlas for the textured particle pass (Wave 18.2).
//
// Packs MC particle PNGs into a single atlas distinct from the block
// atlas so the ParticlePass can bind its own texture without
// disturbing the block-texture bind group already in the mesh3d pass.
//
// Atlas packing is intentionally simple (row-shelf, same algorithm as
// textures.rs) so this module has no new dependencies.
//
// Sprite metadata:
//   - smoke_0..7  : 8 individual 16x16 sprites.
//   - flame       : one 16x128 strip (8 frames stacked vertically).
//     Stored as a single wide entry with `frames = 8` so callers
//     can compute per-frame UV via `sprite_uv("flame", frame)`.
//   - lava        : single 16x16 sprite (1 frame).
//   - big_smoke_0..11 : 12 individual sprites (furnace plume).

use std::collections::HashMap;
use std::path::Path;

use image::RgbaImage;

/// UV rect in atlas texture coordinates: [u_min, v_min, u_max, v_max].
pub(crate) type UvRect = [f32; 4];

/// Metadata for one named sprite (or multi-frame strip).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpriteEntry {
    /// UV of frame 0 inside the atlas.
    pub(crate) uv: UvRect,
    /// Number of animation frames.  For sprites stored as a single
    /// PNG strip (flame.png) this is > 1 and the caller steps V
    /// downward by `(uv[3] - uv[1]) / frames` per frame.
    pub(crate) frames: u32,
}

impl SpriteEntry {
    /// UV rect for frame `frame_idx` (0-based).  Returns frame 0's
    /// UV when the frame index is out of range.
    pub(crate) fn frame_uv(&self, frame_idx: u32) -> UvRect {
        if self.frames <= 1 {
            return self.uv;
        }
        let total_v = self.uv[3] - self.uv[1];
        let frame_v = total_v / self.frames as f32;
        let v0 = self.uv[1] + frame_v * frame_idx.min(self.frames - 1) as f32;
        [self.uv[0], v0, self.uv[2], v0 + frame_v]
    }
}

/// Compiled particle atlas: pixels + sprite lookup table.
#[derive(Debug)]
pub(crate) struct ParticleAtlasData {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
    sprites: HashMap<String, SpriteEntry>,
}

impl ParticleAtlasData {
    /// Look up a sprite UV rect by name.  For multi-frame sprites
    /// (flame) the returned rect covers frame `frame_idx` only.
    /// Returns `None` for unknown names.
    pub(crate) fn sprite_uv(&self, name: &str, frame_idx: u32) -> Option<UvRect> {
        let entry = self.sprites.get(name)?;
        Some(entry.frame_uv(frame_idx))
    }

    /// How many animation frames the named sprite has.  Returns 0
    /// for unknown names, 1 for static sprites, >1 for strips.
    pub(crate) fn frame_count(&self, name: &str) -> u32 {
        self.sprites.get(name).map(|e| e.frames).unwrap_or(0)
    }

    /// All loaded sprite names.
    pub(crate) fn sprite_names(&self) -> impl Iterator<Item = &str> {
        self.sprites.keys().map(|s| s.as_str())
    }
}

// -------------------------------------------------------------------
// Builder
// -------------------------------------------------------------------

const TILE_PX: u32 = 16;
const ATLAS_SIZES: [u32; 4] = [64, 128, 256, 512];

/// Build the particle atlas from the MC particle directory under
/// `mc_assets_root/textures/particle/`.
///
/// Returns `None` when the directory does not exist or contains no
/// PNG files (caller falls back to the block atlas path for tinting).
pub(crate) fn build_particle_atlas(mc_assets_root: &Path) -> Option<ParticleAtlasData> {
    let particle_dir = mc_assets_root.join("textures").join("particle");
    let entries = collect_particle_pngs(&particle_dir)?;
    if entries.is_empty() {
        return None;
    }
    Some(pack_atlas(entries))
}

/// One decoded particle PNG ready for packing.
struct PackEntry {
    /// Sprite name (file stem, e.g. "smoke_0", "flame", "lava").
    name: String,
    /// Decoded source image converted to RGBA8.
    img: RgbaImage,
    /// Number of animation frames.  For flame.png this is derived
    /// from the image height / TILE_PX.
    frames: u32,
}

/// Walk `dir` and decode every `.png` into a `PackEntry`.
fn collect_particle_pngs(dir: &Path) -> Option<Vec<PackEntry>> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut entries: Vec<PackEntry> = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let img = match image::open(&path) {
            Ok(i) => i.to_rgba8(),
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "particle PNG decode failed");
                continue;
            }
        };
        let (pw, ph) = img.dimensions();
        // Determine frame count: if the image is taller than TILE_PX
        // we treat the extra height as stacked frames (vertical strip).
        let frames = if ph > TILE_PX && ph % TILE_PX == 0 {
            ph / TILE_PX
        } else {
            1
        };
        // Normalise width: upscale images narrower than TILE_PX to
        // TILE_PX so the packer only handles one tile width.
        let normalised = if pw < TILE_PX {
            image::imageops::resize(
                &img,
                TILE_PX,
                ph * TILE_PX / pw.max(1),
                image::imageops::FilterType::Nearest,
            )
        } else {
            img
        };
        entries.push(PackEntry {
            name: stem,
            img: normalised,
            frames,
        });
    }
    if entries.is_empty() {
        None
    } else {
        // Stable sort by name so atlas layout is deterministic.
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Some(entries)
    }
}

/// Pack `entries` into a power-of-two atlas using a row-shelf
/// algorithm.  Each entry occupies TILE_PX wide x (frames * TILE_PX)
/// tall pixels.
fn pack_atlas(entries: Vec<PackEntry>) -> ParticleAtlasData {
    // Pick the smallest atlas that fits all entries.
    let total_area: u32 = entries
        .iter()
        .map(|e| TILE_PX * (e.frames * TILE_PX))
        .sum();
    let mut atlas_size = ATLAS_SIZES[0];
    for &s in &ATLAS_SIZES {
        atlas_size = s;
        if s * s >= total_area * 2 {
            break;
        }
    }

    let mut atlas: Vec<u8> = vec![0u8; (atlas_size * atlas_size * 4) as usize];
    let mut sprites: HashMap<String, SpriteEntry> = HashMap::new();

    let mut cursor_x = 0u32;
    let mut cursor_y = 0u32;
    let mut row_h = 0u32;

    for entry in &entries {
        let tile_w = TILE_PX;
        let tile_h = entry.frames * TILE_PX;

        // Wrap to next row when the entry would exceed atlas width.
        if cursor_x + tile_w > atlas_size {
            cursor_x = 0;
            cursor_y += row_h;
            row_h = 0;
        }

        // Grow atlas if needed (doubles up to cap).
        while cursor_y + tile_h > atlas_size {
            atlas_size *= 2;
            if atlas_size > 2048 {
                atlas_size = 2048;
                tracing::warn!(
                    "particle atlas hit 2048 cap; some sprites may be clipped"
                );
                break;
            }
            let new_sz = (atlas_size * atlas_size * 4) as usize;
            let mut new_atlas = vec![0u8; new_sz];
            // Copy old pixels.
            let old_w = atlas_size / 2;
            let old_data = &atlas;
            for row in 0..old_w {
                let src_off = (row * old_w * 4) as usize;
                let dst_off = (row * atlas_size * 4) as usize;
                new_atlas[dst_off..dst_off + (old_w * 4) as usize]
                    .copy_from_slice(&old_data[src_off..src_off + (old_w * 4) as usize]);
            }
            atlas = new_atlas;
        }

        // Blit entry pixels into the atlas.
        let (src_w, src_h) = entry.img.dimensions();
        let blit_w = tile_w.min(src_w);
        let blit_h = tile_h.min(src_h);
        for row in 0..blit_h {
            let src_off = (row * src_w * 4) as usize;
            let dst_off = ((cursor_y + row) * atlas_size * 4 + cursor_x * 4) as usize;
            let pixel_bytes = (blit_w * 4) as usize;
            if dst_off + pixel_bytes <= atlas.len()
                && src_off + pixel_bytes <= entry.img.as_raw().len()
            {
                atlas[dst_off..dst_off + pixel_bytes]
                    .copy_from_slice(&entry.img.as_raw()[src_off..src_off + pixel_bytes]);
            }
        }

        // Compute UV coordinates.
        let u0 = cursor_x as f32 / atlas_size as f32;
        let v0 = cursor_y as f32 / atlas_size as f32;
        let u1 = (cursor_x + tile_w) as f32 / atlas_size as f32;
        let v1 = (cursor_y + tile_h) as f32 / atlas_size as f32;

        sprites.insert(
            entry.name.clone(),
            SpriteEntry {
                uv: [u0, v0, u1, v1],
                frames: entry.frames,
            },
        );

        cursor_x += tile_w;
        row_h = row_h.max(tile_h);
    }

    tracing::info!(
        sprites = sprites.len(),
        atlas_size,
        "particle atlas built"
    );

    ParticleAtlasData {
        width: atlas_size,
        height: atlas_size,
        rgba: atlas,
        sprites,
    }
}

// -------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a minimal `ParticleAtlasData` with synthetic sprites,
    /// bypassing disk I/O.  Used by integration tests in other modules.
    pub(crate) fn synth_atlas(sprites: &[(&str, u32)]) -> ParticleAtlasData {
        let mut map: HashMap<String, SpriteEntry> = HashMap::new();
        for (i, (name, frames)) in sprites.iter().enumerate() {
            // Spread entries across the UV space so rects are distinct.
            let step = 1.0 / sprites.len() as f32;
            let u0 = i as f32 * step;
            let u1 = u0 + step;
            map.insert(
                (*name).to_string(),
                SpriteEntry {
                    uv: [u0, 0.0, u1, 1.0 / *frames as f32],
                    frames: *frames,
                },
            );
        }
        ParticleAtlasData {
            width: 64,
            height: 64,
            rgba: vec![255u8; 64 * 64 * 4],
            sprites: map,
        }
    }

    #[test]
    fn sprite_uv_returns_none_for_unknown() {
        let atlas = synth_atlas(&[("smoke_0", 1), ("flame", 8)]);
        assert!(atlas.sprite_uv("unknown_sprite", 0).is_none());
        assert!(atlas.sprite_uv("big_smoke_99", 0).is_none());
    }

    #[test]
    fn sprite_uv_returns_some_for_known_static() {
        let atlas = synth_atlas(&[("lava", 1), ("smoke_0", 1)]);
        let uv = atlas.sprite_uv("lava", 0);
        assert!(uv.is_some(), "lava sprite not found");
        let uv = uv.unwrap();
        // UV rect must be non-degenerate.
        assert!(uv[2] > uv[0], "u_max <= u_min");
        assert!(uv[3] > uv[1], "v_max <= v_min");
    }

    #[test]
    fn particle_atlas_contains_smoke_0_through_7_in_synth() {
        let names: Vec<(&str, u32)> = (0..8)
            .map(|i| {
                let n: &'static str = match i {
                    0 => "smoke_0",
                    1 => "smoke_1",
                    2 => "smoke_2",
                    3 => "smoke_3",
                    4 => "smoke_4",
                    5 => "smoke_5",
                    6 => "smoke_6",
                    _ => "smoke_7",
                };
                (n, 1u32)
            })
            .collect();
        let atlas = synth_atlas(&names);
        for i in 0..8 {
            let name = format!("smoke_{i}");
            assert!(
                atlas.sprite_uv(&name, 0).is_some(),
                "smoke_{i} missing from atlas"
            );
        }
    }

    #[test]
    fn particle_atlas_flame_strip_frame_count() {
        // flame.png stored as 8-frame vertical strip.
        let atlas = synth_atlas(&[("flame", 8)]);
        assert_eq!(atlas.frame_count("flame"), 8);
    }

    #[test]
    fn particle_atlas_flame_uv_per_frame_is_distinct() {
        // Each flame frame must occupy a distinct V band.
        let atlas = synth_atlas(&[("flame", 8)]);
        let f0 = atlas.sprite_uv("flame", 0).unwrap();
        let f1 = atlas.sprite_uv("flame", 1).unwrap();
        let f7 = atlas.sprite_uv("flame", 7).unwrap();
        // Frames advance in V.
        assert!(f1[1] > f0[1], "frame 1 V not above frame 0");
        assert!(f7[1] > f1[1], "frame 7 V not above frame 1");
        // U range stays consistent.
        assert!((f0[0] - f1[0]).abs() < 1e-5, "U min drifted between frames");
        assert!((f0[2] - f1[2]).abs() < 1e-5, "U max drifted between frames");
    }

    #[test]
    fn particle_atlas_frame_out_of_range_clamps_to_last() {
        let atlas = synth_atlas(&[("flame", 4)]);
        let f3 = atlas.sprite_uv("flame", 3).unwrap();
        let f99 = atlas.sprite_uv("flame", 99).unwrap();
        // Should clamp to last frame (3), not panic or wrap.
        assert_eq!(f3, f99, "out-of-range frame did not clamp to last");
    }

    #[test]
    fn particle_atlas_static_sprite_same_uv_all_frames() {
        let atlas = synth_atlas(&[("lava", 1)]);
        let uv0 = atlas.sprite_uv("lava", 0).unwrap();
        let uv5 = atlas.sprite_uv("lava", 5).unwrap();
        // Static sprite: any frame index returns the same rect.
        assert_eq!(uv0, uv5, "static sprite UV changed with frame index");
    }

    #[test]
    fn sprite_names_iter_covers_all_entries() {
        let atlas = synth_atlas(&[("smoke_0", 1), ("flame", 8), ("lava", 1)]);
        let names: Vec<&str> = atlas.sprite_names().collect();
        assert!(names.contains(&"smoke_0"));
        assert!(names.contains(&"flame"));
        assert!(names.contains(&"lava"));
    }

    #[test]
    fn frame_count_returns_zero_for_missing_sprite() {
        let atlas = synth_atlas(&[("smoke_0", 1)]);
        assert_eq!(atlas.frame_count("nonexistent"), 0);
    }

    #[test]
    fn entry_frame_uv_v_span_per_frame_consistent() {
        // Each frame of a 4-frame strip should have the same V height.
        let entry = SpriteEntry {
            uv: [0.0, 0.0, 1.0, 0.5], // total strip height = 0.5
            frames: 4,
        };
        let expected_frame_v = 0.5 / 4.0; // 0.125
        for i in 0..4 {
            let uv = entry.frame_uv(i);
            let h = uv[3] - uv[1];
            assert!(
                (h - expected_frame_v).abs() < 1e-6,
                "frame {i} height {h} != expected {expected_frame_v}"
            );
        }
    }

    #[test]
    fn entry_frame_uv_frame0_starts_at_strip_top() {
        let entry = SpriteEntry {
            uv: [0.25, 0.10, 0.75, 0.30],
            frames: 2,
        };
        let f0 = entry.frame_uv(0);
        assert!(
            (f0[1] - 0.10).abs() < 1e-6,
            "frame 0 v0 should equal strip top"
        );
    }
}
