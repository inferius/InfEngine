//! W240: merge per-block chunk submeshes into ONE submesh per render
//! layer, stamping each vertex's block id into the `anim_id` HIGH
//! bits (low 8 bits keep the animation slot). The shader resolves
//! per-block colour / PBR / emissive from a uniform table indexed by
//! that id, so a whole chunk draws as one command per layer instead
//! of one per block type - the measured VD-32 render wall (117-160 ms
//! at 67k chunks) was the draw-command count, ~3-6 per chunk.
//!
//! The returned `ChunkSubmesh.block` field carries the LAYER sentinel
//! (`infengine_render::CHUNK_LAYER_*` as u16), not a block id - the
//! delivery path routes it straight to `set_chunk_mesh_layer`.

use infengine_voxel::{BlockId, ChunkSubmesh};

/// Chunk render layer for a block id. Mirrors the pre-W240 delivery
/// rules exactly: redstone torches drew through the CUTOUT layer,
/// everything else OPAQUE (transparent-render blocks keep their 0.65
/// alpha via the block-params table, same as the old per-draw color).
fn layer_for_block(id: u16) -> u16 {
    if matches!(
        id,
        crate::REDSTONE_TORCH_ID | crate::REDSTONE_WALL_TORCH_ID
    ) {
        infengine_render::CHUNK_LAYER_CUTOUT as u16
    } else {
        infengine_render::CHUNK_LAYER_OPAQUE as u16
    }
}

/// Merge per-block submeshes into per-layer submeshes. Blocks that
/// render through dedicated world-pass meshes (torch, snow) are
/// dropped here (this used to be the delivery arms' skip filter).
pub(crate) fn merge_submeshes(subs: Vec<ChunkSubmesh>) -> Vec<ChunkSubmesh> {
    let mut out: Vec<ChunkSubmesh> = Vec::new();
    for sub in subs {
        if sub.vertices.is_empty() || sub.indices.is_empty() {
            continue;
        }
        if crate::block::block_skips_chunk_mesh(sub.block) {
            continue;
        }
        let layer = layer_for_block(sub.block.0);
        let slot = match out.iter_mut().find(|m| m.block.0 == layer) {
            Some(m) => m,
            None => {
                out.push(ChunkSubmesh {
                    block: BlockId(layer),
                    vertices: Vec::new(),
                    indices: Vec::new(),
                });
                out.last_mut().expect("just pushed")
            }
        };
        let base = slot.vertices.len() as u32;
        let block_bits = (sub.block.0 as u32) << 8;
        for mut v in sub.vertices {
            // Anim slots are <= 32, so the low byte is safe.
            v.anim_id = (v.anim_id & 0xFF) | block_bits;
            slot.vertices.push(v);
        }
        slot.indices.extend(sub.indices.into_iter().map(|i| base + i));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_render::MeshVertex;

    fn sub(block: u16, verts: usize) -> ChunkSubmesh {
        ChunkSubmesh {
            block: BlockId(block),
            vertices: (0..verts)
                .map(|i| MeshVertex::new([i as f32, 0.0, 0.0], [0.0, 1.0, 0.0]))
                .collect(),
            indices: (0..verts as u32).collect(),
        }
    }

    /// Distinct opaque blocks collapse into ONE layer submesh with
    /// block ids stamped into the anim_id high bits and indices
    /// rebased.
    #[test]
    fn opaque_blocks_merge_into_one_submesh() {
        let merged = merge_submeshes(vec![sub(1, 4), sub(2, 4)]);
        assert_eq!(merged.len(), 1);
        let m = &merged[0];
        assert_eq!(m.block.0, infengine_render::CHUNK_LAYER_OPAQUE as u16);
        assert_eq!(m.vertices.len(), 8);
        assert_eq!(m.vertices[0].anim_id >> 8, 1);
        assert_eq!(m.vertices[4].anim_id >> 8, 2);
        // Second sub's indices rebased past the first's vertices.
        assert_eq!(m.indices[4], 4);
    }

    /// Redstone torches split into the cutout layer.
    #[test]
    fn redstone_torch_goes_to_cutout_layer() {
        let merged =
            merge_submeshes(vec![sub(1, 4), sub(crate::REDSTONE_TORCH_ID, 4)]);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|m| m.block.0
            == infengine_render::CHUNK_LAYER_CUTOUT as u16));
    }

    /// Snow layers render via the dedicated decor pass - the merge
    /// drops them like the old delivery filter did (see
    /// block_skips_chunk_mesh for the full id list).
    #[test]
    fn skip_blocks_are_dropped() {
        // W317: snow (15) meshes into the chunk now - water (36)
        // stays a dedicated-pass block and pins the drop path.
        let merged = merge_submeshes(vec![sub(36, 4)]);
        assert!(merged.is_empty());
        let kept = merge_submeshes(vec![sub(15, 4)]);
        assert_eq!(kept.len(), 1, "snow submeshes merge in");
    }
}
