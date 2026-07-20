//! W321 - distance LOD: far surface-shell tiles.
//!
//! Beyond the full-res view distance the world used to just end at
//! the fog curtain; VD 40 at full resolution is ~105k chunks and
//! render-bound (~45-55 FPS extrapolated, [[mc-worldgen-perf-plan]]).
//! This module adds a second ring of cheap SURFACE SHELLS: one tile
//! per chunk column (16x16). W349: the NEAR ring reads the REAL
//! post-carve floors (W314 shared column cache) so its silhouette
//! matches the streamed terrain exactly; the FAR (level 2) ring
//! samples the preliminary spline, whose tens-of-blocks error is
//! invisible past ~500 blocks. A tile is a greedy heightmap mesh:
//! merged top
//! quads + vertical skirts at height steps, textured from the real
//! block atlas (grass/sand/stone top + side tiles), plus a
//! translucent biome-tinted water sheet over sub-sea columns.
//!
//! Tiles upload through `create_chunk_mesh` (uber pool) so they get
//! the multi-draw batching + app-side frustum cull for free. The
//! shell sits 1/16 block BELOW the true surface, so when a full-res
//! chunk streams in over it the real geometry always wins the depth
//! test (no Z-fighting during the overlap window); the tile itself
//! is evicted on the next stream rescan once its column has real
//! meshes.

use std::collections::HashMap;
use std::sync::Arc;

use infengine_render::{MeshHandle, MeshVertex, Renderer};

use crate::resource_pack::{BlockFace, ResourcePack};
use crate::worldgen::{WorldgenCtx, mcgen};

/// Hard cap for the LOD ring radius (chunks). The Options slider +
/// `/lod` clamp to this.
pub(crate) const LOD_DISTANCE_MAX: i32 = 64;
/// Default LOD ring radius for fresh configs (0 = off).
pub(crate) const LOD_DISTANCE_DEFAULT: u8 = 40;

const CS: i32 = 16;
const SEA_LEVEL: i32 = 63;
/// Liquid surface height matches the full-res liquid mesher
/// (top water cell y=62, surface ~62.9).
const WATER_SURFACE: f32 = 62.9;
/// Shells sink 1/16 block under the true surface so streamed-in
/// full-res geometry always wins the depth test while both exist.
const SHELL_SINK: f32 = 1.0 / 16.0;

/// W333: LOD2 tile span in chunks (one far tile = 4x4 chunk
/// columns sampled every 4 blocks from the preliminary surface).
pub(crate) const LOD2_TILE_CHUNKS: i32 = 4;
/// Hard cap for the far ring radius (chunks). 255, NOT 256: the
/// settings field and every slider range are u8, and `256 as u8`
/// silently wrapped to 0 - the W333 far ring shipped permanently
/// disabled (locked slider, lod2_distance clamped to 0).
pub(crate) const LOD2_DISTANCE_MAX: i32 = 255;
/// Default far ring radius (chunks; 0 = off).
pub(crate) const LOD2_DISTANCE_DEFAULT: u8 = 128;

/// Job payload for `workers::Job::LodTile` - everything the worker
/// needs to build one tile off-thread.
pub(crate) struct LodTileJob {
    /// Tile key: chunk column `[cx, cz]` for level 1, a 4x4-chunk
    /// tile coordinate for level 2 (world chunk = key * 4).
    pub(crate) key: [i32; 2],
    /// 1 = near shell (per-column, real floors, trees, water),
    /// 2 = far shell (preliminary-surface heightmap, 4-block texels).
    pub(crate) level: u8,
    pub(crate) ctx: WorldgenCtx,
    pub(crate) pack: Arc<ResourcePack>,
    pub(crate) dim: String,
}

/// Finished tile geometry, shipped back over the worker channel.
pub(crate) struct LodTileResult {
    pub(crate) key: [i32; 2],
    pub(crate) level: u8,
    pub(crate) dim: String,
    pub(crate) y_min: f32,
    pub(crate) y_max: f32,
    /// W353: highest TERRAIN floor in the tile (impostor canopies
    /// push y_max above it). The full-res handover keys on the chunk
    /// containing this height: dropping the shell when the FIRST
    /// chunk of the column meshed (often a cave slice) left the
    /// surface bare for a beat.
    pub(crate) y_surface: f32,
    pub(crate) opaque: (Vec<MeshVertex>, Vec<u32>),
    pub(crate) water: (Vec<MeshVertex>, Vec<u32>),
    pub(crate) water_tint: [f32; 3],
}

/// App-side record of one uploaded tile.
pub(crate) struct LodTile {
    pub(crate) opaque: Option<MeshHandle>,
    pub(crate) water: Option<MeshHandle>,
    pub(crate) y_min: f32,
    pub(crate) y_max: f32,
    /// W353: see `LodTileResult::y_surface`.
    pub(crate) y_surface: f32,
    pub(crate) water_tint: [f32; 3],
}

/// Max-heap entry so the closest missing tile generates first.
/// Mirrors `workers::PendingChunk`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingLodTile {
    pub(crate) neg_dist_sq: i32,
    pub(crate) key: [i32; 2],
}

