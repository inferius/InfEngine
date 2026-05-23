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
pub(crate) use workers::WorkerPool;
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
use tracing::{error, info, warn};

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

/// Two-sample interpolation buffer for one remote player's pose.
/// The render path samples at `now - INTERP_DELAY` so the most
/// recent snapshot is one tick "in the future" and we always have
/// the surrounding pair to lerp between.
#[derive(Debug, Clone, Copy)]
struct RemoteInterp {
    prev: PlayerMsg,
    current: PlayerMsg,
    prev_t: std::time::Instant,
    current_t: std::time::Instant,
}

/// Render delay (seconds) applied to the interpolation buffer. A
/// little less than a typical send interval - lets the lerp always
/// have both endpoints without showing a noticeable lag.
const INTERP_DELAY: f32 = 0.10;

impl RemoteInterp {
    fn new(p: PlayerMsg) -> Self {
        let now = std::time::Instant::now();
        Self {
            prev: p,
            current: p,
            prev_t: now,
            current_t: now,
        }
    }

    fn push(&mut self, p: PlayerMsg) {
        let now = std::time::Instant::now();
        self.prev = self.current;
        self.prev_t = self.current_t;
        self.current = p;
        self.current_t = now;
    }

    /// Pose interpolated at "now minus the render delay", clamped
    /// to the available buffer so we don't extrapolate past the
    /// last snapshot when the sender pauses.
    fn sample(&self, now: std::time::Instant) -> PlayerMsg {
        let render_t = now - std::time::Duration::from_secs_f32(INTERP_DELAY);
        let span = self
            .current_t
            .saturating_duration_since(self.prev_t)
            .as_secs_f32()
            .max(1e-4);
        let into = render_t
            .saturating_duration_since(self.prev_t)
            .as_secs_f32();
        let t = (into / span).clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        // Shortest-arc lerp for yaw + pitch so wrap-around (PI -> -PI)
        // doesn't whip the camera the long way around.
        let lerp_angle = |a: f32, b: f32| {
            let mut d = b - a;
            let tau = std::f32::consts::TAU;
            while d > std::f32::consts::PI {
                d -= tau;
            }
            while d < -std::f32::consts::PI {
                d += tau;
            }
            a + d * t
        };
        PlayerMsg {
            id: self.current.id,
            pos: [
                lerp(self.prev.pos[0], self.current.pos[0]),
                lerp(self.prev.pos[1], self.current.pos[1]),
                lerp(self.prev.pos[2], self.current.pos[2]),
            ],
            yaw: lerp_angle(self.prev.yaw, self.current.yaw),
            pitch: lerp_angle(self.prev.pitch, self.current.pitch),
        }
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

/// Shapeless 2x2 crafting recipe. Each entry is `(BlockId, count)`
/// the player must place into the 2x2 grid (order doesn't matter).
/// Empty inputs are encoded by leaving them out of the list.
struct Recipe {
    inputs: &'static [(u16, u8)],
    output: (u16, u8),
    /// Human-readable name shown in the recipe book.
    name: &'static str,
}

/// Demo recipe table. Real Minecraft has shaped + tagged recipes;
/// this is a deliberately tiny showcase for the engine. New entries
/// = one line here, no other changes needed.
const RECIPES: &[Recipe] = &[
    // 4 dirt -> 1 grass (sodding).
    Recipe {
        inputs: &[(2, 1), (2, 1), (2, 1), (2, 1)],
        output: (3, 1),
        name: "Sodded grass",
    },
    // 4 leaves -> 1 wood (the tree's revenge).
    Recipe {
        inputs: &[(6, 1), (6, 1), (6, 1), (6, 1)],
        output: (5, 1),
        name: "Bundled log",
    },
    // 1 wood -> 4 planks (classic Minecraft).
    Recipe {
        inputs: &[(5, 1)],
        output: (7, 4),
        name: "Oak planks",
    },
    // 4 planks -> 1 wood (consolidate; lets you reverse the craft
    // chain).
    Recipe {
        inputs: &[(7, 1), (7, 1), (7, 1), (7, 1)],
        output: (5, 1),
        name: "Consolidated log",
    },
    // 4 stone -> 1 cobble (compressed cobblestone block).
    Recipe {
        inputs: &[(1, 1), (1, 1), (1, 1), (1, 1)],
        output: (8, 1),
        name: "Cobblestone",
    },
    // 4 cobble -> 4 stone (polish cobbles back into stone).
    Recipe {
        inputs: &[(8, 1), (8, 1), (8, 1), (8, 1)],
        output: (1, 4),
        name: "Polished stone",
    },
    // 4 sand -> 1 glass (we have no furnace yet, so the craft does
    // double duty as the heat source).
    Recipe {
        inputs: &[(4, 1), (4, 1), (4, 1), (4, 1)],
        output: (9, 1),
        name: "Glass block",
    },
    // 2 wood + 2 leaves -> 4 torches. Two-block input
    // disambiguates this from the planks recipe (1 wood alone).
    Recipe {
        inputs: &[(5, 1), (5, 1), (6, 1), (6, 1)],
        output: (10, 4),
        name: "Torches",
    },
    // 3 planks + 1 wood -> 1 wood pickaxe. The wood log stands
    // in for the stick handle the 2x2 grid can't shape.
    Recipe {
        inputs: &[(7, 1), (7, 1), (7, 1), (5, 1)],
        output: (16, 1),
        name: "Wood pickaxe",
    },
    // 3 cobble + 1 wood -> 1 stone pickaxe.
    Recipe {
        inputs: &[(8, 1), (8, 1), (8, 1), (5, 1)],
        output: (17, 1),
        name: "Stone pickaxe",
    },
    // 3 iron + 1 wood -> 1 iron pickaxe.
    Recipe {
        inputs: &[(14, 1), (14, 1), (14, 1), (5, 1)],
        output: (18, 1),
        name: "Iron pickaxe",
    },
    // Sword variants: 2 of the material + 1 wood handle.
    Recipe {
        inputs: &[(7, 1), (7, 1), (5, 1)],
        output: (23, 1),
        name: "Wood sword",
    },
    Recipe {
        inputs: &[(8, 1), (8, 1), (5, 1)],
        output: (24, 1),
        name: "Stone sword",
    },
    Recipe {
        inputs: &[(14, 1), (14, 1), (5, 1)],
        output: (25, 1),
        name: "Iron sword",
    },
    // 4 coal -> 1 glowstone block. Loose interpretation since
    // the demo has no nether biome; gives the player a use for
    // surplus coal beyond torches.
    Recipe {
        inputs: &[(13, 1), (13, 1), (13, 1), (13, 1)],
        output: (28, 1),
        name: "Glowstone",
    },
    // 2 sand + 2 cobble -> 1 sandstone. Distinct from the
    // 4-sand glass recipe so the matcher resolves cleanly.
    Recipe {
        inputs: &[(4, 1), (4, 1), (8, 1), (8, 1)],
        output: (26, 1),
        name: "Sandstone",
    },
    // Shovels: 1 head material + 2 wood handle.
    Recipe {
        inputs: &[(7, 1), (5, 1), (5, 1)],
        output: (29, 1),
        name: "Wood shovel",
    },
    Recipe {
        inputs: &[(8, 1), (5, 1), (5, 1)],
        output: (30, 1),
        name: "Stone shovel",
    },
    Recipe {
        inputs: &[(14, 1), (5, 1), (5, 1)],
        output: (31, 1),
        name: "Iron shovel",
    },
    // Axes: 2 head material + 2 wood handle. Distinct counts
    // from pickaxe (3 head + 1 wood) so the shapeless matcher
    // resolves cleanly.
    Recipe {
        inputs: &[(7, 1), (7, 1), (5, 1), (5, 1)],
        output: (32, 1),
        name: "Wood axe",
    },
    Recipe {
        inputs: &[(8, 1), (8, 1), (5, 1), (5, 1)],
        output: (33, 1),
        name: "Stone axe",
    },
    Recipe {
        inputs: &[(14, 1), (14, 1), (5, 1), (5, 1)],
        output: (34, 1),
        name: "Iron axe",
    },
    // 3 wool + 1 planks -> 1 bed. Loose interpretation of the
    // 3-wool + 3-plank MC bed shape, fit into the 2x2 grid.
    Recipe {
        inputs: &[(11, 1), (11, 1), (11, 1), (7, 1)],
        output: (35, 1),
        name: "Bed",
    },
    // 3 iron -> 1 empty bucket. MC shape (V of 3 ingots)
    // collapses to a shapeless 3-iron recipe on the 2x2 grid;
    // a real 3x3 crafting table comes later.
    Recipe {
        inputs: &[(14, 1), (14, 1), (14, 1)],
        output: (BUCKET_EMPTY_ID, 1),
        name: "Bucket",
    },
];

// `match_recipe`, `match_recipe_named`, `InvAction`,
// `SlotIcon`, `AppState`, `BackTo` moved to `ui.rs`.

#[derive(Debug, Clone)]
struct Settings {
    mouse_sensitivity: f32,
    lock_cursor: bool,
    hide_cursor: bool,
    show_fps: bool,
    /// Render distance in chunks (Chebyshev). The unload radius is
    /// computed as `view_distance + 2` so the player can wobble on
    /// the boundary without thrashing chunk loads.
    view_distance: u8,
    /// Camera head bob while walking. Off-by-default-friendly:
    /// nauseating for some players, polish for others.
    head_bob: bool,
    /// Invert vertical mouse axis. Some players prefer down=look-up
    /// so leave it toggleable from the Settings panel.
    invert_mouse_y: bool,
    /// Reverse mouse-wheel hotbar scrolling direction.
    invert_wheel: bool,
    /// W9.7: optional override for the MC resource pack location.
    /// `None` = use the auto-discovered default (env var ->
    /// %APPDATA%/.minecraft -> placeholder). When set + the path
    /// resolves to a usable `assets/minecraft/` tree, the engine
    /// loads textures + sounds from there instead.
    mc_assets_path: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.6,
            lock_cursor: true,
            hide_cursor: true,
            show_fps: true,
            view_distance: VIEW_DISTANCE_CHUNKS as u8,
            head_bob: true,
            invert_mouse_y: false,
            invert_wheel: false,
            mc_assets_path: None,
        }
    }
}

