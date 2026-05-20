//! Integration tests for L.4: confirm `LightGrid` reads route into
//! the chunk mesher's per-vertex `light` attribute correctly.

use glam::Vec3;
use infengine_voxel::{
    BlockId, CHUNK_SIZE, Chunk, ChunkSubmesh, chunk_mesh_with_atlas_and_light,
};

use super::block_engine::propagate_block_light;
use super::sky_engine::propagate_sky_light;
use super::LightingState;
use crate::block::{TORCH_ID, block_is_opaque};

fn mesh_with_light(
    chunk: &Chunk,
    chunk_coord: [i32; 3],
    state: &LightingState,
) -> Vec<ChunkSubmesh> {
    let cs = CHUNK_SIZE as i32;
    let world_origin = [
        chunk_coord[0] * cs,
        chunk_coord[1] * cs,
        chunk_coord[2] * cs,
    ];
    let origin = Vec3::new(
        world_origin[0] as f32,
        world_origin[1] as f32,
        world_origin[2] as f32,
    );
    chunk_mesh_with_atlas_and_light(
        chunk,
        origin,
        |_, _| [0.0, 0.0, 1.0, 1.0],
        block_is_opaque,
        |air_local, _normal| {
            let world = [
                world_origin[0] + air_local[0],
                world_origin[1] + air_local[1],
                world_origin[2] + air_local[2],
            ];
            let other_cc = [
                world[0].div_euclid(cs),
                world[1].div_euclid(cs),
                world[2].div_euclid(cs),
            ];
            let other_lc = [
                world[0].rem_euclid(cs) as u32,
                world[1].rem_euclid(cs) as u32,
                world[2].rem_euclid(cs) as u32,
            ];
            match state.get(other_cc) {
                Some(g) => [
                    g.get_block(other_lc) as f32 / 15.0,
                    g.get_sky(other_lc) as f32 / 15.0,
                ],
                None => [0.0, 1.0],
            }
        },
    )
}

#[test]
fn open_chunk_top_face_gets_full_sky_light_attribute() {
    let mut chunk = Chunk::empty();
    chunk.set(8, 8, 8, BlockId(1));
    let mut world = infengine_voxel::World::new();
    world.insert_chunk([0, 0, 0], chunk);
    let mut s = LightingState::new();
    propagate_sky_light(&world, &mut s, &[[0, 0, 0]]);
    let chunk_ref = world.chunk([0, 0, 0]).unwrap();
    let subs = mesh_with_light(chunk_ref, [0, 0, 0], &s);
    // The block's +Y face faces (8, 9, 8) - an air cell above the
    // block that the column walk reached at sky=15. All four
    // +Y vertices must have `light.y = 1.0`.
    let stone = subs.iter().find(|s| s.block == BlockId(1)).unwrap();
    let plus_y: Vec<_> = stone
        .vertices
        .iter()
        .filter(|v| (v.normal()[1] - 1.0).abs() < 0.5)
        .collect();
    assert_eq!(plus_y.len(), 4);
    for v in plus_y {
        assert!(
            (v.light()[1] - 1.0).abs() < 1e-4,
            "expected sky=1.0, got {}",
            v.light()[1]
        );
    }
    // The -Y face faces (8, 7, 8) - the air directly below the
    // block which the column walk zeroed out (the block itself
    // blocks the column). It gets re-lit by the horizontal BFS
    // off adjacent sky-15 cells, so sky < 1.0.
    let minus_y: Vec<_> = stone
        .vertices
        .iter()
        .filter(|v| (v.normal()[1] + 1.0).abs() < 0.5)
        .collect();
    for v in minus_y {
        assert!(v.light()[1] < 1.0, "expected sky<1.0 under block, got {}", v.light()[1]);
    }
}