impl Ord for PendingLodTile {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.neg_dist_sq
            .cmp(&other.neg_dist_sq)
            .then_with(|| self.key.cmp(&other.key))
    }
}

impl PartialOrd for PendingLodTile {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Push one quad with the mesher's canonical winding: corners CCW
/// viewed from the positive normal side, indices `[0,2,1, 0,3,2]`
/// (matches `emit_element_quad` / the greedy mesher, W28.4).
#[allow(clippy::too_many_arguments)]
fn push_quad(
    verts: &mut Vec<MeshVertex>,
    idxs: &mut Vec<u32>,
    corners: [[f32; 3]; 4],
    quad_uv: [[f32; 2]; 4],
    normal: [f32; 3],
    tile: [f32; 4],
    anim_id: u32,
) {
    let base = verts.len() as u32;
    let uv0 = [tile[0], tile[1]];
    let size = [tile[2] - tile[0], tile[3] - tile[1]];
    for i in 0..4 {
        verts.push(
            MeshVertex::new(corners[i], normal)
                .with_tile_uv(uv0, size, quad_uv[i])
                .with_anim_id(anim_id)
                .with_light(0.0, 1.0),
        );
    }
    idxs.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// W329: an axis-aligned box (all six faces) with the mesher's
/// winding. Corner tables mirror the greedy mesher per-face
/// convention; the winding pin test sweeps every face's cross
/// product. `side`/`top`/`bottom` pick the atlas rects.
#[allow(clippy::too_many_arguments)]
fn push_box(
    verts: &mut Vec<MeshVertex>,
    idxs: &mut Vec<u32>,
    min: [f32; 3],
    max: [f32; 3],
    side: [f32; 4],
    top: [f32; 4],
    bottom: [f32; 4],
    anim: u32,
) {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    let (w, hgt, d) = (x1 - x0, y1 - y0, z1 - z0);
    // +Y top.
    push_quad(
        verts, idxs,
        [[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]],
        [[0.0, 0.0], [w, 0.0], [w, d], [0.0, d]],
        [0.0, 1.0, 0.0],
        top, anim,
    );
    // -Y bottom (reverse of top).
    push_quad(
        verts, idxs,
        [[x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x0, y0, z0]],
        [[0.0, d], [w, d], [w, 0.0], [0.0, 0.0]],
        [0.0, -1.0, 0.0],
        bottom, anim,
    );
    // -Z / +Z / -X / +X sides reuse the skirt corner convention.
    push_quad(
        verts, idxs,
        [[x1, y0, z0], [x1, y1, z0], [x0, y1, z0], [x0, y0, z0]],
        [[0.0, hgt], [0.0, 0.0], [w, 0.0], [w, hgt]],
        [0.0, 0.0, -1.0],
        side, anim,
    );
    push_quad(
        verts, idxs,
        [[x0, y0, z1], [x0, y1, z1], [x1, y1, z1], [x1, y0, z1]],
        [[0.0, hgt], [0.0, 0.0], [w, 0.0], [w, hgt]],
        [0.0, 0.0, 1.0],
        side, anim,
    );
    push_quad(
        verts, idxs,
        [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        [[0.0, 0.0], [d, 0.0], [d, hgt], [0.0, hgt]],
        [-1.0, 0.0, 0.0],
        side, anim,
    );
    push_quad(
        verts, idxs,
        [[x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]],
        [[0.0, hgt], [d, hgt], [d, 0.0], [0.0, 0.0]],
        [1.0, 0.0, 0.0],
        side, anim,
    );
}

/// Atlas rect for a block face, with the `__missing` sentinel as
/// the fallback so an unmapped id shows the obvious magenta.
fn face_tile(pack: &ResourcePack, block: u16, face: BlockFace) -> [f32; 4] {
    if let Some(t) = pack.block_face_ref(block, face) {
        return t.atlas_uv;
    }
    pack.textures
        .get(&crate::resource_pack::TextureKey::Block("__missing".to_string()))
        .map(|t| t.atlas_uv)
        .unwrap_or([0.0, 0.0, 0.0078125, 0.0078125])
}

/// Build one LOD tile. Pure + worker-safe: all terrain queries go
/// through the W314 cached column data (cheap after first touch).
pub(crate) fn build_lod_tile(job: &LodTileJob) -> LodTileResult {
    let [tx, tz] = job.key;
    let ctx = &job.ctx;
    let pack = &job.pack;

    // W349: heights come from the REAL post-carve floors again.
    // W335 sampled the preliminary spline here - it misses the
    // density terrain by tens of blocks in mountains (hills where
    // none exist, sea sheets over dry land, the user's screens), so
    // the honest column data is back for the NEAR ring. The far
    // LOD2 ring keeps the cheap spline where the error is invisible.
    // 18x18 height sample: the 1-column border feeds edge skirts so
    // tile seams close against neighbouring tiles.
    let mut h = [[0i32; 18]; 18];
    for (dz, row) in h.iter_mut().enumerate() {
        for (dx, cell) in row.iter_mut().enumerate() {
            let wx = tx * CS + dx as i32 - 1;
            let wz = tz * CS + dz as i32 - 1;
            *cell = mcgen::terrain::mc_floor_at(ctx, wx, wz);
        }
    }
    // Inner 16x16 top-block ids from the real surface painter.
    let mut top = [[0u16; 16]; 16];
    for (z, row) in top.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let wx = tx * CS + x as i32;
            let wz = tz * CS + z as i32;
            let hy = h[z + 1][x + 1];
            let mut b = mcgen::terrain::mc_surface_block_at(ctx, wx, wz);
            if b == 0 {
                b = mcgen::terrain::mc_painted_block_at(ctx, wx, hy, wz);
            }
            if b == 0 {
                b = crate::block::STONE_ID;
            }
            *cell = b;
        }
    }

    let mut ov: Vec<MeshVertex> = Vec::new();
    let mut oi: Vec<u32> = Vec::new();
    let mut y_min = f32::MAX;
    let mut y_max = f32::MIN;

    // Top faces, greedily merged along X per row (same height +
    // same block id). Corner order for +Y: (x0,z0) (x1,z0) (x1,z1)
    // (x0,z1) - CCW from above.
    for z in 0..16usize {
        let mut x = 0usize;
        while x < 16 {
            let b = top[z][x];
            let hy = h[z + 1][x + 1];
            let mut run = 1usize;
            while x + run < 16 && top[z][x + run] == b && h[z + 1][x + 1 + run] == hy {
                run += 1;
            }
            let x0 = (tx * CS + x as i32) as f32;
            let x1 = x0 + run as f32;
            let z0 = (tz * CS + z as i32) as f32;
            let z1 = z0 + 1.0;
            let y = (hy + 1) as f32 - SHELL_SINK;
            y_min = y_min.min(y);
            y_max = y_max.max(y);
            let tile = face_tile(pack, b, BlockFace::Top);
            let slot = pack
                .block_face_ref(b, BlockFace::Top)
                .map(|t| pack.anim_slot_for_ref(t) as u32)
                .unwrap_or(0);
            let anim = ((b as u32) << 8) | slot;
            let r = run as f32;
            push_quad(
                &mut ov,
                &mut oi,
                [[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]],
                [[0.0, 0.0], [r, 0.0], [r, 1.0], [0.0, 1.0]],
                [0.0, 1.0, 0.0],
                tile,
                anim,
            );
            x += run;
        }
    }

    // Skirts: for every column edge whose neighbour is LOWER, a
    // vertical sheet from this top down to the neighbour's top,
    // textured with the column's side face. Sheets taller than 16
    // split so quad_uv stays inside its packed 0..16 range.
    for z in 0..16usize {
        for x in 0..16usize {
            let b = top[z][x];
            let hy = h[z + 1][x + 1];
            let side_tile = face_tile(pack, b, BlockFace::Sides);
            let anim = (b as u32) << 8;
            let x0 = (tx * CS + x as i32) as f32;
            let z0 = (tz * CS + z as i32) as f32;
            let top_y = (hy + 1) as f32 - SHELL_SINK;
            // (neighbour dh index offset, face normal, edge line)
            let neighbours: [(i32, i32, [f32; 3]); 4] = [
                (0, -1, [0.0, 0.0, -1.0]), // -Z edge
                (0, 1, [0.0, 0.0, 1.0]),   // +Z edge
                (-1, 0, [-1.0, 0.0, 0.0]), // -X edge
                (1, 0, [1.0, 0.0, 0.0]),   // +X edge
            ];
            for (ndx, ndz, normal) in neighbours {
                let nh = h[(z as i32 + 1 + ndz) as usize][(x as i32 + 1 + ndx) as usize];
                if nh >= hy {
                    continue;
                }
                let bottom_y = (nh + 1) as f32 - SHELL_SINK;
                y_min = y_min.min(bottom_y);
                // Split into <=16-block vertical segments.
                let mut seg_top = top_y;
                while seg_top > bottom_y {
                    let seg_bot = (seg_top - 16.0).max(bottom_y);
                    let hh = seg_top - seg_bot;
                    let (corners, quad_uv) = skirt_quad(
                        x0, z0, seg_top, seg_bot, hh, ndx, ndz,
                    );
                    push_quad(
                        &mut ov, &mut oi, corners, quad_uv, normal, side_tile, anim,
                    );
                    seg_top = seg_bot;
                }
            }
        }
    }

    // Water sheet over sub-sea columns (ocean + rivers). Greedy
    // along X per row. Rendered translucent with the biome tint via
    // the instance colour (block id 0 in anim_id = "use instance").
    let mut wv: Vec<MeshVertex> = Vec::new();
    let mut wi: Vec<u32> = Vec::new();
    let water_tile = face_tile(pack, crate::block::WATER_ID, BlockFace::Top);
    let water_slot = pack
        .block_face_ref(crate::block::WATER_ID, BlockFace::Top)
        .map(|t| pack.anim_slot_for_ref(t) as u32)
        .unwrap_or(0);
    let wy = WATER_SURFACE - SHELL_SINK;
    for z in 0..16usize {
        let mut x = 0usize;
        while x < 16 {
            if h[z + 1][x + 1] >= SEA_LEVEL {
                x += 1;
                continue;
            }
            let mut run = 1usize;
            while x + run < 16 && h[z + 1][x + 1 + run] < SEA_LEVEL {
                run += 1;
            }
            let x0 = (tx * CS + x as i32) as f32;
            let x1 = x0 + run as f32;
            let z0 = (tz * CS + z as i32) as f32;
            let z1 = z0 + 1.0;
            y_max = y_max.max(wy);
            let r = run as f32;
            push_quad(
                &mut wv,
                &mut wi,
                [[x0, wy, z0], [x1, wy, z0], [x1, wy, z1], [x0, wy, z1]],
                [[0.0, 0.0], [r, 0.0], [r, 1.0], [0.0, 1.0]],
                [0.0, 1.0, 0.0],
                water_tile,
                water_slot,
            );
            x += run;
        }
    }

    // W329: tree impostors - the real biome tables roll density +
    // species on a dedicated stream (see lod_tree_sites); each site
    // becomes a trunk box + canopy box with the block's own atlas
    // art. Roughly ten quads a tree keeps the far forest cheap.
    for site in mcgen::trees::lod_tree_sites(ctx, tx, tz) {
        let lx = site.wx - tx * CS;
        let lz = site.wz - tz * CS;
        if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
            continue;
        }
        let floor = h[(lz + 1) as usize][(lx + 1) as usize];
        if floor < SEA_LEVEL {
            continue; // no impostors wading into the sea
        }
        let trunk_h = site.height.clamp(3, 14) as f32;
        let base_y = (floor + 1) as f32 - SHELL_SINK;
        let x0 = site.wx as f32;
        let z0 = site.wz as f32;
        let log_side = face_tile(pack, site.log, BlockFace::Sides);
        let log_anim = (site.log as u32) << 8;
        push_box(
            &mut ov,
            &mut oi,
            [x0, base_y, z0],
            [x0 + 1.0, base_y + trunk_h, z0 + 1.0],
            log_side,
            log_side,
            log_side,
            log_anim,
        );
        // Canopy: a 5x4x5 crown around the trunk top.
        let leaf = face_tile(pack, site.leaves, BlockFace::Sides);
        let leaf_anim = (site.leaves as u32) << 8;
        let cy0 = base_y + trunk_h - 2.0;
        push_box(
            &mut ov,
            &mut oi,
            [x0 - 2.0, cy0, z0 - 2.0],
            [x0 + 3.0, cy0 + 4.0, z0 + 3.0],
            leaf,
            leaf,
            leaf,
            leaf_anim,
        );
        y_max = y_max.max(cy0 + 4.0);
    }