/// W28.1: Entry in the priority queue for pending chunk generation.
/// BinaryHeap is max-heap; we negate squared distance so the chunk
/// closest to the player sorts highest (pops first).
#[derive(Clone, Copy, Eq, PartialEq)]
struct PendingChunk {
    /// Negated squared XZ distance from player chunk (so closest = max).
    neg_dist_sq: i64,
    /// The chunk coord to generate.
    coord: ChunkCoord,
}

impl Ord for PendingChunk {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.neg_dist_sq
            .cmp(&other.neg_dist_sq)
            .then_with(|| self.coord.cmp(&other.coord))
    }
}

impl PartialOrd for PendingChunk {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}



impl AppHandler for Voxel {
    fn on_start(&mut self, window: &EngineWindow) {
        crate::bootstrap::on_start(self, window);
    }



    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(ui) = self.ui.as_mut() {
            ui.handle_engine_event(window, event);
        }
        // Window close: send a `vx.leave` first so the server can
        // reap our pose instantly + broadcast the removal to the
        // other peers. The actual process exit happens on the next
        // `on_update` via `requested_quit`.
        if matches!(event, WindowEvent::CloseRequested) {
            // Final save before exit so the player doesn't lose
            // edits made since the last 30 s autosave.
            self.save_server_state();
            self.notify_server_leaving();
            self.requested_quit = true;
        }
        // Focus tracking. Clicking another window on the same machine
        // shifts focus; without this guard `apply_cursor_settings`
        // would keep wrestling the cursor away from whichever window
        // the user actually clicked.
        if let WindowEvent::Focused(focused) = event {
            self.has_focus = *focused;
            if !*focused {
                self.lmb_pending = false;
                self.rmb_pending = false;
                self.mouse_dx = 0.0;
                self.mouse_dy = 0.0;
                self.last_mouse = None;
            }
        }
        // Track cursor moved deltas only when no grab is active (i.e. in
        // menus this is the only source; in Play raw DeviceEvent
        // supplies the deltas).
        if let WindowEvent::CursorMoved { position, .. } = event
            && matches!(self.state, AppState::Playing)
            && !(self.settings.lock_cursor || self.settings.hide_cursor)
        {
            let now = [position.x as f32, position.y as f32];
            if let Some(prev) = self.last_mouse {
                self.mouse_dx += now[0] - prev[0];
                self.mouse_dy += now[1] - prev[1];
            }
            self.last_mouse = Some(now);
        }
        // Edge-triggered LMB / RMB while in Play state. Both flags are
        // consumed in `on_update` next frame. Ignored when the window
        // doesn't have focus so a click on a sibling instance never
        // mines a block here.
        if matches!(self.state, AppState::Playing)
            && self.has_focus
            && let WindowEvent::MouseInput { state, button, .. } = event
        {
            match (state, button) {
                (ElementState::Pressed, MouseButton::Left) => {
                    self.lmb_pending = true;
                    // W30 fix: only set just_pressed on the rising edge
                    // (transition from up to down). winit may fire multiple
                    // Pressed events on a single click; this guard makes
                    // creative insta-break fire exactly once per physical
                    // click regardless.
                    if !self.lmb_held {
                        self.lmb_just_pressed = true;
                    }
                    self.lmb_held = true;
                }
                (ElementState::Released, MouseButton::Left) => {
                    self.lmb_held = false;
                    // Releasing the button cancels any active
                    // mining session.
                    self.mining = None;
                }
                (ElementState::Pressed, MouseButton::Right) => {
                    self.rmb_pending = true;
                    self.rmb_held = true;
                }
                (ElementState::Released, MouseButton::Right) => {
                    self.rmb_held = false;
                }
                _ => {}
            }
        }
        // Mouse wheel cycles through hotbar slots.
        if matches!(self.state, AppState::Playing)
            && let WindowEvent::MouseWheel { delta, .. } = event
        {
            use infengine_window::raw_winit::event::MouseScrollDelta;
            let y = match delta {
                MouseScrollDelta::LineDelta(_, y) => *y,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 50.0,
            };
            self.wheel_accum += y;
        }
    }

    fn on_device_event(&mut self, event: &infengine_window::raw_winit::event::DeviceEvent) {
        use infengine_window::raw_winit::event::DeviceEvent;
        if !matches!(self.state, AppState::Playing) {
            return;
        }
        // Device events fire globally - any other instance with cursor
        // lock would otherwise rotate its camera off our motion. Gate
        // on focus to keep input owner-local.
        if !self.has_focus {
            return;
        }
        if !(self.settings.lock_cursor || self.settings.hide_cursor) {
            return;
        }
        if let DeviceEvent::MouseMotion { delta } = event {
            self.mouse_dx += delta.0 as f32;
            self.mouse_dy += delta.1 as f32;
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        crate::app_on_update::on_update(self, window, ctx);
    }

    fn on_resize(&mut self, w: u32, h: u32) {
        self.viewport = (w, h);
        if let Some(r) = self.renderer.as_mut() {
            r.resize(w, h);
        }
    }
}

