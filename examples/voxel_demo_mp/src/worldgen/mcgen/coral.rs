//! W228 - warm-ocean coral, from the decompiled 26.1.2 sources:
//! `CoralFeature` (the shared placeCoralBlock with its top-coral /
//! sea-pickle / wall-fan decorations) and the three shapes -
//! `CoralClawFeature`, `CoralTreeFeature`, `CoralMushroomFeature` -
//! wired as MC's `warm_ocean_vegetation` placed feature
//! (noise_based_count 400/0/20, in_square, OCEAN_FLOOR_WG, biome
//! warm_ocean; the configured feature is a simple_random_selector over
//! the three shapes).
//!
//! Determinism: the shapes read the world ("is this cell water?") to
//! gate their growth loops, and MC's loops chain rng rolls on those
//! reads - a chunk-local read would diverge between the 3x3 replays.
//! So every placement decision runs against a VIRTUAL water model
//! instead: post-carve column solidity ([`column_is_solid`]) + the
//! feature-local written-cell sets (blocks vs decorations, standing in
//! for MC's `is(CORALS)` / `is(WATER)` checks against its own writes).
//! Real chunk writes still guard on the actual cell (`set_if`), so a
//! rare aquifer air pocket just leaves a cosmetic hole - decisions and
//! rng streams stay identical in every slice. Honest divergences:
//! wall fans map onto the plain fans (no facing states), sea pickles
//! collapse to the single engine pickle block, cross render for
//! plants/fans.

use std::collections::HashSet;
use std::sync::OnceLock;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::features::{block_id_by_name, stream_seed};
use super::noise::{NoiseParams, NormalNoise, SplitMix64};
use super::terrain::{column_is_solid, mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;
use crate::worldgen::climate::SEA_LEVEL;

const SALT_CORAL: u64 = 0xC02A_11F0;

/// The five coral colours' block/plant/fan ids, resolved once.
struct CoralIds {
    blocks: [u16; 5],
    plants: [u16; 5],
    fans: [u16; 5],
    pickle: u16,
}

fn coral_ids() -> &'static CoralIds {
    static IDS: OnceLock<CoralIds> = OnceLock::new();
    IDS.get_or_init(|| {
        let id = |n: &str| block_id_by_name(n).expect("coral block registered");
        CoralIds {
            blocks: [
                id("tube_coral_block"),
                id("brain_coral_block"),
                id("bubble_coral_block"),
                id("fire_coral_block"),
                id("horn_coral_block"),
            ],
            plants: [
                id("tube_coral"),
                id("brain_coral"),
                id("bubble_coral"),
                id("fire_coral"),
                id("horn_coral"),
            ],
            fans: [
                id("tube_coral_fan"),
                id("brain_coral_fan"),
                id("bubble_coral_fan"),
                id("fire_coral_fan"),
                id("horn_coral_fan"),
            ],
            pickle: id("sea_pickle"),
        }
    })
}

/// MC `Biome.BIOME_INFO_NOISE` stand-in (constant seed class 2345),
/// the same one the kelp/seagrass counts sample.
fn biome_info_noise() -> &'static NormalNoise {
    static NOISE: OnceLock<NormalNoise> = OnceLock::new();
    NOISE.get_or_init(|| {
        let params = NoiseParams { first_octave: 0, amplitudes: vec![1.0] };
        NormalNoise::new(2345, &params)
    })
}

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// Everything one coral run threads around: the target chunk, the
/// virtual-water context and the feature-local written sets.
struct CoralCtx<'a> {
    chunk: &'a mut Chunk,
    origin: [i32; 3],
    ctx: &'a WorldgenCtx,
    /// Solid coral blocks this pass decided to place.
    blocks: HashSet<[i32; 3]>,
    /// Plant/fan/pickle decorations this pass decided to place.
    deco: HashSet<[i32; 3]>,
}