    // Per-tile water tint from the biome at the tile centre.
    let mcb = mcgen::terrain::mc_full_biome_at(ctx, tx * CS + 8, tz * CS + 8);
    let water_tint = mcgen::colors::water_color(mcb);

    if y_min > y_max {
        y_min = 0.0;
        y_max = 0.0;
    }
    let y_surface = h
        .iter()
        .flatten()
        .copied()
        .max()
        .unwrap_or(0) as f32;
    LodTileResult {
        key: job.key,
        level: 1,
        dim: job.dim.clone(),
        y_min,
        y_max,
        y_surface,
        opaque: (ov, oi),
        water: (wv, wi),
        water_tint,
    }
}

/// W333: build one FAR shell tile (level 2). Covers
/// `LOD2_TILE_CHUNKS`^2 chunk columns sampled every 4 blocks from
/// the overworld PRELIMINARY surface spline - no column data, no
/// carve, no aquifer: a sample costs microseconds, so the 256-chunk
/// horizon stays cheap to build. Texels are 4x4 blocks textured
/// with the biome surface block; sub-sea texels take the water
/// sheet. No trees at this range.
pub(crate) fn build_lod2_tile(job: &LodTileJob) -> LodTileResult {
    const STEP: i32 = 4;
    const N: usize = (LOD2_TILE_CHUNKS as usize * 16) / STEP as usize;
    let [tx, tz] = job.key;
    let ctx = &job.ctx;
    let pack = &job.pack;
    let seed = ctx.effective_seed();
    let ow = mcgen::terrain::overworld_for(seed);
    let base_x = tx * LOD2_TILE_CHUNKS * CS;
    let base_z = tz * LOD2_TILE_CHUNKS * CS;

    // (N+2)^2 height samples: a 1-texel border feeds edge skirts.
    let mut h = [[0i32; N + 2]; N + 2];
    for (iz, row) in h.iter_mut().enumerate() {
        for (ix, cell) in row.iter_mut().enumerate() {
            let wx = base_x + (ix as i32 - 1) * STEP;
            let wz = base_z + (iz as i32 - 1) * STEP;
            *cell = ow.preliminary_surface(wx, wz);
        }
    }
    let mut ov: Vec<MeshVertex> = Vec::new();
    let mut oi: Vec<u32> = Vec::new();
    let mut wv: Vec<MeshVertex> = Vec::new();
    let mut wi: Vec<u32> = Vec::new();
    let mut y_min = f32::MAX;
    let mut y_max = f32::MIN;
    let water_tile = face_tile(pack, crate::block::WATER_ID, BlockFace::Top);
    let water_slot = pack
        .block_face_ref(crate::block::WATER_ID, BlockFace::Top)
        .map(|t| pack.anim_slot_for_ref(t) as u32)
        .unwrap_or(0);
    let wy = WATER_SURFACE - SHELL_SINK;
    for iz in 0..N {
        for ix in 0..N {
            let hy = h[iz + 1][ix + 1];
            let wx = base_x + ix as i32 * STEP;
            let wz = base_z + iz as i32 * STEP;
            let x0 = wx as f32;
            let z0 = wz as f32;
            let mc = mcgen::terrain::mc_full_biome_at(ctx, wx + 2, wz + 2);
            let block =
                crate::worldgen::climate::surface_block(mc.to_engine(), hy).0;
            let tile = face_tile(pack, block, BlockFace::Top);
            let anim = (block as u32) << 8;
            let y = (hy + 1) as f32 - SHELL_SINK;
            y_min = y_min.min(y);
            y_max = y_max.max(y);
            let sp = STEP as f32;
            push_quad(
                &mut ov,
                &mut oi,
                [
                    [x0, y, z0],
                    [x0 + sp, y, z0],
                    [x0 + sp, y, z0 + sp],
                    [x0, y, z0 + sp],
                ],
                [[0.0, 0.0], [sp, 0.0], [sp, sp], [0.0, sp]],
                [0.0, 1.0, 0.0],
                tile,
                anim,
            );
            let side_tile = face_tile(pack, block, BlockFace::Sides);
            let neighbours: [(i32, i32, [f32; 3]); 4] = [
                (0, -1, [0.0, 0.0, -1.0]),
                (0, 1, [0.0, 0.0, 1.0]),
                (-1, 0, [-1.0, 0.0, 0.0]),
                (1, 0, [1.0, 0.0, 0.0]),
            ];
            for (ndx, ndz, normal) in neighbours {
                let nh = h[(iz as i32 + 1 + ndz) as usize]
                    [(ix as i32 + 1 + ndx) as usize];
                if nh >= hy {
                    continue;
                }
                let bottom_y = (nh + 1) as f32 - SHELL_SINK;
                y_min = y_min.min(bottom_y);
                let mut seg_top = y;
                while seg_top > bottom_y {
                    let seg_bot = (seg_top - 16.0).max(bottom_y);
                    let hh = seg_top - seg_bot;
                    let (corners, quad_uv) = skirt_quad_w(
                        x0, z0, seg_top, seg_bot, hh, ndx, ndz, sp,
                    );
                    push_quad(
                        &mut ov, &mut oi, corners, quad_uv, normal,
                        side_tile, anim,
                    );
                    seg_top = seg_bot;
                }
            }
            if hy < SEA_LEVEL {
                y_max = y_max.max(wy);
                push_quad(
                    &mut wv,
                    &mut wi,
                    [
                        [x0, wy, z0],
                        [x0 + sp, wy, z0],
                        [x0 + sp, wy, z0 + sp],
                        [x0, wy, z0 + sp],
                    ],
                    [[0.0, 0.0], [sp, 0.0], [sp, sp], [0.0, sp]],
                    [0.0, 1.0, 0.0],
                    water_tile,
                    water_slot,
                );
            }
        }
    }
    let mcb = mcgen::terrain::mc_full_biome_at(
        ctx,
        base_x + LOD2_TILE_CHUNKS * 8,
        base_z + LOD2_TILE_CHUNKS * 8,
    );
    let water_tint = mcgen::colors::water_color(mcb);
    if y_min > y_max {
        y_min = 0.0;
        y_max = 0.0;
    }
    LodTileResult {
        key: job.key,
        level: 2,
        dim: job.dim.clone(),
        y_min,
        y_max,
        // Far ring: geometry is bare terrain, y_max IS the surface.
        y_surface: y_max,
        opaque: (ov, oi),
        water: (wv, wi),
        water_tint,
    }
}

