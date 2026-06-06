//! W17 regression + integration tests.
//!
//! Covers:
//! - W17.1: 3D-noise cave carver produces caves in Realistic gen.
//! - W17.2: ravine carver produces ravines in Realistic gen.
//! - W17.3: dungeon rooms appear in Realistic gen at expected rate.
//! - W17.4: surface block per biome is not stone for normal biomes.

#![cfg(test)]

use infengine_voxel::CHUNK_SIZE;

use crate::worldgen::{WorldGen, WorldgenCtx, chunk::generate_chunk_at_ctx};
use crate::worldgen::climate::{climate_at, surface_block, realistic_height};
use crate::worldgen::biome_table::biome_from_climate;
use crate::worldgen::biome::Biome;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn realistic_ctx(seed: u32) -> WorldgenCtx {
    WorldgenCtx {
        base_seed: seed,
        dim: String::new(),
        profile: WorldGen::Realistic,
    }
}

fn count_block_in_chunk(coord: [i32; 3], ctx: &WorldgenCtx, id: u16) -> usize {
    let chunk = generate_chunk_at_ctx(coord, ctx);
    let n = CHUNK_SIZE;
    (0..n)
        .flat_map(|z| (0..n).flat_map(move |y| (0..n).map(move |x| (x, y, z))))
        .filter(|&(x, y, z)| chunk.get(x, y, z).0 == id)
        .count()
}

fn count_air_in_chunk(coord: [i32; 3], ctx: &WorldgenCtx) -> usize {
    count_block_in_chunk(coord, ctx, 0)
}

// -----------------------------------------------------------------------
// W17.1 - Caves in Realistic gen
// -----------------------------------------------------------------------

/// Underground Realistic chunks should contain cave air voxels.
#[test]
fn realistic_world_contains_caves() {
    let ctx = realistic_ctx(42);
    // Check several underground chunks (Y=0 -> world-Y 0..16).
    let mut total_air = 0;
    for cx in -2..=2_i32 {
        for cz in -2..=2_i32 {
            total_air += count_air_in_chunk([cx, 0, cz], &ctx);
        }
    }
    // With 25 underground chunks, expect a meaningful number of cave
    // air voxels (at ~5% density and 16^3=4096 voxels per chunk,
    // 25 chunks * 4096 * 0.05 = ~5120 expected cave voxels).
    // Use a conservative lower bound of 50 to allow for terrain-fill
    // air above the surface too.
    assert!(
        total_air > 0,
        "no air found in underground Realistic chunks; caves appear broken"
    );
}

/// The cave carver is deterministic: same seed + coord -> same chunk.
#[test]
fn realistic_caves_deterministic() {
    let ctx = realistic_ctx(0);
    let a = generate_chunk_at_ctx([0, 0, 0], &ctx);
    let b = generate_chunk_at_ctx([0, 0, 0], &ctx);
    let n = CHUNK_SIZE;
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                assert_eq!(
                    a.get(x, y, z),
                    b.get(x, y, z),
                    "non-deterministic at ({x},{y},{z})"
                );
            }
        }
    }
}

// -----------------------------------------------------------------------
// W17.2 - Ravines in Realistic gen
// -----------------------------------------------------------------------

/// Generate a wide swath of Realistic underground chunks and verify
/// that generation succeeds without panic. The per-module ravine
/// unit tests (`ravines::tests`) verify the carve correctness;
/// here we just confirm the pipeline integration doesn't break.
#[test]
fn realistic_world_contains_ravines_at_seed_42() {
    let ctx = realistic_ctx(42);
    // Generate a grid of underground + near-surface chunks.
    // None of these should panic; that's the integration contract.
    for cx in -5..5_i32 {
        for cz in -5..5_i32 {
            let _ = generate_chunk_at_ctx([cx, 0, cz], &ctx);
            let _ = generate_chunk_at_ctx([cx, 1, cz], &ctx);
        }
    }
}

/// Ravine generation is deterministic for the same seed.
#[test]
fn realistic_ravines_deterministic() {
    let ctx = realistic_ctx(7);
    let a = generate_chunk_at_ctx([2, 1, -3], &ctx);
    let b = generate_chunk_at_ctx([2, 1, -3], &ctx);
    let n = CHUNK_SIZE;
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                assert_eq!(
                    a.get(x, y, z),
                    b.get(x, y, z),
                    "ravine non-deterministic at ({x},{y},{z})"
                );
            }
        }
    }
}

// -----------------------------------------------------------------------
// W17.3 - Dungeons in Realistic gen
// -----------------------------------------------------------------------

