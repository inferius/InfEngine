//! PNG loader + atlas builder.
//!
//! Walks `assets/minecraft/textures/{block,item,particle}/` and packs
//! every 16x16 tile into a single runtime atlas. Animated tiles
//! (`.mcmeta` sibling describes the frame strip) get every frame
//! placed sequentially in the atlas so the renderer can advance
//! the V offset by `(1 / total_atlas_height) * tile_height` per
//! frame without uploading multiple textures.
//!
//! Packing is intentionally simple - a row-based shelf packer.
//! - Atlas dims start at 256x256 + grow in powers of two up to 2048.
//! - Tiles are placed left-to-right; row wraps when full; atlas grows
//!   when the last row would overflow.
//! - Particle textures live in the same atlas as block + item tiles
//!   (W9.2 keeps a single atlas to avoid binding-group churn; we can
//!   split into a particle_atlas in W9.5 if it becomes a bottleneck).

use std::collections::HashMap;
use std::path::Path;

use image::RgbaImage;

use super::mcmeta::McMetaAnim;
use super::{TextureKey, TextureRef};

/// Tile dimensions. Every MC block / item texture is 16x16 (animated
/// ones stack vertically); particle frames are usually 8x8 but we
/// upscale them to 16x16 so the packer only has to handle one tile
/// size.
const TILE_PX: u32 = 16;

/// Atlas growth ladder. The packer starts at the smallest size that
/// fits + doubles on overflow up to the cap.
const ATLAS_SIZES: [u32; 4] = [256, 512, 1024, 2048];

/// One entry the packer needs to materialise on the atlas.
struct PackEntry {
    key: TextureKey,
    /// The decoded source PNG, already converted to RGBA8.
    img: RgbaImage,
    /// `.mcmeta`-derived animation descriptor. `frames.len() = 1`
    /// for a static tile.
    anim: McMetaAnim,
    /// True when this image is an item icon (so the W9.3 dispatch
    /// can populate `item_icons` from a single pass).
    is_item: bool,
    /// True when this image is a particle frame strip (multiple
    /// frames stacked horizontally OR vertically - we treat both
    /// the same).
    is_particle: bool,
}

/// Output of the packer: the atlas pixels + every key -> `TextureRef`
/// resolved into the atlas UV space.
pub(crate) struct AtlasOutput {
    pub(crate) atlas_size: (u32, u32),
    pub(crate) atlas_rgba: Vec<u8>,
    pub(crate) textures: HashMap<TextureKey, TextureRef>,
    pub(crate) item_icons: HashMap<String, TextureRef>,
}

/// Scan the MC `assets/minecraft/textures/` tree at `root` + build
/// the atlas. Returns `None` when no PNGs were found (the caller
/// falls back to the placeholder pack).
pub(crate) fn load_atlas(root: &Path) -> Option<AtlasOutput> {
    let mut entries: Vec<PackEntry> = Vec::new();
    let tex_root = root.join("textures");
    scan_dir(&tex_root.join("block"), &mut entries, false, false);
    scan_dir(&tex_root.join("item"), &mut entries, true, false);
    scan_dir(&tex_root.join("particle"), &mut entries, false, true);
    if entries.is_empty() {
        return None;
    }
    tracing::info!(count = entries.len(), "scanned MC texture entries");
    apply_biome_tints(&tex_root, &mut entries);
    Some(pack(entries))
}

// ---------------------------------------------------------------
// W135: biome-colormap tinting.
// ---------------------------------------------------------------

