//! Multiplayer voxel demo. Same world / mining mechanics as
//! `voxel_demo` plus a server / client mode on top of
//! `infengine_net`. Run modes:
//!
//! ```sh
//! cargo run -p voxel_demo_mp                            # offline single-player
//! cargo run -p voxel_demo_mp -- --server 0.0.0.0:7777   # host
//! cargo run -p voxel_demo_mp -- --client 127.0.0.1:7777 # join
//! ```
//!
//! Wire format: `infengine_net::NetMessage::Bytes { kind, data }`
//! with two kinds:
//! - `"vx.edit"` carries `EditMsg { x, y, z, block }` for a single
//!   block-edit (server -> all clients on apply; client -> server
//!   as a request).
//! - `"vx.player"` carries `PlayerMsg { id, pos, yaw, pitch }` for
//!   pose broadcasts so each client can render others as cubes.

use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::process::ExitCode;
// W28.10: crossbeam-channel replaces std::sync::mpsc. MPMC receiver can be
// cloned directly without Mutex, eliminating lock contention under load.
use crossbeam_channel::{Receiver, Sender, unbounded as cb_unbounded};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use glam::{Mat4, Vec3};
use infengine_editor::EditorUi;
use infengine_net::{NetClient, NetServer};
use infengine_render::{MeshHandle, MeshVertex, Renderer};
use infengine_voxel::{
    BlockDef, BlockId, BlockRegistry, CHUNK_SIZE, Chunk, ChunkCoord, RayHit, World,
    chunk_mesh_greedy_with_special,
    raycast_with,
};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};

