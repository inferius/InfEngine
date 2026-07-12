//! W247 - shared plumbing for the scattered single-piece structures
//! (desert pyramid, swamp hut, igloo, ...): the random_spread
//! placement roll and the oriented `ScatteredFeaturePiece` transform.

use super::features::stream_seed;
use super::mineshaft::Bb;
use super::noise::SplitMix64;

/// `RandomSpreadStructurePlacement` (LINEAR spread): the candidate
/// start chunk of a spacing x spacing grid cell.
pub(crate) fn random_spread_start(
    seed: u32,
    grid_x: i32,
    grid_z: i32,
    spacing: i32,
    separation: i32,
    salt: u64,
) -> [i32; 2] {
    let mut rng = SplitMix64::new(stream_seed(seed, grid_x, grid_z, salt));
    let range = (spacing - separation).max(1);
    let ox = (rng.next_u64() % range as u64) as i32;
    let oz = (rng.next_u64() % range as u64) as i32;
    [grid_x * spacing + ox, grid_z * spacing + oz]
}

/// An oriented `ScatteredFeaturePiece`: bounding box + facing.
/// `dir`: 0 = north, 1 = south, 2 = west, 3 = east.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScatteredPiece {
    pub(crate) bb: Bb,
    pub(crate) dir: u8,
}

impl ScatteredPiece {
    /// `StructurePiece.makeBoundingBox`: X-axis facings transpose the
    /// footprint.
    pub(crate) fn new(
        west: i32,
        floor: i32,
        north: i32,
        dir: u8,
        width: i32,
        height: i32,
        depth: i32,
    ) -> Self {
        let (dx, dz) = if dir <= 1 { (width, depth) } else { (depth, width) };
        ScatteredPiece {
            bb: Bb {
                x0: west,
                y0: floor,
                z0: north,
                x1: west + dx - 1,
                y1: floor + height - 1,
                z1: north + dz - 1,
            },
            dir,
        }
    }

    /// `StructurePiece.getWorldX/Y/Z`. Local y may be negative
    /// (pieces hang cellars below their box).
    pub(crate) fn world(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        let b = &self.bb;
        match self.dir {
            0 => [b.x0 + x, b.y0 + y, b.z1 - z],
            1 => [b.x0 + x, b.y0 + y, b.z0 + z],
            2 => [b.x1 - z, b.y0 + y, b.z0 + x],
            _ => [b.x0 + z, b.y0 + y, b.z0 + x],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_axis_facings_transpose_the_footprint() {
        // 7 wide x 9 deep hut: Z facings keep 7x9, X facings flip it.
        let n = ScatteredPiece::new(0, 64, 0, 0, 7, 7, 9);
        assert_eq!((n.bb.x1 - n.bb.x0 + 1, n.bb.z1 - n.bb.z0 + 1), (7, 9));
        let e = ScatteredPiece::new(0, 64, 0, 3, 7, 7, 9);
        assert_eq!((e.bb.x1 - e.bb.x0 + 1, e.bb.z1 - e.bb.z0 + 1), (9, 7));
        // Local (0,0,0) maps to a box corner in every orientation.
        for dir in 0..4 {
            let p = ScatteredPiece::new(10, 60, 20, dir, 7, 7, 9);
            let w = p.world(0, 0, 0);
            assert!(w[0] == p.bb.x0 || w[0] == p.bb.x1);
            assert!(w[2] == p.bb.z0 || w[2] == p.bb.z1);
            assert_eq!(w[1], 60);
        }
    }
}
