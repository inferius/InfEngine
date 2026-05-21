//! Placeholder atlas used when no MC resource pack is available.
//!
//! Mirrors the legacy procedural atlas built by `main::build_block_atlas`
//! but wrapped in a `ResourcePack` so all downstream code can read
//! from a single source of truth - the `ResourcePack` - regardless
//! of whether real MC textures loaded.

use std::collections::HashMap;
use std::path::PathBuf;

use super::{ResourcePack, TextureKey, TextureRef};

/// Placeholder atlas dimensions. 64x64 = 4 cols x 4 rows of 16x16
/// tiles, matching the legacy ATLAS_COLS / ATLAS_ROWS / TILE_PX.
const PLACEHOLDER_W: u32 = 64;
const PLACEHOLDER_H: u32 = 64;

/// Build a fully-formed placeholder pack with `is_fallback = true`.
/// The atlas is a 4x4 grid of solid-colour tiles drawn from
/// `BLOCK_TYPES`; the entries map every block name to its
/// `(col, row)` tile so the renderer can keep its existing atlas
/// UV math working unchanged.
pub(crate) fn placeholder_pack() -> ResourcePack {
    let (w, h, rgba) = build_placeholder_atlas();
    let mut textures: HashMap<TextureKey, TextureRef> = HashMap::new();
    let mut item_icons: HashMap<String, TextureRef> = HashMap::new();

    // Walk the bin's BlockType table + populate one TextureRef per
    // (col, row) tile. We register entries under BOTH our internal
    // name (`bt.name`) and the MC asset name from the name_map so
    // `icon_ref` / `block_face_ref` (which translate ids -> MC
    // names) hit the same tiles the mesher does.
    use crate::block::{BLOCK_TYPES, TileSpec};
    use super::name_map;
    let u_step = 1.0 / 4.0_f32;
    let v_step = 1.0 / 4.0_f32;
    let uv_of = |t: &TileSpec| -> [f32; 4] {
        let u0 = t.col as f32 * u_step;
        let v0 = t.row as f32 * v_step;
        [u0, v0, u0 + u_step, v0 + v_step]
    };
    for bt in BLOCK_TYPES {
        let top_ref = TextureRef::placeholder(uv_of(&bt.top));
        // Internal name.
        textures.insert(TextureKey::Block(bt.name.to_string()), top_ref);
        // MC name from the lookup table, when one exists. Otherwise
        // fall back to the internal name (the loader's path uses
        // `block_face_ref` which already chains internal -> MC).
        if let Some(map) = name_map::lookup_block(bt.id) {
            textures.insert(TextureKey::Block(map.block.to_string()), top_ref);
            for face in [map.top, map.bottom, map.sides] {
                if !face.is_empty() {
                    textures.insert(TextureKey::Block(face.to_string()), top_ref);
                }
            }
        }
        // Block-form item icons reuse the top face.
        item_icons.insert(bt.name.to_string(), top_ref);
        if let Some(item_name) = name_map::lookup_item(bt.id) {
            item_icons.insert(item_name.to_string(), top_ref);
        }
        // W37 fix: "torch_on" alias removed since W37 reverts torch
        // texture key to plain "torch" (modern MC filename).
        let _ = bt.id;
    }

    // W37: register the magenta "__missing" sentinel using the legacy
    // 4x4 grid's top-left UV. The placeholder pack is the
    // is_fallback=true variant + already has no texture detail, so
    // having __missing map to the first tile is acceptable. MC-atlas
    // textures.rs path paints + registers a true magenta-black checker.
    let missing_uv = [0.0, 0.0, u_step, v_step];
    textures.insert(
        TextureKey::Block("__missing".to_string()),
        TextureRef::placeholder(missing_uv),
    );

    ResourcePack {
        textures,
        item_icons,
        sounds: HashMap::new(),
        source: PathBuf::from("<placeholder>"),
        is_fallback: true,
        atlas_size: (w, h),
        atlas_rgba: rgba,
        // Wave 9.y: the placeholder pack has no animated tiles -
        // every entry is a static colour swatch. An empty
        // registry keeps the GPU uniform fully zeroed and the
        // shader's anim_id == 0 fast path holds.
        anim_registry: super::anim_registry::AnimRegistry::new(),
        anim_slots: HashMap::new(),
        // Wave 18.2: no particle atlas in the placeholder pack.
        particle_atlas: None,
    }
}

fn build_placeholder_atlas() -> (u32, u32, Vec<u8>) {
    use crate::block::{ATLAS_COLS, ATLAS_ROWS, BLOCK_TYPES, TILE_PX};
    let w = ATLAS_COLS * TILE_PX;
    let h = ATLAS_ROWS * TILE_PX;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let paint = |rgba: &mut Vec<u8>, col: u32, row: u32, color: [u8; 3]| {
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let px = col * TILE_PX + tx;
                let py = row * TILE_PX + ty;
                let idx = ((py * w + px) * 4) as usize;
                let n = ((tx.wrapping_mul(31) ^ ty.wrapping_mul(17)) % 64) as i32 - 32;
                let bias = (n as f32 / 32.0) * 24.0;
                rgba[idx] = (color[0] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = (color[1] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = (color[2] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = 255;
            }
        }
    };
    let f2u = |v: f32| (v * 255.0).clamp(0.0, 255.0) as u8;
    for bt in BLOCK_TYPES {
        let base = [f2u(bt.color[0]), f2u(bt.color[1]), f2u(bt.color[2])];
        for spec in [&bt.top, &bt.bottom, &bt.sides] {
            if seen.insert((spec.col, spec.row)) {
                paint(&mut rgba, spec.col, spec.row, base);
            }
        }
    }
    (w, h, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_pack_has_correct_atlas_size() {
        let p = placeholder_pack();
        assert_eq!(p.atlas_size, (PLACEHOLDER_W, PLACEHOLDER_H));
        assert_eq!(p.atlas_rgba.len(), (PLACEHOLDER_W * PLACEHOLDER_H * 4) as usize);
        assert!(p.is_fallback);
    }

    #[test]
    fn placeholder_pack_registers_every_block_name() {
        let p = placeholder_pack();
        // Sample a handful of well-known names from the registry.
        for name in ["stone", "dirt", "grass", "sand", "wood"] {
            assert!(
                p.block_texture(name).is_some(),
                "missing texture entry for {name}",
            );
        }
    }
}