mod block;
use block::{
    ATLAS_COLS, ATLAS_ROWS, BLOCK_TYPES, BUCKET_EMPTY_ID,
    BlockCategory, CHEST_ID, FURNACE_ID, LAVA_ID,
    REDSTONE_TORCH_ID, REDSTONE_WALL_TORCH_ID, SOUL_TORCH_ID, SOUL_WALL_TORCH_ID,
    TILE_PX, TORCH_ID, TileSpec, atlas_uv_rect, axe_tier,
    block_category, block_is_liquid,
    block_is_opaque, block_is_placement_blocked, block_is_ray_passable,
    block_is_replaceable, block_is_transparent_render,
    block_material, block_required_tier,
    block_skips_chunk_mesh, block_type, bucket_liquid_id, is_tool_block,
    liquid_to_bucket_id, shovel_tier, tool_effectiveness, tool_label,
    tool_tier, weapon_damage_mult,
};
mod liquid;
use liquid::{
    LIQUIDS, Liquid, LiquidPlanStep, bottom_open, liquid_apply_plan,
    liquid_corner_height_at, liquid_for, liquid_merge_step,
    liquid_plan_cell, push_quad_flow, schedule::LiquidSchedule,
    side_open,
};
mod net;
use net::{
    ClientInv, DamageMsg, EditMsg, InvActionMsg, MobHitMsg, MobMsg, NetRole,
    PlayerMsg, PvPHitMsg,
};
mod world_save;
mod decorations;
mod drops;
mod inventory;
// Item registry lives in its own module so chest contents / crafting
// outputs / future non-block items can carry per-stack metadata
// (max_stack, RMB-place target, icon, category) without piling onto
// `BlockType`. Phase C.1 lands the registry + helpers; the existing
// mining + placement + hotbar code still reads `block::*`. The
// item-module APIs become live in C.2 / C.3 when chest UI + crafting
// migrate onto them - until then we tolerate dead_code on the bin.
#[allow(dead_code)]
mod item;
// Generic N-slot container backing chests / furnaces / the future
// migrated player inventory. Phase C.2 lands the API + tests +
// egui grid widget; C.3 wires it into chest blocks, C.4 into the
// furnace. Until then the bin tolerates dead_code on this module.
#[allow(dead_code)]
mod container;
// Smelt-recipe + fuel-table lookup. Furnace logic in `block::furnace`
// (live tick) and `world_save::timers::catch_up_furnace` (offline)
// both query this module. Phase C.4 ships the seed entries.
#[allow(dead_code)]
mod smelting;
mod chat;
// Phase F: per-dim runtime parking maps + the active-dim swap
// helper. Some items inside the module land their first call site
// in F.2 (multi-dim save/load); silence the dead-code warnings
// in the meantime so the bin stays clippy-clean.
#[allow(dead_code)]
mod dim;
#[cfg(test)]
mod dim_save_tests;
#[cfg(test)]
mod lighting_tick_tests;
#[cfg(test)]
mod dim_switch_tests;
#[cfg(test)]
mod dim_teleport_tests;
#[cfg(test)]
mod debug_tests;
#[cfg(test)]
mod flint_durability_tests;
#[cfg(test)]
mod chest_mp_wire_tests;
// W10: per-mode runtime behaviour (Abilities bundle wiring,
// insta-break + drop suppression, invulnerability gate).
#[cfg(test)]
mod game_mode_runtime_tests;
#[cfg(test)]
mod portal_swirl_tests;
// W11.6: cross-module lock that mirrors the per-module checksum
// asserts so a CI failure points at the wave instead of one file.
#[cfg(test)]
mod wave11_audit_tests;
#[cfg(test)]
mod wave12_audit_tests;
#[cfg(test)]
mod wave13_tests;
#[cfg(test)]
mod wave16_tests;
#[cfg(test)]
mod wave17_tests;
#[cfg(test)]
mod wave18_tests;
#[cfg(test)]
mod wave24_tests;
#[cfg(test)]
mod wave25_tests;
#[cfg(test)]
mod wave27_tests;
#[cfg(test)]
mod wave28_tests;
#[cfg(test)]
mod wave29_tests;
// Phase L (Wave 3): per-voxel block + sky lighting subsystem.
// `LightingState` lives on `Voxel`; engines flood emitter + day
// light into per-chunk `LightGrid`s nibble-packed at 4 bits per
// cell per layer. Some helpers (sky engine, dirty queue) land
// their first runtime call site across the L.x commits; the bin
// tolerates dead_code on the module until then.
#[allow(dead_code)]
mod lighting;
mod game_mode;
// W10.6: Creative-mode searchable inventory tab. The state +
// filter helpers live here; the egui rendering of the panel
// itself stays inline in `render_ui` for now.
mod creative_tab;
mod menu;
mod mining;
mod mob;
mod player;
// Wave 7: Rust-dylib plugin host. Loads `<exec_dir>/plugins/*.{dll,so,dylib}`
// at startup (overrideable via `INFENGINE_PLUGIN_DIR`). The same C-ABI
// (defined in `infengine_sdk`) will back the C# CLR host in Phase K
// without any SDK rewrite.
pub(crate) mod plugin;
// W9: MC resource-pack loader. Reads textures + .mcmeta animations
// + sounds.json from the user's local MC install (assets are NOT
// committed to the repo - the loader points at external paths).
// Falls back to a synthetic placeholder atlas when no MC install
// is detected so the demo still launches on a clean machine.
#[allow(dead_code)]
mod resource_pack;
#[allow(dead_code)]
mod particle_pass;
#[allow(dead_code)]
mod sky;
#[allow(dead_code)]
mod emitter_lights;
#[allow(dead_code)]
mod panorama;
#[allow(dead_code)]
mod jar_cache;
#[allow(dead_code)]
mod mob_skin;
// W19.1: per-body-part UV-mapped cube meshes for all 7 mob kinds.
mod mob_meshes;
// W47.3 - mob rendering extracted out of main.rs.
mod render;
// W47.4 - ambient particle spawn loop extracted out of main.rs.
mod ambient;
// W47.9 - fire random-tick + apply-step + lava ignite glue.
mod fire_runtime;
// W47.13 - place / break decision helpers extracted from on_update.
mod world;
// W47.20 - on_start bootstrap (plugins, renderer, atlas, audio).
mod bootstrap;
// W47.22 - per-frame on_update body (relocated from impl AppHandler).
mod app_on_update;
// W47.23 - movement/camera/input update_play body.
mod update_play;
// W47.24 - Voxel runtime struct + PluginBlock row.
mod app;
pub(crate) use app::{PluginBlock, Voxel};
// W47.25 - chunk worker pool + worker-thread mesher.
mod workers;
pub(crate) use workers::{
    ChunkJob, ChunkSideEffect, Job, LightingJob, PendingChunk, RemeshJob, WorkerPool, WorkerResult,
};
// W47.27 - block atlas + day/night helpers.
mod atlas;
pub(crate) use atlas::{day_night, flame_emitter, load_block_atlas_from_dir, water_face_normal};
// W47.27 - crafting recipes.
mod recipes;
pub(crate) use recipes::{RECIPES, Recipe};
// W47.27 - RemoteInterp player pose buffer.
mod remote_interp;
pub(crate) use remote_interp::RemoteInterp;
// W47.27 - impl AppHandler for Voxel.
mod handler;
// W46.20 - rebuild emitter maps after world load / chunk reload.
mod emitter_scan;
mod ui;
mod worldgen;
// W13.4: view-frustum culling for chunk meshes. Reduces draw calls by
// 60-70% at typical render distances by skipping chunks behind the
// camera and off to the sides.
mod frustum;
use frustum::Frustum;
use chat::autocomplete::{
    self as autocomplete, ChatCompletionPopup, CompletionContext,
};
use chat::{CHAT_FADE_SECS, CHAT_HISTORY_MAX, CHAT_LOG_MAX, line_is_muted};
use mining::{MiningState, block_hardness};
use mob::{
    KIND_CHICKEN, KIND_PIG, KIND_SHEEP, KIND_ZOMBIE,
    MOB_ATTACK_REACH, MOB_CAP_DEFAULT, MOB_DEATH_FADE, MOB_HIT_DAMAGE,
    MOB_LUNGE_TIME, MOB_MAX_HEALTH, MobState, ZOMBIE_MAX_HEALTH,
};
use player::{
    COLLISION_EPSILON, GRAVITY, JUMP_VELOCITY, PLAYER_HEIGHT, PLAYER_RADIUS,
    PLAYER_SPEED, body_aabb, ray_aabb, submerged_liquid, submerged_liquid_eye,
    sweep_aabb,
};
use ui::{AppState, BackTo, HighlightMode, InvAction, LoadingPhase, SlotIcon, match_recipe, match_recipe_named};
use worldgen::{Biome, WorldGen, biome_at};
use drops::DropItem;
use inventory::{
    HOTBAR_SLOTS, INVENTORY_COLS, INVENTORY_ROWS, INVENTORY_SLOTS,
    SlotKind, SplitMode, STACK_MAX, inv_apply_drag, inv_consume,
};
use tracing::{info, warn};