/// Corner + quad_uv table for one vertical skirt segment. Corner
/// order is CCW viewed from the outward normal; UV `v` runs down
/// from the segment top so the texture hangs from the surface edge.
#[allow(clippy::too_many_arguments)]
fn skirt_quad(
    x0: f32,
    z0: f32,
    seg_top: f32,
    seg_bot: f32,
    hh: f32,
    ndx: i32,
    ndz: i32,
) -> ([[f32; 3]; 4], [[f32; 2]; 4]) {
    skirt_quad_w(x0, z0, seg_top, seg_bot, hh, ndx, ndz, 1.0)
}

/// W333: width-parametrised skirt (LOD2 texels are 4 blocks wide).
#[allow(clippy::too_many_arguments)]
fn skirt_quad_w(
    x0: f32,
    z0: f32,
    seg_top: f32,
    seg_bot: f32,
    hh: f32,
    ndx: i32,
    ndz: i32,
    w: f32,
) -> ([[f32; 3]; 4], [[f32; 2]; 4]) {
    let x1 = x0 + w;
    let z1 = z0 + w;
    // W325: corner order follows the greedy mesher's per-face tables
    // (CCW viewed from the OUTWARD normal, triangles [0,2,1][0,3,2]).
    // The first cut had every sheet wound inside-out, so skirts
    // back-face-culled from outside and the terrain read as hollow
    // ("see-through at the terrace steps" report). Verified by
    // cross-product per face.
    match (ndx, ndz) {
        // -Z edge, outward normal -Z.
        (0, -1) => (
            [
                [x1, seg_bot, z0],
                [x1, seg_top, z0],
                [x0, seg_top, z0],
                [x0, seg_bot, z0],
            ],
            [[0.0, hh], [0.0, 0.0], [w, 0.0], [w, hh]],
        ),
        // +Z edge, outward normal +Z.
        (0, 1) => (
            [
                [x0, seg_bot, z1],
                [x0, seg_top, z1],
                [x1, seg_top, z1],
                [x1, seg_bot, z1],
            ],
            [[0.0, hh], [0.0, 0.0], [w, 0.0], [w, hh]],
        ),
        // -X edge, outward normal -X.
        (-1, 0) => (
            [
                [x0, seg_top, z0],
                [x0, seg_top, z1],
                [x0, seg_bot, z1],
                [x0, seg_bot, z0],
            ],
            [[0.0, 0.0], [w, 0.0], [w, hh], [0.0, hh]],
        ),
        // +X edge, outward normal +X.
        _ => (
            [
                [x1, seg_bot, z0],
                [x1, seg_bot, z1],
                [x1, seg_top, z1],
                [x1, seg_top, z0],
            ],
            [[0.0, hh], [w, hh], [w, 0.0], [0.0, 0.0]],
        ),
    }
}

