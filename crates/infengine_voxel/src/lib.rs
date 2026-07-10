//! Voxel chunks + naive mesh builder.
//!
//! Phase 14 ships the minimum surface needed to build a Minecraft-like
//! game on top of InfEngine: a fixed-size `Chunk` of `BlockId` values,
//! a small `BlockRegistry` that maps block ids to descriptive metadata
//! (display name + a flat RGB colour for now; texture array support
//! lands in a later phase), and a `chunk_mesh` builder that emits
//! interior-face-culled quads ready to feed into
//! `infengine_render::Renderer::create_mesh`.
//!
//! The mesher is the standard "naive" algorithm: for every solid voxel
//! we inspect each of its six axis-aligned neighbours, and emit a quad
//! only when the neighbour is empty (or outside the chunk). Greedy
//! meshing - merging coplanar same-material quads - is a future
//! optimisation; the naive form is enough to render single chunks at
//! 16^3 (4096 voxels) without breaking a sweat.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use glam::Vec3;
use infengine_render::{MeshInstance, MeshVertex};
use serde::{Deserialize, Serialize};

/// Numeric id of a block. `0` is reserved for `BlockId::AIR`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u16);

impl BlockId {
    pub const AIR: BlockId = BlockId(0);

    pub fn is_air(self) -> bool {
        self.0 == 0
    }
}

/// Per-block metadata. The texture-array path lands later; for now each
/// block carries a single RGB colour plus PBR `(metallic, roughness)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDef {
    pub name: String,
    pub color: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
    /// `true` for blocks with alpha-cutout textures (leaves, grass,
    /// glass). Transparent blocks don't fully occlude their
    /// neighbours - chunk meshing still emits the neighbour face
    /// instead of culling it.
    #[serde(default)]
    pub transparent: bool,
}

impl BlockDef {
    pub fn solid(name: impl Into<String>, color: [f32; 3]) -> Self {
        Self {
            name: name.into(),
            color,
            metallic: 0.0,
            // 0.95 instead of the old 0.85: almost-matte default so
            // the Cook-Torrance specular peak doesn't wash out dark
            // terrain albedo. Per-block overrides applied in main.rs.
            roughness: 0.95,
            transparent: false,
        }
    }

    /// Like `solid`, but tags the block as transparent so neighbour
    /// faces aren't culled when they touch it.
    pub fn transparent(name: impl Into<String>, color: [f32; 3]) -> Self {
        Self {
            name: name.into(),
            color,
            metallic: 0.0,
            roughness: 0.95,
            transparent: true,
        }
    }

    /// Variant with explicit PBR parameters. Used by callers that
    /// tune metallic/roughness per block kind (W36.1).
    pub fn with_pbr(
        name: impl Into<String>,
        color: [f32; 3],
        transparent: bool,
        metallic: f32,
        roughness: f32,
    ) -> Self {
        Self {
            name: name.into(),
            color,
            metallic,
            roughness,
            transparent,
        }
    }
}

/// Lookup table from id -> metadata. Block 0 is hard-coded as the
/// "air" entry so registries never need to ship it.
#[derive(Debug, Default)]
pub struct BlockRegistry {
    blocks: HashMap<BlockId, BlockDef>,
}

impl BlockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: BlockId, def: BlockDef) {
        self.blocks.insert(id, def);
    }

    pub fn get(&self, id: BlockId) -> Option<&BlockDef> {
        self.blocks.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BlockId, &BlockDef)> {
        self.blocks.iter()
    }
}

/// Side length of a chunk in voxels. Cubic 16^3 chunks match the
/// Minecraft convention; experiments with 32^3 or 8^3 are possible by
/// editing this constant but the demo assumes 16.
pub const CHUNK_SIZE: usize = 16;
pub const CHUNK_VOXELS: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// 3D index of a voxel inside a chunk.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct VoxelCoord {
    pub x: usize,
    pub y: usize,
    pub z: usize,
}

/// Cubic block of voxel ids. Stored flat in row-major `[x, y, z]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    data: Vec<BlockId>,
}

impl Default for Chunk {
    fn default() -> Self {
        Self::empty()
    }
}

impl Chunk {
    pub fn empty() -> Self {
        Self {
            data: vec![BlockId::AIR; CHUNK_VOXELS],
        }
    }

    pub fn filled(block: BlockId) -> Self {
        Self {
            data: vec![block; CHUNK_VOXELS],
        }
    }

    fn index(x: usize, y: usize, z: usize) -> usize {
        // Flat row-major layout. Bounds checks happen in `get` / `set`.
        x + CHUNK_SIZE * (y + CHUNK_SIZE * z)
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        if x >= CHUNK_SIZE || y >= CHUNK_SIZE || z >= CHUNK_SIZE {
            return BlockId::AIR;
        }
        self.data[Self::index(x, y, z)]
    }

    pub fn set(&mut self, x: usize, y: usize, z: usize, block: BlockId) {
        if x >= CHUNK_SIZE || y >= CHUNK_SIZE || z >= CHUNK_SIZE {
            return;
        }
        self.data[Self::index(x, y, z)] = block;
    }

    /// Apply `f` to every (x, y, z) in raster order. Useful for
    /// procedurally filling a chunk from a height function.
    pub fn fill_with(&mut self, mut f: impl FnMut(usize, usize, usize) -> BlockId) {
        for z in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    self.data[Self::index(x, y, z)] = f(x, y, z);
                }
            }
        }
    }

    pub fn raw(&self) -> &[BlockId] {
        &self.data
    }
}