/// W135: bake the MC biome tint into the tintable tiles at atlas
/// build time. Vanilla ships grass tops, every leaf variant, the
/// grass/fern plants etc. as GRAYSCALE textures and multiplies them
/// by a biome colour at render time (`tintindex` + the 256x256
/// `colormap/{grass,foliage}.png` keyed by temperature/downfall).
/// The engine never applied ANY tint, so the whole overworld read
/// washed-out gray ("u nas se to MC nepodoba"). The chunk mesh has
/// no per-vertex colour channel yet, so this bakes ONE faithful
/// climate - the plains colour MC samples for temp 0.8 / downfall
/// 0.4 - into the atlas; per-biome variation (desert olive, swamp
/// murk) is the follow-up that needs a real tint vertex attribute.
///
/// Colours come from the pack's own colormaps (sampled exactly like
/// `GrassColor.get` / `FoliageColor.get`); spruce/birch/lily-pad use
/// MC's hardcoded constants (`FoliageColor.java`). Tiles that are
/// not grayscale are left alone, so a pre-tinted pack (Bedrock-style
/// `_carried` textures) never gets tinted twice.
fn apply_biome_tints(tex_root: &Path, entries: &mut [PackEntry]) {
    let cm_dir = tex_root.join("colormap");
    let load_cm = |name: &str| -> Option<RgbaImage> {
        image::open(cm_dir.join(name)).ok().map(|i| i.to_rgba8())
    };
    // MC GrassColor/FoliageColor: x = (1-temp)*255, y = (1-(downfall
    // *temp))*255. Plains = (0.8, 0.4). Fallbacks are the vanilla
    // 26.1.2 colormap values at those coords.
    let sample = |img: Option<RgbaImage>, fallback: [u8; 3]| -> [u8; 3] {
        let Some(img) = img else { return fallback };
        let t: f32 = 0.8;
        let d: f32 = 0.4 * t;
        let x = (((1.0 - t) * 255.0) as u32).min(img.width() - 1);
        let y = (((1.0 - d) * 255.0) as u32).min(img.height() - 1);
        let p = img.get_pixel(x, y);
        [p[0], p[1], p[2]]
    };
    let grass = sample(load_cm("grass.png"), [145, 189, 89]);
    let foliage = sample(load_cm("foliage.png"), [119, 171, 47]);
    let mut tinted = 0usize;
    for e in entries.iter_mut() {
        let TextureKey::Block(stem) = &e.key else {
            continue;
        };
        let Some(tint) = tint_for_stem(stem, grass, foliage) else {
            continue;
        };
        if !is_grayscale(&e.img) {
            continue;
        }
        for p in e.img.pixels_mut() {
            p[0] = ((p[0] as u16 * tint[0] as u16) / 255) as u8;
            p[1] = ((p[1] as u16 * tint[1] as u16) / 255) as u8;
            p[2] = ((p[2] as u16 * tint[2] as u16) / 255) as u8;
        }
        tinted += 1;
    }
    tracing::info!(
        tinted,
        grass = ?grass,
        foliage = ?foliage,
        "W135 biome colormap tints baked",
    );
}

/// Which biome tint a texture stem takes. Mirrors the vanilla
/// blockstate `tintindex` roster for the blocks the engine ships:
/// grass-colormap surfaces, foliage-colormap leaves/vines, and the
/// `FoliageColor.java` hardcoded evergreen/birch/lily constants.
fn tint_for_stem(
    stem: &str,
    grass: [u8; 3],
    foliage: [u8; 3],
) -> Option<[u8; 3]> {
    const SPRUCE: [u8; 3] = [0x61, 0x99, 0x61];
    const BIRCH: [u8; 3] = [0x80, 0xA7, 0x55];
    const LILY: [u8; 3] = [0x20, 0x80, 0x30];
    match stem {
        "grass_block_top"
        | "grass_block_side_overlay"
        | "short_grass"
        | "tall_grass_top"
        | "tall_grass_bottom"
        | "fern"
        | "large_fern_top"
        | "large_fern_bottom"
        | "sugar_cane" => Some(grass),
        "oak_leaves"
        | "jungle_leaves"
        | "acacia_leaves"
        | "dark_oak_leaves"
        | "mangrove_leaves"
        | "vine" => Some(foliage),
        "spruce_leaves" => Some(SPRUCE),
        "birch_leaves" => Some(BIRCH),
        "lily_pad" => Some(LILY),
        _ => None,
    }
}

/// True when every sampled opaque pixel is (near-)gray. Guards the
/// tint against pre-coloured packs.
fn is_grayscale(img: &RgbaImage) -> bool {
    img.pixels()
        .filter(|p| p[3] > 8)
        .take(512)
        .all(|p| p[0].abs_diff(p[1]) < 16 && p[1].abs_diff(p[2]) < 16)
}