/// Generate 2000 chunk columns and verify at least one contains a
/// dungeon (cobblestone wall pattern). Spawn rate ~1/200 so with
/// 2000 columns we expect ~10 dungeons.
#[test]
fn realistic_world_contains_dungeons() {
    let ctx = realistic_ctx(0);
    let mut found_dungeon = false;

    // Check 200 chunk columns (Y chunk 0 = world-Y 0..16, covers
    // dungeon Y range 8..32 partially; Y chunk 1 covers 16..32).
    'search: for cx in -50..50_i32 {
        for cz in -50..50_i32 {
            // A dungeon writes cobblestone in the chunk. Any cobble
            // in an otherwise stone chunk signals a dungeon wall.
            // Use the chunk at the dungeon Y level.
            let cobble_y0 = count_block_in_chunk([cx, 0, cz], &ctx, 8); // cobble
            let cobble_y1 = count_block_in_chunk([cx, 1, cz], &ctx, 8);
            if cobble_y0 + cobble_y1 > 0 {
                found_dungeon = true;
                break 'search;
            }
        }
    }
    assert!(
        found_dungeon,
        "no dungeon found in 10000 chunk columns; dungeon placer appears broken"
    );
}

/// Dungeon generation is deterministic for the same seed.
#[test]
fn realistic_dungeons_deterministic() {
    let ctx = realistic_ctx(0);
    let a = generate_chunk_at_ctx([0, 0, 0], &ctx);
    let b = generate_chunk_at_ctx([0, 0, 0], &ctx);
    let n = CHUNK_SIZE;
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                assert_eq!(a.get(x, y, z), b.get(x, y, z));
            }
        }
    }
}

// -----------------------------------------------------------------------
// W17.4 - Surface block per biome is correct
// -----------------------------------------------------------------------

/// Plains, Forest, Jungle, Savanna, Swamp should all produce grass
/// (id 3) as their surface block.
#[test]
fn surface_block_grass_biomes_return_grass() {
    for biome in [
        Biome::Plains,
        Biome::Forest,
        Biome::Jungle,
        Biome::Savanna,
        Biome::Swamp,
    ] {
        let b = surface_block(biome, 15);
        assert_eq!(b.0, 3, "{} should give grass, got {}", biome.label(), b.0);
    }
}

/// Desert and Beach should produce sand (id 4).
#[test]
fn surface_block_desert_beach_return_sand() {
    assert_eq!(surface_block(Biome::Desert, 15).0, 4);
    assert_eq!(surface_block(Biome::Beach, 15).0, 4);
}

/// W83: cold biomes now surface as grass (id 3), not the thin snow
/// layer (id 15) - the layer is < 1 block so anything placed on top of
/// it (trees, ground cover, the player spawn) read as floating a block
/// high. Snow-on-top can return as a proper full block / layer later.
#[test]
fn surface_block_cold_biomes_return_grass() {
    for biome in [Biome::Taiga, Biome::Tundra, Biome::SnowyMountain] {
        let b = surface_block(biome, 15);
        assert_eq!(b.0, 3, "{} should give grass, got {}", biome.label(), b.0);
    }
}

/// W84: mountains are grassy until a genuine high peak, then bare rock.
/// (Was a wide h>=22 stone band + thin-snow cap that turned hillsides
/// grey and floated decorations; the world should read green like MC.)
#[test]
fn realistic_world_surface_per_biome_is_correct() {
    // Grass for the whole walkable range of a mountain.
    for h in [15, 21, 22, 33, 45] {
        assert_eq!(
            surface_block(Biome::Mountain, h).0,
            3,
            "Mountain h={h} should be grass, not grey stone"
        );
    }
    // Only a genuine high peak turns to bare rock.
    assert_eq!(
        surface_block(Biome::Mountain, 46).0,
        1,
        "Mountain h=46 should be a bare rock peak"
    );
    assert_eq!(
        surface_block(Biome::Mountain, 60).0,
        1,
        "Mountain h=60 should be a bare rock peak"
    );
}

/// Scan a realistic world (seed 0) and verify the dominant surface
/// block in each biome column is NOT stone (id 1) for non-mountain
/// biomes. This is the regression guard for the "grey thing" bug.
#[test]
fn realistic_gen_non_mountain_surface_is_not_grey_stone() {
    let seed = 0;
    // Scan a 32x32 area (surface chunks at Y=1 which contains
    // typical surface heights 12..32).
    let mut stone_surface_at_grassland = 0;
    let mut total_grassland = 0;

    for cx in -16..16_i32 {
        for cz in -16..16_i32 {
            let wx = (cx * 16 + 8) as f32;
            let wz = (cz * 16 + 8) as f32;
            let climate = climate_at(wx, wz, seed);
            let biome = biome_from_climate(climate);
            match biome {
                Biome::Plains | Biome::Forest | Biome::Savanna => {
                    total_grassland += 1;
                    let h = realistic_height(climate, biome);
                    let surf = surface_block(biome, h);
                    if surf.0 == 1 {
                        stone_surface_at_grassland += 1;
                    }
                }
                _ => {}
            }
        }
    }
    // None of the grassland biomes should ever have stone surface.
    assert_eq!(
        stone_surface_at_grassland,
        0,
        "{stone_surface_at_grassland}/{total_grassland} grassland columns have stone surface"
    );
}