impl CoralCtx<'_> {
    /// The virtual "is water" the DECISIONS use: sub-sea, post-carve
    /// open, and not something this pass already wrote.
    fn is_water(&self, p: [i32; 3]) -> bool {
        p[1] < SEA_LEVEL
            && p[1] >= 0
            && !column_is_solid(self.ctx, p[0], p[1], p[2])
            && !self.blocks.contains(&p)
            && !self.deco.contains(&p)
    }

    /// MC `is(CORALS)`: a plant/fan this pass wrote (a coral block may
    /// replace a decoration, never another coral block).
    fn is_coral_deco(&self, p: [i32; 3]) -> bool {
        self.deco.contains(&p)
    }

    /// Real chunk write, guarded on the actual cell content: water is
    /// always writable; decorations may be overwritten by blocks.
    fn set(&mut self, p: [i32; 3], id: u16, over_deco: bool) {
        let cs = CHUNK_SIZE as i32;
        let (lx, ly, lz) = (p[0] - self.origin[0], p[1] - self.origin[1], p[2] - self.origin[2]);
        if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
            return;
        }
        let cur = self.chunk.get(lx as usize, ly as usize, lz as usize).0;
        let ids = coral_ids();
        let cur_is_deco =
            ids.plants.contains(&cur) || ids.fans.contains(&cur) || cur == ids.pickle;
        if cur == 36 || (over_deco && cur_is_deco) {
            self.chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
        }
    }
}

/// `CoralFeature.placeCoralBlock`: place one coral block if the target
/// is (virtual) water-or-coral-decoration with water above, then roll
/// the top decoration (25% random coral plant/fan, else 5% sea pickle)
/// and the four 20% side wall-fans. Roll ORDER and gating match the
/// source exactly (all gates are virtual-model reads, so every replay
/// consumes the same stream).
fn place_coral_block(cc: &mut CoralCtx<'_>, rng: &mut SplitMix64, pos: [i32; 3], block: u16) -> bool {
    let above = [pos[0], pos[1] + 1, pos[2]];
    let target_ok = cc.is_water(pos) || cc.is_coral_deco(pos);
    if !target_ok || !cc.is_water(above) {
        return false;
    }
    let ids = coral_ids();
    cc.set(pos, block, true);
    cc.blocks.insert(pos);
    cc.deco.remove(&pos);
    if rng.next_f64() < 0.25 {
        // Random element of the CORALS tag: 5 plants + 5 fans.
        let pick = next_int(rng, 10) as usize;
        let id = if pick < 5 { ids.plants[pick] } else { ids.fans[pick - 5] };
        cc.set(above, id, false);
        cc.deco.insert(above);
    } else if rng.next_f64() < 0.05 {
        // Sea pickle (the count state collapses to the one block).
        let _ = next_int(rng, 4);
        cc.set(above, ids.pickle, false);
        cc.deco.insert(above);
    }
    for dir in [[0i32, -1i32], [1, 0], [0, 1], [-1, 0]] {
        if rng.next_f64() < 0.2 {
            let side = [pos[0] + dir[0], pos[1], pos[2] + dir[1]];
            if cc.is_water(side) {
                // WALL_CORALS pick; the engine maps wall fans onto the
                // plain fans (no facing states).
                let pick = next_int(rng, 5) as usize;
                cc.set(side, ids.fans[pick], false);
                cc.deco.insert(side);
            }
        }
    }
    true
}

