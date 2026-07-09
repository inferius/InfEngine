//! W214 - MC worm carvers, ported from the decompiled 26.1.2
//! `CaveWorldCarver` / `CanyonWorldCarver` / `WorldCarver`.
//!
//! Every overworld biome lists the same three carvers (`cave`,
//! `cave_extra_underground`, `canyon`); they run ON TOP of the
//! density-native caves (W213) and produce the classic winding
//! tunnels, occasional rooms and ravines. Algorithm per source chunk:
//! a probability gate, then random tunnel walks (cos/sin heading with
//! damped random pitch/yaw drift, sine-bulged radius, occasional
//! split into two perpendicular branches) carving ellipsoids whose
//! open cells the AQUIFER decides (air / water / pressure-sealed) -
//! exactly `WorldCarver.getCarveState` with density 0.
//!
//! Like MC's `applyCarvers`, every target chunk replays the source
//! chunks within +-8 chunks; the walks are deterministic per
//! (seed, source chunk, carver), so tunnels cross chunk borders
//! seamlessly. `canReach` + the ellipsoid chunk-bbox test prune the
//! work to walks that can touch the target chunk.
//!
//! Config constants are the three `configured_carver` JSONs' values
//! (embedded + pinned by a test, so a data refresh that changes them
//! fails loudly). Vertical anchors keep MC coordinates (identity map):
//! the cave carvers' `above_bottom 8` start band and the `lava_level`
//! (-56) sit below the engine's realized y>=0 band, so deep-started
//! tunnels contribute only their upper parts and carver lava never
//! appears - consistent with the W213 aquifer note. Divergences:
//! SplitMix64 streams (rules, not seed identity); no carving mask
//! (idempotent re-carves; air isn't replaceable); the grass-floor
//! repair is chunk-local (a carved grass cell at a vertical seam can
//! miss repainting the dirt below).

use std::collections::HashSet;
use std::f32::consts::TAU;
use std::sync::OnceLock;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::aquifer::{Aquifer, Substance};
use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::overworld::Overworld;
use crate::worldgen::WorldgenCtx;

const SALT_CARVER: u64 = 0xCA57_0000;
/// `applyCarvers` source-chunk radius (dx/dz -8..=8).
const SOURCE_RANGE: i32 = 8;
/// `WorldCarver.getRange()` - the tunnel length base (range 4 ->
/// maxDistance (4*2-1)*16 = 112 blocks).
const TUNNEL_RANGE: i32 = 4;
/// MC world floor for anchor math + the canyon width-factor index.
const MIN_GEN_Y: i32 = -64;
/// MC gen depth (canyon width factors span the whole band).
const GEN_DEPTH: usize = 384;
/// Top clamp: MC protects the top 7 blocks of the gen band; the
/// engine band tops out at 255.
const TOP_PROTECT: i32 = 247;

// --- configured_carver values (pinned against the embedded JSONs) ---
const CAVE_PROBABILITY: f32 = 0.15;
const CAVE_EXTRA_PROBABILITY: f32 = 0.07;
const CANYON_PROBABILITY: f32 = 0.01;
/// cave y: uniform [above_bottom 8 .. absolute 180] (MC coords).
const CAVE_Y_MIN: i32 = MIN_GEN_Y + 8;
const CAVE_Y_MAX: i32 = 180;
/// cave_extra_underground y: uniform [above_bottom 8 .. absolute 47].
const CAVE_EXTRA_Y_MAX: i32 = 47;
/// canyon y: uniform [10 .. 67].
const CANYON_Y_MIN: i32 = 10;
const CANYON_Y_MAX: i32 = 67;

fn next_f32(r: &mut SplitMix64) -> f32 {
    r.next_f64() as f32
}

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// `Mth.randomBetween` / UniformFloat (max exclusive).
fn uniform_f(r: &mut SplitMix64, min: f32, max: f32) -> f32 {
    min + next_f32(r) * (max - min)
}

/// UniformInt (max INCLUSIVE).
fn uniform_i(r: &mut SplitMix64, min: i32, max: i32) -> i32 {
    min + next_int(r, max - min + 1)
}

/// TrapezoidFloat.sample (sum of two uniforms).
fn trapezoid_f(r: &mut SplitMix64, min: f32, max: f32, plateau: f32) -> f32 {
    let range = max - min;
    let a = (range - plateau) / 2.0;
    let b = range - a;
    min + next_f32(r) * b + next_f32(r) * a
}

