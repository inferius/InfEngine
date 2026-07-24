//! Liquid mesh helpers - corner heights, BFS-to-cliff distance,
//! quad-with-flow-vector emission, and the `side_open` /
//! `bottom_open` predicates the renderer uses to gate face
//! emission. Pure functions: the actual mesh build lives in
//! `main.rs::maybe_rebuild_liquid_mesh` so the free fns can be
//! unit-tested without a renderer.

use std::collections::{HashMap, HashSet, VecDeque};

use infengine_render::MeshVertex;
use infengine_voxel::{BlockId, Chunk, World, CHUNK_SIZE};

use crate::block::{
    block_is_aabb_passable, block_is_washable, block_is_waterlogged,
    block_skips_chunk_mesh,
};

/// W203: does block `id` count as part of a `liquid_id` body for
/// MESHING? MC's underwater plants are waterlogged - the cell contains
/// both the plant and water - so the water body must treat them as
/// water: no interior "air pocket" faces around a kelp stalk, the
/// surface stays continuous over kelp that reaches it, and the depth
/// fade sees through them. Applies to water only; lava has no
/// waterlogged blocks.
pub(crate) fn counts_as_liquid(id: u16, liquid_id: u16) -> bool {
    id == liquid_id
        || (liquid_id == crate::block::WATER_ID && block_is_waterlogged(id))
}

/// W358: read-only cell access for the liquid mesher. `World::get`
/// is a chunk-HashMap lookup per call, and `emit_liquid_cell` makes
/// ~20-40 of them per liquid cell (four corner heights, the depth
/// walk, six face gates) - a shore chunk with ~2k liquid cells paid
/// 40-60k hashes per rebuild, the sustained 7-11 ms `liqmesh`
/// pre-render frames while moving near water (the R19 FPS-drop
/// log). The mesher now reads through `CachedWorld`, which memoizes
/// chunk pointers; plain `World` keeps the trait impl so the sim
/// and the unit tests are unchanged.
pub(crate) trait BlockRead {
    fn get(&self, p: [i32; 3]) -> BlockId;
}

impl BlockRead for World {
    #[inline]
    fn get(&self, p: [i32; 3]) -> BlockId {
        World::get(self, p)
    }
}

/// Tiny direct-mapped chunk-pointer cache. The slot index is the
/// chunk-coord parity on each axis, so a face neighbour on ANY axis
/// lands in a different slot and never evicts the home chunk - the
/// exact access pattern of the mesher's neighbour reads. Interior
/// mutability keeps the mesher's `&self` reads free of plumbing.
/// Valid for one rebuild pass: holding `&World` borrows the world
/// immutably, so entries cannot go stale while it exists.
// W384: the frame-thread mesher is gone; the lib unit tests still
// exercise this read path.
#[allow(dead_code)]
pub(crate) struct CachedWorld<'a> {
    world: &'a World,
    slots: [std::cell::Cell<([i32; 3], Option<&'a Chunk>)>; 8],
}

impl<'a> CachedWorld<'a> {
    #[allow(dead_code)]
    pub(crate) fn new(world: &'a World) -> Self {
        Self {
            world,
            slots: std::array::from_fn(|_| {
                std::cell::Cell::new(([i32::MAX, i32::MAX, i32::MAX], None))
            }),
        }
    }
}

/// W384: owned chunk snapshot for the WORKER-side liquid mesher.
/// The dispatcher clones the target chunk + its 26 neighbours (the
/// corner-height reads touch XZ diagonals, the depth walk descends
/// up to 12 cells) so the worker never borrows the live `World`.
/// Missing chunks read as air - the snapshot union always covers
/// every cell a home-chunk emit can reach.
pub(crate) struct SnapshotWorld {
    pub(crate) chunks: std::collections::HashMap<[i32; 3], Chunk>,
}

impl BlockRead for SnapshotWorld {
    #[inline]
    fn get(&self, p: [i32; 3]) -> BlockId {
        let cs = CHUNK_SIZE as i32;
        let cc = [
            p[0].div_euclid(cs),
            p[1].div_euclid(cs),
            p[2].div_euclid(cs),
        ];
        match self.chunks.get(&cc) {
            Some(c) => c.get(
                p[0].rem_euclid(cs) as usize,
                p[1].rem_euclid(cs) as usize,
                p[2].rem_euclid(cs) as usize,
            ),
            None => BlockId::AIR,
        }
    }
}

impl BlockRead for CachedWorld<'_> {
    #[inline]
    fn get(&self, p: [i32; 3]) -> BlockId {
        let cs = CHUNK_SIZE as i32;
        let cc = [
            p[0].div_euclid(cs),
            p[1].div_euclid(cs),
            p[2].div_euclid(cs),
        ];
        let idx =
            ((cc[0] & 1) | ((cc[1] & 1) << 1) | ((cc[2] & 1) << 2)) as usize;
        let slot = &self.slots[idx];
        let (scc, sch) = slot.get();
        let chunk = if scc == cc {
            sch
        } else {
            let ch = self.world.chunk(cc);
            slot.set((cc, ch));
            ch
        };
        match chunk {
            Some(c) => c.get(
                p[0].rem_euclid(cs) as usize,
                p[1].rem_euclid(cs) as usize,
                p[2].rem_euclid(cs) as usize,
            ),
            None => BlockId::AIR,
        }
    }
}

