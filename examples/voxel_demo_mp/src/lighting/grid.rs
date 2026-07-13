//! Per-chunk light grid storage.
//!
//! Phase L.1: a `LightGrid` holds two 4-bit-packed arrays (block
//! light + sky light) for one `CHUNK_SIZE`^3 chunk (16x16x16 = 4096
//! cells). Each layer occupies 2048 bytes (16*16*16 / 2). Total
//! per-chunk footprint = 4096 bytes.
//!
//! Mirrors MC's per-section `DataLayer` packing
//! (`net/minecraft/world/level/chunk/DataLayer.java`):
//! - Cells are addressed by `(x, y, z)` in `0..CHUNK_SIZE` each.
//! - Flat index = `x + 16 * (y + 16 * z)` (matches `Chunk::index`).
//! - Each byte stores two cells. The LOW nibble (bits 0..3) is the
//!   EVEN flat index; the HIGH nibble (bits 4..7) is the ODD index.
//!
//! Two layers are stored independently (block + sky) because MC's
//! `BlockLightSectionStorage` + `SkyLightSectionStorage` are
//! separate `DataLayer`s. Reading both for shader output is a
//! sequential pair of nibble fetches.

use infengine_voxel::CHUNK_SIZE;

/// Bytes per nibble-packed 16^3 layer: 4096 cells / 2 cells per byte.
pub(crate) const LIGHT_LAYER_BYTES: usize = (CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE) / 2;

/// Per-chunk block + sky light, each 4-bit packed.
///
/// `block` carries emitter-propagated light (torch=14, glowstone=15,
/// lava=15, fire=15). `sky` carries top-down + horizontally flooded
/// daylight (0 in caves, 15 above the world surface). Both range
/// `0..=15`.
///
/// The grid is `Box`-allocated so a `Voxel` field that owns a
/// `HashMap<ChunkCoord, LightGrid>` doesn't blow the stack on a
/// hot insertion.
///
/// W24: `Clone` is derived so workers can receive a snapshot copy
/// of the 6-neighbour halo without sharing a reference into the
/// main-thread lighting state.
///
/// W275: `PartialEq` so the lighting worker can DIFF a re-flooded
/// grid against its warm-start copy and skip the remesh + grid swap
/// when nothing changed (two boxed memcmps).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LightGrid {
    block: Box<[u8; LIGHT_LAYER_BYTES]>,
    sky: Box<[u8; LIGHT_LAYER_BYTES]>,
}

impl Default for LightGrid {
    fn default() -> Self {
        Self::empty()
    }
}

impl LightGrid {
    /// Fresh grid with every cell at 0 in both layers.
    pub(crate) fn empty() -> Self {
        Self {
            block: Box::new([0u8; LIGHT_LAYER_BYTES]),
            sky: Box::new([0u8; LIGHT_LAYER_BYTES]),
        }
    }

    /// Fresh grid with every cell of the `sky` layer pre-filled to
    /// 15 and `block` at 0. Used as the seed state for the sky-light
    /// engine - the top-down column walk then carves opacity-blocked
    /// columns down to 0 before the horizontal BFS smooths edges.
    #[allow(dead_code)] // wired up in L.3 sky engine.
    pub(crate) fn sky_full() -> Self {
        let mut g = Self::empty();
        // Pack 15|15 = 0xFF in every byte.
        for b in g.sky.iter_mut() {
            *b = 0xFF;
        }
        g
    }

    /// Flat array index for `(x, y, z)` in `0..CHUNK_SIZE`.
    /// Matches `infengine_voxel::Chunk::index`.
    #[inline]
    fn flat(local: [u32; 3]) -> usize {
        let x = local[0] as usize;
        let y = local[1] as usize;
        let z = local[2] as usize;
        debug_assert!(x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE);
        x + CHUNK_SIZE * (y + CHUNK_SIZE * z)
    }

    #[inline]
    fn nibble_get(buf: &[u8; LIGHT_LAYER_BYTES], local: [u32; 3]) -> u8 {
        let idx = Self::flat(local);
        let byte = buf[idx >> 1];
        if idx & 1 == 0 {
            byte & 0x0F
        } else {
            (byte >> 4) & 0x0F
        }
    }

