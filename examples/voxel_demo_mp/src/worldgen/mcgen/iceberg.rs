//! W226 - icebergs, ported from the decompiled 26.1.2 `IcebergFeature`.
//!
//! Two silhouettes: a rotated ELLIPSE (30%) or a random ROUND spire
//! (70%, occasionally +7..25 extra height); an over-water body, a
//! steeper under-water root (up to 18 deep), a 60%-height snow cap
//! (30% of bergs skip it), and usually a carved-out ellipse bite. The
//! packed-ice variant rolls 1/16 chunks, the blue-ice one 1/200, both
//! only in the frozen oceans - and the packed ice they leave behind is
//! exactly what the W223 blue-ice sprouts grow off.
//!
//! Honest divergence: MC's `smooth()` edge pass (removing lone
//! single-support blocks) is skipped - cosmetic, roll-free. Reads at
//! write time are chunk-local; every chunk replays the same rolls, so
//! the berg is seamless across borders.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::terrain::mc_full_biome_at;
use crate::worldgen::WorldgenCtx;
use crate::worldgen::climate::SEA_LEVEL;

const SALT_ICEBERG: u64 = 0x1CEB_E960;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_f32(r: &mut SplitMix64) -> f32 {
    r.next_f64() as f32
}

/// Everything one berg needs (threads through the helpers).
struct Berg<'a> {
    chunk: &'a mut Chunk,
    origin: [i32; 3],
    /// Sea-level anchor of the berg.
    center: [i32; 2],
    main: u16,
    snow_top: bool,
    is_ellipse: bool,
    ellipse_a: i32,
    ellipse_c: i32,
    angle: f64,
}

