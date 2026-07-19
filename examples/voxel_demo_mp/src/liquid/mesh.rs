//! Liquid mesh helpers - corner heights, BFS-to-cliff distance,
//! quad-with-flow-vector emission, and the `side_open` /
//! `bottom_open` predicates the renderer uses to gate face
//! emission. Pure functions: the actual mesh build lives in
//! `main.rs::maybe_rebuild_liquid_mesh` so the free fns can be
//! unit-tested without a renderer.

use std::collections::{HashMap, HashSet, VecDeque};

use infengine_render::MeshVertex;
use infengine_voxel::World;

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

/// Y position of the top of a liquid voxel's slope ramp. Returns 1.0
/// (full cube) for sources, for cells with the same liquid directly
/// above (column), and for the falling marker (`level = 8`); else
/// linearly interpolates from 1/9 (level 7, weakest flow) up to 7/9
/// (level 1, strongest flow). Air / solid cells return 0.0.
pub(crate) fn liquid_surface_height_at(
    world: &World,
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
pub(crate) fn liquid_corner_height_at(
    world: &World,
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
pub(crate) fn side_open(
    world: &World,
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
pub(crate) fn bottom_open(
    world: &World,
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