    #[inline]
    fn nibble_set(buf: &mut [u8; LIGHT_LAYER_BYTES], local: [u32; 3], v: u8) {
        let idx = Self::flat(local);
        let v = v & 0x0F;
        let byte = &mut buf[idx >> 1];
        if idx & 1 == 0 {
            *byte = (*byte & 0xF0) | v;
        } else {
            *byte = (*byte & 0x0F) | (v << 4);
        }
    }

    /// Block-light value at `(x, y, z)` local to the chunk. Range 0..=15.
    pub(crate) fn get_block(&self, local: [u32; 3]) -> u8 {
        Self::nibble_get(&self.block, local)
    }

    /// Sky-light value at `(x, y, z)` local to the chunk. Range 0..=15.
    pub(crate) fn get_sky(&self, local: [u32; 3]) -> u8 {
        Self::nibble_get(&self.sky, local)
    }

    /// Write a block-light value. Top nibble is masked off internally
    /// so callers can pass any `u8` without panicking.
    pub(crate) fn set_block(&mut self, local: [u32; 3], v: u8) {
        Self::nibble_set(&mut self.block, local, v);
    }

    /// Write a sky-light value. Same masking rules as `set_block`.
    pub(crate) fn set_sky(&mut self, local: [u32; 3], v: u8) {
        Self::nibble_set(&mut self.sky, local, v);
    }

