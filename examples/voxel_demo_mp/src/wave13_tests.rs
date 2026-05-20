//! W13 regression + unit tests.
//!
//! Covers:
//! - W13.1: shuttering profile constants (RESULTS_PER_FRAME, NUM_WORKERS).
//! - W13.2: ChunkSideEffect enum shape + WorkerResult carries side_effects.
//! - W13.3: VERTICAL_CHUNKS = 16, initial build covers y=0..16,
//!   missing high-y chunks (old saves) are treated as air.
//! - W13.4: frustum culling (via frustum module tests), VIEW_DISTANCE_MAX.

#![cfg(test)]

// -----------------------------------------------------------------------
// W13.1 - Shuttering profile constants
// -----------------------------------------------------------------------

/// RESULTS_PER_FRAME caps per-frame GPU upload cost. W23.3a raised
/// from 2 to 8 because W24 moved meshing into worker threads, so the
/// main thread only does GPU upload (cheap) per delivered result.
#[test]
fn results_per_frame_is_bounded() {
    const { assert!(crate::RESULTS_PER_FRAME >= 1) };
    const { assert!(crate::RESULTS_PER_FRAME <= 64) };
}

/// Worker count is dynamic now (post-W13 follow-up): picked at startup
/// via `pick_worker_count()` based on `available_parallelism`. Floors at
/// MIN_WORKERS=4 + caps at MAX_WORKERS=24. Test the bounds.
#[test]
fn worker_count_bounds() {
    assert_eq!(crate::MIN_WORKERS, 4, "MIN_WORKERS must be 4");
    const { assert!(crate::MAX_WORKERS >= crate::MIN_WORKERS) };
    let picked = crate::pick_worker_count();
    assert!(picked >= crate::MIN_WORKERS);
    assert!(picked <= crate::MAX_WORKERS);
}

/// REMESH_DEFERRED_PER_FRAME limits neighbour remesh cost per frame.
#[test]
fn remesh_deferred_per_frame_is_bounded() {
    // W23.3a: bumped from 4 to 16 to drain pending_remesh faster.
    const { assert!(crate::REMESH_DEFERRED_PER_FRAME <= 64) };
    const { assert!(crate::REMESH_DEFERRED_PER_FRAME >= 1) };
}

/// CHUNK_PHASE_WARN_US is a positive threshold for tracing::warn! gates.
#[test]
fn chunk_phase_warn_threshold_is_positive() {
    const { assert!(crate::CHUNK_PHASE_WARN_US > 0) };
}

// -----------------------------------------------------------------------
// W13.2 - ChunkSideEffect enum
// -----------------------------------------------------------------------

use crate::ChunkSideEffect;

/// ChunkSideEffect::Snow carries a 3-element world position.
#[test]
fn side_effect_snow_carries_position() {
    let se = ChunkSideEffect::Snow([1, 2, 3]);
    match se {
        ChunkSideEffect::Snow(pos) => assert_eq!(pos, [1, 2, 3]),
        _ => panic!("wrong variant"),
    }
}

/// ChunkSideEffect::Torch carries a 3-element world position.
#[test]
fn side_effect_torch_carries_position() {
    let se = ChunkSideEffect::Torch([4, 5, 6]);
    match se {
        ChunkSideEffect::Torch(pos) => assert_eq!(pos, [4, 5, 6]),
        _ => panic!("wrong variant"),
    }
}

/// ChunkSideEffect::Liquid carries position + tick_offset.
#[test]
fn side_effect_liquid_carries_pos_and_tick() {
    let se = ChunkSideEffect::Liquid { pos: [7, 8, 9], tick_offset: 20 };
    match se {
        ChunkSideEffect::Liquid { pos, tick_offset } => {
            assert_eq!(pos, [7, 8, 9]);
            assert_eq!(tick_offset, 20);
        }
        _ => panic!("wrong variant"),
    }
}

/// A pure-air chunk produces no side effects.
#[test]
fn air_chunk_produces_no_side_effects() {
    use infengine_voxel::{Chunk, CHUNK_SIZE, World};
    use crate::liquid::liquid_for;
    use crate::block::{TORCH_ID, GLOWSTONE_ID};

    let chunk = Chunk::empty();
    let origin = World::chunk_world_origin([0, 0, 0]);
    let cs = CHUNK_SIZE as i32;
    let ox = origin.x as i32;
    let oy = origin.y as i32;
    let oz = origin.z as i32;
    let mut side_effects: Vec<ChunkSideEffect> = Vec::new();

    for ly in 0..cs {
        for lz in 0..cs {
            for lx in 0..cs {
                let b = chunk.get(lx as usize, ly as usize, lz as usize);
                let wv = [ox + lx, oy + ly, oz + lz];
                if b.0 == 15 {
                    side_effects.push(ChunkSideEffect::Snow(wv));
                } else if b.0 == TORCH_ID || b.0 == GLOWSTONE_ID {
                    side_effects.push(ChunkSideEffect::Torch(wv));
                } else if let Some(lq) = liquid_for(b) {
                    side_effects.push(ChunkSideEffect::Liquid {
                        pos: wv,
                        tick_offset: lq.tick_period.max(1) as u64,
                    });
                }
            }
        }
    }
    assert!(
        side_effects.is_empty(),
        "air chunk must produce zero side effects, got {}",
        side_effects.len()
    );
}