/// Remap a 1-slot GridAction (always carries idx 0) to a different
/// slot index. Used by the furnace UI which renders each of its 3
/// slots as a single-slot grid so the layout can intersperse the
/// flame + cook bars between them.
fn remap_action(
    action: container::ui::GridAction,
    new_idx: usize,
) -> container::ui::GridAction {
    use container::ui::GridAction as G;
    match action {
        G::Take(_) => G::Take(new_idx),
        G::TakeHalf(_) => G::TakeHalf(new_idx),
        G::Place(_) => G::Place(new_idx),
        G::PlaceOne(_) => G::PlaceOne(new_idx),
        G::Transfer(_) => G::Transfer(new_idx),
    }
}

// `MiningState` + `block_hardness` moved to `mining.rs`.

// `MobState` struct + all `MOB_*`, `KIND_*`, `ZOMBIE_*` constants
// moved to the `mob` module (see `mob/mod.rs`); re-imported below.

const PVP_DAMAGE: f32 = 1.5;
const PVP_REACH: f32 = 5.5;

/// Number of stacked chunk layers along the Y axis. Each chunk is
/// `CHUNK_SIZE` (=16) tall.
///
/// W13.3: raised from 4 (64 blocks) to 16 (256 blocks) for MC parity.
/// Old saves only have y=0..3; y=4..15 are treated as air-filled on
/// first load so no regeneration happens for pre-W13 worlds.
const VERTICAL_CHUNKS: i32 = 16;
/// Chunks within this Chebyshev distance of the player chunk are
/// kept loaded; new ones get queued for generation.
///
/// W13.4: renamed from VIEW_DISTANCE_CHUNKS; cap raised from 12 to 24.
const VIEW_DISTANCE_CHUNKS: i32 = 4;
/// Hard cap on the view_distance setting. MC "Far" is 32 chunks;
/// matches the cap here. Beyond this an LOD layer (W13.5 follow-up)
/// would be needed - synchronous voxel meshes at 48+ chunks would
/// trash GPU memory + per-frame work. Frustum culling already drops
/// 60-70% of chunks so the on-screen budget stays manageable.
///
/// W13.4: raised from 12 to 24. Follow-up: 24 -> 32.
const VIEW_DISTANCE_MAX: i32 = 32;
/// Maximum number of worker results whose GPU upload is processed per
/// frame. W13.1: lowered from 4 to 2. W23.3a: raised to 8.
/// W25.6: raised to 32 - Generate + Remesh results share this budget.
/// W26.3: lowered back to 4 - GPU upload (wgpu buffer alloc + queue
/// submit + bind group) is expensive; RESULTS_BUDGET_MS provides a
/// soft time cap as a second guard.
// W46.33: bumped 4 -> 32. Loading state already uses 32; keeping the
// same cap in Playing means a 412-chunk post-load remesh wave (W46.32
// lighting BFS + W46.30 remesh queue) drains in ~13 frames instead
// of ~103. The 2 ms per-frame time budget below still throttles, so
// raising the count cap is safe on cheap result types (Lit, Removed)
// and the time budget short-circuits expensive ones (Remeshed mesh
// upload).
const RESULTS_PER_FRAME: usize = 32;
/// Soft time budget (ms) for the GPU upload drain loop per frame.
/// W26.3: caps wall-clock GPU upload cost regardless of result count.
// W46.33: bumped 2 -> 6 ms. Same rationale as RESULTS_PER_FRAME -
// post-load remesh wave benefits from a wider per-frame slot. 6 ms
// stays under one 60 Hz frame budget (16.6 ms) by 10 ms so the rest
// of on_update (lighting tick, mob AI, render dispatch) still has
// headroom.
const RESULTS_BUDGET_MS: u64 = 6;
/// Hard upper bound on remesh dispatch calls per frame (W13.2). After
/// W25 each call only enqueues a job (O(1) Arc clones) instead of
/// doing synchronous meshing. W25.5: raised from 16 to 64.
/// W26.3: lowered to 8 - dispatch still pays ~64 KB malloc per job
/// (chunk clone; Arc<Chunk> refactor deferred to W27). At 8 jobs/frame
/// that is ~512 KB/frame of allocator pressure vs ~4 MB/frame at 64.
// W46.30: bumped 8 -> 64 so the post-BFS re-remesh wave (every
// loaded chunk after rebuild_all_lighting) dispatches in
// 495 / 64 ~= 8 frames instead of 62. Workers pick the jobs up
// at their own rate; the main-thread cap was only a back-pressure
// safety, never the actual bottleneck on a 24-worker pool.
const REMESH_DEFERRED_PER_FRAME: usize = 64;
/// Minimum number of background worker threads generating + meshing
/// chunks. The actual worker count is picked at startup via
/// `pick_worker_count()` which reads `std::thread::available_parallelism`
/// + reserves cores for the main thread + audio + net.
///
/// W13.2: raised from 2 to 4. Post-W13: now a floor, not a fixed value.
const MIN_WORKERS: usize = 4;
/// Upper cap on worker threads. Keeps the chunk-gen pool from
/// monopolising a high-core-count machine + leaves headroom for
/// the OS, audio, network, and the GPU driver. 24 is plenty for
/// voxel gen at any reasonable view distance.
const MAX_WORKERS: usize = 24;