/// The `overworld_carver_replaceables` tag as engine block ids
/// (base stone family, substrate, sands, gravel, snow, water, the
/// iron/copper ores). Names the engine doesn't have yet just drop out.
fn carvable(id: u16) -> bool {
    static SET: OnceLock<HashSet<u16>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            "stone", "granite", "diorite", "andesite", "tuff", "deepslate",
            "dirt", "coarse_dirt", "podzol", "rooted_dirt", "mycelium", "mud",
            "grass_block", "moss_block", "sand", "red_sand", "sandstone",
            "red_sandstone", "gravel", "snow_block", "packed_ice", "calcite",
            "water", "iron_ore", "copper_ore", "deepslate_iron_ore",
            "deepslate_copper_ore",
        ]
        .iter()
        .filter_map(|n| block_id_by_name(n))
        .collect()
    })
    .contains(&id)
}

/// Where a carve pass writes: the real chunk slice (world gen), or the
/// column solidity bitset (W226.1 dry-run - the post-carve heightmap
/// the feature stage reads; MC updates its heightmaps on every carver
/// `setBlock`, so features anchor to the carved floor, not the fill).
pub(crate) enum CarveSink<'a> {
    Chunk {
        chunk: &'a mut Chunk,
        /// Target chunk world origin.
        origin: [i32; 3],
    },
    Mask {
        solid: &'a mut [[[u64; 4]; CHUNK_SIZE]; CHUNK_SIZE],
        seed: u32,
    },
}

/// Everything one carve pass needs; threads through the tunnel
/// recursion instead of MC's 15-argument methods.
struct CarveCtx<'a> {
    ow: &'a Overworld,
    aquifer: &'a mut Aquifer,
    sink: CarveSink<'a>,
    /// Target chunk column coords.
    ccx: i32,
    ccz: i32,
}

impl CarveCtx<'_> {
    fn center_x(&self) -> f64 {
        (self.ccx * 16 + 8) as f64
    }
    fn center_z(&self) -> f64 {
        (self.ccz * 16 + 8) as f64
    }
}

/// `WorldCarver.canReach` - can the rest of this walk still touch the
/// target chunk?
fn can_reach(cc: &CarveCtx<'_>, x: f64, z: f64, step: i32, dist: i32, thickness: f32) -> bool {
    let xd = x - cc.center_x();
    let zd = z - cc.center_z();
    let remaining = (dist - step) as f64;
    let rr = thickness as f64 + 2.0 + 16.0;
    xd * xd + zd * zd - remaining * remaining <= rr * rr
}

/// `WorldCarver.carveEllipsoid`: carve the target-chunk cells inside
/// the ellipsoid at (x, y, z), skipping per `skip(xd, yd, zd, wy)`.
fn carve_ellipsoid(
    cc: &mut CarveCtx<'_>,
    x: f64,
    y: f64,
    z: f64,
    hr: f64,
    vr: f64,
    skip: &dyn Fn(f64, f64, f64, i32) -> bool,
) {
    if (x - cc.center_x()).abs() > 16.0 + hr * 2.0 || (z - cc.center_z()).abs() > 16.0 + hr * 2.0 {
        return;
    }
    let min_wy = ((y - vr).floor() as i32 - 1).max(MIN_GEN_Y + 1);
    let max_wy = ((y + vr).floor() as i32 + 1).min(TOP_PROTECT);
    // Chunk sink: outside this vertical chunk entirely - the vertical
    // neighbours' replays own those cells. (The mask sink spans the
    // whole column.)
    if let CarveSink::Chunk { origin, .. } = &cc.sink {
        let cs = CHUNK_SIZE as i32;
        if max_wy < origin[1] || min_wy > origin[1] + cs - 1 {
            return;
        }
    }
    let min_lx = (((x - hr).floor() as i32) - cc.ccx * 16 - 1).max(0);
    let max_lx = (((x + hr).floor() as i32) - cc.ccx * 16).min(15);
    let min_lz = (((z - hr).floor() as i32) - cc.ccz * 16 - 1).max(0);
    let max_lz = (((z + hr).floor() as i32) - cc.ccz * 16).min(15);
    for lx in min_lx..=max_lx {
        let wx = cc.ccx * 16 + lx;
        let xd = (wx as f64 + 0.5 - x) / hr;
        for lz in min_lz..=max_lz {
            let wz = cc.ccz * 16 + lz;
            let zd = (wz as f64 + 0.5 - z) / hr;
            if xd * xd + zd * zd >= 1.0 {
                continue;
            }
            let mut has_grass = false;
            for wy in (min_wy + 1..=max_wy).rev() {
                let yd = (wy as f64 - 0.5 - y) / vr;
                if skip(xd, yd, zd, wy) {
                    continue;
                }
                carve_block(cc, [lx, wy, lz], [wx, wz], &mut has_grass);
            }
        }
    }
}