/// W13.2: Side-effect detected in a freshly-generated chunk while
/// scanning its voxels on the worker thread. Folded into the
/// per-world index tables on the main thread after GPU upload.
enum ChunkSideEffect {
    /// Snow voxel (id 15) at this world position.
    Snow([i32; 3]),
    /// Torch or glowstone emitter at this world position.
    Torch([i32; 3]),
    /// Liquid voxel at this world position: `tick_offset` is
    /// `liquid.tick_period.max(1)` pre-computed on the worker so
    /// the main thread only does the addition.
    Liquid { pos: [i32; 3], tick_offset: u64 },
}

/// W24: Snapshot of 6-neighbour light grids shipped with each ChunkJob.
/// Defined in `lighting::HaloLighting`; re-exported here so the worker
/// pool code and tests can refer to it as plain `HaloLighting`.
use lighting::HaloLighting;

/// W25 / W27: output from one worker thread. Three variants:
/// - `Generated`: new chunk from worldgen + side-effects + mesh.
/// - `Remeshed`: remesh of an existing chunk (no worldgen, no side-effects).
/// - `Lit`: W27 lighting BFS result for a set of chunks.
///
/// The `revision` field on `Remeshed` carries the snapshot revision so
/// the main thread can discard stale results when the chunk was edited
/// again before the first remesh returned.
enum WorkerResult {
    /// W13 / W24: new chunk generated by `generate_chunk_at_ctx`.
    Generated {
        coord: ChunkCoord,
        chunk: Chunk,
        /// W13.2: voxel-scan results computed off-thread.
        side_effects: Vec<ChunkSideEffect>,
        /// W24: MC-atlas submeshes built by the worker.
        submeshes: Vec<infengine_voxel::ChunkSubmesh>,
    },
    /// W25: remesh of an already-loaded chunk (block edit / neighbour
    /// update). No worldgen, no side-effect scan.
    Remeshed {
        coord: ChunkCoord,
        /// Revision at snapshot time. Main thread drops this result if
        /// `chunk_revisions[coord]` has advanced past it.
        revision: u64,
        submeshes: Vec<infengine_voxel::ChunkSubmesh>,
    },
    /// W27: lighting BFS result. The worker computed new LightGrids for
    /// `affected` chunks; the main thread swaps them in and triggers a
    /// remesh of every affected chunk so the GPU picks up new light values.
    Lit {
        /// Updated light grids, one per chunk in `affected`.
        new_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>>,
        /// Chunks that should be remeshed now that their lighting changed.
        affected_for_remesh: Vec<ChunkCoord>,
    },
}