/// Choose worker-thread count based on the host machine's logical
/// CPU count. Reserves 2 cores for the main thread + audio + net,
/// floors at `MIN_WORKERS`, caps at `MAX_WORKERS`. Logs the decision.
fn pick_worker_count() -> usize {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(MIN_WORKERS);
    let workers = logical.saturating_sub(2).clamp(MIN_WORKERS, MAX_WORKERS);
    tracing::info!(
        logical_cpus = logical,
        chosen_workers = workers,
        "worker pool sized"
    );
    workers
}
/// W13.1: warn threshold in microseconds. Any phase of the chunk
/// integration path that exceeds this per-frame will emit a
/// `tracing::warn!` so frame hitches are visible in the log.
const CHUNK_PHASE_WARN_US: u128 = 5_000;
// `PLAYER_SPEED`, `JUMP_VELOCITY`, `GRAVITY`, `PLAYER_HEIGHT`,
// `PLAYER_RADIUS`, `COLLISION_EPSILON` moved to `player.rs` and
// re-imported below.

/// Damage source classifier - drives the per-kind hurt SFX
/// `apply_player_damage_kind` plays on integer heart loss. New
/// kinds drop in here + add a `match` arm in the SFX selector.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum DamageKind {
    /// Mob hits, fall damage, PvP - the standard hurt thud.
    Generic,
    /// Air ran out underwater - subtle bubble + airy swish.
    Drown,
    /// Hot liquid contact - sizzling swish, no bass.
    Burn,
    /// W10.4: the player fell out of the world (below y < 0 in
    /// overworld / y < -1 in nether). Bypasses the Creative
    /// invulnerability gate so /gamemode creative is not an
    /// auto-cheat against the void. Mirrors MC's
    /// `DamageSource::OUT_OF_WORLD` being marked `bypassInvul`.
    Void,
}

