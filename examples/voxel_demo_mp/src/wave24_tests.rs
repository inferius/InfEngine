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
    let subs = crate::workers::mesh_chunk_in_worker(&chunk, [0, 0, 0], &pack, &halo, &no_doors, &no_rails, &std::collections::HashMap::new(), &std::collections::HashMap::new(), &crate::workers::BoundaryHalo::empty());
    let total_verts: usize = subs.iter().map(|s| s.vertices.len()).sum();
    assert!(
        total_verts > 0,
        "solid stone chunk must produce at least one vertex (got 0 from {} submeshes)",
        subs.len()
    );
}

/// W450 diagnostic (R33 half-door): a two-cell oak door in one
/// chunk must emit geometry in BOTH cells through the real worker
/// mesher. Ignored by default - needs the on-disk MC pack.
#[test]
#[ignore]
#[allow(clippy::print_stderr)]
fn w450_two_cell_door_emits_both_halves() {
    use crate::lighting::HaloLighting;
    use crate::resource_pack;
    use infengine_voxel::{BlockId, Chunk};

    let pack = resource_pack::ResourcePack::load();
    if pack.is_fallback {
        eprintln!("no MC pack - vacuous");
        return;
    }
    let halo = HaloLighting {
        own: None,
        px: None,
        nx: None,
        pz: None,
        nz: None,
        py: None,
        ny: None,
    };
    for (upper_id, label) in [(77u16, "village same-id"), (78, "player pair")] {
        let mut chunk = Chunk::empty();
        for z in 0..infengine_voxel::CHUNK_SIZE {
            for x in 0..infengine_voxel::CHUNK_SIZE {
                chunk.set(x, 0, z, BlockId(1));
            }
        }
        chunk.set(8, 1, 8, BlockId(77));
        chunk.set(8, 2, 8, BlockId(upper_id));
        let mut doors = std::collections::HashMap::new();
        doors.insert([8i32, 1, 8], (0u8, false));
        let subs = crate::workers::mesh_chunk_in_worker(
            &chunk,
            [0, 0, 0],
            &pack,
            &halo,
            &doors,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &crate::workers::BoundaryHalo::empty(),
        );
        let mut lower_verts = 0usize;
        let mut upper_verts = 0usize;
        for s in &subs {
            for v in &s.vertices {
                let (x, y, z) = (v.position[0], v.position[1], v.position[2]);
                if (7.9..=9.1).contains(&x) && (7.9..=9.1).contains(&z) {
                    if (1.0..2.0).contains(&y) {
                        lower_verts += 1;
                    } else if (2.0..3.0).contains(&y) {
                        upper_verts += 1;
                    }
                }
            }
        }
        eprintln!(
            "{label}: lower cell verts={lower_verts} upper cell verts={upper_verts}",
        );
        assert!(lower_verts > 0, "{label}: lower half emitted nothing");
        assert!(upper_verts > 0, "{label}: upper half emitted nothing");
    }
}

