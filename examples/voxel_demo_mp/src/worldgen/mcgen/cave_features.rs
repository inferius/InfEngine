//! W223 - glow lichen (`MultifaceGrowthFeature`, simplified).
//!
//! MC grows face-hugging lichen quads on cave walls/ceilings and
//! spreads them with chance 0.5. The engine block (627, emission 7)
//! renders as a cross, so the port keeps the PLACEMENT semantics -
//! 104..157 tries per chunk, only >= 13 blocks under the surface, in
//! cave air adjacent to a stone-family wall or ceiling - and grows a
//! small cluster per hit. Honest divergence: cross render instead of
//! face quads, cluster growth instead of the per-face spread walk.

use std::collections::HashSet;
use std::sync::OnceLock;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::terrain::mc_floor_at;
use crate::worldgen::WorldgenCtx;

const SALT_LICHEN: u64 = 0x11C4_E200;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// `can_be_placed_on` from the configured JSON, resolved to ids.
fn lichen_hosts() -> &'static HashSet<u16> {
    static SET: OnceLock<HashSet<u16>> = OnceLock::new();
    SET.get_or_init(|| {
        ["stone", "andesite", "diorite", "granite", "calcite", "tuff", "deepslate"]
            .iter()
            .filter_map(|n| block_id_by_name(n))
            .collect()
    })
}

/// Per-chunk glow-lichen pass (underground_decoration step).
pub(crate) fn place_glow_lichen(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let Some(lichen) = block_id_by_name("glow_lichen") else {
        return;
    };
    let hosts = lichen_hosts();
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let mut rng = SplitMix64::new(stream_seed(
        seed,
        coord[0],
        coord[2],
        SALT_LICHEN ^ (coord[1] as u64) << 8,
    ));
    // count uniform 104..=157 per chunk COLUMN; this slice keeps its
    // share by y-filtering the full-range tries.
    let count = 104 + next_int(&mut rng, 54);
    for _ in 0..count {
        let lx = next_int(&mut rng, 16);
        let lz = next_int(&mut rng, 16);
        let wy = next_int(&mut rng, 256);
        let ly = wy - origin[1];
        if !(1..cs - 1).contains(&ly) || !(0..cs).contains(&lx) || !(0..cs).contains(&lz) {
            continue;
        }
        let wx = origin[0] + lx;
        let wz = origin[2] + lz;
        // surface_relative_threshold OCEAN_FLOOR -13: caves only.
        if wy > mc_floor_at(ctx, wx, wz) - 13 {
            continue;
        }
        let (ux, uy, uz) = (lx as usize, ly as usize, lz as usize);
        if chunk.get(ux, uy, uz).0 != 0 {
            continue;
        }
        // Wall or ceiling host (can_place_on_floor = false).
        let ceiling = hosts.contains(&chunk.get(ux, uy + 1, uz).0);
        let wall = [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)].into_iter().any(|(dx, dz)| {
            let (nx, nz) = (lx + dx, lz + dz);
            (0..cs).contains(&nx)
                && (0..cs).contains(&nz)
                && hosts.contains(&chunk.get(nx as usize, uy, nz as usize).0)
        });
        if !ceiling && !wall {
            continue;
        }
        chunk.set(ux, uy, uz, BlockId(lichen));
        // chance_of_spreading 0.5: grow up to 3 neighbours.
        for (dx, dy, dz) in [(1i32, 0i32, 0i32), (-1, 0, 0), (0, 0, 1)] {
            if rng.next_f64() >= 0.5 {
                continue;
            }
            let (nx, ny, nz) = (lx + dx, ly + dy, lz + dz);
            if (0..cs).contains(&nx)
                && (0..cs).contains(&ny)
                && (0..cs).contains(&nz)
                && chunk.get(nx as usize, ny as usize, nz as usize).0 == 0
            {
                let n_ceiling = ny + 1 < cs
                    && hosts.contains(&chunk.get(nx as usize, (ny + 1) as usize, nz as usize).0);
                if n_ceiling || wall {
                    chunk.set(nx as usize, ny as usize, nz as usize, BlockId(lichen));
                }
            }
        }
    }
}
