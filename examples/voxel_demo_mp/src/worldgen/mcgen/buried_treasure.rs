//! W253 - buried treasure (`BuriedTreasurePieces.java` port).
//!
//! Placement (`structure_set/buried_treasures.json`): spacing 1 /
//! separation 0 - EVERY chunk is its own grid cell - gated by
//! `frequency 0.01` (`legacy_type_2`; own per-chunk stream, same
//! trade-off as the outpost's frequency roll) and `#is_beach` at the
//! chest column. `locate_offset [9, 0, 9]`: the chest column is the
//! chunk-local (9, 9).
//!
//! The dig ports `postProcess`: from the ocean-floor height walk DOWN
//! until the block BELOW is the hard-stone family (sandstone / stone
//! / andesite / granite / diorite); the chest replaces that cell and
//! every exposed neighbour packs shut - hanging faces (empty below,
//! not the up-face) take the hard below-state, the rest take the soft
//! cover (the current cell's block, sand when the cell is empty).
//! All reads replay `mc_painted_block_at`, the pre-decoration paint
//! oracle, so every vertical cube of the column derives the identical
//! dig without chunk reads. Deviation: MC reads the LIVE feature-time
//! world (earlier decorations included); ours reads the painted
//! terrain - on a beach column the two differ only if another feature
//! already touched the exact chest column.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::registry_id;
use super::features::stream_seed;
use super::noise::SplitMix64;
use super::terrain::{mc_floor_at, mc_full_biome_at, mc_painted_block_at};
use crate::worldgen::WorldgenCtx;

/// `structure_set/buried_treasures.json` `frequency`.
const FREQUENCY: f32 = 0.01;
/// Own stream for the legacy_type_2 roll.
const SALT_TREASURE_FREQ: u64 = 0xD4E6_0070;
/// `locate_offset` - the chest column within the chunk.
const OFFSET: i32 = 9;

