//! Per-voxel lighting subsystem.
//!
//! Phase L (Wave 3): block + sky light propagation with a day/night
//! shader multiplier. Lights are stored per-chunk as nibble-packed
//! `LightGrid`s (4 bits per cell per layer) and refreshed by two
//! BFS engines:
//!
//! - `block_engine.rs` floods block-emitted light (torch=14,
//!   glowstone/lava/fire=15) outward through transparent voxels.
//! - `sky_engine.rs` runs a top-down column walk for daylight then
//!   a horizontal flood for caves / overhangs.
//!
//! Lighting is NEVER persisted on disk (matches MC) - the runtime
//! re-derives both layers from the block grid on world load + on
//! dim switch.
//!
//! Module layout mirrors MC's `net/minecraft/world/level/lighting`
//! package at a coarse level. Each engine is the "incremental"
//! variant simplified to a conservative full re-flood of the
//! touched chunk + its 8 neighbours per block edit.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

pub(crate) mod grid;
pub(crate) mod block_engine;
pub(crate) mod sky_engine;

// ---------------------------------------------------------------------------
// W27.2: LightingWorldAccess - thin read-only trait so BFS engines can run
// against both the live `World` (main thread) and an owned `ChunkSnapshot`
// (worker thread). The worker owns its own snapshot and never borrows from
// the main-thread World.
// ---------------------------------------------------------------------------

/// Read-only voxel access needed by the BFS lighting engines.
///
/// Two implementations:
/// - `&World` (main thread): reads the live world directly.
/// - `&ChunkSnapshot` (worker thread): reads an owned map of cloned chunks.
///
/// The propagate_* functions in block_engine + sky_engine accept
/// `&impl LightingWorldAccess` so they compile for both call sites.
pub(crate) trait LightingWorldAccess {
    /// Look up the block at world voxel coord `pos`. Returns air
    /// (`BlockId::AIR`) for positions outside any loaded chunk.
    fn get_block(&self, pos: [i32; 3]) -> BlockId;
}

impl LightingWorldAccess for World {
    #[inline]
    fn get_block(&self, pos: [i32; 3]) -> BlockId {
        self.get(pos)
    }
}

/// W27.2: Owned snapshot of a set of chunks used by worker-thread BFS.
/// Built on the main thread by cloning the relevant `Chunk`s; the worker
/// reads it without any borrow into the main-thread `World`.
pub(crate) struct ChunkSnapshot {
    /// Owned chunk map: chunk coord -> voxel data.
    pub(crate) chunks: HashMap<ChunkCoord, Chunk>,
}

impl ChunkSnapshot {
    /// Build a snapshot from an iterator of `(coord, chunk)` pairs.
    pub(crate) fn from_iter(iter: impl IntoIterator<Item = (ChunkCoord, Chunk)>) -> Self {
        Self {
            chunks: iter.into_iter().collect(),
        }
    }
}

impl LightingWorldAccess for ChunkSnapshot {
    #[inline]
    fn get_block(&self, pos: [i32; 3]) -> BlockId {
        let cs = CHUNK_SIZE as i32;
        let cc = [
            pos[0].div_euclid(cs),
            pos[1].div_euclid(cs),
            pos[2].div_euclid(cs),
        ];
        let lc = [
            pos[0].rem_euclid(cs) as usize,
            pos[1].rem_euclid(cs) as usize,
            pos[2].rem_euclid(cs) as usize,
        ];
        self.chunks
            .get(&cc)
            .map(|c| c.get(lc[0], lc[1], lc[2]))
            .unwrap_or(BlockId::AIR)
    }
}

#[cfg(test)]
mod grid_tests;
#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod day_factor_tests;
#[cfg(test)]
mod mesh_integration_tests;

pub(crate) use grid::LightGrid;

// ---------------------------------------------------------------------------
// W24: HaloLighting - snapshot of own + 6-neighbour LightGrids shipped with
// a ChunkJob so the worker can bake per-vertex lighting off the main thread.
// ---------------------------------------------------------------------------

