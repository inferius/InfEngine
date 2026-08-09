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
//! - `"vx.player"` carries `PlayerMsg { id, pos, yaw, pitch, sel }`
//!   for pose broadcasts so each client can render others as cubes
//!   (W471: `sel` = sender's selected hotbar slot, used server-side
//!   for durability wear on validated breaks / hits).

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
    CHEST_ID, LAVA_ID,
    REDSTONE_TORCH_ID, REDSTONE_WALL_TORCH_ID, SOUL_TORCH_ID, SOUL_WALL_TORCH_ID,
    TILE_PX, TORCH_ID, TileSpec, atlas_uv_rect,
    block_is_liquid,
    block_is_opaque, block_is_placement_blocked, block_is_ray_passable,
    block_is_replaceable, block_is_transparent_render,
    block_material,
    block_type, bucket_liquid_id, is_tool_block,
    liquid_to_bucket_id, tool_label,
};
mod liquid;
use liquid::{
    LIQUIDS, Liquid, LiquidPlanStep, liquid_apply_plan, liquid_for,
    liquid_merge_step, liquid_plan_cell, schedule::LiquidSchedule,
};
mod net;
use net::{
    ClientInv, DamageMsg, EditMsg, MobHitMsg, MobMsg, NetRole,
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
// W319 - villager trades (generated 1:1 from the MC villager_trade
// data JSONs; the trade panel + RMB-on-villager open live in ui/ +
// app_on_update).
mod trades;
mod trades_table;
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
// W321 - distance LOD: far surface-shell tiles beyond view distance.
mod lod;
// W242 - MC loot tables (chest loot rollers; first user is the
// monster-room dungeon chest).
mod loot;
mod game_mode;
// W10.6: Creative-mode searchable inventory tab. The state +
// filter helpers live here; the egui rendering of the panel
// itself stays inline in `render_ui` for now.
mod creative_tab;
mod menu;
// W134 - minecarts: rail-riding cart entities (sim + interactions).
mod minecart;
mod effects;
mod hunger;
mod xp;
mod mining;
mod mob;
mod player;
// W140 - arrow projectiles (skeleton ranged attack).
mod explosion;
mod tnt;
mod projectile;
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
mod panorama;
#[allow(dead_code)]
mod jar_cache;
#[allow(dead_code)]
mod mob_skin;
// W19.1: per-body-part UV-mapped cube meshes for all 7 mob kinds.
mod mob_meshes;
// W91 - universal MC entity-model system (data-driven bones/cubes, one
// shared cube->mesh + MC->engine transform; replaces per-part magic
// numbers in render/mobs.rs). See memory: mc-faithful-porting.
mod entity_model;
// W47.3 - mob rendering extracted out of main.rs.
mod render;
// W47.4 - ambient particle spawn loop extracted out of main.rs.
mod ambient;
// W123 - underwater/cave/music ambience audio timers.
mod audio_ambience;
// W124 - MC language files + localized names + search haystack.
mod lang;
// W47.9 - fire random-tick + apply-step + lava ignite glue.
mod fire_runtime;
// W47.13 - place / break decision helpers extracted from on_update.
mod world;
// W47.20 - on_start bootstrap (plugins, renderer, atlas, audio).
mod bootstrap;
// W47.22 - per-frame on_update body (relocated from impl AppHandler).
mod app_on_update;
// W198 - shared perf snapshot for the F3 overlay ms breakdown.
mod perf;
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
pub(crate) use atlas::{day_night, flame_emitter, load_block_atlas_from_dir};
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
use mining::MiningState;
use mob::{
    KIND_CHICKEN, KIND_ZOMBIE,
    MOB_ATTACK_REACH, MOB_CAP_DEFAULT, MOB_DEATH_FADE, MOB_HIT_DAMAGE,
    MOB_LUNGE_TIME, MobState,
};
use player::{
    COLLISION_EPSILON, GRAVITY, JUMP_VELOCITY, PLAYER_HEIGHT, PLAYER_RADIUS,
    PLAYER_SPEED, body_aabb, ray_aabb, submerged_liquid, submerged_liquid_eye,
};
use ui::{AppState, BackTo, HighlightMode, InvAction, LoadingPhase, SlotIcon, match_recipe, match_recipe_named};
use worldgen::{Biome, WorldGen, biome_at};
use drops::DropItem;
use inventory::{HOTBAR_SLOTS, INVENTORY_SLOTS, inv_consume};
use tracing::{info, warn};

// W470: the old `remap_action` single-slot GridAction remapper is
// gone - `container::ui::GridInteract::base_index` does the same
// job inside the widget.

// `MiningState` moved to `mining.rs`; the per-block hardness +
// tool math lives in `block::mining_math` (W473).

// `MobState` struct + all `MOB_*`, `KIND_*`, `ZOMBIE_*` constants
// moved to the `mob` module (see `mob/mod.rs`); re-imported below.

// W473: `PVP_DAMAGE` (flat 1.5) is gone - melee damage comes from
// `block::mining_math::attack_damage` (MC ATTACK_DAMAGE totals).
const PVP_REACH: f32 = 5.5;

/// Number of stacked chunk layers along the Y axis. Each chunk is
/// `CHUNK_SIZE` (=16) tall.
///
/// W13.3: raised from 4 (64 blocks) to 16 (256 blocks) for MC parity.
/// W260: raised to 24 (384 blocks, chunks -4..=19) for MC 1.18+
/// parity - the world spans WORLD_MIN_Y..=WORLD_MAX_Y. Old saves
/// only carry chunks the old range wrote; the new bands regenerate
/// deterministically on first visit.
const VERTICAL_CHUNKS: i32 = 24;
/// The lowest chunk layer (`cy`) of the world (MC 1.18+: -4).
const MIN_CHUNK_Y: i32 = -4;
/// World-space block Y bounds (MC 1.18+: -64..=319).
const WORLD_MIN_Y: i32 = MIN_CHUNK_Y * 16;
const WORLD_MAX_Y: i32 = WORLD_MIN_Y + VERTICAL_CHUNKS * 16 - 1;
/// Chunks within this Chebyshev distance of the player chunk are
/// kept loaded; new ones get queued for generation.
///
/// W13.4: renamed from VIEW_DISTANCE_CHUNKS; cap raised from 12 to 24.
/// W229.2: default raised 4 -> 12 = MC's default render distance. The
/// old 4 (64 blocks) put the fog curtain practically in the player's
/// face and was a huge part of "does not look like MC"; the W198.x
/// perf passes made 12 comfortably real-time.
const VIEW_DISTANCE_CHUNKS: i32 = 12;
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
// W198.11: lowered 64 -> 16. At 64, the post-lighting re-mesh wave built
// 64 worker snapshots (chunk clones) on the MAIN thread in a single frame
// after every lighting batch - a ~20-30ms stream_ms spike = the "hitch
// when the next chunk loads" the user reported. 16/frame spreads the same
// wave over more frames (chunks light up as a gentle sweep, no stall);
// the 24-worker pool still meshes them just as fast.
// W313: 16 -> 64 with a 2 ms time guard in the drain - a lighting
// wave pushes hundreds of remeshes and 16/frame left a 2.3k queue
// (dark-vs-lit chunk seams for minutes in the user's nether log).
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
    /// W420: landing damage. Split from Generic so armor points do
    /// NOT absorb it (MC fall damage bypasses armor; only the
    /// feather-falling enchant will).
    Fall,
    /// W438: magic damage (poison / harming) - bypasses armor
    /// points like MC (only protection enchants will reduce it).
    Magic,
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
// W415: STAMINA_DRAIN / _REGEN / _REACTIVATE retired - the MC
// sprint gate is the hunger bar (food > 6, Player.canStartSprinting)
// and sprinting charges FoodData exhaustion instead of a stamina
// pool. `stamina` stays pinned at STAMINA_MAX until the HUD bar is
// removed with the W415 phase-2 hunger HUD.
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




fn main() -> ExitCode {
    bootstrap::main()
}