/// W450 diagnostic: freshly generated village chunks must contain
/// BOTH door cells (lower + upper). Distinguishes a live carve bug
/// from pre-W311 saves that baked air above every template door.
#[test]
#[ignore]
#[allow(clippy::print_stderr)]
fn w450_new_village_doors_are_two_cells() {
    use infengine_voxel::CHUNK_SIZE;
    let ctx = crate::worldgen::WorldgenCtx {
        base_seed: 3616343644,
        dim: "overworld".into(),
        profile: crate::worldgen::WorldGen::Mc,
    };
    let seed = ctx.effective_seed();
    let mut start = None;
    'scan: for gx in -6..=6 {
        for gz in -6..=6 {
            let [sx, sz] =
                crate::worldgen::mcgen::village::potential_start(seed, gx, gz);
            if crate::worldgen::mcgen::village::layout_for_start(&ctx, sx, sz)
                .is_some()
            {
                start = Some((sx, sz));
                break 'scan;
            }
        }
    }
    let Some((sx, sz)) = start else {
        eprintln!("no village for this seed - vacuous");
        return;
    };
    let cs = CHUNK_SIZE as i32;
    let mut doors_lower = 0usize;
    let mut doors_upper = 0usize;
    let mut orphan_lowers = 0usize;
    for dcx in -3..=3 {
        for dcz in -3..=3 {
            // Generate a full column of slices so vertical spans hold.
            let mut column: std::collections::HashMap<i32, infengine_voxel::Chunk> =
                std::collections::HashMap::new();
            for cy in 3..=6 {
                column.insert(
                    cy,
                    crate::worldgen::generate_chunk_at_ctx(
                        [sx + dcx, cy, sz + dcz],
                        &ctx,
                    ),
                );
            }
            let get = |wy: i32, lx: usize, lz: usize| -> u16 {
                let cy = wy.div_euclid(cs);
                let ly = wy.rem_euclid(cs) as usize;
                column
                    .get(&cy)
                    .map(|c| c.get(lx, ly, lz).0)
                    .unwrap_or(0)
            };
            for lx in 0..CHUNK_SIZE {
                for lz in 0..CHUNK_SIZE {
                    for wy in (3 * cs)..(7 * cs - 1) {
                        let id = get(wy, lx, lz);
                        if !crate::block::door::is_door_block(id) {
                            continue;
                        }
                        let below = get(wy - 1, lx, lz);
                        let above = get(wy + 1, lx, lz);
                        if crate::block::door::is_door_block(below) {
                            doors_upper += 1;
                        } else {
                            doors_lower += 1;
                            if !crate::block::door::is_door_block(above) {
                                orphan_lowers += 1;
                                eprintln!(
                                    "orphan lower at chunk ({},{}) local ({lx},{lz}) wy={wy} above={above}",
                                    sx + dcx,
                                    sz + dcz,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "doors: lowers={doors_lower} uppers={doors_upper} orphans={orphan_lowers}",
    );
    assert!(doors_lower > 0, "village has no doors at all?");
    assert_eq!(orphan_lowers, 0, "lower door halves without an upper");
}

/// W456: pillar-axis records die at GENERATION when the final chunk
/// contradicts them - horizontal grain on a trunk-stacked cell is
/// the fallen-log-overwritten-by-tree artifact (the R36 "rings
/// facing forward" trunk).
#[test]
fn w456_trunk_stacked_pillar_records_drop() {
    use crate::worldgen::mcgen::village::CarvedOriented;
    use infengine_voxel::{BlockId, Chunk};
    let log = crate::block::BLOCK_TYPES
        .iter()
        .find(|b| crate::block::is_pillar_block(b.id))
        .map(|b| b.id)
        .expect("a pillar block exists");
    let mut chunk = Chunk::empty();
    // Vertical trunk at (4, 3..=5, 4); lone fallen log at (9, 3, 9).
    for y in 3..=5 {
        chunk.set(4, y, 4, BlockId(log));
    }
    chunk.set(9, 3, 9, BlockId(log));
    let corg = [0, 0, 0];
    let rec = |pos: [i32; 3], kind: u8, state: u8| CarvedOriented {
        pos,
        kind,
        state,
        open: false,
    };
    // Trunk-stacked cell with horizontal grain: drop.
    assert!(!crate::workers::keep_oriented_record(
        &rec([4, 4, 4], 5, 1),
        &chunk,
        corg,
    ));
    // A lone fallen log keeps its horizontal grain.
    assert!(crate::workers::keep_oriented_record(
        &rec([9, 3, 9], 5, 2),
        &chunk,
        corg,
    ));
    // Overwritten by a non-pillar block: drop (W421).
    assert!(!crate::workers::keep_oriented_record(
        &rec([1, 1, 1], 5, 1),
        &chunk,
        corg,
    ));
    // Out-of-chunk records drop; stairs records pass untouched.
    assert!(!crate::workers::keep_oriented_record(
        &rec([40, 3, 40], 5, 1),
        &chunk,
        corg,
    ));
    assert!(crate::workers::keep_oriented_record(
        &rec([1, 1, 1], 0, 1),
        &chunk,
        corg,
    ));
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
    let subs = crate::workers::mesh_chunk_in_worker(&chunk, [0, 0, 0], &pack, &halo, &no_doors, &no_rails, &std::collections::HashMap::new(), &std::collections::HashMap::new(), &crate::workers::BoundaryHalo::empty());
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
        &crate::workers::BoundaryHalo::empty(),
    );
    let total_verts: usize = subs.iter().map(|s| s.vertices.len()).sum();
    assert_eq!(
        total_verts, 20,
        "snow slab must emit 5 quads (got {total_verts} verts from {} submeshes)",
        subs.len()
    );
}