#[test]
fn torch_neighbour_face_gets_high_block_light() {
    // Solid block at (5, 8, 8) with a torch right next to it at
    // (6, 8, 8). The +X face of the solid faces the torch's air
    // cell, so that face's vertices see block_light close to
    // 14/15 (= 0.933).
    let mut chunk = Chunk::empty();
    chunk.set(5, 8, 8, BlockId(1));
    chunk.set(6, 8, 8, BlockId(TORCH_ID));
    let mut world = infengine_voxel::World::new();
    world.insert_chunk([0, 0, 0], chunk);
    let mut s = LightingState::new();
    propagate_block_light(&world, &mut s, &[[0, 0, 0]]);
    let chunk_ref = world.chunk([0, 0, 0]).unwrap();
    let subs = mesh_with_light(chunk_ref, [0, 0, 0], &s);
    let stone = subs.iter().find(|s| s.block == BlockId(1)).unwrap();
    let plus_x: Vec<_> = stone
        .vertices
        .iter()
        .filter(|v| (v.normal()[0] - 1.0).abs() < 0.5)
        .collect();
    assert_eq!(plus_x.len(), 4);
    // Torch sits AT the air cell -> block_light = 14/15.
    let expected = 14.0 / 15.0;
    for v in plus_x {
        assert!(
            (v.light()[0] - expected).abs() < 1e-4,
            "expected block_light={}, got {}",
            expected,
            v.light()[0]
        );
    }
}

#[test]
fn covered_floor_face_under_ceiling_has_sky_zero() {
    // Stone slab at y=10 covering whole chunk. The slab's -Y
    // face faces a dark air cell at y=9 (sky=0 under the slab).
    let mut chunk = Chunk::empty();
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            chunk.set(x, 10, z, BlockId(1));
        }
    }
    let mut world = infengine_voxel::World::new();
    world.insert_chunk([0, 0, 0], chunk);
    let mut s = LightingState::new();
    propagate_sky_light(&world, &mut s, &[[0, 0, 0]]);
    let chunk_ref = world.chunk([0, 0, 0]).unwrap();
    let subs = mesh_with_light(chunk_ref, [0, 0, 0], &s);
    let stone = subs.iter().find(|s| s.block == BlockId(1)).unwrap();
    let minus_y: Vec<_> = stone
        .vertices
        .iter()
        .filter(|v| (v.normal()[1] + 1.0).abs() < 0.5)
        .collect();
    // Every -Y vertex (the slab's underside) has sky=0.
    for v in minus_y {
        assert!(v.light()[1].abs() < 1e-4, "expected sky=0, got {}", v.light()[1]);
    }
}

#[test]
fn boundary_face_samples_neighbour_chunk_when_known() {
    // Block at (15, 8, 8) in chunk [0,0,0]. Its +X face faces
    // chunk [1,0,0]'s air cell at (0, 8, 8). If the neighbour
    // chunk's lighting grid is present + lit, the face vertex
    // picks up that value.
    let mut chunk_a = Chunk::empty();
    chunk_a.set(15, 8, 8, BlockId(1));
    let chunk_b = Chunk::empty();
    let mut world = infengine_voxel::World::new();
    world.insert_chunk([0, 0, 0], chunk_a);
    world.insert_chunk([1, 0, 0], chunk_b);
    let mut s = LightingState::new();
    propagate_sky_light(&world, &mut s, &[[0, 0, 0], [1, 0, 0]]);
    let chunk_ref = world.chunk([0, 0, 0]).unwrap();
    let subs = mesh_with_light(chunk_ref, [0, 0, 0], &s);
    let stone = subs.iter().find(|s| s.block == BlockId(1)).unwrap();
    let plus_x: Vec<_> = stone
        .vertices
        .iter()
        .filter(|v| (v.normal()[0] - 1.0).abs() < 0.5)
        .collect();
    // Neighbour chunk's (0, 8, 8) cell sits under open sky -> 15.
    for v in plus_x {
        assert!((v.light()[1] - 1.0).abs() < 1e-4);
    }
}

#[test]
fn missing_neighbour_grid_falls_back_to_full_sky_default() {
    // No lighting state at all - the mesher's closure returns
    // the `[0.0, 1.0]` default. Every face should carry that.
    let mut chunk = Chunk::empty();
    chunk.set(4, 4, 4, BlockId(1));
    let mut world = infengine_voxel::World::new();
    world.insert_chunk([0, 0, 0], chunk);
    let s = LightingState::new();
    let chunk_ref = world.chunk([0, 0, 0]).unwrap();
    let subs = mesh_with_light(chunk_ref, [0, 0, 0], &s);
    let stone = subs.iter().find(|s| s.block == BlockId(1)).unwrap();
    for v in &stone.vertices {
        // W28.6: block=0, sky=1 -> with 1/255 rounding tolerance.
        let light = v.light();
        assert!(light[0].abs() < 1e-4, "expected block=0, got {}", light[0]);
        assert!((light[1] - 1.0).abs() < 1e-4, "expected sky=1, got {}", light[1]);
    }
}