impl Berg<'_> {
    /// Replace only air / snow / ice / water (setIcebergBlock's guard).
    fn set(&mut self, wx: i32, wy: i32, wz: i32, id: u16) {
        let cs = CHUNK_SIZE as i32;
        let (lx, ly, lz) = (wx - self.origin[0], wy - self.origin[1], wz - self.origin[2]);
        if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
            return;
        }
        let here = self.chunk.get(lx as usize, ly as usize, lz as usize).0;
        let replaceable = here == 0
            || here == 36
            || crate::block::block_type(here)
                .is_some_and(|bt| matches!(bt.name, "snow_block" | "ice" | "packed_ice" | "blue_ice"));
        if replaceable {
            self.chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
        }
    }

    fn get(&self, wx: i32, wy: i32, wz: i32) -> Option<u16> {
        let cs = CHUNK_SIZE as i32;
        let (lx, ly, lz) = (wx - self.origin[0], wy - self.origin[1], wz - self.origin[2]);
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            Some(self.chunk.get(lx as usize, ly as usize, lz as usize).0)
        } else {
            None
        }
    }

    fn signed_dist_ellipse(&self, xo: i32, zo: i32, lo: [i32; 2], a: i32, c: i32) -> f64 {
        let (dx, dz) = ((xo - lo[0]) as f64, (zo - lo[1]) as f64);
        let (s, co) = self.angle.sin_cos();
        ((dx * co - dz * s) / a.max(1) as f64).powi(2)
            + ((dx * s + dz * co) / c.max(1) as f64).powi(2)
            - 1.0
    }

    fn ellipse_c_at(&self, yo: i32, height: i32) -> i32 {
        let mut c = self.ellipse_c;
        if yo > 0 && height - yo <= 3 {
            c -= 4 - (height - yo);
        }
        c
    }

    /// One shell cell (generateIcebergBlock).
    #[allow(clippy::too_many_arguments)]
    fn shell_cell(
        &mut self,
        rng: &mut SplitMix64,
        height: i32,
        xo: i32,
        yo: i32,
        zo: i32,
        radius: i32,
        a: i32,
    ) {
        let dist = if self.is_ellipse {
            self.signed_dist_ellipse(xo, zo, [0, 0], a, self.ellipse_c_at(yo, height))
        } else {
            let off = 10.0 * next_f32(rng).clamp(0.2, 0.8) as f64 / radius.max(1) as f64;
            off + (xo as f64).powi(2) + (zo as f64).powi(2) - (radius as f64).powi(2)
        };
        if dist >= 0.0 {
            return;
        }
        let compare = if self.is_ellipse { -0.5 } else { (-6 - next_int(rng, 3)) as f64 };
        if dist > compare && rng.next_f64() > 0.9 {
            return;
        }
        // setIcebergBlock: snow cap over the top 40% (when rolled).
        let (wx, wy, wz) = (self.center[0] + xo, SEA_LEVEL + yo, self.center[1] + zo);
        let h_diff = height - yo;
        let randomness = !self.is_ellipse || rng.next_f64() > 0.05;
        let divisor = if self.is_ellipse { 3 } else { 2 };
        let under_water = self.get(wx, wy, wz) == Some(36);
        if self.snow_top
            && !under_water
            && h_diff <= next_int(rng, (height / divisor).max(1)) + (height as f64 * 0.6) as i32
            && randomness
        {
            let snow = block_id_by_name("snow_block").unwrap_or(self.main);
            self.set(wx, wy, wz, snow);
        } else {
            self.set(wx, wy, wz, self.main);
        }
    }

    /// The carved bite (generateCutOut's carve).
    fn carve(&mut self, radius: i32, yo: i32, under_water: bool, angle: f64, lo: [i32; 2]) {
        let a = radius + 1 + self.ellipse_a / 3;
        let c = (radius - 3).min(3) + self.ellipse_c / 2 - 1;
        for xo in -a..a {
            for zo in -a..a {
                let (dx, dz) = ((xo - lo[0]) as f64, (zo - lo[1]) as f64);
                let (s, co) = angle.sin_cos();
                let dist = ((dx * co - dz * s) / a.max(1) as f64).powi(2)
                    + ((dx * s + dz * co) / c.max(1) as f64).powi(2)
                    - 1.0;
                if dist >= 0.0 {
                    continue;
                }
                let (wx, wy, wz) = (self.center[0] + xo, SEA_LEVEL + yo, self.center[1] + zo);
                let Some(here) = self.get(wx, wy, wz) else { continue };
                let bergy = crate::block::block_type(here).is_some_and(|bt| {
                    matches!(bt.name, "packed_ice" | "blue_ice" | "snow_block" | "ice")
                });
                if !bergy {
                    continue;
                }
                if under_water {
                    self.set_force(wx, wy, wz, 36);
                } else {
                    self.set_force(wx, wy, wz, 0);
                }
            }
        }
    }

    /// The carve writes unconditionally over berg material.
    fn set_force(&mut self, wx: i32, wy: i32, wz: i32, id: u16) {
        let cs = CHUNK_SIZE as i32;
        let (lx, ly, lz) = (wx - self.origin[0], wy - self.origin[1], wz - self.origin[2]);
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            self.chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
        }
    }
}

fn radius_round(rng: &mut SplitMix64, yo: i32, height: i32, width: i32) -> i32 {
    let k = 3.5 - next_f32(rng);
    let mut scale = (1.0 - (yo as f32).powi(2) / (height as f32 * k)) * width as f32;
    if height > 15 + next_int(rng, 5) {
        let t = if yo < 3 + next_int(rng, 6) { yo / 2 } else { yo };
        scale = (1.0 - t as f32 / (height as f32 * k * 0.4)) * width as f32;
    }
    (scale / 2.0).ceil() as i32
}

fn radius_ellipse(yo: i32, height: i32, width: i32) -> i32 {
    let scale = (1.0 - (yo as f32).powi(2) / height as f32) * width as f32;
    (scale / 2.0).ceil() as i32
}

fn radius_steep(rng: &mut SplitMix64, yo: i32, height: i32, width: i32) -> i32 {
    let k = 1.0 + next_f32(rng) / 2.0;
    let scale = (1.0 - yo as f32 / (height as f32 * k)) * width as f32;
    (scale / 2.0).ceil() as i32
}