/// `WorldCarver.carveBlock`: replaceable check, aquifer substance,
/// grass-floor repair (chunk sink) / solidity-bit clear (mask sink).
fn carve_block(cc: &mut CarveCtx<'_>, local: [i32; 3], world_xz: [i32; 2], has_grass: &mut bool) {
    let [lx, wy, lz] = local;
    let [wx, wz] = world_xz;
    match &mut cc.sink {
        CarveSink::Chunk { chunk, origin } => {
            let ly = wy - origin[1];
            if !(0..CHUNK_SIZE as i32).contains(&ly) {
                return;
            }
            let (ux, uy, uz) = (lx as usize, ly as usize, lz as usize);
            let cur = chunk.get(ux, uy, uz).0;
            if cur == 3 || block_id_by_name("mycelium") == Some(cur) {
                *has_grass = true;
            }
            if !carvable(cur) {
                return;
            }
            // getCarveState: lava_level (-56) is below the engine band;
            // the aquifer (with density 0) decides the rest. Solid =
            // the barrier pressure keeps the block.
            let new = match cc.aquifer.substance(cc.ow, wx, wy, wz, 0.0) {
                Substance::Solid => return,
                Substance::Air => BlockId::AIR,
                Substance::Water => BlockId(36),
                Substance::Lava => BlockId(37),
            };
            chunk.set(ux, uy, uz, new);
            // Grass repair: carving through a grass column repaints the
            // new dirt floor (MC's hasGrass + topMaterial). Dry only.
            if *has_grass && uy > 0 && new == BlockId::AIR && chunk.get(ux, uy - 1, uz).0 == 2 {
                chunk.set(ux, uy - 1, uz, BlockId(3));
            }
        }
        CarveSink::Mask { solid, seed } => {
            // Only solid fill cells matter for the heightmap; a carved
            // open/water cell stays non-solid. The fill's solid cells
            // are all carvable (stone family / ores / surface blocks)
            // except bedrock - honest edge: rare non-replaceables the
            // surface can emit (ice) and raw-ore vein cells stay solid
            // in the real chunk, so the mask floor may sit a block low
            // there (plants just fail their water check - no floaters).
            if !(0..256).contains(&wy) {
                return;
            }
            let (ux, uz) = (lx as usize, lz as usize);
            let (w, b) = ((wy >> 6) as usize, wy & 63);
            if (solid[ux][uz][w] >> b) & 1 == 0 {
                return;
            }
            if crate::worldgen::strata::bedrock_floor_block(wx, wy, wz, *seed).is_some() {
                return;
            }
            if matches!(cc.aquifer.substance(cc.ow, wx, wy, wz, 0.0), Substance::Solid) {
                return;
            }
            solid[ux][uz][w] &= !(1u64 << b);
        }
    }
}

/// `CaveWorldCarver.shouldSkip`.
fn cave_skip(xd: f64, yd: f64, zd: f64, floor: f64) -> bool {
    if yd <= floor {
        return true;
    }
    xd * xd + yd * yd + zd * zd >= 1.0
}

/// `CaveWorldCarver.getThickness`.
fn cave_thickness(r: &mut SplitMix64) -> f32 {
    let mut t = next_f32(r) * 2.0 + next_f32(r);
    if next_int(r, 10) == 0 {
        t *= next_f32(r) * next_f32(r) * 3.0 + 1.0;
    }
    t
}