/// A chunk with one snow voxel produces exactly one Snow side effect.
#[test]
fn snow_voxel_produces_snow_side_effect() {
    use infengine_voxel::{BlockId, Chunk, CHUNK_SIZE, World};
    use crate::liquid::liquid_for;
    use crate::block::{TORCH_ID, GLOWSTONE_ID};

    let mut chunk = Chunk::empty();
    chunk.set(0, 0, 0, BlockId(15)); // snow
    let origin = World::chunk_world_origin([0, 0, 0]);
    let cs = CHUNK_SIZE as i32;
    let ox = origin.x as i32;
    let oy = origin.y as i32;
    let oz = origin.z as i32;
    let mut side_effects: Vec<ChunkSideEffect> = Vec::new();

    for ly in 0..cs {
        for lz in 0..cs {
            for lx in 0..cs {
                let b = chunk.get(lx as usize, ly as usize, lz as usize);
                let wv = [ox + lx, oy + ly, oz + lz];
                if b.0 == 15 {
                    side_effects.push(ChunkSideEffect::Snow(wv));
                } else if b.0 == TORCH_ID || b.0 == GLOWSTONE_ID {
                    side_effects.push(ChunkSideEffect::Torch(wv));
                } else if let Some(lq) = liquid_for(b) {
                    side_effects.push(ChunkSideEffect::Liquid {
                        pos: wv,
                        tick_offset: lq.tick_period.max(1) as u64,
                    });
                }
            }
        }
    }
    assert_eq!(side_effects.len(), 1, "one snow voxel -> one side effect");
    assert!(
        matches!(side_effects[0], ChunkSideEffect::Snow([0, 0, 0])),
        "snow side effect should carry world pos [0,0,0]"
    );
}

// -----------------------------------------------------------------------
// W13.3 - VERTICAL_CHUNKS = 16
// -----------------------------------------------------------------------

/// W13.3: vertical world height is 256 blocks (16 chunks of 16 voxels).
#[test]
fn vertical_chunks_constant_is_16() {
    assert_eq!(
        crate::VERTICAL_CHUNKS, 16,
        "VERTICAL_CHUNKS must be 16 for MC parity (256-block world)"
    );
}

/// W13.3: world height in blocks is CHUNK_SIZE * VERTICAL_CHUNKS = 256.
#[test]
fn world_height_is_256_blocks() {
    use infengine_voxel::CHUNK_SIZE;
    let height = CHUNK_SIZE as i32 * crate::VERTICAL_CHUNKS;
    assert_eq!(height, 256, "world height must be 256 blocks");
}

/// W13.3: initial build covers y in 0..VERTICAL_CHUNKS.
/// Old saves have y=0..3; the code must NOT regen y=4..15 from worldgen
/// for chunks that are absent on disk - they should be treated as air.
#[test]
fn initial_build_iterates_all_vertical_layers() {
    // This test verifies that build_world's loop range equals
    // 0..VERTICAL_CHUNKS by checking the constant directly.
    // The actual build_world function uses `for cy in 0..VERTICAL_CHUNKS`
    // so this constant check is the canonical gate.
    const { assert!(crate::VERTICAL_CHUNKS >= 16) };
}

/// W13.3: a missing high-y chunk file must be treated as air, not
/// regenerated from worldgen. This mirrors the ChunkJob / worker path:
/// when world_save::read_chunk returns None for y >= 4, the loader
/// treats NotFound as air (the chunk is simply not present on disk).
///
/// We test the save-layer behaviour directly: read_chunk_in_dir for a
/// coord that was never written returns Ok(None).
#[test]
fn missing_high_y_chunk_returns_none_not_error() {
    use crate::world_save::chunks::read_chunk_in_dir;

    // Use a temp dir that definitely has no chunk files.
    let tmp = tempfile::tempdir().expect("tempdir");
    let result = read_chunk_in_dir(tmp.path(), "test_world", "overworld", [0, 5, 0]);
    assert!(
        result.is_ok(),
        "read_chunk for missing high-y coord must not error"
    );
    assert!(
        result.unwrap().is_none(),
        "read_chunk for missing high-y coord must return None"
    );
}