/// `CoralClawFeature.placeFeature`.
fn coral_claw(cc: &mut CoralCtx<'_>, rng: &mut SplitMix64, origin: [i32; 3], block: u16) {
    if !place_coral_block(cc, rng, origin, block) {
        return;
    }
    const DIRS: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
    let claw = next_int(rng, 4) as usize;
    let n_branches = next_int(rng, 2) + 2;
    // toShuffledList of [claw, clockwise, counter-clockwise].
    let mut candidates = [claw, (claw + 1) % 4, (claw + 3) % 4];
    for i in (1..3).rev() {
        let j = next_int(rng, i as i32 + 1) as usize;
        candidates.swap(i, j);
    }
    for &branch in candidates.iter().take(n_branches as usize) {
        let mut pos = origin;
        let sideway_len = next_int(rng, 2) + 1;
        pos[0] += DIRS[branch][0];
        pos[2] += DIRS[branch][1];
        // Segment direction: the claw axis for the claw branch; else
        // 50/50 branch-or-up after an up step.
        let (seg_dir, inway_len): (Option<usize>, i32) = if branch == claw {
            (Some(claw), next_int(rng, 3) + 2)
        } else {
            pos[1] += 1;
            let up = next_int(rng, 2) == 1;
            (if up { None } else { Some(branch) }, next_int(rng, 3) + 3)
        };
        let mut i = 0;
        while i < sideway_len && place_coral_block(cc, rng, pos, block) {
            match seg_dir {
                Some(d) => {
                    pos[0] += DIRS[d][0];
                    pos[2] += DIRS[d][1];
                }
                None => pos[1] += 1,
            }
            i += 1;
        }
        // Step back along the segment, then up, then walk the claw.
        match seg_dir {
            Some(d) => {
                pos[0] -= DIRS[d][0];
                pos[2] -= DIRS[d][1];
            }
            None => pos[1] -= 1,
        }
        pos[1] += 1;
        for _ in 0..inway_len {
            pos[0] += DIRS[claw][0];
            pos[2] += DIRS[claw][1];
            if !place_coral_block(cc, rng, pos, block) {
                break;
            }
            if rng.next_f64() < 0.25 {
                pos[1] += 1;
            }
        }
    }
}

/// `CoralTreeFeature.placeFeature`.
fn coral_tree(cc: &mut CoralCtx<'_>, rng: &mut SplitMix64, origin: [i32; 3], block: u16) {
    let mut pos = origin;
    let trunk_height = next_int(rng, 3) + 1;
    for _ in 0..trunk_height {
        if !place_coral_block(cc, rng, pos, block) {
            return;
        }
        pos[1] += 1;
    }
    let top = pos;
    let n_branches = next_int(rng, 3) + 2;
    // Plane.HORIZONTAL.shuffledCopy.
    const DIRS: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
    let mut order = [0usize, 1, 2, 3];
    for i in (1..4).rev() {
        let j = next_int(rng, i as i32 + 1) as usize;
        order.swap(i, j);
    }
    for &branch in order.iter().take(n_branches as usize) {
        let mut pos = top;
        pos[0] += DIRS[branch][0];
        pos[2] += DIRS[branch][1];
        let branch_height = next_int(rng, 5) + 2;
        let mut segment = 0;
        let mut j = 0;
        while j < branch_height && place_coral_block(cc, rng, pos, block) {
            segment += 1;
            pos[1] += 1;
            if j == 0 || (segment >= 2 && rng.next_f64() < 0.25) {
                pos[0] += DIRS[branch][0];
                pos[2] += DIRS[branch][1];
                segment = 0;
            }
            j += 1;
        }
    }
}

/// `CoralMushroomFeature.placeFeature`: a hollow box shell with 10%
/// holes, sunk 1-3 blocks, corners trimmed by the source's edge tests.
fn coral_mushroom(cc: &mut CoralCtx<'_>, rng: &mut SplitMix64, origin: [i32; 3], block: u16) {
    let height = next_int(rng, 3) + 3;
    let width = next_int(rng, 3) + 3;
    let length = next_int(rng, 3) + 3;
    let sink = next_int(rng, 3) + 1;
    for x in 0..=width {
        for y in 0..=height {
            for z in 0..=length {
                let pos = [origin[0] + x, origin[1] + y - sink, origin[2] + z];
                let x_edge = x == 0 || x == width;
                let y_edge = y == 0 || y == height;
                let z_edge = z == 0 || z == length;
                // The source's corner trims + hollow-shell test
                // ((!x||!y) && (!z||!y) && (!x||!z) && (x||y||z),
                // boolean-simplified); the 10% hole roll fires only
                // when the geometry passes, like the source's
                // short-circuit.
                let shell = (!y_edge || (!x_edge && !z_edge))
                    && (!x_edge || !z_edge)
                    && (x_edge || y_edge || z_edge);
                if shell && rng.next_f64() >= 0.1 {
                    place_coral_block(cc, rng, pos, block);
                }
            }
        }
    }
}

