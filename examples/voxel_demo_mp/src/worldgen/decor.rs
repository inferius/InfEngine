//! W79 - unified decoration driver for the modern worldgen profiles
//! (Realistic + Natural). One entry point runs every decoration pass in
//! a fixed order so the per-profile `gen_*` fns don't each have to list
//! them, and so new passes (ground cover, biome features) plug in here
//! once for both profiles.
//!
//! Each pass is a sibling module (`trees`, `flowers`, `cactus`, ...);
//! `decor` only orchestrates. The Classic / Flat / Islands profiles keep
//! their own (legacy / bare) decoration paths - those use a different
//! biome system (`biome_at` vs the climate table) so they are not routed
//! through here.

use infengine_voxel::{Chunk, ChunkCoord};

use super::{WorldgenCtx, cactus, flowers, trees};

/// Run every decoration pass on a freshly carved chunk. Order matters:
/// trees first (they own the vertical space + cast the canopy), then the
/// surface-level passes (flowers, cactus) on whatever ground is left.
pub(crate) fn place_realistic(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    trees::place_realistic_from_coord(chunk, coord, ctx);
    flowers::place_realistic_from_coord(chunk, coord, ctx);
    cactus::place_realistic_from_coord(chunk, coord, ctx);
}