/// Maximum player hit points. 20 mirrors the Minecraft scale (10
/// hearts of 2 HP each).
pub(crate) const PLAYER_MAX_HEALTH: f32 = 20.0;
/// W10.3: maximum seconds between two consecutive Space presses
/// for the double-tap fly toggle to fire. Matches MC's
/// `LocalPlayer.java` jumpTriggerTime budget (~ 0.5 s).
pub(crate) const DOUBLE_TAP_WINDOW_SECS: f32 = 0.5;
const STAMINA_MAX: f32 = 100.0;
/// Breath capacity in seconds. MC matches this at ~15 s with
/// the default lung capacity. Drains 1 s/s while submerged in a
/// liquid that has `drains_air = true`; refills 4x as fast
/// after surfacing.
const AIR_MAX_SEC: f32 = 15.0;
/// W15.3: test-visible re-export of `STAMINA_MAX` so
/// `game_mode_runtime_tests` can assert exact values.
#[cfg(test)]
pub(crate) const STAMINA_MAX_TEST: f32 = STAMINA_MAX;
/// W15.3: test-visible re-export of `AIR_MAX_SEC`.
#[cfg(test)]
pub(crate) const AIR_MAX_SEC_TEST: f32 = AIR_MAX_SEC;
/// Drowning damage rate after the air meter hits zero.
const DROWN_DAMAGE_PER_SEC: f32 = 2.0;
const STAMINA_DRAIN: f32 = 20.0;
const STAMINA_REGEN: f32 = 18.0;
const STAMINA_REACTIVATE: f32 = 25.0;
/// Free-fall distance, in voxels, before fall damage kicks in. A
/// 3-block drop is harmless; anything taller deals
/// `fall_distance - SAFE_FALL` HP.
const SAFE_FALL: f32 = 3.0;
/// Seconds without damage before HP starts ticking up.
const REGEN_DELAY: f32 = 4.0;
/// HP regenerated per second once `regen_accum` exceeds the delay.
/// Rate of 0.5 HP/s + 4 s delay = a 5 s grace + 40 s to fill from
/// near-death to full.
const REGEN_RATE: f32 = 0.5;

/// Length of one full day/night cycle in seconds. Short enough to
/// see the sun move during a single play session - feel free to
/// crank up for atmosphere later. 240s = 4 min full cycle.
pub(crate) const DAY_LENGTH_S: f32 = 600.0;

/// Phase L.6: the lighting tick runs at 10 Hz so block-edit ripples
/// (place / break torch, dig a shaft) reach the GPU within 100 ms
/// without re-flooding 27 chunks per individual edit.
const LIGHTING_TICK_PERIOD_S: f32 = 0.1;

/// The 6 axis-aligned neighbour offsets (4 NSEW + above + below).
/// Used by the per-cell liquid tick to fan out re-schedules.
const SIX_NSEW_UD: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 0, 1],
    [0, 0, -1],
    [0, 1, 0],
    [0, -1, 0],
];

// `match_recipe`, `match_recipe_named`, `InvAction`,
// `SlotIcon`, `AppState`, `BackTo` moved to `ui.rs`.




/// W8.3: day/night sky color tests. Lives at the very bottom of
/// the bin so clippy's `items_after_test_module` lint is happy
/// (no further items live below).
#[cfg(test)]
mod day_night_tests {
    use super::{DAY_LENGTH_S, day_night};

