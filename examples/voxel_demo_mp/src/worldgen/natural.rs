//! W71 - "Natural" worldgen profile: MC-inspired fractal terrain.
//!
//! The Classic + Realistic profiles shape their height with a handful
//! of sin/cos terms, which reads as smooth, visibly periodic rolling
//! hills. Natural swaps that for multi-octave value noise (fBm) -
//! broad continentalness + biome-scaled hills + fine detail + a ridged
//! term for mountain peaks - so the terrain looks like MC's irregular,
//! non-repeating landscape. Biome selection, surface blocks, caves,
//! ravines, ores, trees, flowers and dungeons all reuse the Realistic
//! machinery; only the height curve changes.
//!
//! MC mirror: `net/minecraft/world/level/levelgen` density functions
//! stack octave noise into continentalness / erosion / peaks-and-
//! valleys; this approximates that layering with plain value-noise fBm.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

use super::biome::Biome;
use super::biome_table::biome_from_climate;
use super::climate::{SEA_LEVEL, climate_at, near_surface_block, surface_block};
use super::ore::pick_stone_or_ore;
use super::{WorldgenCtx, caves, decor, dungeons, ravines, strata};

/// Integer-grid hash -> `[0, 1)`. Stable per `(ix, iz, seed)`.
fn hash2(ix: i32, iz: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x8DA6_B343)
        ^ (iz as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0x9E37_79B1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smooth value noise at `(x, z)` in `[-1, 1]`: bilinear over the unit
/// integer grid with a smoothstep fade on each axis.
fn value_noise2(x: f32, z: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let fx = x - ix as f32;
    let fz = z - iz as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sz = fz * fz * (3.0 - 2.0 * fz);
    let v00 = hash2(ix, iz, seed);
    let v10 = hash2(ix + 1, iz, seed);
    let v01 = hash2(ix, iz + 1, seed);
    let v11 = hash2(ix + 1, iz + 1, seed);
    let a = lerp(v00, v10, sx);
    let b = lerp(v01, v11, sx);
    lerp(a, b, sz) * 2.0 - 1.0
}

/// Fractal Brownian motion: `octaves` of value noise summed at
/// doubling frequency / halving amplitude, normalised to `[-1, 1]`.
fn fbm2(x: f32, z: f32, seed: u32, octaves: u32) -> f32 {
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for o in 0..octaves {
        let s = seed.wrapping_add(o.wrapping_mul(0x9E37));
        sum += value_noise2(x * freq, z * freq, s) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// MC-inspired terrain height at `(wx, wz)`. Layers broad
/// continentalness, biome-scaled hills, fine detail, and a ridged
/// term that sharpens mountain peaks.
pub(crate) fn natural_height(wx: f32, wz: f32, seed: u32, biome: Biome) -> i32 {
    let cont = fbm2(wx * 0.0035, wz * 0.0035, seed ^ 0x00C0_FFEE, 5);
    let hills = fbm2(wx * 0.013, wz * 0.013, seed ^ 0xB117_0000, 4);
    let detail = fbm2(wx * 0.05, wz * 0.05, seed ^ 0xD7A1_5EED, 3);
    // Amplitudes kept moderate: dramatic enough to read as varied
    // terrain, but not so jagged that the sky-light BFS (horizontal
    // light spill into deep valleys) tanks the frame budget. Swamp /
    // beach stay near sea level but >= 0 so they're walkable, not
    // permanent void / ocean.
    let (bias, amp, ridged) = match biome {
        // W112: mountain amp trimmed (18->14, ridge 8->6 below) so peaks
        // top out ~sea+44 (h~107). That keeps the tallest terrain inside
        // the ±3-chunk vertical streaming window from a sea-level valley
        // (no clipped peaks) now that the whole band sits at SEA_LEVEL=63.
        // Taller mountains return once the vertical load range grows.
        Biome::Mountain | Biome::SnowyMountain => (12.0, 14.0, true),
        Biome::Jungle => (5.0, 8.0, false),
        Biome::Forest | Biome::Taiga => (4.0, 6.0, false),
        Biome::Desert => (2.0, 4.0, false),
        Biome::Tundra => (2.0, 4.0, false),
        Biome::Plains | Biome::Savanna => (2.0, 3.0, false),
        Biome::Swamp => (0.0, 2.0, false),
        Biome::Beach => (0.0, 1.5, false),
    };
    // W110: continentalness now drives a smooth ocean->land curve so the
    // world has PROPER seas + coastlines, not just the rare shallow dip
    // the old linear `cont * 8` produced (the "no proper water" report).
    // Strongly-negative continentalness = a deep ocean basin; near zero =
    // coast / shallows; positive = inland that rises with the biome. MC
    // stacks a continentalness spline the same way.
    let base = cont * 9.0 + cont.signum() * cont * cont * 7.0;
    // Land mask: 0 over the deep basin, ramping to 1 on land, so the
    // biome's hills + bias don't lift the seabed into islands everywhere
    // (ocean floors stay flat like MC's abyssal plains). Ocean forms
    // where `base` (continentalness) is low enough.
    let land = ((base + 5.0) / 7.0).clamp(0.0, 1.0);
    let mut elev = base + (hills * amp + bias) * land + detail * 1.5;
    if ridged {
        // `1 - |hills|` folds the noise into ridge lines for peaks -
        // land only, so no ridged spikes rise out of the abyssal plain.
        elev += (1.0 - hills.abs()) * 6.0 * land;
    }
    let h = SEA_LEVEL as f32 + elev;
    // Guarantee dry, walkable ground around the origin so a fresh spawn
    // never lands in an ocean / sub-sea pit (the "spawned in / stuck in
    // the terrain" reports). Blend a ">= sea + 3" floor in at (0, 0),
    // fading to the raw height by ~200 blocks out so the wider world
    // keeps its natural lows and coastlines. `.max` only lifts lows -
    // hills and peaks pass through untouched.
    let d = (wx * wx + wz * wz).sqrt();
    let pull = (1.0 - d / 200.0).clamp(0.0, 1.0);
    lerp(h, h.max(SEA_LEVEL as f32 + 3.0), pull).round() as i32
}

/// Natural chunk generator: the Realistic biome / surface / carve /
/// decorate pipeline driven by the fBm height curve above.
pub(crate) fn gen_natural(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    // W73: precompute height + biome per (x,z) column ONCE. The fBm
    // height is expensive and the old per-cell path recomputed it 16x
    // per column (once for every y) - a pointless chunk-gen cost that
    // helped tank the frame rate while streaming. `.max(1)` also keeps
    // a solid block under every column so a low-continentalness area is
    // a shallow seabed, never a bottomless void to spawn into / fall
    // through (the "player fell through the world" report).
    let mut col: [[(i32, Biome); CHUNK_SIZE]; CHUNK_SIZE] =
        [[(0, Biome::Plains); CHUNK_SIZE]; CHUNK_SIZE];
    for (lx, row) in col.iter_mut().enumerate() {
        for (lz, cell) in row.iter_mut().enumerate() {
            let wx = origin.x + lx as f32;
            let wz = origin.z + lz as f32;
            let biome = biome_from_climate(climate_at(wx, wz, seed));
            *cell = (natural_height(wx, wz, seed, biome).max(1), biome);
        }
    }
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let wy = origin.y as i32 + y as i32;
        // W121: jagged bedrock floor (block 45) instead of the old flat
        // stone slab. Row 0 is always solid so a low-continentalness area
        // is a seabed, never a void the player falls through (W73); the
        // rows above jitter so the floor reads like MC's noisy bedrock.
        if let Some(b) = strata::bedrock_floor_block(wx as i32, wy, wz as i32, seed) {
            return b;
        }
        let (h, biome) = col[x][z];
        if wy > h {
            if wy <= SEA_LEVEL && h < SEA_LEVEL {
                return BlockId(36); // water fills below sea level
            }
            return BlockId::AIR;
        }
        if wy == h {
            surface_block(biome, h)
        } else if wy >= h - 3 {
            near_surface_block(biome)
        } else {
            // W121: ore + deepslate strata. `pick_stone_or_ore` ->
            // `strata::pick_block` lays the MC-1.18 ore roster by depth
            // band and swaps stone/ores to their deepslate variants in
            // the deep stratum.
            pick_stone_or_ore(wx, wy, wz, biome, seed)
        }
    });
    caves::carve(&mut chunk, coord, ctx);
    ravines::carve(&mut chunk, coord, ctx);
    decor::place_realistic(&mut chunk, coord, ctx);
    dungeons::place(&mut chunk, coord, ctx);
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_water_coverage() {
        // W110 verify: % of columns below sea level (= water surface) over a
        // 4000x4000 area beyond the spawn-pull radius. Run with
        // `cargo test dump_water_coverage -- --ignored --nocapture`.
        let mut water = 0u32;
        let mut total = 0u32;
        let mut deep = 0u32;
        let mut hmin = i32::MAX;
        let mut hmax = i32::MIN;
        let mut hsum = 0i64;
        for wx in (-2000..2000).step_by(11) {
            for wz in (-2000..2000).step_by(11) {
                let biome = biome_from_climate(climate_at(wx as f32, wz as f32, 0));
                let h = natural_height(wx as f32, wz as f32, 0, biome).max(1);
                total += 1;
                if h < SEA_LEVEL {
                    water += 1;
                }
                if h < SEA_LEVEL - 4 {
                    deep += 1;
                }
                hmin = hmin.min(h);
                hmax = hmax.max(h);
                hsum += h as i64;
            }
        }
        let pct = 100.0 * water as f32 / total as f32;
        let dpct = 100.0 * deep as f32 / total as f32;
        let hmean = hsum as f32 / total as f32;
        println!("W110 WATER COVERAGE: {water}/{total} = {pct:.1}% below sea (deep>4: {dpct:.1}%)");
        println!("W112 HEIGHT RANGE: min={hmin} max={hmax} mean={hmean:.1} (sea={SEA_LEVEL})");
    }

    /// W121 verify: sample a real generated chunk volume and print ore
    /// counts per Y band + assert (a) bedrock at the world floor, (b)
    /// deepslate only below the strata threshold (never plain stone-form
    /// ore there), (c) the full ore roster actually generates. Run with
    /// `cargo test dump_underground -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_underground() {
        use super::strata::{self, WORLD_FLOOR};
        use std::collections::BTreeMap;

        let ctx = WorldgenCtx {
            base_seed: 0,
            dim: String::new(),
            profile: crate::worldgen::WorldGen::Natural,
        };
        // Names for the ids we care about (stone-form + deepslate-form).
        let name = |id: u16| -> &'static str {
            match id {
                1 => "stone",
                96 => "deepslate",
                45 => "bedrock",
                13 => "coal",
                14 => "iron",
                117 => "gold",
                118 => "diamond",
                119 => "redstone",
                120 => "lapis",
                121 => "emerald",
                122 => "copper",
                123 => "ds_coal",
                124 => "ds_iron",
                125 => "ds_diamond",
                126 => "ds_gold",
                246 => "ds_redstone",
                247 => "ds_lapis",
                248 => "ds_emerald",
                249 => "ds_copper",
                _ => "other",
            }
        };
        let is_stone_ore = |id: u16| matches!(id, 13 | 14 | 117..=122);
        let is_deepslate_ore = |id: u16| matches!(id, 123..=126 | 246..=249);

        // Per-Y-band tallies: band = world-Y / 8.
        let mut band_counts: BTreeMap<i32, BTreeMap<u16, u64>> = BTreeMap::new();
        let mut total: BTreeMap<u16, u64> = BTreeMap::new();
        let mut bedrock_y0 = 0u64;
        let mut bedrock_y0_total = 0u64;
        let mut stone_ore_in_deep = 0u64;
        let mut deepslate_above_thresh = 0u64;

        // Sweep a grid of chunk columns (spread out so the rare ores show
        // up) across the full underground chunk-Y stack (0..4 -> y0..64).
        let cxs: [i32; 6] = [10, 40, -25, 70, -55, 120];
        let czs: [i32; 6] = [-15, 33, 60, -48, 90, 5];
        for &cx in &cxs {
            for &cz in &czs {
                for cy in 0..4 {
                    let chunk = gen_natural([cx, cy, cz], &ctx);
                    for lz in 0..CHUNK_SIZE {
                        for ly in 0..CHUNK_SIZE {
                            for lx in 0..CHUNK_SIZE {
                                let id = chunk.get(lx, ly, lz).0;
                                let wy = cy * CHUNK_SIZE as i32 + ly as i32;
                                // Bedrock-floor accounting.
                                if wy == WORLD_FLOOR {
                                    bedrock_y0_total += 1;
                                    if id == 45 {
                                        bedrock_y0 += 1;
                                    }
                                }
                                // Strata consistency: a stone-form ore must
                                // never appear in the deepslate zone, and
                                // deepslate bulk/ore must never appear above
                                // the threshold.
                                if strata::is_deepslate(wy) && is_stone_ore(id) {
                                    stone_ore_in_deep += 1;
                                }
                                if !strata::is_deepslate(wy)
                                    && (id == 96 || is_deepslate_ore(id))
                                    && wy > WORLD_FLOOR
                                {
                                    deepslate_above_thresh += 1;
                                }
                                if is_stone_ore(id) || is_deepslate_ore(id) {
                                    let band = wy / 8;
                                    *band_counts
                                        .entry(band)
                                        .or_default()
                                        .entry(id)
                                        .or_default() += 1;
                                    *total.entry(id).or_default() += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        println!("W121 UNDERGROUND DUMP (Natural, seed=0, {} chunk-columns):", cxs.len() * czs.len());
        println!("  deepslate threshold depth-fraction = {} (stone->deepslate)", strata::DEEPSLATE_DEPTH);
        println!("  bedrock at y={WORLD_FLOOR}: {bedrock_y0}/{bedrock_y0_total} voxels");
        println!("  ORE COUNTS PER Y-BAND (band = floor(y/8)):");
        for (band, counts) in &band_counts {
            let y0 = band * 8;
            let deep = strata::is_deepslate(y0);
            let tag = if deep { "DEEP " } else { "norm " };
            let mut parts: Vec<String> = counts
                .iter()
                .map(|(id, n)| format!("{}={n}", name(*id)))
                .collect();
            parts.sort();
            println!("    y{y0:>3}..{:<3} [{tag}] {}", y0 + 7, parts.join(" "));
        }
        println!("  TOTALS:");
        let mut tparts: Vec<(u16, u64)> = total.iter().map(|(k, v)| (*k, *v)).collect();
        tparts.sort();
        for (id, n) in &tparts {
            println!("    {:>12} = {n}", name(*id));
        }
        println!(
            "  CONSISTENCY: stone_ore_in_deep_zone={stone_ore_in_deep} (must be 0), \
             deepslate_above_threshold={deepslate_above_thresh} (must be 0)"
        );

        // Hard assertions (the dump is also a guard).
        assert_eq!(bedrock_y0, bedrock_y0_total, "y={WORLD_FLOOR} must be all bedrock");
        assert_eq!(stone_ore_in_deep, 0, "stone-form ore found in deepslate zone");
        assert_eq!(deepslate_above_thresh, 0, "deepslate found above the threshold");
        // Every ore TYPE must generate at least once across the sample,
        // counting stone-form + deepslate-form together (e.g. diamond is
        // almost always deepslate-form in MC, so the stone variant alone
        // can legitimately be absent in a finite sample).
        let pair_total = |stone: u16, deep: u16| {
            total.get(&stone).copied().unwrap_or(0) + total.get(&deep).copied().unwrap_or(0)
        };
        for (label, stone, deep) in [
            ("coal", 13u16, 123u16),
            ("iron", 14, 124),
            ("gold", 117, 126),
            ("diamond", 118, 125),
            ("redstone", 119, 246),
            ("lapis", 120, 247),
            ("copper", 122, 249),
        ] {
            assert!(
                pair_total(stone, deep) > 0,
                "ore '{label}' (stone {stone} / deepslate {deep}) never generated"
            );
        }
    }

    #[test]
    fn fbm_stays_in_unit_range() {
        for i in 0..1000 {
            let x = i as f32 * 1.7;
            let z = i as f32 * 0.9;
            let n = fbm2(x * 0.01, z * 0.01, 42, 5);
            assert!((-1.05..=1.05).contains(&n), "fbm out of range: {n}");
        }
    }

    #[test]
    fn natural_height_varies_and_is_deterministic() {
        let a = natural_height(100.0, 200.0, 7, Biome::Plains);
        let b = natural_height(100.0, 200.0, 7, Biome::Plains);
        assert_eq!(a, b, "deterministic");
        // Sweep a line; the surface must not be constant (proves the
        // noise actually shapes terrain rather than a flat plane).
        let mut seen = std::collections::HashSet::new();
        for wx in 0..300 {
            seen.insert(natural_height(wx as f32, 0.0, 7, Biome::Plains));
        }
        assert!(seen.len() > 4, "terrain too flat: {} distinct", seen.len());
    }

    #[test]
    fn mountains_outclimb_plains() {
        // Summed over a patch, the mountain biome sits higher than
        // plains under the same noise field.
        let mut mh = 0i64;
        let mut ph = 0i64;
        for i in 0..64 {
            let wx = (i * 7) as f32;
            let wz = (i * 13) as f32;
            mh += natural_height(wx, wz, 5, Biome::Mountain) as i64;
            ph += natural_height(wx, wz, 5, Biome::Plains) as i64;
        }
        assert!(mh > ph, "mountains {mh} should outclimb plains {ph}");
    }

    #[test]
    fn natural_origin_is_dry_land() {
        // W73: within the spawn-pull radius the surface must sit safely
        // above sea level for every biome, so a fresh spawn lands on dry
        // ground rather than in an ocean / sub-sea pit.
        for biome in [Biome::Plains, Biome::Swamp, Biome::Beach, Biome::Forest] {
            let h = natural_height(0.0, 0.0, 12_345, biome);
            assert!(
                h >= SEA_LEVEL + 3,
                "origin must be dry land, got {h} for {biome:?}"
            );
        }
    }

    #[test]
    fn natural_chunk_has_solid_floor_no_void() {
        // W73: every column's y=0 must be solid so a low-continentalness
        // area is a seabed, never a bottomless void the player falls
        // through on spawn.
        let ctx = WorldgenCtx {
            base_seed: 0,
            dim: String::new(),
            profile: crate::worldgen::WorldGen::Natural,
        };
        for cc in [[0, 0, 0], [5, 0, -3], [-7, 0, 9]] {
            let chunk = gen_natural(cc, &ctx);
            for x in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_ne!(
                        chunk.get(x, 0, z).0,
                        0,
                        "y=0 must be solid at column ({x}, {z})",
                    );
                }
            }
        }
    }
}