/// One mesh emitted by the builder, grouped by block id so each call
/// can be dispatched with a single instance colour through the PBR
/// pipeline.
#[derive(Debug)]
pub struct ChunkSubmesh {
    pub block: BlockId,
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

/// Build per-block submeshes for a chunk. Faces between two solid
/// voxels are skipped; the boundary with the world outside the chunk
/// (treated as air) emits faces.
///
/// `origin` is the world-space corner of the chunk - vertex positions
/// are emitted in world space, so the caller can either feed the mesh
/// in directly or use an identity model matrix when drawing.
pub fn chunk_mesh(chunk: &Chunk, origin: Vec3) -> Vec<ChunkSubmesh> {
    // Back-compat shim: every face samples the full 0..1 UV rect
    // and every non-air block is treated as opaque.
    chunk_mesh_with_atlas(chunk, origin, |_, _| [0.0, 0.0, 1.0, 1.0], |_| true)
}

/// Like `chunk_mesh`, but each face's UVs are pre-scaled into the
/// `[u0, v0, u1, v1]` rect that `uv_rect(block, normal)` returns.
/// `is_opaque(block)` lets the caller mark transparent blocks
/// (leaves, glass) so their neighbour faces stay visible instead
/// of getting occluded. A pair of touching identical transparent
/// blocks still culls the shared face to avoid in-cluster
/// over-draw.
///
/// The lighting attribute on every emitted vertex stays at the
/// `MeshVertex::new` default (`[0.0, 1.0]` = no block light, full
/// sky). Phase L wires up a `_with_light` variant that overrides
/// this from a per-chunk `LightGrid`.
pub fn chunk_mesh_with_atlas<F, O>(
    chunk: &Chunk,
    origin: Vec3,
    uv_rect: F,
    is_opaque: O,
) -> Vec<ChunkSubmesh>
where
    F: Fn(BlockId, [i8; 3]) -> [f32; 4],
    O: Fn(BlockId) -> bool,
{
    chunk_mesh_with_atlas_and_light(chunk, origin, uv_rect, is_opaque, |_, _| [0.0, 1.0])
}

/// Like `chunk_mesh_with_atlas` but each emitted face samples the
/// `light_at(local_air_cell, normal)` closure to produce a
/// `(block_light_norm, sky_light_norm)` pair written into every
/// vertex of that face.
///
/// `local_air_cell` is the local-chunk-coord of the air cell the
/// face is FACING (i.e. the cell on the OUTSIDE of the solid voxel,
/// where the player's camera looks at). For boundary faces against
/// a non-existent chunk-outside neighbour, the closure is invoked
/// with the in-chunk cell address even though the resulting light
/// will be ambient at best - callers that want correct boundary
/// lighting have to fold in neighbour chunks themselves.
pub fn chunk_mesh_with_atlas_and_light<F, O, L>(
    chunk: &Chunk,
    origin: Vec3,
    uv_rect: F,
    is_opaque: O,
    light_at: L,
) -> Vec<ChunkSubmesh>
where
    F: Fn(BlockId, [i8; 3]) -> [f32; 4],
    O: Fn(BlockId) -> bool,
    L: Fn([i32; 3], [i8; 3]) -> [f32; 2],
{
    // W9.y.4 shim: callers without an animation table get
    // `anim_id = 0` on every vertex (static UV). The
    // `*_and_anim` variant below is the real workhorse.
    chunk_mesh_with_atlas_and_light_and_anim(
        chunk,
        origin,
        uv_rect,
        is_opaque,
        light_at,
        |_, _| 0u32,
    )
}

/// Like `chunk_mesh_with_atlas_and_light` but each emitted face
/// also samples an `anim_id_at(block, normal)` closure to stamp
/// the vertex's `anim_id` slot (Wave 9.y). `0` keeps the face
/// static; `1..=32` indexes into the renderer's `tile_anim`
/// uniform array so the fragment shader walks through the
/// tile's frame strip.
#[allow(clippy::too_many_arguments)]
pub fn chunk_mesh_with_atlas_and_light_and_anim<F, O, L, A>(
    chunk: &Chunk,
    origin: Vec3,
    uv_rect: F,
    is_opaque: O,
    light_at: L,
    anim_id_at: A,
) -> Vec<ChunkSubmesh>
where
    F: Fn(BlockId, [i8; 3]) -> [f32; 4],
    O: Fn(BlockId) -> bool,
    L: Fn([i32; 3], [i8; 3]) -> [f32; 2],
    A: Fn(BlockId, [i8; 3]) -> u32,
{
    // W16.2 shim: no special-model dispatch; every block uses the
    // legacy cube path.  The `*_and_special` variant below is the
    // real workhorse after W16.2.
    chunk_mesh_with_atlas_light_anim_and_special(
        chunk,
        origin,
        uv_rect,
        is_opaque,
        light_at,
        anim_id_at,
        |_block, _voxel_origin, _verts, _idxs| false,
    )
}

/// W28.4: Greedy mesher - same API as
/// `chunk_mesh_with_atlas_and_light_and_anim` but merges coplanar
/// same-material faces into larger quads.
///
/// Merging criterion: two adjacent faces on the same slice can be
/// merged when they share the same `block`, `light`, `anim_id`, and
/// `uv_rect`. Tiled atlas textures scale naturally - the merged
/// quad's UV range is widened proportionally. Animated or uniquely-lit
/// faces (e.g. torchlit surfaces) are not merged because their per-face
/// values differ.
///
/// Special-shaped blocks (torches, slabs, ...) do NOT go through this
/// path - call `chunk_mesh_with_atlas_light_anim_and_special` for those
/// and add the greedy quads on top if needed.
///
/// Performance: for a fully-uniform chunk this reduces face count
/// from 6*N to 6 (one quad per face direction). Typical terrain
/// (mixed blocks) sees a 30-60% reduction in opaque face count.
pub fn chunk_mesh_greedy<F, O, L, A>(
    chunk: &Chunk,
    origin: Vec3,
    uv_rect: F,
    is_opaque: O,
    light_at: L,
    anim_id_at: A,
) -> Vec<ChunkSubmesh>
where
    F: Fn(BlockId, [i8; 3]) -> [f32; 4],
    O: Fn(BlockId) -> bool,
    L: Fn([i32; 3], [i8; 3]) -> [f32; 2],
    A: Fn(BlockId, [i8; 3]) -> u32,
{
    let cs = CHUNK_SIZE;
    let mut per_block: HashMap<BlockId, (Vec<MeshVertex>, Vec<u32>)> = HashMap::new();

    // Iterate over the 6 face directions. For each direction we sweep
    // slices perpendicular to the normal and run the greedy algorithm
    // on each 2D slice.
    //
    // Axes: face_axis selects which chunk axis the normal points along.
    // u_axis / v_axis are the two axes spanning the slice plane.
    // For a face with normal along +X: face_axis=0, u_axis=1(Y), v_axis=2(Z)
    // The slice index `d` steps from 0 to cs along face_axis.
    for face in FACES {
        // W30 fix: pick (ua, va) so ua x va = +face_axis (right-hand rule).
        // X face: Y x Z = +X. Y face: Z x X = +Y. Z face: X x Y = +Z.
        // The previous Y mapping (0, 2) produced X x Z = -Y which inverted
        // the winding for +/-Y faces -> top/bottom faces of every chunk were
        // emitted as backfaces and culled.
        let (fa, ua, va) = match face.normal {
            [n, 0, 0] if n != 0 => (0, 1, 2),
            [0, n, 0] if n != 0 => (1, 2, 0),
            _ => (2, 0, 1),
        };
        let side = if face.normal[fa] > 0 { 1i32 } else { -1i32 };

        // mask[u][v] = Some((block, light_packed, anim_id, uv_rect_tile)) or None
        // light_packed: round to 16 steps so tiny float noise doesn't prevent merging
        #[derive(Clone, PartialEq, Eq)]
        struct FaceKey {
            block: u16,
            // light packed to u8 each (0-15 steps)
            bl: u8,
            sl: u8,
            anim: u32,
            // uv tile index - quantise [u0,v0,u1,v1] to u16 * 65535
            uv0u: u16,
            uv0v: u16,
            uv1u: u16,
            uv1v: u16,
        }

        let mut mask: Vec<Option<FaceKey>> = vec![None; cs * cs];
        // Also store the raw (non-quantised) uv_rect and light for the
        // first cell of a run (they're all equal after quantisation).
        let mut mask_uv: Vec<[f32; 4]> = vec![[0.0; 4]; cs * cs];
        let mut mask_light: Vec<[f32; 2]> = vec![[0.0; 2]; cs * cs];

        for d in 0..cs {
            // Fill mask for slice d.
            for u in 0..cs {
                for v in 0..cs {
                    let mut coord = [0usize; 3];
                    coord[fa] = d;
                    coord[ua] = u;
                    coord[va] = v;
                    let block = chunk.get(coord[0], coord[1], coord[2]);
                    if block.is_air() {
                        mask[u * cs + v] = None;
                        continue;
                    }

                    // Neighbour in the face direction
                    let mut nc = [coord[0] as i32, coord[1] as i32, coord[2] as i32];
                    nc[fa] += side;
                    let nb = if (0..cs as i32).contains(&nc[fa]) {
                        chunk.get(nc[0] as usize, nc[1] as usize, nc[2] as usize)
                    } else {
                        BlockId::AIR
                    };

                    // Culling: same rules as naive mesher
                    let visible = if nb.is_air() {
                        true
                    } else if is_opaque(nb) {
                        false
                    } else {
                        nb != block
                    };
                    if !visible {
                        mask[u * cs + v] = None;
                        continue;
                    }

                    let air_coord = [nc[0], nc[1], nc[2]];
                    let light = light_at(air_coord, face.normal);
                    let anim = anim_id_at(block, face.normal);
                    let uv = uv_rect(block, face.normal);

                    // Pack key for merge decision
                    mask_uv[u * cs + v] = uv;
                    mask_light[u * cs + v] = light;
                    mask[u * cs + v] = Some(FaceKey {
                        block: block.0,
                        bl: (light[0].clamp(0.0, 1.0) * 15.0).round() as u8,
                        sl: (light[1].clamp(0.0, 1.0) * 15.0).round() as u8,
                        anim,
                        uv0u: (uv[0].clamp(0.0, 1.0) * 65535.0) as u16,
                        uv0v: (uv[1].clamp(0.0, 1.0) * 65535.0) as u16,
                        uv1u: (uv[2].clamp(0.0, 1.0) * 65535.0) as u16,
                        uv1v: (uv[3].clamp(0.0, 1.0) * 65535.0) as u16,
                    });
                }
            }

            // Greedy scan of the filled mask.
            let mut done: Vec<bool> = vec![false; cs * cs];
            for u0 in 0..cs {
                for v0 in 0..cs {
                    let idx0 = u0 * cs + v0;
                    if done[idx0] || mask[idx0].is_none() {
                        continue;
                    }
                    let key = mask[idx0].clone().unwrap();
                    let block = BlockId(key.block);

                    // Expand along v axis first.
                    let mut v1 = v0 + 1;
                    while v1 < cs {
                        let idx = u0 * cs + v1;
                        if done[idx] || mask[idx].as_ref() != Some(&key) {
                            break;
                        }
                        v1 += 1;
                    }

                    // Expand along u axis, checking the full v strip.
                    let mut u1 = u0 + 1;
                    'u_expand: while u1 < cs {
                        for v in v0..v1 {
                            let idx = u1 * cs + v;
                            if done[idx] || mask[idx].as_ref() != Some(&key) {
                                break 'u_expand;
                            }
                        }
                        u1 += 1;
                    }

                    // Mark the rectangle [u0..u1] x [v0..v1] as done.
                    for u in u0..u1 {
                        for v in v0..v1 {
                            done[u * cs + v] = true;
                        }
                    }

                    let w = (u1 - u0) as f32; // width in voxels
                    let h = (v1 - v0) as f32; // height in voxels

                    // Build world-space corners. The face normal axis is
                    // fixed at `d` (or `d+1` for positive face), the u/v
                    // axes span the rectangle.
                    let d_f = d as f32;
                    let face_offset = if side > 0 { 1.0f32 } else { 0.0f32 };
                    let u0_f = u0 as f32;
                    let v0_f = v0 as f32;

                    // Four corners in CCW order when viewed from
                    // the positive normal direction. The corner order
                    // matches `FACES` so the winding is consistent.
                    let corner_pos = |du: f32, dv: f32| -> [f32; 3] {
                        let mut c = [0.0f32; 3];
                        c[fa] = d_f + face_offset;
                        c[ua] = u0_f + du;
                        c[va] = v0_f + dv;
                        [
                            origin.x + c[0],
                            origin.y + c[1],
                            origin.z + c[2],
                        ]
                    };

                    // W28.4 fix: corner order must match emit_element_quad's
                    // index winding [0,2,1, 0,3,2] producing CCW triangles
                    // viewed from the face normal direction. The original
                    // ordering had corners 1 and 3 swapped, which inverted
                    // the cross product and turned every greedy quad into
                    // a backface (culled by FrontFace::Ccw + cull_mode::Back).
                    let corners = if side > 0 {
                        [
                            corner_pos(0.0, 0.0),
                            corner_pos(0.0, h  ),
                            corner_pos(w,   h  ),
                            corner_pos(w,   0.0),
                        ]
                    } else {
                        [
                            corner_pos(w,   0.0),
                            corner_pos(w,   h  ),
                            corner_pos(0.0, h  ),
                            corner_pos(0.0, 0.0),
                        ]
                    };

                    // W30.3: use tile-fract emit so UV stays within the atlas
                    // tile rect even for merged quads spanning multiple voxels.
                    let base_uv = mask_uv[idx0];
                    let light = mask_light[idx0];
                    let [u0t, v0t, u1t, v1t] = base_uv;
                    let tile_uv0 = [u0t, v0t];
                    let tile_size = [u1t - u0t, v1t - v0t];
                    // W34.2: explicit per-face quad_uv tables derived by
                    // matching each greedy corner position to the OLD
                    // (per-voxel) mesher's UV assignment.
                    //
                    // OLD mesher UV: uvs = [(u0,v1),(u1,v1),(u1,v0),(u0,v0)]
                    // Shader: final_uv = tile_uv0 + fract(quad_uv) * tile_size
                    // So fract(quad_uv[i]) == (uvs[i] - [u0,v0]) / tile_size.
                    //
                    // Corners for side>0: [(0,0),(0,h),(w,h),(w,0)]
                    // Corners for side<0: [(w,0),(w,h),(0,h),(0,0)]
                    //
                    // Derivation per face (fa=face axis, ua=u axis, va=v axis):
                    //
                    // +X (fa=0,ua=1=Y,va=2=Z), side>0 = OLD order matched:
                    //   frac_u=Z=dv, frac_v=1-Y=1-du
                    //   C0(0,0):[0,w-E]  C1(0,h):[h-E,w-E]  C2(w,h):[h-E,0]  C3(w,0):[0,0]
                    //
                    // -X (fa=0,ua=1=Y,va=2=Z), side<0, greedy != OLD order:
                    //   frac_u=1-Z=1-dv, frac_v=1-Y=1-du
                    //   C0(w,0):[h-E,0]  C1(w,h):[0,0]  C2(0,h):[0,w-E]  C3(0,0):[h-E,w-E]
                    //
                    // +Y (fa=1,ua=2=Z,va=0=X), side>0 = OLD order matched:
                    //   frac_u=X=dv, frac_v=1-Z=1-du
                    //   C0(0,0):[0,w-E]  C1(0,h):[h-E,w-E]  C2(w,h):[h-E,0]  C3(w,0):[0,0]
                    //
                    // -Y (fa=1,ua=2=Z,va=0=X), side<0 = OLD order matched:
                    //   frac_u=X=dv, frac_v=1-Z=1-du
                    //   C0(w,0):[0,w-E]  C1(w,h):[h-E,w-E]  C2(0,h):[h-E,0]  C3(0,0):[0,0]
                    //   (same formula as +X/+Y because -Y side<0 corners happen to
                    //   map to the same (du,dv)->(frac_u,frac_v) relationship)
                    //
                    // +Z (fa=2,ua=0=X,va=1=Y), side>0, greedy != OLD order:
                    //   frac_u=1-X=1-du, frac_v=1-Y=1-dv
                    //   C0(0,0):[w-E,h-E]  C1(0,h):[w-E,0]  C2(w,h):[0,0]  C3(w,0):[0,h-E]
                    //
                    // -Z (fa=2,ua=0=X,va=1=Y), side<0, greedy != OLD order:
                    //   frac_u=X=du, frac_v=1-Y=1-dv
                    //   C0(w,0):[w-E,h-E]  C1(w,h):[w-E,0]  C2(0,h):[0,0]  C3(0,0):[0,h-E]
                    const E: f32 = 0.004; // W229.1: symmetric inset, must exceed the u16 UV quantization error
                    let quad_uv_corners: [[f32; 2]; 4] = match face.normal {
                        // +X, +Y, -Y: C0=(0,0),C1=(0,h),C2=(w,h),C3=(w,0)
                        //   quad_uv = [dv_edge, (w-du)_edge]
                        [1, 0, 0] | [0, 1, 0] => [
                            [E,     w - E],
                            [h - E, w - E],
                            [h - E, E    ],
                            [E,     E    ],
                        ],
                        // -Y: C0=(w,0),C1=(w,h),C2=(0,h),C3=(0,0)
                        //   same texture projection as group above
                        [0, -1, 0] => [
                            [E,     w - E],
                            [h - E, w - E],
                            [h - E, E    ],
                            [E,     E    ],
                        ],
                        // -X: C0=(w,0),C1=(w,h),C2=(0,h),C3=(0,0)
                        //   frac_u=1-dv (inverted Z), frac_v=1-du (inverted Y)
                        [-1, 0, 0] => [
                            [h - E, E    ],
                            [E,     E    ],
                            [E,     w - E],
                            [h - E, w - E],
                        ],
                        // +Z: C0=(0,0),C1=(0,h),C2=(w,h),C3=(w,0)
                        //   frac_u=1-du (inverted X), frac_v=1-dv (inverted Y)
                        [0, 0, 1] => [
                            [w - E, h - E],
                            [w - E, E    ],
                            [E,     E    ],
                            [E,     h - E],
                        ],
                        // -Z: C0=(w,0),C1=(w,h),C2=(0,h),C3=(0,0)
                        //   frac_u=du (X), frac_v=1-dv (inverted Y)
                        _ => [
                            [w - E, h - E],
                            [w - E, E    ],
                            [E,     E    ],
                            [E,     h - E],
                        ],
                    };

                    let normal_f32 = [
                        face.normal[0] as f32,
                        face.normal[1] as f32,
                        face.normal[2] as f32,
                    ];

                    let entry = per_block
                        .entry(block)
                        .or_insert_with(|| (Vec::with_capacity(64), Vec::with_capacity(96)));

                    emit_element_quad_tiled(
                        &mut entry.0,
                        &mut entry.1,
                        corners,
                        normal_f32,
                        tile_uv0,
                        tile_size,
                        quad_uv_corners,
                        light,
                        key.anim,
                    );
                }
            }
        }
    }

