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
            roughness: 0.85,
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
            roughness: 0.85,
            transparent: true,
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
#[derive(Debug, Clone)]
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
                    emit_quad(&mut entry.0, &mut entry.1, p, face, rect);
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

fn emit_quad(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    face: &Face,
    uv_rect: [f32; 4],
) {
    let normal = [
        face.normal[0] as f32,
        face.normal[1] as f32,
        face.normal[2] as f32,
    ];
    let base = vertices.len() as u32;
    // Standard quad UVs mapped into the supplied atlas tile rect.
    // Rect layout = [u0, v0, u1, v1] (top-left, bottom-right).
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
            .with_uv(uvs[i]),
        );
    }
    // Two triangles, CCW from outside: 0-2-1 and 0-3-2 - matches the
    // engine's cube_geometry winding so back-face culling keeps the
    // outward face visible.
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
/// distance.
pub fn raycast(world: &World, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RayHit> {
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
        if !world.get(voxel).is_air() {
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
}