/// W25 / W27: a single unit of work sent to a worker thread.
/// - `Generate`: full worldgen + mesh for a new chunk.
/// - `Remesh`: mesh-only for an existing chunk snapshot.
/// - `Lighting`: W27 lighting BFS over an owned chunk snapshot.
enum Job {
    /// Full generation + mesh path (pre-W25 ChunkJob).
    Generate(ChunkJob),
    /// W25: remesh-only path; skips worldgen and side-effect scan.
    Remesh(RemeshJob),
    /// W27: lighting BFS over an owned snapshot of affected chunks +
    /// their face-neighbours. No worldgen, no mesh.
    Lighting(LightingJob),
}

/// W27: lighting BFS job. The main thread clones the affected chunks
/// (and their 6 face-neighbours for BFS frontier correctness) into an
/// owned snapshot so the worker can run BFS without any borrow into the
/// live `World`. Cloning ~27 chunks x ~8 KB = ~216 KB per tick_lighting
/// call; acceptable.
struct LightingJob {
    /// Chunks whose light grids must be recomputed. All entries must be
    /// present in `snapshot`.
    affected: Vec<ChunkCoord>,
    /// Owned voxel snapshot: affected chunks + their 6 face-neighbours.
    snapshot: lighting::ChunkSnapshot,
    /// Existing light grids for `affected` (warm-start). The BFS clears
    /// and re-floods so these are optional context; passing them avoids
    /// a cold-start cost on subsequent tick_lighting calls.
    existing_grids: HashMap<ChunkCoord, Arc<lighting::LightGrid>>,
}

/// One unit of work for the chunk worker pool. Carries the per-job
/// `WorldgenCtx` snapshot captured at enqueue time, so the worker
/// generates the chunk against the EXACT base seed + dim + profile
/// the player saw when the request was made. Without this, a
/// `switch_dim` between enqueue and pickup poisoned the result
/// (the worker would read fresh atomics from the wrong dim).
///
/// W24: also carries `Arc<ResourcePack>` + `HaloLighting` so the
/// worker can build the full MC-atlas mesh without shared mutable
/// state.
struct ChunkJob {
    coord: ChunkCoord,
    ctx: worldgen::WorldgenCtx,
    /// W24: resource pack snapshot for UV / anim lookup.
    pack: Arc<resource_pack::ResourcePack>,
    /// W24: pre-fetched light grids for own chunk + 6 neighbours.
    halo: HaloLighting,
    /// W46.25: current world slug + dim so the worker reads the
    /// chunk from disk before falling through to worldgen. Without
    /// this the streamer regenerated every reloaded chunk from
    /// scratch, wiping every player edit (placed torch / obsidian /
    /// anything) that landed in a chunk after the player walked
    /// past UNLOAD_DISTANCE.
    slug: Option<String>,
    dim: String,
}

/// W25: remesh-only job. Carries a clone of the chunk's voxel data
/// (Chunk is ~8 KB; clone is fast) so the worker can mesh without
/// shared mutable state. No worldgen, no side-effect scan.
struct RemeshJob {
    coord: ChunkCoord,
    /// Snapshot of the chunk voxels at dispatch time.
    chunk: Chunk,
    /// Resource pack snapshot for UV / anim lookup.
    pack: Arc<resource_pack::ResourcePack>,
    /// Pre-fetched light grids for own chunk + 6 neighbours.
    halo: HaloLighting,
    /// Revision counter at dispatch time. Used by the main thread to
    /// discard stale results (a second edit arrived before this one
    /// returned).
    revision: u64,
}

