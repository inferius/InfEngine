//! W371 - underground cave-biome decoration (lush_caves and
//! dripstone_caves).
//!
//! MC picks its cave biomes with the 3D multinoise router: depth in
//! (0.2, 0.9), LUSH at the wettest humidity band and DRIPSTONE at
//! deep-inland continentalness (`OverworldBiomeBuilder
//! .addUndergroundBiomes`). The probe here reads the same climate
//! samplers (`Overworld::cave_kind_at`), then dresses the cave air
//! found inside this chunk slice:
//!
//! - dripstone: pointed stalagmites (floor) + stalactites (ceiling),
//!   1..3 cells of `pointed_dripstone`.
//! - lush: moss floors (stone family -> `moss_block`, plus carpets),
//!   `glow_lichen` + `spore_blossom` ceilings, the odd `azalea` bush
//!   on mossed ground.
//!
//! Chunk-local writes only, deterministic per chunk via the usual
//! `stream_seed` salt. `dripstone_block` landed in W374 (floors,
//! ceilings + large columns); `cave_vines` is still pending in the
//! parity matrix and upgrades the lush dressing once it lands.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::overworld::CaveKind;
use super::terrain::{mc_floor_at, overworld_for};
use crate::worldgen::WorldgenCtx;

const SALT_CAVE_DECO: u64 = 0xCADE_0371;

pub(crate) fn place_cave_deco(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    // Whole-chunk fast reject: everything at/above the surface band
    // cannot be a cave interior.
    let ow = overworld_for(ctx.effective_seed());
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        coord[0],
        coord[2],
        SALT_CAVE_DECO ^ (coord[1] as u64).wrapping_mul(0x9E37_79B9),
    ));
    let moss = block_id_by_name("moss_block");
    let carpet = block_id_by_name("moss_carpet");
    let lichen = block_id_by_name("glow_lichen");
    let blossom = block_id_by_name("spore_blossom");
    let azalea = block_id_by_name("azalea");
    let dripstone = block_id_by_name("pointed_dripstone");
    let drip_rock = block_id_by_name("dripstone_block");

    let stoneish = |id: u16| -> bool {
        crate::block::block_type(id).is_some_and(|bt| {
            matches!(bt.name, "stone" | "deepslate" | "tuff" | "andesite" | "diorite" | "granite")
        })
    };

    for lx in 0..cs {
        for lz in 0..cs {
            let wx = origin[0] + lx;
            let wz = origin[2] + lz;
            let surface = mc_floor_at(ctx, wx, wz);
            // One biome probe per column, sampled lazily at the
            // first cave candidate's height.
            let mut kind: Option<Option<CaveKind>> = None;
            for ly in 1..cs - 1 {
                let wy = origin[1] + ly;
                // Cave band: well under the surface, above bedrock.
                if wy >= surface - 8 || wy < -60 {
                    continue;
                }
                let here = chunk.get(lx as usize, ly as usize, lz as usize);
                if !here.is_air() {
                    continue;
                }
                let below = chunk.get(lx as usize, (ly - 1) as usize, lz as usize).0;
                let above = chunk.get(lx as usize, (ly + 1) as usize, lz as usize).0;
                let on_floor = stoneish(below);
                let under_ceiling = stoneish(above);
                if !on_floor && !under_ceiling {
                    continue;
                }
                let k = *kind.get_or_insert_with(|| ow.cave_kind_at(wx, wy, wz));
                let Some(k) = k else { break };
                let roll = (rng.next_u64() % 1000) as u32;
                match k {
                    CaveKind::Dripstone => {
                        // W374: dripstone_block underlay wherever a
                        // spike roots + bare rock patches + the odd
                        // large column - the caves read as dripstone
                        // rock, not spikes on plain stone
                        // (dripstone_cluster / large_dripstone).
                        if on_floor
                            && roll < 340
                            && let Some(rock) = drip_rock
                        {
                            chunk.set(
                                lx as usize,
                                (ly - 1) as usize,
                                lz as usize,
                                BlockId(rock),
                            );
                        }
                        if under_ceiling
                            && (600..820).contains(&roll)
                            && let Some(rock) = drip_rock
                        {
                            chunk.set(
                                lx as usize,
                                (ly + 1) as usize,
                                lz as usize,
                                BlockId(rock),
                            );
                        }
                        if on_floor
                            && (340..355).contains(&roll)
                            && let Some(rock) = drip_rock
                        {
                            // Large dripstone: a squat column, 3..7
                            // tall (large_dripstone, simplified to
                            // its silhouette).
                            let h = 3 + (rng.next_u64() % 5) as i32;
                            for i in 0..h {
                                let y = ly + i;
                                if y < cs
                                    && chunk
                                        .get(lx as usize, y as usize, lz as usize)
                                        .is_air()
                                {
                                    chunk.set(
                                        lx as usize,
                                        y as usize,
                                        lz as usize,
                                        BlockId(rock),
                                    );
                                }
                            }
                        }
                        if on_floor && roll < 90 {
                            // Stalagmite 1..3 up.
                            if let Some(id) = dripstone {
                                let h = 1 + (rng.next_u64() % 3) as i32;
                                for i in 0..h {
                                    let y = ly + i;
                                    if y < cs
                                        && chunk
                                            .get(lx as usize, y as usize, lz as usize)
                                            .is_air()
                                    {
                                        chunk.set(
                                            lx as usize,
                                            y as usize,
                                            lz as usize,
                                            BlockId(id),
                                        );
                                    }
                                }
                            }
                        } else if under_ceiling && (90..180).contains(&roll) {
                            // Stalactite 1..3 down.
                            if let Some(id) = dripstone {
                                let h = 1 + (rng.next_u64() % 3) as i32;
                                for i in 0..h {
                                    let y = ly - i;
                                    if y >= 0
                                        && chunk
                                            .get(lx as usize, y as usize, lz as usize)
                                            .is_air()
                                    {
                                        chunk.set(
                                            lx as usize,
                                            y as usize,
                                            lz as usize,
                                            BlockId(id),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    CaveKind::Lush => {
                        if on_floor {
                            // Moss the floor generously; dress some
                            // of it with carpets / an azalea bush.
                            if roll < 500
                                && let Some(id) = moss
                            {
                                chunk.set(
                                    lx as usize,
                                    (ly - 1) as usize,
                                    lz as usize,
                                    BlockId(id),
                                );
                                let dress = (rng.next_u64() % 100) as u32;
                                if dress < 12
                                    && let Some(c) = carpet
                                {
                                    chunk.set(
                                        lx as usize,
                                        ly as usize,
                                        lz as usize,
                                        BlockId(c),
                                    );
                                } else if dress < 16
                                    && let Some(a) = azalea
                                {
                                    chunk.set(
                                        lx as usize,
                                        ly as usize,
                                        lz as usize,
                                        BlockId(a),
                                    );
                                }
                            }
                        } else if under_ceiling {
                            if roll < 150 {
                                if let Some(id) = lichen {
                                    chunk.set(
                                        lx as usize,
                                        ly as usize,
                                        lz as usize,
                                        BlockId(id),
                                    );
                                }
                            } else if roll < 190
                                && let Some(id) = blossom
                            {
                                chunk.set(
                                    lx as usize,
                                    ly as usize,
                                    lz as usize,
                                    BlockId(id),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
