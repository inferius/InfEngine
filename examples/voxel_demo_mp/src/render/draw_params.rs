//! W232: precomputed per-block draw parameters for the chunk draw
//! queue.
//!
//! The per-frame queue loop in `app_on_update` used to re-derive
//! colour / alpha / PBR / emissive through a registry lookup plus a
//! `light_emission_for_id` match for EVERY visible submesh EVERY
//! frame - tens of thousands of lookups at view distance 12 and one
//! of the two big `dq` costs on the F3 panel. Every one of those
//! values is a pure function of the block id, so build the table
//! once and index it.

use std::sync::OnceLock;

use infengine_voxel::BlockId;

pub(crate) struct DrawParams {
    /// Registry colour with the transparent-render alpha baked in
    /// (0.65 for `F_TRANSPARENT_RENDER` blocks, 1.0 otherwise).
    pub(crate) color: [f32; 4],
    pub(crate) metallic: f32,
    pub(crate) roughness: f32,
    /// W46.19 emissive boost: 0.85 + (emission/15) * 0.85 for
    /// light-emitting blocks (torch ~1.64, glowstone ~1.85), 0.0
    /// for everything else.
    pub(crate) emissive: f32,
}

/// Indexed by block id; `None` for ids without a `BLOCK_TYPES` row.
static TABLE: OnceLock<Vec<Option<DrawParams>>> = OnceLock::new();

// W240: `for_block` (the per-draw lookup) retired - the draw loop
// queues merged per-layer meshes with an identity instance and the
// shader resolves per-block params from the uploaded table below.
#[cfg(test)]
#[inline]
pub(crate) fn for_block(id: u16) -> Option<&'static DrawParams> {
    TABLE
        .get_or_init(build)
        .get(id as usize)
        .and_then(|p| p.as_ref())
}

/// W240: the same table shaped for the GPU block-params uniform
/// (index = block id; missing rows = identity white). Uploaded once
/// via `Renderer::set_block_params` so the merged chunk meshes can
/// resolve per-block colour / PBR / emissive in the shader.
pub(crate) fn block_param_entries() -> Vec<infengine_render::BlockParamEntry> {
    let table = TABLE.get_or_init(build);
    table
        .iter()
        .map(|row| match row {
            Some(p) => infengine_render::BlockParamEntry {
                color: p.color,
                material: [p.metallic, p.roughness, p.emissive, 0.0],
            },
            None => infengine_render::BlockParamEntry::default(),
        })
        .collect()
}

fn build() -> Vec<Option<DrawParams>> {
    let max_id = crate::block::BLOCK_TYPES
        .iter()
        .map(|bt| bt.id)
        .max()
        .unwrap_or(0) as usize;
    let mut table: Vec<Option<DrawParams>> = Vec::new();
    table.resize_with(max_id + 1, || None);
    for bt in crate::block::BLOCK_TYPES {
        let (metallic, roughness) = crate::block::block_pbr(bt.id);
        let alpha = if crate::block::block_is_transparent_render(BlockId(bt.id)) {
            0.65
        } else {
            1.0
        };
        let emission_level =
            crate::block::light_emission::light_emission_for_id(bt.id);
        let emissive = if emission_level > 0 {
            0.85 + (emission_level as f32 / 15.0) * 0.85
        } else {
            0.0
        };
        table[bt.id as usize] = Some(DrawParams {
            color: [bt.color[0], bt.color[1], bt.color[2], alpha],
            metallic,
            roughness,
            emissive,
        });
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table row matches what the old per-frame derivation
    /// produced for a handful of representative blocks.
    #[test]
    fn table_matches_registry_derivation() {
        for bt in crate::block::BLOCK_TYPES {
            let p = for_block(bt.id).expect("every BLOCK_TYPES row has params");
            let (metallic, roughness) = crate::block::block_pbr(bt.id);
            assert_eq!(p.metallic, metallic, "metallic for {}", bt.name);
            assert_eq!(p.roughness, roughness, "roughness for {}", bt.name);
            let want_alpha =
                if crate::block::block_is_transparent_render(BlockId(bt.id)) {
                    0.65
                } else {
                    1.0
                };
            assert_eq!(p.color[3], want_alpha, "alpha for {}", bt.name);
            let lvl = crate::block::light_emission::light_emission_for_id(bt.id);
            if lvl > 0 {
                assert!(p.emissive > 0.8, "emissive for {}", bt.name);
            } else {
                assert_eq!(p.emissive, 0.0, "emissive for {}", bt.name);
            }
        }
    }

    /// Unknown ids return None instead of panicking.
    #[test]
    fn unknown_id_is_none() {
        assert!(for_block(u16::MAX).is_none());
    }
}