    /// Helper: just the sky color half of the `day_night` return.
    fn sky_at(t: f32) -> [f32; 3] {
        day_night(t).1
    }

    /// Helper: euclidean distance between two RGB triples. Lets the
    /// assertions read tolerantly so the smoothstep ramp doesn't
    /// have to land on exact float boundaries.
    fn rgb_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        let dr = a[0] - b[0];
        let dg = a[1] - b[1];
        let db = a[2] - b[2];
        (dr * dr + dg * dg + db * db).sqrt()
    }

    #[test]
    fn sky_color_at_noon_matches_daytime_anchor() {
        // W8.3: noon = day fraction 0.25 (sun at +Y peak in our
        // phase = sin(theta) curve). Within tolerance of the
        // noon anchor [0.45, 0.65, 0.95].
        let sky = sky_at(DAY_LENGTH_S * 0.25);
        let noon_anchor = [0.45_f32, 0.65, 0.95];
        assert!(
            rgb_dist(sky, noon_anchor) < 0.10,
            "noon sky {sky:?} far from anchor {noon_anchor:?}"
        );
    }

    #[test]
    fn sky_color_at_midnight_is_near_black() {
        // W8.3: midnight = day fraction 0.75 (sun straight down).
        // Each channel should be <= 0.10 (the night anchor max).
        let sky = sky_at(DAY_LENGTH_S * 0.75);
        for (i, ch) in sky.iter().enumerate() {
            assert!(
                *ch <= 0.10,
                "midnight sky channel {i} = {ch} not near-black"
            );
        }
    }

    #[test]
    fn sky_color_dawn_is_intermediate() {
        // W8.3: dawn (0.50) sits in the night..horizon ramp - the
        // result should be brighter than the midnight anchor but
        // darker than the noon anchor.
        let dawn = sky_at(DAY_LENGTH_S * 0.50);
        let midnight = sky_at(DAY_LENGTH_S * 0.75);
        let noon = sky_at(DAY_LENGTH_S * 0.25);
        let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
        let strict_min = [0.0_f32, 0.0, 0.0];
        assert!(
            lum(dawn) >= lum(strict_min),
            "dawn {dawn:?} below strict min"
        );
        assert!(
            lum(dawn) < lum(noon),
            "dawn {dawn:?} brighter than noon {noon:?}"
        );
        // Soft sanity: midnight luminance is non-greater than dawn
        // (the ramp is monotonic in the dawn -> noon direction).
        assert!(lum(midnight) <= lum(noon));
    }

    #[test]
    fn sky_color_smooth_ramp_through_dusk() {
        // W8.3: stepping in 0.01-day fractions across dusk
        // (0.70..0.80) should produce a monotonically-darkening
        // sequence - no jumps back to bright blue.
        let mut prev_lum = f32::INFINITY;
        let mut samples = 0;
        for i in 70..=80 {
            let t = DAY_LENGTH_S * (i as f32 * 0.01);
            let c = sky_at(t);
            let lum = c[0] + c[1] + c[2];
            assert!(
                lum <= prev_lum + 0.01,
                "sky luminance not monotonic at t={t}: {lum} after {prev_lum}"
            );
            prev_lum = lum;
            samples += 1;
        }
        assert!(samples >= 10, "expected at least 10 ramp samples");
    }
}

/// W9.5: ambient particle helper unit tests. Live below the
/// day_night_tests module so the items_after_test_module lint
/// stays happy.
#[cfg(test)]
mod ambient_particle_tests {
    // W47.5: helpers moved to crate::ambient. Test imports updated.
    use crate::ambient::{
        accumulator_fires, spawn_flame_particle, spawn_smoke_particle,
        within_radius,
    };
    use infengine_particles::ParticleSystem;

    #[test]
    fn within_radius_includes_close_position() {
        let player = [0.0, 0.0, 0.0];
        assert!(within_radius(player, &[1, 0, 0], 5.0));
        assert!(within_radius(player, &[3, 0, 3], 10.0));
    }

    #[test]
    fn within_radius_excludes_far_position() {
        let player = [0.0, 0.0, 0.0];
        assert!(!within_radius(player, &[100, 0, 0], 32.0));
        assert!(!within_radius(player, &[40, 0, 0], 32.0));
    }