/// One cave carver pass for source chunk `(scx, scz)` (the y range is
/// the only difference between `cave` and `cave_extra_underground`).
#[allow(clippy::too_many_arguments)]
fn cave_carve(cc: &mut CarveCtx<'_>, r: &mut SplitMix64, scx: i32, scz: i32, y_min: i32, y_max: i32) {
    let max_dist = (TUNNEL_RANGE * 2 - 1) * 16;
    // nextInt(nextInt(nextInt(15) + 1) + 1), evaluated inside-out.
    let inner = next_int(r, 15) + 1;
    let mid = next_int(r, inner) + 1;
    let cave_count = next_int(r, mid);
    for _ in 0..cave_count {
        let x = (scx * 16 + next_int(r, 16)) as f64;
        let y = uniform_i(r, y_min, y_max) as f64;
        let z = (scz * 16 + next_int(r, 16)) as f64;
        let h_mul = uniform_f(r, 0.7, 1.4) as f64;
        let v_mul = uniform_f(r, 0.8, 1.3) as f64;
        let floor = uniform_f(r, -1.0, -0.4) as f64;
        let mut tunnels = 1;
        if next_int(r, 4) == 0 {
            let y_scale = uniform_f(r, 0.1, 0.9) as f64;
            let thickness = 1.0 + next_f32(r) * 6.0;
            let hr = 1.5 + thickness as f64; // sin(pi/2) = 1
            carve_ellipsoid(cc, x + 1.0, y, z, hr, hr * y_scale, &|xd, yd, zd, _| {
                cave_skip(xd, yd, zd, floor)
            });
            tunnels += next_int(r, 4);
        }
        for _ in 0..tunnels {
            let h_rot = next_f32(r) * TAU;
            let v_rot = (next_f32(r) - 0.5) / 4.0;
            let thickness = cave_thickness(r);
            let dist = max_dist - next_int(r, max_dist / 4);
            let params = TunnelParams { h_mul, v_mul, thickness, floor, y_scale: 1.0 };
            cave_tunnel(cc, r.next_u64(), [x, y, z], &params, h_rot, v_rot, 0, dist);
        }
    }
}

/// The per-tunnel constants (MC threads them as 8 extra arguments).
struct TunnelParams {
    h_mul: f64,
    v_mul: f64,
    thickness: f32,
    floor: f64,
    y_scale: f64,
}

/// `CaveWorldCarver.createTunnel` - the winding walk, with the split
/// into two perpendicular branches at `splitPoint`.
#[allow(clippy::too_many_arguments)]
fn cave_tunnel(
    cc: &mut CarveCtx<'_>,
    tunnel_seed: u64,
    start: [f64; 3],
    p: &TunnelParams,
    mut h_rot: f32,
    mut v_rot: f32,
    step: i32,
    dist: i32,
) {
    let [mut x, mut y, mut z] = start;
    let mut r = SplitMix64::new(tunnel_seed);
    let split_point = next_int(&mut r, (dist / 2).max(1)) + dist / 4;
    let steep = next_int(&mut r, 6) == 0;
    let mut y_rota = 0.0f32;
    let mut x_rota = 0.0f32;
    for current in step..dist {
        let hr = 1.5 + (std::f32::consts::PI * current as f32 / dist as f32).sin() as f64 * p.thickness as f64;
        let vr = hr * p.y_scale;
        let cos_pitch = v_rot.cos();
        x += (h_rot.cos() * cos_pitch) as f64;
        y += v_rot.sin() as f64;
        z += (h_rot.sin() * cos_pitch) as f64;
        v_rot *= if steep { 0.92 } else { 0.7 };
        v_rot += x_rota * 0.1;
        h_rot += y_rota * 0.1;
        x_rota *= 0.9;
        y_rota *= 0.75;
        x_rota += (next_f32(&mut r) - next_f32(&mut r)) * next_f32(&mut r) * 2.0;
        y_rota += (next_f32(&mut r) - next_f32(&mut r)) * next_f32(&mut r) * 4.0;
        if current == split_point && p.thickness > 1.0 {
            let branch_a = TunnelParams {
                thickness: next_f32(&mut r) * 0.5 + 0.5,
                y_scale: 1.0,
                ..*p
            };
            let seed_a = r.next_u64();
            cave_tunnel(
                cc,
                seed_a,
                [x, y, z],
                &branch_a,
                h_rot - std::f32::consts::FRAC_PI_2,
                v_rot / 3.0,
                current,
                dist,
            );
            let branch_b = TunnelParams {
                thickness: next_f32(&mut r) * 0.5 + 0.5,
                y_scale: 1.0,
                ..*p
            };
            let seed_b = r.next_u64();
            cave_tunnel(
                cc,
                seed_b,
                [x, y, z],
                &branch_b,
                h_rot + std::f32::consts::FRAC_PI_2,
                v_rot / 3.0,
                current,
                dist,
            );
            return;
        }
        if next_int(&mut r, 4) != 0 {
            if !can_reach(cc, x, z, current, dist, p.thickness) {
                return;
            }
            let floor = p.floor;
            carve_ellipsoid(cc, x, y, z, hr * p.h_mul, vr * p.v_mul, &|xd, yd, zd, _| {
                cave_skip(xd, yd, zd, floor)
            });
        }
    }
}