fn next_f32(r: &mut SplitMix64) -> f32 {
    (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

/// One resolved dig: the chest cell + the packing writes around it.
#[derive(Debug, Clone)]
pub(crate) struct TreasureDig {
    pub(crate) chest: [i32; 3],
    pub(crate) packs: Vec<([i32; 3], u16)>,
}

fn is_beach(b: McBiome) -> bool {
    matches!(b, McBiome::Beach | McBiome::SnowyBeach)
}

/// The hard-stone family the dig stops on (`postProcess`'s
/// sandstone/stone/andesite/granite/diorite list).
fn is_hard(id: u16) -> bool {
    [
        registry_id("sandstone"),
        registry_id("stone"),
        registry_id("andesite"),
        registry_id("granite"),
        registry_id("diorite"),
    ]
    .iter()
    .flatten()
    .any(|h| *h == id)
}

/// Resolve the treasure dig of a chunk column, `None` when the
/// frequency roll, biome gate or dig fails. Pure in (seed, chunk).
pub(crate) fn dig_for_chunk(ctx: &WorldgenCtx, cx: i32, cz: i32) -> Option<TreasureDig> {
    let seed = ctx.effective_seed();
    let mut rng = SplitMix64::new(stream_seed(seed, cx, cz, SALT_TREASURE_FREQ));
    if next_f32(&mut rng) >= FREQUENCY {
        return None;
    }
    let cs = CHUNK_SIZE as i32;
    let wx = cx * cs + OFFSET;
    let wz = cz * cs + OFFSET;
    if !is_beach(mc_full_biome_at(ctx, wx, wz)) {
        return None;
    }
    let sand = registry_id("sand").unwrap_or(45);
    // getHeight(OCEAN_FLOOR_WG) = first free y above the seabed.
    let mut y = mc_floor_at(ctx, wx, wz) + 1;
    while y > 1 {
        let below = mc_painted_block_at(ctx, wx, y - 1, wz);
        if is_hard(below) {
            // Pack: the current cell's block is the soft cover (sand
            // when the cell is empty/liquid - our paint oracle folds
            // both into 0).
            let cur = mc_painted_block_at(ctx, wx, y, wz);
            let soft = if cur != 0 { cur } else { sand };
            let mut packs = Vec::new();
            const DIRS: [[i32; 3]; 6] = [
                [0, -1, 0],
                [0, 1, 0],
                [-1, 0, 0],
                [1, 0, 0],
                [0, 0, -1],
                [0, 0, 1],
            ];
            for d in DIRS {
                let n = [wx + d[0], y + d[1], wz + d[2]];
                if mc_painted_block_at(ctx, n[0], n[1], n[2]) != 0 {
                    continue;
                }
                let below_n = mc_painted_block_at(ctx, n[0], n[1] - 1, n[2]);
                let up_face = d == [0, 1, 0];
                if below_n == 0 && !up_face {
                    packs.push((n, below));
                } else {
                    packs.push((n, soft));
                }
            }
            return Some(TreasureDig { chest: [wx, y, wz], packs });
        }
        y -= 1;
    }
    None
}

/// The per-chunk buried-treasure pass (underground_structures). The
/// writes never leave the chest column's +-1 envelope (x/z 8..10), so
/// no neighbour replay is needed - only the vertical cubes share the
/// pure dig.
pub(crate) fn place_buried_treasure(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let Some(dig) = dig_for_chunk(ctx, coord[0], coord[2]) else {
        return;
    };
    let cs = CHUNK_SIZE as i32;
    let chest_id = registry_id("chest").unwrap_or(0);
    let mut put = |w: [i32; 3], id: u16| {
        let (lx, ly, lz) = (w[0] - origin[0], w[1] - origin[1], w[2] - origin[2]);
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
        }
    };
    for (pos, id) in &dig.packs {
        put(*pos, *id);
    }
    put(dig.chest, chest_id);
}

/// Deterministic membership for the chest-loot pick: is this world
/// cell THE buried chest of its chunk?
pub(crate) fn is_treasure_chest(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    dig_for_chunk(ctx, pos[0].div_euclid(cs), pos[2].div_euclid(cs))
        .is_some_and(|d| d.chest == pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_holds_one_percent() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 77,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let seed = ctx.effective_seed();
        let mut hits = 0usize;
        let total = 40_000;
        for i in 0..total {
            let mut rng = SplitMix64::new(stream_seed(
                seed,
                i * 3,
                -i * 5,
                SALT_TREASURE_FREQ,
            ));
            if next_f32(&mut rng) < FREQUENCY {
                hits += 1;
            }
        }
        let ratio = hits as f32 / total as f32;
        assert!((ratio - FREQUENCY).abs() < 0.005, "roll drifted: {ratio}");
    }

    /// Probe: buried treasures near a seed/centre + full-pipeline
    /// verify the first one (the chest must materialise + probe as
    /// the treasure chest).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_buried_treasures -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_buried_treasures() {
        let env_i32 = |name: &str, default: i32| -> i32 {
            std::env::var(name)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        let seed: u32 = std::env::var("INFENGINE_DUMP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3592732596);
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 96);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<TreasureDig> = Vec::new();
        for cx in (ccx - radius)..=(ccx + radius) {
            for cz in (ccz - radius)..=(ccz + radius) {
                if let Some(d) = dig_for_chunk(&ctx, cx, cz) {
                    println!(
                        "buried treasure in chunk ({cx}, {cz}): chest {:?}, {} packs",
                        d.chest,
                        d.packs.len()
                    );
                    found.push(d);
                }
            }
        }
        println!("total: {} treasures in +-{radius} chunks", found.len());
        if let Some(d) = found.first() {
            let coord = [
                d.chest[0].div_euclid(cs),
                d.chest[1].div_euclid(cs),
                d.chest[2].div_euclid(cs),
            ];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let got = chunk
                .get(
                    d.chest[0].rem_euclid(cs) as usize,
                    d.chest[1].rem_euclid(cs) as usize,
                    d.chest[2].rem_euclid(cs) as usize,
                )
                .0;
            println!("full-pipeline chest cell holds id {got}");
            assert_eq!(
                Some(got),
                registry_id("chest"),
                "the treasure chest must materialise"
            );
            assert!(is_treasure_chest(&ctx, d.chest), "membership must re-derive");
        }
    }
}