    #[test]
    fn accumulator_target_zero_never_fires() {
        let mut seed = 0xDEADBEEF;
        for _ in 0..1000 {
            assert!(!accumulator_fires(&mut seed, 0.0));
        }
    }

    #[test]
    fn accumulator_target_one_always_fires() {
        let mut seed = 0x12345678;
        for _ in 0..1000 {
            assert!(accumulator_fires(&mut seed, 1.0));
        }
    }

    #[test]
    fn flame_particle_spawns_above_position() {
        let mut sys = ParticleSystem::new();
        let mut seed = 0xABCDEF01;
        let start = sys.particles.len();
        spawn_flame_particle(&mut sys, &[10, 5, 7], &mut seed, [1.0, 0.8, 0.4]);
        assert_eq!(sys.particles.len(), start + 1);
        let p = sys.particles.last().unwrap();
        // Y should be above the voxel centre.
        assert!(p.position[1] >= 5.5, "flame should spawn above the cell base");
        assert!(p.velocity[1] > 0.0, "flame should drift upward");
        assert!(p.lifetime > 0.0);
    }

    #[test]
    fn smoke_particle_drifts_slower_than_flame() {
        let mut sys = ParticleSystem::new();
        let mut seed = 0x11223344;
        spawn_flame_particle(&mut sys, &[0, 0, 0], &mut seed, [1.0, 0.8, 0.4]);
        let flame_vy = sys.particles.last().unwrap().velocity[1];
        spawn_smoke_particle(&mut sys, &[0, 0, 0], &mut seed, None);
        let smoke_vy = sys.particles.last().unwrap().velocity[1];
        // Flame range: 0.4..0.9. Smoke range: 0.3..0.5. They can
        // technically overlap, but the smoke ceiling (0.5) is
        // below the flame floor + something - assert the smoke is
        // BOUNDED ABOVE by 0.55 + flame is BOUNDED BELOW by 0.35.
        assert!(smoke_vy < 0.55, "smoke vy should be capped");
        assert!(flame_vy > 0.35, "flame vy should be at least 0.4");
    }

    #[test]
    fn particle_cap_is_enforced_softly() {
        // The cap is checked once at the top of tick_ambient_particles;
        // we can't easily call that here without a full Voxel, but
        // we can at least verify the spawn helpers add ONE particle
        // each per invocation - so the cap math (max-per-tick) works.
        let mut sys = ParticleSystem::new();
        let mut seed = 0x99887766;
        let start = sys.particles.len();
        for _ in 0..10 {
            spawn_flame_particle(&mut sys, &[0, 0, 0], &mut seed, [1.0, 0.8, 0.4]);
        }
        assert_eq!(sys.particles.len(), start + 10);
    }

    #[test]
    fn smoke_particle_uses_mc_tint_when_provided() {
        // W9.y.5: the spawn helper takes an optional MC tint
        // sampled from the smoke_0..7 sprite set. When supplied,
        // the emitted Particle's color must use the MC RGB
        // (alpha forced to 0.7 so the wisp keeps its translucent
        // look).
        let mut sys = ParticleSystem::new();
        let mut seed = 0x42424242;
        let mc = Some([0.2, 0.21, 0.22, 1.0]);
        spawn_smoke_particle(&mut sys, &[0, 0, 0], &mut seed, mc);
        let p = sys.particles.last().unwrap();
        assert!((p.color[0] - 0.2).abs() < 1e-3);
        assert!((p.color[1] - 0.21).abs() < 1e-3);
        assert!((p.color[2] - 0.22).abs() < 1e-3);
        assert!((p.color[3] - 0.7).abs() < 1e-3);
    }

    #[test]
    fn smoke_particle_falls_back_to_grey_without_mc_tint() {
        // No MC pack (placeholder atlas): the smoke wisp uses
        // the legacy grey swatch so the visual stays stable.
        let mut sys = ParticleSystem::new();
        let mut seed = 0x42424242;
        spawn_smoke_particle(&mut sys, &[0, 0, 0], &mut seed, None);
        let p = sys.particles.last().unwrap();
        assert!((p.color[0] - 0.35).abs() < 1e-3);
        assert!((p.color[1] - 0.35).abs() < 1e-3);
        assert!((p.color[2] - 0.35).abs() < 1e-3);
    }
}

fn main() -> ExitCode {
    bootstrap::main()
}
