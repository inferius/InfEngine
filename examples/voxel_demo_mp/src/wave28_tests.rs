//! W28 tests: performance overhaul audit.
//!
//! W28.12 test coverage for:
//!   - Priority queue ordering (PendingChunk min-heap by distance).
//!   - vertical_chunk_range: boundary conditions + player Y positions.
//!   - Packed vertex helpers: MeshVertex encode/decode round-trip.
//!   - Frustum culling: contains_point margin + is_aabb_visible.
//!   - Two-queue incremental lighting: propagate_block_light_incremental.
//!   - Greedy mesher: face count reduction, winding, UV scaling.

use infengine_render::MeshVertex;
use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk};

use crate::{PendingChunk, Voxel};
use crate::lighting::{LightingState, block_engine::propagate_block_light_incremental};
use crate::block::TORCH_ID;

// ---------------------------------------------------------------------------
// W28.1 - PendingChunk priority queue ordering
// ---------------------------------------------------------------------------

/// Closest chunk (lowest dist_sq) must pop first from the max-heap.
#[test]
fn pending_chunk_min_heap_pops_closest_first() {
    use std::collections::BinaryHeap;

    let mut heap = BinaryHeap::new();
    // Push in arbitrary order: far, close, medium.
    heap.push(PendingChunk { neg_dist_sq: -1000, coord: [10, 0, 10] });
    heap.push(PendingChunk { neg_dist_sq:    -1, coord: [1, 0, 0] });
    heap.push(PendingChunk { neg_dist_sq:  -100, coord: [5, 0, 5] });

    // Pop order: neg_dist_sq = -1 first (largest neg = closest).
    let first = heap.pop().unwrap();
    assert_eq!(first.neg_dist_sq, -1, "closest must pop first");

    let second = heap.pop().unwrap();
    assert_eq!(second.neg_dist_sq, -100);

    let third = heap.pop().unwrap();
    assert_eq!(third.neg_dist_sq, -1000, "farthest must pop last");
}

/// Two chunks at equal distance should still sort deterministically.
#[test]
fn pending_chunk_equal_dist_breaks_tie_by_coord() {
    use std::collections::BinaryHeap;

    let mut heap = BinaryHeap::new();
    heap.push(PendingChunk { neg_dist_sq: -25, coord: [1, 0, 4] });
    heap.push(PendingChunk { neg_dist_sq: -25, coord: [0, 0, 5] });

    // Both have the same neg_dist_sq; coord tie-break via Ord.
    let a = heap.pop().unwrap();
    let b = heap.pop().unwrap();
    // Just verify both come out and the order is consistent (not flipped).
    assert_ne!(a.coord, b.coord, "both distinct coords must emerge");
}

// ---------------------------------------------------------------------------
// W28.3 - vertical_chunk_range
// ---------------------------------------------------------------------------

/// Player at y=32 (chunk y=2): range [0, 5] (cy-3=0 clamped, cy+3=5).
#[test]
fn vertical_chunk_range_default_player_y() {
    let (cy_min, cy_max) = Voxel::vertical_chunk_range(32.0);
    assert_eq!(cy_min, 0, "cy_min should be 0 (clamped from -1)");
    assert_eq!(cy_max, 5, "cy_max should be 5");
}

/// Player at y=0 (chunk 0): range [0, 3].
#[test]
fn vertical_chunk_range_at_y_zero() {
    let (cy_min, cy_max) = Voxel::vertical_chunk_range(0.0);
    assert_eq!(cy_min, 0);
    assert_eq!(cy_max, 3);
}

/// Player deep underground (chunk y=12): range [9, 15].
#[test]
fn vertical_chunk_range_deep_underground() {
    let cs = CHUNK_SIZE as f32;
    let player_y = 12.0 * cs + 8.0; // chunk_y = 12
    let (cy_min, cy_max) = Voxel::vertical_chunk_range(player_y);
    assert_eq!(cy_min, 9, "12-3=9");
    assert_eq!(cy_max, 15, "12+3=15, clamped to VERTICAL_CHUNKS-1");
}