/// W24: Snapshot of the 6-neighbour light grids that the mesher
/// needs when building faces on chunk boundaries. Pre-fetched on the
/// main thread and shipped with each ChunkJob so workers can
/// bake per-vertex lighting without accessing the main-thread
/// LightingState.
///
/// W26.1: each slot is now `Option<Arc<LightGrid>>` instead of
/// `Option<LightGrid>`. Cloning a `HaloLighting` bumps 7 refcounts
/// (7 x 8 bytes) instead of copying 7 x 4096 bytes (~28 KB). The
/// `LightingState` stores `Arc<LightGrid>` internally so the halo
/// snapshot is zero-copy on the main thread.
///
/// `None` means the neighbour chunk's lighting hasn't been computed
/// yet; the mesher falls back to `[0.0, 1.0]` (no block light, full
/// sky) - the same fallback used by the main-thread
/// `build_lit_submeshes`.
///
/// `Default` yields all-None (all slots missing = all lighting fallback).
#[derive(Clone, Default)]
pub(crate) struct HaloLighting {
    /// Own chunk's light grid.
    pub(crate) own: Option<Arc<LightGrid>>,
    /// +X neighbour (dx=+1, dy=0, dz=0).
    pub(crate) px: Option<Arc<LightGrid>>,
    /// -X neighbour.
    pub(crate) nx: Option<Arc<LightGrid>>,
    /// +Z neighbour.
    pub(crate) pz: Option<Arc<LightGrid>>,
    /// -Z neighbour.
    pub(crate) nz: Option<Arc<LightGrid>>,
    /// +Y neighbour.
    pub(crate) py: Option<Arc<LightGrid>>,
    /// -Y neighbour.
    pub(crate) ny: Option<Arc<LightGrid>>,
}

impl HaloLighting {
    /// Look up `[block_light, sky_light]` for a world-voxel coordinate
    /// `world` that may lie in the own chunk or in one of the 6
    /// direct neighbours. `own_coord` is the chunk coord this halo
    /// belongs to (used to compute the delta).
    ///
    /// Returns `[0.0, 1.0]` (no block light, full sky) when the
    /// required grid is absent.
    pub(crate) fn lookup(&self, own_coord: ChunkCoord, world: [i32; 3]) -> [f32; 2] {
        let cs = CHUNK_SIZE as i32;
        let other_cc = [
            world[0].div_euclid(cs),
            world[1].div_euclid(cs),
            world[2].div_euclid(cs),
        ];
        let other_lc = [
            world[0].rem_euclid(cs) as u32,
            world[1].rem_euclid(cs) as u32,
            world[2].rem_euclid(cs) as u32,
        ];
        let grid = if other_cc == own_coord {
            self.own.as_ref()
        } else {
            let delta = [
                other_cc[0] - own_coord[0],
                other_cc[1] - own_coord[1],
                other_cc[2] - own_coord[2],
            ];
            match delta {
                [1, 0, 0] => self.px.as_ref(),
                [-1, 0, 0] => self.nx.as_ref(),
                [0, 0, 1] => self.pz.as_ref(),
                [0, 0, -1] => self.nz.as_ref(),
                [0, 1, 0] => self.py.as_ref(),
                [0, -1, 0] => self.ny.as_ref(),
                _ => None,
            }
        };
        match grid {
            Some(g) => [
                g.get_block(other_lc) as f32 / 15.0,
                g.get_sky(other_lc) as f32 / 15.0,
            ],
            None => [0.0, 1.0],
        }
    }
}

/// Per-`Voxel` lighting state: one `LightGrid` per loaded chunk
/// plus a dirty-set of world voxel positions that need re-lighting
/// on the next lighting tick.
///
/// Lives parallel to `World.chunks`. A chunk's grid is created
/// lazily on the first `get_or_init`; entries get cleared when the
/// chunk unloads (dim swap, save unload). The `dirty` set is drained
/// each lighting tick and translates voxel coords into the set of
/// chunks that need a full re-flood pass.
///
/// W26.1: grids are stored as `Arc<LightGrid>` so that
/// `HaloLighting` snapshots are reference-count bumps instead of
/// 4096-byte memcpy calls. Mutable access goes through
/// `Arc::make_mut` (copy-on-write) so in the common case where the
/// arc is unique (not shared with any in-flight worker), we skip the
/// allocation entirely.
#[derive(Debug, Default)]
pub(crate) struct LightingState {
    /// Loaded chunks -> their nibble-packed light grids.
    pub(crate) grids: HashMap<ChunkCoord, Arc<LightGrid>>,
    /// World voxel positions touched since the last lighting tick.
    /// Drained by `take_dirty` + fed back as chunk coords to the
    /// engines.
    pub(crate) dirty: HashSet<[i32; 3]>,
}

