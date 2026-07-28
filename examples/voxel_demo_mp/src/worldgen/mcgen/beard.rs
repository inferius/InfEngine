//! W444 - Beardifier port: structure terrain adaptation as a DENSITY
//! contribution, replacing the W399 carve-time "beard shave" (which
//! cut sheer-walled pits around hillside houses - the R31 "villages
//! ringed by black trenches" report).
//!
//! Source: `net.minecraft.world.level.levelgen.Beardifier` (26.1.2).
//! Rigid `beard_thin` pieces add `getBeardContribution(dx, dyToGround,
//! dz) * 0.8` to `final_density` inside a 24^3 kernel: positive below
//! the piece's ground line (terrain rises to meet floors), negative
//! above it (overhangs melt away). Honest gaps: jigsaw junctions (the
//! `* 0.4` road-smoothing term) are not recorded by our solver yet,
//! and only the beard_thin structures (village, outpost) feed the
//! field - ancient city `beard_box` / trial chamber `encapsulate`
//! keep their existing carve-time handling.

use super::jigsaw::{PlacedPiece, template_bb};
use super::{outpost, village, village_data};
use crate::worldgen::{WorldGen, WorldgenCtx};
use infengine_voxel::CHUNK_SIZE;
use std::sync::OnceLock;

/// Kernel half-width: a contribution dies once any axis distance
/// leaves `[-12, 11]` (Java `isInKernelRange` on `d + 12 < 24`).
const KERNEL_RADIUS: i32 = 12;

/// One rigid `beard_thin` piece: the REAL template footprint (not
/// the solver's expansion-hack box) + MC's ground line
/// (`box.minY + groundLevelDelta`).
#[derive(Clone, Copy)]
pub(crate) struct BeardRigid {
    pub(crate) x0: i32,
    pub(crate) z0: i32,
    pub(crate) x1: i32,
    pub(crate) z1: i32,
    pub(crate) ground_y: i32,
}

/// Java `Beardifier.BEARD_KERNEL`: 24^3 gaussian falloff, indexed
/// `[z + 12][x + 12][y + 12]`, values computed at `dy + 0.5`.
fn kernel() -> &'static [f32] {
    static K: OnceLock<Vec<f32>> = OnceLock::new();
    K.get_or_init(|| {
        let mut k = vec![0f32; 24 * 24 * 24];
        for zi in 0..24i32 {
            for xi in 0..24i32 {
                for yi in 0..24i32 {
                    let dx = xi - KERNEL_RADIUS;
                    let dy = (yi - KERNEL_RADIUS) as f64 + 0.5;
                    let dz = zi - KERNEL_RADIUS;
                    let dist_sqr = (dx * dx + dz * dz) as f64 + dy * dy;
                    k[(zi * 576 + xi * 24 + yi) as usize] =
                        (-dist_sqr / 16.0).exp() as f32;
                }
            }
        }
        k
    })
}

/// Java `Mth.fastInvSqrt` (bit hack + one Newton step) - kept
/// bit-faithful so the density field matches MC's.
fn fast_inv_sqrt(d: f64) -> f64 {
    let half = 0.5 * d;
    let bits = 6910469410427058090i64 - ((d.to_bits() as i64) >> 1);
    let y = f64::from_bits(bits as u64);
    y * (1.5 - half * y * y)
}

/// Java `getBeardContribution(dx, dy, dz, yToGround)`.
fn beard_contribution(dx: i32, dy: i32, dz: i32, y_to_ground: i32) -> f64 {
    let xi = dx + KERNEL_RADIUS;
    let yi = dy + KERNEL_RADIUS;
    let zi = dz + KERNEL_RADIUS;
    if !(0..24).contains(&xi) || !(0..24).contains(&yi) || !(0..24).contains(&zi)
    {
        return 0.0;
    }
    let dy_off = y_to_ground as f64 + 0.5;
    let dist_sqr = (dx * dx + dz * dz) as f64 + dy_off * dy_off;
    let value = -dy_off * fast_inv_sqrt(dist_sqr / 2.0) / 2.0;
    value * kernel()[(zi * 576 + xi * 24 + yi) as usize] as f64
}

/// Sum of the `beard_thin` contributions at one world cell - the
/// Java `compute` loop's `BEARD_THIN` arm (`dy = dyToGround`, factor
/// `0.8`).
pub(crate) fn beard_at(pieces: &[BeardRigid], x: i32, y: i32, z: i32) -> f64 {
    let mut v = 0.0;
    for p in pieces {
        let dx = (p.x0 - x).max(x - p.x1).max(0);
        let dz = (p.z0 - z).max(z - p.z1).max(0);
        let dy_to_ground = y - p.ground_y;
        v += beard_contribution(dx, dy_to_ground, dz, dy_to_ground) * 0.8;
    }
    v
}