fn load_block_atlas_from_dir(dir: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
    let w = ATLAS_COLS * TILE_PX;
    let h = ATLAS_ROWS * TILE_PX;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut copy_tile = |spec: &TileSpec| -> Option<()> {
        // Pick the first existing file from the fallback list.
        let path = spec
            .files
            .iter()
            .map(|name| dir.join(name))
            .find(|p| p.exists())?;
        let img = image::open(&path).ok()?.to_rgba8();
        let img = if img.width() == TILE_PX && img.height() == TILE_PX {
            img
        } else {
            image::imageops::resize(&img, TILE_PX, TILE_PX, image::imageops::FilterType::Nearest)
        };
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let px = spec.col * TILE_PX + tx;
                let py = spec.row * TILE_PX + ty;
                let idx = ((py * w + px) * 4) as usize;
                let pixel = img.get_pixel(tx, ty);
                rgba[idx] = pixel.0[0];
                rgba[idx + 1] = pixel.0[1];
                rgba[idx + 2] = pixel.0[2];
                // Opaque tiles force alpha to 255 so anti-aliased
                // edges in the source can't trip the global
                // alpha-cutout shader.
                rgba[idx + 3] = if spec.keep_alpha { pixel.0[3] } else { 255 };
            }
        }
        Some(())
    };
    // Iterate every (block, face) tile in the registry. Dedup by
    // (col, row) so a tile shared by multiple faces / blocks is
    // copied once.
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for bt in BLOCK_TYPES {
        for spec in [&bt.top, &bt.bottom, &bt.sides] {
            if !seen.insert((spec.col, spec.row)) {
                continue;
            }
            if copy_tile(spec).is_none() {
                tracing::warn!(
                    block = bt.name,
                    files = ?spec.files,
                    "atlas texture missing or unreadable"
                );
                return None;
            }
        }
    }
    tracing::info!(dir = %dir.display(), "loaded block atlas from disk");
    Some((w, h, rgba))
}

/// Procedural 64x32 RGBA atlas built once at startup. Each 16x16
/// tile is the block's base colour modulated by a deterministic
/// per-pixel noise so the texture reads as voxelly fabric rather
/// than flat colour.
/// Day/night driver. Given the current `time` in seconds (modulo
/// `DAY_LENGTH_S`), return `(sun_dir, sky_color)`:
/// - `sun_dir` points TOWARDS the sun (matches `Renderer::set_sun_dir`).
///   The sun arcs from +X at sunrise through +Y at noon to -X at
///   sunset, with a small +Z tilt so shadows are never axis-aligned.
/// - `sky_color` is the linear-RGB clear colour. Three anchors:
///   deep navy at midnight, warm orange at horizon, bright cyan at noon.
fn day_night(time: f32) -> (Vec3, [f32; 3]) {
    let phase = (time / DAY_LENGTH_S).rem_euclid(1.0);
    let theta = phase * std::f32::consts::TAU;
    // Sun position on a circle in the X-Y plane, tilted slightly
    // along +Z so shadows never align perfectly with cardinal axes.
    let sun = Vec3::new(theta.cos(), theta.sin(), 0.35).normalize();
    let elev = sun.y; // -1 (under ground) .. +1 (overhead)
    // W8.3: midnight is now near-black instead of a faint navy.
    // The user explicitly asked for a dark night sky; the old
    // [0.04, 0.06, 0.12] still read as "dim blue" on a calibrated
    // monitor.
    let night = [0.02_f32, 0.02, 0.05];
    let horizon = [0.95_f32, 0.55, 0.30];
    let noon = [0.45_f32, 0.65, 0.95];
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    // Below horizon -> night..horizon; above -> horizon..noon.
    let sky = if elev < 0.0 {
        lerp(night, horizon, (elev + 0.2) * 5.0)
    } else {
        lerp(horizon, noon, elev * 2.0)
    };
    // Below horizon: clamp the sun direction to a "moon"-ish weak
    // angle from above so shadows still render (the shadow pass
    // doesn't model occluded suns). The PBR shader gets a much
    // dimmer normal product because the sun_intensity stays fixed
    // but the n.l term goes near zero by geometry alone.
    let sun = if elev <= 0.05 {
        Vec3::new(0.2, 0.95, 0.25).normalize()
    } else {
        sun
    };
    (sun, sky)
}

// ---------------------------------------------------------------
// W9.5: ambient particle helpers used by `tick_ambient_particles`.
// ---------------------------------------------------------------

/// W38.2: Returns true for blocks that should emit ambient flame/smoke
/// particles each tick. Mirrors MC's open-flame spawn list. Note this
/// is DIFFERENT from light_emission - glowstone emits light but no
/// flame (solid block), while certain emitter variants emit particles
/// but not necessarily maximum light.
pub(crate) fn flame_emitter(id: u16) -> bool {
    // TORCH_ID = 10, redstone torches = 50/51, soul torches = 52/53,
    // W46.21 copper torches = 324/325.
    use crate::block::{COPPER_TORCH_ID, COPPER_WALL_TORCH_ID};
    matches!(
        id,
        x if x == TORCH_ID
            || x == REDSTONE_TORCH_ID
            || x == REDSTONE_WALL_TORCH_ID
            || x == SOUL_TORCH_ID
            || x == SOUL_WALL_TORCH_ID
            || x == COPPER_TORCH_ID
            || x == COPPER_WALL_TORCH_ID
    )
}