/// `CanyonWorldCarver.carve` + `doCarve` (ravines).
fn canyon_carve(cc: &mut CarveCtx<'_>, r: &mut SplitMix64, scx: i32, scz: i32) {
    let max_dist = (TUNNEL_RANGE * 2 - 1) * 16;
    let mut x = (scx * 16 + next_int(r, 16)) as f64;
    let mut y = uniform_i(r, CANYON_Y_MIN, CANYON_Y_MAX) as f64;
    let mut z = (scz * 16 + next_int(r, 16)) as f64;
    let mut h_rot = next_f32(r) * TAU;
    let mut v_rot = uniform_f(r, -0.125, 0.125);
    let y_scale = 3.0f64; // yScale: constant 3.0 in the JSON
    let thickness = trapezoid_f(r, 0.0, 6.0, 2.0);
    let dist = (max_dist as f32 * uniform_f(r, 0.75, 1.0)) as i32;

    let mut tr = SplitMix64::new(r.next_u64());
    // initWidthFactors: per-world-Y squared width factor, re-rolled
    // every `width_smoothness`(3)-ish row.
    let mut width = [1.0f32; GEN_DEPTH];
    let mut factor = 1.0f32;
    for (i, w) in width.iter_mut().enumerate() {
        if i == 0 || next_int(&mut tr, 3) == 0 {
            factor = 1.0 + next_f32(&mut tr) * next_f32(&mut tr);
        }
        *w = factor * factor;
    }
    let mut y_rota = 0.0f32;
    let mut x_rota = 0.0f32;
    for current in 0..dist {
        let mut hr = 1.5 + (std::f32::consts::PI * current as f32 / dist as f32).sin() as f64 * thickness as f64;
        let mut vr = hr * y_scale;
        hr *= uniform_f(&mut tr, 0.75, 1.0) as f64;
        // updateVerticalRadius with center factor 0 / default 1.
        vr *= uniform_f(&mut tr, 0.75, 1.0) as f64;
        let cos_pitch = v_rot.cos();
        x += (h_rot.cos() * cos_pitch) as f64;
        y += v_rot.sin() as f64;
        z += (h_rot.sin() * cos_pitch) as f64;
        v_rot *= 0.7;
        v_rot += x_rota * 0.05;
        h_rot += y_rota * 0.05;
        x_rota *= 0.8;
        y_rota *= 0.5;
        x_rota += (next_f32(&mut tr) - next_f32(&mut tr)) * next_f32(&mut tr) * 2.0;
        y_rota += (next_f32(&mut tr) - next_f32(&mut tr)) * next_f32(&mut tr) * 4.0;
        if next_int(&mut tr, 4) != 0 {
            if !can_reach(cc, x, z, current, dist, thickness) {
                return;
            }
            carve_ellipsoid(cc, x, y, z, hr, vr, &|xd, yd, zd, wy| {
                let idx = ((wy - MIN_GEN_Y - 1).max(0) as usize).min(GEN_DEPTH - 1);
                (xd * xd + zd * zd) * width[idx] as f64 + yd * yd / 6.0 >= 1.0
            });
        }
    }
}

/// W214 entry: replay every source chunk within +-8 whose carvers can
/// touch this chunk. Runs after the W213 density fill (so it cuts
/// through the painted surface, with the grass repair - MC's own
/// post-surface carve semantics) and before decoration.
pub(crate) fn carve_mc(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    ow: &Overworld,
    aquifer: &mut Aquifer,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    let cc = CarveCtx {
        ow,
        aquifer,
        sink: CarveSink::Chunk { chunk, origin },
        ccx: coord[0],
        ccz: coord[2],
    };
    run_carvers(cc, seed);
}

/// W226.1 entry: dry-run the SAME carver walks (identical rng streams)
/// against the column's fill solidity bitset, clearing every bit the
/// real carve opens. `mc_floor_at` / `column_is_solid` read the result,
/// so the feature stage anchors to post-carve terrain like MC's live
/// heightmaps - the fix for seagrass floating over carver-cut seabeds.
pub(crate) fn carve_mask(
    solid: &mut [[[u64; 4]; CHUNK_SIZE]; CHUNK_SIZE],
    cx: i32,
    cz: i32,
    seed: u32,
    ow: &Overworld,
    aquifer: &mut Aquifer,
) {
    let cc = CarveCtx { ow, aquifer, sink: CarveSink::Mask { solid, seed }, ccx: cx, ccz: cz };
    run_carvers(cc, seed);
}