/// Recursively walk `dir` + push every PNG into `out`.
fn scan_dir(dir: &Path, out: &mut Vec<PackEntry>, is_item: bool, is_particle: bool) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // MC ships per-block subfolders for state variants
            // (e.g. acacia_log/, oak_planks/). Recurse one level.
            scan_dir(&path, out, is_item, is_particle);
            continue;
        }
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
                tracing::debug!(
                    error = %err,
                    path = %path.display(),
                    "PNG decode failed - skipping",
                );
                continue;
            }
        };
        // The PNG may be a single tile (16x16) OR a vertical strip
        // (16x(16*N)) for animated textures. The frame count is
        // height / 16; we pass it to the mcmeta parser so the
        // "missing frames field" branch can synthesize a full walk.
        let frame_count = (img.height() / TILE_PX).max(1);
        let anim = super::mcmeta::load_for(&path, frame_count);
        let key = if is_particle {
            TextureKey::Particle(stem.clone())
        } else if is_item {
            TextureKey::Item(stem.clone())
        } else {
            TextureKey::Block(stem.clone())
        };
        out.push(PackEntry {
            key,
            img,
            anim,
            is_item,
            is_particle,
        });
    }
}

/// Pack every entry into a fresh atlas. Picks the smallest atlas
/// size that fits + grows as needed. Returns the final atlas plus
/// the per-key UV table.
fn pack(mut entries: Vec<PackEntry>) -> AtlasOutput {
    // Sort by descending tile-count: animated strips first so their
    // big rows land before tiny static tiles fragment the layout.
    entries.sort_by(|a, b| {
        let af = (a.img.height() / TILE_PX).max(1);
        let bf = (b.img.height() / TILE_PX).max(1);
        bf.cmp(&af).then(a_key_lexi(&a.key).cmp(&a_key_lexi(&b.key)))
    });
    for &cap in &ATLAS_SIZES {
        if let Some(out) = try_pack_at(&entries, cap) {
            return out;
        }
    }
    // Cap reached: log + drop the overflow.
    tracing::warn!(
        cap = ATLAS_SIZES[ATLAS_SIZES.len() - 1],
        "MC atlas at maximum size; dropping overflow tiles",
    );
    try_pack_at(&entries, ATLAS_SIZES[ATLAS_SIZES.len() - 1])
        .or_else(|| try_pack_at(&[], ATLAS_SIZES[0]))
        .expect("empty pack always succeeds")
}

fn a_key_lexi(k: &TextureKey) -> String {
    match k {
        TextureKey::Block(s) => format!("0:{s}"),
        TextureKey::Item(s) => format!("1:{s}"),
        TextureKey::Particle(s) => format!("2:{s}"),
    }
}