/// Y position of the top of a liquid voxel's slope ramp. Returns 1.0
/// (full cube) for sources, for cells with the same liquid directly
/// above (column), and for the falling marker (`level = 8`); else
/// linearly interpolates from 1/9 (level 7, weakest flow) up to 7/9
/// (level 1, strongest flow). Air / solid cells return 0.0.
pub(crate) fn liquid_surface_height_at<W: BlockRead>(
    world: &W,
    level_map: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    liquid_id: u16,
) -> f32 {
    if !counts_as_liquid(world.get(pos).0, liquid_id) {
        return 0.0;
    }
    if counts_as_liquid(world.get([pos[0], pos[1] + 1, pos[2]]).0, liquid_id) {
        return 1.0;
    }
    let level = level_map.get(&pos).copied().unwrap_or(0);
    if level == 0 {
        return 1.0;
    }
    if level >= 8 {
        return 1.0;
    }
    (8 - level as i32).max(1) as f32 / 9.0
}

/// MC-style corner height: 10:1 weighted average of the four
/// touching cells' surface heights (liquid=10, air=1, solid=skip).
/// A liquid cell directly above any of the four touching cells
/// snaps the corner to 1.0 (column lock - mirrors MC's
/// `getHeight() == 1` when `hasSameAbove`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn liquid_corner_height_at<W: BlockRead>(
    world: &W,
    level_map: &HashMap<[i32; 3], u8>,
    vx: i32,
    vy: i32,
    vz: i32,
    cx: i32,
    cz: i32,
    liquid_id: u16,
) -> f32 {
    let touching = [
        [vx + cx - 1, vy, vz + cz - 1],
        [vx + cx, vy, vz + cz - 1],
        [vx + cx - 1, vy, vz + cz],
        [vx + cx, vy, vz + cz],
    ];
    // Phase 1: column-above lock. If any touching cell is part
    // of a falling column (same liquid both at this y and at
    // y+1), the corner snaps to full height.
    for t in &touching {
        if counts_as_liquid(world.get(*t).0, liquid_id)
            && counts_as_liquid(world.get([t[0], t[1] + 1, t[2]]).0, liquid_id)
        {
            return 1.0;
        }
    }
    // Phase 2: weighted blend - 10:1 liquid:air. Solids skip.
    const LIQUID_WEIGHT: f32 = 10.0;
    const AIR_WEIGHT: f32 = 1.0;
    let mut weighted_sum = 0.0_f32;
    let mut weight_total = 0.0_f32;
    for t in &touching {
        let b = world.get(*t);
        if counts_as_liquid(b.0, liquid_id) {
            let h = liquid_surface_height_at(world, level_map, *t, liquid_id);
            weighted_sum += h * LIQUID_WEIGHT;
            weight_total += LIQUID_WEIGHT;
        } else if b.is_air() {
            weight_total += AIR_WEIGHT;
        }
    }
    if weight_total <= 0.0 {
        return 0.0;
    }
    weighted_sum / weight_total
}