/// The shared +-8 source-chunk replay; both sinks consume identical
/// rng rolls (the walks never read what they carve).
fn run_carvers(mut cc: CarveCtx<'_>, seed: u32) {
    for scx in (cc.ccx - SOURCE_RANGE)..=(cc.ccx + SOURCE_RANGE) {
        for scz in (cc.ccz - SOURCE_RANGE)..=(cc.ccz + SOURCE_RANGE) {
            // Carver order matches the biome JSON list.
            let mut r0 = SplitMix64::new(stream_seed(seed, scx, scz, SALT_CARVER));
            if next_f32(&mut r0) <= CAVE_PROBABILITY {
                cave_carve(&mut cc, &mut r0, scx, scz, CAVE_Y_MIN, CAVE_Y_MAX);
            }
            let mut r1 = SplitMix64::new(stream_seed(seed, scx, scz, SALT_CARVER ^ (1 << 32)));
            if next_f32(&mut r1) <= CAVE_EXTRA_PROBABILITY {
                cave_carve(&mut cc, &mut r1, scx, scz, CAVE_Y_MIN, CAVE_EXTRA_Y_MAX);
            }
            let mut r2 = SplitMix64::new(stream_seed(seed, scx, scz, SALT_CARVER ^ (2 << 32)));
            if next_f32(&mut r2) <= CANYON_PROBABILITY {
                canyon_carve(&mut cc, &mut r2, scx, scz);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The code constants must match the embedded configured_carver
    /// JSONs - a data refresh that changes them fails here, not
    /// silently in a world.
    #[test]
    fn constants_match_the_embedded_jsons() {
        let cave: serde_json::Value =
            serde_json::from_str(include_str!("data/feature/carver/cave.json")).unwrap();
        let extra: serde_json::Value =
            serde_json::from_str(include_str!("data/feature/carver/cave_extra_underground.json"))
                .unwrap();
        let canyon: serde_json::Value =
            serde_json::from_str(include_str!("data/feature/carver/canyon.json")).unwrap();
        let f = |v: &serde_json::Value, k: &str| v["config"][k].as_f64().unwrap();
        // 1e-6: the constants are f32 (as in MC), the JSON reads f64.
        assert!((f(&cave, "probability") - CAVE_PROBABILITY as f64).abs() < 1e-6);
        assert!((f(&extra, "probability") - CAVE_EXTRA_PROBABILITY as f64).abs() < 1e-6);
        assert!((f(&canyon, "probability") - CANYON_PROBABILITY as f64).abs() < 1e-6);
        assert_eq!(
            cave["config"]["y"]["min_inclusive"]["above_bottom"].as_i64().unwrap(),
            (CAVE_Y_MIN - MIN_GEN_Y) as i64
        );
        assert_eq!(cave["config"]["y"]["max_inclusive"]["absolute"].as_i64().unwrap(), CAVE_Y_MAX as i64);
        assert_eq!(
            extra["config"]["y"]["max_inclusive"]["absolute"].as_i64().unwrap(),
            CAVE_EXTRA_Y_MAX as i64
        );
        assert_eq!(canyon["config"]["y"]["min_inclusive"]["absolute"].as_i64().unwrap(), CANYON_Y_MIN as i64);
        assert_eq!(canyon["config"]["y"]["max_inclusive"]["absolute"].as_i64().unwrap(), CANYON_Y_MAX as i64);
        assert!((canyon["config"]["yScale"].as_f64().unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(canyon["config"]["shape"]["width_smoothness"].as_i64().unwrap(), 3);
    }

    #[test]
    fn thickness_and_trapezoid_stay_in_range() {
        let mut r = SplitMix64::new(42);
        for _ in 0..2000 {
            let t = cave_thickness(&mut r);
            assert!((0.0..=12.0).contains(&t), "thickness {t} out of range");
            let tr = trapezoid_f(&mut r, 0.0, 6.0, 2.0);
            assert!((0.0..=6.0).contains(&tr), "trapezoid {tr} out of range");
        }
    }
}