impl LightingState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Drop every stored grid + flush the dirty set. Used on
    /// dim switch (the active dim's chunks completely change) so
    /// the next access re-derives lighting from scratch.
    pub(crate) fn clear(&mut self) {
        self.grids.clear();
        self.dirty.clear();
    }

    /// Mark `voxel` as needing a re-light on the next lighting
    /// tick. The exact recompute strategy lives in the engine
    /// modules; this just records the request.
    pub(crate) fn mark_dirty(&mut self, voxel: [i32; 3]) {
        self.dirty.insert(voxel);
    }

    /// Get-or-create the `LightGrid` for `chunk`. Brand-new entries
    /// start fully zeroed; the engines populate them.
    ///
    /// W26.1: returns `&mut LightGrid` via `Arc::make_mut` so that
    /// when the arc is uniquely owned (no in-flight worker holds a
    /// clone), no allocation is needed. When a worker does hold a
    /// clone, a fresh copy is made here (copy-on-write semantics).
    pub(crate) fn ensure(&mut self, chunk: ChunkCoord) -> &mut LightGrid {
        let arc = self.grids.entry(chunk).or_insert_with(|| Arc::new(LightGrid::empty()));
        Arc::make_mut(arc)
    }

    /// Borrow the grid for `chunk` without creating one. Returns
    /// `None` for chunks the engines have never visited (treated
    /// by callers as "all zero" / "treat as full sky 15" depending
    /// on context).
    ///
    /// W26.1: returns `Option<&Arc<LightGrid>>` so callers can
    /// cheaply clone the Arc for `HaloLighting` snapshots.
    pub(crate) fn get(&self, chunk: ChunkCoord) -> Option<&Arc<LightGrid>> {
        self.grids.get(&chunk)
    }

    /// Drain the dirty set, returning all queued voxel positions.
    /// Caller is expected to feed them into the engines + clear
    /// downstream state.
    pub(crate) fn take_dirty(&mut self) -> HashSet<[i32; 3]> {
        std::mem::take(&mut self.dirty)
    }

    /// Drop the grid for `chunk` if present. Used on chunk unload.
    #[allow(dead_code)] // wired up in a follow-up commit that streams chunks out.
    pub(crate) fn forget(&mut self, chunk: ChunkCoord) {
        self.grids.remove(&chunk);
    }

    /// W27.5: Insert or replace the light grid for `chunk` with a
    /// pre-computed `Arc<LightGrid>` from a worker-thread BFS result.
    /// The worker creates a fresh `Arc` (unique refcount); the main
    /// thread atomically stores it here. Any snapshot held by an
    /// in-flight remesh job sees the old `Arc` via COW - no race.
    pub(crate) fn set_grid(&mut self, chunk: ChunkCoord, grid: Arc<LightGrid>) {
        self.grids.insert(chunk, grid);
    }

    /// W27.2: Build a `LightingState` pre-seeded with existing light
    /// grids. Used by the worker thread to warm-start the BFS from
    /// existing values rather than from all-zero. Clones the Arc
    /// handles (cheap) - does NOT copy grid bytes.
    pub(crate) fn with_grids(grids: HashMap<ChunkCoord, Arc<LightGrid>>) -> Self {
        Self {
            grids,
            dirty: HashSet::new(),
        }
    }
}

/// Block-emission lookup. Mirrors MC's `BlockBehaviour.lightEmission`
/// constants in `net/minecraft/world/level/block/Blocks.java`.
///
/// W11.2: the source-of-truth table moved to
/// `crate::block::light_emission::MC_EMITTERS` - a 1:1 transcribe
/// of every `.lightLevel(...)` modifier in MC's `Blocks.java`.
/// This wrapper just forwards to the engine-id resolver.
pub(crate) fn block_emission(id: u16) -> u8 {
    crate::block::light_emission::light_emission_for_id(id)
}

// ---------------------------------------------------------------
// Day / night ramp.
// ---------------------------------------------------------------