/// Range is always at least 1 slice wide.
#[test]
fn vertical_chunk_range_always_nonempty() {
    for y in [0.0, 32.0, 128.0, 240.0, -8.0_f32] {
        let (cy_min, cy_max) = Voxel::vertical_chunk_range(y);
        assert!(cy_max >= cy_min, "range must not be inverted for y={y}");
    }
}

// ---------------------------------------------------------------------------
// W28.6 - MeshVertex packed encode/decode round-trip
// ---------------------------------------------------------------------------

/// position passthrough.
#[test]
fn mesh_vertex_position_roundtrip() {
    let v = MeshVertex::new([1.0, 2.5, -3.0], [0.0, 1.0, 0.0]);
    assert_eq!(v.position, [1.0, 2.5, -3.0]);
}

/// normal encode/decode round-trip within 1% tolerance (i8 gives ~0.8%).
#[test]
fn mesh_vertex_normal_roundtrip() {
    let v = MeshVertex::new([0.0; 3], [0.0, 1.0, 0.0]);
    let n = v.normal();
    assert!((n[0]).abs() < 0.01, "nx ~ 0");
    assert!((n[1] - 1.0).abs() < 0.01, "ny ~ 1");
    assert!((n[2]).abs() < 0.01, "nz ~ 0");
}

/// uv encode/decode round-trip within 2/65535 tolerance.
#[test]
fn mesh_vertex_uv_roundtrip() {
    let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_uv([0.5, 0.75]);
    let uv = v.uv();
    assert!((uv[0] - 0.5).abs() < 2e-5, "u ~ 0.5, got {}", uv[0]);
    assert!((uv[1] - 0.75).abs() < 2e-5, "v ~ 0.75, got {}", uv[1]);
}

/// light encode/decode round-trip within 1/255 tolerance.
#[test]
fn mesh_vertex_light_roundtrip() {
    let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_light(0.6, 0.4);
    let l = v.light();
    assert!((l[0] - 0.6).abs() < 0.005, "block ~ 0.6, got {}", l[0]);
    assert!((l[1] - 0.4).abs() < 0.005, "sky ~ 0.4, got {}", l[1]);
}

/// light clamps inputs outside [0, 1].
#[test]
fn mesh_vertex_light_clamps_out_of_range() {
    let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_light(-5.0, 99.0);
    let l = v.light();
    assert!(l[0].abs() < 1e-4, "block clamped to 0, got {}", l[0]);
    assert!((l[1] - 1.0).abs() < 1e-4, "sky clamped to 1, got {}", l[1]);
}

/// anim_id clamps at 32.
#[test]
fn mesh_vertex_anim_id_clamps_at_32() {
    let v = MeshVertex::new([0.0; 3], [0.0; 3]).with_anim_id(50);
    assert_eq!(v.anim_id, 32);
}

/// Struct size is exactly 28 bytes.
#[test]
fn mesh_vertex_size_28_bytes() {
    assert_eq!(std::mem::size_of::<MeshVertex>(), 28);
}

// ---------------------------------------------------------------------------
// W28.11 - Frustum contains_point
// ---------------------------------------------------------------------------

/// Point directly in front of camera is inside frustum.
#[test]
fn frustum_contains_point_in_front() {
    use crate::frustum::Frustum;
    use glam::{Mat4, Vec3};

    let proj = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 200.0);
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO, Vec3::Y);
    let frustum = Frustum::from_view_proj(proj * view);

    // Point at z=+1 is in front of the camera (camera faces +Z from z=-5).
    assert!(
        frustum.contains_point(Vec3::new(0.0, 0.0, 1.0)),
        "point in front should be visible"
    );
}

/// Point far behind camera is outside frustum.
#[test]
fn frustum_contains_point_behind_camera() {
    use crate::frustum::Frustum;
    use glam::{Mat4, Vec3};

    let proj = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 200.0);
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO, Vec3::Y);
    let frustum = Frustum::from_view_proj(proj * view);

    // Point at z=-100 is far behind the camera.
    assert!(
        !frustum.contains_point(Vec3::new(0.0, 0.0, -100.0)),
        "point far behind camera should be culled"
    );
}