    /// Combined light value for the shader at the given cell.
    ///
    /// Returns `max(block, round(sky * day_factor))` clamped to
    /// `0..=15`. `day_factor` is the global day/night ramp:
    /// 1.0 at noon, ~0.04 at midnight. Block light is daylight-
    /// independent so torches stay bright at night; sky light
    /// gets multiplied so caves stay dark and outdoor cells
    /// dim at night.
    pub(crate) fn combined(&self, local: [u32; 3], day_factor: f32) -> u8 {
        let b = self.get_block(local) as f32;
        let s = self.get_sky(local) as f32 * day_factor.clamp(0.0, 1.0);
        // Round to nearest, then clamp into 0..=15.
        let m = b.max(s);
        m.round().clamp(0.0, 15.0) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_grid_is_all_zeros() {
        let g = LightGrid::empty();
        for x in 0..CHUNK_SIZE as u32 {
            for y in 0..CHUNK_SIZE as u32 {
                for z in 0..CHUNK_SIZE as u32 {
                    assert_eq!(g.get_block([x, y, z]), 0);
                    assert_eq!(g.get_sky([x, y, z]), 0);
                }
            }
        }
    }

    #[test]
    fn sky_full_grid_has_15_everywhere_in_sky_layer() {
        let g = LightGrid::sky_full();
        for x in 0..CHUNK_SIZE as u32 {
            for y in 0..CHUNK_SIZE as u32 {
                for z in 0..CHUNK_SIZE as u32 {
                    assert_eq!(g.get_block([x, y, z]), 0);
                    assert_eq!(g.get_sky([x, y, z]), 15);
                }
            }
        }
    }

    #[test]
    fn block_round_trip_all_cells() {
        // Set + read every cell with a position-derived value.
        let mut g = LightGrid::empty();
        for x in 0..CHUNK_SIZE as u32 {
            for y in 0..CHUNK_SIZE as u32 {
                for z in 0..CHUNK_SIZE as u32 {
                    let v = ((x ^ y ^ z) & 0x0F) as u8;
                    g.set_block([x, y, z], v);
                }
            }
        }
        for x in 0..CHUNK_SIZE as u32 {
            for y in 0..CHUNK_SIZE as u32 {
                for z in 0..CHUNK_SIZE as u32 {
                    let expected = ((x ^ y ^ z) & 0x0F) as u8;
                    assert_eq!(g.get_block([x, y, z]), expected);
                    assert_eq!(g.get_sky([x, y, z]), 0);
                }
            }
        }
    }

    #[test]
    fn sky_round_trip_all_cells() {
        let mut g = LightGrid::empty();
        for x in 0..CHUNK_SIZE as u32 {
            for y in 0..CHUNK_SIZE as u32 {
                for z in 0..CHUNK_SIZE as u32 {
                    let v = ((x.wrapping_add(y).wrapping_add(z)) & 0x0F) as u8;
                    g.set_sky([x, y, z], v);
                }
            }
        }
        for x in 0..CHUNK_SIZE as u32 {
            for y in 0..CHUNK_SIZE as u32 {
                for z in 0..CHUNK_SIZE as u32 {
                    let expected = ((x.wrapping_add(y).wrapping_add(z)) & 0x0F) as u8;
                    assert_eq!(g.get_sky([x, y, z]), expected);
                    assert_eq!(g.get_block([x, y, z]), 0);
                }
            }
        }
    }

    #[test]
    fn high_nibble_does_not_leak_into_neighbour() {
        // Adjacent flat-index cells live in the same byte; write
        // both layers' adjacent cells to make sure the low + high
        // nibbles stay isolated.
        let mut g = LightGrid::empty();
        // Flat index 0 = (0,0,0) = LOW nibble, index 1 = (1,0,0)
        // = HIGH nibble. Verify by setting them independently.
        g.set_block([0, 0, 0], 15);
        assert_eq!(g.get_block([0, 0, 0]), 15);
        assert_eq!(g.get_block([1, 0, 0]), 0);
        g.set_block([1, 0, 0], 7);
        assert_eq!(g.get_block([0, 0, 0]), 15);
        assert_eq!(g.get_block([1, 0, 0]), 7);
        // Sky layer must be untouched by block writes.
        assert_eq!(g.get_sky([0, 0, 0]), 0);
        assert_eq!(g.get_sky([1, 0, 0]), 0);
    }

    #[test]
    fn set_masks_top_nibble() {
        // Writing 0xFF should land 0x0F, not 0xFF.
        let mut g = LightGrid::empty();
        g.set_block([3, 4, 5], 0xFF);
        assert_eq!(g.get_block([3, 4, 5]), 15);
        g.set_sky([3, 4, 5], 0x1F);
        assert_eq!(g.get_sky([3, 4, 5]), 15);
    }

    #[test]
    fn combined_takes_max_of_block_and_scaled_sky() {
        let mut g = LightGrid::empty();
        g.set_block([0, 0, 0], 10);
        g.set_sky([0, 0, 0], 15);
        // Day: 15*1.0 = 15 > 10 -> sky wins.
        assert_eq!(g.combined([0, 0, 0], 1.0), 15);
        // Midnight: 15*0.04 = 0.6 (rounds to 1) < 10 -> block wins.
        assert_eq!(g.combined([0, 0, 0], 0.04), 10);
        // Dusk-ish: 15*0.5 = 7.5 -> rounds to 8, block 10 wins.
        assert_eq!(g.combined([0, 0, 0], 0.5), 10);
    }

    #[test]
    fn combined_clamps_day_factor_above_one() {
        let mut g = LightGrid::empty();
        g.set_sky([1, 2, 3], 15);
        // day_factor > 1.0 must still return at most 15.
        assert_eq!(g.combined([1, 2, 3], 5.0), 15);
        assert_eq!(g.combined([1, 2, 3], 1.0), 15);
    }

    #[test]
    fn combined_clamps_day_factor_below_zero() {
        let mut g = LightGrid::empty();
        g.set_block([2, 2, 2], 5);
        g.set_sky([2, 2, 2], 15);
        // day_factor < 0.0 should be treated as 0 -> only block
        // contributes, so result = 5.
        assert_eq!(g.combined([2, 2, 2], -1.0), 5);
    }

    #[test]
    fn memory_footprint_per_chunk_is_4096_bytes() {
        // The two boxed arrays together must total 4096 bytes;
        // any drift (e.g. accidental padding or wrong constant)
        // doubles the per-chunk lighting memory budget.
        assert_eq!(LIGHT_LAYER_BYTES, 2048);
        assert_eq!(LIGHT_LAYER_BYTES * 2, 4096);
        // sanity-check that the boxed array's Sized matches.
        assert_eq!(std::mem::size_of::<[u8; LIGHT_LAYER_BYTES]>(), 2048);
    }
}