/// Map a `(day_time, day_length_s)` pair to the shader's sky-light
/// multiplier.
///
/// Returns a value in `[0.04, 1.0]`:
/// - Day (phase ~0.0..0.5): 1.0 (full daylight).
/// - Night (phase ~0.55..0.95): 0.04 (faint moonlight floor).
/// - Dawn (0.5..0.55) + Dusk (0.95..1.0): smoothstep ramp.
///
/// Phase is computed from `day_time / day_length_s` and wrapped via
/// `rem_euclid(1.0)` so the function is total over the real line.
/// The shader reads this as `vec2<f32>(block_light_norm,
/// sky_light_norm * day_factor)` then takes a max for the effective
/// light value.
pub(crate) fn compute_day_factor(day_time: f32, day_length_s: f32) -> f32 {
    let phase = (day_time / day_length_s.max(1.0)).rem_euclid(1.0);
    // Cubic smoothstep: 3t^2 - 2t^3.
    fn smoothstep(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
    const NIGHT_FLOOR: f32 = 0.04;
    const DAY_PEAK: f32 = 1.0;
    if phase < 0.5 {
        // Full day.
        DAY_PEAK
    } else if phase < 0.55 {
        // Dusk: lerp from day -> night via smoothstep.
        let t = (phase - 0.5) / 0.05;
        let s = smoothstep(t);
        DAY_PEAK + (NIGHT_FLOOR - DAY_PEAK) * s
    } else if phase < 0.95 {
        NIGHT_FLOOR
    } else if phase < 1.0 {
        // Dawn: lerp from night -> day.
        let t = (phase - 0.95) / 0.05;
        let s = smoothstep(t);
        NIGHT_FLOOR + (DAY_PEAK - NIGHT_FLOOR) * s
    } else {
        DAY_PEAK
    }
}

// ---------------------------------------------------------------------------
// W24 / W26.1: HaloLighting unit tests - run via `cargo test --lib`.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod halo_tests {
    use std::sync::Arc;
    use super::{HaloLighting, LightGrid, LightingState};

    /// LightGrid is Clone; cloned copy preserves nibble values.
    #[test]
    fn light_grid_clone_preserves_values() {
        let mut g = LightGrid::empty();
        g.set_block([0, 0, 0], 10);
        g.set_sky([1, 1, 1], 15);
        let c = g.clone();
        assert_eq!(c.get_block([0, 0, 0]), 10);
        assert_eq!(c.get_sky([1, 1, 1]), 15);
        // Mutating a second clone doesn't affect the first.
        let mut g2 = g.clone();
        g2.set_block([0, 0, 0], 5);
        assert_eq!(c.get_block([0, 0, 0]), 10, "original clone unaffected");
    }

    /// All-None halo (default) returns the fallback [0.0, 1.0] everywhere.
    #[test]
    fn halo_none_grids_return_fallback() {
        let halo = HaloLighting::default();
        assert_eq!(halo.lookup([0, 0, 0], [0, 0, 0]), [0.0, 1.0]);
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        assert_eq!(halo.lookup([0, 0, 0], [cs, 0, 0]), [0.0, 1.0]);
    }

    /// Lookup reads from own grid when world coord maps to own chunk.
    #[test]
    fn halo_own_grid_lookup() {
        let mut own = LightGrid::empty();
        own.set_block([3, 4, 5], 7);
        own.set_sky([3, 4, 5], 14);
        let halo = HaloLighting { own: Some(Arc::new(own)), ..Default::default() };
        let v = halo.lookup([0, 0, 0], [3, 4, 5]);
        assert!((v[0] - 7.0 / 15.0).abs() < 1e-5, "block light mismatch");
        assert!((v[1] - 14.0 / 15.0).abs() < 1e-5, "sky light mismatch");
    }

    /// Lookup reads from px grid for world coords in the +X chunk.
    #[test]
    fn halo_px_grid_lookup() {
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let mut px = LightGrid::empty();
        px.set_block([0, 0, 0], 12);
        let halo = HaloLighting { px: Some(Arc::new(px)), ..Default::default() };
        // World [cs, 0, 0] = local (0,0,0) in chunk [1,0,0] = px slot.
        let v = halo.lookup([0, 0, 0], [cs, 0, 0]);
        assert!((v[0] - 12.0 / 15.0).abs() < 1e-5, "block light from px grid");
    }

    /// Lookup reads from nx grid for world coords in the -X chunk.
    #[test]
    fn halo_nx_grid_lookup() {
        let mut nx = LightGrid::empty();
        nx.set_block([15, 0, 0], 9);
        let halo = HaloLighting { nx: Some(Arc::new(nx)), ..Default::default() };
        // World [-1, 0, 0] = local (15,0,0) in chunk [-1,0,0] = nx slot.
        let v = halo.lookup([0, 0, 0], [-1, 0, 0]);
        assert!((v[0] - 9.0 / 15.0).abs() < 1e-5, "block light from nx grid");
    }

    /// Diagonal neighbour (chunk [+1,+1,0]) falls through to fallback.
    #[test]
    fn halo_diagonal_returns_fallback() {
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let mut diag = LightGrid::empty();
        diag.set_block([0, 0, 0], 8);
        // Put the grid in px but request the diagonal chunk.
        let halo = HaloLighting { px: Some(Arc::new(diag)), ..Default::default() };
        // World [cs, cs, 0] = chunk [1, 1, 0] = diagonal, no slot.
        let v = halo.lookup([0, 0, 0], [cs, cs, 0]);
        assert_eq!(v, [0.0, 1.0], "diagonal neighbour -> fallback");
    }

    /// HaloLighting itself is Clone.
    #[test]
    fn halo_lighting_is_clone() {
        let mut own = LightGrid::empty();
        own.set_block([0, 0, 0], 5);
        let halo = HaloLighting { own: Some(Arc::new(own)), ..Default::default() };
        let cloned = halo.clone();
        let v = cloned.lookup([0, 0, 0], [0, 0, 0]);
        assert!((v[0] - 5.0 / 15.0).abs() < 1e-5);
    }

    // -----------------------------------------------------------------
    // W26.1: Arc<LightGrid> correctness + performance tests.
    // -----------------------------------------------------------------

    /// W26.1: LightingState::get returns Option<&Arc<LightGrid>>.
    /// This means `cloned()` bumps the refcount, not the grid bytes.
    #[test]
    fn lighting_state_get_returns_arc() {
        let mut state = LightingState::new();
        let cc = [0i32, 0, 0];
        // Before insertion: None.
        assert!(state.get(cc).is_none());
        // Insert via ensure.
        state.ensure(cc).set_block([1, 2, 3], 7);
        // get() returns Some(&Arc<LightGrid>).
        let arc = state.get(cc).expect("grid present");
        assert_eq!(arc.get_block([1, 2, 3]), 7);
    }

    /// W26.1: cloning a HaloLighting with all 7 grids filled is cheap
    /// (refcount bumps only, not 7x4096-byte copies).
    /// We verify by cloning 1000 times; the test must complete quickly.
    #[test]
    fn halo_clone_is_cheap_with_arc_grids() {
        let mut state = LightingState::new();
        // Populate 7 neighbour coords.
        let cc = [5i32, 5, 5];
        let neighbours = [
            cc,
            [cc[0] + 1, cc[1], cc[2]],
            [cc[0] - 1, cc[1], cc[2]],
            [cc[0], cc[1], cc[2] + 1],
            [cc[0], cc[1], cc[2] - 1],
            [cc[0], cc[1] + 1, cc[2]],
            [cc[0], cc[1] - 1, cc[2]],
        ];
        for n in neighbours {
            state.ensure(n).set_block([0, 0, 0], 5);
        }
        let halo = HaloLighting {
            own: state.get(cc).cloned(),
            px:  state.get([cc[0] + 1, cc[1], cc[2]]).cloned(),
            nx:  state.get([cc[0] - 1, cc[1], cc[2]]).cloned(),
            pz:  state.get([cc[0], cc[1], cc[2] + 1]).cloned(),
            nz:  state.get([cc[0], cc[1], cc[2] - 1]).cloned(),
            py:  state.get([cc[0], cc[1] + 1, cc[2]]).cloned(),
            ny:  state.get([cc[0], cc[1] - 1, cc[2]]).cloned(),
        };
        // Clone 1000x - must finish in well under 1 s.
        let t0 = std::time::Instant::now();
        for _ in 0..1000 {
            let _copy = halo.clone();
        }
        let elapsed_ms = t0.elapsed().as_millis();
        // Even on a slow CI machine, 1000 Arc refcount bumps x 7 slots
        // should complete in < 50 ms comfortably.
        assert!(
            elapsed_ms < 50,
            "1000 HaloLighting clones took {elapsed_ms} ms (expected < 50 ms)"
        );
    }

    /// W26.1: ensure() COW - mutating after a clone does not affect
    /// the snapshot held by HaloLighting.
    #[test]
    fn lighting_state_ensure_cow_after_snapshot() {
        let mut state = LightingState::new();
        let cc = [0i32, 0, 0];
        state.ensure(cc).set_block([0, 0, 0], 3);
        // Snapshot the Arc before further mutations.
        let snap = state.get(cc).cloned().expect("grid present");
        // Mutate via ensure (should trigger COW because snap holds a ref).
        state.ensure(cc).set_block([0, 0, 0], 9);
        // The snapshot must still hold the old value.
        assert_eq!(snap.get_block([0, 0, 0]), 3, "snapshot not affected by COW mutation");
        // The state now reflects the new value.
        assert_eq!(state.get(cc).unwrap().get_block([0, 0, 0]), 9);
    }
}