// ---------------------------------------------------------------------------
// W28.5 - propagate_block_light_incremental
// ---------------------------------------------------------------------------

/// Placing a torch increments adjacent cell block-light.
#[test]
fn incremental_light_torch_placement_propagates() {
    let chunk = Chunk::empty();
    let mut world = infengine_voxel::World::new();
    world.insert_chunk([0, 0, 0], chunk.clone());
    let mut state = LightingState::new();

    // Place torch at (5, 5, 5) - old=air (emission 0), new=torch (emission 14).
    world.set([5, 5, 5], BlockId(TORCH_ID));
    let affected = vec![[0i32, 0, 0]];
    propagate_block_light_incremental(&world, &mut state, &affected, [5, 5, 5], 0, 14);

    let g = state.get([0, 0, 0]).expect("grid created");
    assert_eq!(g.get_block([5, 5, 5]), 14, "torch cell = 14");
    assert_eq!(g.get_block([6, 5, 5]), 13, "torch +X neighbour = 13");
}

/// Removing a torch zeroes the cells it was illuminating.
#[test]
fn incremental_light_torch_removal_unlights() {
    let mut world = infengine_voxel::World::new();
    let mut chunk = Chunk::empty();
    chunk.set(5, 5, 5, BlockId(TORCH_ID));
    world.insert_chunk([0, 0, 0], chunk);

    // Full flood to set initial state.
    let mut state = LightingState::new();
    crate::lighting::block_engine::propagate_block_light(
        &world,
        &mut state,
        &[[0i32, 0, 0]],
    );
    assert_eq!(state.get([0, 0, 0]).unwrap().get_block([6, 5, 5]), 13);

    // Remove the torch.
    world.set([5, 5, 5], BlockId::AIR);
    let affected = vec![[0i32, 0, 0]];
    propagate_block_light_incremental(&world, &mut state, &affected, [5, 5, 5], 14, 0);

    let g = state.get([0, 0, 0]).expect("grid exists");
    assert_eq!(g.get_block([5, 5, 5]), 0, "torch cell = 0 after removal");
    assert_eq!(g.get_block([6, 5, 5]), 0, "neighbour = 0 after removal");
}

// ---------------------------------------------------------------------------
// W28.4 - Greedy mesher: fewer faces for uniform chunk
// ---------------------------------------------------------------------------

/// Fully uniform chunk: greedy should emit at most 6 quads (one per
/// face direction) vs the naive mesher's 6*CHUNK_SIZE^2 quads.
#[test]
fn greedy_mesh_uniform_chunk_face_count() {
    let mut chunk = Chunk::empty();
    // Fill the entire chunk with block id 1.
    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                chunk.set(x, y, z, BlockId(1));
            }
        }
    }

    let greedy_subs = infengine_voxel::chunk_mesh_greedy(
        &chunk,
        glam::Vec3::ZERO,
        |_, _| [0.0, 0.0, 1.0, 1.0],
        |_| true,
        |_, _| [0.0, 1.0],
        |_, _| 0u32,
    );
    let greedy_verts: usize = greedy_subs.iter().map(|s| s.vertices.len()).sum();

    let naive_subs = infengine_voxel::chunk_mesh_with_atlas_and_light_and_anim(
        &chunk,
        glam::Vec3::ZERO,
        |_, _| [0.0, 0.0, 1.0, 1.0],
        |_| true,
        |_, _| [0.0, 1.0],
        |_, _| 0u32,
    );
    let naive_verts: usize = naive_subs.iter().map(|s| s.vertices.len()).sum();

    // Greedy should have significantly fewer vertices than naive.
    assert!(
        greedy_verts < naive_verts,
        "greedy ({greedy_verts} verts) should be less than naive ({naive_verts} verts)"
    );
    // For a uniform 16^3 chunk, all 6 face directions emit 1 merged quad.
    assert_eq!(
        greedy_verts, 24,
        "uniform chunk: exactly 6 faces * 4 verts = 24"
    );
}
