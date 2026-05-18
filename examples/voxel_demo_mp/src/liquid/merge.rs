//! Source-merge pass: a flow voxel with two same-liquid sources as
//! horizontal neighbours and a stable pocket (no air drain) promotes
//! to a source. This is MC's infinite-water trick, with our stricter
//! stability check on top so a pocket with a side drain doesn't
//! promote.

use std::collections::HashMap;

use infengine_voxel::World;

use crate::block::block_is_washable;

use super::Liquid;

/// Pure source-merge pass. Returns positions promoted to source this
/// tick (caller writes `level = 0` + broadcasts).
pub(crate) fn liquid_merge_step(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    liquid: &Liquid,
) -> Vec<[i32; 3]> {
    let liquid_id = liquid.block_id;
    let mut promoted: Vec<[i32; 3]> = Vec::new();
    let candidates: Vec<[i32; 3]> = level_map
        .iter()
        .filter(|(_, d)| **d > 0)
        .take(256)
        .map(|(v, _)| *v)
        .collect();
    for pos in &candidates {
        if world.get(*pos).0 != liquid_id {
            continue;
        }
        let horiz = [
            [pos[0] + 1, pos[1], pos[2]],
            [pos[0] - 1, pos[1], pos[2]],
            [pos[0], pos[1], pos[2] + 1],
            [pos[0], pos[1], pos[2] - 1],
        ];
        let mut sources = 0u8;
        let mut stable = true;
        for n in &horiz {
            let nb = world.get(*n);
            if nb.0 == liquid_id {
                let nd = level_map.get(n).copied().unwrap_or(0);
                if nd == 0 {
                    sources += 1;
                } else {
                    stable = false;
                }
            } else if nb.is_air() || block_is_washable(nb) {
                stable = false;
            }
        }
        if sources >= 2 && stable {
            promoted.push(*pos);
        }
    }
    promoted
}