    per_block
        .into_iter()
        .map(|(block, (vertices, indices))| ChunkSubmesh {
            block,
            vertices,
            indices,
        })
        .collect()
}

/// W28.4: Greedy mesher with special-model support. Same API as
/// `chunk_mesh_with_atlas_light_anim_and_special` but uses greedy
/// merging for regular cube blocks. Special-shaped blocks handled by
/// `emit_special` are emitted exactly once (the cube path skips them).
///
/// This is the recommended replacement for
/// `chunk_mesh_with_atlas_light_anim_and_special` in performance-
/// sensitive paths.
///
/// W122: `occluder_at(local_coord)` reports whether the voxel at a
/// chunk-local coordinate (which may be out of `[0, CHUNK_SIZE)` for
/// neighbour cells) is a full AO occluder. It drives per-corner
/// ambient occlusion + smooth-light averaging. Callers that can resolve
/// neighbour chunks should answer across borders; otherwise return
/// `false` out of range (no contact shadow at the chunk seam, but no
/// artefact either).
#[allow(clippy::too_many_arguments)]
pub fn chunk_mesh_greedy_with_special<F, O, L, A, S, OC>(
    chunk: &Chunk,
    origin: Vec3,
    uv_rect: F,
    is_opaque: O,
    light_at: L,
    anim_id_at: A,
    occluder_at: OC,
    mut emit_special: S,
) -> Vec<ChunkSubmesh>
where
    F: Fn(BlockId, [i8; 3]) -> [f32; 4],
    O: Fn(BlockId) -> bool,
    L: Fn([i32; 3], [i8; 3]) -> [f32; 2],
    A: Fn(BlockId, [i8; 3]) -> u32,
    OC: Fn([i32; 3]) -> bool,
    S: FnMut(BlockId, Vec3, &mut Vec<MeshVertex>, &mut Vec<u32>) -> bool,
{
    let cs = CHUNK_SIZE;
    let mut per_block: HashMap<BlockId, (Vec<MeshVertex>, Vec<u32>)> = HashMap::new();

    // First pass: call emit_special for every non-air voxel.
    // Track which voxels are "special" (handled by emit_special) in
    // a flat bitset so the greedy pass can skip them.
    let mut special_mask = vec![false; cs * cs * cs];
    for z in 0..cs {
        for y in 0..cs {
            for x in 0..cs {
                let block = chunk.get(x, y, z);
                if block.is_air() {
                    continue;
                }
                let p = Vec3::new(
                    origin.x + x as f32,
                    origin.y + y as f32,
                    origin.z + z as f32,
                );
                let entry = per_block
                    .entry(block)
                    .or_insert_with(|| (Vec::with_capacity(64), Vec::with_capacity(96)));
                if emit_special(block, p, &mut entry.0, &mut entry.1) {
                    special_mask[x + cs * (y + cs * z)] = true;
                }
            }
        }
    }

    // Second pass: greedy meshing for non-special voxels.
    // Mirrors chunk_mesh_greedy but skips cells in special_mask.
    for face in FACES {
        // W30 fix: see chunk_mesh_greedy for the Y-axis handedness rationale.
        let (fa, ua, va) = match face.normal {
            [n, 0, 0] if n != 0 => (0, 1, 2),
            [0, n, 0] if n != 0 => (1, 2, 0),
            _ => (2, 0, 1),
        };
        let side = if face.normal[fa] > 0 { 1i32 } else { -1i32 };

        // W122: the merge key carries all four corners' quantised
        // (block, sky, ao) so two cells merge only when their smooth
        // lighting + AO is identical. Flat fully-lit faces (corners all
        // equal) still merge into big quads; gradients/contact corners
        // split to per-cell, exactly where MC wants the detail.
        #[derive(Clone, PartialEq, Eq)]
        struct FaceKey {
            block: u16,
            corners: [[u8; 3]; 4],
            anim: u32,
            uv0u: u16, uv0v: u16, uv1u: u16, uv1v: u16,
        }

        let mut mask: Vec<Option<FaceKey>> = vec![None; cs * cs];
        let mut mask_uv: Vec<[f32; 4]> = vec![[0.0; 4]; cs * cs];
        // Per-corner (block, sky, ao) in [c00, c01, c10, c11] order
        // (cu along ua, cv along va).
        let mut mask_corner: Vec<[[f32; 3]; 4]> = vec![[[0.0; 3]; 4]; cs * cs];

        for d in 0..cs {
            for u in 0..cs {
                for v in 0..cs {
                    let mut coord = [0usize; 3];
                    coord[fa] = d; coord[ua] = u; coord[va] = v;
                    let (cx, cy, cz) = (coord[0], coord[1], coord[2]);
                    let block = chunk.get(cx, cy, cz);
                    if block.is_air() || special_mask[cx + cs * (cy + cs * cz)] {
                        mask[u * cs + v] = None;
                        continue;
                    }
                    let mut nc = [cx as i32, cy as i32, cz as i32];
                    nc[fa] += side;
                    let nb = if (0..cs as i32).contains(&nc[fa]) {
                        chunk.get(nc[0] as usize, nc[1] as usize, nc[2] as usize)
                    } else {
                        BlockId::AIR
                    };
                    let visible = if nb.is_air() {
                        true
                    } else if is_opaque(nb) {
                        false
                    } else {
                        nb != block
                    };
                    if !visible {
                        mask[u * cs + v] = None;
                        continue;
                    }
                    let anim = anim_id_at(block, face.normal);
                    let uv = uv_rect(block, face.normal);
                    // W122: per-corner smooth light + AO. `nc` is the
                    // air-side cell; for each corner sample the 2x2
                    // air-side neighbourhood around it (the two edge
                    // neighbours + the diagonal). AO follows the
                    // standard "3 - occluders, 0 if both sides solid"
                    // rule; smooth light averages the non-occluder
                    // cells so light bleeds across the corner.
                    let corner_val = |cu: i32, cv: i32| -> [f32; 3] {
                        let su = if cu == 0 { -1 } else { 1 };
                        let sv = if cv == 0 { -1 } else { 1 };
                        let mut side_u = nc; side_u[ua] += su;
                        let mut side_v = nc; side_v[va] += sv;
                        let mut diag = nc; diag[ua] += su; diag[va] += sv;
                        let o_n = occluder_at(nc);
                        let o_su = occluder_at(side_u);
                        let o_sv = occluder_at(side_v);
                        let o_d = occluder_at(diag);
                        let level = if o_su && o_sv {
                            0u8
                        } else {
                            3 - (o_su as u8 + o_sv as u8 + o_d as u8)
                        };
                        let ao = match level {
                            0 => 0.5,
                            1 => 0.7,
                            2 => 0.85,
                            _ => 1.0,
                        };
                        let mut bsum = 0.0f32;
                        let mut ssum = 0.0f32;
                        let mut cnt = 0u32;
                        for (cell, occ) in
                            [(nc, o_n), (side_u, o_su), (side_v, o_sv), (diag, o_d)]
                        {
                            if !occ {
                                let l = light_at(cell, face.normal);
                                bsum += l[0];
                                ssum += l[1];
                                cnt += 1;
                            }
                        }
                        let (bl, sl) = if cnt > 0 {
                            (bsum / cnt as f32, ssum / cnt as f32)
                        } else {
                            let l = light_at(nc, face.normal);
                            (l[0], l[1])
                        };
                        [bl, sl, ao]
                    };
                    let c00 = corner_val(0, 0);
                    let c01 = corner_val(0, 1);
                    let c10 = corner_val(1, 0);
                    let c11 = corner_val(1, 1);
                    let mk = |c: &[f32; 3]| -> [u8; 3] {
                        [
                            (c[0].clamp(0.0, 1.0) * 15.0).round() as u8,
                            (c[1].clamp(0.0, 1.0) * 15.0).round() as u8,
                            (c[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                        ]
                    };
                    mask_uv[u * cs + v] = uv;
                    mask_corner[u * cs + v] = [c00, c01, c10, c11];
                    mask[u * cs + v] = Some(FaceKey {
                        block: block.0,
                        corners: [mk(&c00), mk(&c01), mk(&c10), mk(&c11)],
                        anim,
                        uv0u: (uv[0].clamp(0.0, 1.0) * 65535.0) as u16,
                        uv0v: (uv[1].clamp(0.0, 1.0) * 65535.0) as u16,
                        uv1u: (uv[2].clamp(0.0, 1.0) * 65535.0) as u16,
                        uv1v: (uv[3].clamp(0.0, 1.0) * 65535.0) as u16,
                    });
                }
            }

            for u0 in 0..cs {
                for v0 in 0..cs {
                    let idx0 = u0 * cs + v0;
                    if mask[idx0].is_none() { continue; }
                    let key = mask[idx0].clone().unwrap();
                    let block = BlockId(key.block);

                    // W226.2: NO greedy expansion - unit quads only.
                    // Merging produced quads of different sizes sharing
                    // partial edges (W122 splits runs at smooth-light
                    // steps); a vertex mid-edge of a neighbouring quad
                    // (T-junction) rasterizes with subpixel pinholes -
                    // the dotted see-through lines on tree trunks. MC
                    // itself emits per-face unit quads: bitwise-equal
                    // shared corners make the surface watertight by
                    // construction. Re-enabling merging requires a
                    // T-junction-free design (weld neighbour vertices
                    // into long edges) - tracked in the perf plan.
                    let u1 = u0 + 1;
                    let v1 = v0 + 1;

                    let w = (u1 - u0) as f32;
                    let h = (v1 - v0) as f32;
                    let d_f = d as f32;
                    let face_offset = if side > 0 { 1.0f32 } else { 0.0f32 };
                    let corner_pos = |du: f32, dv: f32| -> [f32; 3] {
                        let mut c = [0.0f32; 3];
                        c[fa] = d_f + face_offset;
                        c[ua] = u0 as f32 + du;
                        c[va] = v0 as f32 + dv;
                        [origin.x + c[0], origin.y + c[1], origin.z + c[2]]
                    };
                    // W28.4 fix: same winding fix as chunk_mesh_greedy -
                    // swap corners 1 and 3 so the cross product points along
                    // the face normal rather than against it.
                    let corners = if side > 0 {
                        [corner_pos(0.0,0.0), corner_pos(0.0,h), corner_pos(w,h), corner_pos(w,0.0)]
                    } else {
                        [corner_pos(w,0.0), corner_pos(w,h), corner_pos(0.0,h), corner_pos(0.0,0.0)]
                    };
                    // W30.3 + W34.2: tile-fract emit with per-face UV table.
                    let base_uv = mask_uv[idx0];
                    // W122: map the anchor cell's 4 corner values
                    // ([c00, c01, c10, c11]) onto the emitted vertex
                    // order (which mirrors `corners` above). All merged
                    // cells share this key's corners, so the anchor's
                    // values are the run's outer-corner values.
                    let cc = mask_corner[idx0];
                    let corner_light = if side > 0 {
                        [cc[0], cc[1], cc[3], cc[2]]
                    } else {
                        [cc[2], cc[3], cc[1], cc[0]]
                    };
                    let [u0t, v0t, u1t, v1t] = base_uv;
                    let tile_uv0 = [u0t, v0t];
                    let tile_size = [u1t - u0t, v1t - v0t];
                    // W34.2: same explicit per-face table as chunk_mesh_greedy.
                    // See that function for the full derivation comment.
                    const E: f32 = 0.004; // W229.1: symmetric inset, must exceed the u16 UV quantization error
                    let quad_uv_corners: [[f32; 2]; 4] = match face.normal {
                        [1, 0, 0] | [0, 1, 0] => [
                            [E,     w - E],
                            [h - E, w - E],
                            [h - E, E    ],
                            [E,     E    ],
                        ],
                        [0, -1, 0] => [
                            [E,     w - E],
                            [h - E, w - E],
                            [h - E, E    ],
                            [E,     E    ],
                        ],
                        [-1, 0, 0] => [
                            [h - E, E    ],
                            [E,     E    ],
                            [E,     w - E],
                            [h - E, w - E],
                        ],
                        [0, 0, 1] => [
                            [w - E, h - E],
                            [w - E, E    ],
                            [E,     E    ],
                            [E,     h - E],
                        ],
                        _ => [
                            [w - E, h - E],
                            [w - E, E    ],
                            [E,     E    ],
                            [E,     h - E],
                        ],
                    };
                    let nf = [face.normal[0] as f32, face.normal[1] as f32, face.normal[2] as f32];
                    let entry = per_block
                        .entry(block)
                        .or_insert_with(|| (Vec::with_capacity(64), Vec::with_capacity(96)));
                    emit_element_quad_tiled_corners(
                        &mut entry.0, &mut entry.1,
                        corners, nf,
                        tile_uv0, tile_size, quad_uv_corners,
                        corner_light, key.anim,
                    );
                }
            }
        }
    }

    per_block
        .into_iter()
        .map(|(block, (vertices, indices))| ChunkSubmesh { block, vertices, indices })
        .collect()
}

/// Like `chunk_mesh_with_atlas_and_light_and_anim` but an extra
/// `emit_special` closure gives the caller a chance to override
/// the default 6-face cube geometry for special-shaped blocks
/// (torches, slabs, stairs, cross-pattern plants, ...).
///
/// `emit_special(block, voxel_world_origin, vertices, indices)`
/// is called BEFORE the cube path for every non-air voxel.
/// If the closure returns `true`, the cube path is skipped for
/// that voxel (the closure took responsibility for emitting all
/// geometry). Returning `false` falls through to the standard
/// 6-face cube emission.
///
/// The `light_at` and `anim_id_at` callbacks are NOT forwarded to
/// `emit_special`; the closure is expected to capture them (or
/// equivalent) from the outer scope when it needs lighting/anim.
#[allow(clippy::too_many_arguments)]
pub fn chunk_mesh_with_atlas_light_anim_and_special<F, O, L, A, S>(
    chunk: &Chunk,
    origin: Vec3,
    uv_rect: F,
    is_opaque: O,
    light_at: L,
    anim_id_at: A,
    mut emit_special: S,
) -> Vec<ChunkSubmesh>
where
    F: Fn(BlockId, [i8; 3]) -> [f32; 4],
    O: Fn(BlockId) -> bool,
    L: Fn([i32; 3], [i8; 3]) -> [f32; 2],
    A: Fn(BlockId, [i8; 3]) -> u32,
    S: FnMut(BlockId, Vec3, &mut Vec<MeshVertex>, &mut Vec<u32>) -> bool,
{
    let mut per_block: HashMap<BlockId, (Vec<MeshVertex>, Vec<u32>)> = HashMap::new();

    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let block = chunk.get(x, y, z);
                if block.is_air() {
                    continue;
                }
                let p = Vec3::new(
                    origin.x + x as f32,
                    origin.y + y as f32,
                    origin.z + z as f32,
                );
                let entry = per_block
                    .entry(block)
                    .or_insert_with(|| (Vec::with_capacity(64), Vec::with_capacity(96)));

                // W16.2: let the caller override geometry for
                // special-shaped blocks (torch, slab, cross, ...).
                // If the closure signals it handled this voxel,
                // skip the default 6-face cube path entirely.
                if emit_special(block, p, &mut entry.0, &mut entry.1) {
                    continue;
                }

                // For each axis-aligned neighbour, emit a quad when
                // the neighbour is air.
                for face in FACES {
                    let (nx, ny, nz) = (
                        (x as i32) + face.normal[0] as i32,
                        (y as i32) + face.normal[1] as i32,
                        (z as i32) + face.normal[2] as i32,
                    );
                    let neighbour = if (0..CHUNK_SIZE as i32).contains(&nx)
                        && (0..CHUNK_SIZE as i32).contains(&ny)
                        && (0..CHUNK_SIZE as i32).contains(&nz)
                    {
                        chunk.get(nx as usize, ny as usize, nz as usize)
                    } else {
                        BlockId::AIR
                    };
                    // Cull rules:
                    //   neighbour air                     -> emit
                    //   neighbour opaque                  -> skip
                    //   neighbour transparent (== self)   -> skip
                    //   neighbour transparent (!= self)   -> emit
                    if !neighbour.is_air() {
                        if is_opaque(neighbour) {
                            continue;
                        }
                        if neighbour == block {
                            continue;
                        }
                    }
                    let rect = uv_rect(block, face.normal);
                    // Sample light at the AIR-SIDE cell (nx, ny,
                    // nz). For boundary faces the air-side falls
                    // outside the chunk's `CHUNK_SIZE` range; pass
                    // the raw signed coord through so callers
                    // that have a world-space `LightGrid` lookup
                    // can route across chunk borders.
                    let light = light_at([nx, ny, nz], face.normal);
                    let anim = anim_id_at(block, face.normal);
                    emit_quad_lit_anim(
                        &mut entry.0,
                        &mut entry.1,
                        p,
                        face,
                        rect,
                        light,
                        anim,
                    );
                }
            }
        }
    }

    per_block
        .into_iter()
        .map(|(block, (vertices, indices))| ChunkSubmesh {
            block,
            vertices,
            indices,
        })
        .collect()
}