/// Collect the rigid pieces of every village / outpost whose real
/// footprint comes within kernel range of chunk `(cx, cz)` - the
/// Java `forStructuresInChunk` sweep (`isCloseToChunk(chunkPos,
/// 12)`), minus junctions. Layout solves triggered here run under
/// `terrain::LayoutProbeGuard` (entered inside the layout builders),
/// so their height probes read beard-free columns and cannot recurse
/// back into this fetch.
pub(crate) fn rigid_pieces_near_chunk(
    seed: u32,
    cx: i32,
    cz: i32,
) -> Vec<BeardRigid> {
    let cs = CHUNK_SIZE as i32;
    // Layout probes only need the effective seed; dim stays empty so
    // `effective_seed()` returns `seed` unchanged.
    let ctx = WorldgenCtx {
        base_seed: seed,
        dim: String::new(),
        profile: WorldGen::Classic,
    };
    let mut out: Vec<BeardRigid> = Vec::new();
    let mut push_layout = |layout: &[PlacedPiece]| {
        for piece in layout {
            // terrain_matching plates drape on the heightmap - MC
            // only beards RIGID projections.
            if piece.terrain_matching {
                continue;
            }
            let Some(t) = village_data::template(&piece.template) else {
                continue;
            };
            let bb = template_bb(t.size, piece.position, piece.rotation);
            if bb.x0 - KERNEL_RADIUS > cx * cs + cs - 1
                || bb.x1 + KERNEL_RADIUS < cx * cs
                || bb.z0 - KERNEL_RADIUS > cz * cs + cs - 1
                || bb.z1 + KERNEL_RADIUS < cz * cs
            {
                continue;
            }
            out.push(BeardRigid {
                x0: bb.x0,
                z0: bb.z0,
                x1: bb.x1,
                z1: bb.z1,
                ground_y: bb.y0 + piece.ground_level_delta,
            });
        }
    };
    // Same start-grid reach the carve passes use (pieces stay within
    // ~80 blocks + an extent of their start anchor).
    const REACH: i32 = 7;
    for gx in (cx - REACH).div_euclid(village::SPACING)
        ..=(cx + REACH).div_euclid(village::SPACING)
    {
        for gz in (cz - REACH).div_euclid(village::SPACING)
            ..=(cz + REACH).div_euclid(village::SPACING)
        {
            let [scx, scz] = village::potential_start(seed, gx, gz);
            if (scx - cx).abs() > REACH || (scz - cz).abs() > REACH {
                continue;
            }
            if let Some(layout) = village::layout_for_start(&ctx, scx, scz) {
                push_layout(&layout);
            }
        }
    }
    for gx in (cx - REACH).div_euclid(outpost::SPACING)
        ..=(cx + REACH).div_euclid(outpost::SPACING)
    {
        for gz in (cz - REACH).div_euclid(outpost::SPACING)
            ..=(cz + REACH).div_euclid(outpost::SPACING)
        {
            let [scx, scz] = outpost::potential_start(seed, gx, gz);
            if (scx - cx).abs() > REACH || (scz - cz).abs() > REACH {
                continue;
            }
            if let Some(layout) = outpost::layout_for_start(&ctx, scx, scz) {
                push_layout(&layout);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kernel matches the Java shape: symmetric in X/Z, and the
    /// contribution is positive below the ground line (fills) and
    /// negative above it (carves).
    #[test]
    fn beard_contribution_signs_and_range() {
        // 3 below ground, at the wall.
        let below = beard_contribution(0, -3, 0, -3);
        assert!(below > 0.0, "below ground must fill: {below}");
        // 3 above ground.
        let above = beard_contribution(0, 3, 0, 3);
        assert!(above < 0.0, "above ground must carve: {above}");
        // Outside the kernel: zero.
        assert_eq!(beard_contribution(12, 0, 0, 0), 0.0);
        assert_eq!(beard_contribution(0, 12, 0, 12), 0.0);
        // X/Z symmetry.
        let a = beard_contribution(4, -2, 1, -2);
        let b = beard_contribution(1, -2, 4, -2);
        assert!((a - b).abs() < 1e-12, "{a} vs {b}");
    }

    /// `beard_at` accumulates the 0.8-weighted term and decays with
    /// distance from the piece.
    #[test]
    fn beard_at_decays_with_distance() {
        let piece = BeardRigid {
            x0: 0,
            z0: 0,
            x1: 8,
            z1: 8,
            ground_y: 64,
        };
        let near = beard_at(&[piece], 4, 60, 4);
        let far = beard_at(&[piece], 18, 60, 4);
        assert!(near > 0.0, "under the floor fills: {near}");
        assert!(near > far, "decay: near {near} vs far {far}");
        assert_eq!(beard_at(&[piece], 40, 60, 4), 0.0, "outside kernel");
    }
}