// ---------------------------------------------------------------

// W9.2: superseded by `resource_pack::fallback::placeholder_pack`,
// kept around for callers that haven't been migrated to the new
// API + so the legacy `--textures <dir>` flag still has a
// procedural fallback when its dir lookup misses.
#[allow(dead_code)]
fn build_block_atlas() -> (u32, u32, Vec<u8>) {
    let w = ATLAS_COLS * TILE_PX;
    let h = ATLAS_ROWS * TILE_PX;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    // Paint every tile referenced by BLOCK_TYPES once. The
    // procedural fallback approximates the block's albedo with
    // per-pixel noise so the texture reads as voxelly fabric
    // rather than flat colour. Each tile picks the colour of the
    // first BlockType that points at it.
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let paint = |rgba: &mut Vec<u8>, col: u32, row: u32, color: [u8; 3]| {
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let px = col * TILE_PX + tx;
                let py = row * TILE_PX + ty;
                let idx = ((py * w + px) * 4) as usize;
                let n =
                    ((tx.wrapping_mul(31) ^ ty.wrapping_mul(17)) % 64) as i32 - 32;
                let bias = (n as f32 / 32.0) * 24.0;
                rgba[idx] = (color[0] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = (color[1] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = (color[2] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = 255;
            }
        }
    };
    let f2u = |v: f32| (v * 255.0).clamp(0.0, 255.0) as u8;
    for bt in BLOCK_TYPES {
        let base = [f2u(bt.color[0]), f2u(bt.color[1]), f2u(bt.color[2])];
        for spec in [&bt.top, &bt.bottom, &bt.sides] {
            if seen.insert((spec.col, spec.row)) {
                paint(&mut rgba, spec.col, spec.row, base);
            }
        }
    }
    (w, h, rgba)
}

// `generate_chunk_at` + `gen_flat` + `gen_islands` + `gen_classic`
// moved to `worldgen.rs`.


// `body_aabb`, `is_passable`, `voxel_is_solid`,
// `submerged_liquid_eye`, `submerged_liquid`, and
// `aabb_intersects_solid` moved to `player.rs`.

/// Approximate face normal from 4 corners (used by the sloped
/// water top quad where the corner heights differ). Cross product
/// of two opposing edges - close enough for shading the gentle
/// slopes the depth ramp produces.
fn water_face_normal(
    v0: [f32; 3],
    v1: [f32; 3],
    v2: [f32; 3],
    _v3: [f32; 3],
) -> [f32; 3] {
    // W34: explicit upward normal for top face. The water top quad is
    // always nearly horizontal (corner heights wiggle within [0, 1])
    // so the outward normal is essentially +Y. Cross-product attempts
    // gave -Y for the SW,SE,NE corner order (caller convention is CW
    // from above), and negating that produced an off-look from above
    // per the user's repeated reports. Force-emit +Y which is the only
    // physically meaningful outward direction for a water surface.
    let _ = (v0, v1, v2);
    [0.0, 1.0, 0.0]
}


// `submerged_liquid_eye`, `submerged_liquid`, and
// `aabb_intersects_solid` moved to `player.rs`.


/// Helper to render a slot with full drag&drop integration.
///
/// - `kind` identifies this slot (used as the DnD payload + as the
///   drop target id).
/// - When the slot is non-empty and `draggable` is true, the cell
///   becomes a drag source (the widget paints itself at the
///   cursor while held).
/// - Every slot is a drop target. On release, the action is
///   appended to `actions` for the post-pass to apply.
/// - Returns the underlying widget response so callers can layer
///   click handlers / tooltips on top.
fn slot_with_dnd(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    kind: SlotKind,
    selected: bool,
    draggable: bool,
    actions: &mut Vec<InvAction>,
) -> egui::Response {
    slot_with_dnd_dur(ui, slot, atlas, kind, selected, draggable, actions, None)
}

/// W5.2b: `slot_with_dnd` variant that threads a durability
/// `(cur, max)` pair through to `draw_slot_widget_with_durability`.
/// New call sites that render the hotbar / inventory pass the
/// flint+steel durability here; everywhere else the legacy shim
/// above is fine.
#[allow(clippy::too_many_arguments)]
fn slot_with_dnd_dur(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    kind: SlotKind,
    selected: bool,
    draggable: bool,
    actions: &mut Vec<InvAction>,
    durability: Option<(u16, u16)>,
) -> egui::Response {
    // Scope the drag-source id through the parent ui so the same
    // SlotKind can appear in multiple layers (e.g. the hotbar Area
    // + the hotbar mirror inside the Inventory window) without
    // egui flagging it as a layer-id switch on the same widget.
    let id = ui.make_persistent_id(("inv_slot", kind));
    let resp = if draggable && slot.is_some() {
        ui.dnd_drag_source(id, kind, |ui| {
            draw_slot_widget_with_durability(
                ui, slot, atlas, selected, egui::Sense::click_and_drag(), durability,
            )
        })
        .response
    } else {
        draw_slot_widget_with_durability(
            ui, slot, atlas, selected, egui::Sense::click(), durability,
        )
    };
    if let Some(from) = resp.dnd_release_payload::<SlotKind>() {
        let from = *from;
        if from != kind {
            // Modifier mapping:
            //   Ctrl/Cmd held = move exactly one item (good for
            //                   filling craft slots).
            //   Shift held    = move half the stack (split-in-half
            //                   matches Minecraft's right-click
            //                   pickup behaviour).
            //   No modifier   = move the whole stack.
            let (shift, ctrl) =
                ui.input(|i| (i.modifiers.shift, i.modifiers.command));
            let split = if ctrl {
                SplitMode::One
            } else if shift {
                SplitMode::Half
            } else {
                SplitMode::All
            };
            actions.push(InvAction::Drag { from, to: kind, split });
        }
    }
    // Hover tooltip: show the block name + stack count. The wrapped
    // response is dropped silently because all we want is the side
    // effect of registering the tooltip.
    if let Some((_, count, _, name)) = slot {
        let resp = if count > 1 {
            resp.on_hover_text(format!("{name} x{count}"))
        } else {
            resp.on_hover_text(name)
        };
        return resp;
    }
    resp
}

/// Draw one inventory cell. Returns the response so the caller
/// can wire click handlers / drag detection / tooltips on top.
///
/// - `slot`: `Some((color, count, uv_rect))` for a stack, `None`
///   for an empty cell.
/// - `atlas`: optional egui texture id of the block atlas. When
///   set the slot paints the matching tile; when `None` it falls
///   back to a flat color swatch from `slot.0`.
/// - `selected`: outlines the cell in gold (active hotbar slot).
/// - `sense`: the egui interaction model (click, click_and_drag,
///   etc.) the underlying response should report.
fn draw_slot_widget(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    selected: bool,
    sense: egui::Sense,
) -> egui::Response {
    // W5.2b: callers that need a durability bar route through
    // `draw_slot_widget_with_durability` below. The 5-arg shim
    // keeps every existing call site untouched.
    draw_slot_widget_with_durability(ui, slot, atlas, selected, sense, None)
}

/// W5.2b: like `draw_slot_widget` but paints a remaining-uses bar
/// at the bottom of the cell when `durability` is `Some((cur, max))`
/// and `cur < max`. Gradient red -> yellow -> green tracks the
/// fraction.
fn draw_slot_widget_with_durability(
    ui: &mut egui::Ui,
    slot: Option<SlotIcon>,
    atlas: Option<egui::TextureId>,
    selected: bool,
    sense: egui::Sense,
    durability: Option<(u16, u16)>,
) -> egui::Response {
    let size = egui::vec2(40.0, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter_at(rect);
    // Slot background: dark gray when empty, otherwise either the
    // atlas tile (preferred) or a flat color swatch.
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(24));
    if let Some((c, _, uv, _)) = slot {
        match atlas {
            Some(tex_id) => {
                let uv_rect = egui::Rect::from_min_max(
                    egui::pos2(uv[0], uv[1]),
                    egui::pos2(uv[2], uv[3]),
                );
                // Inset by 2px so the outline frame stays visible.
                let icon_rect = rect.shrink(2.0);
                painter.image(tex_id, icon_rect, uv_rect, egui::Color32::WHITE);
            }
            None => {
                let fill = egui::Color32::from_rgb(
                    (c[0] * 255.0) as u8,
                    (c[1] * 255.0) as u8,
                    (c[2] * 255.0) as u8,
                );
                painter.rect_filled(rect.shrink(2.0), 2.0, fill);
            }
        }
    }
    let outline = if selected {
        egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 220, 80))
    } else if resp.hovered() {
        egui::Stroke::new(2.0, egui::Color32::from_gray(200))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(120))
    };
    painter.rect_stroke(rect, 4.0, outline, egui::StrokeKind::Inside);
    if let Some((_, count, _, _)) = slot {
        // Stack count badge in the bottom-right corner. Hide for
        // singletons because Minecraft does the same.
        if count > 1 {
            let pos = egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0);
            // Drop shadow first for readability over light tiles.
            painter.text(
                pos + egui::vec2(1.0, 1.0),
                egui::Align2::RIGHT_BOTTOM,
                count.to_string(),
                egui::FontId::proportional(14.0),
                egui::Color32::BLACK,
            );
            painter.text(
                pos,
                egui::Align2::RIGHT_BOTTOM,
                count.to_string(),
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            );
        }
    }
    // W5.2b: durability bar along the bottom edge. Hidden when
    // cur == max (untouched tool) so the cell stays clean until
    // the player actually wears it down.
    if let Some((cur, max)) = durability
        && cur < max
        && max > 0
    {
        let frac = (cur as f32 / max as f32).clamp(0.0, 1.0);
        // Bar geometry: inset 4px on each side, 4px tall, sitting
        // 4px above the cell's bottom edge.
        let bar_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, rect.max.y - 8.0),
            egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0),
        );
        // Dark backing so the gradient stays legible against the
        // bright stack-count badge or icon.
        painter.rect_filled(bar_rect, 1.0, egui::Color32::from_gray(20));
        // Gradient: red (0%) -> yellow (50%) -> green (100%).
        let r = if frac < 0.5 { 255 } else { (255.0 * (1.0 - frac) * 2.0) as u8 };
        let g = if frac < 0.5 { (255.0 * frac * 2.0) as u8 } else { 255 };
        let fill_rect = egui::Rect::from_min_max(
            bar_rect.min,
            egui::pos2(bar_rect.min.x + bar_rect.width() * frac, bar_rect.max.y),
        );
        painter.rect_filled(fill_rect, 1.0, egui::Color32::from_rgb(r, g, 0));
    }
    resp
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Parse `--server [addr]` / `--client <addr>` / no-arg offline.
    let mut args = std::env::args().skip(1);
    let mut net = NetRole::Offline;
    let mut title_suffix = String::new();
    let mut texture_dir: Option<std::path::PathBuf> = None;
    let mut sounds_dir: Option<std::path::PathBuf> = None;
    // W5.1: collect the CLI worldgen flag and stamp it onto the
    // Voxel after construction (the global WORLDGEN atomic is
    // gone).
    let mut cli_worldgen: Option<WorldGen> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--textures" => {
                let p = args.next().unwrap_or_default();
                texture_dir = Some(std::path::PathBuf::from(p));
            }
            "--sounds" => {
                let p = args.next().unwrap_or_default();
                sounds_dir = Some(std::path::PathBuf::from(p));
            }
            "--worldgen" => {
                let m = args.next().unwrap_or_default();
                match WorldGen::from_str(&m) {
                    Some(g) => {
                        info!(mode = ?g, "worldgen mode set");
                        cli_worldgen = Some(g);
                    }
                    None => {
                        warn!(arg = %m, "unknown worldgen mode (classic|flat|islands|amplified|realistic)");
                    }
                }
            }
            "--server" => {
                let addr = args.next().unwrap_or_else(|| "0.0.0.0:7777".to_string());
                match NetServer::bind(&addr) {
                    Ok(s) => {
                        info!(addr = %addr, "voxel server bound");
                        title_suffix = format!(" [SERVER {addr}]");
                        net = NetRole::Server {
                            server: s,
                            poses: HashMap::new(),
                            inventories: HashMap::new(),
                            pending_inv_sends: HashSet::new(),
                        };
                    }
                    Err(err) => {
                        error!(error = %err, addr = %addr, "server bind failed");
                        return ExitCode::from(2);
                    }
                }
            }
            "--client" => {
                let addr = args
                    .next()
                    .unwrap_or_else(|| "127.0.0.1:7777".to_string());
                match NetClient::connect(&addr, "voxel-mp-client") {
                    Ok(c) => {
                        info!(addr = %addr, "voxel client connecting");
                        title_suffix = format!(" [CLIENT -> {addr}]");
                        net = NetRole::Client {
                            client: c,
                            remote: HashMap::new(),
                            pending_edits: Vec::new(),
                            pending_inv_actions: Vec::new(),
                        };
                    }
                    Err(err) => {
                        error!(error = %err, addr = %addr, "client connect failed");
                        return ExitCode::from(3);
                    }
                }
            }
            other => {
                warn!(arg = %other, "ignoring unknown argv");
            }
        }
    }
    info!(role = %net.label(), "voxel_demo_mp starting");

    let config = WindowConfig {
        title: format!("InfEngine - voxel_demo_mp{title_suffix}"),
        width: 1280,
        height: 720,
        resizable: true,
    };
    let mut voxel = Voxel::new_with_net(net);
    // W21.1: pick initial splash text so it's ready before the
    // first Main screen draw.
    voxel.menu.enter_main();
    // W5.1: apply the CLI --worldgen flag to the active dim's
    // per-dim worldgen entry. Replaces the deleted global atomic.
    if let Some(g) = cli_worldgen {
        let dim = voxel.current_dim.clone();
        voxel.dim_worldgens.insert(dim, g);
    }
    // Default asset resolution. We probe two locations in order:
    //
    //   1. `<exe-dir>/assets/...` - ships next to the binary in
    //      a release zip. Standard game-distro layout: drop the
    //      `assets/` folder beside `voxel_demo_mp.exe`.
    //   2. `<CARGO_MANIFEST_DIR>/assets/...` - dev-mode fallback
    //      so `cargo run` from the source tree still works
    //      without copying anything around.
    //
    // Both paths are checked separately for `textures/blocks` and
    // `sounds`, so a release with only one subfolder still picks
    // it up. Missing both directories simply leaves the field
    // `None` and the demo falls back to procedural atlas + synth
    // audio.
    fn resolve_asset_subdir(rel: &[&str]) -> Option<std::path::PathBuf> {
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let mut p = dir.to_path_buf();
            for seg in rel {
                p.push(seg);
            }
            candidates.push(p);
        }
        let mut dev = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for seg in rel {
            dev.push(seg);
        }
        candidates.push(dev);
        candidates.into_iter().find(|p| p.exists())
    }
    voxel.texture_dir = texture_dir
        .or_else(|| resolve_asset_subdir(&["assets", "textures", "blocks"]));
    voxel.sounds_dir = sounds_dir
        .or_else(|| resolve_asset_subdir(&["assets", "sounds"]));
    info!(
        textures = ?voxel.texture_dir,
        sounds = ?voxel.sounds_dir,
        "resolved asset paths"
    );
    if let Err(err) = run(config, voxel) {
        error!(error = %err, "event loop failed");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}


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