/// Queue every loaded LOD tile for the uber chunk pass, culled by
/// the tile's column AABB. Water draws with the biome tint + the
/// vanilla-ish 0.72 alpha (W286) through the TRANSLUCENT layer.
pub(crate) fn queue_lod_draws(
    renderer: &mut Renderer,
    tiles: &HashMap<[i32; 2], LodTile>,
    frustum: &crate::frustum::Frustum,
) {
    let model = glam::Mat4::IDENTITY.to_cols_array();
    for (key, t) in tiles {
        let min = glam::Vec3::new(
            (key[0] * CS) as f32,
            t.y_min - 1.0,
            (key[1] * CS) as f32,
        );
        let max = glam::Vec3::new(
            min.x + CS as f32,
            t.y_max + 1.0,
            min.z + CS as f32,
        );
        if !frustum.is_aabb_visible(min, max) {
            continue;
        }
        if let Some(hm) = t.opaque {
            renderer.queue_chunk_draw(hm, model, [1.0, 1.0, 1.0, 1.0], 0.0, 0.5);
        }
        if let Some(hw) = t.water {
            renderer.queue_chunk_draw(
                hw,
                model,
                [t.water_tint[0], t.water_tint[1], t.water_tint[2], 0.72],
                0.0,
                1.0,
            );
        }
    }
}

