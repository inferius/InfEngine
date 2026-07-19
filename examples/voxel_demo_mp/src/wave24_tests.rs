//! W24 regression + unit tests (binary-crate side).
//!
//! Tests that require binary-only types (mesh_chunk_in_worker,
//! WorkerResult, ResourcePack loading path in main.rs) live here.
//! HaloLighting + LightGrid Clone tests live in lighting/mod.rs
//! because those types are also accessible via the lib crate.
//!
//! NOTE: this module is gated behind `#[cfg(test)]` and lives in the
//! binary crate (`src/main.rs` mod tree). It shares a compilation
//! unit with the other wave*_tests modules and compiles only when
//! running `cargo test --bin voxel_demo_mp`. The lib crate's
//! `cargo test --lib` path covers the lighting unit tests separately.

#![cfg(test)]

// -----------------------------------------------------------------------
// W24.3 - mesh_chunk_in_worker produces submeshes for solid chunks
// -----------------------------------------------------------------------

/// Calling mesh_chunk_in_worker on a chunk filled with stone (id=1)
/// and a fallback resource pack must return at least one non-empty
/// submesh. Validates the worker meshing path end-to-end.
#[test]
fn mesh_chunk_in_worker_solid_chunk_produces_submeshes() {
    use crate::lighting::HaloLighting;
    use crate::resource_pack;
    use infengine_voxel::{BlockId, Chunk};

    let mut chunk = Chunk::empty();
    for y in 0..infengine_voxel::CHUNK_SIZE {
        for z in 0..infengine_voxel::CHUNK_SIZE {
            for x in 0..infengine_voxel::CHUNK_SIZE {
                chunk.set(x, y, z, BlockId(1));
            }
        }
    }
    let pack = resource_pack::ResourcePack::load();
    let halo = HaloLighting {
        own: None,
        px: None,
        nx: None,
        pz: None,
        nz: None,
        py: None,
        ny: None,
    };
    let no_doors = std::collections::HashMap::new();
    let no_rails = std::collections::HashMap::new();
    let subs = crate::workers::mesh_chunk_in_worker(&chunk, [0, 0, 0], &pack, &halo, &no_doors, &no_rails, &std::collections::HashMap::new(), &std::collections::HashMap::new());
    let total_verts: usize = subs.iter().map(|s| s.vertices.len()).sum();
    assert!(
        total_verts > 0,
        "solid stone chunk must produce at least one vertex (got 0 from {} submeshes)",
        subs.len()
    );
}

/// Calling mesh_chunk_in_worker on an empty (all-air) chunk must
/// return no vertices.
#[test]
fn mesh_chunk_in_worker_air_chunk_produces_no_vertices() {
    use crate::lighting::HaloLighting;
    use crate::resource_pack;
    use infengine_voxel::Chunk;

    let chunk = Chunk::empty();
    let pack = resource_pack::ResourcePack::load();
    let halo = HaloLighting {
        own: None,
        px: None,
        nx: None,
        pz: None,
        nz: None,
        py: None,
        ny: None,
    };
    let no_doors = std::collections::HashMap::new();
    let no_rails = std::collections::HashMap::new();
    let subs = crate::workers::mesh_chunk_in_worker(&chunk, [0, 0, 0], &pack, &halo, &no_doors, &no_rails, &std::collections::HashMap::new(), &std::collections::HashMap::new());
    let total_verts: usize = subs.iter().map(|s| s.vertices.len()).sum();
    assert_eq!(total_verts, 0, "air chunk must produce zero vertices");
}

// -----------------------------------------------------------------------
// W24.4 / W25 - WorkerResult carries submeshes field
// -----------------------------------------------------------------------

/// WorkerResult::Generated has all expected fields - confirms the enum
/// shape after the W25 refactor to a two-variant enum.
#[test]
fn worker_result_generated_has_submeshes_field() {
    use infengine_voxel::{Chunk, ChunkSubmesh};
    let _r = crate::WorkerResult::Generated {
        coord: [0, 0, 0],
        chunk: Chunk::empty(),
        side_effects: vec![],
        submeshes: Vec::<ChunkSubmesh>::new(),
        dim: "overworld".to_string(),
    };
}

/// WorkerResult::Remeshed carries a revision counter and submeshes -
/// confirms the W25 remesh variant shape.
#[test]
fn worker_result_remeshed_has_revision_and_submeshes() {
    use infengine_voxel::ChunkSubmesh;
    let _r = crate::WorkerResult::Remeshed {
        coord: [1, 2, 3],
        revision: 42,
        submeshes: Vec::<ChunkSubmesh>::new(),
        dim: "overworld".to_string(),
    };
}

/// W316: a lone snow-layer cell (id 15) must emit the slab geometry
/// (5 faces x 4 verts = 20) - the invisible-snow regression check.
#[test]
fn snow_layer_cell_emits_slab() {
    use crate::lighting::HaloLighting;
    use crate::resource_pack;
    use infengine_voxel::BlockId;
    let mut chunk = infengine_voxel::Chunk::empty();
    chunk.set(4, 4, 4, BlockId(15));
    let pack = resource_pack::ResourcePack::load();
    let halo = HaloLighting {
        own: None,
        px: None,
        nx: None,
        pz: None,
        nz: None,
        py: None,
        ny: None,
    };
    let no_doors = std::collections::HashMap::new();
    let no_rails = std::collections::HashMap::new();
    let subs = crate::workers::mesh_chunk_in_worker(
        &chunk,
        [0, 0, 0],
        &pack,
        &halo,
        &no_doors,
        &no_rails,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
    );
    let total_verts: usize = subs.iter().map(|s| s.vertices.len()).sum();
    assert_eq!(
        total_verts, 20,
        "snow slab must emit 5 quads (got {total_verts} verts from {} submeshes)",
        subs.len()
    );
}