/// One row-packed attempt at the given square atlas size. Returns
/// `None` when entries overflow the atlas; the caller then bumps to
/// the next size in `ATLAS_SIZES`.
fn try_pack_at(entries: &[PackEntry], size: u32) -> Option<AtlasOutput> {
    let w = size;
    let h = size;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut textures: HashMap<TextureKey, TextureRef> = HashMap::new();
    let mut item_icons: HashMap<String, TextureRef> = HashMap::new();

    // Shelf packer state. Each shelf has a fixed height matching the
    // tallest tile placed on it.
    let mut cursor_x: u32 = 0;
    let mut cursor_y: u32 = 0;
    let mut shelf_h: u32 = 0;
    let mut placed = 0usize;

    for e in entries {
        let tile_w = TILE_PX;
        let tile_h = e.img.height().max(TILE_PX);
        // Wrap onto a new shelf if this tile doesn't fit on the row.
        if cursor_x + tile_w > w {
            cursor_y += shelf_h;
            cursor_x = 0;
            shelf_h = 0;
        }
        // Overflow the atlas vertically -> bump atlas size.
        if cursor_y + tile_h > h {
            return None;
        }
        // Place the source pixels into the atlas - support source
        // sizes other than 16xN by nearest-resizing the width down
        // to 16. MC ships pure 16-wide textures, but some packs are
        // 32x or 64x; resizing keeps the atlas footprint bounded.
        let src = if e.img.width() == TILE_PX {
            e.img.clone()
        } else {
            let target_h = (e.img.height() * TILE_PX) / e.img.width().max(1);
            image::imageops::resize(
                &e.img,
                TILE_PX,
                target_h.max(TILE_PX),
                image::imageops::FilterType::Nearest,
            )
        };
        let real_h = src.height();
        for ty in 0..real_h {
            for tx in 0..tile_w {
                let px = cursor_x + tx;
                let py = cursor_y + ty;
                if px >= w || py >= h {
                    continue;
                }
                let idx = ((py * w + px) * 4) as usize;
                let p = src.get_pixel(tx, ty);
                rgba[idx] = p.0[0];
                rgba[idx + 1] = p.0[1];
                rgba[idx + 2] = p.0[2];
                rgba[idx + 3] = p.0[3];
            }
        }
        let uv = uv_of(cursor_x, cursor_y, tile_w, TILE_PX, w, h);
        let frames = (real_h / TILE_PX).max(1);
        let frametime_ms = if frames > 1 {
            // mcmeta supplies the per-frame time; for a strip
            // without a sidecar, fall back to ~125 ms / frame
            // (matches MC's water_still default of 2 ticks).
            if e.anim.frametime_ms == 0 {
                125
            } else {
                e.anim.frametime_ms
            }
        } else {
            0
        };
        let tr = TextureRef {
            atlas_uv: uv,
            frames,
            frametime_ms,
            interpolate: e.anim.interpolate,
        };
        textures.insert(e.key.clone(), tr);
        if e.is_item
            && let TextureKey::Item(name) = &e.key
        {
            item_icons.insert(name.clone(), tr);
        }
        cursor_x += tile_w;
        if tile_h > shelf_h {
            shelf_h = tile_h;
        }
        // Particles flag is accepted but does not affect packing
        // shape today; future W9.5 may split them into a separate
        // atlas. Reference the field so clippy stays quiet.
        let _ = e.is_particle;
        placed += 1;
    }

    // W37: paint a magenta-black "missing texture" sentinel tile in the
    // atlas + register it under the well-known key `__missing`. Callers
    // that fail to resolve a block / item name look this up and render
    // the obvious magenta checker instead of falling back to the
    // top-left atlas tile (which on a typical MC atlas is the stone
    // texture and was confusing the user - "vetsina ikon je
    // cobblestone"). Pattern matches the MC / Source-engine "missing
    // texture" standard.
    if cursor_x + TILE_PX > w {
        cursor_y += shelf_h;
        cursor_x = 0;
        shelf_h = 0;
    }
    if cursor_y + TILE_PX <= h {
        let missing_x = cursor_x;
        let missing_y = cursor_y;
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let px = missing_x + tx;
                let py = missing_y + ty;
                let idx = ((py * w + px) * 4) as usize;
                let cell = (tx / 8 + ty / 8) % 2;
                // Magenta / black 8x8 checkerboard.
                if cell == 0 {
                    rgba[idx] = 0xFF;
                    rgba[idx + 1] = 0x00;
                    rgba[idx + 2] = 0xFF;
                } else {
                    rgba[idx] = 0x00;
                    rgba[idx + 1] = 0x00;
                    rgba[idx + 2] = 0x00;
                }
                rgba[idx + 3] = 0xFF;
            }
        }
        let missing_uv = uv_of(missing_x, missing_y, TILE_PX, TILE_PX, w, h);
        textures.insert(
            TextureKey::Block("__missing".to_string()),
            TextureRef {
                atlas_uv: missing_uv,
                frames: 1,
                frametime_ms: 0,
                interpolate: false,
            },
        );
        // Bump cursor + shelf height for completeness (no further tiles
        // placed but keep state consistent).
        let _ = cursor_x;
        let _ = shelf_h;
    }

    tracing::info!(size = w, placed, "MC atlas packed");
    Some(AtlasOutput {
        atlas_size: (w, h),
        atlas_rgba: rgba,
        textures,
        item_icons,
    })
}

/// Compute the (u0, v0, u1, v1) rect for a tile in the atlas.
/// `tile_h_used` is the V extent of FRAME 0 only - animated strips
/// report the single-frame height even though the strip occupies a
/// taller region. The renderer scrolls V by `tile_h_used / atlas_h`
/// per frame to walk through the strip.
fn uv_of(x: u32, y: u32, w_tile: u32, h_tile_used: u32, w: u32, h: u32) -> [f32; 4] {
    let u0 = x as f32 / w as f32;
    let v0 = y as f32 / h as f32;
    let u1 = (x + w_tile) as f32 / w as f32;
    let v1 = (y + h_tile_used) as f32 / h as f32;
    [u0, v0, u1, v1]
}