/// W333: queue the FAR ring (4x4-chunk tiles, 64-block AABBs).
pub(crate) fn queue_lod2_draws(
    renderer: &mut Renderer,
    tiles: &HashMap<[i32; 2], LodTile>,
    frustum: &crate::frustum::Frustum,
) {
    let model = glam::Mat4::IDENTITY.to_cols_array();
    let span = (LOD2_TILE_CHUNKS * CS) as f32;
    for (key, t) in tiles {
        let min = glam::Vec3::new(
            (key[0] * LOD2_TILE_CHUNKS * CS) as f32,
            t.y_min - 1.0,
            (key[1] * LOD2_TILE_CHUNKS * CS) as f32,
        );
        let max = glam::Vec3::new(min.x + span, t.y_max + 1.0, min.z + span);
        if !frustum.is_aabb_visible(min, max) {
            continue;
        }
        if let Some(hm) = t.opaque {
            renderer.queue_chunk_draw(hm, model, [1.0, 1.0, 1.0, 1.0], 0.0, 0.5);
        }
        if let Some(hw) = t.water {
            renderer.queue_chunk_draw(
                hw,
                model,
                [t.water_tint[0], t.water_tint[1], t.water_tint[2], 0.72],
                0.0,
                1.0,
            );
        }
    }
}