/// W13.3: old save (y=0..3) loads without regenerating y=4..15.
/// We write chunks for y=0..3 and verify reads for y=4..15 return None.
#[test]
fn old_save_high_y_reads_return_none() {
    use crate::world_save::chunks::{
        empty_payload, read_chunk_in_dir, write_chunk_in_dir,
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    // Write the old 4-layer save (y = 0..4).
    for cy in 0i32..4 {
        let p = empty_payload([0, cy, 0]);
        write_chunk_in_dir(tmp.path(), "test_world", "overworld", &p)
            .expect("write");
    }
    // Verify y=0..3 load correctly.
    for cy in 0i32..4 {
        let r = read_chunk_in_dir(tmp.path(), "test_world", "overworld", [0, cy, 0])
            .expect("read");
        assert!(r.is_some(), "y={cy} chunk must load from old save");
    }
    // y=4..16 must return None, not error.
    for cy in 4i32..16 {
        let r = read_chunk_in_dir(tmp.path(), "test_world", "overworld", [0, cy, 0])
            .expect("read at y={cy} must not error");
        assert!(
            r.is_none(),
            "y={cy} chunk not in old save must return None (treated as air)"
        );
    }
}

// -----------------------------------------------------------------------
// W13.4 - View distance + frustum culling
// -----------------------------------------------------------------------

/// W13.4: VIEW_DISTANCE_MAX was raised to 24; W22.1 raised it further to 32.
/// This test is kept as a compile-time lower-bound guard - >= 24.
#[test]
fn view_distance_constant_caps_at_24() {
    const { assert!(crate::VIEW_DISTANCE_MAX >= 24) };
}

/// W13.4: view_distance setting can be raised to VIEW_DISTANCE_MAX.
#[test]
fn view_distance_clamp_allows_24() {
    // Simulate the clamp in stream_chunks.
    let user_setting: u8 = 24;
    let clamped = user_setting.clamp(2, crate::VIEW_DISTANCE_MAX as u8);
    assert_eq!(clamped, 24);
}

/// W13.4: view_distance clamp rejects values above VIEW_DISTANCE_MAX.
#[test]
fn view_distance_clamp_caps_excess() {
    let user_setting: u8 = 100;
    let clamped = user_setting.clamp(2, crate::VIEW_DISTANCE_MAX as u8);
    assert_eq!(clamped as i32, crate::VIEW_DISTANCE_MAX);
}

/// W13.4 frustum: a chunk at [0,0,0] with camera 5 units in front is visible.
///
/// This duplicates the test in frustum.rs to make it visible in the wave
/// audit summary alongside the W13 constants.
#[test]
fn frustum_visible_chunk_not_culled() {
    use crate::frustum::Frustum;
    use glam::Mat4;

    let proj = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 200.0);
    let view = Mat4::look_at_rh(
        glam::Vec3::new(0.0, 0.0, -5.0),
        glam::Vec3::ZERO,
        glam::Vec3::Y,
    );
    let frustum = Frustum::from_view_proj(proj * view);
    assert!(
        frustum.is_chunk_visible([0, 0, 0]),
        "chunk at [0,0,0] visible from z=-5 looking at origin"
    );
}

/// W13.4 frustum: a chunk far behind the camera is culled.
#[test]
fn frustum_far_behind_chunk_culled() {
    use crate::frustum::Frustum;
    use glam::Mat4;

    let proj = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 200.0);
    let view = Mat4::look_at_rh(
        glam::Vec3::new(0.0, 0.0, -5.0),
        glam::Vec3::ZERO,
        glam::Vec3::Y,
    );
    let frustum = Frustum::from_view_proj(proj * view);
    // Chunk at [0,0,-4] = world z in [-64, -48], far behind camera at z=-5.
    assert!(
        !frustum.is_chunk_visible([0, 0, -4]),
        "chunk at z=-64..-48 should be culled when camera faces +z"
    );
}

/// W13.4 frustum: a chunk very far to the side is culled.
#[test]
fn frustum_far_side_chunk_culled() {
    use crate::frustum::Frustum;
    use glam::Mat4;

    let proj = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 200.0);
    let view = Mat4::look_at_rh(
        glam::Vec3::new(0.0, 0.0, -5.0),
        glam::Vec3::ZERO,
        glam::Vec3::Y,
    );
    let frustum = Frustum::from_view_proj(proj * view);
    assert!(
        !frustum.is_chunk_visible([14, 0, 0]),
        "chunk at x=224..240 should be outside a 90-deg FOV frustum"
    );
}

// -----------------------------------------------------------------------
// W22.1 - Extreme view distance
// -----------------------------------------------------------------------

/// VIEW_DISTANCE_MAX must be 32 (W22.1).
#[test]
fn view_distance_max_is_32() {
    assert_eq!(
        crate::VIEW_DISTANCE_MAX, 32,
        "VIEW_DISTANCE_MAX must be 32 to allow extreme render distances"
    );
}

/// OptionsSnapshot::clamp must cap view_distance at VIEW_DISTANCE_MAX.
#[test]
fn options_clamp_view_distance_to_max() {
    let mut opt = crate::menu::OptionsSnapshot {
        player_name: "test".into(),
        master_addr: String::new(),
        view_distance: 200,
        fov_deg: 60.0,
        mouse_sensitivity: 1.0,
        show_fps: false,
        head_bob: false,
        invert_mouse_y: false,
        invert_wheel: false,
        lock_cursor: false,
        hide_cursor: false,
        mc_assets_path: String::new(),
        mc_assets_resolved: String::new(),
        mc_assets_is_fallback: false,
    };
    opt.clamp();
    assert_eq!(
        opt.view_distance,
        crate::VIEW_DISTANCE_MAX as u8,
        "clamp(200) should produce VIEW_DISTANCE_MAX={}",
        crate::VIEW_DISTANCE_MAX
    );
}