/// BFS over walkable cells looking for the nearest "drop" (an
/// open cell directly below). Used by `plan` to bias horizontal
/// flow toward the nearest cliff edge (MC's `getSlopeDistance`).
///
/// Mirrors MC's `canPassThrough`: traverse air, washable
/// decorations, AND same-liquid NON-SOURCE flow cells. Sources of
/// the same liquid block the path (matches MC's
/// `isSourceBlockOfThisType` gate inside `canMaybePassThrough`).
/// Without same-liquid traversal a spreader cell whose neighbours
/// are already flow cells would see u32::MAX in every direction
/// and the bias would collapse to "spread radially".
pub(crate) fn distance_to_drop_at(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    liquid_id: u16,
    start: [i32; 3],
    max_steps: u32,
) -> u32 {
    let walkable = |p: [i32; 3]| -> bool {
        let b = world.get(p);
        if b.is_air() || block_is_washable(b) {
            return true;
        }
        // Same-liquid flow cell (non-source) is walkable.
        if b.0 == liquid_id {
            let lvl = level_map.get(&p).copied().unwrap_or(0);
            return lvl > 0;
        }
        false
    };
    // The drop test: cell below is "open" (air / washable / flow).
    // A same-liquid source below does NOT count as a drop - water
    // wouldn't fall through it.
    let drop_at = |p: [i32; 3]| -> bool {
        let b = world.get(p);
        if b.is_air() || block_is_washable(b) {
            return true;
        }
        if b.0 == liquid_id {
            let lvl = level_map.get(&p).copied().unwrap_or(0);
            return lvl > 0;
        }
        false
    };
    if !walkable(start) {
        return u32::MAX;
    }
    if drop_at([start[0], start[1] - 1, start[2]]) {
        return 0;
    }
    let mut visited: HashSet<[i32; 3]> = HashSet::new();
    visited.insert(start);
    let mut queue: VecDeque<([i32; 3], u32)> = VecDeque::new();
    queue.push_back((start, 0));
    let dirs: [[i32; 3]; 4] =
        [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
    while let Some((cell, dist)) = queue.pop_front() {
        if dist >= max_steps {
            continue;
        }
        for d in &dirs {
            let n = [cell[0] + d[0], cell[1] + d[1], cell[2] + d[2]];
            if visited.contains(&n) {
                continue;
            }
            if !walkable(n) {
                continue;
            }
            visited.insert(n);
            if drop_at([n[0], n[1] - 1, n[2]]) {
                return dist + 1;
            }
            queue.push_back((n, dist + 1));
        }
    }
    u32::MAX
}

/// "Side face is open" predicate: emit a liquid side face only if
/// the neighbour is air, a passable decoration (flower / torch),
/// or a dedicated-render block (snow layer). A neighbour of the
/// SAME liquid id cancels - their shared face is fully inside the
/// body.
///
/// The `F_DEDICATED_RENDER` opt-in catches the snow layer (no
/// `F_AABB_PASS`, custom slab render) - the water cell next to a
/// snow voxel still needs its side face so the player sees the
/// pool's edge above the snow ramp instead of a hard cull.
///
/// Extracted out of the mesh build loop so a unit test can verify
/// face emission against synthetic worlds without spinning up a
/// renderer.
pub(crate) fn side_open<W: BlockRead>(
    world: &W,
    nx: i32,
    ny: i32,
    nz: i32,
    liquid_id: u16,
) -> bool {
    let nb = world.get([nx, ny, nz]);
    // W203: waterlogged plant cells count as the water body - no face
    // toward them, or every kelp stalk grows an air-pocket box.
    if counts_as_liquid(nb.0, liquid_id) {
        return false;
    }
    nb.is_air()
        || block_is_aabb_passable(nb)
        || block_skips_chunk_mesh(nb)
        // W317: snow left the skip list (it meshes as a thin slab
        // now) but still does not occlude its cell - keep the face.
        || nb.0 == 15
}

/// "Bottom face is open": emit the bottom face for a liquid cell
/// only when there's actually space underneath the player can see
/// through. Solid block below = no face. Same-liquid below = no
/// face (would be inside the body). Air / passable decoration /
/// dedicated-render block (snow) = emit so the underwater air
/// pocket under a cliff doesn't show up as transparent sky.
///
/// MC's `LiquidBlockRenderer.tesselate` emits a bottom face in
/// the same situation (`shouldRenderFace(level, pos.below(), DOWN)`).
pub(crate) fn bottom_open<W: BlockRead>(
    world: &W,
    nx: i32,
    ny: i32,
    nz: i32,
    liquid_id: u16,
) -> bool {
    let nb = world.get([nx, ny, nz]);
    // W203: same waterlogged-cell rule as `side_open`.
    if counts_as_liquid(nb.0, liquid_id) {
        return false;
    }
    nb.is_air()
        || block_is_aabb_passable(nb)
        || block_skips_chunk_mesh(nb)
        // W317: snow left the skip list (it meshes as a thin slab
        // now) but still does not occlude its cell - keep the face.
        || nb.0 == 15
}

/// Append one quad to `verts` / `idx`. Corners must be CCW from
/// the `normal` side. Two triangles, four vertices, six indices.
///
/// W200: liquid quads now carry REAL atlas coordinates for the MC
/// animated liquid texture (water_still / lava_still) instead of the
/// old repurposed-UV flow vector + procedural shader ripple. `tile` is
/// the frame-0 atlas rect `[u0, v0, u1, v1]` from `block_face_ref`;
/// `anim` is the tile-anim slot (0 = static) so the shader scrolls the
/// frame strip exactly like animated chunk-mesh tiles. Each corner gets
/// quad_uv (0,0)/(1,0)/(1,1)/(0,1) - one full texture tile per cell,
/// matching MC's per-block liquid tiling.
///
/// W199: `depth` is the normalized water-column depth at this cell
/// ([0,1] over ~12 blocks), baked into the spare light byte so the
/// shader can fade the surface from translucent (shallow) to opaque
/// (deep). Pass 0.0 for lava (opaque anyway) or non-depth quads.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_quad_flow(
    verts: &mut Vec<MeshVertex>,
    idx: &mut Vec<u32>,
    v0: [f32; 3],
    v1: [f32; 3],
    v2: [f32; 3],
    v3: [f32; 3],
    normal: [f32; 3],
    tile: [f32; 4],
    anim: u32,
    depth: f32,
) {
    let base = verts.len() as u32;
    let uv0 = [tile[0], tile[1]];
    let size = [tile[2] - tile[0], tile[3] - tile[1]];
    const CORNER_UV: [[f32; 2]; 4] =
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    for (v, cuv) in [v0, v1, v2, v3].into_iter().zip(CORNER_UV) {
        verts.push(
            MeshVertex::new(v, normal)
                .with_tile_uv(uv0, size, cuv)
                .with_anim_id(anim)
                .with_water_depth(depth),
        );
    }
    idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// W384: emit the slope-aware quads for ONE liquid cell at `p` into
/// the per-kind vertex/index buckets. Moved verbatim from
/// `Voxel::emit_liquid_cell` (app/liquid_tick.rs) and parametrized
/// over `BlockRead` + owned inputs so the WORKER pool can run it -
/// the in-place main-thread mesh was the R21 log's sustained
/// 10-12 ms `liqmesh` pre-render cost while streaming.
pub(crate) fn emit_liquid_cell<W: BlockRead>(
    w: &W,
    levels: &HashMap<[i32; 3], u8>,
    tex: &HashMap<u16, ([f32; 4], u32)>,
    p: [i32; 3],
    buckets: &mut HashMap<u16, (Vec<MeshVertex>, Vec<u32>)>,
) {
    let block = w.get(p);
    // W203: waterlogged plant cells ARE water for the liquid mesh.
    let liquid_id = if block_is_waterlogged(block.0) {
        crate::block::WATER_ID
    } else {
        block.0
    };
    if super::liquid_for(BlockId(liquid_id)).is_none() {
        return;
    }
    let (vertices, indices) = buckets.entry(liquid_id).or_default();
    let vx = p[0];
    let vy = p[1];
    let vz = p[2];
    let h00 = liquid_corner_height_at(w, levels, vx, vy, vz, 0, 0, liquid_id);
    let h10 = liquid_corner_height_at(w, levels, vx, vy, vz, 1, 0, liquid_id);
    let h11 = liquid_corner_height_at(w, levels, vx, vy, vz, 1, 1, liquid_id);
    let h01 = liquid_corner_height_at(w, levels, vx, vy, vz, 0, 1, liquid_id);
    let fx = vx as f32;
    let fy = vy as f32;
    let fz = vz as f32;
    // W200: frame-0 atlas rect + tile-anim slot of the MC animated
    // liquid texture, resolved per liquid id ON THE MAIN THREAD
    // (ResourcePack is bin-side; this module builds in the lib too).
    // Missing entry = full-tile UV, the pack-less dev-run fallback.
    let (tile, anim) = tex
        .get(&liquid_id)
        .copied()
        .unwrap_or(([0.0, 0.0, 1.0, 1.0], 0));
    // W199: normalized water-column depth for the shader's
    // depth-based surface opacity.
    let depth = {
        const DEPTH_FULL: i32 = 12;
        let mut d = 1i32;
        while d <= DEPTH_FULL
            && counts_as_liquid(w.get([vx, vy - d, vz]).0, liquid_id)
        {
            d += 1;
        }
        d as f32 / DEPTH_FULL as f32
    };
    // Top face - culled under same-liquid AND under a full opaque
    // block above (W276).
    let above = w.get([vx, vy + 1, vz]);
    let above_same = counts_as_liquid(above.0, liquid_id);
    let above_occludes =
        !above.is_air() && crate::block::block_is_opaque(above);
    if !above_same && !above_occludes {
        let t0 = [fx, fy + h00, fz];
        let t1 = [fx, fy + h01, fz + 1.0];
        let t2 = [fx + 1.0, fy + h11, fz + 1.0];
        let t3 = [fx + 1.0, fy + h10, fz];
        let normal = water_face_normal(t0, t1, t2, t3);
        push_quad_flow(
            vertices, indices, t0, t1, t2, t3, normal, tile, anim, depth,
        );
    }
    if side_open(w, vx - 1, vy, vz, liquid_id) {
        push_quad_flow(
            vertices,
            indices,
            [fx, fy, fz + 1.0],
            [fx, fy + h01, fz + 1.0],
            [fx, fy + h00, fz],
            [fx, fy, fz],
            [-1.0, 0.0, 0.0],
            tile,
            anim,
            depth,
        );
    }
    if side_open(w, vx + 1, vy, vz, liquid_id) {
        push_quad_flow(
            vertices,
            indices,
            [fx + 1.0, fy, fz],
            [fx + 1.0, fy + h10, fz],
            [fx + 1.0, fy + h11, fz + 1.0],
            [fx + 1.0, fy, fz + 1.0],
            [1.0, 0.0, 0.0],
            tile,
            anim,
            depth,
        );
    }
    if side_open(w, vx, vy, vz - 1, liquid_id) {
        push_quad_flow(
            vertices,
            indices,
            [fx, fy, fz],
            [fx, fy + h00, fz],
            [fx + 1.0, fy + h10, fz],
            [fx + 1.0, fy, fz],
            [0.0, 0.0, -1.0],
            tile,
            anim,
            depth,
        );
    }
    if side_open(w, vx, vy, vz + 1, liquid_id) {
        push_quad_flow(
            vertices,
            indices,
            [fx + 1.0, fy, fz + 1.0],
            [fx + 1.0, fy + h11, fz + 1.0],
            [fx, fy + h01, fz + 1.0],
            [fx, fy, fz + 1.0],
            [0.0, 0.0, 1.0],
            tile,
            anim,
            depth,
        );
    }
    if bottom_open(w, vx, vy - 1, vz, liquid_id) {
        push_quad_flow(
            vertices,
            indices,
            [fx, fy, fz],
            [fx + 1.0, fy, fz],
            [fx + 1.0, fy, fz + 1.0],
            [fx, fy, fz + 1.0],
            [0.0, -1.0, 0.0],
            tile,
            anim,
            depth,
        );
    }
}

/// W384: build the full liquid mesh of one chunk (CPU side) - the
/// 16^3 scan with the W198.2 raw-array liquid gate and the W198.6
/// interior-cell skip, exactly the old main-thread loop. Returns
/// only non-empty buckets.
pub(crate) fn mesh_liquid_chunk(
    snap: &SnapshotWorld,
    levels: &HashMap<[i32; 3], u8>,
    tex: &HashMap<u16, ([f32; 4], u32)>,
    cc: [i32; 3],
) -> Vec<(u16, Vec<MeshVertex>, Vec<u32>)> {
    let cs = CHUNK_SIZE as i32;
    let mut buckets: HashMap<u16, (Vec<MeshVertex>, Vec<u32>)> =
        HashMap::new();
    let (bx, by, bz) = (cc[0] * cs, cc[1] * cs, cc[2] * cs);
    if let Some(chunk) = snap.chunks.get(&cc) {
        let cu = cs as usize;
        for lx in 0..cs {
            for ly in 0..cs {
                for lz in 0..cs {
                    let (ux, uy, uz) = (lx as usize, ly as usize, lz as usize);
                    let raw = chunk.get(ux, uy, uz);
                    let watery = block_is_waterlogged(raw.0);
                    let id = if watery {
                        BlockId(crate::block::WATER_ID)
                    } else {
                        raw
                    };
                    if super::liquid_for(id).is_none() {
                        continue;
                    }
                    let interior = ux > 0
                        && uy > 0
                        && uz > 0
                        && ux < cu - 1
                        && uy < cu - 1
                        && uz < cu - 1;
                    if interior {
                        let same = |dx: usize, dy: usize, dz: usize| {
                            counts_as_liquid(chunk.get(dx, dy, dz).0, id.0)
                        };
                        if same(ux + 1, uy, uz)
                            && same(ux - 1, uy, uz)
                            && same(ux, uy + 1, uz)
                            && same(ux, uy - 1, uz)
                            && same(ux, uy, uz + 1)
                            && same(ux, uy, uz - 1)
                        {
                            continue;
                        }
                    }
                    emit_liquid_cell(
                        snap,
                        levels,
                        tex,
                        [bx + lx, by + ly, bz + lz],
                        &mut buckets,
                    );
                }
            }
        }
    }
    buckets
        .into_iter()
        .filter(|(_, (v, i))| !v.is_empty() && !i.is_empty())
        .map(|(k, (v, i))| (k, v, i))
        .collect()
}

/// Approximate face normal for the sloped water top quad. Moved
/// from `atlas.rs` (bin-side) in W384 so the lib-built mesher can
/// use it. See the W34 note: the surface is near-horizontal, the
/// only physically meaningful outward normal is +Y.
pub(crate) fn water_face_normal(
    v0: [f32; 3],
    v1: [f32; 3],
    v2: [f32; 3],
    _v3: [f32; 3],
) -> [f32; 3] {
    let _ = (v0, v1, v2);
    [0.0, 1.0, 0.0]
}