/// Drop every tile's meshes + clear the queues. Used on world exit,
/// dim switch and world reset.
pub(crate) fn clear_lod_state(
    renderer: Option<&mut Renderer>,
    tiles: &mut HashMap<[i32; 2], LodTile>,
    pending: &mut std::collections::BinaryHeap<PendingLodTile>,
    pending_set: &mut std::collections::HashSet<[i32; 2]>,
) {
    if let Some(r) = renderer {
        for (_k, t) in tiles.drain() {
            if let Some(h) = t.opaque {
                r.remove_chunk_mesh(h);
            }
            if let Some(h) = t.water {
                r.remove_chunk_mesh(h);
            }
        }
    } else {
        tiles.clear();
    }
    pending.clear();
    pending_set.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_lod_orders_closest_first() {
        let mut heap = std::collections::BinaryHeap::new();
        heap.push(PendingLodTile { neg_dist_sq: -100, key: [10, 0] });
        heap.push(PendingLodTile { neg_dist_sq: -4, key: [2, 0] });
        heap.push(PendingLodTile { neg_dist_sq: -25, key: [5, 0] });
        assert_eq!(heap.pop().unwrap().key, [2, 0]);
        assert_eq!(heap.pop().unwrap().key, [5, 0]);
        assert_eq!(heap.pop().unwrap().key, [10, 0]);
    }

    #[test]
    fn build_lod_tile_produces_surface_shell() {
        // Real mcgen column replay + whatever pack resolves on this
        // host (placeholder or MC) - the tile must come back with
        // whole-quad geometry inside the world's Y range, and every
        // top face sunk 1/16 under the true block surface.
        let pack = std::sync::Arc::new(
            crate::resource_pack::ResourcePack::load(),
        );
        let job = LodTileJob {
            key: [3, -2],
            level: 1,
            ctx: crate::worldgen::WorldgenCtx {
                base_seed: 1337,
                dim: "overworld".to_string(),
                profile: crate::worldgen::WorldGen::Mc,
            },
            pack,
            dim: "overworld".to_string(),
        };
        let res = build_lod_tile(&job);
        assert_eq!(res.key, [3, -2]);
        assert!(!res.opaque.0.is_empty(), "shell has top-face geometry");
        assert_eq!(res.opaque.0.len() % 4, 0, "vertices are whole quads");
        assert_eq!(
            res.opaque.1.len(),
            res.opaque.0.len() / 4 * 6,
            "6 indices per quad",
        );
        assert!(
            res.y_min >= -64.0 && res.y_max <= 320.0 && res.y_min <= res.y_max,
            "y range sane: {} {}",
            res.y_min,
            res.y_max,
        );
        // Every top-face vertex sits SHELL_SINK below a whole block
        // boundary (skirt corners share those Y values too).
        let y0 = res.opaque.0[0].position[1];
        let frac = (y0 + SHELL_SINK).fract().abs();
        assert!(
            frac < 1e-4 || (1.0 - frac) < 1e-4,
            "top face y {y0} not on a sunk block boundary",
        );
    }

    #[test]
    fn w325_skirt_windings_face_outward() {
        // The [0,2,1][0,3,2] index pattern means triangle one is
        // (c0, c2, c1); its geometric normal (c2-c0)x(c1-c0) must
        // point along the OUTWARD face normal or back-face culling
        // hides the sheet from outside (the W321 "hollow terraces"
        // report - every skirt was wound inside-out).
        let cases = [
            ((0i32, -1i32), [0.0f32, 0.0, -1.0]),
            ((0, 1), [0.0, 0.0, 1.0]),
            ((-1, 0), [-1.0, 0.0, 0.0]),
            ((1, 0), [1.0, 0.0, 0.0]),
        ];
        for ((dx, dz), n) in cases {
            let (c, _uv) = skirt_quad(0.0, 0.0, 10.0, 4.0, 6.0, dx, dz);
            let a = glam::Vec3::from(c[0]);
            let b = glam::Vec3::from(c[2]);
            let d = glam::Vec3::from(c[1]);
            let cross = (b - a).cross(d - a);
            let dot = cross.dot(glam::Vec3::from(n));
            assert!(
                dot > 0.0,
                "skirt ({dx},{dz}) winds inward: cross {cross:?} vs normal {n:?}",
            );
        }
    }

    #[test]
    fn w329_box_faces_wind_outward() {
        // Every push_box face's triangle-one normal must point away
        // from the box centre, or the impostor culls from outside
        // (the skirt lesson, now pinned for all six orientations).
        let mut v: Vec<infengine_render::MeshVertex> = Vec::new();
        let mut i: Vec<u32> = Vec::new();
        push_box(
            &mut v,
            &mut i,
            [0.0, 0.0, 0.0],
            [2.0, 3.0, 4.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            0,
        );
        assert_eq!(v.len(), 24, "six quads");
        let centre = glam::Vec3::new(1.0, 1.5, 2.0);
        for q in 0..6 {
            let c0 = glam::Vec3::from(v[q * 4].position);
            let c1 = glam::Vec3::from(v[q * 4 + 1].position);
            let c2 = glam::Vec3::from(v[q * 4 + 2].position);
            let n = (c2 - c0).cross(c1 - c0);
            let mid = (c0 + c2) * 0.5;
            assert!(
                n.dot(mid - centre) > 0.0,
                "face {q} winds inward (normal {n:?})",
            );
        }
    }

    #[test]
    fn w329_forest_tile_grows_impostors() {
        // A plains/forest-ish tile on the user seed must produce
        // tree quads beyond the bare heightmap: compare a build
        // against the same tile's site list.
        let pack = std::sync::Arc::new(
            crate::resource_pack::ResourcePack::load(),
        );
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 2120129009,
            dim: "overworld".to_string(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        // Scan outward for a tile with at least 3 dry tree sites.
        for r in 0..24i32 {
            for tz in -r..=r {
                for tx in -r..=r {
                    if tx.abs().max(tz.abs()) != r {
                        continue;
                    }
                    let sites =
                        crate::worldgen::mcgen::trees::lod_tree_sites(&ctx, tx, tz);
                    let dry = sites
                        .iter()
                        .filter(|s| {
                            crate::worldgen::mcgen::terrain::mc_floor_at(
                                &ctx, s.wx, s.wz,
                            ) >= SEA_LEVEL
                        })
                        .count();
                    if dry < 3 {
                        continue;
                    }
                    let res = build_lod_tile(&LodTileJob {
                        key: [tx, tz],
                        level: 1,
                        ctx,
                        pack,
                        dim: "overworld".to_string(),
                    });
                    // 16x16 top faces alone cap at 256 quads plus
                    // skirts; each dry site adds 12 quads (2 boxes),
                    // so the vertex count must clear the no-tree
                    // baseline by at least dry*48.
                    assert!(
                        res.opaque.0.len() >= dry * 48,
                        "expected tree quads for {dry} sites, got {} verts",
                        res.opaque.0.len(),
                    );
                    return;
                }
            }
        }
        panic!("no forested tile found near origin for seed");
    }

    #[test]
    fn w325_ocean_tile_gets_a_water_sheet() {
        // Walk tiles outward from the origin until one is mostly
        // sub-sea, then require a translucent water sheet with the
        // biome tint. Guards the whole water path in build_lod_tile.
        let pack = std::sync::Arc::new(
            crate::resource_pack::ResourcePack::load(),
        );
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 2120129009,
            dim: "overworld".to_string(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        for r in 0..24i32 {
            for tz in -r..=r {
                for tx in -r..=r {
                    if tx.abs().max(tz.abs()) != r {
                        continue;
                    }
                    let deep = (0..16).all(|i| {
                        crate::worldgen::mcgen::terrain::mc_floor_at(
                            &ctx,
                            tx * 16 + i,
                            tz * 16 + 8,
                        ) < SEA_LEVEL - 2
                    });
                    if !deep {
                        continue;
                    }
                    let res = build_lod_tile(&LodTileJob {
                        key: [tx, tz],
                        level: 1,
                        ctx,
                        pack,
                        dim: "overworld".to_string(),
                    });
                    assert!(
                        !res.water.0.is_empty(),
                        "sub-sea tile [{tx},{tz}] must carry a water sheet",
                    );
                    assert!(
                        res.water_tint.iter().any(|c| *c > 0.05),
                        "water tint must not be black: {:?}",
                        res.water_tint,
                    );
                    assert!(
                        (res.y_max - (WATER_SURFACE - SHELL_SINK)).abs() < 1e-3,
                        "water sheet must raise y_max to the surface",
                    );
                    return;
                }
            }
        }
        panic!("no ocean tile found within 24 tiles of origin for seed");
    }

    #[test]
    fn w333_far_tile_builds_from_preliminary_surface() {
        // Level-2 tiles must come back as whole quads inside the
        // world Y range without touching column data, and a mostly
        // sub-sea far tile must still carry a water sheet.
        let pack = std::sync::Arc::new(
            crate::resource_pack::ResourcePack::load(),
        );
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 2120129009,
            dim: "overworld".to_string(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let land = build_lod2_tile(&LodTileJob {
            key: [0, 0],
            level: 2,
            ctx: ctx.clone(),
            pack: std::sync::Arc::clone(&pack),
            dim: "overworld".to_string(),
        });
        assert_eq!(land.level, 2);
        assert!(!land.opaque.0.is_empty(), "far shell has geometry");
        assert_eq!(land.opaque.0.len() % 4, 0, "whole quads");
        assert_eq!(land.opaque.1.len(), land.opaque.0.len() / 4 * 6);
        assert!(
            land.y_min >= -64.0
                && land.y_max <= 320.0
                && land.y_min <= land.y_max,
            "y range sane: {} {}",
            land.y_min,
            land.y_max,
        );
        // Find a deep far tile via the same preliminary surface the
        // builder samples, then require its water sheet.
        let ow = crate::worldgen::mcgen::terrain::overworld_for(
            ctx.effective_seed(),
        );
        let span = LOD2_TILE_CHUNKS * CS;
        for r in 0..16i32 {
            for tz in -r..=r {
                for tx in -r..=r {
                    if tx.abs().max(tz.abs()) != r {
                        continue;
                    }
                    let deep = (0..span / 4).all(|i| {
                        ow.preliminary_surface(
                            tx * span + i * 4,
                            tz * span + span / 2,
                        ) < SEA_LEVEL - 2
                    });
                    if !deep {
                        continue;
                    }
                    let res = build_lod2_tile(&LodTileJob {
                        key: [tx, tz],
                        level: 2,
                        ctx,
                        pack,
                        dim: "overworld".to_string(),
                    });
                    assert!(
                        !res.water.0.is_empty(),
                        "deep far tile [{tx},{tz}] must carry water",
                    );
                    return;
                }
            }
        }
        panic!("no deep far tile found within 16 tiles for seed");
    }

    #[test]
    fn skirt_quads_span_requested_heights() {
        // A 20-block drop must split into a 16 + 4 segment pair
        // (exercised in build_lod_tile); here pin the corner maths
        // of a single segment: top/bottom Y land on the corners and
        // the UV v-extent equals the segment height.
        let (corners, quad_uv) = skirt_quad(0.0, 0.0, 10.0, 4.0, 6.0, 0, -1);
        let ys: Vec<f32> = corners.iter().map(|c| c[1]).collect();
        assert!(ys.contains(&10.0) && ys.contains(&4.0));
        let vmax = quad_uv.iter().map(|q| q[1]).fold(0.0f32, f32::max);
        assert!((vmax - 6.0).abs() < 1e-6);
        // All four corners sit on the z=0 edge line.
        assert!(corners.iter().all(|c| c[2] == 0.0));
    }
}