/// Per-chunk coral pass (`warm_ocean_vegetation`, vegetal step). 3x3
/// source replay - the shapes reach several blocks out of their source
/// chunk.
pub(crate) fn place_corals(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let mut cc = CoralCtx { chunk, origin, ctx, blocks: HashSet::new(), deco: HashSet::new() };
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            if mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8) != McBiome::WarmOcean {
                continue;
            }
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_CORAL));
            // noise_based_count: factor 400, offset 0, ratio 20.
            let n = biome_info_noise().value(
                (scx * cs) as f64 / 400.0,
                0.0,
                (scz * cs) as f64 / 400.0,
            );
            let count = (n * 20.0).ceil().max(0.0) as u32;
            for _ in 0..count {
                let rx = next_int(&mut rng, 16);
                let rz = next_int(&mut rng, 16);
                let wx = scx * cs + rx;
                let wz = scz * cs + rz;
                // simple_random_selector over the three shapes.
                let shape = next_int(&mut rng, 3);
                // CORAL_BLOCKS tag pick.
                let colour = next_int(&mut rng, 5) as usize;
                let block = coral_ids().blocks[colour];
                let floor = mc_floor_at(ctx, wx, wz);
                if floor + 1 >= SEA_LEVEL {
                    continue; // not submerged (the biome modifier's job)
                }
                let base = [wx, floor + 1, wz];
                match shape {
                    0 => coral_tree(&mut cc, &mut rng, base, block),
                    1 => coral_claw(&mut cc, &mut rng, base, block),
                    _ => coral_mushroom(&mut cc, &mut rng, base, block),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{WorldGen, generate_chunk_at_ctx};

    /// Evidence: warm-ocean chunks actually grow coral (blocks + at
    /// least some decoration), and the pass is deterministic.
    #[test]
    fn warm_ocean_chunks_grow_coral() {
        let c = WorldgenCtx { base_seed: 0, dim: String::new(), profile: WorldGen::Mc };
        let ids = coral_ids();
        let mut warm_chunks = 0u32;
        let mut blocks = 0u32;
        let mut deco = 0u32;
        for cx in -24..24 {
            for cz in -24..24 {
                let (wx, wz) = (cx * 16 + 8, cz * 16 + 8);
                if mc_full_biome_at(&c, wx, wz) != McBiome::WarmOcean {
                    continue;
                }
                let floor = mc_floor_at(&c, wx, wz);
                if floor + 4 >= SEA_LEVEL {
                    continue;
                }
                warm_chunks += 1;
                let cy = (floor + 2).div_euclid(16);
                let chunk = generate_chunk_at_ctx([cx, cy, cz], &c);
                for x in 0..16 {
                    for y in 0..16 {
                        for z in 0..16 {
                            let id = chunk.get(x, y, z).0;
                            if ids.blocks.contains(&id) {
                                blocks += 1;
                            } else if ids.plants.contains(&id) || ids.fans.contains(&id) {
                                deco += 1;
                            }
                        }
                    }
                }
                if blocks > 0 && deco > 0 {
                    // Determinism: the same chunk regenerates identically.
                    let again = generate_chunk_at_ctx([cx, cy, cz], &c);
                    for x in 0..16 {
                        for y in 0..16 {
                            for z in 0..16 {
                                assert_eq!(chunk.get(x, y, z), again.get(x, y, z));
                            }
                        }
                    }
                    return;
                }
                if warm_chunks > 220 {
                    break;
                }
            }
        }
        panic!("no coral found: {warm_chunks} warm-ocean chunks, blocks={blocks} deco={deco}");
    }
}
