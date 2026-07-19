//! W321 - distance LOD: far surface-shell tiles.
//!
//! Beyond the full-res view distance the world used to just end at
//! the fog curtain; VD 40 at full resolution is ~105k chunks and
//! render-bound (~45-55 FPS extrapolated, [[mc-worldgen-perf-plan]]).
//! This module adds a second ring of cheap SURFACE SHELLS: one tile
//! per chunk column (16x16), built worker-side from the cached
//! column data (`mc_floor_at` / `mc_surface_block_at`, W314 shared
//! core cache) WITHOUT generating, lighting or storing the full
//! chunk column. A tile is a greedy heightmap mesh: merged top
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

/// Job payload for `workers::Job::LodTile` - everything the worker
/// needs to build one tile off-thread.
pub(crate) struct LodTileJob {
    /// Chunk-column key `[cx, cz]`.
    pub(crate) key: [i32; 2],
    pub(crate) ctx: WorldgenCtx,
    pub(crate) pack: Arc<ResourcePack>,
    pub(crate) dim: String,
}

/// Finished tile geometry, shipped back over the worker channel.
pub(crate) struct LodTileResult {
    pub(crate) key: [i32; 2],
    pub(crate) dim: String,
    pub(crate) y_min: f32,
    pub(crate) y_max: f32,
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
    // Inner 16x16 top-block ids.
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

    // Per-tile water tint from the biome at the tile centre.
    let mcb = mcgen::terrain::mc_full_biome_at(ctx, tx * CS + 8, tz * CS + 8);
    let water_tint = mcgen::colors::water_color(mcb);

    if y_min > y_max {
        y_min = 0.0;
        y_max = 0.0;
    }
    LodTileResult {
        key: job.key,
        dim: job.dim.clone(),
        y_min,
        y_max,
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
    let x1 = x0 + 1.0;
    let z1 = z0 + 1.0;
    match (ndx, ndz) {
        // -Z edge, normal faces -Z: CCW from -Z looking toward +Z.
        (0, -1) => (
            [
                [x1, seg_bot, z0],
                [x0, seg_bot, z0],
                [x0, seg_top, z0],
                [x1, seg_top, z0],
            ],
            [[0.0, hh], [1.0, hh], [1.0, 0.0], [0.0, 0.0]],
        ),
        // +Z edge, normal +Z.
        (0, 1) => (
            [
                [x0, seg_bot, z1],
                [x1, seg_bot, z1],
                [x1, seg_top, z1],
                [x0, seg_top, z1],
            ],
            [[0.0, hh], [1.0, hh], [1.0, 0.0], [0.0, 0.0]],
        ),
        // -X edge, normal -X.
        (-1, 0) => (
            [
                [x0, seg_bot, z0],
                [x0, seg_bot, z1],
                [x0, seg_top, z1],
                [x0, seg_top, z0],
            ],
            [[0.0, hh], [1.0, hh], [1.0, 0.0], [0.0, 0.0]],
        ),
        // +X edge, normal +X.
        _ => (
            [
                [x0, seg_bot, z1],
                [x0, seg_bot, z0],
                [x0, seg_top, z0],
                [x0, seg_top, z1],
            ],
            [[0.0, hh], [1.0, hh], [1.0, 0.0], [0.0, 0.0]],
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