/// Write `bytes` to `path`, creating parents as needed. Helper for
/// tests that stage synthetic MC layouts.
#[cfg(test)]
pub(crate) fn write_fixture(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

/// Encode a synthetic 16x(16*frames) RGBA PNG for tests. Each frame
/// is a solid colour drawn from a 3-byte palette index.
#[cfg(test)]
pub(crate) fn synth_png(frames: u32, palette: &[[u8; 3]]) -> Vec<u8> {
    let mut img = image::RgbaImage::new(TILE_PX, TILE_PX * frames);
    for f in 0..frames {
        let c = palette[f as usize % palette.len()];
        for y in 0..TILE_PX {
            for x in 0..TILE_PX {
                img.put_pixel(x, f * TILE_PX + y, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }
    let mut bytes: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn stage_minimal_pack(root: &Path) -> PathBuf {
        let mc = root.join("assets").join("minecraft");
        let blk = mc.join("textures").join("block");
        std::fs::create_dir_all(&blk).unwrap();
        std::fs::write(blk.join("stone.png"), synth_png(1, &[[100, 100, 100]])).unwrap();
        std::fs::write(blk.join("dirt.png"), synth_png(1, &[[120, 90, 60]])).unwrap();
        // Animated water: 4 frames stacked vertically + an mcmeta
        // sidecar pointing at frametime=2 ticks (= 100 ms).
        std::fs::write(
            blk.join("water_still.png"),
            synth_png(4, &[[20, 40, 200], [30, 60, 210], [40, 80, 220], [50, 100, 230]]),
        )
        .unwrap();
        std::fs::write(
            blk.join("water_still.png.mcmeta"),
            br#"{"animation":{"frametime":2,"frames":[0,1,2,3]}}"#,
        )
        .unwrap();
        let itm = mc.join("textures").join("item");
        std::fs::create_dir_all(&itm).unwrap();
        std::fs::write(itm.join("iron_ingot.png"), synth_png(1, &[[220, 220, 220]])).unwrap();
        std::fs::write(itm.join("bucket.png"), synth_png(1, &[[180, 180, 180]])).unwrap();
        mc
    }

    #[test]
    fn load_atlas_returns_some_with_textures() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_minimal_pack(tmp.path());
        let out = load_atlas(&mc).expect("expected an atlas");
        // 3 block + 2 item entries + W37 __missing sentinel = 6 total.
        assert_eq!(out.textures.len(), 6);
        assert!(out.textures.contains_key(&TextureKey::Block("stone".to_string())));
        assert!(out.textures.contains_key(&TextureKey::Item("iron_ingot".to_string())));
        assert!(out.item_icons.contains_key("iron_ingot"));
        assert!(out.textures.contains_key(&TextureKey::Block("__missing".to_string())));
    }

    #[test]
    fn load_atlas_picks_smallest_size_that_fits() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_minimal_pack(tmp.path());
        let out = load_atlas(&mc).unwrap();
        // 5 tiles + 1 animated strip is far below the 256x256 cap.
        assert_eq!(out.atlas_size, (256, 256));
    }

    #[test]
    fn w135_grayscale_grass_top_gets_colormap_tint() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_minimal_pack(tmp.path());
        let blk = mc.join("textures").join("block");
        // Grayscale grass top (the vanilla situation).
        std::fs::write(
            blk.join("grass_block_top.png"),
            synth_png(1, &[[150, 150, 150]]),
        )
        .unwrap();
        // Pre-coloured leaves must stay untouched (Bedrock-style pack).
        std::fs::write(
            blk.join("oak_leaves.png"),
            synth_png(1, &[[40, 160, 40]]),
        )
        .unwrap();
        // Solid-green colormap page -> the plains sample IS that green
        // (the sampler clamps into the 16x16 fixture).
        let cm = mc.join("textures").join("colormap");
        std::fs::create_dir_all(&cm).unwrap();
        std::fs::write(cm.join("grass.png"), synth_png(1, &[[100, 200, 50]]))
            .unwrap();
        let out = load_atlas(&mc).unwrap();
        let (w, _h) = out.atlas_size;
        let pixel_of = |key: &str| -> (u8, u8, u8) {
            let tr = out
                .textures
                .get(&TextureKey::Block(key.to_string()))
                .unwrap();
            let px = (tr.atlas_uv[0] * w as f32) as u32 + 2;
            let py = (tr.atlas_uv[1] * w as f32) as u32 + 2;
            let idx = ((py * w + px) * 4) as usize;
            (
                out.atlas_rgba[idx],
                out.atlas_rgba[idx + 1],
                out.atlas_rgba[idx + 2],
            )
        };
        // 150 * (100, 200, 50) / 255 = (58, 117, 29).
        let (r, g, b) = pixel_of("grass_block_top");
        assert!(
            (r as i32 - 58).abs() <= 2
                && (g as i32 - 117).abs() <= 2
                && (b as i32 - 29).abs() <= 2,
            "grass top should carry the colormap tint, got ({r},{g},{b})",
        );
        assert_eq!(
            pixel_of("oak_leaves"),
            (40, 160, 40),
            "pre-coloured tiles must not be re-tinted",
        );
    }

    #[test]
    fn animated_water_records_frames_and_frametime() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_minimal_pack(tmp.path());
        let out = load_atlas(&mc).unwrap();
        let water = out
            .textures
            .get(&TextureKey::Block("water_still".to_string()))
            .expect("water_still entry missing");
        assert_eq!(water.frames, 4);
        // frametime=2 ticks -> 100 ms.
        assert_eq!(water.frametime_ms, 100);
    }

    #[test]
    fn static_tile_records_one_frame_and_zero_frametime() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_minimal_pack(tmp.path());
        let out = load_atlas(&mc).unwrap();
        let stone = out
            .textures
            .get(&TextureKey::Block("stone".to_string()))
            .expect("stone entry missing");
        assert_eq!(stone.frames, 1);
        assert_eq!(stone.frametime_ms, 0);
    }

    #[test]
    fn item_icons_separate_from_block_table() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_minimal_pack(tmp.path());
        let out = load_atlas(&mc).unwrap();
        // "iron_ingot" lives in `item_icons` AND `textures[Item(...)]`,
        // but NOT in `textures[Block(...)]`.
        assert!(out.item_icons.contains_key("iron_ingot"));
        assert!(out.textures.contains_key(&TextureKey::Item("iron_ingot".to_string())));
        assert!(!out.textures.contains_key(&TextureKey::Block("iron_ingot".to_string())));
    }

    #[test]
    fn missing_textures_dir_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        // No `textures/` subtree at all.
        let mc = tmp.path().join("minecraft");
        std::fs::create_dir_all(&mc).unwrap();
        assert!(load_atlas(&mc).is_none());
    }

    #[test]
    fn uv_rect_round_trips_atlas_size() {
        // (16, 0, 16x16, 256) -> u0=1/16, v0=0, u1=2/16, v1=1/16.
        let uv = uv_of(16, 0, 16, 16, 256, 256);
        let eps = 1e-5;
        assert!((uv[0] - 0.0625).abs() < eps);
        assert!((uv[1] - 0.0).abs() < eps);
        assert!((uv[2] - 0.125).abs() < eps);
        assert!((uv[3] - 0.0625).abs() < eps);
    }

    #[test]
    fn many_tiles_grow_atlas_size() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join("assets").join("minecraft");
        let blk = mc.join("textures").join("block");
        std::fs::create_dir_all(&blk).unwrap();
        // 300 tiles * 16x16 = needs more than 256x256 (which fits
        // 16 * 16 = 256 tiles). Should land on 512.
        for i in 0..300u32 {
            std::fs::write(
                blk.join(format!("dummy_{i:03}.png")),
                synth_png(1, &[[i as u8, 0, 0]]),
            )
            .unwrap();
        }
        let out = load_atlas(&mc).unwrap();
        assert!(out.atlas_size.0 >= 512, "atlas should grow past 256");
        assert!(out.textures.len() >= 256, "should pack a lot of tiles");
    }
}