struct Face {
    normal: [i8; 3],
    /// Four corners of the face in CCW order when viewed from `normal`.
    /// Positions are relative to the voxel's `(x, y, z)` corner.
    corners: [[f32; 3]; 4],
}

// Each entry mirrors the cube_geometry layout in `infengine_render`
// (CCW from the outside) so the existing PBR pipeline lights the
// voxel mesh correctly.
const FACES: &[Face] = &[
    // +X
    Face {
        normal: [1, 0, 0],
        corners: [
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ],
    },
    // -X
    Face {
        normal: [-1, 0, 0],
        corners: [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
        ],
    },
    // +Y
    Face {
        normal: [0, 1, 0],
        corners: [
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
    },
    // -Y
    Face {
        normal: [0, -1, 0],
        corners: [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
    },
    // +Z
    Face {
        normal: [0, 0, 1],
        corners: [
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ],
    },
    // -Z
    Face {
        normal: [0, 0, -1],
        corners: [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
    },
];

/// Emit one quad with per-vertex `light` `(block, sky)` + anim
/// slot id (Wave 9.y). All 4 corners share the same values - the
/// 4-corner AO smoothing pass is a follow-up; per-corner anim
/// would only matter if a single tile straddled two animated
/// strips (it doesn't).
fn emit_quad_lit_anim(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    face: &Face,
    uv_rect: [f32; 4],
    light: [f32; 2],
    anim_id: u32,
) {
    let normal = [
        face.normal[0] as f32,
        face.normal[1] as f32,
        face.normal[2] as f32,
    ];
    let base = vertices.len() as u32;
    let [u0, v0, u1, v1] = uv_rect;
    let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
    for (i, corner) in face.corners.iter().enumerate() {
        vertices.push(
            MeshVertex::new(
                [
                    origin.x + corner[0],
                    origin.y + corner[1],
                    origin.z + corner[2],
                ],
                normal,
            )
            .with_uv(uvs[i])
            .with_light(light[0], light[1])
            .with_anim_id(anim_id),
        );
    }
    indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// Emit one quad from four pre-computed world-space corners.
///
/// W16.2: used by the demo's `emit_special` closure when walking
/// per-element model geometry (torches, slabs, cross-pattern plants,
/// stairs). The caller has already applied element `from`/`to` scaling
/// and any `ElementRotation` transform.
///
/// `corners` - four world-space positions in CCW order as seen from
/// the face's outward normal.
/// `normal`  - face normal (need not be unit length; used for
/// lighting look-up only).
/// `uv_rect` - `[u0, v0, u1, v1]` in atlas space (same layout as
/// `uv_rect` closures).
/// `light`   - `[block_light_norm, sky_light_norm]` (0..=1 each).
/// `anim_id` - animation slot (0 = static).
pub fn emit_element_quad(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    uv_rect: [f32; 4],
    light: [f32; 2],
    anim_id: u32,
) {
    let base = vertices.len() as u32;
    let [u0, v0, u1, v1] = uv_rect;
    // UV layout matches `emit_quad_lit_anim`: corners are CCW from
    // the outside, UVs assigned bottom-left -> bottom-right ->
    // top-right -> top-left.
    let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
    for (i, corner) in corners.iter().enumerate() {
        vertices.push(
            MeshVertex::new(*corner, normal)
                .with_uv(uvs[i])
                .with_light(light[0], light[1])
                .with_anim_id(anim_id),
        );
    }
    indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// W30.3: Greedy-mesh variant of `emit_element_quad` that populates the
/// W30.1 tile-fract vertex attributes. Each corner gets a distinct
/// `quad_uv` so the shader's `fract(quad_uv) * tile_size` always stays
/// within the atlas tile's rect regardless of how large the merged quad is.
///
/// `corners`      - four world-space positions in CCW order.
/// `normal`       - face normal.
/// `tile_uv0`     - atlas tile origin `[u0, v0]` in `[0, 1]` space.
/// `tile_size`    - atlas tile extents `[u1-u0, v1-v0]` in `[0, 1]` space.
/// `quad_uv_corners` - per-corner UV in voxel coords; x in `[0, w]`, y in `[0, h]`.
/// `light`        - `[block_light, sky_light]` normalised.
/// `anim_id`      - animation slot (0 = static).
#[allow(clippy::too_many_arguments)]
pub fn emit_element_quad_tiled(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    tile_uv0: [f32; 2],
    tile_size: [f32; 2],
    quad_uv_corners: [[f32; 2]; 4],
    light: [f32; 2],
    anim_id: u32,
) {
    let base = vertices.len() as u32;
    for (i, corner) in corners.iter().enumerate() {
        vertices.push(
            MeshVertex::new(*corner, normal)
                .with_tile_uv(tile_uv0, tile_size, quad_uv_corners[i])
                .with_light(light[0], light[1])
                .with_anim_id(anim_id),
        );
    }
    indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// W122: per-corner variant of `emit_element_quad_tiled`. Each of the
/// four corners carries its own `[block_light, sky_light, ao]` so the
/// shader interpolates smooth lighting + ambient occlusion across the
/// face (MC's smooth-lit look). `corner_light[i]` matches `corners[i]`.
#[allow(clippy::too_many_arguments)]
pub fn emit_element_quad_tiled_corners(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    tile_uv0: [f32; 2],
    tile_size: [f32; 2],
    quad_uv_corners: [[f32; 2]; 4],
    corner_light: [[f32; 3]; 4],
    anim_id: u32,
) {
    let base = vertices.len() as u32;
    for (i, corner) in corners.iter().enumerate() {
        let cl = corner_light[i];
        vertices.push(
            MeshVertex::new(*corner, normal)
                .with_tile_uv(tile_uv0, tile_size, quad_uv_corners[i])
                .with_light(cl[0], cl[1])
                .with_ao(cl[2])
                .with_anim_id(anim_id),
        );
    }
    indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// Build a `MeshInstance` ready to feed into
/// `Renderer::draw_mesh_pbr`. Vertex positions in submeshes are already
/// in world space, so the model matrix is identity. The caller is
/// expected to pull `color` / `metallic` / `roughness` from the block
/// registry.
pub fn submesh_instance(def: &BlockDef) -> MeshInstance {
    MeshInstance::new(
        glam::Mat4::IDENTITY.to_cols_array(),
        [def.color[0], def.color[1], def.color[2], 1.0],
    )
    .with_material(def.metallic, def.roughness)
}

/// Integer chunk coordinate in the world's chunk grid. Each unit is
/// one `CHUNK_SIZE`-step in world space.
pub type ChunkCoord = [i32; 3];

/// Container that owns multiple chunks indexed by chunk coordinate.
/// World-space voxel queries go through `get` / `set`; the world
/// resolves the chunk + local index automatically.
#[derive(Debug, Default)]
pub struct World {
    chunks: HashMap<ChunkCoord, Chunk>,
}

impl World {
    pub fn new() -> Self {
        Self {
            chunks: HashMap::new(),
        }
    }

    pub fn insert_chunk(&mut self, coord: ChunkCoord, chunk: Chunk) {
        self.chunks.insert(coord, chunk);
    }

    pub fn chunk(&self, coord: ChunkCoord) -> Option<&Chunk> {
        self.chunks.get(&coord)
    }

    pub fn chunk_mut(&mut self, coord: ChunkCoord) -> Option<&mut Chunk> {
        self.chunks.get_mut(&coord)
    }

    /// Drop the chunk at `coord` from the world. Returns the removed
    /// chunk if it was present.
    pub fn remove_chunk(&mut self, coord: ChunkCoord) -> Option<Chunk> {
        self.chunks.remove(&coord)
    }

    pub fn iter_chunks(&self) -> impl Iterator<Item = (&ChunkCoord, &Chunk)> {
        self.chunks.iter()
    }

    /// Lookup the voxel at a world-space integer coordinate. Returns
    /// `BlockId::AIR` for coordinates outside any registered chunk.
    pub fn get(&self, world: [i32; 3]) -> BlockId {
        let (cc, lc) = split_coord(world);
        match self.chunks.get(&cc) {
            Some(chunk) => chunk.get(lc[0], lc[1], lc[2]),
            None => BlockId::AIR,
        }
    }

    /// Write a voxel at a world-space integer coordinate. Returns the
    /// chunk coordinate that was modified so callers can rebuild its
    /// mesh. No-op when the chunk does not exist (the world doesn't
    /// auto-create chunks - the caller decides).
    pub fn set(&mut self, world: [i32; 3], block: BlockId) -> Option<ChunkCoord> {
        let (cc, lc) = split_coord(world);
        let chunk = self.chunks.get_mut(&cc)?;
        chunk.set(lc[0], lc[1], lc[2], block);
        Some(cc)
    }

    pub fn chunk_world_origin(coord: ChunkCoord) -> Vec3 {
        Vec3::new(
            (coord[0] as f32) * CHUNK_SIZE as f32,
            (coord[1] as f32) * CHUNK_SIZE as f32,
            (coord[2] as f32) * CHUNK_SIZE as f32,
        )
    }
}

fn split_coord(world: [i32; 3]) -> (ChunkCoord, [usize; 3]) {
    let cs = CHUNK_SIZE as i32;
    let cc = [
        world[0].div_euclid(cs),
        world[1].div_euclid(cs),
        world[2].div_euclid(cs),
    ];
    let lc = [
        world[0].rem_euclid(cs) as usize,
        world[1].rem_euclid(cs) as usize,
        world[2].rem_euclid(cs) as usize,
    ];
    (cc, lc)
}

/// Result of a ray cast through the voxel world. `voxel` is the
/// solid voxel the ray hit, `normal` is the integer face normal the
/// ray entered through (e.g. `[0, 1, 0]` for the top face), and
/// `place_at` is the empty voxel coord just outside that face -
/// where a player click would place a new block.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct RayHit {
    pub voxel: [i32; 3],
    pub normal: [i32; 3],
    pub place_at: [i32; 3],
}

/// Cast `dir` from `origin` through the voxel grid using the
/// Amanatides + Woo algorithm. Returns the first solid voxel within
/// `max_dist` or `None` if the ray exits the world / runs out of
/// distance. Treats every non-air voxel as a hit; see
/// `raycast_with` for a version that lets the caller skip
/// pass-through decorations (torches, flowers, ...).
pub fn raycast(world: &World, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RayHit> {
    raycast_with(world, origin, dir, max_dist, |b| !b.is_air())
}

/// Like `raycast` but the caller supplies `is_blocked(block) -> bool`
/// so non-blocking decorations (torches mounted in air, flowers,
/// etc.) can be skipped: the ray traverses the voxel even though it
/// isn't air. The first voxel whose block matches the predicate is
/// returned.
pub fn raycast_with<F: Fn(BlockId) -> bool>(
    world: &World,
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
    is_blocked: F,
) -> Option<RayHit> {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }

    let mut voxel = [
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    ];
    let step = [
        if dir.x > 0.0 {
            1
        } else if dir.x < 0.0 {
            -1
        } else {
            0
        },
        if dir.y > 0.0 {
            1
        } else if dir.y < 0.0 {
            -1
        } else {
            0
        },
        if dir.z > 0.0 {
            1
        } else if dir.z < 0.0 {
            -1
        } else {
            0
        },
    ];

    fn boundary(coord: f32, d: f32) -> f32 {
        if d > 0.0 {
            (coord.floor() + 1.0 - coord) / d
        } else if d < 0.0 {
            (coord - coord.floor()) / -d
        } else {
            f32::INFINITY
        }
    }
    let mut t_max = [
        boundary(origin.x, dir.x),
        boundary(origin.y, dir.y),
        boundary(origin.z, dir.z),
    ];
    let t_delta = [
        if step[0] != 0 {
            (1.0 / dir.x).abs()
        } else {
            f32::INFINITY
        },
        if step[1] != 0 {
            (1.0 / dir.y).abs()
        } else {
            f32::INFINITY
        },
        if step[2] != 0 {
            (1.0 / dir.z).abs()
        } else {
            f32::INFINITY
        },
    ];

    let mut last_axis: i32 = -1;
    let mut last_step = 0;

    // Cap iterations defensively in case of numerical edge cases.
    for _ in 0..1024 {
        if is_blocked(world.get(voxel)) {
            let normal = if last_axis < 0 {
                [0, 0, 0]
            } else {
                let mut n = [0, 0, 0];
                n[last_axis as usize] = -last_step;
                n
            };
            let mut place_at = voxel;
            if last_axis >= 0 {
                place_at[last_axis as usize] -= last_step;
            }
            return Some(RayHit {
                voxel,
                normal,
                place_at,
            });
        }
        // Step into the neighbour with the smallest t_max.
        let axis = if t_max[0] < t_max[1] && t_max[0] <= t_max[2] {
            0
        } else if t_max[1] <= t_max[2] {
            1
        } else {
            2
        };
        if t_max[axis] > max_dist {
            return None;
        }
        voxel[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        last_axis = axis as i32;
        last_step = step[axis];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_voxel_emits_six_faces() {
        let mut chunk = Chunk::empty();
        chunk.set(8, 8, 8, BlockId(1));
        let submeshes = chunk_mesh(&chunk, Vec3::ZERO);
        let total: usize = submeshes.iter().map(|s| s.indices.len()).sum();
        // 6 faces * 6 indices per quad = 36.
        assert_eq!(total, 36);
        assert_eq!(submeshes.len(), 1);
        assert_eq!(submeshes[0].block, BlockId(1));
    }

    #[test]
    fn interior_faces_culled() {
        let chunk = Chunk::filled(BlockId(1));
        let submeshes = chunk_mesh(&chunk, Vec3::ZERO);
        let total: u32 = submeshes.iter().map(|s| s.indices.len() as u32).sum();
        // A 16^3 fully-solid chunk only emits faces on its outer
        // boundary: 6 faces * 16 * 16 quads * 6 indices = 9216.
        assert_eq!(total, 6 * 16 * 16 * 6);
    }

    #[test]
    fn block_registry_round_trip() {
        let mut reg = BlockRegistry::new();
        reg.insert(BlockId(1), BlockDef::solid("stone", [0.5, 0.5, 0.55]));
        assert_eq!(reg.get(BlockId(1)).unwrap().name, "stone");
        assert!(reg.get(BlockId(2)).is_none());
    }

    #[test]
    fn world_chunk_split() {
        let (cc, lc) = split_coord([17, 0, -1]);
        assert_eq!(cc, [1, 0, -1]);
        assert_eq!(lc, [1, 0, CHUNK_SIZE - 1]);
    }

    #[test]
    fn world_get_set() {
        let mut w = World::new();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        assert!(w.get([5, 5, 5]).is_air());
        let modified = w.set([5, 5, 5], BlockId(1));
        assert_eq!(modified, Some([0, 0, 0]));
        assert_eq!(w.get([5, 5, 5]), BlockId(1));
    }

    #[test]
    fn raycast_hits_first_solid() {
        let mut w = World::new();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        w.set([5, 5, 5], BlockId(1));
        // Cast from (5, 5, 0) toward +Z so the ray walks (5,5,1),
        // (5,5,2)... until (5,5,5).
        let hit = raycast(&w, Vec3::new(5.5, 5.5, 0.0), Vec3::Z, 20.0).expect("ray should hit");
        assert_eq!(hit.voxel, [5, 5, 5]);
        // Entered through the -Z face (block normal pointing -Z).
        assert_eq!(hit.normal, [0, 0, -1]);
        assert_eq!(hit.place_at, [5, 5, 4]);
    }

    #[test]
    fn raycast_misses_into_air() {
        let w = World::new();
        let hit = raycast(&w, Vec3::ZERO, Vec3::X, 100.0);
        assert!(hit.is_none());
    }

    #[test]
    fn chunk_mesh_with_light_bakes_per_face_values() {
        // Single solid voxel: 6 faces, each with a sampled light
        // value. The closure returns `(block, sky)` for the air
        // cell on the OUTSIDE of each face.
        let mut chunk = Chunk::empty();
        chunk.set(8, 8, 8, BlockId(1));
        let subs = chunk_mesh_with_atlas_and_light(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| true,
            |air_local, _normal| {
                // Encode the air-side coord into the light pair
                // so we can assert which face the vertex belongs
                // to. block_light = (x - 7) / 15, sky = (y - 7).
                let x = air_local[0] as f32;
                let y = air_local[1] as f32;
                [(x - 7.0) / 15.0, (y - 7.0).clamp(0.0, 15.0) / 15.0]
            },
        );
        assert_eq!(subs.len(), 1);
        let verts = &subs[0].vertices;
        // 6 faces * 4 verts each = 24 vertices.
        assert_eq!(verts.len(), 24);
        // Every vertex `light().0` (block_light) sits in [0, 1].
        for v in verts {
            let l = v.light();
            assert!(l[0] >= 0.0 && l[0] <= 1.0);
            assert!(l[1] >= 0.0 && l[1] <= 1.0);
        }
        // Pick the +X face: air-side cell is (9, 8, 8) -> block
        // light = (9 - 7) / 15 = 2 / 15.
        let expected_block = 2.0 / 15.0;
        let plus_x_verts: Vec<_> = verts
            .iter()
            .filter(|v| (v.normal()[0] - 1.0).abs() < 0.5)
            .collect();
        assert_eq!(plus_x_verts.len(), 4);
        for v in plus_x_verts {
            // W28.6: light() decode has 1/255 rounding tolerance.
            assert!((v.light()[0] - expected_block).abs() < 1e-2);
        }
    }

    #[test]
    fn chunk_mesh_default_light_is_no_block_full_sky() {
        // `chunk_mesh_with_atlas` (no light variant) must leave
        // every vertex at `light = [0.0, 1.0]` so legacy callers
        // see the unlit-day default.
        let mut chunk = Chunk::empty();
        chunk.set(4, 4, 4, BlockId(1));
        let subs = chunk_mesh_with_atlas(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| true,
        );
        for sub in subs {
            for v in &sub.vertices {
                // W28.6: packed light, decode with 1/255 tolerance.
                let l = v.light();
                assert!(l[0].abs() < 1e-4, "expected block=0, got {}", l[0]);
                assert!((l[1] - 1.0).abs() < 1e-4, "expected sky=1, got {}", l[1]);
            }
        }
    }

    #[test]
    fn mesh_vertex_with_light_clamps_inputs() {
        // `with_light` clamps both inputs to [0, 1] regardless of
        // raw call-site values.
        let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_light(-1.0, 2.5);
        let l = v.light();
        assert!(l[0].abs() < 1e-4, "expected 0, got {}", l[0]);
        assert!((l[1] - 1.0).abs() < 1e-4, "expected 1, got {}", l[1]);
        let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_light(0.5, 0.25);
        let l = v.light();
        // W28.6: light is packed to u8/255 so tolerance must allow
        // 1/255 = ~0.004 quantisation loss.
        assert!((l[0] - 0.5).abs() < 5e-3, "expected 0.5, got {}", l[0]);
        assert!((l[1] - 0.25).abs() < 5e-3, "expected 0.25, got {}", l[1]);
    }

    #[test]
    fn chunk_mesh_legacy_path_writes_anim_id_zero() {
        // `chunk_mesh_with_atlas_and_light` (no anim closure) MUST
        // leave every vertex at `anim_id = 0` so static UVs hold.
        let mut chunk = Chunk::empty();
        chunk.set(4, 4, 4, BlockId(1));
        let subs = chunk_mesh_with_atlas_and_light(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| true,
            |_, _| [0.0, 1.0],
        );
        for sub in subs {
            for v in &sub.vertices {
                assert_eq!(v.anim_id, 0);
            }
        }
    }

    #[test]
    fn chunk_mesh_anim_closure_stamps_per_face() {
        // The anim closure receives `(block, normal)` and returns
        // the slot id. Stamping `7` on +Y and `0` elsewhere must
        // produce a 4-vertex +Y quad with anim_id == 7 and 20
        // other vertices with anim_id == 0.
        let mut chunk = Chunk::empty();
        chunk.set(8, 8, 8, BlockId(1));
        let subs = chunk_mesh_with_atlas_and_light_and_anim(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| true,
            |_, _| [0.0, 1.0],
            |_block, normal| if normal[1] > 0 { 7 } else { 0 },
        );
        assert_eq!(subs.len(), 1);
        let verts = &subs[0].vertices;
        let plus_y_with_anim: Vec<_> = verts
            .iter()
            .filter(|v| v.normal()[1] > 0.5 && v.anim_id == 7)
            .collect();
        assert_eq!(plus_y_with_anim.len(), 4);
        // Every other face stays at 0.
        let other_zero: Vec<_> = verts
            .iter()
            .filter(|v| v.normal()[1] <= 0.5)
            .collect();
        assert!(other_zero.iter().all(|v| v.anim_id == 0));
    }

    #[test]
    fn chunk_mesh_anim_id_clamps_at_32_per_with_anim_id() {
        // The vertex builder clamps anim_id to 32. Even when the
        // closure returns 50, the emitted vertex sees 32 so it
        // can never index out of bounds in the GPU uniform.
        let mut chunk = Chunk::empty();
        chunk.set(0, 0, 0, BlockId(1));
        let subs = chunk_mesh_with_atlas_and_light_and_anim(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| true,
            |_, _| [0.0, 1.0],
            |_, _| 50,
        );
        for sub in subs {
            for v in &sub.vertices {
                assert_eq!(v.anim_id, 32);
            }
        }
    }

    // ---------------------------------------------------------------
    // W16.2 - `chunk_mesh_with_atlas_light_anim_and_special` tests.
    // ---------------------------------------------------------------

    #[test]
    fn default_cube_block_still_renders_six_faces_via_special_path() {
        // When `emit_special` returns false for every block, the
        // output must match the legacy cube path exactly.
        let mut chunk = Chunk::empty();
        chunk.set(8, 8, 8, BlockId(1));
        let subs = chunk_mesh_with_atlas_light_anim_and_special(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| true,
            |_, _| [0.0, 1.0],
            |_, _| 0u32,
            |_block, _origin, _verts, _idxs| false, // never handle
        );
        let total_indices: usize = subs.iter().map(|s| s.indices.len()).sum();
        // 6 faces * 6 indices = 36.
        assert_eq!(total_indices, 36, "cube should still emit 36 indices");
    }

    #[test]
    fn emit_special_true_suppresses_cube_path() {
        // If emit_special returns true, the cube geometry is NOT added.
        let mut chunk = Chunk::empty();
        chunk.set(0, 0, 0, BlockId(99));
        let subs = chunk_mesh_with_atlas_light_anim_and_special(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| false, // not opaque
            |_, _| [0.0, 1.0],
            |_, _| 0u32,
            // Claim handling but emit nothing = empty mesh.
            |_block, _origin, _verts, _idxs| true,
        );
        let total: usize = subs.iter().map(|s| s.vertices.len()).sum();
        assert_eq!(total, 0, "emit_special=true should suppress cube path");
    }

    #[test]
    fn emit_special_can_inject_custom_geometry() {
        // emit_special injects exactly 4 vertices + 6 indices for
        // one custom quad.
        let mut chunk = Chunk::empty();
        chunk.set(4, 4, 4, BlockId(7));
        let subs = chunk_mesh_with_atlas_light_anim_and_special(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| false,
            |_, _| [0.0, 1.0],
            |_, _| 0u32,
            |_block, origin, verts, idxs| {
                // Inject a dummy horizontal quad at voxel top.
                emit_element_quad(
                    verts,
                    idxs,
                    [
                        [origin.x, origin.y + 1.0, origin.z],
                        [origin.x + 1.0, origin.y + 1.0, origin.z],
                        [origin.x + 1.0, origin.y + 1.0, origin.z + 1.0],
                        [origin.x, origin.y + 1.0, origin.z + 1.0],
                    ],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0, 1.0],
                    [0.0, 1.0],
                    0,
                );
                true // handled
            },
        );
        let total_v: usize = subs.iter().map(|s| s.vertices.len()).sum();
        let total_i: usize = subs.iter().map(|s| s.indices.len()).sum();
        assert_eq!(total_v, 4, "custom quad = 4 vertices");
        assert_eq!(total_i, 6, "custom quad = 6 indices");
    }

    #[test]
    fn emit_special_selective_override_mixed_blocks() {
        // Block 1 uses special (returns true, emits nothing).
        // Block 2 falls through to cube (returns false).
        let mut chunk = Chunk::empty();
        chunk.set(0, 0, 0, BlockId(1)); // special
        chunk.set(15, 15, 15, BlockId(2)); // cube
        let subs = chunk_mesh_with_atlas_light_anim_and_special(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.0, 0.0, 1.0, 1.0],
            |_| true,
            |_, _| [0.0, 1.0],
            |_, _| 0u32,
            |block, _origin, _verts, _idxs| block == BlockId(1),
        );
        // Block 1 contributed 0 vertices.
        let b1_verts: usize = subs
            .iter()
            .filter(|s| s.block == BlockId(1))
            .map(|s| s.vertices.len())
            .sum();
        // Block 2 emits 6 * 4 = 24 vertices (6 cube faces).
        let b2_verts: usize = subs
            .iter()
            .filter(|s| s.block == BlockId(2))
            .map(|s| s.vertices.len())
            .sum();
        assert_eq!(b1_verts, 0, "special block contributes no verts");
        assert_eq!(b2_verts, 24, "cube block contributes 24 verts");
    }

    #[test]
    fn emit_element_quad_produces_four_vertices_six_indices() {
        let mut verts = Vec::new();
        let mut idxs = Vec::new();
        emit_element_quad(
            &mut verts,
            &mut idxs,
            [
                [0.0, 0.5, 0.0],
                [1.0, 0.5, 0.0],
                [1.0, 0.5, 1.0],
                [0.0, 0.5, 1.0],
            ],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.5, 0.8],
            3,
        );
        assert_eq!(verts.len(), 4);
        assert_eq!(idxs.len(), 6);
        assert!(verts.iter().all(|v| v.anim_id == 3));
    }

    // ---------------------------------------------------------------
    // W33.1: Greedy mesher UV orientation tests.
    // ---------------------------------------------------------------

    /// For every face of a single-voxel greedy mesh, verify that the
    /// four per-corner quad_uv values encode the expected BL/BR/TR/TL
    /// texture orientation matching emit_quad_lit_anim:
    ///   c0 -> bottom-left  (tex_u ~ 0, tex_v ~ 1)
    ///   c1 -> bottom-right (tex_u ~ 1, tex_v ~ 1)
    ///   c2 -> top-right    (tex_u ~ 1, tex_v ~ 0)
    ///   c3 -> top-left     (tex_u ~ 0, tex_v ~ 0)
    ///
    /// quad_uv is in [0, 16] voxel coords.
    /// For a 1x1 quad (w=h=1) with E=0.001:
    ///   c0.x ~ 0,     c0.y ~ 0.999
    ///   c1.x ~ 0.999, c1.y ~ 0.999
    ///   c2.x ~ 0.999, c2.y ~ 0.0
    ///   c3.x ~ 0.0,   c3.y ~ 0.0
    ///
    /// Tolerance is 0.05 to absorb u16 quantisation (1/65535 * 16 ~ 0.00025).
    #[test]
    fn greedy_mesh_quad_uv_corners_bl_br_tr_tl_all_faces() {
        let mut chunk = Chunk::empty();
        chunk.set(8, 8, 8, BlockId(1));

        // Tile rect [0.25, 0.25, 0.75, 0.75] so tile_size > 0 everywhere.
        let tile_rect = [0.25f32, 0.25f32, 0.75f32, 0.75f32];
        let subs = chunk_mesh_greedy(
            &chunk,
            Vec3::ZERO,
            |_, _| tile_rect,
            |_| true,
            |_, _| [0.0f32, 1.0f32],
            |_, _| 0u32,
        );
        assert_eq!(subs.len(), 1);
        let verts = &subs[0].vertices;
        // 6 faces * 4 verts = 24.
        assert_eq!(verts.len(), 24);

        // Group vertices by face normal.
        // For each normal: decode quad_uv for all 4 verts and check
        // that the expected pattern is present as a set (winding order
        // may differ from CPU corner order, so we check set membership
        // rather than index position).
        let faces_normals: &[[i8; 3]] = &[
            [1, 0, 0], [-1, 0, 0],
            [0, 1, 0], [0, -1, 0],
            [0, 0, 1], [0, 0, -1],
        ];
        const TOL: f32 = 0.05;
        const E_TARGET: f32 = 0.999; // fract target for "high" corner

        for &normal in faces_normals {
            let nf = [normal[0] as f32, normal[1] as f32, normal[2] as f32];
            let face_verts: Vec<_> = verts
                .iter()
                .filter(|v| {
                    let n = v.normal();
                    (n[0] - nf[0]).abs() < 0.5
                        && (n[1] - nf[1]).abs() < 0.5
                        && (n[2] - nf[2]).abs() < 0.5
                })
                .collect();
            assert_eq!(
                face_verts.len(),
                4,
                "expected 4 verts for normal {:?}",
                normal
            );

            // Collect the 4 (quad_uv.x, quad_uv.y) pairs. Since quad_uv is
            // in [0, 16] voxel space, use fract of each component (mod 1)
            // to get the tile-local [0, 1) value.
            let uvs: Vec<[f32; 2]> = face_verts
                .iter()
                .map(|v| {
                    let q = v.quad_uv_f32();
                    [q[0].fract(), q[1].fract()]
                })
                .collect();

            // Check that the four expected fractional UV values appear
            // (as a multiset). Expected: (0,~1), (~1,~1), (~1,0), (0,0).
            let expected = [
                [0.0f32, E_TARGET],
                [E_TARGET, E_TARGET],
                [E_TARGET, 0.0f32],
                [0.0f32, 0.0f32],
            ];

            for exp in &expected {
                let found = uvs.iter().any(|got| {
                    (got[0] - exp[0]).abs() < TOL && (got[1] - exp[1]).abs() < TOL
                });
                assert!(
                    found,
                    "normal {:?}: expected quad_uv fract ~{:?} not found; got {:?}",
                    normal, exp, uvs
                );
            }
        }
    }

    // W34.2: greedy merging reduces triangle count on uniform surfaces.
    // A 16x16 flat floor (all same block, all top faces visible) should
    // produce exactly 1 merged quad per direction, not 256 quads.
    #[test]
    fn greedy_flat_floor_produces_one_quad_not_256() {
        let cs = CHUNK_SIZE;
        let mut chunk = Chunk::empty();
        // Fill y=0 row with block id 1.
        for x in 0..cs {
            for z in 0..cs {
                chunk.set(x, 0, z, BlockId(1));
            }
        }
        let subs = chunk_mesh_greedy(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.25f32, 0.25f32, 0.75f32, 0.75f32],
            |_| true,
            |_, _| [0.0f32, 1.0f32],
            |_, _| 0u32,
        );
        assert_eq!(subs.len(), 1);
        // Top face: 1 merged quad (4 verts). Bottom face: 1 merged quad (4
        // verts). 4 side strips (each 16x1 merged -> 4 verts each).
        // Total faces: top(1) + bottom(1) + sides(4) = 6 quads, 24 verts,
        // but many side faces are internal (between block and air) so
        // each of the 4 outer edge walls gives 1 quad.
        // Key assertion: top face = 1 quad, not 256.
        let top_verts: usize = subs[0]
            .vertices
            .chunks(4)
            .filter(|quad| {
                quad.iter().any(|v| {
                    let n = v.normal();
                    (n[1] - 1.0f32).abs() < 0.5
                })
            })
            .count();
        // One merged quad for the top face = 1 group of 4 vertices.
        assert_eq!(top_verts, 1, "greedy should merge 16x16 top face into 1 quad, got {top_verts}");

        // Naive mesher would produce 256 quads for top face.
        let naive_subs = chunk_mesh_with_atlas_light_anim_and_special(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.25f32, 0.25f32, 0.75f32, 0.75f32],
            |_| true,
            |_, _| [0.0f32, 1.0f32],
            |_, _| 0u32,
            |_, _, _, _| false,
        );
        let naive_top_verts: usize = naive_subs
            .iter()
            .flat_map(|s| s.vertices.chunks(4))
            .filter(|quad| {
                quad.iter().any(|v| {
                    let n = v.normal();
                    (n[1] - 1.0f32).abs() < 0.5
                })
            })
            .count();
        assert_eq!(
            naive_top_verts, 256,
            "naive should produce 256 top quads, got {naive_top_verts}"
        );
    }

    /// W229.1 diagnostic: every runtime-mesher vertex must sit EXACTLY
    /// on the integer voxel grid, and the shared edge between two
    /// neighbouring coplanar quads must use bitwise-identical
    /// coordinates - any deviation is a seam the rasterizer shows as a
    /// pinhole line between blocks.
    #[test]
    #[allow(clippy::print_stdout)]
    fn runtime_mesher_vertices_are_exact_grid_points() {
        let cs = CHUNK_SIZE;
        let mut chunk = Chunk::empty();
        for x in 0..cs {
            for z in 0..cs {
                chunk.set(x, 0, z, BlockId(1));
            }
        }
        let subs = chunk_mesh_greedy_with_special(
            &chunk,
            Vec3::new(160.0, 96.0, -320.0), // a realistic chunk origin
            |_, _| [0.25f32, 0.25, 0.75, 0.75],
            |_| true,
            |_, _| [0.0f32, 1.0f32],
            |_, _| 0u32,
            |_| false,
            |_, _, _, _| false,
        );
        let mut bad = 0usize;
        for s in &subs {
            for v in &s.vertices {
                for (i, base) in [(0usize, 160.0f32), (1, 96.0), (2, -320.0)] {
                    let rel = v.position[i] - base;
                    if rel.fract() != 0.0 {
                        if bad < 8 {
                            println!("non-integer vertex coord: {:?}", v.position);
                        }
                        bad += 1;
                    }
                }
            }
        }
        assert_eq!(bad, 0, "{bad} vertex coords off the voxel grid");
    }

    /// W226.2: the runtime mesher (`chunk_mesh_greedy_with_special`)
    /// must be watertight: no emitted vertex may lie strictly inside an
    /// axis-aligned edge of another quad. Such T-junctions rasterize
    /// with subpixel pinholes - the user-reported dotted see-through
    /// lines across dark-oak trunks at light-step heights (W122 splits
    /// merged quads at smooth-light steps, so quads of different sizes
    /// shared partial edges).
    #[test]
    fn runtime_mesher_emits_no_t_junctions() {
        let cs = CHUNK_SIZE;
        let mut chunk = Chunk::empty();
        for x in 0..cs {
            for z in 0..cs {
                chunk.set(x, 0, z, BlockId(1));
            }
        }
        // A light field with a step confined to one corner: under
        // greedy merging this yields an L-shaped layout of small quads
        // beside big ones - partial shared edges = T-junctions.
        let light = |c: [i32; 3], _n: [i8; 3]| -> [f32; 2] {
            if c[0] <= 1 && c[2] <= 1 { [0.0, 0.2] } else { [0.0, 1.0] }
        };
        let subs = chunk_mesh_greedy_with_special(
            &chunk,
            Vec3::ZERO,
            |_, _| [0.25f32, 0.25f32, 0.75f32, 0.75f32],
            |_| true,
            light,
            |_, _| 0u32,
            |_| false,
            |_, _, _, _| false,
        );
        // Gather every quad's outline edges + every vertex. Positions
        // sit on the integer voxel grid, so comparisons are exact.
        let mut edges: Vec<([f32; 3], [f32; 3])> = Vec::new();
        let mut verts: Vec<[f32; 3]> = Vec::new();
        for s in &subs {
            for q in s.vertices.chunks(4) {
                let p: Vec<[f32; 3]> = q.iter().map(|v| v.position).collect();
                verts.extend(p.iter().copied());
                for i in 0..4 {
                    edges.push((p[i], p[(i + 1) % 4]));
                }
            }
        }
        // A vertex strictly inside an axis-aligned edge = T-junction.
        let mut bad = 0usize;
        for v in &verts {
            for (a, b) in &edges {
                let axis = (0..3).find(|&k| a[k] != b[k]);
                let Some(axis) = axis else { continue };
                if (0..3).any(|k| k != axis && (a[k] != b[k] || v[k] != a[k])) {
                    continue;
                }
                let (lo, hi) = if a[axis] < b[axis] { (a[axis], b[axis]) } else { (b[axis], a[axis]) };
                if v[axis] > lo && v[axis] < hi {
                    bad += 1;
                }
            }
        }
        assert_eq!(bad, 0, "mesh has {bad} T-junction vertex-on-edge incidences");
    }
}