/// Per-chunk iceberg pass (frozen oceans; packed 1/16, blue 1/200).
pub(crate) fn place_icebergs(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mc = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
            if !matches!(mc, McBiome::FrozenOcean | McBiome::DeepFrozenOcean) {
                continue;
            }
            // (variant key, rarity, block name)
            for (k, rarity, block) in [(0u64, 16u64, "packed_ice"), (1, 200, "blue_ice")] {
                let Some(main) = block_id_by_name(block) else { continue };
                let mut rng =
                    SplitMix64::new(stream_seed(seed, scx, scz, SALT_ICEBERG ^ (k << 40)));
                if !rng.next_u64().is_multiple_of(rarity) {
                    continue;
                }
                let ox = scx * cs + next_int(&mut rng, 16);
                let oz = scz * cs + next_int(&mut rng, 16);
                iceberg(chunk, origin, &mut rng, [ox, oz], main);
            }
        }
    }
}

fn iceberg(chunk: &mut Chunk, origin: [i32; 3], rng: &mut SplitMix64, center: [i32; 2], main: u16) {
    let snow_top = rng.next_f64() > 0.7;
    let angle = rng.next_f64() * 2.0 * std::f64::consts::PI;
    let ellipse_a = 11 - next_int(rng, 5);
    let ellipse_c = 3 + next_int(rng, 3);
    let is_ellipse = rng.next_f64() > 0.7;
    let mut over = if is_ellipse { next_int(rng, 6) + 6 } else { next_int(rng, 15) + 3 };
    if !is_ellipse && rng.next_f64() > 0.9 {
        over += next_int(rng, 19) + 7;
    }
    let under = (over + next_int(rng, 11)).min(18);
    let width = (over + next_int(rng, 7) - next_int(rng, 5)).min(11);
    let a = if is_ellipse { ellipse_a } else { 11 };
    let mut berg = Berg {
        chunk,
        origin,
        center,
        main,
        snow_top,
        is_ellipse,
        ellipse_a,
        ellipse_c,
        angle,
    };
    // Over-water body.
    for xo in -a..a {
        for zo in -a..a {
            for yo in 0..over {
                let radius = if is_ellipse {
                    radius_ellipse(yo, over, width)
                } else {
                    radius_round(rng, yo, over, width)
                };
                if is_ellipse || xo < radius {
                    berg.shell_cell(rng, over, xo, yo, zo, radius, a);
                }
            }
        }
    }
    // Under-water root (steeper).
    for xo in -a..a {
        for zo in -a..a {
            let mut yo = -1;
            while yo > -under {
                let new_a = if is_ellipse {
                    (a as f32 * (1.0 - (yo as f32).powi(2) / (under as f32 * 8.0))).ceil() as i32
                } else {
                    a
                };
                let radius = radius_steep(rng, -yo, under, width);
                if xo < radius {
                    berg.shell_cell(rng, under, xo, yo, zo, radius, new_a);
                }
                yo -= 1;
            }
        }
    }
    // The bite.
    let do_cut = if is_ellipse { rng.next_f64() > 0.1 } else { rng.next_f64() > 0.7 };
    if do_cut {
        let sign_x = if rng.next_u64().is_multiple_of(2) { -1 } else { 1 };
        let sign_z = if rng.next_u64().is_multiple_of(2) { -1 } else { 1 };
        let mut x_off = next_int(rng, (width / 2 - 2).max(1));
        if rng.next_u64().is_multiple_of(2) {
            x_off = width / 2 + 1 - next_int(rng, (width - width / 2 - 1).max(1));
        }
        let mut z_off = next_int(rng, (width / 2 - 2).max(1));
        if rng.next_u64().is_multiple_of(2) {
            z_off = width / 2 + 1 - next_int(rng, (width - width / 2 - 1).max(1));
        }
        if is_ellipse {
            let o = next_int(rng, (ellipse_a - 5).max(1));
            x_off = o;
            z_off = o;
        }
        let lo = [sign_x * x_off, sign_z * z_off];
        let cut_angle = if is_ellipse {
            angle + std::f64::consts::FRAC_PI_2
        } else {
            rng.next_f64() * 2.0 * std::f64::consts::PI
        };
        for yo in 0..(over - 3).max(0) {
            let radius = radius_round(rng, yo, over, width);
            berg.carve(radius, yo, false, cut_angle, lo);
        }
        let stop = -over + next_int(rng, 5);
        let mut yo = -1;
        while yo > stop {
            let radius = radius_steep(rng, -yo, over, width);
            berg.carve(radius, yo, true, cut_angle, lo);
            yo -= 1;
        }
    }
}
