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
}

impl BlockDef {
    pub fn solid(name: impl Into<String>, color: [f32; 3]) -> Self {
        Self {
            name: name.into(),
            color,
            metallic: 0.0,
            roughness: 0.85,
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
                    if !neighbour.is_air() {
                        continue;
                    }
                    emit_quad(&mut entry.0, &mut entry.1, p, face);
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

fn emit_quad(vertices: &mut Vec<MeshVertex>, indices: &mut Vec<u32>, origin: Vec3, face: &Face) {
    let normal = [
        face.normal[0] as f32,
        face.normal[1] as f32,
        face.normal[2] as f32,
    ];
    let base = vertices.len() as u32;
    // Standard quad UVs: bottom-left, bottom-right, top-right, top-left.
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
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
}
